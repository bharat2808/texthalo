//! App-owned, pinned unquantized models. Never accept a URL or path from the webview.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
#[derive(Clone, Copy, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}
pub const MODELS: &[Model] = &[
    Model {
        id: "tiny",
        name: "Tiny",
        bytes: 77691713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    Model {
        id: "base",
        name: "Base",
        bytes: 147951465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    Model {
        id: "small",
        name: "Small",
        bytes: 487601967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
    Model {
        id: "medium",
        name: "Medium",
        bytes: 1533763059,
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
    },
    Model {
        id: "large-v3-turbo",
        name: "Large v3 Turbo",
        bytes: 1624555275,
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
    },
    Model {
        id: "large-v3",
        name: "Large v3",
        bytes: 3095033483,
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
    },
];
pub fn model(id: &str) -> Result<Model, String> {
    MODELS
        .iter()
        .copied()
        .find(|m| m.id == id)
        .ok_or_else(|| "Choose a supported Whisper model.".into())
}
pub fn validate_settings(settings: &crate::config::AssistantSettings) -> Result<(), String> {
    if !matches!(settings.transcription_engine.as_str(), "apple" | "whisper") {
        return Err("Choose a supported transcription source.".into());
    }
    model(&settings.whisper_model)?;
    if settings.language.contains('\0') {
        return Err("Invalid transcription language.".into());
    }
    if settings.transcription_engine == "whisper" && settings.language != "auto" {
        let code = settings.language.split(['-', '_']).next().unwrap_or("");
        if whisper_rs::get_lang_id(code)
            .is_none_or(|id| id > 98 && !settings.whisper_model.starts_with("large-v3"))
        {
            return Err("Choose a supported Whisper language.".into());
        }
    }
    Ok(())
}
pub fn directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("whisper-models"))
        .map_err(|e| e.to_string())
}
pub fn path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    model(id)?;
    Ok(directory(app)?.join(format!("ggml-{id}.bin")))
}
pub fn installed(app: &AppHandle, id: &str) -> bool {
    let Ok(m) = model(id) else { return false };
    let Ok(p) = path(app, id) else { return false };
    p.metadata().is_ok_and(|s| s.len() == m.bytes)
        && std::fs::read_to_string(p.with_extension("verified")).is_ok_and(|hash| hash == m.sha256)
}
struct Download {
    id: String,
    received: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
pub struct ModelState {
    download: Mutex<Option<Download>>,
    error: Mutex<Option<String>>,
}
#[derive(Serialize)]
pub struct ModelStatus {
    #[serde(flatten)]
    model: Model,
    installed: bool,
}
#[tauri::command]
pub fn whisper_models(app: AppHandle) -> serde_json::Value {
    let state = app.state::<ModelState>();
    let download = state.download.lock().unwrap();
    serde_json::json!({"models":MODELS.iter().map(|m| ModelStatus { model:*m, installed:installed(&app,m.id) }).collect::<Vec<_>>(),
        "download":download.as_ref().map(|d| serde_json::json!({"id":d.id,"received":d.received.load(Ordering::Relaxed),"total":model(&d.id).unwrap().bytes})),
        "error":state.error.lock().unwrap().clone()})
}
#[tauri::command]
pub fn whisper_cancel_download(app: AppHandle) {
    if let Some(d) = app.state::<ModelState>().download.lock().unwrap().as_ref() {
        d.cancel.store(true, Ordering::SeqCst);
    }
}
#[tauri::command]
pub async fn whisper_download(app: AppHandle, id: String) -> Result<(), String> {
    let m = model(&id)?;
    let state = app.state::<ModelState>();
    let received = Arc::new(AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = state.download.lock().unwrap();
        if active.is_some() {
            return Err("Another model is downloading. Cancel it first.".into());
        }
        if installed(&app, &id) {
            return Ok(());
        }
        *active = Some(Download {
            id: id.clone(),
            received: received.clone(),
            cancel: cancel.clone(),
        });
        *state.error.lock().unwrap() = None;
    }
    let result = download(&app, m, received, cancel).await;
    // Cancel is an ordinary user action, not a persistent model error.
    let result = if result
        .as_ref()
        .err()
        .is_some_and(|error| error == "Model download canceled.")
    {
        Ok(())
    } else {
        result
    };
    *state.download.lock().unwrap() = None;
    *state.error.lock().unwrap() = result.as_ref().err().cloned();
    let _ = app.emit("texthalo:whisper-models", whisper_models(app.clone()));
    result
}
async fn canceled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
async fn request<T>(
    future: impl std::future::Future<Output = Result<T, reqwest::Error>>,
    cancel: &AtomicBool,
) -> Result<T, String> {
    tokio::select! { biased;
        _ = canceled(cancel) => Err("Model download canceled.".into()),
        result = future => result.map_err(|e|e.to_string()),
    }
}
async fn download(
    app: &AppHandle,
    m: Model,
    received: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let dest = path(app, m.id)?;
    let part = dest.with_extension("part");
    let result = async {
        tokio::fs::create_dir_all(directory(app)?)
            .await
            .map_err(|e| e.to_string())?;
        let file = tokio::fs::File::create(&part)
            .await
            .map_err(|e| e.to_string())?;
        file.set_len(m.bytes).await.map_err(|e| e.to_string())?;
        drop(file);
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| e.to_string())?;
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/{REVISION}/ggml-{}.bin",
            m.id
        );
        let size = m.bytes.div_ceil(4);
        let jobs = (0..4).map(|index| {
            let part = part.clone();
            let client = client.clone();
            let url = url.clone();
            let cancel = cancel.clone();
            let received = received.clone();
            async move {
                let start = index * size;
                let end = ((index + 1) * size).min(m.bytes) - 1;
                let mut response = request(
                    client
                        .get(url)
                        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                        .send(),
                    &cancel,
                )
                .await?;
                let expected_range = format!("bytes {start}-{end}/{}", m.bytes);
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
                    || response
                        .headers()
                        .get(reqwest::header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        != Some(expected_range.as_str())
                {
                    return Err("The model host returned an invalid download range.".into());
                }
                let mut output = tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(part)
                    .await
                    .map_err(|e| e.to_string())?;
                output
                    .seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(|e| e.to_string())?;
                let mut count = 0;
                while let Some(bytes) = request(response.chunk(), &cancel).await? {
                    if cancel.load(Ordering::SeqCst) {
                        return Err("Model download canceled.".into());
                    }
                    count += bytes.len() as u64;
                    if count > end - start + 1 {
                        return Err("The model host sent too much data.".into());
                    }
                    output.write_all(&bytes).await.map_err(|e| e.to_string())?;
                    received.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                }
                output.sync_all().await.map_err(|e| e.to_string())?;
                if count != end - start + 1 {
                    return Err("The model download ended early.".into());
                }
                Ok::<(), String>(())
            }
        });
        futures_util::future::try_join_all(jobs).await?;
        let hash_path = part.clone();
        let hash_cancel = cancel.clone();
        let hash = tauri::async_runtime::spawn_blocking(move || {
            let mut file = std::fs::File::open(hash_path).map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                if hash_cancel.load(Ordering::SeqCst) {
                    return Err("Model download canceled.".into());
                }
                let n = std::io::Read::read(&mut file, &mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            Ok::<_, String>(format!("{:x}", hash.finalize()))
        })
        .await
        .map_err(|e| e.to_string())??;
        if cancel.load(Ordering::SeqCst) {
            return Err("Model download canceled.".into());
        }
        if hash != m.sha256 {
            return Err("Model verification failed. Download it again.".into());
        }
        if dest.exists() {
            tokio::fs::remove_file(&dest)
                .await
                .map_err(|e| e.to_string())?;
        }
        tokio::fs::rename(&part, &dest)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::write(dest.with_extension("verified"), hash)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(part).await;
    }
    result
}
#[tauri::command]
pub fn whisper_remove(app: AppHandle, id: String) -> Result<(), String> {
    model(&id)?;
    let state = app.state::<ModelState>();
    let active = state.download.lock().unwrap();
    if active.is_some() {
        return Err("Wait for the download to finish or cancel it first.".into());
    }
    crate::whisper::unload(&app)?;
    let path = path(&app, &id)?;
    for file in [path.clone(), path.with_extension("verified")] {
        if file.exists() {
            std::fs::remove_file(file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_catalog_only() {
        for id in ["../small", "small-q5_0", "https://evil/model", "small.en"] {
            assert!(super::model(id).is_err());
        }
        assert!(super::model("small").is_ok());
    }
    #[test]
    fn invalid_source_model_and_language_are_rejected_before_saving() {
        let mut settings = crate::config::AssistantSettings::default();
        assert!(validate_settings(&settings).is_ok());
        settings.transcription_engine = "remote".into();
        assert!(validate_settings(&settings).is_err());
        settings.transcription_engine = "whisper".into();
        settings.whisper_model = "../outside".into();
        assert!(validate_settings(&settings).is_err());
        settings.whisper_model = "small".into();
        settings.language = "en\0".into();
        assert!(validate_settings(&settings).is_err());
        settings.language = "yue".into();
        assert!(validate_settings(&settings).is_err());
        settings.whisper_model = "large-v3".into();
        assert!(validate_settings(&settings).is_ok());
        settings.language = "auto".into();
        assert!(validate_settings(&settings).is_ok());
    }
    #[tokio::test]
    async fn cancel_interrupts_a_stalled_network_request() {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            signal.store(true, Ordering::SeqCst);
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request(
                std::future::pending::<Result<(), reqwest::Error>>(),
                &cancel,
            ),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err(), "Model download canceled.");
    }
}
