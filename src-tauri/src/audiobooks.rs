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
#[serde(rename_all = "camelCase")]
struct Book {
    #[serde(default)]
    schema_version: u32,
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
        json!({"backfillProgress":backfill_progress,"id":self.id,"key":self.key,"title":self.title,"platform":"kindle","pages":self.pages.len(),"generated":self.generated,"seconds":self.seconds(),"position":self.position,"speed":self.speed,"complete":self.complete,"fromBeginning":self.from_beginning,"message":self.message,"activity":self.activity,"backfillBlocked":self.backfill.as_ref().and_then(|b|b.blocked.as_ref()),
            "checkpoints":self.checkpoints.iter().enumerate().map(|(i,c)|chapter_view(self,i,c)).collect::<Vec<_>>(),
            "passage":current.map(|s|json!({"text":s.text,"page":s.sources.first().and_then(|p|self.pages.get(p.page)).map(|p|&p.progress)}))})
    }
}
struct Playing {
    book: String,
    job: u64,
    player: Arc<crate::pcm::Player>,
}
#[derive(Default)]
struct Library {
    worker: bool,
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
    json!({"position":book.position,"speed":book.speed,"sentenceId":sentence.map(|s|&s.id),"sampleOffset":sentence.map(|s|at.saturating_sub(s.start_sample).min(s.end_sample-s.start_sample))})
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
    json!({"id":c.id,"label":c.label,"sentence":c.sentence,"page":book.pages.get(c.page).map(|p|&p.progress),"seconds":book.sentences.get(c.sentence).filter(|_|c.sentence<book.generated).map(|s|s.start_sample as f64/RATE as f64),"duration":duration,"complete":complete,"status":if complete{"complete"}else if book.message.is_some(){"attention"}else if c.sentence<book.generated{"partial"}else{"preparing"},"needsEarlier":!c.verified_start})
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
    let first = next
        .checkpoints
        .first_mut()
        .ok_or("Missing imported chapter")?;
    first.sentence = 0;
    first.page = 0;
    first.verified_start = true;
    if let Some(label) = prefix.pages.first().and_then(|p| p.heading.clone()) {
        first.label = label;
    } else if prefix.from_beginning {
        first.label = "Opening".into();
    }
    next.pages.splice(0..0, prefix.pages.clone());
    next.sentences.splice(0..0, prefix.sentences.clone());
    next.generated += sentence_count;
    next.committed_samples += added;
    next.from_beginning = prefix.from_beginning;
    next.complete = next.from_beginning
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
            (audio_path(&dir.join("backfill"), prefix)?, added),
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
    let fingerprint = reader["fingerprint"]
        .as_str()
        .ok_or("Missing page fingerprint")?;
    let normal = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if book
        .pages
        .iter()
        .any(|p| p.fingerprint == fingerprint || (!normal.is_empty() && p.text == normal))
    {
        return Err("Repeated page detected; generation stopped safely".into());
    }
    let heading = chapter(text);
    if heading.is_some() && !book.pending.is_empty() {
        return Err(
            "An unfinished sentence precedes a chapter. Review the source before continuing."
                .into(),
        );
    }
    let page = book.pages.len();
    let progress = reader["progress"].as_str().unwrap_or("").to_owned();
    if let Some(label) = heading {
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
        heading: chapter(text),
        progress,
        fingerprint: fingerprint.into(),
        text: normal.clone(),
    });
    if !normal.is_empty() {
        book.pending.push(Source {
            page,
            start: 0,
            end: normal.len(),
            text: normal,
        });
    }
    assemble(book, false)
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
            json!({"reader":r,"book":book.map(Book::view),"generating":l.producing.as_ref().is_some_and(|(id,_,_)|book.is_some_and(|b|&b.id==id))}),
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
    let service = app.state::<Audiobooks>();
    let mut l = service.0.lock().unwrap();
    let b = l.books.get(id).ok_or("Audiobook not found")?;
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
#[tauri::command]
pub async fn audiobook_play(
    app: AppHandle,
    id: String,
    seconds: Option<f64>,
) -> Result<(), String> {
    let handle = app.clone();
    let book_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || play(&handle, &book_id, seconds))
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
        if let Err(error) = audiobook_generate(app.clone(), Some(id.clone()), false).await {
            let service = app.state::<Audiobooks>();
            let mut l = service.0.lock().unwrap();
            if let Some(book) = l.books.get_mut(&id) {
                book.message = Some(error);
                let _ = directory(&id).and_then(|d| save(&d, book));
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
    for _ in 0..100 {
        if !finishing && !active() {
            return Err("Preparation stopped".into());
        }
        let at = page_number(r["progress"].as_str().unwrap_or(""))
            .ok_or("Reader page number unavailable")?;
        if at == goal {
            break;
        }
        r = command(
            json!({"type":"navigate", "bookId":r["bookId"], "fingerprint":r["fingerprint"], "direction":if at<goal {"next"} else {"previous"}}),
        )?;
        if let Some(owned) = tracker.as_deref_mut() {
            *owned = r.clone();
        }
    }
    if page_number(r["progress"].as_str().unwrap_or("")) != Some(goal) {
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
    if text != target.text {
        return Err(
            "Reader layout or saved passage changed. Restore the original layout before retrying."
                .into(),
        );
    }
    Ok(result["reader"].clone())
}
fn prefix_book(book: &Book, backfill: &Backfill) -> Result<Book, String> {
    let mut prefix = book.clone();
    prefix.id = identity();
    prefix.audio_file = default_audio();
    prefix.backfill = None;
    prefix.pages.clear();
    prefix.sentences.clear();
    prefix.checkpoints.clear();
    prefix.pending.clear();
    prefix.generated = 0;
    prefix.committed_samples = 0;
    prefix.position = 0.;
    prefix.bookmark_sentence = None;
    prefix.bookmark_offset = 0;
    prefix.complete = false;
    prefix.end_verified = false;
    prefix.from_beginning = backfill.from_beginning;
    for page in backfill.pages.iter().rev() {
        // Raw OCR headings were preserved separately before whitespace normalization.
        let text = page.text.clone();
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
fn backfill_step(
    app: &AppHandle,
    id: &str,
    token: &str,
    reader: &mut Value,
    settings: &crate::config::Settings,
) -> Result<bool, String> {
    let service = app.state::<Audiobooks>();
    let result = backfill_step_inner(app, id, token, reader, settings);
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
) -> Result<bool, String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let book = {
        let mut l = service.0.lock().unwrap();
        let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
        if b.checkpoints.first().is_none_or(|c| c.verified_start) {
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
        if state.pages.len() >= 100 {
            return Err("Chapter start could not be verified within 100 pages".into());
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
            let bf = b.backfill.as_mut().unwrap();
            if bf
                .pages
                .iter()
                .chain(b.pages.iter())
                .any(|p| p.fingerprint == page.fingerprint)
            {
                return Err("Repeated page while finding chapter start".into());
            }
            bf.verified = page.heading.is_some() || at["startOfBook"] == true;
            bf.label = page.heading.clone();
            bf.from_beginning = at["startOfBook"] == true;
            bf.pages.push(page);
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
    let prefix_dir = directory(id)?.join("backfill");
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
    if auto && book.generated > 0 {
        play(app, id, None)?;
        auto = false;
    }
    let mut reader = if let Some(page) = book.pages.last() {
        restore_page(
            &bridge,
            &service,
            id,
            token,
            &book,
            page,
            RestorePolicy::Resume,
        )?
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
                b.seconds() - b.position,
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
            if auto {
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
            b.checkpoints.first().is_some_and(|c| !c.verified_start)
                && b.backfill.as_ref().is_none_or(|bf| bf.blocked.is_none())
                && (buffer >= 120.
                    || reader["endOfBook"] == true
                    || (chapter_only && b.checkpoints.len() > 1))
        };
        if earlier {
            match backfill_step(app, id, token, &mut reader, &settings) {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    let mut l = service.0.lock().unwrap();
                    let b = l.books.get_mut(id).ok_or("Audiobook removed")?;
                    if b.checkpoints.first().is_some_and(|c| !c.verified_start) {
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
                b.complete = b.from_beginning && b.checkpoints.iter().all(|c| c.verified_start);
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
        let mut next = b.clone();
        next.activity = Some("forward".into());
        capture(&mut next, &reader, text)?;
        if next.pending.iter().map(|p| p.text.len()).sum::<usize>() > 20_000 {
            return Err("No reliable sentence boundary across several pages. Review the source before continuing.".into());
        }
        save(&directory(id)?, &next)?;
        *b = next;
        need_next = true;
    }
}
fn generate(app: &AppHandle, id: Option<String>, full: bool) -> Result<String, String> {
    generate_mode(app, id, full, false)
}
fn generate_mode(
    app: &AppHandle,
    id: Option<String>,
    full: bool,
    chapter_only: bool,
) -> Result<String, String> {
    let service = app.state::<Audiobooks>();
    let bridge = app.state::<crate::reader_bridge::ReaderBridge>();
    let reader = bridge.command(json!({"type":"inspect"}))?;
    let book_key = key(&reader)?;
    let mut library = service.0.lock().unwrap();
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
        if full && reader["startOfBook"] != true {
            return Err(
                "Open the beginning of the book before creating a complete audiobook.".into(),
            );
        }
        let book = Book {
            schema_version: 2,
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
    if full && !library.books[&id].from_beginning {
        return Err("This recording starts at your imported page. A complete audiobook must be imported from the beginning.".into());
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
    tauri::async_runtime::spawn_blocking(move || generate_mode(&app, Some(id), false, true))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn book() -> Book {
        Book {
            schema_version: 2,
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
        let prefix_dir = dir.join("backfill");
        fs::create_dir(&prefix_dir).unwrap();
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
        let pd = dir.join("backfill");
        fs::create_dir(&pd).unwrap();
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
