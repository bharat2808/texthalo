//! Local archive of completed speech. Audio and metadata live in app support only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const LIMIT: usize = 50;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub created_at: u64,
    pub engine: String,
    pub voice: String,
    pub text: String,
    pub duration_seconds: Option<f64>,
    pub audio_file: String,
}

/// Incremental WAV writer. It only touches the producer thread, never CoreAudio's callback.
pub struct PcmRecorder {
    path: PathBuf,
    writer: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    failed: bool,
}

pub fn reserve_audio_path(dir: &Path, extension: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create audio history: {e}"))?;
    Ok(dir.join(format!("{}.{}", new_id(), extension)))
}

impl PcmRecorder {
    pub fn new(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create audio history: {e}"))?;
        let id = new_id();
        let path = dir.join(format!("{id}.wav.part"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 24_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(&path, spec)
            .map_err(|e| format!("create audio history recording: {e}"))?;
        Ok(Self {
            path,
            writer: Some(writer),
            failed: false,
        })
    }

    pub fn write(&mut self, samples: &[f32]) {
        if self.failed {
            return;
        }
        if let Some(writer) = self.writer.as_mut() {
            for sample in samples {
                let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                if writer.write_sample(pcm).is_err() {
                    self.failed = true;
                    self.writer = None;
                    let _ = std::fs::remove_file(&self.path);
                    break;
                }
            }
        }
    }

    pub fn finish(mut self) -> Option<PathBuf> {
        let writer = self.writer.take()?;
        if self.failed || writer.finalize().is_err() {
            let _ = std::fs::remove_file(&self.path);
            return None;
        }
        let final_path = self.path.with_extension("");
        std::fs::rename(&self.path, &final_path).ok()?;
        Some(final_path)
    }
}

impl Drop for PcmRecorder {
    fn drop(&mut self) {
        // Unfinished and canceled readings must not leave partial recordings behind.
        self.writer.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn save_entry(
    dir: &Path,
    audio_path: &Path,
    engine: &str,
    voice: &str,
    text: &str,
    duration_seconds: Option<f64>,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create audio history: {e}"))?;
    let id = audio_path
        .file_stem()
        .and_then(|n| n.to_str())
        .ok_or("invalid audio history filename")?
        .strip_suffix(".wav")
        .unwrap_or_else(|| {
            audio_path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("")
        })
        .to_string();
    let audio_file = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid audio history filename")?
        .to_string();
    let entry = Entry {
        id: id.clone(),
        created_at: now_ms(),
        engine: engine.to_string(),
        voice: voice.to_string(),
        text: text.to_string(),
        duration_seconds,
        audio_file,
    };
    let encoded = serde_json::to_vec(&entry).map_err(|e| format!("encode audio history: {e}"))?;
    let tmp = dir.join(format!("{id}.json.part"));
    std::fs::write(&tmp, encoded).map_err(|e| format!("write audio history: {e}"))?;
    std::fs::rename(tmp, dir.join(format!("{id}.json")))
        .map_err(|e| format!("save audio history: {e}"))?;
    prune(dir);
    Ok(())
}

pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let Ok(files) = std::fs::read_dir(dir) else {
        return Ok(entries);
    };
    for file in files.flatten() {
        if file.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = std::fs::read(file.path()) {
            if let Ok(entry) = serde_json::from_slice::<Entry>(&raw) {
                if valid_id(&entry.id) && valid_filename(&entry.audio_file) {
                    entries.push(entry);
                }
            }
        }
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("invalid audio history id".into());
    }
    let prefix = format!("{id}.");
    if let Ok(files) = std::fs::read_dir(dir) {
        for file in files.flatten() {
            let Some(name) = file.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with(&prefix) {
                std::fs::remove_file(file.path())
                    .map_err(|e| format!("delete audio history: {e}"))?;
            }
        }
    }
    Ok(())
}

pub fn clear(dir: &Path) -> Result<(), String> {
    let files = match std::fs::read_dir(dir) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("read audio history: {e}")),
    };
    for file in files.flatten() {
        if file.file_type().map(|t| t.is_file()).unwrap_or(false) {
            std::fs::remove_file(file.path()).map_err(|e| format!("clear audio history: {e}"))?;
        }
    }
    Ok(())
}

pub fn audio_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("invalid audio history id".into());
    }
    let raw = std::fs::read(dir.join(format!("{id}.json")))
        .map_err(|e| format!("read audio history: {e}"))?;
    let entry: Entry =
        serde_json::from_slice(&raw).map_err(|e| format!("read audio history: {e}"))?;
    if !valid_filename(&entry.audio_file) {
        return Err("invalid audio history filename".into());
    }
    let path = dir.join(entry.audio_file);
    if !path.is_file() {
        return Err("the saved audio file is missing".into());
    }
    Ok(path)
}

fn prune(dir: &Path) {
    if let Ok(entries) = list(dir) {
        for entry in entries.into_iter().skip(LIMIT) {
            let _ = delete(dir, &entry.id);
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn valid_filename(name: &str) -> bool {
    !name.is_empty() && Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name)
}

fn new_id() -> String {
    format!("{}-{}", now_ms(), NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
