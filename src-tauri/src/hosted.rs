//! Authenticated TextHalo API client used by the hosted Fish Audio engine.
use futures_util::{SinkExt, StreamExt};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::{Arc, Mutex, OnceLock}, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_secure_storage::{OptionsRequest, SecureStorageExt};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const CREDENTIALS_KEY: &str = "desktop-credentials-v1";
static TOKEN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CREDENTIAL_CACHE: OnceLock<Mutex<Option<DesktopCredentials>>> = OnceLock::new();
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static SIGNIN_CANCEL: Mutex<Option<tokio::sync::oneshot::Sender<()>>> = Mutex::new(None);

pub fn api_origin() -> Option<String> {
    let configured = option_env!("TEXTHALO_API_BASE_URL")
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    let configured = if configured.is_empty() && cfg!(debug_assertions) {
        "http://localhost:8788"
    } else {
        configured
    };
    let parsed = url::Url::parse(configured).ok()?;
    let valid_https = parsed.scheme() == "https";
    let valid_local_debug_http = cfg!(debug_assertions)
        && parsed.scheme() == "http"
        && matches!(
            parsed.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
    if (valid_https || valid_local_debug_http)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        Some(parsed.as_str().trim_end_matches('/').to_string())
    } else {
        None
    }
}
pub fn service_configured() -> bool {
    api_origin().is_some()
}
/// Remember the app handle without touching the Keychain. Secure-storage access is
/// deferred until the user opens Account or signs in, so startup cannot trigger prompts.
pub fn initialize_secure_storage(app: &AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
}

fn secure_storage_app() -> Result<&'static AppHandle, String> {
    APP_HANDLE.get().ok_or_else(|| "Secure sign-in storage is not ready. Restart TextHalo and try again.".to_string())
}

fn storage_request(data: Option<String>) -> OptionsRequest {
    OptionsRequest { prefixed_key: Some(CREDENTIALS_KEY.into()), data, sync: Some(true), keychain_access: None }
}

pub fn is_signed_in() -> bool {
    read_credentials().is_ok()
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopCredentials { access_token: String, refresh_token: String, expires_at: u64 }
fn read_credentials() -> Result<DesktopCredentials, String> {
    let cache = CREDENTIAL_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache.lock().map_err(|_| "Could not access the saved TextHalo sign-in.".to_string())?;
    if let Some(credentials) = cached.as_ref() { return Ok(credentials.clone()); }
    let app = secure_storage_app()?;
    let stored = app.secure_storage().get_item(app.clone(), storage_request(None))
        .map_err(|_| "Could not read secure sign-in storage.".to_string())?.data;
    let raw = stored.ok_or_else(|| "Sign in to use Fish Audio voices.".to_string())?;
    let credentials: DesktopCredentials = serde_json::from_str(&raw).map_err(|_| "Your desktop sign-in needs renewal. Sign in again from the Account screen.".to_string())?;
    *cached = Some(credentials.clone());
    Ok(credentials)
}
#[tauri::command]
pub fn desktop_is_signed_in(app: AppHandle) -> bool {
    initialize_secure_storage(&app);
    is_signed_in()
}
fn write_credentials(credentials: &DesktopCredentials) -> Result<(), String> {
    let serialized = serde_json::to_string(credentials).map_err(|_| "Could not prepare your secure sign-in.".to_string())?;
    let app = secure_storage_app()?;
    app.secure_storage().set_item(app.clone(), storage_request(Some(serialized)))
        .map_err(|_| "Could not securely save your sign-in.".to_string())?;
    *CREDENTIAL_CACHE.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "Could not access the saved TextHalo sign-in.".to_string())? = Some(credentials.clone());
    Ok(())
}
async fn access_token(force_refresh: bool) -> Result<String, String> {
    let _guard = TOKEN_LOCK.lock().await;
    let mut credentials = read_credentials()?;
    if force_refresh || credentials.expires_at <= unix_now().saturating_add(45) {
        let response = reqwest::Client::new().post(format!("{}/v1/desktop/sessions/refresh", origin()?))
            .json(&json!({"refreshToken": credentials.refresh_token})).send().await
            .map_err(|_| "Could not refresh your TextHalo sign-in. Check your internet connection.".to_string())?;
        if !response.status().is_success() { let _ = clear_token(); return Err("Your sign-in expired. Sign in again to continue.".into()); }
        let refreshed: Value = response.json().await.map_err(|_| "The sign-in service returned an invalid session.".to_string())?;
        credentials.access_token = refreshed.get("accessToken").and_then(Value::as_str).ok_or("The sign-in service returned an invalid session.")?.to_string();
        credentials.refresh_token = refreshed.get("refreshToken").and_then(Value::as_str).ok_or("The sign-in service returned an invalid session.")?.to_string();
        credentials.expires_at = unix_now().saturating_add(refreshed.get("expiresIn").and_then(Value::as_u64).unwrap_or(300));
        write_credentials(&credentials)?;
    }
    Ok(credentials.access_token)
}
fn unix_now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() }
pub fn clear_token() -> Result<(), String> {
    let app = secure_storage_app()?;
    app.secure_storage().remove_item(app.clone(), storage_request(None))
        .map_err(|_| "Could not remove the saved sign-in from secure storage.".to_string())?;
    if let Some(cache) = CREDENTIAL_CACHE.get() {
        *cache.lock().map_err(|_| "Could not access the saved TextHalo sign-in.".to_string())? = None;
    }
    Ok(())
}
fn origin() -> Result<String, String> {
    api_origin()
        .ok_or_else(|| "The TextHalo backend URL is not configured in this app build.".into())
}
fn api_error(status: reqwest::StatusCode, body: &Value) -> String {
    match body.get("error").and_then(Value::as_str).unwrap_or("") {
        "unauthorized" => "Your sign-in expired. Sign in again to continue.".into(),
        "insufficient_credits" => "You’re out of hosted credits. Open Billing to review your balance or add a credit pack.".into(),
        "voice_clone_entitlement_required" | "subscription_required" => "Voice cloning requires an active Creator plan.".into(),
        "feature_disabled" => "Voice cloning is not enabled on this server yet.".into(),
        "unknown_voice" | "unknown_voice_clone" => "That voice is no longer available. Choose another voice.".into(),
        "voice_not_ready" => "This voice is still training. Try again when it is ready.".into(),
        "voice_clone_limit_reached" => "You’ve reached the saved voice clone limit for your plan.".into(),
        _ if status.is_server_error() => "The TextHalo speech service is temporarily unavailable.".into(),
        _ => "The request could not be completed. Check your settings and try again.".into(),
    }
}
async fn request(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<reqwest::Response, String> {
    let url = format!("{}{}", origin()?, path);
    let token = access_token(false).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not start the TextHalo network client.".to_string())?;
    let mut req = client.request(method, url).bearer_auth(token);
    if let Some(json) = body {
        req = req.json(&json);
    }
    req.send().await.map_err(|_| {
        "Could not reach the TextHalo service. Check your internet connection.".to_string()
    })
}
async fn json_request(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let response = request(method, path, body).await?;
    let status = response.status();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(api_error(status, &value));
    }
    Ok(value)
}

#[tauri::command]
pub async fn begin_desktop_signin(app: AppHandle) -> Result<String, String> {
    initialize_secure_storage(&app);
    cancel_desktop_signin_inner();
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| "Could not start secure app sign-in. Please try again.".to_string())?;
    let address = listener.local_addr().map_err(|_| "Could not start secure app sign-in.".to_string())?;
    let redirect_uri = format!("http://127.0.0.1:{}/desktop-auth-callback", address.port());
    let mut state_bytes = Vec::with_capacity(32);
    state_bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    state_bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let state = URL_SAFE_NO_PAD.encode(state_bytes);
    let mut verifier_bytes = Vec::with_capacity(32);
    verifier_bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    verifier_bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut web = url::Url::parse(option_env!("TEXTHALO_WEBSITE_BASE_URL").unwrap_or("https://texthalo.app"))
        .map_err(|_| "The TextHalo website URL is invalid in this app build.".to_string())?;
    if web.scheme() != "https" && !(cfg!(debug_assertions) && web.scheme() == "http" && matches!(web.host_str(), Some("localhost" | "127.0.0.1"))) {
        return Err("The TextHalo sign-in website must use HTTPS.".into());
    }
    web.set_path("/desktop-connect/");
    web.query_pairs_mut().append_pair("redirect_uri", &redirect_uri).append_pair("state", &state).append_pair("code_challenge", &challenge);
    let url = web.to_string();
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    *SIGNIN_CANCEL.lock().map_err(|_| "Could not start secure app sign-in.".to_string())? = Some(cancel);
    tauri::async_runtime::spawn(async move {
        let result = tokio::select! {
            _ = cancelled => Err("Sign-in cancelled.".to_string()),
            result = tokio::time::timeout(Duration::from_secs(120), accept_desktop_callback(listener, address, redirect_uri, state, verifier)) => {
                match result { Ok(result) => result, Err(_) => Err("Sign-in timed out. Start again from TextHalo.".to_string()) }
            }
        };
        if let Ok(mut pending) = SIGNIN_CANCEL.lock() { *pending = None; }
        let payload = match result {
            Ok(()) => json!({"success": true}),
            Err(message) => json!({"success": false, "message": message}),
        };
        let _ = app.emit("texthalo:desktop-auth", payload);
    });
    Ok(url)
}

fn cancel_desktop_signin_inner() {
    if let Ok(mut pending) = SIGNIN_CANCEL.lock() {
        if let Some(cancel) = pending.take() { let _ = cancel.send(()); }
    }
}

#[tauri::command]
pub fn cancel_desktop_signin() {
    cancel_desktop_signin_inner();
}

async fn accept_desktop_callback(listener: TcpListener, expected_addr: SocketAddr, redirect_uri: String, expected_state: String, verifier: String) -> Result<(), String> {
    let (mut socket, peer) = listener.accept().await.map_err(|_| "Could not receive the sign-in return.".to_string())?;
    if !peer.ip().is_loopback() { return Err("Rejected an invalid local sign-in callback.".into()); }
    let mut request = vec![0u8; 8192];
    let size = socket.read(&mut request).await.map_err(|_| "Could not read the sign-in return.".to_string())?;
    let first_line = std::str::from_utf8(&request[..size]).ok().and_then(|s| s.lines().next()).ok_or("Invalid sign-in callback.")?;
    let mut parts = first_line.split_whitespace();
    if parts.next() != Some("GET") { return Err("Invalid sign-in callback method.".into()); }
    let target = parts.next().ok_or("Invalid sign-in callback URL.")?;
    let callback = url::Url::parse(&format!("http://127.0.0.1:{}{}", expected_addr.port(), target)).map_err(|_| "Invalid sign-in callback URL.")?;
    if callback.path() != "/desktop-auth-callback" { return Err("Invalid sign-in callback path.".into()); }
    let code = callback.query_pairs().find(|(key, _)| key == "code").map(|(_, value)| value.into_owned()).ok_or("The sign-in callback did not contain a code.")?;
    let state = callback.query_pairs().find(|(key, _)| key == "state").map(|(_, value)| value.into_owned()).ok_or("The sign-in callback did not contain state.")?;
    if state != expected_state { return Err("Sign-in state did not match. Start again from TextHalo.".into()); }
    let result = exchange_desktop_code(&code, &verifier, &state, &redirect_uri).await;
    let (status, message) = match &result { Ok(()) => ("200 OK", "Sign-in complete. You can return to the TextHalo app."), Err(_) => ("400 Bad Request", "Sign-in could not be completed. Return to TextHalo and try again.") };
    let body = format!("<!doctype html><meta charset=utf-8><title>TextHalo</title><body style='font:16px system-ui;background:#f5f4ef;color:#18231f;display:grid;place-items:center;height:90vh'><main><h1>{message}</h1><p>You may close this tab.</p></main></body>");
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}", body.len());
    let _ = socket.write_all(response.as_bytes()).await;
    result
}

async fn exchange_desktop_code(code: &str, verifier: &str, state: &str, redirect_uri: &str) -> Result<(), String> {
    let response = reqwest::Client::new().post(format!("{}/v1/desktop/handoffs/exchange", origin()?))
        .json(&json!({"code":code,"verifier":verifier,"state":state,"redirectUri":redirect_uri}))
        .send().await.map_err(|_| "Could not reach the TextHalo service.".to_string())?;
    let status = response.status();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    if !status.is_success() { return Err(api_error(status, &body)); }
    let access = body.get("accessToken").and_then(Value::as_str).ok_or("Invalid desktop session response.")?.to_string();
    let refresh = body.get("refreshToken").and_then(Value::as_str).ok_or("Invalid desktop session response.")?.to_string();
    let expires = body.get("expiresIn").and_then(Value::as_u64).unwrap_or(300);
    let account = reqwest::Client::new().get(format!("{}/v1/account", origin()?)).bearer_auth(&access).send().await.map_err(|_| "Could not validate your TextHalo session.".to_string())?;
    if !account.status().is_success() { return Err("The TextHalo session could not be validated.".into()); }
    write_credentials(&DesktopCredentials { access_token: access, refresh_token: refresh, expires_at: unix_now().saturating_add(expires) })
}

#[tauri::command]
pub async fn desktop_sign_out() -> Result<(), String> {
    if let Ok(credentials) = read_credentials() {
        let _ = reqwest::Client::new().post(format!("{}/v1/desktop/sessions/revoke", origin()?))
            .json(&json!({"refreshToken":credentials.refresh_token})).send().await;
    }
    clear_token()
}
#[tauri::command]
pub async fn desktop_account() -> Result<Value, String> {
    json_request(reqwest::Method::GET, "/v1/account", None).await
}
#[tauri::command]
pub async fn desktop_voices(query: String, page: u32) -> Result<Value, String> {
    let q = url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    json_request(
        reqwest::Method::GET,
        &format!("/v1/voices?query={q}&page={page}&pageSize=24"),
        None,
    )
    .await
}
#[tauri::command]
pub async fn desktop_voice_preview(
    voice_id: String,
    sample_id: Option<String>,
) -> Result<Vec<u8>, String> {
    let encoded = url::form_urlencoded::byte_serialize(voice_id.as_bytes()).collect::<String>();
    let sample_query = sample_id
        .map(|id| {
            format!(
                "?sampleId={}",
                url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>()
            )
        })
        .unwrap_or_default();
    let response = request(
        reqwest::Method::GET,
        &format!("/v1/voices/{encoded}/preview{sample_query}"),
        None,
    )
    .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(api_error(status, &body));
    }
    Ok(response
        .bytes()
        .await
        .map_err(|_| "Could not download this voice preview.".to_string())?
        .to_vec())
}
#[tauri::command]
pub async fn desktop_clones() -> Result<Value, String> {
    json_request(reqwest::Method::GET, "/v1/voices/clones", None).await
}
#[tauri::command]
pub async fn desktop_delete_clone(voice_id: String) -> Result<(), String> {
    let encoded = url::form_urlencoded::byte_serialize(voice_id.as_bytes()).collect::<String>();
    let response = request(
        reqwest::Method::DELETE,
        &format!("/v1/voices/clones/{encoded}"),
        None,
    )
    .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(api_error(status, &body));
    }
    Ok(())
}
#[tauri::command]
pub async fn desktop_upload_clone(
    path: String,
    name: String,
    consent: bool,
) -> Result<Value, String> {
    if !consent {
        return Err("Confirm that you own or have permission to clone this voice.".into());
    }
    if name.trim().is_empty() || name.chars().count() > 100 {
        return Err("Enter a voice name of up to 100 characters.".into());
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| "Could not read the selected audio file.".to_string())?;
    if bytes.len() > 25 * 1024 * 1024 {
        return Err("Choose an audio file smaller than 25 MB.".into());
    }
    let mime = match std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        _ => return Err("Choose a WAV, MP3, M4A, OGG, or FLAC audio file.".into()),
    };
    let token = access_token(false).await?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(
            std::path::Path::new(&path)
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("voice.wav")
                .to_string(),
        )
        .mime_str(mime)
        .map_err(|_| "Unsupported audio file.".to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("audio", part)
        .text("name", name.trim().to_string())
        .text("consent", "true");
    let response = reqwest::Client::new()
        .post(format!("{}/v1/voices/clones", origin()?))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .map_err(|_| "Could not reach the TextHalo service.".to_string())?;
    let status = response.status();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(api_error(status, &body));
    }
    Ok(body)
}
#[tauri::command]
pub async fn desktop_checkout(plan_id: String) -> Result<String, String> {
    checkout("/v1/billing/checkout", json!({"planId":plan_id})).await
}
#[tauri::command]
pub async fn desktop_topup(pack_id: String) -> Result<String, String> {
    checkout("/v1/billing/topup", json!({"packId":pack_id})).await
}
async fn checkout(path: &str, body: Value) -> Result<String, String> {
    let result = json_request(reqwest::Method::POST, path, Some(body)).await?;
    let url = result
        .get("url")
        .and_then(Value::as_str)
        .ok_or("The checkout service returned no destination.")?;
    let parsed = url::Url::parse(url)
        .map_err(|_| "The checkout service returned an invalid destination.")?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("checkout.stripe.com") {
        return Err("The checkout destination could not be verified.".into());
    }
    Ok(url.into())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Ready {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    AwaitingPlayback {
        #[serde(rename = "nextChunkIndex")]
        next_chunk_index: u64,
        #[serde(rename = "audioBytesSent")]
        audio_bytes_sent: u64,
    },
    Finished {
        status: String,
        #[serde(rename = "availableCredits")]
        _available_credits: Option<u64>,
    },
    Error {
        error: Option<String>,
        #[serde(rename = "code")]
        _code: Option<String>,
    },
    Other,
}
#[derive(Serialize)]
struct StreamStart<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    token: &'a str,
    #[serde(rename = "idempotencyKey")]
    key: String,
    text: &'a str,
    #[serde(rename = "voiceId")]
    voice_id: &'a str,
    #[serde(rename = "modelId")]
    model_id: &'a str,
    #[serde(rename = "enhanceText")]
    enhance_text: bool,
}

pub async fn stream_speech(
    app: AppHandle,
    job_id: u64,
    text: String,
    voice_id: String,
    model_id: String,
    enhance: bool,
) -> Result<(), String> {
    let token = access_token(false).await?;
    let base = origin()?;
    let url = base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1)
        + "/v1/tts/stream";
    let (mut socket, _) = connect_async(&url).await.map_err(|_| {
        "Could not connect to live speech. Check your network and backend URL.".to_string()
    })?;
    if model_id.is_empty() {
        return Err("Refresh the hosted voice list before generating speech.".into());
    }
    let start = StreamStart {
        kind: "start",
        token: &token,
        key: uuid::Uuid::new_v4().to_string(),
        text: &text,
        voice_id: &voice_id,
        model_id: &model_id,
        enhance_text: enhance,
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&start)
                .map_err(|_| "Could not prepare the speech request.".to_string())?
                .into(),
        ))
        .await
        .map_err(|_| "Could not start speech.".to_string())?;
    let player = Arc::new(crate::pcm::Player::new()?);
    let app_state = app.state::<crate::AppState>();
    if !app_state.job.lock().unwrap().is_current(job_id) {
        return Ok(());
    }
    app_state.spoken.activate_pcm(player.clone());
    let mut buffer = Vec::<u8>::new();
    let mut received = 0u64;
    let mut is_started = false;
    let mut finished = false;
    while let Some(message) = socket.next().await {
        if !app_state.job.lock().unwrap().is_current(job_id) {
            let _ = socket.close(None).await;
            return Ok(());
        }
        match message.map_err(|_| "The live speech connection was interrupted.".to_string())? {
            Message::Binary(bytes) => {
                received = received.saturating_add(bytes.len() as u64);
                buffer.extend_from_slice(&bytes);
                let even = buffer.len() & !1;
                let samples = buffer[..even]
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect::<Vec<_>>();
                buffer.drain(..even);
                if !samples.is_empty() {
                    player.push(samples, false);
                    if !is_started {
                        player.start()?;
                        is_started = true;
                        let chars = text.chars().count();
                        let mut job = app_state.job.lock().unwrap();
                        if job.is_current(job_id) {
                            job.set(crate::Phase::Speaking, None, Some(chars));
                            let _ = app.emit("kiegen:status", &job.status);
                        }
                    }
                }
            }
            Message::Text(raw) => {
                let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
                match serde_json::from_value::<StreamEvent>(value.clone())
                    .unwrap_or(StreamEvent::Other)
                {
                    StreamEvent::Ready { session_id } => {
                        let mut job = app_state.job.lock().unwrap();
                        if job.is_current(job_id) {
                            job.set(
                                crate::Phase::Preparing,
                                Some(format!(
                                    "Connected · {}",
                                    &session_id[..8.min(session_id.len())]
                                )),
                                Some(text.chars().count()),
                            );
                            let _ = app.emit("kiegen:status", &job.status);
                        }
                    }
                    StreamEvent::AwaitingPlayback {
                        next_chunk_index,
                        audio_bytes_sent,
                    } => {
                        while player.played_audio_bytes() < audio_bytes_sent
                            && app_state.job.lock().unwrap().is_current(job_id)
                        {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                        }
                        if !app_state.job.lock().unwrap().is_current(job_id) {
                            let _ = socket.close(None).await;
                            return Ok(());
                        }
                        let played = player.played_audio_bytes();
                        socket.send(Message::Text(json!({"type":"continue","nextChunkIndex":next_chunk_index,"playedAudioBytes":played}).to_string().into())).await.map_err(|_|"Could not continue the live speech stream.".to_string())?;
                    }
                    StreamEvent::Finished { status, .. } => {
                        if status != "completed" {
                            return Err("The live speech request did not complete.".into());
                        }
                        finished = true;
                        break;
                    }
                    StreamEvent::Error { error, .. } => {
                        return Err(api_error(
                            reqwest::StatusCode::BAD_REQUEST,
                            &json!({"error":error.unwrap_or_else(||"stream_failed".into())}),
                        ))
                    }
                    StreamEvent::Other => {}
                }
            }
            Message::Close(_) => break,
            Message::Ping(bytes) => socket
                .send(Message::Pong(bytes))
                .await
                .map_err(|_| "Live speech connection closed.".to_string())?,
            _ => {}
        }
    }
    if !finished {
        return Err("The live speech stream ended before it finished.".into());
    }
    player.push(Vec::new(), true);
    while player.is_playing()? && app_state.job.lock().unwrap().is_current(job_id) {
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    Ok(())
}
