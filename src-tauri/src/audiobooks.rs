//! App-owned audiobook library. Metadata is the commit record: only its PCM prefix
//! is playable. Startup rolls back an interrupted append before exposing a book.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
const RATE: u64 = 24_000;
// Leave room above the hosted continuation threshold for arriving audio.
const STREAM_BUFFER_SECONDS: f64 = 12.0;
fn identity() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn default_audio() -> String {
    "audio.wav".into()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    page: usize,
    start: usize,
    end: usize,
    text: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sentence {
    #[serde(default = "identity")]
    id: String,
    #[serde(default)]
    continuation_of: Option<String>,
    text: String,
    sources: Vec<Source>,
    start_sample: u64,
    end_sample: u64,
    /// Provider and voice used to generate this sentence's audio, e.g.
    /// `kokoro:af_heart`. Empty for sentences created before voice tracking.
    #[serde(default)]
    voice_key: String,
    /// Human-readable voice name captured when this audio was generated.
    #[serde(default)]
    voice_label: String,
}

/// Display identity. Cache identity also includes model and synthesis controls.
fn voice_key(settings: &crate::config::Settings) -> String {
    use crate::config::Engine;
    match settings.engine {
        Engine::Kokoro => format!("kokoro:{}", settings.kokoro.voice),
        Engine::Apple => format!("apple:{}", settings.voice.as_deref().unwrap_or("default")),
        Engine::Chatterbox => format!(
            "chatterbox:{}:{}",
            settings.chatterbox.voice,
            settings
                .chatterbox
                .ref_audio
                .as_deref()
                .unwrap_or("default")
        ),
        Engine::Fish => format!("fish:{}", settings.fish.voice_id),
    }
}
fn voice_label(settings: &crate::config::Settings) -> String {
    use crate::config::Engine;
    match settings.engine {
        Engine::Fish if !settings.fish.voice_name.trim().is_empty() => {
            settings.fish.voice_name.clone()
        }
        Engine::Fish => settings.fish.voice_id.clone(),
        Engine::Kokoro => settings.kokoro.voice.clone(),
        Engine::Apple => settings.voice.clone().unwrap_or_else(|| "default".into()),
        Engine::Chatterbox => settings.chatterbox.voice.clone(),
    }
}
fn synthesis_key(settings: &crate::config::Settings) -> String {
    use crate::config::Engine;
    match settings.engine {
        Engine::Kokoro => json!([
            "kokoro",
            settings.kokoro.voice,
            settings.kokoro.quant,
            settings.kokoro.speed
        ]),
        Engine::Apple => json!(["apple", settings.voice, settings.rate]),
        Engine::Chatterbox => json!([
            "chatterbox",
            settings.chatterbox.voice,
            settings.chatterbox.ref_audio,
            settings.chatterbox.quant,
            settings.chatterbox.exaggeration,
            settings.chatterbox.cfg_weight
        ]),
        Engine::Fish => json!([
            "fish",
            settings.fish.voice_id,
            settings.fish.model_id,
            settings.fish.enhance_text
        ]),
    }
    .to_string()
}
fn recording_key(book: &Book) -> String {
    if !book.synthesis_key.is_empty() {
        return book.synthesis_key.clone();
    }
    // Before provider selection was supported, all audiobook audio was Kokoro.
    let legacy_voice = format!("kokoro:{}", book.voice.voice);
    if book
        .sentences
        .iter()
        .take(book.generated)
        .any(|s| !s.voice_key.is_empty() && s.voice_key != legacy_voice)
    {
        return format!("unknown:{}", book.range_id);
    }
    synthesis_key(&crate::config::Settings {
        engine: crate::config::Engine::Kokoro,
        kokoro: book.voice.clone(),
        ..Default::default()
    })
}
fn sentence_voice_key(book: &Book, sentence: &Sentence) -> String {
    if sentence.voice_key.is_empty() {
        format!("kokoro:{}", book.voice.voice)
    } else {
        sentence.voice_key.clone()
    }
}
fn range_has_voice(book: &Book, voice: &str) -> bool {
    book.sentences
        .iter()
        .take(book.generated)
        .any(|sentence| sentence_voice_key(book, sentence) == voice)
}
fn voice_at_position(book: &Book, seconds: f64) -> Option<String> {
    let sample = (seconds.max(0.) * RATE as f64) as u64;
    book.sentences
        .iter()
        .take(book.generated)
        .find(|sentence| sentence.end_sample > sample)
        .or_else(|| book.sentences.iter().take(book.generated).last())
        .map(|sentence| sentence_voice_key(book, sentence))
}
fn reset_recording(book: &mut Book, settings: &crate::config::Settings) {
    book.range_id = identity();
    book.audio_file = format!("{}.wav", identity());
    book.pages.clear();
    book.sentences.clear();
    book.checkpoints.clear();
    book.pending.clear();
    book.generated = 0;
    book.committed_samples = 0;
    book.position = 0.;
    book.pending_start = None;
    book.reader_hint = None;
    book.recovery = None;
    book.bookmark_sentence = None;
    book.bookmark_offset = 0;
    book.complete = false;
    book.continuous = false;
    book.end_verified = false;
    book.from_beginning = false;
    book.synthesis_key = synthesis_key(settings);
    book.voice = settings.kokoro.clone();
}
fn select_generation_voice(
    dir: &Path,
    book: &mut Book,
    settings: &crate::config::Settings,
) -> Result<(), String> {
    let mut next = book.clone();
    if recording_key(book) != synthesis_key(settings) && book.generated > 0 {
        let mut previous = book.clone();
        previous.saved_ranges.clear();
        previous.full_scan = None;
        next.saved_ranges.push(Box::new(previous));
        reset_recording(&mut next, settings);
        recover(dir, &next)?;
    } else {
        next.synthesis_key = synthesis_key(settings);
        next.voice = settings.kokoro.clone();
    }
    save(dir, &next)?;
    *book = next;
    Ok(())
}
fn render_sentence(
    app: &AppHandle,
    settings: &crate::config::Settings,
    text: &str,
    scratch: &Path,
    active: impl Fn() -> bool,
) -> Result<Vec<f32>, String> {
    let state = app.state::<crate::AppState>();
    let _guard = state.synthesis.for_engine(settings.engine).lock().unwrap();
    if !active() {
        return Err("Preparation stopped".into());
    }
    if let Some(reason) = crate::selected_engine_refusal(settings) {
        return Err(reason);
    }
    if settings.engine == crate::config::Engine::Fish {
        return tauri::async_runtime::block_on(crate::hosted::render_speech(
            text,
            &settings.fish,
            active,
        ));
    }
    let result = state
        .spoken
        .render(settings, text, scratch)
        .and_then(|_| crate::spoken::Spoken::load_audio(scratch));
    let _ = fs::remove_file(scratch);
    result
}
fn start_page_stream_playback(
    app: &AppHandle,
    service: &Audiobooks,
    id: &str,
    token: &str,
    samples: Vec<f32>,
) -> Result<(), String> {
    let mut l = service.0.lock().unwrap();
    if !l
        .producing
        .as_ref()
        .is_some_and(|(i, t, _)| i == id && t == token)
    {
        return Err("Preparation stopped".into());
    }
    let b = l.books.get(id).ok_or("Audiobook removed")?;
    let position = selected_position(b).unwrap_or(b.committed_samples as f64 / RATE as f64);
    let range_id = b.range_id.clone();
    let player = Arc::new(crate::pcm::Player::new()?);
    player.update_file(&audio_path(&directory(id)?, b)?, b.committed_samples, false)?;
    player.push(samples, false);
    player.set_speed(b.speed)?;
    let job = crate::begin_speech(app, crate::Phase::Preparing);
    let state = app.state::<crate::AppState>();
    let mut active = state.job.lock().unwrap();
    if !active.is_current(job) {
        return Err("Playback superseded".into());
    }
    state.spoken.activate_pcm(player.clone());
    player.seek(position)?;
    active.set(crate::Phase::Speaking, None, None);
    let _ = app.emit("kiegen:status", &active.status);
    drop(active);
    l.playing = Some(Playing {
        saved_chapter: false,
        range_id,
        book: id.into(),
        job,
        player,
    });
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
struct Page {
    #[serde(default)]
    heading: Option<String>,
    progress: String,
    fingerprint: String,
    text: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Checkpoint {
    #[serde(default = "identity")]
    id: String,
    #[serde(default)]
    verified_start: bool,
    label: String,
    sentence: usize,
    page: usize,
}
#[derive(Clone, Serialize, Deserialize)]
struct ReaderHint {
    progress: String,
    fingerprint: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Book {
    #[serde(default)]
    schema_version: u32,
    #[serde(default = "identity")]
    range_id: String,
    #[serde(default)]
    saved_ranges: Vec<Box<Book>>,
    #[serde(default)]
    pending_start: Option<String>,
    #[serde(default)]
    reader_hint: Option<ReaderHint>,
    #[serde(default)]
    recovery: Option<Value>,
    #[serde(default)]
    full_scan: Option<Box<full_scan::FullScan>>,
    #[serde(default)]
    continuous: bool,
    #[serde(default = "default_audio")]
    audio_file: String,
    #[serde(default)]
    end_verified: bool,
    #[serde(default)]
    activity: Option<String>,
    #[serde(default)]
    bookmark_sentence: Option<String>,
    #[serde(default)]
    bookmark_offset: u64,
    id: String,
    key: String,
    title: String,
    url: String,
    book_id: String,
    pages: Vec<Page>,
    sentences: Vec<Sentence>,
    checkpoints: Vec<Checkpoint>,
    pending: Vec<Source>,
    committed_samples: u64,
    generated: usize,
    position: f64,
    speed: f32,
    complete: bool,
    from_beginning: bool,
    message: Option<String>,
    #[serde(default)]
    voice: crate::config::KokoroSettings,
    #[serde(default)]
    synthesis_key: String,
}
impl Book {
    fn ranges(&self) -> impl Iterator<Item = &Book> {
        self.full_scan
            .as_ref()
            .map(|scan| scan.draft())
            .into_iter()
            .chain(std::iter::once(self))
            .chain(self.saved_ranges.iter().map(|b| b.as_ref()))
    }
    fn range_mut(&mut self, id: &str) -> Option<&mut Book> {
        if self.range_id == id {
            return Some(self);
        }
        if let Some(scan) = self.full_scan.as_mut() {
            if scan.draft().range_id == id {
                return Some(scan.draft_mut());
            }
        }
        self.saved_ranges
            .iter_mut()
            .find(|b| b.range_id == id)
            .map(|b| b.as_mut())
    }
    fn visible_ranges(&self) -> Vec<&Book> {
        if let Some(scan) = self.full_scan.as_ref().filter(|s| s.transcript_ready()) {
            // The verified full transcript supplies the order and coverage of this audiobook.
            vec![scan.draft()]
        } else {
            std::iter::once(self)
                .chain(self.saved_ranges.iter().map(|b| b.as_ref()))
                .collect()
        }
    }
    fn seconds(&self) -> f64 {
        self.committed_samples as f64 / RATE as f64
    }
    fn view(&self) -> Value {
        self.view_for_voice(None)
    }
    fn view_for_voice(&self, voice: Option<&str>) -> Value {
        let at = (self.position * RATE as f64) as u64;
        let current = self
            .sentences
            .iter()
            .take(self.generated)
            .find(|s| s.end_sample > at);
        let ranges = self
            .visible_ranges()
            .into_iter()
            .filter(|range| voice.is_none_or(|voice| range_has_voice(range, voice)))
            .collect::<Vec<_>>();
        let mut checkpoints = ranges
            .iter()
            .flat_map(|b| {
                b.checkpoints
                    .iter()
                    .enumerate()
                    .map(move |(i, c)| chapter_view(b, i, c))
                    .filter(|chapter| voice.is_none_or(|voice| chapter["voiceKey"] == voice))
            })
            .collect::<Vec<_>>();
        // A single recording is already in transcript order, including opening locations.
        if ranges.len() > 1 {
            checkpoints.sort_by_key(|c| {
                ReaderPosition::parse(c["page"].as_str().unwrap_or(""))
                    .map(|p| match p {
                        ReaderPosition::FrontMatter(n) => n,
                        ReaderPosition::Page(n) => n.saturating_add(4000),
                        ReaderPosition::Location(n) => n,
                    })
                    .unwrap_or(u64::MAX)
            });
        }
        let ready_seconds = checkpoints
            .iter()
            .filter(|c| c["complete"] == true)
            .map(|c| c["duration"].as_f64().unwrap_or(0.))
            .sum::<f64>();
        json!({"fullPreparation":self.full_scan.as_ref().map(|s|s.view()),"id":self.id,"key":self.key,"title":self.title,"platform":"kindle","pages":self.pages.len(),"generated":self.generated,"seconds":self.seconds(),"totalPages":ranges.iter().map(|b|b.pages.len()).sum::<usize>(),"totalGenerated":ranges.iter().map(|b|b.generated).sum::<usize>(),"totalSeconds":ready_seconds,"generatedSeconds":ranges.iter().map(|b|b.seconds()).sum::<f64>(),"position":self.position,"speed":self.speed,"complete":self.complete && coverage_intact(self),"fromBeginning":self.from_beginning,"message":self.message,"activity":self.activity,
            "recovery": self.recovery, "resume": voice.map_or_else(|| resume(self), |voice| resume_for_voice(self, voice)), "pendingPage": self.pending_start.as_ref().and_then(|f| self.pages.iter().find(|p| &p.fingerprint == f)).map(|p| &p.progress),
            "checkpoints": checkpoints,
            "passage":current.map(|s|json!({"text":s.text,"page":s.sources.first().and_then(|p|self.pages.get(p.page)).map(|p|&p.progress)}))})
    }
}
const FISH_BATCH_MIN: usize = 5;
const FISH_BATCH_MAX: usize = 6;

/// Select a Fish Audio request size without crossing a chapter boundary. Short
/// final groups are allowed at chapter and book ends; otherwise wait for more
/// captured text so the hosted model gets sentence context.
fn next_fish_batch_len(book: &Book, allow_short: bool) -> Option<usize> {
    let start = book.generated;
    if start >= book.sentences.len() {
        return None;
    }
    let next_chapter = book
        .checkpoints
        .iter()
        .map(|c| c.sentence)
        .find(|&n| n > start);
    let end = next_chapter
        .unwrap_or(book.sentences.len())
        .min(book.sentences.len());
    let available = end.saturating_sub(start);
    if available == 0 {
        return None;
    }
    if available >= FISH_BATCH_MIN {
        Some(available.min(FISH_BATCH_MAX))
    } else if next_chapter.is_some() || book.end_verified || allow_short {
        Some(available)
    } else {
        None
    }
}

/// Turn several transcript sentences into one audio unit. Source spans remain
/// attached to the combined text, and checkpoint indices move with the splice.
fn coalesce_fish_batch(book: &mut Book, count: usize) {
    if count <= 1 || book.generated + count > book.sentences.len() {
        return;
    }
    let start = book.generated;
    let end = start + count;
    let batch_id = book.sentences[start].id.clone();
    let text = book.sentences[start..end]
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let sources = book.sentences[start..end]
        .iter()
        .flat_map(|sentence| sentence.sources.clone())
        .collect::<Vec<_>>();
    let removed_ids = book.sentences[start + 1..end]
        .iter()
        .map(|sentence| sentence.id.clone())
        .collect::<std::collections::HashSet<_>>();
    book.sentences[start].text = text;
    book.sentences[start].sources = sources;
    book.sentences.drain(start + 1..end);
    for checkpoint in &mut book.checkpoints {
        if checkpoint.sentence >= end {
            checkpoint.sentence -= count - 1;
        }
    }
    if book
        .bookmark_sentence
        .as_ref()
        .is_some_and(|id| removed_ids.contains(id))
    {
        book.bookmark_sentence = Some(batch_id);
        book.bookmark_offset = 0;
    }
}

// Session audio can play immediately. Only a closed, fully rendered chapter is resumable.
fn saved_playback(book: &Book, position: Option<f64>) -> Option<(f64, u64, usize)> {
    saved_playback_for_voice(book, position, None)
}
fn saved_playback_for_voice(
    book: &Book,
    position: Option<f64>,
    voice: Option<&str>,
) -> Option<(f64, u64, usize)> {
    let chapters = book
        .checkpoints
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            let chapter = chapter_view(book, *i, c);
            chapter["complete"] == true && voice.is_none_or(|voice| chapter["voiceKey"] == voice)
        })
        .collect::<Vec<_>>();
    let bounds = |i: usize, c: &Checkpoint| {
        let start = book.sentences.get(c.sentence)?.start_sample;
        let end_index = book
            .checkpoints
            .get(i + 1)
            .map_or(book.generated, |c| c.sentence);
        let end = book.sentences.get(end_index.checked_sub(1)?)?.end_sample;
        (end > start).then_some((start, end))
    };
    // Playlist: a selected chapter keeps playing through the complete chapters
    // that follow it, stopping after the last consecutive complete chapter.
    let playlist_end = |k: usize, i: usize, end: u64| {
        let mut playlist_end = end;
        let mut next = i + 1;
        for &(j, c) in &chapters[k + 1..] {
            if j != next {
                break;
            }
            if let Some((_, chapter_end)) = bounds(j, c) {
                playlist_end = chapter_end;
                next += 1;
            } else {
                break;
            }
        }
        playlist_end
    };
    let wanted = position.unwrap_or(book.position);
    if wanted.is_finite() && wanted >= 0. {
        let sample = (wanted * RATE as f64).round() as u64;
        for (k, &(i, c)) in chapters.iter().enumerate() {
            if let Some((start, end)) = bounds(i, c) {
                if sample >= start && sample < end {
                    return Some((wanted, playlist_end(k, i, end), i));
                }
            }
        }
    }
    if position.is_some() {
        return None;
    }
    let &(i, c) = chapters.first()?;
    let (start, end) = bounds(i, c)?;
    Some((start as f64 / RATE as f64, playlist_end(0, i, end), i))
}
fn resume_at(range: &Book, position: Option<f64>) -> Option<Value> {
    resume_at_voice(range, position, None)
}
fn resume_at_voice(range: &Book, position: Option<f64>, voice: Option<&str>) -> Option<Value> {
    let (seconds, _, index) = saved_playback_for_voice(range, position, voice)?;
    let sample = (seconds * RATE as f64).round() as u64;
    let sentence = range
        .sentences
        .iter()
        .take(range.generated)
        .find(|s| s.end_sample > sample)?;
    let page = range.pages.get(sentence.sources.first()?.page)?;
    Some(
        json!({"page":page.progress,"chapter":chapter_label(range,&range.checkpoints[index]),
        "seconds":seconds,"rangeId":range.range_id}),
    )
}
fn resume(book: &Book) -> Option<Value> {
    book.ranges().find_map(|range| resume_at(range, None))
}
fn resume_for_voice(book: &Book, voice: &str) -> Option<Value> {
    book.ranges()
        .filter(|range| range_has_voice(range, voice))
        .find_map(|range| resume_at_voice(range, None, Some(voice)))
}
fn different_page(book: &Book, reader: &Value) -> bool {
    let sample = (book.position * RATE as f64) as u64;
    let saved = book
        .sentences
        .iter()
        .take(book.generated)
        .find(|s| s.end_sample > sample)
        .or_else(|| {
            book.generated
                .checked_sub(1)
                .and_then(|i| book.sentences.get(i))
        })
        .and_then(|s| s.sources.first())
        .and_then(|s| book.pages.get(s.page))
        .map(|p| p.progress.clone());
    match (
        saved.as_deref().and_then(page_number),
        reader["progress"].as_str().and_then(page_number),
    ) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}
fn listening_buffer(book: &Book) -> f64 {
    // A selected unfinished sentence may require the next page even when older audio is cached.
    if book.pending_start.is_some() {
        0.
    } else {
        (book.seconds() - book.position).max(0.)
    }
}
fn selected_position(book: &Book) -> Option<f64> {
    let fingerprint = book.pending_start.as_ref()?;
    let page = book
        .pages
        .iter()
        .position(|p| &p.fingerprint == fingerprint)?;
    book.sentences
        .iter()
        .take(book.generated)
        .find(|s| s.sources.iter().any(|p| p.page == page))
        .map(|s| s.start_sample as f64 / RATE as f64)
}
fn activate_range(book: &mut Book, range: &str) -> Result<(), String> {
    if book.range_id == range {
        return Ok(());
    }
    let index = book
        .saved_ranges
        .iter()
        .position(|b| b.range_id == range)
        .ok_or("Saved passage not found")?;
    let mut ranges = std::mem::take(&mut book.saved_ranges);
    let mut selected = *ranges.remove(index);
    let mut previous = book.clone();
    previous.activity = None;
    previous.saved_ranges.clear();
    ranges.push(Box::new(previous));
    selected.saved_ranges = ranges;
    *book = selected;
    Ok(())
}
#[cfg(test)]
fn select_page(
    dir: &Path,
    book: &mut Book,
    reader: &Value,
    text: &str,
) -> Result<Option<f64>, String> {
    select_page_matching(dir, book, reader, text, None)
}
fn select_listening_page(
    dir: &Path,
    book: &mut Book,
    reader: &Value,
    text: &str,
    settings: &crate::config::Settings,
) -> Result<Option<f64>, String> {
    select_generation_voice(dir, book, settings)?;
    select_page_matching(dir, book, reader, text, Some(&synthesis_key(settings)))
}
fn select_page_matching(
    dir: &Path,
    book: &mut Book,
    reader: &Value,
    text: &str,
    requested_recording: Option<&str>,
) -> Result<Option<f64>, String> {
    if key(reader)? != book.key {
        return Err("Connected book changed".into());
    }
    let fingerprint = reader["fingerprint"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Missing page identity")?;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || normalized.chars().count() > 20_000 {
        return Err("No supported passage on this page".into());
    }
    let number = reader["progress"].as_str().and_then(page_number);
    let found = std::iter::once(&*book)
        .chain(book.saved_ranges.iter().map(|b| b.as_ref()))
        .filter(|b| requested_recording.is_none_or(|key| recording_key(b) == key))
        .find_map(|b| {
            b.pages
                .iter()
                .position(|p| {
                    p.fingerprint == fingerprint
                        || number.is_some() && page_number(&p.progress) == number
                })
                .map(|i| (b.range_id.clone(), i, b.pages[i].text.clone()))
        });
    let mut next = book.clone();
    if let Some((range, index, _)) = found.filter(|(_, _, saved)| saved == &normalized) {
        activate_range(&mut next, &range)?;
        next.pending_start = Some(next.pages[index].fingerprint.clone());
    } else {
        let mut ranges = std::mem::take(&mut next.saved_ranges);
        next.activity = None;
        ranges.push(Box::new(next.clone()));
        next.saved_ranges = ranges;
        next.range_id = identity();
        next.audio_file = format!("{}.wav", identity());
        next.full_scan = None;
        next.continuous = false;
        next.reader_hint = None;
        next.recovery = None;
        next.pages.clear();
        next.sentences.clear();
        next.checkpoints.clear();
        next.pending.clear();
        next.generated = 0;
        next.committed_samples = 0;
        next.position = 0.;
        next.bookmark_sentence = None;
        next.bookmark_offset = 0;
        next.complete = false;
        next.end_verified = false;
        next.from_beginning = reader["startOfBook"] == true;
        capture(&mut next, reader, text)?;
        next.pending_start = Some(fingerprint.into());
    }
    let position = selected_position(&next);
    if let Some(position) = position {
        next.position = position;
        next.pending_start = None;
        remember_position(&mut next);
    }
    next.message = None;
    next.activity = if position.is_none() {
        Some("selected".into())
    } else {
        None
    };
    recover(dir, &next)?;
    save(dir, &next)?;
    *book = next;
    save_bookmark(dir, book)?;
    Ok(position)
}
struct Playing {
    saved_chapter: bool,
    range_id: String,
    book: String,
    job: u64,
    player: Arc<crate::pcm::Player>,
}
#[derive(Default)]
struct Library {
    worker: bool,
    transitioning: bool,
    selection_revision: u64,
    books: BTreeMap<String, Book>,
    producing: Option<(String, String, bool)>,
    playing: Option<Playing>,
    dismissed: Option<String>,
}
#[derive(Clone, Default)]
pub struct Audiobooks(Arc<Mutex<Library>>);
fn root() -> Result<PathBuf, String> {
    let path = crate::engine_paths::app_support_dir()
        .ok_or("Cannot locate app support")?
        .join("audiobooks");
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn directory(id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid audiobook identity")?;
    let path = root()?.join(id);
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn save(dir: &Path, book: &Book) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let temp = dir.join("book.tmp");
    let mut file = opts.open(&temp).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(book).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(temp, dir.join("book.json")).map_err(|e| e.to_string())?;
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn audio_path(dir: &Path, book: &Book) -> Result<PathBuf, String> {
    if book.audio_file != "audio.wav"
        && !book
            .audio_file
            .strip_suffix(".wav")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
    {
        return Err("Invalid recording identity".into());
    }
    Ok(dir.join(&book.audio_file))
}
fn bookmark_value(book: &Book) -> Value {
    let at = (book.position * RATE as f64).round() as u64;
    let sentence = book
        .sentences
        .iter()
        .take(book.generated)
        .find(|s| s.end_sample > at)
        .or_else(|| book.sentences.get(book.generated.checked_sub(1)?));
    json!({"rangeId":book.range_id,"position":book.position,"speed":book.speed,"sentenceId":sentence.map(|s|&s.id),"sampleOffset":sentence.map(|s|at.saturating_sub(s.start_sample).min(s.end_sample-s.start_sample))})
}
fn remember_position(book: &mut Book) {
    let value = bookmark_value(book);
    book.bookmark_sentence = value["sentenceId"].as_str().map(str::to_owned);
    book.bookmark_offset = value["sampleOffset"].as_u64().unwrap_or(0);
}
fn stable_position(book: &Book) -> Option<f64> {
    let id = book.bookmark_sentence.as_ref()?;
    let s = book
        .sentences
        .iter()
        .take(book.generated)
        .find(|s| &s.id == id)?;
    Some(
        (s.start_sample + book.bookmark_offset.min(s.end_sample - s.start_sample)) as f64
            / RATE as f64,
    )
}
// Labels can prove that a numbered chapter is missing; they do not prove page coverage.
fn chapter_number(label: &str) -> Option<u32> {
    let lower = label.to_lowercase();
    let suffix = lower.strip_prefix("chapter ")?.trim();
    if let Ok(number) = suffix.parse::<u32>() {
        return Some(number);
    }
    let words = suffix.replace('-', " ");
    let mut number = 0u32;
    for word in words.split_whitespace() {
        let value = match word {
            "one" => 1,
            "two" => 2,
            "three" => 3,
            "four" => 4,
            "five" => 5,
            "six" => 6,
            "seven" => 7,
            "eight" => 8,
            "nine" => 9,
            "ten" => 10,
            "eleven" => 11,
            "twelve" => 12,
            "thirteen" => 13,
            "fourteen" => 14,
            "fifteen" => 15,
            "sixteen" => 16,
            "seventeen" => 17,
            "eighteen" => 18,
            "nineteen" => 19,
            "twenty" => 20,
            "thirty" => 30,
            "forty" => 40,
            "fifty" => 50,
            "sixty" => 60,
            "seventy" => 70,
            "eighty" => 80,
            "ninety" => 90,
            "hundred" => {
                number = number.max(1) * 100;
                continue;
            }
            "and" => continue,
            _ => return None,
        };
        number += value;
    }
    (number > 0).then_some(number)
}
fn numbered_successor(a: &Checkpoint, b: &Checkpoint) -> bool {
    chapter_number(&a.label)
        .zip(chapter_number(&b.label))
        .is_none_or(|(a, b)| a.checked_add(1) == Some(b))
}
fn page_span_continuous(book: &Book, start: usize, end: usize) -> bool {
    book.continuous
        || book
            .pages
            .get(start..end.min(book.pages.len()))
            .is_some_and(|pages| {
                pages.windows(2).all(|pair| {
                    page_number(&pair[0].progress)
                        .zip(page_number(&pair[1].progress))
                        .is_none_or(|(a, b)| b >= a && b - a <= 3)
                })
            })
}
fn coverage_intact(book: &Book) -> bool {
    book.continuous
        || (page_span_continuous(book, 0, book.pages.len())
            && book
                .checkpoints
                .windows(2)
                .all(|c| numbered_successor(&c[0], &c[1])))
}
fn chapter_label<'a>(book: &Book, c: &'a Checkpoint) -> &'a str {
    if book
        .pages
        .get(c.page)
        .is_some_and(|p| p.heading.is_none() && p.progress == c.label)
    {
        if c.verified_start {
            "Opening pages"
        } else {
            "Selected passage"
        }
    } else {
        &c.label
    }
}
fn chapter_view(book: &Book, index: usize, c: &Checkpoint) -> Value {
    let end = book
        .checkpoints
        .get(index + 1)
        .map_or(book.sentences.len(), |n| n.sentence);
    let following = book.checkpoints.get(index + 1);
    let closed = (following.is_some() || book.end_verified)
        && (book.continuous || following.is_none_or(|next| numbered_successor(c, next)))
        && page_span_continuous(
            book,
            c.page,
            following.map_or(book.pages.len(), |next| next.page + 1),
        );
    let chapter_voices = book
        .sentences
        .iter()
        .skip(c.sentence)
        .take(end.saturating_sub(c.sentence))
        .filter(|s| s.end_sample > s.start_sample)
        .map(|s| {
            if s.voice_key.is_empty() {
                format!("kokoro:{}", book.voice.voice)
            } else {
                s.voice_key.clone()
            }
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mixed_voices = chapter_voices.len() > 1;
    let complete = c.verified_start
        && closed
        && end <= book.generated
        && (index + 1 < book.checkpoints.len() || book.pending.is_empty())
        && !mixed_voices;
    let duration = book
        .sentences
        .iter()
        .skip(c.sentence)
        .take(end.saturating_sub(c.sentence))
        .filter(|s| s.end_sample > s.start_sample)
        .map(|s| s.end_sample - s.start_sample)
        .sum::<u64>() as f64
        / RATE as f64;
    let voice_label = if mixed_voices {
        String::new()
    } else {
        book.sentences
            .get(c.sentence)
            .filter(|s| s.end_sample > s.start_sample)
            .map(|s| s.voice_label.clone())
            .filter(|label| !label.is_empty())
            .unwrap_or_default()
    };
    let voice_key = if mixed_voices {
        "mixed".to_string()
    } else {
        chapter_voices.into_iter().next().unwrap_or_default()
    };
    json!({"id":c.id,"rangeId":book.range_id,"label":chapter_label(book,c),"sentence":c.sentence,"page":book.pages.get(c.page).map(|p|&p.progress),"seconds":book.sentences.get(c.sentence).filter(|_|c.sentence<book.generated).map(|s|s.start_sample as f64/RATE as f64),"duration":duration,"complete":complete,"status":if complete{"complete"}else if book.message.is_some(){"attention"}else if c.sentence<book.generated{"partial"}else{"preparing"},"needsEarlier":!c.verified_start,"voiceKey":voice_key,"voiceLabel":voice_label})
}
fn save_bookmark(dir: &Path, book: &Book) -> Result<(), String> {
    let bytes = bookmark_value(book).to_string();
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(dir.join("bookmark.tmp"))
        .map_err(|e| e.to_string())?;
    file.write_all(bytes.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(dir.join("bookmark.tmp"), dir.join("bookmark.json")).map_err(|e| e.to_string())?;
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn load_bookmark(dir: &Path, book: &mut Book) {
    if let Some(value) = fs::read(dir.join("bookmark.json"))
        .ok()
        .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
    {
        // An interrupted range switch can leave the previous bookmark beside the new manifest.
        if value["rangeId"]
            .as_str()
            .is_some_and(|range| range != book.range_id)
        {
            return;
        }
        book.bookmark_sentence = value["sentenceId"].as_str().map(str::to_owned);
        book.bookmark_offset = value["sampleOffset"].as_u64().unwrap_or(0);
        if let Some(position) = value["position"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.)
        {
            book.position = position.min(book.seconds());
        }
        if let Some(speed) = value["speed"]
            .as_f64()
            .filter(|n| n.is_finite() && (0.5..=2.).contains(n))
        {
            book.speed = speed as f32;
        }
        if let Some(position) = stable_position(book) {
            book.position = position;
        }
    }
}
fn header(samples: u64) -> Result<Vec<u8>, String> {
    let bytes = u32::try_from(samples.checked_mul(2).ok_or("Audio exceeds WAV capacity")?)
        .map_err(|_| "Audio exceeds WAV capacity")?;
    let riff = bytes.checked_add(36).ok_or("Audio exceeds WAV capacity")?;
    let mut h = Vec::with_capacity(44);
    h.extend(b"RIFF");
    h.extend(riff.to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend((RATE as u32).to_le_bytes());
    h.extend((RATE as u32 * 2).to_le_bytes());
    h.extend(2u16.to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend(bytes.to_le_bytes());
    Ok(h)
}
fn recover(dir: &Path, book: &Book) -> Result<(), String> {
    if book.generated > book.sentences.len()
        || book
            .sentences
            .iter()
            .take(book.generated)
            .enumerate()
            .any(|(i, s)| {
                s.end_sample <= s.start_sample
                    || s.start_sample
                        != if i == 0 {
                            0
                        } else {
                            book.sentences[i - 1].end_sample
                        }
            })
        || book.committed_samples
            != book
                .sentences
                .get(book.generated.wrapping_sub(1))
                .map_or(0, |s| s.end_sample)
    {
        return Err("Invalid sentence commit index".into());
    }
    let committed_header = header(book.committed_samples)?;
    let path = audio_path(dir, book)?;
    if !path.exists() && book.committed_samples == 0 {
        crate::kokoro::write_wav(&path, &[], RATE as u32)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let length = 44 + book.committed_samples * 2;
    if file.metadata().map_err(|e| e.to_string())?.len() < length {
        return Err("Audiobook audio is shorter than its committed sentence index".into());
    }
    file.set_len(length).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    file.write_all(&committed_header)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn commit(dir: &Path, book: &mut Book, samples: &[f32]) -> Result<(), String> {
    if samples.is_empty() || samples.iter().any(|s| !s.is_finite()) {
        return Err("Sentence produced no valid audio".into());
    }
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())
        .collect();
    commit_pcm(dir, book, &bytes, None)
}
fn commit_with_voice(
    dir: &Path,
    book: &mut Book,
    samples: &[f32],
    voice: &str,
    label: &str,
) -> Result<(), String> {
    if samples.is_empty() || samples.iter().any(|s| !s.is_finite()) {
        return Err("Sentence produced no valid audio".into());
    }
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())
        .collect();
    if let Some(sentence) = book.sentences.get_mut(book.generated) {
        sentence.voice_label = label.to_string();
    }
    commit_pcm(dir, book, &bytes, Some(voice))
}
fn commit_pcm(
    dir: &Path,
    book: &mut Book,
    bytes: &[u8],
    voice: Option<&str>,
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return Err("Sentence produced no valid PCM audio".into());
    }
    let mut next = book.clone();
    let sentence = next
        .sentences
        .get_mut(next.generated)
        .ok_or("No pending sentence")?;
    sentence.start_sample = next.committed_samples;
    sentence.end_sample = next.committed_samples + (bytes.len() / 2) as u64;
    if let Some(voice) = voice {
        sentence.voice_key = voice.to_string();
    }
    next.committed_samples = sentence.end_sample;
    next.generated += 1;
    let h = header(next.committed_samples)?;
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .open(audio_path(dir, book)?)
            .map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(44 + book.committed_samples * 2))
            .map_err(|e| e.to_string())?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        f.write_all(&h)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        save(dir, &next)
    })();
    if result.is_err() {
        let published = fs::read(dir.join("book.json"))
            .ok()
            .and_then(|v| serde_json::from_slice::<Book>(&v).ok())
            .is_some_and(|b| {
                b.committed_samples == next.committed_samples && b.generated == next.generated
            });
        if published {
            *book = next;
        } else {
            recover(dir, book)?;
        }
    } else {
        *book = next;
    }
    result
}
#[cfg(test)]
fn cached_suffix(book: &Book, reader: &Value, text: &str) -> Option<Box<Book>> {
    cached_suffix_match(book, reader, text)
        .ok()
        .flatten()
        .map(|(range, _)| range)
}
fn cached_suffix_match(
    book: &Book,
    reader: &Value,
    text: &str,
) -> Result<Option<(Box<Book>, usize)>, String> {
    let wanted_voice = recording_key(book);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let number = reader["progress"].as_str().and_then(page_number);
    let mut found = None;
    for range in &book.saved_ranges {
        if recording_key(range) != wanted_voice {
            continue;
        }
        let exact = range.pages.first().is_some_and(|p| {
            p.text == normalized
                && (reader["fingerprint"].as_str() == Some(&p.fingerprint)
                    || number.is_some() && page_number(&p.progress) == number)
        });
        let offset = if exact {
            Some(0)
        } else {
            head_offset(range, &normalized)?
        };
        if let Some(offset) = offset {
            if found.is_some() {
                return Err("More than one saved range matches this passage. Locate the missing passage before continuing.".into());
            }
            found = Some((range.clone(), offset));
        }
    }
    Ok(found)
}
fn publish_suffix(dir: &Path, book: &mut Book, suffix: &Book) -> Result<(), String> {
    if book.generated != book.sentences.len() || !book.pending.is_empty() {
        return Err("Finish the current passage before joining saved audio".into());
    }
    let mut next = book.clone();
    remember_position(&mut next);
    let samples = next.committed_samples;
    let pages = next.pages.len();
    let sentences = next.sentences.len();
    next.pages.extend(suffix.pages.clone());
    for mut sentence in suffix.sentences.clone() {
        if sentence.end_sample > 0 {
            sentence.start_sample += samples;
            sentence.end_sample += samples;
        }
        for source in &mut sentence.sources {
            source.page += pages;
        }
        next.sentences.push(sentence);
    }
    for mut source in suffix.pending.clone() {
        source.page += pages;
        next.pending.push(source);
    }
    for (index, mut checkpoint) in suffix.checkpoints.clone().into_iter().enumerate() {
        if index == 0 && !checkpoint.verified_start {
            // Keep the existing chapter identity when its missing beginning has been filled.
            if let Some(previous) = next.checkpoints.last_mut() {
                previous.id = checkpoint.id;
            }
            continue;
        }
        checkpoint.page += pages;
        checkpoint.sentence += sentences;
        next.checkpoints.push(checkpoint);
    }
    next.generated += suffix.generated;
    next.committed_samples += suffix.committed_samples;
    next.saved_ranges.retain(|r| r.range_id != suffix.range_id);
    next.end_verified = suffix.end_verified;
    next.complete = coverage_intact(&next)
        && next.from_beginning
        && next.end_verified
        && next.saved_ranges.is_empty()
        && next.generated == next.sentences.len()
        && next.pending.is_empty()
        && next.checkpoints.iter().all(|c| c.verified_start);
    next.audio_file = format!("{}.wav", identity());
    let target = audio_path(dir, &next)?;
    let result = (|| {
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        output
            .write_all(&header(next.committed_samples)?)
            .map_err(|e| e.to_string())?;
        for source in [book, suffix] {
            let mut input = fs::File::open(audio_path(dir, source)?).map_err(|e| e.to_string())?;
            input.seek(SeekFrom::Start(44)).map_err(|e| e.to_string())?;
            if std::io::copy(&mut input.take(source.committed_samples * 2), &mut output)
                .map_err(|e| e.to_string())?
                != source.committed_samples * 2
            {
                return Err("Saved recording is shorter than its committed audio".into());
            }
        }
        output.sync_all().map_err(|e| e.to_string())?;
        save(dir, &next)
    })();
    let published = fs::read(dir.join("book.json"))
        .ok()
        .and_then(|v| serde_json::from_slice::<Book>(&v).ok())
        .is_some_and(|b| b.audio_file == next.audio_file);
    if published {
        *book = next;
        let _ = save_bookmark(dir, book);
        Ok(())
    } else {
        let _ = fs::remove_file(target);
        result.and(Err("Saved audio was not published".into()))
    }
}
fn refresh_chapter_files(dir: &Path, book: &Book) -> Result<(), String> {
    let folder = dir.join("chapters");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for (index, c) in book.checkpoints.iter().enumerate() {
        uuid::Uuid::parse_str(&c.id).map_err(|_| "Invalid chapter identity")?;
        let Some(start) = book
            .sentences
            .get(c.sentence)
            .filter(|_| c.sentence < book.generated)
            .map(|s| s.start_sample)
        else {
            continue;
        };
        let end = book
            .checkpoints
            .get(index + 1)
            .map_or(book.generated, |n| n.sentence.min(book.generated));
        let samples = book
            .sentences
            .get(end.saturating_sub(1))
            .map_or(0, |s| s.end_sample)
            .saturating_sub(start);
        let path = folder.join(format!("{}.wav", c.id));
        let meta = folder.join(format!("{}.json", c.id));
        let expected = json!({"audio":book.audio_file,"start":start,"samples":samples});
        if fs::read(&meta)
            .ok()
            .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
            .as_ref()
            == Some(&expected)
            && path.exists()
        {
            continue;
        }
        let temporary = folder.join(format!("{}.tmp", c.id));
        let mut out = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        out.write_all(&header(samples)?)
            .map_err(|e| e.to_string())?;
        let mut input = fs::File::open(audio_path(dir, book)?).map_err(|e| e.to_string())?;
        input
            .seek(SeekFrom::Start(44 + start * 2))
            .map_err(|e| e.to_string())?;
        if std::io::copy(&mut input.take(samples * 2), &mut out).map_err(|e| e.to_string())?
            != samples * 2
        {
            return Err("Chapter audio is incomplete".into());
        }
        out.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        fs::write(meta, expected.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn key(reader: &Value) -> Result<String, String> {
    let url = url::Url::parse(reader["url"].as_str().ok_or("Missing reader URL")?)
        .map_err(|e| e.to_string())?;
    let id = reader["bookId"].as_str().ok_or("Missing book identity")?;
    if reader["status"] != "ready"
        || !matches!(url.host_str(), Some("read.amazon.ca" | "read.amazon.com"))
        || url.scheme() != "https"
        || id.len() != 10
        || !id.chars().all(|c| c.is_ascii_alphanumeric())
        || !url.query_pairs().any(|(k, v)| k == "asin" && v == id)
    {
        return Err("Select a supported, open Kindle book".into());
    }
    Ok(format!("{}:{id}", url.host_str().unwrap()))
}
fn page_number(progress: &str) -> Option<u64> {
    let mut p = progress.split_whitespace();
    if !p.next()?.eq_ignore_ascii_case("Page") {
        return None;
    }
    p.next()?.parse().ok()
}
// A common OCR split drops the p in Chapter. Only accept a numbered heading at the page start.
fn ocr_chapter_prefix(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    let first = words.next()?.to_ascii_lowercase();
    if !matches!(first.as_str(), "cha" | "chap") || !words.next()?.eq_ignore_ascii_case("ter") {
        return None;
    }
    let mut label = String::from("Chapter");
    let mut recognized = None;
    for word in words.take(4) {
        label.push(' ');
        label.push_str(word);
        if chapter_number(&label).is_some() {
            recognized = Some(label.clone());
        } else {
            break;
        }
    }
    recognized
}
fn repair_ocr_chapter_boundaries(book: &mut Book) -> bool {
    let mut changed = false;
    for page in 0..book.pages.len() {
        if book.pages[page].heading.is_some() {
            continue;
        }
        let Some(label) = ocr_chapter_prefix(&book.pages[page].text) else {
            continue;
        };
        // Never split a sentence assembled across pages or infer a missing chapter start.
        let Some(sentence) = book.sentences.iter().position(|s| {
            s.sources
                .first()
                .is_some_and(|source| source.page == page && source.start == 0)
                && s.sources.iter().all(|source| source.page == page)
        }) else {
            continue;
        };
        if let Some(checkpoint) = book
            .checkpoints
            .iter_mut()
            .find(|c| c.page == page && c.sentence == sentence)
        {
            checkpoint.label = label.clone();
            checkpoint.verified_start = true;
        } else {
            book.checkpoints.push(Checkpoint {
                id: identity(),
                verified_start: true,
                label: label.clone(),
                sentence,
                page,
            });
        }
        book.pages[page].heading = Some(label);
        changed = true;
    }
    book.checkpoints.sort_by_key(|c| c.sentence);
    for range in &mut book.saved_ranges {
        changed |= repair_ocr_chapter_boundaries(range);
    }
    changed
}
fn chapter(text: &str) -> Option<String> {
    if let Some(label) = ocr_chapter_prefix(text) {
        return Some(label);
    }
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    let lower = line.to_lowercase();
    ((lower == "prologue"
        || lower == "epilogue"
        || lower.starts_with("chapter ")
        || lower.starts_with("part "))
        && line.len() < 100)
        .then(|| line.to_owned())
}
#[path = "audiobooks/full_scan.rs"]
mod full_scan;
#[path = "audiobooks/matching.rs"]
mod matching;
use matching::{
    contains_earlier_context, continuation_offset, head_offset, page_matches, text_words,
};
#[cfg(test)]
fn locate_continuation_with(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
) -> Result<(Value, String, usize), String> {
    locate_continuation_with_limit(command, active, book, 12)
}
#[cfg(test)]
fn locate_continuation_with_limit(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
    view_limit: usize,
) -> Result<(Value, String, usize), String> {
    locate_continuation_report(&command, &active, book, view_limit, &|_| {})
}
fn locate_continuation_report(
    command: &dyn Fn(Value) -> Result<Value, String>,
    active: &dyn Fn() -> bool,
    book: &Book,
    view_limit: usize,
    report: &dyn Fn(Value),
) -> Result<(Value, String, usize), String> {
    let mut trace = Vec::<Value>::new();
    let mut nearby = None::<Value>;
    let hint = book
        .reader_hint
        .as_ref()
        .and_then(|h| ReaderPosition::parse(&h.progress))
        .or_else(|| {
            book.pages
                .last()
                .and_then(|p| ReaderPosition::parse(&p.progress))
        });
    let mut reader = command(json!({"type":"inspect"}))?;
    if key(&reader)? != book.key {
        return Err("Connected book changed".into());
    }
    if !active() {
        return Err("Preparation stopped".into());
    }
    let mut visited = std::collections::HashSet::new();
    if view_limit > 12 {
        let seek_direction = ReaderPosition::parse(reader["progress"].as_str().unwrap_or(""))
            .zip(hint)
            .and_then(|(at, goal)| match at.compare(goal) {
                Some(std::cmp::Ordering::Less) => Some("next"),
                Some(_) => Some("previous"),
                None if matches!(at, ReaderPosition::Location(1))
                    && reader["startOfBook"] == true =>
                {
                    Some("next")
                }
                None if matches!(goal, ReaderPosition::Location(1)) => Some("previous"),
                None => None,
            });
        for _ in 0..view_limit {
            if !active() {
                return Err("Preparation stopped".into());
            }
            if key(&reader)? != book.key {
                return Err("Connected book changed".into());
            }
            let Some((at, goal)) =
                ReaderPosition::parse(reader["progress"].as_str().unwrap_or("")).zip(hint)
            else {
                break;
            };
            // Approach within two views before the bounded passage search. Starting
            // six pages away could reverse after five turns before reaching the hint.
            let distance = match (at, goal) {
                (ReaderPosition::FrontMatter(a), ReaderPosition::FrontMatter(b))
                | (ReaderPosition::Page(a), ReaderPosition::Page(b))
                | (ReaderPosition::Location(a), ReaderPosition::Location(b)) => Some(a.abs_diff(b)),
                _ => None,
            };
            let comparison = at.compare(goal);
            if distance.is_some_and(|d| d <= 2)
                || (seek_direction == Some("next")
                    && comparison == Some(std::cmp::Ordering::Greater))
                || (seek_direction == Some("previous")
                    && comparison == Some(std::cmp::Ordering::Less))
            {
                break;
            }
            if !visited.insert(
                reader["fingerprint"]
                    .as_str()
                    .ok_or("Missing page identity")?
                    .to_owned(),
            ) {
                return Err(
                    "Reader navigation repeated a view while locating the saved passage".into(),
                );
            }
            let Some(direction) = seek_direction else {
                break;
            };
            if reader[if direction == "next" {
                "endOfBook"
            } else {
                "startOfBook"
            }] == true
            {
                break;
            }
            reader = command(
                json!({"type":"navigate","bookId":reader["bookId"],"fingerprint":reader["fingerprint"],"direction":direction}),
            )?;
        }
        visited.clear();
    }
    let mut previous_forward_view: Option<String> = None;
    let scan_limit = 12;
    let mut direction: Option<&str> = None;
    let mut reversed = false;
    let mut scans = 0;
    let mut travelled = 0;
    let mut anchored = false;
    for _ in 0..scan_limit * 3 {
        if !active() {
            return Err("Preparation stopped".into());
        }
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        let fingerprint = reader["fingerprint"]
            .as_str()
            .ok_or("Missing page identity")?
            .to_owned();
        let fresh = visited.insert(fingerprint.clone());
        if !fresh {
            // Skipping a view breaks adjacency: never join OCR across that gap.
            previous_forward_view = None;
            if !reversed {
                break;
            }
            let d = direction.unwrap_or("next");
            if reader[if d == "next" {
                "endOfBook"
            } else {
                "startOfBook"
            }] == true
            {
                break;
            }
            reader = command(
                json!({"type":"navigate","bookId":reader["bookId"],"fingerprint":fingerprint,"direction":d}),
            )?;
            continue;
        }
        scans += 1;
        let result =
            command(json!({"type":"ocr","bookId":reader["bookId"],"fingerprint":fingerprint}))?;
        if !active() {
            return Err("Preparation stopped".into());
        }
        if result["reader"]["fingerprint"] != reader["fingerprint"]
            || key(&result["reader"])? != book.key
        {
            return Err("The reader moved while locating your saved sentence. Try again.".into());
        }
        let raw = result["text"]
            .as_str()
            .ok_or("No text returned while locating your saved sentence")?;
        if raw.chars().count() > 20_000 {
            return Err("Page exceeds supported text size".into());
        }
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        trace.push(json!({"page":reader["progress"],"fingerprint":fingerprint,"direction":direction,"reason":"saved-ending-not-found"}));
        if trace.len() > 64 {
            trace.remove(0);
        }
        if let Some(offset) = continuation_offset(book, &normalized)? {
            report(json!({"step":"matched","label":"Found your saved passage","views":trace}));
            return Ok((result["reader"].clone(), raw.into(), offset));
        }
        if let Some((index, _)) = matching::earlier_anchor(book, &normalized)? {
            anchored = true;
            if index < book.generated {
                nearby = Some(
                    json!({"sentenceId":book.sentences[index].id,"seconds":book.sentences[index].start_sample as f64/RATE as f64,"page":reader["progress"]}),
                );
            }
            direction = Some("next");
            trace.last_mut().unwrap()["reason"] = json!("earlier-sentences-matched");
        }
        report(
            json!({"step":"searching","label":format!("Finding your saved passage · {}",reader["progress"].as_str().unwrap_or("")),"nearby":nearby,"views":trace}),
        );
        if let Some(previous) = &previous_forward_view {
            let previous_words = text_words(previous);
            let current_words = text_words(&normalized);
            let overlap = (6..=previous_words.len().min(current_words.len()))
                .rev()
                .find(|&length| {
                    previous_words[previous_words.len() - length..]
                        .iter()
                        .zip(&current_words[..length])
                        .all(|(a, b)| a.key == b.key)
                })
                .unwrap_or(0);
            let retained = previous_words.len() - overlap;
            if retained > 0 {
                let left = retained.saturating_sub(128);
                let start = if left == 0 {
                    0
                } else {
                    previous_words[left - 1].end
                };
                let prefix = previous[start..previous_words[retained - 1].end].trim();
                let prefix_length = prefix.len() + 1;
                let joined = format!("{prefix} {normalized}");
                if let Some(end) = continuation_offset(book, &joined)? {
                    if end >= prefix_length {
                        report(
                            json!({"step":"matched","label":"Found your saved passage across adjacent views","views":trace}),
                        );
                        return Ok((result["reader"].clone(), raw.into(), end - prefix_length));
                    }
                }
            }
        }
        let preferred = if contains_earlier_context(book, &normalized) {
            "next"
        } else if reader["progress"]
            .as_str()
            .and_then(ReaderPosition::parse)
            .zip(hint)
            .is_some_and(|(at, goal)| at.compare(goal) == Some(std::cmp::Ordering::Greater))
        {
            "previous"
        } else {
            "next"
        };
        let mut d = *direction.get_or_insert(preferred);
        travelled += 1;
        let edge = reader[if d == "next" {
            "endOfBook"
        } else {
            "startOfBook"
        }] == true;
        if scans >= scan_limit {
            break;
        }
        if !anchored && !reversed && (travelled >= 5 || edge) {
            d = if d == "next" { "previous" } else { "next" };
            direction = Some(d);
            reversed = true;
        } else if edge {
            break;
        }
        previous_forward_view = (d == "next").then_some(normalized);
        reader = command(
            json!({"type":"navigate","bookId":reader["bookId"],"fingerprint":reader["fingerprint"],"direction":d}),
        )?;
    }
    report(
        json!({"step":"unresolved","label":"Couldn’t verify the exact sentence","nearby":nearby,"views":trace}),
    );
    Err("Could not confidently locate your saved sentence in nearby views. Your saved audio is available; locate your saved position or choose Listen from this page.".into())
}
fn locate_for_job(
    app: &AppHandle,
    bridge: &crate::reader_bridge::ReaderBridge,
    service: &Audiobooks,
    id: &str,
    token: &str,
    book: &Book,
    view_limit: usize,
    expected_voice: &str,
) -> Result<(Value, String, usize), String> {
    locate_continuation_report(
        &|request| {
            bridge.command_while(request, || {
                generation_voice_active(app, service, id, token, expected_voice)
            })
        },
        &|| generation_voice_active(app, service, id, token, expected_voice),
        book,
        view_limit,
        &|update| {
            let mut l = service.0.lock().unwrap();
            if l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
                if let Some(b) = l.books.get_mut(id) {
                    b.activity = Some("locating".into());
                    b.recovery = Some(update);
                }
            }
        },
    )
}
fn heading_from(text: &str, offset: usize) -> Option<String> {
    let mut at = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let normal = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if at >= offset {
            return if at == offset { chapter(line) } else { None };
        }
        at += normal.len() + 1;
    }
    None
}
fn assemble(book: &mut Book, finished: bool) -> Result<(), String> {
    let joined = book
        .pending
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let (sentences, tail) = crate::streaming::book_sentences(&joined, finished);
    let used = joined.len() - tail.len();
    let slice = |start: usize, end: usize| {
        let mut offset = 0;
        let mut sources = Vec::new();
        for p in &book.pending {
            let left = start.max(offset);
            let right = end.min(offset + p.text.len());
            if right > left {
                sources.push(Source {
                    page: p.page,
                    start: p.start + left - offset,
                    end: p.start + right - offset,
                    text: p.text[left - offset..right - offset].into(),
                });
            }
            offset += p.text.len() + 1;
        }
        sources
    };
    let mut offset = 0;
    let mut ready = Vec::new();
    for text in sentences {
        let end = offset + text.len();
        if text.chars().count() > 5000 {
            return Err("A sentence exceeds the local narration limit. Review this page.".into());
        }
        if !text.trim().is_empty() {
            ready.push(Sentence {
                id: identity(),
                continuation_of: None,
                text: text.trim().into(),
                sources: slice(offset, end),
                start_sample: 0,
                end_sample: 0,
                voice_key: String::new(),
                voice_label: String::new(),
            });
        }
        offset = end;
    }
    let pending = slice(used, joined.len());
    book.sentences.extend(ready);
    book.pending = pending;
    Ok(())
}
// Convert normalized whitespace offsets back to the raw passage so chapter lines survive.
fn raw_prefix(text: &str, offset: usize) -> Result<&str, String> {
    let mut normalized = 0;
    let mut search = 0;
    for (i, word) in text.split_whitespace().enumerate() {
        let at = search + text[search..].find(word).ok_or("Invalid passage offset")?;
        if i > 0 {
            normalized += 1;
        }
        if normalized == offset {
            return Ok(&text[..at]);
        }
        normalized += word.len();
        search = at + word.len();
    }
    if normalized == offset {
        Ok(text)
    } else {
        Err("Invalid passage offset".into())
    }
}
fn stage_suffix_gap(
    book: &Book,
    reader: &Value,
    text: &str,
    start: usize,
) -> Result<Option<Book>, String> {
    if start == 0 {
        return Ok(None);
    }
    let raw = raw_prefix(text, start)?;
    let prefix = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if book.pages.last().is_some_and(|p| {
        p.fingerprint == reader["fingerprint"].as_str().unwrap_or("") && p.text == prefix
    }) {
        return Ok(None);
    }
    let offset = continuation_offset(book, &prefix)?.unwrap_or(0);
    if prefix[offset..].trim().is_empty() {
        return Ok(None);
    }
    if offset == 0 && contains_earlier_context(book, &prefix) {
        return Err("The gap overlaps recorded text and could not be aligned confidently.".into());
    }
    let mut next = book.clone();
    capture_from(&mut next, reader, raw, offset)?;
    Ok(Some(next))
}
fn capture(book: &mut Book, reader: &Value, text: &str) -> Result<(), String> {
    capture_from(book, reader, text, 0)
}
fn capture_from(book: &mut Book, reader: &Value, text: &str, offset: usize) -> Result<(), String> {
    capture_from_mode(book, reader, text, offset, false)
}
fn repeated_capture(book: &Book, reader: &Value, normal: &str, verified_forward: bool) -> bool {
    book.pages.iter().any(|p| {
        p.fingerprint == reader["fingerprint"].as_str().unwrap_or("")
            || (!normal.is_empty()
                && p.text == normal
                && !(verified_forward
                    && ReaderPosition::parse(&p.progress)
                        .zip(reader["progress"].as_str().and_then(ReaderPosition::parse))
                        .is_some_and(|(a, b)| a.compare(b) == Some(std::cmp::Ordering::Less))))
    })
}
fn capture_from_mode(
    book: &mut Book,
    reader: &Value,
    text: &str,
    offset: usize,
    verified_forward: bool,
) -> Result<(), String> {
    let fingerprint = reader["fingerprint"]
        .as_str()
        .ok_or("Missing page fingerprint")?;
    let normal = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !normal.is_char_boundary(offset) {
        return Err("Invalid continuation offset".into());
    }
    let offset = offset + normal[offset..].len() - normal[offset..].trim_start().len();
    if offset == normal.len() {
        book.reader_hint = Some(ReaderHint {
            progress: reader["progress"].as_str().unwrap_or("").into(),
            fingerprint: fingerprint.into(),
        });
        return Ok(());
    }
    let novel = normal[offset..].to_owned();
    if repeated_capture(book, reader, &normal, verified_forward) {
        return Err("Repeated page detected; generation stopped safely".into());
    }
    let heading = if offset == 0 {
        chapter(text)
    } else {
        heading_from(text, offset)
    };
    if heading.is_some() && !book.pending.is_empty() {
        return Err(
            "An unfinished sentence precedes a chapter. Review the source before continuing."
                .into(),
        );
    }
    let page = book.pages.len();
    let progress = reader["progress"].as_str().unwrap_or("").to_owned();
    if let Some(label) = heading.clone() {
        book.checkpoints.push(Checkpoint {
            id: identity(),
            verified_start: true,
            label,
            sentence: book.sentences.len(),
            page,
        });
    } else if page == 0 {
        book.checkpoints.push(Checkpoint {
            id: identity(),
            verified_start: book.from_beginning,
            label: progress.clone(),
            sentence: 0,
            page,
        });
    }
    book.pages.push(Page {
        heading,
        progress,
        fingerprint: fingerprint.into(),
        text: normal.clone(),
    });
    if !normal.is_empty() {
        book.pending.push(Source {
            page,
            start: offset,
            end: normal.len(),
            text: novel,
        });
    }
    assemble(book, false)?;
    book.reader_hint = Some(ReaderHint {
        progress: reader["progress"].as_str().unwrap_or("").into(),
        fingerprint: fingerprint.into(),
    });
    Ok(())
}
impl Audiobooks {
    pub fn setup(app: &AppHandle) -> Self {
        let service = Self::default();
        match root().and_then(|r| fs::read_dir(r).map_err(|e| e.to_string())) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let dir = entry.path();
                    if !dir.is_dir() {
                        continue;
                    }
                    match fs::read(dir.join("book.json"))
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| {
                            serde_json::from_slice::<Book>(&bytes).map_err(|e| e.to_string())
                        })
                        .and_then(|mut b| {
                            if dir.file_name().and_then(|s| s.to_str()) != Some(b.id.as_str()) {
                                return Err("Book identity mismatch".into());
                            }
                            recover(&dir, &b)?;
                            let _ = fs::remove_file(dir.join("sentence.tmp.wav"));
                            load_bookmark(&dir, &mut b);
                            if b.schema_version < 2 {
                                let backup = dir.join("book.legacy.json");
                                if !backup.exists() {
                                    fs::copy(dir.join("book.json"), &backup)
                                        .map_err(|e| e.to_string())?;
                                }
                                b.end_verified = b.complete;
                                for (i, c) in b.checkpoints.iter_mut().enumerate() {
                                    c.verified_start = (i == 0 && b.from_beginning)
                                        || b.pages.get(c.page).is_some_and(|p| {
                                            chapter(&c.label).is_some()
                                                && p.text.strip_prefix(&c.label).is_some_and(
                                                    |tail| {
                                                        tail.is_empty()
                                                            || tail.starts_with(char::is_whitespace)
                                                    },
                                                )
                                        });
                                }
                                b.schema_version = 2;
                                save(&dir, &b)?;
                                save_bookmark(&dir, &b)?;
                            }
                            if b.schema_version < 4 {
                                // Preserve recordings, but discard obsolete backward-preparation errors.
                                if b.message
                                    .as_deref()
                                    .is_some_and(|m| m.starts_with("Chapter preparation"))
                                {
                                    b.message = None;
                                }
                                b.schema_version = 4;
                                save(&dir, &b)?;
                            }
                            for range in &b.saved_ranges {
                                recover(&dir, range)?;
                            }
                            full_scan::recover_preparation(&dir, &mut b);
                            if repair_ocr_chapter_boundaries(&mut b) {
                                save(&dir, &b)?;
                            }
                            b.complete &= coverage_intact(&b);
                            b.activity = None;
                            Ok(b)
                        }) {
                        Ok(book) => {
                            service
                                .0
                                .lock()
                                .unwrap()
                                .books
                                .insert(book.id.clone(), book);
                        }
                        Err(e) => eprintln!("[TextHalo] audiobook recovery: {e}"),
                    }
                }
            }
            Err(e) => eprintln!("[TextHalo] audiobook library: {e}"),
        }
        let app = app.clone();
        let monitor = service.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let mut l = monitor.0.lock().unwrap();
            let Some(p) = l.playing.as_ref() else {
                continue;
            };
            let id = p.book.clone();
            let range_id = p.range_id.clone();
            let snapshot = p.player.snapshot();
            let current = app
                .state::<crate::AppState>()
                .job
                .lock()
                .unwrap()
                .is_current(p.job);
            if let Some(book) = l.books.get_mut(&id) {
                if let Err(e) =
                    directory(&id).and_then(|d| record_playback(&d, book, &range_id, &snapshot))
                {
                    book.message = Some(e);
                }
            }
            if !current || !snapshot.available {
                if l.producing
                    .as_ref()
                    .is_some_and(|(book, _, full)| book == &id && !full)
                {
                    l.producing = None;
                }
                l.playing = None;
            }
        });
        service
    }
    /// Stop and Quit flush immediately, rather than waiting for the periodic monitor.
    pub(crate) fn stopped(&self) {
        let mut library = self.0.lock().unwrap();
        if library
            .producing
            .as_ref()
            .is_some_and(|(_, _, explicit)| !explicit)
        {
            library.producing = None;
        }
        if let Some(playing) = library.playing.take() {
            let snapshot = playing.player.snapshot();
            if let Some(book) = library.books.get_mut(&playing.book) {
                if let Err(error) = directory(&book.id)
                    .and_then(|dir| record_playback(&dir, book, &playing.range_id, &snapshot))
                {
                    book.message = Some(error);
                }
            }
            if library
                .producing
                .as_ref()
                .is_some_and(|(id, _, explicit)| id == &playing.book && !explicit)
            {
                library.producing = None;
            }
        }
    }

    pub(crate) fn tray(&self, reader: &Value) -> Option<Value> {
        let context = self.reader_context(reader)?;
        let k = key(&context["reader"]).ok()?;
        if self.0.lock().unwrap().dismissed.as_deref() == Some(&k) {
            return None;
        }
        Some(context)
    }
    fn reader_context(&self, reader: &Value) -> Option<Value> {
        if reader["connected"] != true {
            return None;
        }
        let r = &reader["reader"];
        let k = key(r).ok()?;
        let mut l = self.0.lock().unwrap();
        if let Some(book) = l.books.values_mut().find(|b| b.key == k) {
            // Reconnection resolves this warning without dismissing other preparation errors.
            crate::reader_bridge::clear_connection_error(&mut book.message);
        }
        let book = l.books.values().find(|b| b.key == k);
        Some(
            json!({"reader":r,"differentPage":book.is_some_and(|b|different_page(b,r)),"book":book.map(Book::view),"generating":l.producing.as_ref().is_some_and(|(id,_,_)|book.is_some_and(|b|&b.id==id))}),
        )
    }
    pub(crate) fn minimized_preparation(&self, reader: &Value) -> Option<String> {
        if reader["connected"] != true || reader["reader"]["windowState"] != "minimized" {
            return None;
        }
        let k = key(&reader["reader"]).ok()?;
        let l = self.0.lock().unwrap();
        let (id, _, _) = l.producing.as_ref()?;
        l.books
            .get(id)
            .filter(|b| b.key == k && !b.full_scan.as_ref().is_some_and(|s| s.transcript_ready()))
            .map(|b| b.id.clone())
    }
    pub(crate) fn has_playback(&self, app: &AppHandle) -> bool {
        self.0.lock().unwrap().playing.as_ref().is_some_and(|p| {
            app.state::<crate::AppState>()
                .job
                .lock()
                .unwrap()
                .is_current(p.job)
                && p.player.snapshot().available
        })
    }
    fn snapshot(&self, app: &AppHandle) -> Value {
        let active_voice = voice_key(&app.state::<crate::AppState>().settings.lock().unwrap());
        let reader = app.state::<crate::reader_bridge::ReaderBridge>().status();
        let tray = self.tray(&reader);
        let reader_context = self.reader_context(&reader);
        let warning = self
            .minimized_preparation(&reader)
            .map(|id| json!({"bookId":id,"code":"chrome-minimized"}));
        let l = self.0.lock().unwrap();
        let playback = l
            .playing
            .as_ref()
            .filter(|p| {
                app.state::<crate::AppState>()
                    .job
                    .lock()
                    .unwrap()
                    .is_current(p.job)
                    && p.player.snapshot().available
            })
            .map(|p| {
                let audio = p.player.snapshot();
                let voice = l.books.get(&p.book).and_then(|book| {
                    book.ranges()
                        .find(|range| range.range_id == p.range_id)
                        .and_then(|range| voice_at_position(range, audio.position))
                });
                json!({"bookId":p.book,"audio":audio,"voiceKey":voice})
            });
        let books = l
            .books
            .values()
            .map(|book| {
                let playing = l.playing.as_ref().filter(|p| p.book == book.id);
                let range_and_audio = playing.and_then(|playing| {
                    let range = book
                        .ranges()
                        .find(|range| range.range_id == playing.range_id)?;
                    Some((range, playing.player.snapshot()))
                });
                let display_voice = range_and_audio
                    .as_ref()
                    .and_then(|(range, audio)| voice_at_position(range, audio.position));
                // Keep every saved voice group visible so the user can choose any
                // recording. Playback and resume state still follow the audible voice.
                let mut view = book.view();
                let resume_voice = display_voice.as_deref().unwrap_or(&active_voice);
                view["resume"] = resume_for_voice(book, resume_voice)
                    .map_or(Value::Null, |resume| resume);
                if let Some((range, audio)) = range_and_audio {
                    let sample = (audio.position * RATE as f64) as u64;
                    view["passage"] = range.sentences.iter().take(range.generated)
                        .find(|s| s.end_sample > sample)
                        .map(|s| json!({"text":s.text,"page":s.sources.first().and_then(|p|range.pages.get(p.page)).map(|p|&p.progress)}))
                        .unwrap_or(Value::Null);
                    if let Some(resume) =
                        resume_at_voice(range, Some(audio.position), display_voice.as_deref())
                    {
                        view["resume"] = resume;
                    }
                    view["position"] = json!(audio.position);
                    view["speed"] = json!(audio.speed);
                    view["seconds"] = json!(range.seconds());
                    view["generated"] = json!(range.generated);
                }
                view
            })
            .collect::<Vec<_>>();
        json!({"readerConnected":reader["connected"] == true,"readerWarning":warning,"books":books,"tray":tray,"readerContext":reader_context,"generating":l.producing.as_ref().map(|(id,_,_)|id),"playback":playback,"engine":format!("{:?}", app.state::<crate::AppState>().settings.lock().unwrap().engine)})
    }
}
#[tauri::command]
pub fn audiobook_state(app: AppHandle) -> Value {
    app.state::<Audiobooks>().snapshot(&app)
}
fn play(app: &AppHandle, id: &str, position: Option<f64>) -> Result<(), String> {
    play_selected(app, id, position, None, false)
}
fn check_selection(library: &Library, revision: Option<u64>) -> Result<(), String> {
    if revision.is_some_and(|r| r != library.selection_revision) {
        return Err("A newer listening choice replaced this request.".into());
    }
    Ok(())
}
fn play_selected(
    app: &AppHandle,
    id: &str,
    position: Option<f64>,
    revision: Option<u64>,
    saved_chapter: bool,
) -> Result<(), String> {
    play_selected_range(app, id, position, revision, saved_chapter, None)
}
fn play_selected_range(
    app: &AppHandle,
    id: &str,
    position: Option<f64>,
    revision: Option<u64>,
    saved_chapter: bool,
    range_id: Option<&str>,
) -> Result<(), String> {
    let service = app.state::<Audiobooks>();
    let mut l = service.0.lock().unwrap();
    check_selection(&l, revision)?;
    if l.transitioning {
        return Err("A passage is being selected. Try again in a moment.".into());
    }
    let book = l.books.get(id).ok_or("Audiobook not found")?;
    let b = if let Some(range) = range_id {
        book.ranges()
            .find(|b| b.range_id == range)
            .ok_or("Saved chapter not found")?
    } else {
        book
    };
    if b.pending_start.is_some() {
        return Err("Prepare the selected page before listening.".into());
    }
    if b.generated == 0 {
        return Err("Preparing the first sentence…".into());
    }
    let saved = if saved_chapter {
        let voice = voice_at_position(b, position.unwrap_or(b.position));
        Some(saved_playback_for_voice(b, position, voice.as_deref()).ok_or("Only completed chapters can be continued. Choose Listen from this page to start a new session.")?)
    } else {
        None
    };
    let seconds = saved.map(|(seconds, _, _)| seconds).unwrap_or_else(|| {
        position.unwrap_or(if b.complete && b.position >= b.seconds() - 0.05 {
            0.0
        } else {
            stable_position(b).unwrap_or(b.position)
        })
    });
    if !seconds.is_finite() || seconds < 0. || seconds > b.seconds() {
        return Err("Choose a position within generated audio".into());
    }
    let samples = saved.map_or(b.committed_samples, |(_, end, _)| end);
    let speed = b.speed;
    let finished = saved_chapter || !l.producing.as_ref().is_some_and(|(i, _, _)| i == id);
    let root = directory(id)?;
    let folder = book
        .full_scan
        .as_ref()
        .filter(|s| s.draft().range_id == b.range_id)
        .map(|s| s.folder(&root))
        .transpose()?
        .unwrap_or(root.clone());
    let audio = audio_path(&folder, b)?;
    let playing_range = b.range_id.clone();
    let job = crate::begin_speech(app, crate::Phase::Preparing);
    let state = app.state::<crate::AppState>();
    let mut active = state.job.lock().unwrap();
    if !active.is_current(job) {
        return Err("Playback superseded".into());
    }
    let player = Arc::new(crate::pcm::Player::new()?);
    player.update_file(&audio, samples, finished)?;
    if let Some((_, _, index)) = saved {
        let start = b.sentences[b.checkpoints[index].sentence].start_sample;
        player.set_playback_start(start)?;
    }
    state.spoken.activate_pcm(player.clone());
    player.set_speed(speed)?;
    player.seek(seconds)?;
    active.set(crate::Phase::Speaking, None, None);
    let _ = app.emit("kiegen:status", &active.status);
    drop(active);
    l.playing = Some(Playing {
        saved_chapter,
        range_id: playing_range.clone(),
        book: id.into(),
        job,
        player,
    });
    if let Some(b) = l.books.get_mut(id) {
        let range = b
            .range_mut(&playing_range)
            .ok_or("Saved chapter not found")?;
        range.position = seconds;
        remember_position(range);
        if folder != root || playing_range == b.range_id {
            save_bookmark(&folder, b.range_mut(&playing_range).unwrap())?;
        }
        if range_id.is_some() {
            save(&root, b)?;
        }
    }
    Ok(())
}
// Playback may reference the full-scan draft without activating it or cancelling its producer.
fn record_playback(
    root: &Path,
    book: &mut Book,
    range_id: &str,
    snapshot: &crate::pcm::PlaybackStatus,
) -> Result<(), String> {
    let folder = book
        .full_scan
        .as_ref()
        .filter(|s| s.draft().range_id == range_id)
        .map(|s| s.folder(root))
        .transpose()?
        .unwrap_or_else(|| root.to_path_buf());
    let range = book.range_mut(range_id).ok_or("Saved chapter not found")?;
    range.position = snapshot.position.min(range.seconds());
    range.speed = snapshot.speed;
    remember_position(range);
    if range_id == book.range_id {
        save_bookmark(root, book)
    } else {
        if folder != root {
            save_bookmark(&folder, book.range_mut(range_id).unwrap())?;
        }
        save(root, book)
    }
}
struct PassageTransition {
    service: Audiobooks,
    revision: u64,
}
impl Drop for PassageTransition {
    fn drop(&mut self) {
        self.service.0.lock().unwrap().transitioning = false;
    }
}
// Cancel before switching manifests: an in-flight sentence must never append to a new range.
fn wait_for_producer(app: &AppHandle, id: &str) -> Result<PassageTransition, String> {
    let service = app.state::<Audiobooks>();
    let transition = {
        let mut l = service.0.lock().unwrap();
        if l.transitioning {
            return Err("A passage is already being selected.".into());
        }
        if l.worker && !l.producing.as_ref().is_some_and(|(book, _, _)| book == id) {
            return Err("Another preparation is still finishing. Try again in a moment.".into());
        }
        l.producing = None;
        l.transitioning = true;
        l.selection_revision = l.selection_revision.wrapping_add(1);
        PassageTransition {
            service: service.inner().clone(),
            revision: l.selection_revision,
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while service.0.lock().unwrap().worker {
        if std::time::Instant::now() >= deadline {
            return Err("The previous page is still finishing. Try again in a moment.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    crate::stop_speaking(app.clone());
    Ok(transition)
}
async fn switch_range(app: &AppHandle, id: &str, range: &str) -> Result<u64, String> {
    let service = app.state::<Audiobooks>();
    {
        let l = service.0.lock().unwrap();
        if l.books
            .get(id)
            .is_some_and(|b| b.range_id == range && b.pending_start.is_none())
        {
            return Ok(l.selection_revision);
        }
    }
    let app = app.clone();
    let id = id.to_owned();
    let range = range.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        // Validate the requested checkpoint before interrupting playback.
        let service = app.state::<Audiobooks>();
        if !service.0.lock().unwrap().books.get(&id).is_some_and(|b| {
            b.range_id == range || b.saved_ranges.iter().any(|r| r.range_id == range)
        }) {
            return Err("Saved passage not found".into());
        }
        let _transition = wait_for_producer(&app, &id)?;
        let mut l = service.0.lock().unwrap();
        let book = l.books.get_mut(&id).ok_or("Audiobook not found")?;
        let mut next = book.clone();
        activate_range(&mut next, &range)?;
        next.pending_start = None;
        next.activity = None;
        save(&directory(&id)?, &next)?;
        *book = next;
        save_bookmark(&directory(&id)?, book)?;
        Ok(_transition.revision)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn audiobook_listen_page(
    app: AppHandle,
    id: String,
    fingerprint: String,
) -> Result<String, String> {
    let handle = app.clone();
    let book_id = id.clone();
    let (position, revision) = tauri::async_runtime::spawn_blocking(move || {
        let bridge = handle.state::<crate::reader_bridge::ReaderBridge>();
        let initial = bridge.command(json!({"type":"inspect"}))?;
        if initial["fingerprint"].as_str() != Some(&fingerprint) {
            return Err("The open page changed. Select it again.".into());
        }
        {
            let service = handle.state::<Audiobooks>();
            let l = service.0.lock().unwrap();
            let book = l.books.get(&book_id).ok_or("Audiobook not found")?;
            if key(&initial)? != book.key {
                return Err("Open the selected audiobook in the browser".into());
            }
        }
        let _transition = wait_for_producer(&handle, &book_id)?;
        let reader = bridge.command(json!({"type":"inspect"}))?;
        if reader["fingerprint"].as_str() != Some(&fingerprint) {
            return Err("The open page changed. Select it again.".into());
        }
        let result = bridge
            .command(json!({"type":"ocr","bookId":reader["bookId"],"fingerprint":fingerprint}))?;
        if result["reader"]["fingerprint"].as_str() != Some(&fingerprint) {
            return Err("The open page changed while capturing it.".into());
        }
        let text = result["text"]
            .as_str()
            .ok_or("No text returned for the page")?;
        let service = handle.state::<Audiobooks>();
        let mut l = service.0.lock().unwrap();
        let book = l.books.get_mut(&book_id).ok_or("Audiobook not found")?;
        let settings = handle
            .state::<crate::AppState>()
            .settings
            .lock()
            .unwrap()
            .clone();
        let position = select_listening_page(
            &directory(&book_id)?,
            book,
            &result["reader"],
            text,
            &settings,
        )?;
        Ok::<_, String>((position, _transition.revision))
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(seconds) = position {
        launch_playback(app, id.clone(), Some(seconds), revision, false).await?;
    } else {
        let book_id = id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            generate_mode(&app, Some(book_id), false, Some(revision))
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    Ok(id)
}
#[tauri::command]
pub async fn audiobook_play(
    app: AppHandle,
    id: String,
    seconds: Option<f64>,
    range_id: Option<String>,
) -> Result<(), String> {
    // Validate before switching ranges or interrupting an active listening session.
    let active_voice = voice_key(&app.state::<crate::AppState>().settings.lock().unwrap());
    let (range_id, seconds, preparing, revision) = {
        let service = app.state::<Audiobooks>();
        let l = service.0.lock().unwrap();
        let book = l.books.get(&id).ok_or("Audiobook not found")?;
        let target = if let Some(range) = range_id.as_deref() {
            book.ranges()
                .find(|b| b.range_id == range)
                .ok_or("Saved chapter not found")?
        } else if seconds.is_none() {
            let resume = resume_for_voice(book, &active_voice).ok_or(
                "No completed chapters yet. Choose Listen from this page to start listening.",
            )?;
            let range = resume["rangeId"].as_str().unwrap();
            book.ranges().find(|b| b.range_id == range).unwrap()
        } else {
            book
        };
        let requested_voice = if range_id.is_none() && seconds.is_none() {
            Some(active_voice.clone())
        } else {
            voice_at_position(target, seconds.unwrap_or(target.position))
        };
        let (seconds, _, _) = saved_playback_for_voice(target, seconds, requested_voice.as_deref())
            .ok_or("Only completed chapters can be continued.")?;
        (
            Some(target.range_id.clone()),
            Some(seconds),
            book.full_scan.is_some(),
            l.selection_revision,
        )
    };
    if preparing {
        return tauri::async_runtime::spawn_blocking(move || {
            play_selected_range(
                &app,
                &id,
                seconds,
                Some(revision),
                true,
                range_id.as_deref(),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let revision = if let Some(range) = range_id {
        switch_range(&app, &id, &range).await?
    } else {
        app.state::<Audiobooks>()
            .0
            .lock()
            .unwrap()
            .selection_revision
    };
    launch_playback(app, id, seconds, revision, true).await
}
async fn launch_playback(
    app: AppHandle,
    id: String,
    seconds: Option<f64>,
    revision: u64,
    saved_chapter: bool,
) -> Result<(), String> {
    let handle = app.clone();
    let book_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        play_selected(&handle, &book_id, seconds, Some(revision), saved_chapter)
    })
    .await
    .map_err(|e| e.to_string())??;
    // Saved chapters play locally; page sessions may keep preparing forward.
    let reader = app.state::<crate::reader_bridge::ReaderBridge>().status();
    let should_buffer = {
        let service = app.state::<Audiobooks>();
        let l = service.0.lock().unwrap();
        !saved_chapter
            && !l.worker
            && reader["connected"] == true
            && l.books.get(&id).is_some_and(|b| {
                !b.complete && key(&reader["reader"]).ok().as_deref() == Some(&b.key)
            })
    };
    if should_buffer {
        let handle = app.clone();
        let book_id = id.clone();
        let preparation = tauri::async_runtime::spawn_blocking(move || {
            generate_mode(&handle, Some(book_id), false, Some(revision))
        })
        .await
        .map_err(|e| e.to_string())?;
        if let Err(error) = preparation {
            let service = app.state::<Audiobooks>();
            let mut l = service.0.lock().unwrap();
            if l.selection_revision == revision {
                if let Some(book) = l.books.get_mut(&id) {
                    book.message = Some(error);
                    let _ = directory(&id).and_then(|d| save(&d, book));
                }
            }
        }
    }
    Ok(())
}
#[tauri::command]
pub fn audiobook_dismiss(app: AppHandle) -> Result<(), String> {
    let r = app.state::<crate::reader_bridge::ReaderBridge>().status();
    let k = key(&r["reader"])?;
    app.state::<Audiobooks>().0.lock().unwrap().dismissed = Some(k);
    Ok(())
}
#[tauri::command]
pub fn audiobook_stop_generation(app: AppHandle) {
    let service = app.state::<Audiobooks>();
    let mut l = service.0.lock().unwrap();
    l.producing = None;
    if let Some(p) = &l.playing {
        p.player.push(Vec::new(), true);
    }
}
fn navigate(
    bridge: &crate::reader_bridge::ReaderBridge,
    service: &Audiobooks,
    id: &str,
    token: &str,
    r: &Value,
    direction: &str,
) -> Result<Value, String> {
    bridge.command_while(json!({"type":"navigate","bookId":r["bookId"],"fingerprint":r["fingerprint"],"direction":direction}), || generation_active(service, id, token))
}
fn generation_active(service: &Audiobooks, id: &str, token: &str) -> bool {
    service
        .0
        .lock()
        .unwrap()
        .producing
        .as_ref()
        .is_some_and(|(i, t, _)| i == id && t == token)
}
fn generation_voice_active(
    app: &AppHandle,
    service: &Audiobooks,
    id: &str,
    token: &str,
    expected: &str,
) -> bool {
    generation_active(service, id, token)
        && synthesis_key(&app.state::<crate::AppState>().settings.lock().unwrap()) == expected
}
#[derive(Clone, Copy)]
enum ReaderPosition {
    FrontMatter(u64),
    Page(u64),
    Location(u64),
}
impl ReaderPosition {
    fn parse(progress: &str) -> Option<Self> {
        let mut words = progress.split_whitespace();
        let unit = words.next()?;
        let token = words.next()?;
        if unit.eq_ignore_ascii_case("page") {
            token
                .parse::<u64>()
                .ok()
                .map(Self::Page)
                .or_else(|| roman_page(token).map(Self::FrontMatter))
        } else if unit.eq_ignore_ascii_case("location") {
            Some(Self::Location(token.parse().ok()?))
        } else {
            None
        }
    }
    fn compare(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::FrontMatter(a), Self::FrontMatter(b))
            | (Self::Page(a), Self::Page(b))
            | (Self::Location(a), Self::Location(b)) => Some(a.cmp(&b)),
            (Self::FrontMatter(_), Self::Page(_)) => Some(std::cmp::Ordering::Less),
            (Self::Page(_), Self::FrontMatter(_)) => Some(std::cmp::Ordering::Greater),
            _ => None,
        }
    }
}
fn roman_page(token: &str) -> Option<u64> {
    if token.is_empty() || token.len() > 15 {
        return None;
    }
    let token = token.to_ascii_uppercase();
    let values = token
        .chars()
        .map(|c| match c {
            'I' => Some(1_u64),
            'V' => Some(5),
            'X' => Some(10),
            'L' => Some(50),
            'C' => Some(100),
            'D' => Some(500),
            'M' => Some(1000),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let number = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if values.get(i + 1).is_some_and(|&next| next > v) {
                -(v as i64)
            } else {
                v as i64
            }
        })
        .sum::<i64>();
    if !(1..=3999).contains(&number) {
        return None;
    }
    let mut remaining = number as u64;
    let mut canonical = String::new();
    for (value, symbol) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while remaining >= value {
            canonical.push_str(symbol);
            remaining -= value;
        }
    }
    (canonical == token).then_some(number as u64)
}
fn producer(
    app: &AppHandle,
    id: &str,
    token: &str,
    full: bool,
    autoplay: bool,
) -> Result<(), String> {
    if full {
        return full_scan::producer(app, id, token)
            .map_err(|error| format!("Audiobook preparation needs attention: {error}"));
    }
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let settings = app
        .state::<crate::AppState>()
        .settings
        .lock()
        .unwrap()
        .clone();
    let id_dir = directory(id)?;
    let book = service
        .0
        .lock()
        .unwrap()
        .books
        .get(id)
        .cloned()
        .ok_or("Audiobook not found")?;
    let mut book = book;
    select_generation_voice(&id_dir, &mut book, &settings)?;
    let expected_voice = synthesis_key(&settings);
    service
        .0
        .lock()
        .unwrap()
        .books
        .insert(id.to_string(), book.clone());
    let mut auto = autoplay;
    let stream_page_session = auto && (book.pending_start.is_some() || book.pages.is_empty());
    if auto && book.generated > 0 && book.pending_start.is_none() {
        play(app, id, None)?;
        auto = false;
    }
    let mut reader = if let Some(selected_fingerprint) = book.pending_start.as_deref() {
        // Listen from this page already captured the explicitly selected view in
        // select_page. Do not run the saved-position locator, which would navigate
        // away from that view and can fail when the old recording is unrelated.
        let reader = bridge.command_while(json!({"type":"inspect"}), || {
            generation_active(&service, id, token)
        })?;
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        if reader["fingerprint"].as_str() != Some(selected_fingerprint) {
            return Err("The selected page changed. Choose Listen from this page again.".into());
        }
        reader
    } else if !book.pages.is_empty() {
        {
            let mut l = service.0.lock().unwrap();
            if let Some(b) = l.books.get_mut(id) {
                b.activity = Some("locating".into());
            }
        }
        let (reader, text, offset) = locate_for_job(
            app,
            &bridge,
            &service,
            id,
            token,
            &book,
            if full { 10_000 } else { 12 },
            &expected_voice,
        )?;
        let mut l = service.0.lock().unwrap();
        if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
            return Ok(());
        }
        let current = l.books.get_mut(id).ok_or("Audiobook removed")?;
        let mut next = current.clone();
        capture_from(&mut next, &reader, &text, offset)?;
        next.message = None;
        next.activity = Some("forward".into());
        save(&directory(id)?, &next)?;
        *current = next;
        reader
    } else {
        let reader = bridge.command_while(json!({"type":"inspect"}), || {
            generation_active(&service, id, token)
        })?;
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        reader
    };
    let mut need_next = !book.pages.is_empty();
    loop {
        let (sentence, buffer, active) = {
            let mut l = service.0.lock().unwrap();
            let b = l.books.get(id).ok_or("Audiobook removed")?;
            let active = l
                .producing
                .as_ref()
                .is_some_and(|(i, t, _)| i == id && t == token);
            let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
            let sentence = if settings.engine == crate::config::Engine::Fish {
                match next_fish_batch_len(b, reader["endOfBook"] == true) {
                    Some(count) => {
                        coalesce_fish_batch(b, count);
                        if count > 1 {
                            save(&directory(id)?, b)?;
                        }
                        b.sentences.get(b.generated).cloned()
                    }
                    None => None,
                }
            } else {
                b.sentences.get(b.generated).cloned()
            };
            (sentence, listening_buffer(b), active)
        };
        if !active {
            return Ok(());
        }
        if !generation_voice_active(app, &service, id, token, &expected_voice) {
            return Err(
                "Voice settings changed during generation. Resume to use the newly selected voice."
                    .into(),
            );
        }
        // Finish the entire captured page before pausing the producer.
        if let Some(sentence) = sentence {
            let scratch = directory(id)?.join("sentence.tmp.wav");
            let mut streamed_samples = 0usize;
            let samples = if stream_page_session {
                let sentence_start = service.0.lock().unwrap().books[id].committed_samples;
                let mut startup = Vec::new();
                let mut on_audio = |chunk: &[f32]| -> Result<(), String> {
                    let player = service
                        .0
                        .lock()
                        .unwrap()
                        .playing
                        .as_ref()
                        .filter(|p| {
                            p.book == id
                                && !p.saved_chapter
                                && app
                                    .state::<crate::AppState>()
                                    .job
                                    .lock()
                                    .unwrap()
                                    .is_current(p.job)
                        })
                        .map(|p| p.player.clone());
                    if let Some(player) = player {
                        while player.buffered_seconds() >= STREAM_BUFFER_SECONDS
                            && generation_voice_active(app, &service, id, token, &expected_voice)
                        {
                            player.is_playing()?;
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        if !generation_voice_active(app, &service, id, token, &expected_voice) {
                            return Err("Preparation stopped".into());
                        }
                        player.push(chunk.to_vec(), false);
                        streamed_samples += chunk.len();
                    } else if auto {
                        startup.extend_from_slice(chunk);
                        streamed_samples += chunk.len();
                        let threshold = if settings.engine == crate::config::Engine::Fish {
                            1
                        } else {
                            RATE as usize / 2
                        };
                        if startup.len() >= threshold {
                            start_page_stream_playback(
                                app,
                                &service,
                                id,
                                token,
                                std::mem::take(&mut startup),
                            )?;
                            auto = false;
                        }
                    }
                    Ok(())
                };
                let state = app.state::<crate::AppState>();
                let _guard = state.synthesis.for_engine(settings.engine).lock().unwrap();
                if let Some(reason) = crate::selected_engine_refusal(&settings) {
                    return Err(reason);
                }
                let samples = if settings.engine == crate::config::Engine::Fish {
                    tauri::async_runtime::block_on(crate::hosted::render_speech_stream(
                        &sentence.text,
                        &settings.fish,
                        || generation_voice_active(app, &service, id, token, &expected_voice),
                        &mut on_audio,
                        || {
                            service
                                .0
                                .lock()
                                .unwrap()
                                .playing
                                .as_ref()
                                .filter(|p| p.book == id && !p.saved_chapter)
                                .map(|p| {
                                    p.player
                                        .played_audio_bytes()
                                        .saturating_sub(sentence_start.saturating_mul(2))
                                })
                        },
                    ))?
                } else {
                    state.spoken.render_streaming(
                        &settings,
                        &sentence.text,
                        || !generation_voice_active(app, &service, id, token, &expected_voice),
                        &mut on_audio,
                    )?
                };
                drop(on_audio);
                if auto && !startup.is_empty() {
                    start_page_stream_playback(app, &service, id, token, startup)?;
                    auto = false;
                }
                if streamed_samples > 0 && streamed_samples != samples.len() {
                    return Err("Streamed audio length did not match".into());
                }
                samples
            } else {
                render_sentence(app, &settings, &sentence.text, &scratch, || {
                    generation_voice_active(app, &service, id, token, &expected_voice)
                })?
            };
            if !generation_voice_active(app, &service, id, token, &expected_voice) {
                return Err("Voice settings changed during generation. Resume to use the newly selected voice.".into());
            }
            let mut l = service.0.lock().unwrap();
            if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
                return Ok(());
            }
            let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
            commit_with_voice(
                &directory(id)?,
                b,
                &samples,
                &voice_key(&settings),
                &voice_label(&settings),
            )?;
            let selected = selected_position(b);
            if let Some(position) = selected {
                b.position = position;
                b.pending_start = None;
                b.activity = None;
                remember_position(b);
                save(&directory(id)?, b)?;
                save_bookmark(&directory(id)?, b)?;
            }
            let ready = b.pending_start.is_none();
            let committed = b.committed_samples;
            let audio = audio_path(&directory(id)?, b)?;
            if let Some(p) = l.playing.as_ref().filter(|p| {
                p.book == id
                    && !p.saved_chapter
                    && app
                        .state::<crate::AppState>()
                        .job
                        .lock()
                        .unwrap()
                        .is_current(p.job)
            }) {
                if streamed_samples > 0 {
                    p.player.promote_file(&audio, committed, streamed_samples)?;
                } else {
                    p.player.update_file(&audio, committed, false)?;
                }
            }
            drop(l);
            if auto && ready {
                play(app, id, None)?;
                auto = false;
            }
            continue;
        }
        {
            let l = service.0.lock().unwrap();
            refresh_chapter_files(&directory(id)?, &l.books[id])?;
        }
        if !full && buffer >= 120. {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        if need_next {
            if reader["endOfBook"] == true {
                let mut l = service.0.lock().unwrap();
                let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
                assemble(b, true)?;
                if b.generated < b.sentences.len() {
                    save(&directory(id)?, b)?;
                    continue;
                }
                b.end_verified = true;
                b.complete = coverage_intact(b)
                    && b.from_beginning
                    && b.saved_ranges.is_empty()
                    && b.checkpoints.iter().all(|c| c.verified_start);
                refresh_chapter_files(&directory(id)?, b)?;
                b.message = if b.complete {
                    None
                } else {
                    Some("Generated through the end, starting from your imported page.".into())
                };
                save(&directory(id)?, b)?;
                if let Some(p) = l
                    .playing
                    .as_ref()
                    .filter(|p| p.book == id && !p.saved_chapter)
                {
                    p.player.push(Vec::new(), true);
                }
                return Ok(());
            }
            if !generation_active(&service, id, token) {
                return Ok(());
            }
            reader = navigate(&bridge, &service, id, token, &reader, "next")?;
        }
        let result = bridge.command_while(
            json!({"type":"ocr","bookId":reader["bookId"],"fingerprint":reader["fingerprint"]}),
            || generation_active(&service, id, token),
        )?;
        reader = result["reader"].clone();
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        let text = result["text"]
            .as_str()
            .ok_or("No text returned for the page")?;
        if text.chars().count() > 20_000 {
            return Err("Page exceeds supported text size".into());
        }
        let mut l = service.0.lock().unwrap();
        if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
            return Ok(());
        }
        let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
        if b.pages.len() >= 10_000 {
            return Err("This recording reached the 10,000-page safety limit".into());
        }
        if let Some((suffix, start)) = cached_suffix_match(b, &reader, text)? {
            if let Some(next) = stage_suffix_gap(b, &reader, text, start)? {
                save(&directory(id)?, &next)?;
                *b = next;
                need_next = false;
                continue;
            }
            if !b.pending.is_empty() {
                assemble(b, true)?;
                if let Some(last) = b.sentences.last_mut() {
                    last.continuation_of = suffix.sentences.first().map(|s| s.id.clone());
                }
                save(&directory(id)?, b)?;
                need_next = false;
                continue;
            }
            publish_suffix(&directory(id)?, b, &suffix)?;
            let merged = b.clone();
            if let Some(p) = l
                .playing
                .as_ref()
                .filter(|p| p.book == id && !p.saved_chapter)
            {
                p.player.update_file(
                    &audio_path(&directory(id)?, &merged)?,
                    merged.committed_samples,
                    false,
                )?;
            }
            drop(l);
            let (at, raw, offset) = locate_for_job(
                app,
                &bridge,
                &service,
                id,
                token,
                &merged,
                10_000,
                &expected_voice,
            )?;
            reader = at;
            let mut l = service.0.lock().unwrap();
            if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
                return Ok(());
            }
            let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
            let mut next = b.clone();
            capture_from(&mut next, &reader, &raw, offset)?;
            save(&directory(id)?, &next)?;
            *b = next;
            need_next = true;
            continue;
        }
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let offset = continuation_offset(b, &normalized)?;
        if offset.is_none() && contains_earlier_context(b, &normalized) {
            return Err("The reader returned to earlier recorded sentences. Locate your saved position before preparing more audio.".into());
        }
        let mut next = b.clone();
        next.activity = Some("forward".into());
        capture_from(&mut next, &reader, text, offset.unwrap_or(0))?;
        if next.pending.iter().map(|p| p.text.len()).sum::<usize>() > 20_000 {
            return Err("No reliable sentence boundary across several pages. Review the source before continuing.".into());
        }
        save(&directory(id)?, &next)?;
        *b = next;
        need_next = true;
    }
}
fn generate(app: &AppHandle, id: Option<String>, full: bool) -> Result<String, String> {
    generate_mode(app, id, full, None)
}
fn captured_reader(book: &Book, full: bool) -> Option<Value> {
    (full
        && book
            .full_scan
            .as_ref()
            .is_some_and(|s| s.transcript_ready()))
    .then(|| json!({"status":"ready","url":book.url,"bookId":book.book_id,"title":book.title}))
}
fn generate_mode(
    app: &AppHandle,
    id: Option<String>,
    full: bool,
    revision: Option<u64>,
) -> Result<String, String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let captured = if full {
        let library = service.0.lock().unwrap();
        id.as_ref()
            .and_then(|id| library.books.get(id))
            .and_then(|b| captured_reader(b, full))
    } else {
        None
    };
    let reader = if let Some(reader) = captured {
        reader
    } else {
        bridge.command(json!({"type":"inspect"}))?
    };
    let book_key = key(&reader)?;
    let mut library = service.0.lock().unwrap();
    check_selection(&library, revision)?;
    if library.transitioning {
        return Err("A passage is being selected. Try again in a moment.".into());
    }
    if library.worker {
        return Err("The previous page is still finishing. Try again in a moment.".into());
    }
    let existing = library
        .books
        .values()
        .find(|book| book.key == book_key)
        .map(|book| book.id.clone());
    let id = id
        .or(existing)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Some(book) = library.books.get(&id) {
        if book.key != book_key {
            return Err("Open the selected audiobook in the browser".into());
        }
        let requested = synthesis_key(&app.state::<crate::AppState>().settings.lock().unwrap());
        if book.complete && (!full || book.continuous) && recording_key(book) == requested {
            return Err("This audiobook is already complete".into());
        }
    } else {
        let book = Book {
            schema_version: 4,
            range_id: identity(),
            saved_ranges: Vec::new(),
            pending_start: None,
            reader_hint: None,
            recovery: None,
            full_scan: None,
            continuous: false,
            audio_file: default_audio(),
            end_verified: false,
            activity: None,
            bookmark_sentence: None,
            bookmark_offset: 0,
            id: id.clone(),
            key: book_key,
            title: reader["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Kindle book")
                .into(),
            url: reader["url"].as_str().unwrap().into(),
            book_id: reader["bookId"].as_str().unwrap().into(),
            pages: Vec::new(),
            sentences: Vec::new(),
            checkpoints: Vec::new(),
            pending: Vec::new(),
            committed_samples: 0,
            generated: 0,
            position: 0.0,
            speed: 1.0,
            complete: false,
            from_beginning: reader["startOfBook"] == true,
            message: None,
            voice: app
                .state::<crate::AppState>()
                .settings
                .lock()
                .unwrap()
                .kokoro
                .clone(),
            synthesis_key: synthesis_key(&app.state::<crate::AppState>().settings.lock().unwrap()),
        };
        let dir = directory(&id)?;
        recover(&dir, &book)?;
        save(&dir, &book)?;
        library.books.insert(id.clone(), book);
    }
    let playback = library
        .playing
        .as_ref()
        .filter(|p| p.book == id && !p.saved_chapter)
        .map(|p| p.player.snapshot());
    let requested_voice = synthesis_key(&app.state::<crate::AppState>().settings.lock().unwrap());
    let voice_changed = library
        .books
        .get(&id)
        .is_some_and(|book| book.generated > 0 && recording_key(book) != requested_voice);
    // If generation will replace the current recording, restarting its stream here
    // plays the previous voice while the new voice is being prepared.
    let restart = playback.is_some() && !voice_changed;
    if let Some(snapshot) = &playback {
        let b = library.books.get_mut(&id).unwrap();
        b.position = snapshot.position.min(b.seconds());
        b.speed = snapshot.speed;
        remember_position(b);
    }
    let autoplay = !full && !restart;
    let token = uuid::Uuid::new_v4().to_string();
    let book = library.books.get_mut(&id).unwrap();
    book.message = None;
    book.recovery = None;
    if book.pending_start.is_some() {
        book.activity = Some("selected".into());
    }
    save(&directory(&id)?, book)?;
    if voice_changed && playback.is_some() {
        // Replacing the old player must not call Stop: Stop also cancels this
        // audiobook's producer. Retire it before publishing the new producer.
        crate::begin_speech(app, crate::Phase::Idle);
        library.playing = None;
    }
    library.worker = true;
    library.producing = Some((id.clone(), token.clone(), full));
    drop(library);
    if restart && !full {
        let result = (|| {
            play(app, &id, None)?;
            if playback.is_some_and(|p| p.paused) {
                if let Some(p) = service.0.lock().unwrap().playing.as_ref() {
                    p.player.pause(true)?;
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            let mut library = service.0.lock().unwrap();
            library.producing = None;
            library.worker = false;
            return Err(error);
        }
    }
    let handle = app.clone();
    let book_id = id.clone();
    std::thread::spawn(move || {
        let mut result = producer(&handle, &book_id, &token, full, autoplay);
        if !generation_active(&handle.state::<Audiobooks>(), &book_id, &token) {
            result = Ok(());
        }
        let state = handle.state::<Audiobooks>();
        let mut library = state.0.lock().unwrap();
        library.worker = false;
        if let Some(book) = library.books.get_mut(&book_id) {
            book.activity = None;
            if let Err(error) =
                directory(&book_id).and_then(|dir| refresh_chapter_files(&dir, book))
            {
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
        if let Err(error) = result {
            if let Some(book) = library.books.get_mut(&book_id) {
                book.message = Some(error);
                let _ = directory(&book_id).and_then(|dir| save(&dir, book));
            }
        }
        if library
            .producing
            .as_ref()
            .is_some_and(|(_, t, _)| t == &token)
        {
            library.producing = None;
            if let Some(playing) = library.playing.as_ref().filter(|p| p.book == book_id) {
                playing.player.push(Vec::new(), true);
            }
        }
    });
    Ok(id)
}
#[tauri::command]
pub async fn audiobook_generate(
    app: AppHandle,
    id: Option<String>,
    full: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || generate(&app, id, full))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn split_ocr_chapter_headings_survive_line_breaks() {
        for heading in [
            "Cha\nter Three\nAt 5 a.m.",
            "Cha ter\nThree\nAt 5 a.m.",
            "\nChap\nter Three\nAt 5 a.m.",
        ] {
            assert_eq!(chapter(heading).as_deref(), Some("Chapter Three"));
        }
        assert!(chapter("At 5 a.m.\nCha ter Three").is_none());
    }
    #[test]
    fn reconnecting_the_same_book_clears_its_saved_connection_warning() {
        let service = Audiobooks::default();
        let mut b = book();
        b.message = Some("Connect the book from the Chrome extension first.".into());
        let id = b.id.clone();
        service.0.lock().unwrap().books.insert(id.clone(), b);
        let mut state = json!({"connected":false,"reader":reader(12,"page12")});
        assert!(service.reader_context(&state).is_none());
        assert!(service.0.lock().unwrap().books[&id].message.is_some());
        state["connected"] = json!(true);
        state["reader"]["bookId"] = json!("AAAAAAAAAA");
        service.reader_context(&state);
        assert!(service.0.lock().unwrap().books[&id].message.is_some());
        state["reader"]["bookId"] = json!("B0GY3RKVRZ");
        let context = service.reader_context(&state).unwrap();
        assert!(context["book"]["message"].is_null());
        assert!(service.0.lock().unwrap().books[&id].message.is_none());
    }
    #[test]
    fn ocr_split_chapter_headings_close_the_previous_chapter_and_repair_saved_markers() {
        let dir = std::env::temp_dir().join(format!("texthalo-heading-repair-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &reader(1, "one"),
            "Cha ter One\nThe traveler opened the wooden gate and stepped into the dark forest.",
        )
        .unwrap();
        assert_eq!(b.checkpoints[0].label, "Chapter One");
        recover(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.4; 24000]).unwrap();
        capture(
            &mut b,
            &reader(2, "two"),
            "Cha ter Two\nThe sun rose over the mountains while the village began to wake.",
        )
        .unwrap();
        assert_eq!(chapter_view(&b, 0, &b.checkpoints[0])["complete"], true);
        assert_eq!(chapter_view(&b, 1, &b.checkpoints[1])["complete"], false);
        let id = b.checkpoints[0].id.clone();
        let bytes = fs::read(audio_path(&dir, &b).unwrap()).unwrap();
        // Reproduce the saved metadata created before the OCR spelling was supported.
        b.checkpoints.truncate(1);
        b.checkpoints[0].verified_start = false;
        b.checkpoints[0].label = b.pages[0].progress.clone();
        for page in &mut b.pages {
            page.heading = None;
        }
        assert!(repair_ocr_chapter_boundaries(&mut b));
        assert_eq!(b.checkpoints[0].id, id);
        assert_eq!(b.checkpoints.len(), 2);
        assert_eq!(b.checkpoints[1].label, "Chapter Two");
        assert_eq!(chapter_view(&b, 0, &b.checkpoints[0])["complete"], true);
        assert!(!repair_ocr_chapter_boundaries(&mut b));
        assert_eq!(fs::read(audio_path(&dir, &b).unwrap()).unwrap(), bytes);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn recognizing_an_ocr_heading_cannot_fill_a_gap_in_chapter_coverage() {
        let mut b = book();
        capture(
            &mut b,
            &reader(1, "one"),
            "Cha ter One\nThe expedition left the village early in the morning.",
        )
        .unwrap();
        capture(
            &mut b,
            &reader(9, "two"),
            "Cha ter Two\nThe expedition returned home before the winter storm.",
        )
        .unwrap();
        b.generated = b.sentences.len();
        assert_eq!(chapter_view(&b, 0, &b.checkpoints[0])["complete"], false);
        assert!(chapter("I saw cha ter one written on a sign.").is_none());
        assert!(chapter("Cha ter nonsense").is_none());
    }
    #[test]
    fn roman_opening_positions_sort_before_numbered_pages_and_support_recovery() {
        use super::*;
        let third = ReaderPosition::parse("Page iii of 405 • 0%").unwrap();
        let fourth = ReaderPosition::parse("Page IV of 405 • 0%").unwrap();
        let first_numbered = ReaderPosition::parse("Page 1 of 405 • 0%").unwrap();
        assert!(matches!(third, ReaderPosition::FrontMatter(3)));
        assert_eq!(third.compare(fourth), Some(std::cmp::Ordering::Less));
        assert_eq!(
            fourth.compare(first_numbered),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            first_numbered.compare(third),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(third.compare(ReaderPosition::Location(3)), None);
        assert!(ReaderPosition::parse("Page iiii of 405").is_none());
        assert!(ReaderPosition::parse("Location iii of 405").is_none());
    }
    use super::*;
    #[test]
    fn missing_numbered_chapters_cannot_close_a_recorded_chapter() {
        let mut b = book();
        capture(
            &mut b,
            &reader(38, "p38"),
            "Chapter Five\nThe saved fifth chapter ends here.",
        )
        .unwrap();
        capture(
            &mut b,
            &reader(75, "p75"),
            "Chapter Fourteen\nA later chapter was imported separately.",
        )
        .unwrap();
        b.generated = b.sentences.len();
        assert_eq!(chapter_view(&b, 0, &b.checkpoints[0])["complete"], false);
        assert_eq!(chapter_view(&b, 0, &b.checkpoints[0])["status"], "partial");
    }
    #[test]
    fn dismissing_the_floating_suggestion_keeps_library_reader_controls_available() {
        let service = Audiobooks::default();
        let b = book();
        service.0.lock().unwrap().dismissed = Some(b.key.clone());
        service
            .0
            .lock()
            .unwrap()
            .books
            .insert(b.id.clone(), b.clone());
        let status = json!({"connected":true,"reader":reader(1,"one")});
        assert!(service.tray(&status).is_none());
        let context = service.reader_context(&status).unwrap();
        assert_eq!(context["book"]["id"], b.id);
    }
    #[test]
    fn recovery_uses_an_independent_earlier_sentence_group() {
        let mut b = resume_fixture();
        capture(&mut b, &reader(29, "tail"), "The final sentence contains damaged OCR words that are absent from the current capture.").unwrap();
        assert!(continuation_offset(&b, ANCHOR).unwrap().is_none());
        let (sentence, _) = matching::earlier_anchor(&b, ANCHOR).unwrap().unwrap();
        assert!(sentence + 1 < b.sentences.len());
    }
    #[test]
    fn recovery_at_a_chapter_start_uses_passage_matching() {
        let mut b = resume_fixture();
        b.checkpoints[0].label = "Chapter One".into();
        let command = |request: Value| -> Result<Value, String> {
            let r = reader(28, "current");
            match request["type"].as_str().unwrap() {
                "ocr" => Ok(json!({"reader":r,"text":ANCHOR})),
                "inspect" => Ok(r),
                _ => panic!("Unexpected page turn"),
            }
        };
        assert!(locate_continuation_with(command, || true, &b).is_ok());
    }
    #[test]
    fn recovery_refuses_another_book_before_pagination() {
        let mut b = resume_fixture();
        b.checkpoints[0].label = "Chapter One".into();
        let command = |request: Value| -> Result<Value, String> {
            assert_eq!(
                request["type"], "inspect",
                "Must not interact with another book"
            );
            let mut r = reader(28, "other");
            r["bookId"] = json!("AAAAAAAAAA");
            r["url"] = json!("https://read.amazon.ca/?asin=AAAAAAAAAA");
            Ok(r)
        };
        assert_eq!(
            locate_continuation_with(command, || true, &b).unwrap_err(),
            "Connected book changed"
        );
    }
    #[test]
    fn recovery_with_a_verified_chapter_never_requests_the_sidebar() {
        let mut b = resume_fixture();
        b.checkpoints[0].label = "Chapter One".into();
        b.checkpoints[0].verified_start = true;
        let requests = std::cell::RefCell::new(Vec::new());
        let command = |request: Value| -> Result<Value, String> {
            let kind = request["type"].as_str().unwrap();
            requests.borrow_mut().push(kind.to_owned());
            let r = reader(28, "saved-page");
            match kind {
                "inspect" => Ok(r),
                "ocr" => Ok(json!({"reader":r,"text":ANCHOR})),
                _ => panic!("Unexpected command: {kind}"),
            }
        };
        assert!(locate_continuation_with(command, || true, &b).is_ok());
        assert_eq!(*requests.borrow(), vec!["inspect", "ocr"]);
    }
    #[test]
    fn recovery_nearby_replay_does_not_supply_an_append_offset() {
        let mut b = resume_fixture();
        capture(&mut b,&reader(29,"tail"),"The final sentence contains damaged OCR words that are absent from the current capture.").unwrap();
        b.generated = b.sentences.len();
        let before = serde_json::to_value(&b).unwrap();
        let updates = std::cell::RefCell::new(Vec::new());
        let command = |request: Value| -> Result<Value, String> {
            let r = reader(28, "same-view");
            if request["type"] == "ocr" {
                Ok(json!({"reader":r,"text":ANCHOR}))
            } else {
                Ok(r)
            }
        };
        assert!(
            locate_continuation_report(&command, &|| true, &b, 12, &|v| updates
                .borrow_mut()
                .push(v))
            .is_err()
        );
        assert!(updates.borrow().last().unwrap()["nearby"]["sentenceId"].is_string());
        assert_eq!(serde_json::to_value(&b).unwrap(), before);
    }
    #[test]
    fn recovery_searches_both_sides_instead_of_reversing_into_a_visited_view() {
        let b = resume_fixture();
        let at = std::cell::Cell::new(29);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                at.set(if request["direction"] == "previous" {
                    at.get() - 1
                } else {
                    at.get() + 1
                });
            }
            let r = reader(at.get(), &format!("view{}", at.get()));
            if request["type"] == "ocr" {
                Ok(
                    json!({"reader":r,"text":if at.get()==27 {format!("{ANCHOR} {NOVEL}")} else {"Unrelated text in this view.".into()}}),
                )
            } else {
                Ok(r)
            }
        };
        assert!(locate_continuation_with(command, || true, &b).is_ok());
    }
    #[test]
    fn recovery_never_joins_nonadjacent_views_after_reversing_direction() {
        let mut b = book();
        let left = "The family walked along the river together discussing their plans";
        let right = "for the coming summer and returned home before sunset together.";
        capture(&mut b, &reader(49, "saved"), &format!("{left} {right}")).unwrap();
        let at = std::cell::Cell::new(50);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                at.set(if request["direction"] == "next" {
                    at.get() + 1
                } else {
                    at.get() - 1
                });
            }
            let r = reader(at.get(), &format!("p{}", at.get()));
            if request["type"] == "ocr" {
                let text = match at.get() {
                    46 => left.to_owned(),
                    51 => format!("{right} {NOVEL}"),
                    _ => "Unrelated intervening passage.".into(),
                };
                Ok(json!({"reader":r,"text":text}))
            } else {
                Ok(r)
            }
        };
        assert!(
            locate_continuation_with(command, || true, &b).is_err(),
            "Separated context halves must not create an append boundary"
        );
    }
    pub(super) fn book() -> Book {
        Book {
            schema_version: 4,
            range_id: identity(),
            saved_ranges: Vec::new(),
            pending_start: None,
            reader_hint: None,
            recovery: None,
            full_scan: None,
            continuous: false,
            audio_file: default_audio(),
            end_verified: false,
            activity: None,
            bookmark_sentence: None,
            bookmark_offset: 0,
            id: uuid::Uuid::new_v4().to_string(),
            key: "read.amazon.ca:B0GY3RKVRZ".into(),
            title: "Book".into(),
            url: "https://read.amazon.ca/?asin=B0GY3RKVRZ".into(),
            book_id: "B0GY3RKVRZ".into(),
            pages: vec![],
            sentences: vec![],
            checkpoints: vec![],
            pending: vec![],
            committed_samples: 0,
            generated: 0,
            position: 0.,
            speed: 1.,
            complete: false,
            from_beginning: true,
            message: None,
            voice: crate::config::KokoroSettings::default(),
            synthesis_key: String::new(),
        }
    }
    pub(super) fn reader(page: u64, fingerprint: &str) -> Value {
        json!({"status":"ready","url":"https://read.amazon.ca/?asin=B0GY3RKVRZ","bookId":"B0GY3RKVRZ","progress":format!("Page {page}"),"fingerprint":fingerprint})
    }
    #[test]
    fn an_open_browser_page_does_not_change_the_saved_listening_position() {
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "First.").unwrap();
        b.generated = 1;
        b.sentences[0].end_sample = 24000;
        b.committed_samples = 24000;
        b.position = 0.5;
        remember_position(&mut b);
        assert!(different_page(&b, &reader(8, "p8")));
        assert_eq!(b.position, 0.5);
        assert_eq!(stable_position(&b), Some(0.5));
        assert!(!different_page(&b, &reader(1, "p1")));
    }

    #[test]
    fn audiobook_view_hides_chapters_from_other_voice_recordings() {
        let mut current = book();
        current.generated = 1;
        current.committed_samples = 24_000;
        current.sentences.push(Sentence {
            id: "current-sentence".into(),
            continuation_of: None,
            text: "Current voice audio".into(),
            sources: vec![],
            start_sample: 0,
            end_sample: 24_000,
            voice_key: "fish:selected".into(),
            voice_label: "Selected voice".into(),
        });
        current.checkpoints.push(Checkpoint {
            id: "current-chapter".into(),
            verified_start: true,
            label: "Current voice chapter".into(),
            sentence: 0,
            page: 0,
        });
        let mut previous = book();
        previous.generated = 1;
        previous.committed_samples = 24_000;
        previous.sentences.push(Sentence {
            id: "previous-sentence".into(),
            continuation_of: None,
            text: "Previous voice audio".into(),
            sources: vec![],
            start_sample: 0,
            end_sample: 24_000,
            voice_key: "fish:previous".into(),
            voice_label: "Previous voice".into(),
        });
        previous.checkpoints.push(Checkpoint {
            id: "previous-chapter".into(),
            verified_start: true,
            label: "Previous voice chapter".into(),
            sentence: 0,
            page: 0,
        });
        current.saved_ranges.push(Box::new(previous));

        let chapters = current.view_for_voice(Some("fish:selected"))["checkpoints"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0]["id"], "current-chapter");
    }
    #[test]
    fn choosing_an_uncached_page_retains_old_audio_in_the_same_library_book() {
        let dir = std::env::temp_dir().join(format!("texthalo-page-choice-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "First.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.25; 24]).unwrap();
        b.position = 0.0005;
        remember_position(&mut b);
        let id = b.id.clone();
        let sentence = b.sentences[0].id.clone();
        let original = fs::read(audio_path(&dir, &b).unwrap()).unwrap();
        assert_eq!(
            select_page(&dir, &mut b, &reader(8, "p8"), "Selected passage.").unwrap(),
            None
        );
        assert_eq!(b.id, id);
        assert_eq!(b.saved_ranges.len(), 1);
        assert_eq!(b.saved_ranges[0].sentences[0].id, sentence);
        assert_eq!(
            fs::read(audio_path(&dir, &b.saved_ranges[0]).unwrap()).unwrap(),
            original
        );
        assert_eq!(b.pages[0].progress, "Page 8");
        assert!(b.pending_start.is_some());
        assert_eq!(selected_position(&b), None);
        commit(&dir, &mut b, &[0.5; 24]).unwrap();
        assert_eq!(selected_position(&b), Some(0.));
        let new_range = b.range_id.clone();
        assert_eq!(
            select_page(&dir, &mut b, &reader(1, "p1"), "First.").unwrap(),
            Some(0.)
        );
        assert_eq!(b.sentences[0].id, sentence);
        assert_eq!(b.saved_ranges[0].range_id, new_range);
        assert_eq!(fs::read(audio_path(&dir, &b).unwrap()).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cached_page_selection_reuses_audio_and_starts_a_new_session_after_reflow() {
        let dir = std::env::temp_dir().join(format!("texthalo-page-choice-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "First.").unwrap();
        capture(&mut b, &reader(2, "p2"), "Second.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.25; 24000]).unwrap();
        commit(&dir, &mut b, &vec![0.5; 24000]).unwrap();
        let audio = fs::read(audio_path(&dir, &b).unwrap()).unwrap();
        assert_eq!(
            select_page(&dir, &mut b, &reader(2, "p2"), "Second.").unwrap(),
            Some(1.)
        );
        assert_eq!(b.generated, 2);
        assert!(b.saved_ranges.is_empty());
        assert_eq!(b.position, 1.);
        assert_eq!(fs::read(audio_path(&dir, &b).unwrap()).unwrap(), audio);
        assert_eq!(
            select_page(&dir, &mut b, &reader(2, "new-view"), "Different layout.").unwrap(),
            None
        );
        assert_eq!(b.position, 0.);
        assert_eq!(b.saved_ranges.len(), 1);
        assert_eq!(
            fs::read(audio_path(&dir, &b.saved_ranges[0]).unwrap()).unwrap(),
            audio
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_backward_jump_joins_cached_later_audio_without_rendering_it_again() {
        let dir = std::env::temp_dir().join(format!("texthalo-suffix-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        b.from_beginning = false;
        capture(&mut b, &reader(3, "p3"), "Third.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.75; 24]).unwrap();
        let old_id = b.sentences[0].id.clone();
        select_page(&dir, &mut b, &reader(1, "p1"), "CHAPTER ONE\nFirst.").unwrap();
        commit(&dir, &mut b, &[0.25; 24]).unwrap();
        b.pending_start = None;
        b.position = 0.0005;
        remember_position(&mut b);
        capture(&mut b, &reader(2, "p2"), "Second.").unwrap();
        commit(&dir, &mut b, &[0.5; 24]).unwrap();
        let suffix = cached_suffix(&b, &reader(3, "different-fingerprint"), "Third.").unwrap();
        publish_suffix(&dir, &mut b, &suffix).unwrap();
        assert_eq!(b.generated, 3);
        assert_eq!(b.pages.len(), 3);
        assert!(b.saved_ranges.is_empty());
        assert_eq!(b.sentences[2].id, old_id);
        assert_eq!(stable_position(&b), Some(0.0005));
        assert_eq!(
            crate::spoken::Spoken::load_audio(&audio_path(&dir, &b).unwrap())
                .unwrap()
                .len(),
            72
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_bookmark_from_the_previous_range_cannot_overwrite_a_new_selection() {
        let dir = std::env::temp_dir().join(format!("texthalo-bookmark-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "First.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.25; 24000]).unwrap();
        b.position = 0.75;
        remember_position(&mut b);
        save_bookmark(&dir, &b).unwrap();
        let old = fs::read(dir.join("bookmark.json")).unwrap();
        select_page(&dir, &mut b, &reader(8, "p8"), "Eighth.").unwrap();
        commit(&dir, &mut b, &vec![0.5; 24000]).unwrap();
        fs::write(dir.join("bookmark.json"), old).unwrap();
        load_bookmark(&dir, &mut b);
        assert_eq!(b.position, 0.);
        let restored: Book =
            serde_json::from_slice(&fs::read(dir.join("book.json")).unwrap()).unwrap();
        assert_eq!(restored.range_id, b.range_id);
        assert_eq!(restored.saved_ranges.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn listening_from_a_cached_page_prepares_the_selected_fish_voice() {
        let dir = std::env::temp_dir().join(format!("texthalo-fish-page-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut settings = crate::config::Settings::default();
        settings.engine = crate::config::Engine::Fish;
        settings.fish.voice_id = "old-voice".into();
        let mut b = book();
        b.synthesis_key = synthesis_key(&settings);
        let page = reader(8, "p8");
        capture(&mut b, &page, "First sentence. Second sentence.").unwrap();
        recover(&dir, &b).unwrap();
        commit_with_voice(&dir, &mut b, &[0.25; 24000], "fish:old-voice", "Old voice").unwrap();
        let old_range = b.range_id.clone();
        assert_eq!(
            select_listening_page(
                &dir,
                &mut b,
                &page,
                "First sentence. Second sentence.",
                &settings
            )
            .unwrap(),
            Some(0.)
        );
        settings.fish.voice_id = "new-voice".into();
        assert_eq!(
            select_listening_page(
                &dir,
                &mut b,
                &page,
                "First sentence. Second sentence.",
                &settings
            )
            .unwrap(),
            None
        );
        assert_ne!(b.range_id, old_range);
        assert_eq!(b.generated, 0);
        assert_eq!(b.pending_start.as_deref(), Some("p8"));
        assert_eq!(recording_key(&b), synthesis_key(&settings));
        assert!(b
            .saved_ranges
            .iter()
            .any(|r| r.range_id == old_range && r.generated == 1));
        commit_with_voice(&dir, &mut b, &[0.5; 24000], "fish:new-voice", "New voice").unwrap();
        assert_eq!(selected_position(&b), Some(0.));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_selected_unfinished_page_is_not_throttled_by_older_cached_audio() {
        let dir = std::env::temp_dir().join(format!("texthalo-pending-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "First.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.25; (RATE * 121) as usize]).unwrap();
        capture(&mut b, &reader(2, "p2"), "An unfinished sentence").unwrap();
        assert_eq!(listening_buffer(&b), 121.);
        assert_eq!(
            select_page(&dir, &mut b, &reader(2, "p2"), "An unfinished sentence").unwrap(),
            None
        );
        assert_eq!(listening_buffer(&b), 0.);
        capture(&mut b, &reader(3, "p3"), "continues here.").unwrap();
        commit(&dir, &mut b, &[0.5; 24]).unwrap();
        assert_eq!(selected_position(&b), Some(121.));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn an_older_page_choice_cannot_launch_after_a_newer_choice_takes_over() {
        let service = Audiobooks::default();
        let previous = 1;
        {
            let mut library = service.0.lock().unwrap();
            library.selection_revision = previous;
            assert!(check_selection(&library, Some(previous)).is_ok());
            library.selection_revision += 1;
            library.transitioning = true;
        }
        let transition = PassageTransition {
            service: service.clone(),
            revision: 2,
        };
        assert!(check_selection(&service.0.lock().unwrap(), Some(previous)).is_err());
        drop(transition);
        let library = service.0.lock().unwrap();
        assert!(!library.transitioning);
        assert!(check_selection(&library, Some(previous)).is_err());
        assert!(check_selection(&library, Some(2)).is_ok());
    }

    #[test]
    fn bookmark_identifies_the_sentence_instead_of_only_absolute_time() {
        let dir = std::env::temp_dir().join(format!("texthalo-bookmark-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 5"}),
            "First. Second.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.25; 24000]).unwrap();
        commit(&dir, &mut b, &vec![0.5; 48000]).unwrap();
        b.position = 1.5;
        save_bookmark(&dir, &b).unwrap();
        let value: Value =
            serde_json::from_slice(&fs::read(dir.join("bookmark.json")).unwrap()).unwrap();
        assert!(value["sentenceId"].as_str().is_some());
        assert_eq!(value["sampleOffset"], 12000);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn continue_requires_both_chapter_boundaries_and_all_audio() {
        let mut b = book();
        b.from_beginning = false;
        capture(&mut b, &reader(5, "p5"), "A middle passage.").unwrap();
        capture(&mut b, &reader(6, "p6"), "CHAPTER TWO\nA new chapter.").unwrap();
        capture(&mut b, &reader(7, "p7"), "CHAPTER THREE\nThe next chapter.").unwrap();
        for (i, sentence) in b.sentences.iter_mut().enumerate() {
            sentence.start_sample = i as u64 * RATE;
            sentence.end_sample = (i as u64 + 1) * RATE;
        }
        b.generated = b.sentences.len();
        b.committed_samples = b.generated as u64 * RATE;
        let middle = b.checkpoints[1].sentence as f64;
        let end = b.checkpoints[2].sentence as f64;
        assert!(saved_playback(&b, Some(0.)).is_none()); // The imported fragment never becomes resumable.
        assert!(saved_playback(&b, Some(end)).is_none()); // The next chapter is still open.
        assert_eq!(
            saved_playback(&b, Some(middle)),
            Some((middle, (end * RATE as f64) as u64, 1))
        );
        assert_eq!(resume(&b).unwrap()["chapter"], "CHAPTER TWO");
        b.generated = b.checkpoints[2].sentence;
        b.sentences[b.generated].start_sample = 0; // The next chapter has not rendered yet.
        assert!(saved_playback(&b, Some(middle)).is_some());
        b.generated -= 1;
        assert!(resume(&b).is_none()); // A detected end is insufficient until rendering finishes.
    }

    #[test]
    fn completing_only_the_end_of_an_imported_chapter_does_not_enable_continue() {
        let mut b = book();
        b.from_beginning = false;
        capture(&mut b, &reader(5, "p5"), "A middle passage.").unwrap();
        b.generated = b.sentences.len();
        b.sentences[0].end_sample = RATE;
        b.committed_samples = RATE;
        b.end_verified = true;
        assert!(resume(&b).is_none());
        let reopened: Book = serde_json::from_value(serde_json::to_value(&b).unwrap()).unwrap();
        assert!(resume(&reopened).is_none());
        assert!(saved_playback(&reopened, Some(0.)).is_none());
    }

    #[test]
    fn a_midchapter_import_stays_available_but_is_not_labeled_complete() {
        let mut b = book();
        b.from_beginning = false;
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 5"}),
            "A middle passage.",
        )
        .unwrap();
        b.generated = 1;
        b.sentences[0].end_sample = 24000;
        b.committed_samples = 24000;
        assert_eq!(b.view()["checkpoints"][0]["status"], "partial");
        assert_eq!(b.view()["checkpoints"][0]["seconds"], 0.0);
    }

    #[test]
    fn chapter_files_grow_without_duplication_and_keep_their_identity() {
        let dir = std::env::temp_dir().join(format!("texthalo-chapters-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "CHAPTER ONE\nFirst.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.25; 24]).unwrap();
        let id = b.checkpoints[0].id.clone();
        refresh_chapter_files(&dir, &b).unwrap();
        let path = dir.join("chapters").join(format!("{id}.wav"));
        assert_eq!(crate::spoken::Spoken::load_audio(&path).unwrap().len(), 24);
        assert_eq!(b.view()["checkpoints"][0]["status"], "partial");
        capture(
            &mut b,
            &json!({"fingerprint":"b","progress":"Page 2"}),
            "Second.",
        )
        .unwrap();
        commit(&dir, &mut b, &[0.5; 48]).unwrap();
        capture(
            &mut b,
            &json!({"fingerprint":"c","progress":"Page 3"}),
            "CHAPTER TWO\nThird.",
        )
        .unwrap();
        refresh_chapter_files(&dir, &b).unwrap();
        refresh_chapter_files(&dir, &b).unwrap();
        assert_eq!(crate::spoken::Spoken::load_audio(&path).unwrap().len(), 72);
        assert_eq!(b.checkpoints[0].id, id);
        assert_eq!(b.view()["checkpoints"][0]["status"], "complete");
        assert_eq!(b.view()["checkpoints"][1]["status"], "preparing");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_bookmarks_and_sentence_identities_survive_reopening() {
        let dir = std::env::temp_dir().join(format!("texthalo-legacy-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "Original.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.5; 24000]).unwrap();
        let mut legacy = serde_json::to_value(&b).unwrap();
        legacy.as_object_mut().unwrap().remove("schemaVersion");
        legacy.as_object_mut().unwrap().remove("audioFile");
        legacy["sentences"][0].as_object_mut().unwrap().remove("id");
        legacy["checkpoints"][0]
            .as_object_mut()
            .unwrap()
            .remove("id");
        fs::write(
            dir.join("bookmark.json"),
            r#"{"position":0.5,"speed":1.25}"#,
        )
        .unwrap();
        let mut migrated: Book = serde_json::from_value(legacy).unwrap();
        load_bookmark(&dir, &mut migrated);
        recover(&dir, &migrated).unwrap();
        save(&dir, &migrated).unwrap();
        save_bookmark(&dir, &migrated).unwrap();
        let mut reopened: Book =
            serde_json::from_slice(&fs::read(dir.join("book.json")).unwrap()).unwrap();
        load_bookmark(&dir, &mut reopened);
        assert_eq!(migrated.sentences[0].id, reopened.sentences[0].id);
        assert_eq!(migrated.checkpoints[0].id, reopened.checkpoints[0].id);
        assert_eq!(reopened.position, 0.5);
        assert_eq!(reopened.speed, 1.25);
        assert_eq!(reopened.bookmark_offset, 12000);
        fs::remove_dir_all(dir).unwrap();
    }

    const ANCHOR: &str = "The family walked along the river together while discussing their plans for the coming summer. They returned home before sunset and prepared a quiet meal with the fresh vegetables from their garden.";
    const NOVEL: &str = "The next morning brought unexpected visitors who carried important news from the distant village.";
    fn resume_fixture() -> Book {
        let mut b = book();
        capture(&mut b, &reader(28, "old28"), ANCHOR).unwrap();
        b
    }
    #[test]
    fn reflow_resume_finds_the_frontier_inside_a_wider_spread_without_turning() {
        let b = resume_fixture();
        let turns = std::cell::Cell::new(0);
        let text = format!("Earlier material from another page. {ANCHOR} {NOVEL}");
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                turns.set(turns.get() + 1);
                panic!("Anchor is already visible");
            }
            let r = reader(26, "wide26");
            if request["type"] == "ocr" {
                Ok(json!({"reader":r,"text":text}))
            } else {
                Ok(r)
            }
        };
        let (r, normalized, offset) = locate_continuation_with(command, || true, &b).unwrap();
        assert_eq!(r["progress"], "Page 26");
        assert_eq!(turns.get(), 0);
        assert_eq!(normalized[offset..].trim(), NOVEL);
    }
    #[test]
    fn reflow_resume_tolerates_small_ocr_changes_but_rejects_repeated_context() {
        let b = resume_fixture();
        let text = format!("{} {NOVEL}", ANCHOR.replace("vegetables", "vegetab1es"));
        let offset = continuation_offset(&b, &text).unwrap().unwrap();
        assert_eq!(text[offset..].trim(), NOVEL);
        assert!(continuation_offset(&b, &format!("{ANCHOR} {NOVEL} {ANCHOR}")).is_err());
        assert_eq!(
            continuation_offset(&b, "They returned home. The family ate vegetables.").unwrap(),
            None
        );
    }
    #[test]
    fn reflow_capture_keeps_old_sentence_ids_and_only_appends_the_missing_text() {
        let mut b = resume_fixture();
        let ids = b.sentences.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        let spread = format!("Earlier material. {ANCHOR} {NOVEL}");
        let offset = continuation_offset(&b, &spread).unwrap().unwrap();
        capture_from(&mut b, &reader(26, "wide26"), &spread, offset).unwrap();
        assert_eq!(b.sentences.len(), ids.len() + 1);
        assert_eq!(
            b.sentences[..ids.len()]
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(b.sentences.last().unwrap().text, NOVEL);
        let source = &b.sentences.last().unwrap().sources[0];
        assert_eq!(
            &b.pages[source.page].text[source.start..source.end],
            source.text
        );
    }
    #[test]
    fn reflow_resume_preserves_an_unfinished_sentence_across_the_old_boundary() {
        let mut b = book();
        let tail = "The final journey would lead them toward";
        capture(&mut b, &reader(28, "old28"), &format!("{ANCHOR} {tail}")).unwrap();
        let text = format!("Earlier material. {ANCHOR} {tail} the village beside the mountains.");
        let offset = continuation_offset(&b, &text).unwrap().unwrap();
        capture_from(&mut b, &reader(26, "wide26"), &text, offset).unwrap();
        assert_eq!(
            b.sentences.last().unwrap().text,
            format!("{tail} the village beside the mountains.")
        );
        assert_eq!(b.sentences.last().unwrap().sources.len(), 2);
        assert!(b.pending.is_empty());
    }
    #[test]
    fn reflow_resume_searches_forward_from_matching_earlier_sentences() {
        let mut b = book();
        let earlier="A traveler carefully packed his belongings before leaving the house early in the morning. His sister stood beside the open gate and waved goodbye as he disappeared down the road.";
        capture(&mut b, &reader(27, "old27"), earlier).unwrap();
        capture(&mut b, &reader(28, "old28"), ANCHOR).unwrap();
        let at = std::cell::Cell::new(27);
        let turns = std::cell::Cell::new(0);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                assert_eq!(request["direction"], "next");
                at.set(29);
                turns.set(turns.get() + 1);
            }
            let r = reader(at.get(), &format!("wide{}", at.get()));
            if request["type"] == "ocr" {
                Ok(
                    json!({"reader":r,"text":if at.get()==27 {earlier.to_owned()} else {format!("{ANCHOR} {NOVEL}")}}),
                )
            } else {
                Ok(r)
            }
        };
        let (_, text, offset) = locate_continuation_with(command, || true, &b).unwrap();
        assert_eq!(turns.get(), 1);
        assert_eq!(text[offset..].trim(), NOVEL);
    }
    #[test]
    fn reflow_capture_preserves_a_new_chapter_boundary_after_the_anchor() {
        let mut b = resume_fixture();
        let old = b.checkpoints[0].id.clone();
        let count = b.sentences.len();
        let raw = format!("{ANCHOR}\nChapter Five\n{NOVEL}");
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let offset = continuation_offset(&b, &normalized).unwrap().unwrap();
        capture_from(&mut b, &reader(26, "wide26"), &raw, offset).unwrap();
        assert_eq!(b.checkpoints[0].id, old);
        assert_eq!(b.checkpoints.len(), 2);
        assert_eq!(b.checkpoints[1].label, "Chapter Five");
        assert_eq!(b.checkpoints[1].sentence, count);
        assert!(b.checkpoints[1].verified_start);
    }
    #[test]
    fn reflow_search_stops_on_a_repeated_view_and_leaves_cached_audio_unchanged() {
        let b = resume_fixture();
        let before = serde_json::to_value(&b).unwrap();
        let turns = std::cell::Cell::new(0);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                turns.set(turns.get() + 1);
            }
            let r = reader(26, "same-view");
            if request["type"] == "ocr" {
                Ok(
                    json!({"reader":r,"text":"Unrelated contents with no matching sentence context."}),
                )
            } else {
                Ok(r)
            }
        };
        assert!(locate_continuation_with(command, || true, &b).is_err());
        assert_eq!(turns.get(), 1);
        assert_eq!(serde_json::to_value(&b).unwrap(), before);
    }
    #[test]
    fn reflow_guard_rejects_earlier_recorded_text_with_ocr_changes_or_ambiguity() {
        let mut b = resume_fixture();
        capture(&mut b, &reader(29, "later"), NOVEL).unwrap();
        assert!(contains_earlier_context(
            &b,
            &ANCHOR.replace("vegetables", "vegetab1es")
        ));
        assert!(contains_earlier_context(
            &b,
            &ANCHOR.replace("garden", "gardcn")
        ));
        assert!(contains_earlier_context(&b, &ANCHOR.replace("garden.", "")));
        assert!(contains_earlier_context(&b, &format!("{ANCHOR} {ANCHOR}")));
    }
    #[test]
    fn reflow_resume_matches_context_split_across_two_narrow_views() {
        let mut b = book();
        let first="The family walked along the river together while discussing all their plans for the coming summer holidays.";
        let last = "How would anyone know if it were true?";
        capture(&mut b, &reader(28, "old28"), &format!("{first} {last}")).unwrap();
        let at = std::cell::Cell::new(27);
        let turns = std::cell::Cell::new(0);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                assert_eq!(request["direction"], "next");
                at.set(28);
                turns.set(turns.get() + 1);
            }
            let r = reader(at.get(), &format!("narrow{}", at.get()));
            if request["type"] == "ocr" {
                Ok(
                    json!({"reader":r,"text":if at.get()==27 {first.to_owned()} else {format!("{last} {NOVEL}")}}),
                )
            } else {
                Ok(r)
            }
        };
        let (_, text, offset) = locate_continuation_with(command, || true, &b).unwrap();
        assert_eq!(turns.get(), 1);
        assert_eq!(text[offset..].trim(), NOVEL);
    }
    #[test]
    fn retrieval_does_not_treat_new_tail_words_as_ocr_noise() {
        let mut b = book();
        let prefix = "The archivist placed the copper lantern beside a sealed wooden chest in the quiet study. Outside the northern window, travelers waited patiently beneath a spreading oak tree while their horses rested. A messenger delivered a violet envelope containing a detailed map of the abandoned railway station. The village council gathered early that morning to discuss the repairs needed after the storm and carefully record their plans for the summer holidays.";
        capture(&mut b, &reader(28, "old28"), prefix).unwrap();
        let changed = prefix.replace(
            "their plans for the summer holidays.",
            "unexpected visitors arrived during town holidays.",
        );
        assert_eq!(
            continuation_offset(&b, &changed).unwrap(),
            None,
            "New words at the capture boundary must not be swallowed as OCR substitutions"
        );
    }

    #[test]
    fn completion_suffix_without_gap_proceeds_to_join() {
        let b = resume_fixture();
        let text = format!("{ANCHOR} {NOVEL}");
        assert!(
            stage_suffix_gap(&b, &reader(28, "wide"), &text, ANCHOR.len() + 1)
                .unwrap()
                .is_none()
        );
        let raw = format!("\n{ANCHOR}\n\nCHAPTER TWO\n{NOVEL}");
        let normal = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let start = normal.find(NOVEL).unwrap();
        let next = stage_suffix_gap(&b, &reader(28, "wide"), &raw, start)
            .unwrap()
            .unwrap();
        assert_eq!(
            next.pages.last().unwrap().heading.as_deref(),
            Some("CHAPTER TWO")
        );
    }

    #[test]
    fn completion_hint_seek_returns_from_the_cover_before_matching_sentences() {
        for start_at in [0, 97] {
            let mut b = resume_fixture();
            b.reader_hint = Some(ReaderHint {
                progress: "Page 103 of 134".into(),
                fingerprint: "saved103".into(),
            });
            let before = serde_json::to_value(&b).unwrap();
            let at = std::cell::Cell::new(start_at);
            let scans = std::cell::Cell::new(0);
            let command = |request: Value| -> Result<Value, String> {
                if request["type"] == "navigate" {
                    assert_eq!(
                        request["direction"], "next",
                        "Should reach the page hint before reversing"
                    );
                    at.set(at.get() + 1);
                }
                let mut r = reader(at.get(), &format!("view{}", at.get()));
                if at.get() == 0 {
                    r["progress"] = json!("Location 1 of 1583 ● 0%");
                    r["startOfBook"] = json!(true);
                }
                if request["type"] == "ocr" {
                    scans.set(scans.get() + 1);
                    assert!(at.get() >= 97, "Do not OCR distant captured views");
                    Ok(
                        json!({"reader":r,"text":if at.get()==103 {format!("{ANCHOR} {NOVEL}")}else{"Unrelated passage.".into()}}),
                    )
                } else {
                    Ok(r)
                }
            };
            let (found, raw, offset) =
                locate_continuation_with_limit(command, || true, &b, 10_000).unwrap();
            assert_eq!(found["progress"], "Page 103");
            assert_eq!(raw[offset..].trim(), NOVEL);
            assert!(scans.get() <= 7);
            assert_eq!(serde_json::to_value(&b).unwrap(), before);
        }
    }
    #[test]
    fn completion_hint_seek_scans_the_crossing_view_instead_of_reversing() {
        let b = resume_fixture();
        let at = std::cell::Cell::new(1);
        let turns = std::cell::Cell::new(0);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                assert_eq!(request["direction"], "next");
                turns.set(turns.get() + 1);
                at.set(40);
            }
            let r = reader(at.get(), &format!("p{}", at.get()));
            if request["type"] == "ocr" {
                Ok(json!({"reader":r,"text":format!("{ANCHOR} {NOVEL}")}))
            } else {
                Ok(r)
            }
        };
        let (_, text, offset) = locate_continuation_with_limit(command, || true, &b, 100).unwrap();
        assert_eq!(text[offset..].trim(), NOVEL);
        assert_eq!(turns.get(), 1);
    }
    #[test]
    fn completion_hint_seek_avoids_ocr_on_distant_intervening_views() {
        let b = resume_fixture();
        let at = std::cell::Cell::new(1);
        let scans = std::cell::Cell::new(0);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                at.set(at.get() + 3);
            }
            let r = reader(at.get(), &format!("p{}", at.get()));
            if request["type"] == "ocr" {
                scans.set(scans.get() + 1);
                assert!(at.get() >= 22);
                Ok(json!({"reader":r,"text":format!("{ANCHOR} {NOVEL}")}))
            } else {
                Ok(r)
            }
        };
        assert!(locate_continuation_with_limit(command, || true, &b, 100).is_ok());
        assert_eq!(scans.get(), 1);
    }

    #[test]
    fn retrieval_earlier_guard_is_bounded_for_a_large_history() {
        let mut b = book();
        for number in 0..1500 {
            b.sentences.push(Sentence {id:number.to_string(),continuation_of:None,text:format!("The archivist catalogued artifact number {number} beside the northern entrance while recording the details in her notebook."),sources:vec![],start_sample:0,end_sample:0,voice_key:String::new(),voice_label:String::new()});
        }
        let start = std::time::Instant::now();
        assert!(!contains_earlier_context(&b,"The archivist opened the garden door and watched sunlight spread across the empty courtyard. Several unfamiliar travelers waited quietly by a stone fountain while distant bells announced the start of another day."));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn retrieval_resume_uses_broader_sentence_context_when_the_tail_has_multiple_ocr_errors() {
        let mut b = book();
        let context = "The archivist placed the copper lantern beside a sealed wooden chest in the quiet study. Outside the northern window, travelers waited patiently beneath a spreading oak tree while their horses rested. A messenger delivered a violet envelope containing a detailed map of the abandoned railway station. She tried to understand it, but trusting someone's word about hearing from God was difficult to accept. How would anyone know if it were true?";
        capture(&mut b, &reader(28, "old28"), context).unwrap();
        let changed = context
            .replace("trusting", "trustlng")
            .replace("hearing", "hearlng")
            .replace("from God", "frorn God")
            .replace("it were", "itwere");
        let text = format!("Unrelated introductory material. {changed} {NOVEL}");
        let offset = continuation_offset(&b, &text)
            .unwrap()
            .expect("Several sentences should corroborate the noisy tail");
        assert_eq!(text[offset..].trim(), NOVEL);
        assert!(continuation_offset(&b, &format!("{changed} {changed}")).is_err());
    }

    #[test]
    fn completion_finds_saved_later_audio_inside_a_reflowed_page() {
        let mut b = resume_fixture();
        let mut later = book();
        later.from_beginning = false;
        capture(&mut later, &reader(40, "old40"), ANCHOR).unwrap();
        b.saved_ranges.push(Box::new(later));
        let text = format!(
            "{NOVEL} {} Additional following passage.",
            ANCHOR.replace("vegetables", "vegetab1es")
        );
        assert!(
            cached_suffix(&b, &reader(39, "wide39"), &text).is_some(),
            "Saved ranges must be located by passage, not whole-page equality"
        );
    }
    #[test]
    fn reflow_resume_handles_observed_ocr_join_and_letter_errors() {
        let mut b = book();
        let anchor = "She tried to understand it, but trusting someone's word about hearing from God was difficult to accept. How would anyone know if it were true?";
        capture(&mut b, &reader(28, "old28"), anchor).unwrap();
        let changed = anchor.replace("from", "frorn").replace("it were", "itwere");
        let text = format!("Earlier text. {changed} {NOVEL}");
        let offset = continuation_offset(&b, &text)
            .unwrap()
            .expect("Observed OCR changes should match the saved passage");
        assert_eq!(text[offset..].trim(), NOVEL);
        let duplicate = format!("{changed} {changed} {NOVEL}");
        assert!(continuation_offset(&b, &duplicate).is_err());
        let original = b.sentences.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        capture_from(&mut b, &reader(26, "wide26"), &text, offset).unwrap();
        assert_eq!(
            b.sentences[..original.len()]
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(b.sentences.last().unwrap().text, NOVEL);
    }
    #[test]
    fn reflow_resume_and_duplicate_guard_allow_joined_words_in_the_context_prefix() {
        let mut b = book();
        let anchor = ANCHOR.replace("along the river", "along a river");
        capture(&mut b, &reader(28, "old28"), &anchor).unwrap();
        let changed = anchor.replace("The family walked", "Thefamily walkcd");
        let text = format!("{changed} {NOVEL}");
        let offset = continuation_offset(&b, &text)
            .unwrap()
            .expect("Prefix joins must use the same edit budget as full matching");
        assert_eq!(text[offset..].trim(), NOVEL);
        capture(&mut b, &reader(29, "later"), NOVEL).unwrap();
        assert!(contains_earlier_context(&b, &changed));
    }
    #[test]
    fn reflow_resume_does_not_consume_novel_words_when_the_saved_tail_is_missing() {
        let b = resume_fixture();
        let changed = ANCHOR.replace("their garden.", "");
        assert_eq!(
            continuation_offset(&b, &format!("{changed} {NOVEL}")).unwrap(),
            None
        );
    }
    #[test]
    fn reflow_resume_cancellation_after_ocr_prevents_navigation() {
        let b = resume_fixture();
        let active = std::cell::Cell::new(true);
        let command = |request: Value| -> Result<Value, String> {
            assert_ne!(request["type"], "navigate");
            let r = reader(26, "wide26");
            if request["type"] == "ocr" {
                active.set(false);
                Ok(json!({"reader":r,"text":format!("{ANCHOR} {NOVEL}")}))
            } else {
                Ok(r)
            }
        };
        assert_eq!(
            locate_continuation_with(command, || active.get(), &b).unwrap_err(),
            "Preparation stopped"
        );
    }
    #[test]
    fn reflow_resume_maps_utf8_offsets_and_overlapping_adjacent_views() {
        let mut b = book();
        let first =
            "Élodie carefully packed her belongings before leaving the house early in the morning.";
        let shared = "Her sister stood beside the open gate";
        let end = "and waved goodbye as she disappeared down the road.";
        capture(
            &mut b,
            &reader(28, "old28"),
            &format!("{first} {shared} {end}"),
        )
        .unwrap();
        let at = std::cell::Cell::new(27);
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                assert_eq!(request["direction"], "next");
                at.set(28);
            }
            let r = reader(at.get(), &format!("narrow{}", at.get()));
            if request["type"] == "ocr" {
                Ok(
                    json!({"reader":r,"text":if at.get()==27 {format!("{first} {shared}")} else {format!("{shared} {end} {NOVEL}")}}),
                )
            } else {
                Ok(r)
            }
        };
        let (r, raw, offset) = locate_continuation_with(command, || true, &b).unwrap();
        assert_eq!(raw[offset..].trim(), NOVEL);
        capture_from(&mut b, &r, &raw, offset).unwrap();
        let reopened: Book = serde_json::from_value(serde_json::to_value(&b).unwrap()).unwrap();
        assert_eq!(reopened.reader_hint.unwrap().progress, "Page 28");
        assert_eq!(b.sentences.last().unwrap().text, NOVEL);
    }

    #[test]
    fn sentences_cross_pages_without_losing_source_offsets() {
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "Dr. Smith paid 3.50 dollars. A sentence continues",
        )
        .unwrap();
        assert_eq!(b.sentences.len(), 1);
        capture(
            &mut b,
            &json!({"fingerprint":"b","progress":"Page 2"}),
            "on the next page. Held tail",
        )
        .unwrap();
        assert_eq!(
            b.sentences[1].text,
            "A sentence continues on the next page."
        );
        assert_eq!(b.sentences[1].sources.len(), 2);
        assert_eq!(b.pending[0].text, "Held tail");
    }
    #[test]
    fn interrupted_audio_append_recovers_only_committed_sentences() {
        let dir = std::env::temp_dir().join(format!("texthalo-book-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "First. Second.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.25; 24]).unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(dir.join("audio.wav"))
            .unwrap();
        f.write_all(&[9; 80]).unwrap();
        drop(f);
        recover(&dir, &b).unwrap();
        assert_eq!(fs::metadata(dir.join("audio.wav")).unwrap().len(), 44 + 48);
        commit(&dir, &mut b, &[0.5; 48]).unwrap();
        assert_eq!(b.generated, 2);
        assert_eq!(b.sentences[1].start_sample, 24);
        assert_eq!(b.committed_samples, 72);
        let saved: Book =
            serde_json::from_slice(&fs::read(dir.join("book.json")).unwrap()).unwrap();
        assert_eq!(saved.committed_samples, 72);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_manifest_write_rolls_back_the_audio_append() {
        let dir = std::env::temp_dir().join(format!("texthalo-book-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "First. Second.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        fs::create_dir(dir.join("book.tmp")).unwrap();
        assert!(commit(&dir, &mut b, &[0.5; 48]).is_err());
        assert_eq!(b.generated, 0);
        assert_eq!(fs::metadata(dir.join("audio.wav")).unwrap().len(), 44);
        let saved: Book =
            serde_json::from_slice(&fs::read(dir.join("book.json")).unwrap()).unwrap();
        assert_eq!(saved.generated, 0);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn chapter_checkpoint_becomes_playable_only_after_its_sentence_commit() {
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 1"}),
            "CHAPTER ONE\nThe first sentence.",
        )
        .unwrap();
        assert_eq!(b.checkpoints[0].label, "CHAPTER ONE");
        assert!(b.view()["checkpoints"][0]["seconds"].is_null());
        b.generated = 1;
        b.sentences[0].end_sample = 24000;
        b.committed_samples = 24000;
        assert_eq!(b.view()["checkpoints"][0]["seconds"], 0.0);
        assert_eq!(b.sentences[0].sources[0].page, 0);
    }
    #[test]
    fn repeated_page_and_uncertain_chapter_boundary_are_rejected() {
        let mut b = book();
        let r = json!({"fingerprint":"a","progress":"Page 1"});
        capture(&mut b, &r, "A pending thought").unwrap();
        assert!(capture(&mut b, &r, "A pending thought").is_err());
        assert!(capture(
            &mut b,
            &json!({"fingerprint":"b","progress":"Page 2"}),
            "CHAPTER TWO\nA new thought."
        )
        .is_err());
    }
}
