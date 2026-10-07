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
    #[serde(default = "default_audio")]
    audio_file: String,
    #[serde(default)]
    backfill: Option<Box<Backfill>>,
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
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Backfill {
    pages: Vec<Page>,
    verified: bool,
    from_beginning: bool,
    label: Option<String>,
    blocked: Option<String>,
    prefix: Option<Box<Book>>,
    #[serde(default)]
    seed: Option<Box<Book>>,
}
impl Book {
    fn seconds(&self) -> f64 {
        self.committed_samples as f64 / RATE as f64
    }
    fn view(&self) -> Value {
        let at = (self.position * RATE as f64) as u64;
        let current = self
            .sentences
            .iter()
            .take(self.generated)
            .find(|s| s.end_sample > at);
        let backfill_progress = self.backfill.as_ref().map(|b| {
            let prefix = b.prefix.as_ref();
            json!({
                "chapter": b.label,
                "phase": if !b.verified { "searching" } else if prefix.is_some_and(|p| p.generated == p.sentences.len()) { "publishing" } else { "rendering" },
                "pages": b.pages.len(),
                "ready": prefix.map_or(0, |p| p.generated),
                "total": prefix.map(|p| p.sentences.len()),
            })
        });
        let mut checkpoints = std::iter::once(self)
            .chain(self.saved_ranges.iter().map(|b| b.as_ref()))
            .flat_map(|b| {
                b.checkpoints
                    .iter()
                    .enumerate()
                    .map(move |(i, c)| chapter_view(b, i, c))
            })
            .collect::<Vec<_>>();
        checkpoints.sort_by_key(|c| c["page"].as_str().and_then(page_number).unwrap_or(u64::MAX));
        json!({"backfillProgress":backfill_progress,"id":self.id,"key":self.key,"title":self.title,"platform":"kindle","pages":self.pages.len(),"generated":self.generated,"seconds":self.seconds(),"totalPages":self.pages.len()+self.saved_ranges.iter().map(|b|b.pages.len()).sum::<usize>(),"totalGenerated":self.generated+self.saved_ranges.iter().map(|b|b.generated).sum::<usize>(),"totalSeconds":self.seconds()+self.saved_ranges.iter().map(|b|b.seconds()).sum::<f64>(),"position":self.position,"speed":self.speed,"complete":self.complete,"fromBeginning":self.from_beginning,"message":self.message,"activity":self.activity,"backfillBlocked":self.backfill.as_ref().and_then(|b|b.blocked.as_ref()),
            "resume": resume(self), "pendingPage": self.pending_start.as_ref().and_then(|f| self.pages.iter().find(|p| &p.fingerprint == f)).map(|p| &p.progress),
            "checkpoints": checkpoints,
            "passage":current.map(|s|json!({"text":s.text,"page":s.sources.first().and_then(|p|self.pages.get(p.page)).map(|p|&p.progress)}))})
    }
}
fn resume(book: &Book) -> Option<Value> {
    let at = (book.position * RATE as f64) as u64;
    let (index, sentence) = book
        .sentences
        .iter()
        .take(book.generated)
        .enumerate()
        .find(|(_, s)| s.end_sample > at)
        .or_else(|| {
            book.generated
                .checked_sub(1)
                .and_then(|i| book.sentences.get(i).map(|s| (i, s)))
        })?;
    let page = book.pages.get(sentence.sources.first()?.page)?;
    let chapter = book.checkpoints.iter().rev().find(|c| c.sentence <= index);
    Some(json!({"page":page.progress,"chapter":chapter.map(|c|&c.label)}))
}
fn different_page(book: &Book, reader: &Value) -> bool {
    if book.activity.as_deref() == Some("backfill") {
        return false;
    }
    let saved = resume(book).and_then(|r| r["page"].as_str().map(str::to_owned));
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
fn select_page(
    dir: &Path,
    book: &mut Book,
    reader: &Value,
    text: &str,
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
    if let Some((range, index, saved)) = found {
        if saved != normalized {
            return Err("Reader layout or saved passage changed. Restore the original layout before retrying.".into());
        }
        activate_range(&mut next, &range)?;
        next.pending_start = Some(next.pages[index].fingerprint.clone());
    } else {
        let mut ranges = std::mem::take(&mut next.saved_ranges);
        next.activity = None;
        ranges.push(Box::new(next.clone()));
        next.saved_ranges = ranges;
        next.range_id = identity();
        next.audio_file = format!("{}.wav", identity());
        next.backfill = None;
        next.reader_hint = None;
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
fn backfill_directory(dir: &Path, book: &Book) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(&book.range_id).map_err(|_| "Invalid passage identity")?;
    Ok(dir.join("ranges").join(&book.range_id).join("backfill"))
}
struct Playing {
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
fn chapter_view(book: &Book, index: usize, c: &Checkpoint) -> Value {
    let end = book
        .checkpoints
        .get(index + 1)
        .map_or(book.sentences.len(), |n| n.sentence);
    let closed = index + 1 < book.checkpoints.len() || book.end_verified;
    let complete = c.verified_start
        && closed
        && end <= book.generated
        && (index + 1 < book.checkpoints.len() || book.pending.is_empty());
    let duration = book
        .sentences
        .iter()
        .skip(c.sentence)
        .take(end.saturating_sub(c.sentence))
        .filter(|s| s.end_sample > s.start_sample)
        .map(|s| s.end_sample - s.start_sample)
        .sum::<u64>() as f64
        / RATE as f64;
    json!({"id":c.id,"rangeId":book.range_id,"label":c.label,"sentence":c.sentence,"page":book.pages.get(c.page).map(|p|&p.progress),"seconds":book.sentences.get(c.sentence).filter(|_|c.sentence<book.generated).map(|s|s.start_sample as f64/RATE as f64),"duration":duration,"complete":complete,"status":if complete{"complete"}else if book.message.is_some(){"attention"}else if c.sentence<book.generated{"partial"}else{"preparing"},"needsEarlier":!c.verified_start})
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
    let mut next = book.clone();
    let sentence = next
        .sentences
        .get_mut(next.generated)
        .ok_or("No pending sentence")?;
    sentence.start_sample = next.committed_samples;
    sentence.end_sample = next.committed_samples + samples.len() as u64;
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
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|s| ((s.clamp(-1., 1.) * 32767.) as i16).to_le_bytes())
            .collect();
        f.write_all(&bytes)
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
fn publish_prefix(dir: &Path, book: &mut Book, prefix: &Book) -> Result<(), String> {
    if prefix.generated != prefix.sentences.len() || !prefix.pending.is_empty() {
        return Err("Earlier chapter audio is not fully prepared".into());
    }
    if prefix.pages.is_empty() {
        return Err("No earlier pages were captured".into());
    }
    let mut next = book.clone();
    remember_position(&mut next);
    let added = prefix.committed_samples;
    let page_count = prefix.pages.len();
    let sentence_count = prefix.sentences.len();
    for sentence in &mut next.sentences {
        sentence.start_sample += if sentence.end_sample > 0 { added } else { 0 };
        sentence.end_sample += if sentence.end_sample > 0 { added } else { 0 };
        for source in &mut sentence.sources {
            source.page += page_count;
        }
    }
    for source in &mut next.pending {
        source.page += page_count;
    }
    for checkpoint in &mut next.checkpoints {
        checkpoint.sentence += sentence_count;
        checkpoint.page += page_count;
    }
    let mut checkpoints = prefix.checkpoints.clone();
    if next.checkpoints.first().is_some_and(|c| !c.verified_start) {
        let previous = next.checkpoints.remove(0);
        if book.backfill.as_ref().is_none_or(|b| b.seed.is_none()) {
            if let Some(last) = checkpoints.last_mut() {
                last.id = previous.id;
            }
        }
    }
    next.checkpoints.splice(0..0, checkpoints);
    if let Some(seed) = book.backfill.as_ref().and_then(|b| b.seed.as_ref()) {
        next.saved_ranges.retain(|r| r.range_id != seed.range_id);
    }
    next.pages.splice(0..0, prefix.pages.clone());
    next.sentences.splice(0..0, prefix.sentences.clone());
    next.generated += sentence_count;
    next.committed_samples += added;
    next.from_beginning = prefix.from_beginning;
    next.complete = next.from_beginning
        && next.saved_ranges.is_empty()
        && next.checkpoints.iter().all(|c| c.verified_start)
        && next.end_verified
        && next.generated == next.sentences.len()
        && next.pending.is_empty();
    next.message = None;
    next.position = stable_position(&next).unwrap_or(book.position + added as f64 / RATE as f64);
    next.backfill = None;
    next.audio_file = format!("{}.wav", identity());
    next.activity = None;
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
        for (path, samples) in [
            (audio_path(&backfill_directory(dir, book)?, prefix)?, added),
            (audio_path(dir, book)?, book.committed_samples),
        ] {
            let mut input = fs::File::open(path).map_err(|e| e.to_string())?;
            input.seek(SeekFrom::Start(44)).map_err(|e| e.to_string())?;
            let count = std::io::copy(&mut input.take(samples * 2), &mut output)
                .map_err(|e| e.to_string())?;
            if count != samples * 2 {
                return Err("Recording is shorter than its committed audio".into());
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
        let _ = fs::remove_file(&target);
        result.and(Err("Earlier audio was not published".into()))
    }
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
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let number = reader["progress"].as_str().and_then(page_number);
    let mut found = None;
    for range in &book.saved_ranges {
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
    next.complete = next.from_beginning
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
fn chapter(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim();
    let lower = line.to_lowercase();
    ((lower == "prologue"
        || lower == "epilogue"
        || lower.starts_with("chapter ")
        || lower.starts_with("part "))
        && line.len() < 100)
        .then(|| line.to_owned())
}
#[path = "audiobooks/matching.rs"]
mod matching;
use matching::{
    contains_earlier_context, continuation_offset, head_offset, page_matches, text_words,
};
fn locate_continuation_with(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
) -> Result<(Value, String, usize), String> {
    let hint = book
        .reader_hint
        .as_ref()
        .and_then(|h| page_number(&h.progress))
        .or_else(|| book.pages.last().and_then(|p| page_number(&p.progress)));
    let mut reader = command(json!({"type":"inspect"}))?;
    let mut visited = std::collections::HashSet::new();
    let mut previous_forward_view: Option<String> = None;
    for _ in 0..12 {
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
        if !visited.insert(fingerprint.clone()) {
            break;
        }
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
        if let Some(offset) = continuation_offset(book, &normalized)? {
            return Ok((result["reader"].clone(), raw.into(), offset));
        }
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
                        return Ok((result["reader"].clone(), raw.into(), end - prefix_length));
                    }
                }
            }
        }
        let direction = if contains_earlier_context(book, &normalized) {
            "next"
        } else if match (reader["progress"].as_str().and_then(page_number), hint) {
            (Some(at), Some(goal)) => at > goal,
            _ => false,
        } {
            "previous"
        } else {
            "next"
        };
        if reader[if direction == "next" {
            "endOfBook"
        } else {
            "startOfBook"
        }] == true
        {
            break;
        }
        previous_forward_view = (direction == "next").then_some(normalized);
        reader = command(
            json!({"type":"navigate","bookId":reader["bookId"],"fingerprint":reader["fingerprint"],"direction":direction}),
        )?;
    }
    Err("Could not confidently locate your saved sentence in nearby views. Your saved audio is available; locate your saved position or choose Listen from this page.".into())
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
            });
        }
        offset = end;
    }
    let pending = slice(used, joined.len());
    book.sentences.extend(ready);
    book.pending = pending;
    Ok(())
}
fn capture(book: &mut Book, reader: &Value, text: &str) -> Result<(), String> {
    capture_from(book, reader, text, 0)
}
fn capture_from(book: &mut Book, reader: &Value, text: &str, offset: usize) -> Result<(), String> {
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
    if book
        .pages
        .iter()
        .any(|p| p.fingerprint == fingerprint || (!normal.is_empty() && p.text == normal))
    {
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
                            if b.schema_version < 3 {
                                // Persist the new identity before moving its old backfill folder.
                                save(&dir, &b)?;
                                let old = dir.join("backfill");
                                let new = backfill_directory(&dir, &b)?;
                                if old.exists() && !new.exists() {
                                    fs::create_dir_all(new.parent().unwrap())
                                        .map_err(|e| e.to_string())?;
                                    fs::rename(old, new).map_err(|e| e.to_string())?;
                                }
                                b.schema_version = 3;
                                save(&dir, &b)?;
                            }
                            for range in &b.saved_ranges {
                                recover(&dir, range)?;
                            }
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
            let snapshot = p.player.snapshot();
            let current = app
                .state::<crate::AppState>()
                .job
                .lock()
                .unwrap()
                .is_current(p.job);
            if let Some(book) = l.books.get_mut(&id) {
                book.position = snapshot.position.min(book.seconds());
                remember_position(book);
                book.speed = snapshot.speed;
                if let Err(e) = directory(&id).and_then(|d| save_bookmark(&d, book)) {
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
                book.position = snapshot.position.min(book.seconds());
                book.speed = snapshot.speed;
                remember_position(book);
                if let Err(error) = directory(&book.id).and_then(|dir| save_bookmark(&dir, book)) {
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
        if reader["connected"] != true {
            return None;
        }
        let r = &reader["reader"];
        let k = key(r).ok()?;
        let l = self.0.lock().unwrap();
        if l.dismissed.as_deref() == Some(&k) {
            return None;
        }
        let book = l.books.values().find(|b| b.key == k);
        Some(
            json!({"reader":r,"differentPage":book.is_some_and(|b|different_page(b,r)),"book":book.map(Book::view),"generating":l.producing.as_ref().is_some_and(|(id,_,_)|book.is_some_and(|b|&b.id==id))}),
        )
    }
    fn snapshot(&self, app: &AppHandle) -> Value {
        let reader = app.state::<crate::reader_bridge::ReaderBridge>().status();
        let tray = self.tray(&reader);
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
            .map(|p| json!({"bookId":p.book,"audio":p.player.snapshot()}));
        json!({"books":l.books.values().map(Book::view).collect::<Vec<_>>(),"tray":tray,"generating":l.producing.as_ref().map(|(id,_,_)|id),"playback":playback})
    }
}
#[tauri::command]
pub fn audiobook_state(app: AppHandle) -> Value {
    app.state::<Audiobooks>().snapshot(&app)
}
fn play(app: &AppHandle, id: &str, position: Option<f64>) -> Result<(), String> {
    play_selected(app, id, position, None)
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
) -> Result<(), String> {
    let service = app.state::<Audiobooks>();
    let mut l = service.0.lock().unwrap();
    check_selection(&l, revision)?;
    if l.transitioning {
        return Err("A passage is being selected. Try again in a moment.".into());
    }
    let b = l.books.get(id).ok_or("Audiobook not found")?;
    if b.pending_start.is_some() {
        return Err("Prepare the selected page before listening.".into());
    }
    if b.generated == 0 {
        return Err("Preparing the first sentence…".into());
    }
    let seconds = position.unwrap_or(if b.complete && b.position >= b.seconds() - 0.05 {
        0.0
    } else {
        stable_position(b).unwrap_or(b.position)
    });
    if !seconds.is_finite() || seconds < 0. || seconds > b.seconds() {
        return Err("Choose a position within generated audio".into());
    }
    let samples = b.committed_samples;
    let speed = b.speed;
    let finished = !l.producing.as_ref().is_some_and(|(i, _, _)| i == id);
    let audio = audio_path(&directory(id)?, b)?;
    let job = crate::begin_speech(app, crate::Phase::Preparing);
    let state = app.state::<crate::AppState>();
    let mut active = state.job.lock().unwrap();
    if !active.is_current(job) {
        return Err("Playback superseded".into());
    }
    let player = Arc::new(crate::pcm::Player::new()?);
    player.update_file(&audio, samples, finished)?;
    player.start()?;
    state.spoken.activate_pcm(player.clone());
    player.seek(seconds)?;
    player.set_speed(speed)?;
    active.set(crate::Phase::Speaking, None, None);
    let _ = app.emit("kiegen:status", &active.status);
    drop(active);
    l.playing = Some(Playing {
        book: id.into(),
        job,
        player,
    });
    if let Some(b) = l.books.get_mut(id) {
        b.position = seconds;
        remember_position(b);
        save_bookmark(&directory(id)?, b)?;
    }
    Ok(())
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
        let position = select_page(&directory(&book_id)?, book, &result["reader"], text)?;
        Ok::<_, String>((position, _transition.revision))
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(seconds) = position {
        launch_playback(app, id.clone(), Some(seconds), revision).await?;
    } else {
        let book_id = id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            generate_mode(&app, Some(book_id), false, false, Some(revision))
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
    let range_id = range_id.or_else(|| {
        if seconds.is_none() {
            return None;
        }
        let service = app.state::<Audiobooks>();
        let l = service.0.lock().unwrap();
        l.books
            .get(&id)
            .filter(|b| b.pending_start.is_some())
            .map(|b| b.range_id.clone())
    });
    let revision = if let Some(range) = range_id {
        switch_range(&app, &id, &range).await?
    } else {
        app.state::<Audiobooks>()
            .0
            .lock()
            .unwrap()
            .selection_revision
    };
    launch_playback(app, id, seconds, revision).await
}
async fn launch_playback(
    app: AppHandle,
    id: String,
    seconds: Option<f64>,
    revision: u64,
) -> Result<(), String> {
    let handle = app.clone();
    let book_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        play_selected(&handle, &book_id, seconds, Some(revision))
    })
    .await
    .map_err(|e| e.to_string())??;
    // Continue resumes page preparation too, when this book is still connected.
    // Cached playback remains usable without the browser or native helper.
    let reader = app.state::<crate::reader_bridge::ReaderBridge>().status();
    let should_buffer = {
        let service = app.state::<Audiobooks>();
        let l = service.0.lock().unwrap();
        !l.worker
            && reader["connected"] == true
            && l.books.get(&id).is_some_and(|b| {
                !b.complete && key(&reader["reader"]).ok().as_deref() == Some(&b.key)
            })
    };
    if should_buffer {
        let handle = app.clone();
        let book_id = id.clone();
        let preparation = tauri::async_runtime::spawn_blocking(move || {
            generate_mode(&handle, Some(book_id), false, false, Some(revision))
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
    r: &Value,
    direction: &str,
) -> Result<Value, String> {
    bridge.command(json!({"type":"navigate","bookId":r["bookId"],"fingerprint":r["fingerprint"],"direction":direction}))
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
enum RestorePolicy<'a> {
    Resume,
    Borrow(&'a mut Value),
    Return(&'a mut Value),
}
fn restore_page(
    bridge: &crate::reader_bridge::ReaderBridge,
    service: &Audiobooks,
    id: &str,
    token: &str,
    book: &Book,
    target: &Page,
    policy: RestorePolicy<'_>,
) -> Result<Value, String> {
    restore_page_with(
        |request| bridge.command(request),
        || generation_active(service, id, token),
        book,
        target,
        policy,
    )
}
fn restore_page_with(
    command: impl Fn(Value) -> Result<Value, String>,
    active: impl Fn() -> bool,
    book: &Book,
    target: &Page,
    policy: RestorePolicy<'_>,
) -> Result<Value, String> {
    let finishing = matches!(&policy, RestorePolicy::Return(_));
    let mut tracker = match policy {
        RestorePolicy::Resume => None,
        RestorePolicy::Borrow(r) | RestorePolicy::Return(r) => Some(r),
    };
    let goal = page_number(&target.progress)
        .ok_or("A verified page number is required to fill earlier pages")?;
    let mut r = command(json!({"type":"inspect"}))?;
    if key(&r)? != book.key {
        return Err("Connected book changed".into());
    }
    if tracker
        .as_ref()
        .is_some_and(|owned| owned["fingerprint"] != r["fingerprint"])
    {
        return Err(
            "Reader moved while preparing; the app left your selected page unchanged".into(),
        );
    }
    let initial = page_number(r["progress"].as_str().unwrap_or(""))
        .ok_or("Reader page number unavailable")?;
    let direction = if initial < goal { "next" } else { "previous" };
    let mut visited = std::collections::HashSet::new();
    let mut candidate = false;
    for _ in 0..100 {
        if !finishing && !active() {
            return Err("Preparation stopped".into());
        }
        let at = page_number(r["progress"].as_str().unwrap_or(""))
            .ok_or("Reader page number unavailable")?;
        // Captured Kindle views can repeat or skip page numbers; labels are not unique identities.
        // Never reverse to chase a number skipped by a turn; verify the passage at the crossing.
        if r["fingerprint"].as_str() == Some(target.fingerprint.as_str())
            || at == goal
            || (direction == "next" && at > goal)
            || (direction == "previous" && at < goal)
        {
            candidate = true;
            break;
        }
        if !visited.insert((r["progress"].to_string(), r["fingerprint"].to_string())) {
            return Err("Reader navigation repeated a page. Restore the saved passage or choose Listen from this page.".into());
        }
        r = command(
            json!({"type":"navigate", "bookId":r["bookId"], "fingerprint":r["fingerprint"], "direction":direction}),
        )?;
        if let Some(owned) = tracker.as_deref_mut() {
            *owned = r.clone();
        }
    }
    if !candidate {
        return Err("Open the saved page manually to continue".into());
    }
    if !finishing && !active() {
        return Err("Preparation stopped".into());
    }
    let result =
        command(json!({"type":"ocr","bookId":r["bookId"],"fingerprint":r["fingerprint"]}))?;
    if let Some(owned) = tracker.as_deref_mut() {
        *owned = result["reader"].clone();
    }
    let text = result["text"]
        .as_str()
        .ok_or("No text returned for the saved page")?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !page_matches(&target.text, &text)? {
        return Err(
            "Reader layout or saved passage changed. Restore the original layout before retrying."
                .into(),
        );
    }
    Ok(result["reader"].clone())
}
fn prefix_book(book: &Book, backfill: &Backfill) -> Result<Book, String> {
    let mut prefix = backfill.seed.as_deref().unwrap_or(book).clone();
    prefix.id = identity();
    prefix.audio_file = default_audio();
    prefix.backfill = None;
    prefix.saved_ranges.clear();
    prefix.pending_start = None;
    prefix.reader_hint = None;
    if backfill.seed.is_none() {
        prefix.pages.clear();
        prefix.sentences.clear();
        prefix.checkpoints.clear();
        prefix.pending.clear();
        prefix.generated = 0;
        prefix.committed_samples = 0;
    }
    prefix.position = 0.;
    prefix.bookmark_sentence = None;
    prefix.bookmark_offset = 0;
    prefix.complete = false;
    prefix.end_verified = false;
    prefix.from_beginning = backfill.from_beginning;
    for page in backfill.pages.iter().rev() {
        // Raw OCR headings were preserved separately before whitespace normalization.
        let text = if let Some(heading) = &page.heading {
            format!(
                "{}\n{}",
                heading,
                page.text.strip_prefix(heading).unwrap_or(&page.text).trim()
            )
        } else {
            page.text.clone()
        };
        capture(
            &mut prefix,
            &json!({"fingerprint":page.fingerprint,"progress":page.progress}),
            &text,
        )?;
        prefix.pages.last_mut().unwrap().heading = page.heading.clone();
    }
    if !prefix.pending.is_empty() {
        let last_sources = prefix.pending.clone();
        let tail = last_sources
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        prefix.sentences.push(Sentence {
            id: identity(),
            continuation_of: book.sentences.first().map(|s| s.id.clone()),
            text: tail,
            sources: last_sources,
            start_sample: 0,
            end_sample: 0,
        });
        prefix.pending.clear();
    }
    Ok(prefix)
}
fn needs_earlier(book: &Book, full: bool) -> bool {
    if full {
        !book.from_beginning
    } else {
        book.checkpoints.first().is_some_and(|c| !c.verified_start)
    }
}
fn backfill_step(
    app: &AppHandle,
    id: &str,
    token: &str,
    reader: &mut Value,
    settings: &crate::config::Settings,
    full: bool,
) -> Result<bool, String> {
    let service = app.state::<Audiobooks>();
    let result = backfill_step_inner(app, id, token, reader, settings, full);
    if result.is_err() || !generation_active(&service, id, token) {
        let book = service
            .0
            .lock()
            .unwrap()
            .books
            .get(id)
            .cloned()
            .ok_or("Audiobook removed")?;
        if let Some(forward) = book.pages.last() {
            let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
            *reader = restore_page(
                &bridge,
                &service,
                id,
                token,
                &book,
                forward,
                RestorePolicy::Return(reader),
            )
            .map_err(|e| format!("Chapter preparation stopped. {e}"))?;
        }
    }
    if !generation_active(&service, id, token) {
        Ok(false)
    } else {
        result
    }
}
fn backfill_step_inner(
    app: &AppHandle,
    id: &str,
    token: &str,
    reader: &mut Value,
    settings: &crate::config::Settings,
    full: bool,
) -> Result<bool, String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let book = {
        let mut l = service.0.lock().unwrap();
        let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
        if !needs_earlier(b, full) {
            return Ok(false);
        }
        if b.backfill.is_none() {
            b.backfill = Some(Box::default());
        }
        if b.backfill.as_ref().is_some_and(|b| b.blocked.is_some()) {
            return Ok(false);
        }
        b.activity = Some("backfill".into());
        b.clone()
    };
    let state = book.backfill.as_ref().unwrap();
    if !state.verified {
        if state.pages.len() >= if full { 10_000 } else { 100 } {
            return Err("The earlier passage exceeded the reader search limit".into());
        }
        let forward = book.pages.last().ok_or("No imported page")?;
        // Never override manual page changes while borrowing the reader for backfill.
        let current = bridge.command(json!({"type":"inspect"}))?;
        if current["fingerprint"] != reader["fingerprint"] || key(&current)? != book.key {
            return Err("Reader moved while preparing. Retry from the saved layout.".into());
        }
        let target = state.pages.last().unwrap_or(&book.pages[0]);
        let mut at = restore_page(
            &bridge,
            &service,
            id,
            token,
            &book,
            target,
            RestorePolicy::Borrow(reader),
        )?;
        *reader = at.clone();
        if at["startOfBook"] == true {
            let mut l = service.0.lock().unwrap();
            let b = l.books.get_mut(id).unwrap();
            let bf = b.backfill.as_mut().unwrap();
            bf.verified = true;
            bf.from_beginning = true;
            save(&directory(id)?, b)?;
        } else {
            if !generation_active(&service, id, token) {
                return Ok(false);
            }
            at = navigate(&bridge, &at, "previous")?;
            *reader = at.clone();
            if !generation_active(&service, id, token) {
                return Ok(false);
            }
            let result = bridge.command(
                json!({"type":"ocr","bookId":at["bookId"],"fingerprint":at["fingerprint"]}),
            )?;
            at = result["reader"].clone();
            *reader = at.clone();
            let text = result["text"]
                .as_str()
                .ok_or("Earlier page text unavailable")?;
            let normal = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if normal.is_empty() || normal.len() > 20000 {
                return Err("Earlier page has no reliable passage".into());
            }
            let page = Page {
                heading: chapter(text),
                text: normal,
                progress: at["progress"].as_str().unwrap_or("").into(),
                fingerprint: at["fingerprint"]
                    .as_str()
                    .ok_or("Missing page fingerprint")?
                    .into(),
            };
            let mut l = service.0.lock().unwrap();
            let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
            let seed = b
                .saved_ranges
                .iter()
                .find(|r| {
                    r.pages.last().is_some_and(|p| {
                        p.text == page.text
                            && (p.fingerprint == page.fingerprint
                                || page_number(&p.progress).is_some()
                                    && page_number(&p.progress) == page_number(&page.progress))
                    })
                })
                .cloned();
            let bf = b.backfill.as_mut().unwrap();
            if bf
                .pages
                .iter()
                .chain(b.pages.iter())
                .any(|p| p.fingerprint == page.fingerprint)
            {
                return Err("Repeated page while finding chapter start".into());
            }
            if let Some(seed) = seed {
                bf.verified = true;
                bf.from_beginning = seed.from_beginning;
                bf.label = seed.checkpoints.last().map(|c| c.label.clone());
                bf.seed = Some(seed);
            } else {
                bf.verified = (!full && page.heading.is_some()) || at["startOfBook"] == true;
                bf.label = page.heading.clone();
                bf.from_beginning = at["startOfBook"] == true;
                bf.pages.push(page);
            }
            save(&directory(id)?, b)?;
        }
        // Returning after each page makes forward preparation the priority.
        *reader = restore_page(
            &bridge,
            &service,
            id,
            token,
            &book,
            forward,
            RestorePolicy::Borrow(reader),
        )?;
        return Ok(true);
    }
    let prefix_dir = backfill_directory(&directory(id)?, &book)?;
    fs::create_dir_all(&prefix_dir).map_err(|e| e.to_string())?;
    let mut prefix = if let Some(prefix) = &state.prefix {
        (**prefix).clone()
    } else {
        prefix_book(&book, state)?
    };
    if prefix.pages.is_empty() {
        let mut l = service.0.lock().unwrap();
        let b = l.books.get_mut(id).unwrap();
        b.checkpoints[0].verified_start = true;
        b.from_beginning = state.from_beginning;
        b.backfill = None;
        save(&directory(id)?, b)?;
        return Ok(true);
    }
    if let Ok(bytes) = fs::read(prefix_dir.join("book.json")) {
        let saved: Book = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if state.prefix.as_ref().is_some_and(|p| p.id == saved.id) {
            prefix = saved;
        }
    }
    if state.prefix.is_none() {
        if let Some(seed) = &state.seed {
            fs::copy(
                audio_path(&directory(id)?, seed)?,
                audio_path(&prefix_dir, &prefix)?,
            )
            .map_err(|e| e.to_string())?;
        }
        recover(&prefix_dir, &prefix)?;
        save(&prefix_dir, &prefix)?;
        let mut l = service.0.lock().unwrap();
        let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
        b.backfill.as_mut().ok_or("Preparation stopped")?.prefix = Some(Box::new(prefix.clone()));
        save(&directory(id)?, b)?;
    }
    recover(&prefix_dir, &prefix)?;
    save(&prefix_dir, &prefix)?;
    if let Some(sentence) = prefix.sentences.get(prefix.generated).cloned() {
        let scratch = prefix_dir.join("sentence.tmp.wav");
        let app_state = app.state::<crate::AppState>();
        let samples = {
            let _guard = app_state
                .synthesis
                .for_engine(settings.engine)
                .lock()
                .unwrap();
            let result = app_state
                .spoken
                .render(settings, &sentence.text, &scratch)
                .and_then(|_| crate::spoken::Spoken::load_audio(&scratch));
            let _ = fs::remove_file(&scratch);
            result?
        };
        if !generation_active(&service, id, token) {
            return Ok(false);
        }
        commit(&prefix_dir, &mut prefix, &samples)?;
        let mut l = service.0.lock().unwrap();
        let b = l.books.get_mut(id).unwrap();
        b.backfill.as_mut().unwrap().prefix = Some(Box::new(prefix));
        save(&directory(id)?, b)?;
        return Ok(true);
    }
    let mut l = service.0.lock().unwrap();
    if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
        return Ok(false);
    }
    let playing = l
        .playing
        .as_ref()
        .filter(|p| {
            p.book == id
                && app
                    .state::<crate::AppState>()
                    .job
                    .lock()
                    .unwrap()
                    .is_current(p.job)
        })
        .map(|p| p.player.clone());
    let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
    if let Some(p) = &playing {
        b.position = p.snapshot().position.min(b.seconds());
        remember_position(b);
    }
    let added = prefix.committed_samples;
    publish_prefix(&directory(id)?, b, &prefix)?;
    let path = audio_path(&directory(id)?, b)?;
    let samples = b.committed_samples;
    if let Some(p) = playing {
        if let Err(error) = p.update_file_rebased(&path, samples, added) {
            p.stop();
            l.playing = None;
            return Err(error);
        }
    }
    let b = l.books.get(id).ok_or("Audiobook removed")?;
    refresh_chapter_files(&directory(id)?, b)?;
    Ok(true)
}
fn producer(
    app: &AppHandle,
    id: &str,
    token: &str,
    full: bool,
    chapter_only: bool,
    autoplay: bool,
) -> Result<(), String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let book = service
        .0
        .lock()
        .unwrap()
        .books
        .get(id)
        .cloned()
        .ok_or("Audiobook not found")?;
    let mut auto = autoplay;
    if auto && book.generated > 0 && book.pending_start.is_none() {
        play(app, id, None)?;
        auto = false;
    }
    let mut reader = if !book.pages.is_empty() {
        {
            let mut l = service.0.lock().unwrap();
            if let Some(b) = l.books.get_mut(id) {
                b.activity = Some("locating".into());
            }
        }
        let (reader, text, offset) = locate_continuation_with(
            |request| bridge.command(request),
            || generation_active(&service, id, token),
            &book,
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
        let reader = bridge.command(json!({"type":"inspect"}))?;
        if key(&reader)? != book.key {
            return Err("Connected book changed".into());
        }
        reader
    };
    let mut need_next = !book.pages.is_empty();
    let settings = {
        let mut s = app
            .state::<crate::AppState>()
            .settings
            .lock()
            .unwrap()
            .clone();
        s.engine = crate::config::Engine::Kokoro;
        s.kokoro = book.voice.clone();
        s
    };
    loop {
        let (sentence, buffer, active) = {
            let l = service.0.lock().unwrap();
            let b = l.books.get(id).ok_or("Audiobook removed")?;
            let active = l
                .producing
                .as_ref()
                .is_some_and(|(i, t, _)| i == id && t == token);
            (
                b.sentences.get(b.generated).cloned(),
                listening_buffer(b),
                active,
            )
        };
        if !active {
            return Ok(());
        }
        // Finish the entire captured page before pausing the producer.
        if let Some(sentence) = sentence {
            let scratch = directory(id)?.join("sentence.tmp.wav");
            let state = app.state::<crate::AppState>();
            let samples = {
                let _guard = state.synthesis.for_engine(settings.engine).lock().unwrap();
                let result = state
                    .spoken
                    .render(&settings, &sentence.text, &scratch)
                    .and_then(|_| crate::spoken::Spoken::load_audio(&scratch));
                let _ = fs::remove_file(&scratch);
                result?
            };
            let mut l = service.0.lock().unwrap();
            if !l.producing.as_ref().is_some_and(|(_, t, _)| t == token) {
                return Ok(());
            }
            let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
            commit(&directory(id)?, b, &samples)?;
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
            if let Some(p) = l
                .playing
                .as_ref()
                .filter(|p| p.book == id && state.job.lock().unwrap().is_current(p.job))
            {
                p.player.update_file(&audio, committed, false)?;
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
            let b = &l.books[id];
            refresh_chapter_files(&directory(id)?, b)?;
            if chapter_only
                && b.checkpoints
                    .first()
                    .is_some_and(|c| chapter_view(b, 0, c)["complete"] == true)
            {
                return Ok(());
            }
        }
        let earlier = {
            let l = service.0.lock().unwrap();
            let b = &l.books[id];
            b.pending_start.is_none()
                && needs_earlier(b, full)
                && b.backfill.as_ref().is_none_or(|bf| bf.blocked.is_none())
                && (full
                    || buffer >= 120.
                    || reader["endOfBook"] == true
                    || (chapter_only && b.checkpoints.len() > 1))
        };
        if earlier {
            match backfill_step(app, id, token, &mut reader, &settings, full) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    let mut l = service.0.lock().unwrap();
                    let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
                    if needs_earlier(b, full) {
                        b.backfill.get_or_insert_with(Box::default).blocked = Some(error.clone());
                    }
                    return Err(format!("Chapter preparation needs attention: {error}"));
                }
            }
        }
        if !full && !chapter_only && buffer >= 120. {
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
                b.complete = b.from_beginning
                    && b.saved_ranges.is_empty()
                    && b.checkpoints.iter().all(|c| c.verified_start);
                refresh_chapter_files(&directory(id)?, b)?;
                b.message = if b.backfill.as_ref().is_some_and(|bf| bf.blocked.is_some()) {
                    b.message.clone()
                } else if b.complete {
                    None
                } else {
                    Some("Generated through the end, starting from your imported page.".into())
                };
                save(&directory(id)?, b)?;
                if let Some(p) = l.playing.as_ref().filter(|p| p.book == id) {
                    p.player.push(Vec::new(), true);
                }
                return Ok(());
            }
            if !generation_active(&service, id, token) {
                return Ok(());
            }
            reader = navigate(&bridge, &reader, "next")?;
        }
        let result = bridge.command(
            json!({"type":"ocr","bookId":reader["bookId"],"fingerprint":reader["fingerprint"]}),
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
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if start > 0
                && !b.pages.last().is_some_and(|p| {
                    p.fingerprint == reader["fingerprint"].as_str().unwrap_or("")
                        && p.text == normalized[..start].trim()
                })
            {
                let prefix = &normalized[..start];
                let offset = continuation_offset(b, prefix)?.unwrap_or(0);
                if offset == 0 && contains_earlier_context(b, prefix) {
                    return Err(
                        "The gap overlaps recorded text and could not be aligned confidently."
                            .into(),
                    );
                }
                let mut next = b.clone();
                capture_from(&mut next, &reader, prefix, offset)?;
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
            if let Some(p) = l.playing.as_ref().filter(|p| p.book == id) {
                p.player.update_file(
                    &audio_path(&directory(id)?, &merged)?,
                    merged.committed_samples,
                    false,
                )?;
            }
            drop(l);
            let (at, raw, offset) = locate_continuation_with(
                |request| bridge.command(request),
                || generation_active(&service, id, token),
                &merged,
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
    generate_mode(app, id, full, false, None)
}
fn generate_mode(
    app: &AppHandle,
    id: Option<String>,
    full: bool,
    chapter_only: bool,
    revision: Option<u64>,
) -> Result<String, String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let reader = bridge.command(json!({"type":"inspect"}))?;
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
        if book.complete {
            return Err("This audiobook is already complete".into());
        }
    } else {
        let book = Book {
            schema_version: 3,
            range_id: identity(),
            saved_ranges: Vec::new(),
            pending_start: None,
            reader_hint: None,
            audio_file: default_audio(),
            backfill: None,
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
        };
        let dir = directory(&id)?;
        recover(&dir, &book)?;
        save(&dir, &book)?;
        library.books.insert(id.clone(), book);
    }
    let playback = library
        .playing
        .as_ref()
        .filter(|p| p.book == id)
        .map(|p| p.player.snapshot());
    let restart = playback.is_some();
    if let Some(snapshot) = &playback {
        let b = library.books.get_mut(&id).unwrap();
        b.position = snapshot.position.min(b.seconds());
        b.speed = snapshot.speed;
        remember_position(b);
    }
    let autoplay = !full && !chapter_only && !restart;
    let token = uuid::Uuid::new_v4().to_string();
    let book = library.books.get_mut(&id).unwrap();
    book.message = None;
    if book.pending_start.is_some() {
        book.activity = Some("selected".into());
    }
    if let Some(backfill) = &mut book.backfill {
        backfill.blocked = None;
    }
    save(&directory(&id)?, book)?;
    library.worker = true;
    library.producing = Some((id.clone(), token.clone(), full || chapter_only));
    drop(library);
    if restart {
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
        let mut result = producer(&handle, &book_id, &token, full, chapter_only, autoplay);
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

#[tauri::command]
pub async fn audiobook_complete_chapter(app: AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || generate_mode(&app, Some(id), false, true, None))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn book() -> Book {
        Book {
            schema_version: 3,
            range_id: identity(),
            saved_ranges: Vec::new(),
            pending_start: None,
            reader_hint: None,
            audio_file: default_audio(),
            backfill: None,
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
        }
    }
    fn reader(page: u64, fingerprint: &str) -> Value {
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
    fn cached_page_selection_reuses_audio_and_rejects_changed_page_text() {
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
        assert!(select_page(&dir, &mut b, &reader(2, "p2"), "Different layout.").is_err());
        assert_eq!(b.position, 1.);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn filling_a_gap_reuses_the_saved_prefix_and_keeps_sentence_bookmarks() {
        let dir = std::env::temp_dir().join(format!("texthalo-gap-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        capture(&mut b, &reader(1, "p1"), "CHAPTER ONE\nFirst.").unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.25; 24]).unwrap();
        let old_sentence = b.sentences[0].id.clone();
        let chapter_id = b.checkpoints[0].id.clone();
        select_page(&dir, &mut b, &reader(3, "p3"), "Third.").unwrap();
        commit(&dir, &mut b, &[0.75; 24]).unwrap();
        b.pending_start = None;
        b.position = 0.0005;
        remember_position(&mut b);
        let selected_id = b.sentences[0].id.clone();
        let bf = Backfill {
            seed: Some(b.saved_ranges[0].clone()),
            verified: true,
            from_beginning: true,
            pages: vec![Page {
                heading: None,
                progress: "Page 2".into(),
                fingerprint: "p2".into(),
                text: "Second.".into(),
            }],
            ..Default::default()
        };
        let mut prefix = prefix_book(&b, &bf).unwrap();
        assert_eq!(prefix.generated, 1);
        assert_eq!(prefix.sentences[0].id, old_sentence);
        let pd = backfill_directory(&dir, &b).unwrap();
        fs::create_dir_all(&pd).unwrap();
        fs::copy(
            audio_path(&dir, bf.seed.as_ref().unwrap()).unwrap(),
            audio_path(&pd, &prefix).unwrap(),
        )
        .unwrap();
        recover(&pd, &prefix).unwrap();
        save(&pd, &prefix).unwrap();
        commit(&pd, &mut prefix, &[0.5; 24]).unwrap();
        b.backfill = Some(Box::new(bf));
        publish_prefix(&dir, &mut b, &prefix).unwrap();
        assert!(b.saved_ranges.is_empty());
        assert_eq!(b.generated, 3);
        assert_eq!(b.checkpoints.len(), 1);
        assert_eq!(b.checkpoints[0].id, chapter_id);
        assert_eq!(b.sentences[2].id, selected_id);
        assert!((stable_position(&b).unwrap() - 0.0025).abs() < 1e-9);
        let audio = crate::spoken::Spoken::load_audio(&audio_path(&dir, &b).unwrap()).unwrap();
        assert_eq!(audio.len(), 72);
        assert!(audio[0] < audio[24] && audio[24] < audio[48]);
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
    fn backfill_progress_reports_search_rendering_and_publication_separately() {
        let mut b = book();
        b.from_beginning = false;
        capture(
            &mut b,
            &json!({"fingerprint":"a","progress":"Page 13"}),
            "Middle.",
        )
        .unwrap();
        let earlier = Page {
            heading: None,
            progress: "Page 12".into(),
            fingerprint: "b".into(),
            text: "Earlier.".into(),
        };
        b.backfill = Some(Box::new(Backfill {
            pages: vec![earlier],
            ..Default::default()
        }));
        let searching = b.view()["backfillProgress"].clone();
        assert_eq!(searching["phase"], "searching");
        assert_eq!(searching["pages"], 1);
        let mut prefix = book();
        capture(
            &mut prefix,
            &json!({"fingerprint":"c","progress":"Page 11"}),
            "First. Second.",
        )
        .unwrap();
        prefix.generated = 1;
        let bf = b.backfill.as_mut().unwrap();
        bf.verified = true;
        bf.label = Some("Chapter One".into());
        bf.prefix = Some(Box::new(prefix));
        let rendering = b.view()["backfillProgress"].clone();
        assert_eq!(rendering["phase"], "rendering");
        assert_eq!(rendering["chapter"], "Chapter One");
        assert_eq!(rendering["ready"], 1);
        assert_eq!(rendering["total"], 2);
        b.backfill
            .as_mut()
            .unwrap()
            .prefix
            .as_mut()
            .unwrap()
            .generated = 2;
        assert_eq!(b.view()["backfillProgress"]["phase"], "publishing");
        b.backfill = None;
        assert!(b.view()["backfillProgress"].is_null());
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
    fn adding_earlier_audio_keeps_the_same_sentence_and_intra_sentence_offset() {
        let dir = std::env::temp_dir().join(format!("texthalo-prefix-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        b.from_beginning = false;
        capture(
            &mut b,
            &json!({"fingerprint":"middle","progress":"Page 5"}),
            "The middle sentence.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &vec![0.5; 48000]).unwrap();
        b.position = 0.5;
        remember_position(&mut b);
        save_bookmark(&dir, &b).unwrap();
        let id = b.sentences[0].id.clone();
        let mut prefix = book();
        prefix.from_beginning = false;
        capture(
            &mut prefix,
            &json!({"fingerprint":"start","progress":"Page 4"}),
            "CHAPTER TWO\nEarlier text.",
        )
        .unwrap();
        let prefix_dir = backfill_directory(&dir, &b).unwrap();
        fs::create_dir_all(&prefix_dir).unwrap();
        recover(&prefix_dir, &prefix).unwrap();
        save(&prefix_dir, &prefix).unwrap();
        commit(&prefix_dir, &mut prefix, &vec![0.25; 24000]).unwrap();
        publish_prefix(&dir, &mut b, &prefix).unwrap();
        assert_eq!(b.sentences[1].id, id);
        assert_eq!(b.position, 1.5);
        assert_eq!(stable_position(&b), Some(1.5));
        assert_eq!(b.generated, 2);
        assert!(b.checkpoints[0].verified_start);
        assert_eq!(b.checkpoints[0].label, "CHAPTER TWO");
        assert!(!b.from_beginning);
        assert!(!b.complete);
        let mut restored: Book =
            serde_json::from_slice(&fs::read(dir.join("book.json")).unwrap()).unwrap();
        load_bookmark(&dir, &mut restored);
        assert_eq!(restored.position, 1.5);
        assert_eq!(restored.bookmark_offset, 12000);
        recover(&dir, &restored).unwrap();
        assert_eq!(
            crate::spoken::Spoken::load_audio(&audio_path(&dir, &restored).unwrap())
                .unwrap()
                .len(),
            72000
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_prefix_publication_preserves_original_recording_and_bookmark() {
        let dir = std::env::temp_dir().join(format!("texthalo-prefix-{}", identity()));
        fs::create_dir(&dir).unwrap();
        let mut b = book();
        capture(
            &mut b,
            &json!({"fingerprint":"middle","progress":"Page 5"}),
            "Original.",
        )
        .unwrap();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.5; 48]).unwrap();
        let mut prefix = book();
        capture(
            &mut prefix,
            &json!({"fingerprint":"start","progress":"Page 4"}),
            "Earlier.",
        )
        .unwrap();
        let pd = backfill_directory(&dir, &b).unwrap();
        fs::create_dir_all(&pd).unwrap();
        recover(&pd, &prefix).unwrap();
        save(&pd, &prefix).unwrap();
        commit(&pd, &mut prefix, &[0.25; 24]).unwrap();
        fs::create_dir(dir.join("book.tmp")).unwrap();
        assert!(publish_prefix(&dir, &mut b, &prefix).is_err());
        assert_eq!(b.committed_samples, 48);
        assert_eq!(b.audio_file, "audio.wav");
        recover(&dir, &b).unwrap();
        assert_eq!(fs::metadata(dir.join("audio.wav")).unwrap().len(), 140);
        fs::remove_dir_all(dir).unwrap();
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
    #[test]
    fn backfill_orders_pages_and_tracks_an_unfinished_sentence_seam() {
        let mut b = book();
        b.from_beginning = false;
        capture(
            &mut b,
            &json!({"fingerprint":"middle","progress":"Page 5"}),
            "on the imported page.",
        )
        .unwrap();
        let bf = Backfill {
            pages: vec![
                Page {
                    heading: None,
                    progress: "Page 4".into(),
                    fingerprint: "p4".into(),
                    text: "A sentence ends here. This continues".into(),
                },
                Page {
                    heading: Some("CHAPTER TWO".into()),
                    progress: "Page 3".into(),
                    fingerprint: "p3".into(),
                    text: "CHAPTER TWO Earlier.".into(),
                },
            ],
            verified: true,
            ..Default::default()
        };
        let prefix = prefix_book(&b, &bf).unwrap();
        assert_eq!(prefix.pages[0].progress, "Page 3");
        assert_eq!(prefix.pages[1].progress, "Page 4");
        assert!(prefix.pending.is_empty());
        let last = prefix.sentences.last().unwrap();
        assert_eq!(last.text, "This continues");
        assert_eq!(
            last.continuation_of.as_deref(),
            Some(b.sentences[0].id.as_str())
        );
        assert_eq!(last.sources[0].page, 1);
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
    fn completion_backfill_reuses_an_earlier_range_and_only_stages_the_gap() {
        let mut b = book(); b.from_beginning = false;
        capture(&mut b, &reader(40,"current"), NOVEL).unwrap();
        let seed = resume_fixture(); let seed_id = seed.sentences[0].id.clone();
        b.saved_ranges.push(Box::new(seed)); b.backfill = Some(Box::default());
        let gap = "A missing passage belongs between these two recordings.";
        stage_earlier_page(&mut b, Page {heading:None,text:format!("{ANCHOR} {gap}"),progress:"Page 26".into(),fingerprint:"wide26".into()},false,true).unwrap();
        let bf = b.backfill.as_ref().unwrap();
        assert!(bf.verified && bf.from_beginning);
        assert_eq!(bf.pages[0].text,gap);
        assert_eq!(bf.seed.as_ref().unwrap().sentences[0].id,seed_id);
    }
    #[test]
    fn completion_backfill_continues_past_chapter_boundaries() {
        let mut b = book(); b.from_beginning = false;
        capture(&mut b, &reader(40,"current"), "CHAPTER THREE\nExisting chapter.").unwrap();
        assert!(needs_earlier(&b,true)); assert!(!needs_earlier(&b,false));
        b.backfill=Some(Box::default());
        stage_earlier_page(&mut b,Page {heading:Some("CHAPTER TWO".into()),text:"CHAPTER TWO Earlier chapter.".into(),progress:"Page 10".into(),fingerprint:"p10".into()},false,true).unwrap();
        assert!(!b.backfill.as_ref().unwrap().verified);
        stage_earlier_page(&mut b,Page {heading:Some("CHAPTER ONE".into()),text:"CHAPTER ONE Opening chapter.".into(),progress:"Page 1".into(),fingerprint:"p1".into()},true,true).unwrap();
        let bf=b.backfill.as_ref().unwrap(); assert!(bf.verified && bf.from_beginning);
        assert_eq!(prefix_book(&b,bf).unwrap().checkpoints.len(),2);
    }
    #[test]
    fn completion_finds_missing_text_before_a_saved_start_in_the_same_view() {
        let b=resume_fixture();let gap="A previously uncaptured introductory sentence appears here.";
        let raw=format!("{gap}\n{ANCHOR}");
        let page=earlier_slice(&b,&reader(26,"wide26"),&raw).unwrap().unwrap();
        assert_eq!(page.text,gap);
        assert!(earlier_slice(&b,&reader(26,"wide26"),ANCHOR).unwrap().is_none());
    }
    #[test]
    fn retrieval_earlier_guard_is_bounded_for_a_large_history() {
        let mut b=book();
        for number in 0..1500 {
            b.sentences.push(Sentence {id:number.to_string(),continuation_of:None,text:format!("The archivist catalogued artifact number {number} beside the northern entrance while recording the details in her notebook."),sources:vec![],start_sample:0,end_sample:0});
        }
        let start=std::time::Instant::now();
        assert!(!contains_earlier_context(&b,"The archivist opened the garden door and watched sunlight spread across the empty courtyard. Several unfamiliar travelers waited quietly by a stone fountain while distant bells announced the start of another day."));
        assert!(start.elapsed()<Duration::from_secs(2));
    }
    #[test]
    fn completion_prefix_restores_headings_from_normalized_saved_pages() {
        let b = book();
        let bf = Backfill {
            verified: true,
            from_beginning: true,
            pages: vec![
                Page {
                    heading: Some("CHAPTER TWO".into()),
                    text: "CHAPTER TWO Second chapter.".into(),
                    progress: "Page 10".into(),
                    fingerprint: "p10".into(),
                },
                Page {
                    heading: Some("CHAPTER ONE".into()),
                    text: "CHAPTER ONE First chapter.".into(),
                    progress: "Page 1".into(),
                    fingerprint: "p1".into(),
                },
            ],
            ..Default::default()
        };
        let prefix = prefix_book(&b, &bf).unwrap();
        assert_eq!(
            prefix
                .checkpoints
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            ["CHAPTER ONE", "CHAPTER TWO"]
        );
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
    fn completion_prefix_keeps_all_chapters_and_the_existing_chapter_identity() {
        let dir = std::env::temp_dir().join(format!("texthalo-full-prefix-{}", identity()));
        fs::create_dir_all(&dir).unwrap();
        let mut b = book();
        b.from_beginning = false;
        capture(
            &mut b,
            &reader(20, "later"),
            "CHAPTER THREE\nExisting third chapter.",
        )
        .unwrap();
        let original = b.sentences[0].id.clone();
        let chapter = b.checkpoints[0].id.clone();
        recover(&dir, &b).unwrap();
        save(&dir, &b).unwrap();
        commit(&dir, &mut b, &[0.75; 24]).unwrap();
        let mut prefix = book();
        capture(
            &mut prefix,
            &reader(1, "start"),
            "CHAPTER ONE\nFirst chapter.",
        )
        .unwrap();
        capture(
            &mut prefix,
            &reader(10, "second"),
            "CHAPTER TWO\nSecond chapter.",
        )
        .unwrap();
        let pd = backfill_directory(&dir, &b).unwrap();
        fs::create_dir_all(&pd).unwrap();
        recover(&pd, &prefix).unwrap();
        save(&pd, &prefix).unwrap();
        commit(&pd, &mut prefix, &[0.25; 24]).unwrap();
        commit(&pd, &mut prefix, &[0.5; 24]).unwrap();
        publish_prefix(&dir, &mut b, &prefix).unwrap();
        assert_eq!(
            b.checkpoints
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            ["CHAPTER ONE", "CHAPTER TWO", "CHAPTER THREE"]
        );
        assert_eq!(b.checkpoints[2].id, chapter);
        assert_eq!(b.sentences[2].id, original);
        assert!(b.from_beginning);
        fs::remove_dir_all(dir).unwrap();
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
    fn restore_does_not_oscillate_when_kindle_skips_the_saved_page_number() {
        use std::cell::Cell;
        let b = book();
        let at = Cell::new(29);
        let turns = Cell::new(0);
        let target = Page {
            heading: None,
            progress: "Page 28".into(),
            fingerprint: "p28".into(),
            text: "Saved passage.".into(),
        };
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                at.set(if request["direction"] == "previous" {
                    26
                } else {
                    29
                });
                turns.set(turns.get() + 1);
            }
            let r = reader(at.get(), &format!("p{}", at.get()));
            if request["type"] == "ocr" {
                Ok(json!({"reader":r,"text":"Different passage."}))
            } else {
                Ok(r)
            }
        };
        assert!(restore_page_with(command, || true, &b, &target, RestorePolicy::Resume).is_err());
        assert!(
            turns.get() <= 1,
            "Skipped page numbers must not reverse and repeat: {} turns",
            turns.get()
        );
    }
    #[test]
    fn restore_accepts_verified_passage_after_a_page_number_jump() {
        use std::cell::Cell;
        let b = book();
        let at = Cell::new(29);
        let turns = Cell::new(0);
        let target = Page {
            heading: None,
            progress: "Page 28".into(),
            fingerprint: "old-layout".into(),
            text: "Saved passage.".into(),
        };
        let command = |request: Value| -> Result<Value, String> {
            if request["type"] == "navigate" {
                at.set(26);
                turns.set(turns.get() + 1);
            }
            let r = reader(at.get(), &format!("p{}", at.get()));
            if request["type"] == "ocr" {
                Ok(json!({"reader":r,"text":"Saved passage."}))
            } else {
                Ok(r)
            }
        };
        let restored =
            restore_page_with(command, || true, &b, &target, RestorePolicy::Resume).unwrap();
        assert_eq!(restored["progress"], "Page 26");
        assert_eq!(turns.get(), 1);
    }
    #[test]
    fn cancelling_a_borrow_returns_the_owned_reader_to_the_forward_page() {
        use std::cell::Cell;
        let b = book();
        let at = Cell::new(3usize);
        let turns = Cell::new(0usize);
        let view = || json!({"status":"ready","url":b.url,"bookId":b.book_id,"progress":format!("Page {}",at.get()),"fingerprint":format!("p{}",at.get())});
        let command = |r: Value| -> Result<Value, String> {
            match r["type"].as_str().unwrap() {
                "inspect" => Ok(view()),
                "navigate" => {
                    assert_eq!(r["fingerprint"], view()["fingerprint"]);
                    at.set(if r["direction"] == "previous" {
                        at.get() - 1
                    } else {
                        at.get() + 1
                    });
                    turns.set(turns.get() + 1);
                    Ok(view())
                }
                "ocr" => Ok(json!({"reader":view(),"text":format!("Passage {}.",at.get())})),
                _ => panic!("Unexpected command"),
            }
        };
        let target = Page {
            heading: None,
            progress: "Page 1".into(),
            fingerprint: "p1".into(),
            text: "Passage 1.".into(),
        };
        let forward = Page {
            progress: "Page 3".into(),
            fingerprint: "p3".into(),
            text: "Passage 3.".into(),
            ..target.clone()
        };
        let mut owned = view();
        assert!(restore_page_with(
            &command,
            || turns.get() < 1,
            &b,
            &target,
            RestorePolicy::Borrow(&mut owned)
        )
        .is_err());
        assert_eq!(at.get(), 2);
        assert_eq!(owned["fingerprint"], "p2");
        let restored = restore_page_with(
            &command,
            || false,
            &b,
            &forward,
            RestorePolicy::Return(&mut owned),
        )
        .unwrap();
        assert_eq!(restored["progress"], "Page 3");
        assert_eq!(at.get(), 3);
        // A manual page change invalidates ownership even during cancellation cleanup.
        at.set(1);
        let previous_turns = turns.get();
        assert!(restore_page_with(
            &command,
            || false,
            &b,
            &forward,
            RestorePolicy::Return(&mut owned)
        )
        .is_err());
        assert_eq!(at.get(), 1);
        assert_eq!(turns.get(), previous_turns);
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
