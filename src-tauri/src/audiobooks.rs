//! App-owned audiobook library. Metadata is the commit record: only its PCM prefix
//! is playable. Startup rolls back an interrupted append before exposing a book.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
const RATE: u64 = 24_000;
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
    text: String,
    sources: Vec<Source>,
    start_sample: u64,
    end_sample: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Page {
    progress: String,
    fingerprint: String,
    text: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Checkpoint {
    label: String,
    sentence: usize,
    page: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Book {
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
        json!({"id":self.id,"key":self.key,"title":self.title,"platform":"kindle","pages":self.pages.len(),"generated":self.generated,"seconds":self.seconds(),"position":self.position,"speed":self.speed,"complete":self.complete,"fromBeginning":self.from_beginning,"message":self.message,
            "checkpoints":self.checkpoints.iter().map(|c|json!({"label":c.label,"sentence":c.sentence,"page":self.pages.get(c.page).map(|p|&p.progress),"seconds":self.sentences.get(c.sentence).filter(|_|c.sentence<self.generated).map(|s|s.start_sample as f64/RATE as f64)})).collect::<Vec<_>>(),
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
fn save_bookmark(dir: &Path, book: &Book) -> Result<(), String> {
    let bytes = json!({"position":book.position,"speed":book.speed}).to_string();
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
    let path = dir.join("audio.wav");
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
            .open(dir.join("audio.wav"))
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
            label,
            sentence: book.sentences.len(),
            page,
        });
    } else if page == 0 {
        book.checkpoints.push(Checkpoint {
            label: progress.clone(),
            sentence: 0,
            page,
        });
    }
    book.pages.push(Page {
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
                            b.message = None;
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
                book.speed = snapshot.speed;
                if let Err(e) = directory(&id).and_then(|d| save_bookmark(&d, book)) {
                    book.message = Some(e);
                }
            }
            if !current || !snapshot.available {
                l.playing = None;
            }
        });
        service
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
        b.position
    });
    if !seconds.is_finite() || seconds < 0. || seconds > b.seconds() {
        return Err("Choose a position within generated audio".into());
    }
    let samples = b.committed_samples;
    let speed = b.speed;
    let finished = !l.producing.as_ref().is_some_and(|(i, _, _)| i == id);
    let audio = directory(id)?.join("audio.wav");
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
fn restore(bridge: &crate::reader_bridge::ReaderBridge, book: &Book) -> Result<Value, String> {
    let mut r = bridge.command(json!({"type":"inspect"}))?;
    if key(&r)? != book.key {
        return Err("Open this book in the connected browser tab".into());
    }
    if let Some(page) = book.pages.last() {
        let goal = page_number(&page.progress)
            .ok_or("Reopen the last captured page to continue generation")?;
        for _ in 0..100 {
            let current = page_number(r["progress"].as_str().unwrap_or(""))
                .ok_or("Reader page number is unavailable")?;
            if current == goal {
                break;
            }
            r = navigate(bridge, &r, if current < goal { "next" } else { "previous" })?;
        }
        if page_number(r["progress"].as_str().unwrap_or("")) != Some(goal) {
            return Err("Open the last captured page manually to continue".into());
        }
        let result = bridge
            .command(json!({"type":"ocr","bookId":r["bookId"],"fingerprint":r["fingerprint"]}))?;
        let text = result["text"]
            .as_str()
            .ok_or("Page text unavailable")?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text != page.text {
            return Err(
                "Restored passage changed. Restore the original reader layout before continuing."
                    .into(),
            );
        }
        r = result["reader"].clone();
    }
    Ok(r)
}
fn producer(
    app: &AppHandle,
    id: &str,
    token: &str,
    full: bool,
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
    let mut reader = restore(&bridge, &book)?;
    let mut need_next = !book.pages.is_empty();
    let mut auto = autoplay;
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
            if let Some(p) = l
                .playing
                .as_ref()
                .filter(|p| p.book == id && state.job.lock().unwrap().is_current(p.job))
            {
                p.player
                    .update_file(&directory(id)?.join("audio.wav"), committed, false)?;
            }
            drop(l);
            if auto {
                play(app, id, None)?;
                auto = false;
            }
            continue;
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
                b.complete = b.from_beginning;
                b.message = if b.complete {
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
    let restart = library.playing.as_ref().is_some_and(|p| p.book == id);
    let autoplay = !full && !restart;
    let token = uuid::Uuid::new_v4().to_string();
    library.worker = true;
    library.producing = Some((id.clone(), token.clone(), full));
    library.books.get_mut(&id).unwrap().message = None;
    drop(library);
    if restart {
        if let Err(error) = play(app, &id, None) {
            let mut library = service.0.lock().unwrap();
            library.producing = None;
            library.worker = false;
            return Err(error);
        }
    }
    let handle = app.clone();
    let book_id = id.clone();
    std::thread::spawn(move || {
        let result = producer(&handle, &book_id, &token, full, autoplay);
        let state = handle.state::<Audiobooks>();
        let mut library = state.0.lock().unwrap();
        library.worker = false;
        if library
            .producing
            .as_ref()
            .is_some_and(|(_, t, _)| t == &token)
        {
            library.producing = None;
            if let Err(error) = result {
                if let Some(book) = library.books.get_mut(&book_id) {
                    book.message = Some(error);
                    let _ = directory(&book_id).and_then(|dir| save(&dir, book));
                }
            }
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
    use super::*;
    fn book() -> Book {
        Book {
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
