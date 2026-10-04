//! Tauri adapter for the reusable Rust engine. One worker, bounded PCM, latest preview only.
use crate::{
    config::AssistantSettings,
    whisper_core::{Engine, MAX_AUDIO, RATE, STEP, WINDOW},
    whisper_models,
};
use base64::Engine as _;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager};
struct Job {
    id: u64,
    path: PathBuf,
    language: String,
    audio: Vec<f32>,
    finished: bool,
    abort: Arc<AtomicBool>,
}
struct Shared {
    job: Mutex<Option<Job>>,
    wake: Condvar,
    shutdown: AtomicBool,
    unload: AtomicBool,
}
#[derive(Clone)]
pub struct Worker(Arc<Shared>);
impl Default for Worker {
    fn default() -> Self {
        Self(Arc::new(Shared {
            job: Mutex::new(None),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
            unload: AtomicBool::new(false),
        }))
    }
}
pub fn setup(app: &AppHandle) {
    app.manage(whisper_models::ModelState::default());
    let worker = Worker::default();
    app.manage(worker.clone());
    let app = app.clone();
    std::thread::spawn(move || run(app, worker.0));
}
pub fn start(app: &AppHandle, id: u64, settings: &AssistantSettings) -> Result<(), String> {
    if !whisper_models::installed(app, &settings.whisper_model) {
        return Err("Download the selected Whisper model in Dictate settings first.".into());
    }
    let path = whisper_models::path(app, &settings.whisper_model)?;
    let worker = app.state::<Worker>();
    let mut job = worker.0.job.lock().unwrap();
    if let Some(old) = job.take() {
        old.abort.store(true, Ordering::SeqCst);
    }
    *job = Some(Job {
        id,
        path,
        language: settings.language.clone(),
        audio: Vec::new(),
        finished: false,
        abort: Arc::new(AtomicBool::new(false)),
    });
    worker.0.wake.notify_one();
    Ok(())
}
pub fn audio(app: &AppHandle, id: u64, encoded: &str) -> Result<(), String> {
    if encoded.len() > 128 * 1024 {
        return Err("Microphone audio frame is too large.".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid microphone audio.")?;
    if bytes.len() % 4 != 0 {
        return Err("Invalid microphone audio.".into());
    }
    let samples: Vec<_> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    if !samples.iter().all(|s| s.is_finite()) {
        return Err("Invalid microphone audio.".into());
    }
    let worker = app.state::<Worker>();
    let mut guard = worker.0.job.lock().unwrap();
    append_audio(&mut guard, id, samples)?;
    worker.0.wake.notify_one();
    Ok(())
}
fn append_audio(active: &mut Option<Job>, id: u64, samples: Vec<f32>) -> Result<(), String> {
    if let Some(job) = active.as_mut().filter(|j| j.id == id && !j.finished) {
        if job.audio.len() + samples.len() > MAX_AUDIO {
            return Err(
                "Recording reached 90 seconds. Release and try a shorter recording.".into(),
            );
        }
        job.audio.extend(samples);
    }
    Ok(())
}
fn finish_job(active: &mut Option<Job>, id: u64) {
    if let Some(job) = active.as_mut().filter(|j| j.id == id && !j.finished) {
        job.abort.store(true, Ordering::SeqCst);
        job.abort = Arc::new(AtomicBool::new(false));
        job.finished = true;
    }
}
fn cancel_job(active: &mut Option<Job>, id: Option<u64>) {
    if active
        .as_ref()
        .is_some_and(|j| id.is_none() || id == Some(j.id))
    {
        if let Some(job) = active.take() {
            job.abort.store(true, Ordering::SeqCst);
        }
    }
}
pub fn finish(app: &AppHandle, id: u64) {
    let worker = app.state::<Worker>();
    finish_job(&mut worker.0.job.lock().unwrap(), id);
    worker.0.wake.notify_one();
}
pub fn cancel(app: &AppHandle, id: Option<u64>) {
    let Some(worker) = app.try_state::<Worker>() else {
        return;
    };
    cancel_job(&mut worker.0.job.lock().unwrap(), id);
    worker.0.wake.notify_one();
}
pub fn unload(app: &AppHandle) -> Result<(), String> {
    let worker = app.state::<Worker>();
    if worker.0.job.lock().unwrap().is_some() {
        return Err("Finish or cancel the current dictation before deleting a model.".into());
    }
    worker.0.unload.store(true, Ordering::SeqCst);
    worker.0.wake.notify_one();
    Ok(())
}
pub fn shutdown(app: &AppHandle) {
    cancel(app, None);
    if let Some(w) = app.try_state::<Worker>() {
        w.0.shutdown.store(true, Ordering::SeqCst);
        w.0.wake.notify_one();
    }
}
fn run(app: AppHandle, shared: Arc<Shared>) {
    let mut engine: Option<(PathBuf, Engine)> = None;
    let mut current = 0;
    let mut cursor = 0;
    let mut last_count = 0;
    let mut committed = String::new();
    loop {
        let guard = shared.job.lock().unwrap();
        if shared.shutdown.load(Ordering::SeqCst) {
            return;
        }
        if shared.unload.swap(false, Ordering::SeqCst) {
            engine = None;
        }
        let ready = guard.as_ref().is_some_and(|j| {
            j.finished || j.audio.len().saturating_sub(last_count) >= 3 * RATE || j.id != current
        });
        if !ready {
            let (next, timeout) = shared
                .wake
                .wait_timeout(guard, Duration::from_secs(300))
                .unwrap();
            if timeout.timed_out() && next.is_none() {
                engine = None;
            }
            drop(next);
            continue;
        }
        let job = guard.as_ref().unwrap();
        if current != job.id {
            current = job.id;
            cursor = 0;
            last_count = 0;
            committed.clear();
        }
        let id = job.id;
        let path = job.path.clone();
        let language = job.language.clone();
        let finished = job.finished;
        let abort = job.abort.clone();
        let count = job.audio.len();
        let end = (cursor + WINDOW).min(count);
        let audio = if finished {
            job.audio.clone()
        } else {
            job.audio[cursor..end].to_vec()
        };
        drop(guard);
        if engine.as_ref().is_none_or(|(p, _)| p != &path) {
            match Engine::load(&path) {
                Ok(loaded) => engine = Some((path, loaded)),
                Err(error) => {
                    crate::assistant::whisper_output(&app, id, Err(error), true);
                    cancel(&app, Some(id));
                    continue;
                }
            }
        }
        // A new session starts loading immediately, but waits for enough audio before inference.
        if !finished && audio.len() < 3 * RATE {
            last_count = 0;
            continue;
        }
        let model = &engine.as_ref().unwrap().1;
        let result = if finished {
            model.finish(&audio, &language, abort.clone())
        } else {
            model.transcribe(&audio, &language, abort.clone())
        };
        if abort.load(Ordering::SeqCst) {
            continue;
        }
        if !shared
            .job
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|j| j.id == id && Arc::ptr_eq(&j.abort, &abort))
        {
            continue;
        }
        last_count = count;
        match result {
            Ok(text) if !finished => {
                let preview = crate::whisper_core::merge(&committed, &text);
                if end - cursor == WINDOW {
                    committed = preview.clone();
                    cursor += STEP;
                    last_count = end;
                }
                if !preview.is_empty() {
                    crate::assistant::whisper_output(&app, id, Ok(preview), false);
                }
            }
            result => {
                crate::assistant::whisper_output(&app, id, result, true);
                cancel(&app, Some(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job() -> Option<Job> {
        Some(Job {
            id: 7,
            path: PathBuf::new(),
            language: "en".into(),
            audio: vec![0.1; RATE],
            finished: false,
            abort: Arc::new(AtomicBool::new(false)),
        })
    }
    #[test]
    fn release_preserves_all_audio_and_interrupts_only_the_preview() {
        let mut active = job();
        let preview = active.as_ref().unwrap().abort.clone();
        append_audio(&mut active, 7, vec![0.2; RATE]).unwrap();
        finish_job(&mut active, 7);
        assert!(preview.load(Ordering::SeqCst));
        let final_abort = active.as_ref().unwrap().abort.clone();
        assert!(!final_abort.load(Ordering::SeqCst));
        finish_job(&mut active, 7); // Duplicate stop cannot abort the final decode.
        assert!(Arc::ptr_eq(&final_abort, &active.as_ref().unwrap().abort));
        append_audio(&mut active, 7, vec![0.3; RATE]).unwrap();
        assert_eq!(active.as_ref().unwrap().audio.len(), 2 * RATE);
        assert_eq!(active.as_ref().unwrap().audio[0], 0.1);
        assert_eq!(active.as_ref().unwrap().audio[RATE], 0.2);
    }
    #[test]
    fn late_events_cannot_finish_cancel_or_modify_a_new_request() {
        let mut active = job();
        let abort = active.as_ref().unwrap().abort.clone();
        finish_job(&mut active, 6);
        cancel_job(&mut active, Some(6));
        append_audio(&mut active, 6, vec![0.2; RATE]).unwrap();
        assert!(!active.as_ref().unwrap().finished);
        assert!(!abort.load(Ordering::SeqCst));
        assert_eq!(active.as_ref().unwrap().audio.len(), RATE);
        cancel_job(&mut active, Some(7));
        assert!(active.is_none());
        assert!(abort.load(Ordering::SeqCst));
    }
    #[test]
    fn the_audio_limit_rejects_the_frame_without_dropping_earlier_audio() {
        let mut active = job();
        assert!(append_audio(&mut active, 7, vec![0.2; MAX_AUDIO]).is_err());
        assert_eq!(active.as_ref().unwrap().audio.len(), RATE);
    }
}
