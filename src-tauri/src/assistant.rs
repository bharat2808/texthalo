//! Native assistant transport and app-owned action dispatch.
use crate::assistant_job::{HoldJob, Mode};
use crate::{capture, config::Settings, AppState, Phase};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

struct Native {
    child: Child,
    input: ChildStdin,
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Default)]
pub struct AssistantState {
    inner: Mutex<Inner>,
}
#[derive(Default)]
struct Inner {
    hold_error: String,
    cloud: Option<(u64, String, tokio::sync::watch::Sender<bool>)>,
    provider: String,
    mode: Option<Mode>,
    cloud_started: Option<u64>,
    billing_request: Option<(u64, String)>,
    usage: Value,
    native: Option<Native>,
    job: HoldJob,
    status: Value,
    availability: Value,
    spoke: Option<u64>,
    speech_id: Option<u64>,
    speech_feed: Option<tokio::sync::watch::Sender<(String, bool)>>,
    speech_settings: Option<Settings>,
}

fn hold_configuration(settings: &Settings) -> Value {
    let mut bindings = serde_json::Map::new();
    for (mode, key) in [
        ("transcribe", &settings.shortcuts.transcribe),
        ("assistant", &settings.shortcuts.assistant),
    ] {
        if crate::shortcuts::is_hold(key) {
            bindings.insert(mode.into(), json!(key));
        }
    }
    json!({"command":"hold_configure", "id":0, "holdBindings":bindings})
}

pub fn configure_holds(app: &AppHandle) -> Result<(), String> {
    let value = hold_configuration(&app.state::<AppState>().settings.lock().unwrap());
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    // Do not launch a sidecar solely for an empty hold configuration.
    if inner.native.is_none()
        && value["holdBindings"]
            .as_object()
            .is_some_and(|b| b.is_empty())
    {
        return Ok(());
    }
    send(app, &mut inner, value)
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AssistantState::default());
    crate::whisper::setup(app);
    tauri::async_runtime::spawn(crate::hosted_assistant::recover(app.clone()));
    WebviewWindowBuilder::new(
        app,
        "assistant",
        WebviewUrl::App("index.html?assistant".into()),
    )
    .title("TextHalo Assistant")
    .inner_size(380.0, 130.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .focusable(false)
    .resizable(false)
    .focused(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .visible_on_all_workspaces(true)
    .build()?;
    Ok(())
}

fn send(app: &AppHandle, inner: &mut Inner, value: Value) -> Result<(), String> {
    if let Some(native) = inner.native.as_mut() {
        if native
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            inner.native = None;
        }
    }
    if inner.native.is_none() {
        #[cfg(target_os = "macos")]
        let path = {
            let bundled = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .with_file_name("texthalo-assistant");
            if bundled.exists() || !cfg!(debug_assertions) {
                bundled
            } else {
                std::path::PathBuf::from(env!("TEXTHALO_ASSISTANT_DEV_PATH"))
            }
        };
        #[cfg(not(target_os = "macos"))]
        let path = std::path::PathBuf::from("texthalo-assistant");
        let mut child = Command::new(path)
            .env(
                "TEXTHALO_AUDIT_DIR",
                app.path()
                    .app_data_dir()
                    .map_err(|e| e.to_string())?
                    .join("generation-history"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start native assistant: {e}"))?;
        let mut input = child.stdin.take().ok_or("Native input unavailable")?;
        let holds = hold_configuration(&app.state::<AppState>().settings.lock().unwrap());
        writeln!(input, "{holds}")
            .and_then(|_| input.flush())
            .map_err(|e| e.to_string())?;
        let output = child.stdout.take().ok_or("Native output unavailable")?;
        let pid = child.id();
        inner.native = Some(Native { child, input });
        let app = app.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    receive(&app, event);
                }
            }
            let state = app.state::<AssistantState>();
            let mut inner = state.inner.lock().unwrap();
            if inner.native.as_ref().map(|n| n.child.id()) == Some(pid) {
                inner.native = None;
                crate::whisper::cancel(&app, None);
                cancel_cloud(&mut inner);
                stop_assistant_speech(&app, &mut inner);
                inner.job.cancel();
                inner.status = json!({"phase":"error","mode":inner.mode,"provider":inner.provider,"message":"The native assistant stopped. Hold the shortcut again to retry."});
                publish(&app, &inner.status);
            }
        });
    }
    let native = inner.native.as_mut().unwrap();
    writeln!(native.input, "{value}")
        .and_then(|_| native.input.flush())
        .map_err(|e| format!("Native assistant connection failed: {e}"))
}

struct Presentation {
    show: bool,
    compact: bool,
    height: f64,
}

#[derive(Debug, PartialEq)]
enum NarrationOwnership {
    None,
    Current,
    Replaced,
}
fn narration_ownership(owned: Option<u64>, current: u64) -> NarrationOwnership {
    match owned {
        None => NarrationOwnership::None,
        Some(id) if id == current => NarrationOwnership::Current,
        Some(_) => NarrationOwnership::Replaced,
    }
}

fn audio_presentation(status: &Value, phase: &Phase, message: Option<&str>) -> Value {
    let mut value = status.clone();
    value["assistantSpeech"] = json!(true);
    // Copy/Undo after a terminal speech failure owns this window too. Preserve
    // its recovery result instead of redisplaying the old playback error.
    if matches!(phase, Phase::Error) && status["phase"] == "result" && status["action"] != "speak" {
        return value;
    }
    match phase {
        Phase::Idle => {
            value["phase"] = json!("idle");
        }
        Phase::Error => {
            value["phase"] = json!("error");
            value["action"] = Value::Null;
            value["message"] = json!(message
                .unwrap_or("Speech could not complete. The generated text is available to copy."));
        }
        _ => {
            let generating = status["phase"] == "streaming";
            value["phase"] = json!(if generating { "streaming" } else { "speaking" });
            value["message"] = json!(match (generating, matches!(phase, Phase::Speaking)) {
                (true, true) => "Speaking · generating…",
                (true, false) => "Preparing voice · generating…",
                (false, true) => "Speaking…",
                (false, false) => "Preparing voice…",
            });
        }
    }
    value
}

/// Called after releasing the speech-job mutex. An assistant-owned narration uses
/// the same preview window, rather than opening a second playback overlay.
pub(crate) fn sync_speech(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AssistantState>() else {
        return false;
    };
    let mut inner = state.inner.lock().unwrap();
    // Read current speech only after locking the assistant. A tick snapshot taken
    // earlier can predate a newly acquired narration lease and cancel that new job.
    let (id, status) = {
        let state = app.state::<AppState>();
        let job = state.job.lock().unwrap();
        (job.id, job.status.clone())
    };
    match narration_ownership(inner.speech_id, id) {
        NarrationOwnership::None => return false,
        NarrationOwnership::Replaced => {
            // Global Stop and another speech request invalidate the old lease.
            // cancel_locked cannot stop the new job: it checks lease identity.
            cancel_locked(app, &mut inner);
            return false;
        }
        NarrationOwnership::Current => {}
    }
    let next = audio_presentation(&inner.status, &status.phase, status.message.as_deref());
    if inner.status != next {
        if matches!(status.phase, Phase::Idle | Phase::Error) {
            inner.speech_feed = None;
            if matches!(status.phase, Phase::Error) {
                cancel_cloud(&mut inner);
                let id = inner.status["id"].as_u64().unwrap_or(0);
                if inner.native.is_some() {
                    let _ = send(app, &mut inner, json!({"command":"halt","id":id}));
                }
            } else {
                inner.job.cancel();
                if inner.native.is_some() {
                    let _ = send(app, &mut inner, json!({"command":"cancel","id":0}));
                }
            }
        }
        inner.status = next;
        publish(app, &inner.status);
    }
    true
}

fn presentation(status: &Value) -> Presentation {
    let busy = matches!(
        status["phase"].as_str(),
        Some("recording" | "processing" | "capturing_screen" | "streaming" | "speaking")
    );
    let show = !matches!(status["phase"].as_str(), Some("idle" | "canceled"))
        && (busy || !matches!(status["action"].as_str(), Some("speak" | "applied")));
    let expanded =
        status["text"].as_str().is_some_and(|s| !s.is_empty()) && status["action"] != "applied";
    let compact = busy || matches!(status["action"].as_str(), Some("copied" | "undone"));
    let height = if busy && expanded {
        if status["previewExpanded"] == true {
            status["previewHeight"]
                .as_f64()
                .unwrap_or(76.0)
                .clamp(76.0, 4096.0)
        } else {
            76.0
        }
    } else if compact {
        44.0
    } else if expanded {
        360.0
    } else {
        140.0
    };
    Presentation {
        show,
        compact,
        height,
    }
}

fn set_preview_expanded(
    status: &mut Value,
    request_id: Option<u64>,
    expanded: bool,
) -> Result<(), String> {
    let id = status["id"].as_u64().filter(|id| *id != 0);
    if id.is_none() || request_id != id {
        return Err("This preview is no longer current".into());
    }
    if !matches!(
        status["phase"].as_str(),
        Some("recording" | "processing" | "capturing_screen" | "streaming" | "speaking")
    ) {
        return Err("This preview is no longer live".into());
    }
    status["previewExpanded"] = json!(expanded);
    if !expanded {
        status.as_object_mut().unwrap().remove("previewHeight");
    }
    Ok(())
}

fn preserve_preview(previous: &Value, next: &mut Value) {
    if previous["id"].as_u64().is_some() && previous["id"] == next["id"] {
        next["previewExpanded"] = json!(previous["previewExpanded"] == true);
        if let Some(height) = previous["previewHeight"].as_f64() {
            next["previewHeight"] = json!(height);
        }
    }
}

fn set_preview_height(
    status: &mut Value,
    request_id: Option<u64>,
    height: Option<f64>,
) -> Result<(), String> {
    let height = height
        .filter(|height| height.is_finite() && (76.0..=4096.0).contains(height))
        .ok_or("Invalid preview height")?;
    if status["previewExpanded"] != true {
        return Err("This preview is collapsed".into());
    }
    set_preview_expanded(status, request_id, true)?;
    status["previewHeight"] = json!(height.ceil());
    Ok(())
}

fn publish(app: &AppHandle, status: &Value) {
    let _ = app.emit("texthalo:assistant", status);
    let Presentation {
        show,
        compact,
        height,
    } = presentation(status);
    if matches!(status["action"].as_str(), Some("copied" | "undone")) {
        let id = status["id"].clone();
        let action = status["action"].clone();
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let state = app.state::<AssistantState>();
            let inner = state.inner.lock().unwrap();
            if inner.status["id"] == id
                && inner.status["action"] == action
                && inner.status["phase"] == "result"
            {
                if let Some(window) = app.get_webview_window("assistant") {
                    let _ = window.hide();
                }
            }
        });
    }
    let busy = matches!(
        status["phase"].as_str(),
        Some("recording" | "processing" | "capturing_screen" | "streaming" | "speaking")
    );
    let anchor = status.get("bounds").cloned();
    let assistant_error_overlay = is_assistant_error(status);
    let error_height = assistant_error_height(status);
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if assistant_error_overlay {
            if let Some(assistant_window) = app.get_webview_window("assistant") {
                let _ = assistant_window.hide();
            }
            if let Some(window) = app.get_webview_window("speech-overlay") {
                let _ = window.set_focusable(error_height > 190.0);
                crate::overlay::position_sized(&window, 430.0, error_height);
                let _ = window.show();
            }
        } else if let Some(window) = app.get_webview_window("assistant") {
            if show {
                let _ = window.set_focusable(!compact && !busy);
                if compact {
                    crate::overlay::position_sized(&window, 270.0, height);
                } else {
                    let already_visible = window.is_visible().unwrap_or(false);
                    let _ = window.set_size(tauri::LogicalSize::new(380.0, height));
                    if !already_visible {
                        position_panel(&window, anchor.as_ref(), height);
                    }
                }
                #[cfg(target_os = "macos")]
                if let Ok(raw) = window.ns_window() {
                    // Tauri show() makes a window key. Order it forward without activation instead.
                    let native = unsafe { &*(raw as *const objc2_app_kit::NSWindow) };
                    native.orderFrontRegardless();
                }
                #[cfg(not(target_os = "macos"))]
                let _ = window.show();
            } else {
                let _ = window.hide();
            }
        }
    });
}
fn position_panel(window: &tauri::WebviewWindow, anchor: Option<&Value>, height: f64) {
    let Some(anchor) = anchor else {
        let _ = window.center();
        return;
    };
    let x = anchor["x"].as_f64().unwrap_or(0.0);
    let y = anchor["y"].as_f64().unwrap_or(0.0);
    if let Ok(monitors) = window.available_monitors() {
        for monitor in monitors {
            let scale = monitor.scale_factor();
            let origin = monitor.position().to_logical::<f64>(scale);
            let size = monitor.size().to_logical::<f64>(scale);
            if x >= origin.x
                && x < origin.x + size.width
                && y >= origin.y
                && y < origin.y + size.height
            {
                let panel_x = (x + 12.0)
                    .max(origin.x + 12.0)
                    .min(origin.x + (size.width - 392.0).max(12.0));
                let panel_y = (y + anchor["height"].as_f64().unwrap_or(0.0) + 8.0)
                    .max(origin.y + 32.0)
                    .min(origin.y + (size.height - height - 12.0).max(32.0));
                let _ = window.set_position(tauri::LogicalPosition::new(panel_x, panel_y));
                return;
            }
        }
    }
    let _ = window.center();
}

pub(crate) fn whisper_output(
    app: &AppHandle,
    id: u64,
    result: Result<String, String>,
    finished: bool,
) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if !inner.job.current(id) {
        return;
    }
    let value = match result {
        Ok(text) => {
            json!({"command":if finished {"whisper_result"} else {"whisper_partial"},"id":id,"text":text})
        }
        Err(message) => json!({"command":"whisper_error","id":id,"text":message}),
    };
    let _ = send(app, &mut inner, value);
}
fn receive(app: &AppHandle, mut event: Value) {
    if event["phase"] == "whisper_audio" {
        if let (Some(id), Some(audio)) = (event["id"].as_u64(), event["audio"].as_str()) {
            if let Err(error) = crate::whisper::audio(app, id, audio) {
                whisper_output(app, id, Err(error), true);
            }
        }
        return;
    }
    if event["phase"] == "whisper_capture_error" {
        if let Some(id) = event["id"].as_u64() {
            whisper_output(
                app,
                id,
                Err(event["message"]
                    .as_str()
                    .unwrap_or("Microphone conversion failed.")
                    .into()),
                true,
            );
        }
        return;
    }
    if event["phase"] == "whisper_finish" {
        if let Some(id) = event["id"].as_u64() {
            crate::whisper::finish(app, id);
        }
        return;
    }
    if event["phase"] == "whisper_cancel" {
        crate::whisper::cancel(app, event["id"].as_u64());
        return;
    }
    if event["phase"] == "audit_error" {
        let _ = app.emit("texthalo:generation-history-error", &event["message"]);
        eprintln!(
            "Generation history could not be saved: {}",
            event["message"]
        );
        return;
    }
    if matches!(
        event["phase"].as_str(),
        Some("hold_pressed" | "hold_released" | "hold_cancel")
    ) {
        let mode = match event["mode"].as_str() {
            Some("transcribe") => Mode::Transcribe,
            Some("assistant") => Mode::Assistant,
            _ => return,
        };
        let configured = {
            let state = app.state::<AppState>();
            let settings = state.settings.lock().unwrap();
            if mode == Mode::Transcribe {
                settings.shortcuts.transcribe.clone()
            } else {
                settings.shortcuts.assistant.clone()
            }
        };
        if !crate::shortcuts::is_hold(&configured) || event["key"] != configured {
            return;
        }
        match event["phase"].as_str() {
            Some("hold_pressed") => pressed(app, mode, configured, String::new()),
            Some("hold_released") => released(app, mode),
            _ => cancel(app),
        }
        return;
    }
    if event["phase"] == "hold_status" {
        let state = app.state::<AssistantState>();
        let mut inner = state.inner.lock().unwrap();
        inner.hold_error = event["message"].as_str().unwrap_or("").to_string();
        if !inner.availability.is_object() {
            inner.availability = json!({});
        }
        inner.availability["holdError"] = json!(inner.hold_error);
        let _ = app.emit("texthalo:assistant-availability", &inner.availability);
        return;
    }
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if event["phase"] == "availability" {
        event["holdError"] = json!(inner.hold_error);
        cloud_availability(app, &mut event);
        let preferences = app
            .state::<AppState>()
            .settings
            .lock()
            .unwrap()
            .assistant
            .clone();
        event["whisperLanguages"] = crate::whisper_core::languages(&preferences.whisper_model);
        event["whisperReady"] = json!(crate::whisper_models::installed(
            app,
            &preferences.whisper_model
        ));
        inner.availability = event.clone();
        let _ = app.emit("texthalo:assistant-availability", event);
        return;
    }
    let Some(id) = event["id"].as_u64() else {
        return;
    };
    if !inner.job.current(id) {
        return;
    }
    if event.get("mode").is_none() {
        if let Some(mode) = inner.mode {
            event["mode"] = json!(mode);
        }
    }
    if let Some(mode) = event["mode"].as_str() {
        inner.mode = match mode {
            "assistant" => Some(Mode::Assistant),
            "transcribe" => Some(Mode::Transcribe),
            _ => inner.mode,
        };
    }
    if event["phase"] == "generation_input" {
        if inner.provider != "cloud" || inner.cloud_started == Some(id) {
            return;
        }
        inner.cloud_started = Some(id);
        let request = uuid::Uuid::new_v4().to_string();
        let (cancel, receiver) = tokio::sync::watch::channel(false);
        inner.billing_request = Some((id, request.clone()));
        inner.cloud = Some((id, request.clone(), cancel));
        let app = app.clone();
        tauri::async_runtime::spawn(crate::hosted_assistant::run(
            app, id, request, event, receiver,
        ));
        return;
    }
    if inner.status["phase"] == "error"
        && inner.status["assistantSpeech"] == true
        && (event["phase"] == "streaming" || event["action"] == "speak")
    {
        return;
    }
    if event["phase"] == "transcript" {
        if let Some(text) = event["text"].as_str().filter(|t| !t.trim().is_empty()) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let saved = history_path(app).and_then(|path| {
                crate::transcription_history::append(
                    &path,
                    crate::transcription_history::Entry {
                        id: format!("{now}-{id}"),
                        created_at: now,
                        text: text.to_string(),
                        source: event["source"].as_str().unwrap_or("").to_string(),
                    },
                )
            });
            let _ = app.emit(
                "texthalo:transcription-history",
                json!({"error":saved.err()}),
            );
        }
        return;
    }
    if event["phase"] == "error" {
        crate::whisper::cancel(app, Some(id));
        cancel_cloud(&mut inner);
        stop_assistant_speech(app, &mut inner);
    }
    if event["phase"] == "canceled" {
        crate::whisper::cancel(app, Some(id));
        cancel_cloud(&mut inner);
        stop_assistant_speech(app, &mut inner);
        inner.job.cancel();
    } else if matches!(event["phase"].as_str(), Some("result" | "error")) {
        inner.job.finish(id);
    }
    if matches!(event["phase"].as_str(), Some("streaming" | "result")) && event["action"] == "speak"
    {
        if let Some(text) = event["text"].as_str().filter(|s| !s.trim().is_empty()) {
            if inner.spoke != Some(id) {
                let settings = inner
                    .speech_settings
                    .clone()
                    .unwrap_or_else(|| app.state::<AppState>().settings.lock().unwrap().clone());
                let source = capture::SourceMetadata {
                    app_name: event["source"].as_str().map(str::to_owned),
                    window_title: event["windowTitle"].as_str().map(str::to_owned),
                    ..Default::default()
                };
                let (speech_id, feed) = begin_generated_stream(app, settings, source);
                inner.spoke = Some(id);
                inner.speech_id = Some(speech_id);
                inner.speech_feed = Some(feed);
            }
            if let Some(feed) = inner.speech_feed.as_ref() {
                feed.send_replace((text.to_string(), event["phase"] == "result"));
            }
            // Retain the mailbox until cancellation/replacement: dropping its sender
            // here can race the feeder reading the final snapshot.
        }
    }
    if event["action"] == "speak" && matches!(event["phase"].as_str(), Some("streaming" | "result"))
    {
        let state = app.state::<AppState>();
        let job = state.job.lock().unwrap();
        if inner.speech_id == Some(job.id) {
            event = audio_presentation(&event, &job.status.phase, job.status.message.as_deref());
        }
    }
    event["hostedSpeech"] = json!(
        app.state::<AppState>().settings.lock().unwrap().engine == crate::config::Engine::Fish
    );
    event["provider"] = json!(inner.provider);
    event["usage"] = inner.usage.clone();
    preserve_preview(&inner.status, &mut event);
    inner.status = event;
    publish(app, &inner.status);
}

fn speak_generated(app: &AppHandle, text: String) -> u64 {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let speech_id = crate::begin_speech(app, Phase::Preparing);
    let app = app.clone();
    std::thread::spawn(move || {
        crate::run_speech(
            &app,
            speech_id,
            settings,
            text,
            false,
            true,
            capture::SourceMetadata::default(),
        )
    });
    speech_id
}
/// A watch mailbox coalesces draft snapshots while the bounded synthesis input is
/// busy. The sidecar reader never waits on playback or holds its mutex for backpressure.
fn begin_generated_stream(
    app: &AppHandle,
    settings: Settings,
    source: capture::SourceMetadata,
) -> (u64, tokio::sync::watch::Sender<(String, bool)>) {
    let speech_id = crate::begin_speech(app, Phase::Preparing);
    let (feed, latest) = tokio::sync::watch::channel((String::new(), false));
    let (tx, input) = crate::speech_input::channel();
    let worker_app = app.clone();
    std::thread::spawn(move || {
        crate::run_speech_input(&worker_app, speech_id, settings, input, true, source)
    });
    let app = app.clone();
    std::thread::spawn(move || {
        let cancelled = || {
            let state = app.state::<AppState>();
            let job = state.job.lock().unwrap();
            !job.is_current(speech_id) || matches!(job.status.phase, Phase::Idle | Phase::Error)
        };
        if let Err(error) = crate::assistant_stream::forward_snapshots(latest, tx, cancelled) {
            let state = app.state::<AppState>();
            let mut job = state.job.lock().unwrap();
            if job.is_current(speech_id) && !matches!(job.status.phase, Phase::Idle | Phase::Error)
            {
                // Abort audible/queued output immediately; the overlay tick halts
                // the native generator while preserving its copyable partial answer.
                state.spoken.stop();
                job.streaming = false;
                let chars = job.status.chars;
                job.set(Phase::Error, Some(error), chars);
                let _ = app.emit("kiegen:status", &job.status);
            }
        }
    });
    (speech_id, feed)
}

fn cancel_speech_lease(
    ownership: &mut Option<u64>,
    speech: &mut crate::speech_job::SpeechJob,
) -> bool {
    if let Some(id) = ownership.take() {
        if speech.is_current(id) {
            speech.cancel();
            return true;
        }
    }
    false
}
fn stop_assistant_speech(app: &AppHandle, inner: &mut Inner) {
    inner.speech_feed = None;
    let state = app.state::<AppState>();
    let mut speech = state.job.lock().unwrap();
    if cancel_speech_lease(&mut inner.speech_id, &mut speech) {
        state.spoken.stop();
        let _ = app.emit("kiegen:status", &speech.status);
    }
}

fn configuration(command: &str, id: u64, settings: &Settings) -> Value {
    json!({"command":command,"id":id,"language":settings.assistant.language,
        "transcriptionEngine":settings.assistant.transcription_engine,"instruction":settings.assistant.instruction,
        "output":settings.assistant.dictation_output,"restoreClipboard":settings.restore_clipboard})
}

pub fn pressed(app: &AppHandle, mode: Mode, key: String, modifiers: String) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    let Some(id) = inner.job.press(mode) else {
        return;
    };
    crate::whisper::cancel(app, None);
    cancel_cloud(&mut inner);
    inner.usage = Value::Null;
    // Prevent existing narration being captured as the user's request.
    inner.speech_feed = None;
    inner.speech_id = None;
    crate::stop_speaking(app.clone());
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    inner.speech_settings = Some(settings.clone());
    inner.mode = Some(mode);
    let mut command = configuration("start", id, &settings);
    if mode == Mode::Assistant
        && settings.assistant.provider == "automatic"
        && inner.availability["model"].as_str().is_none()
    {
        let _ = send(app, &mut inner, json!({"command":"status","id":0}));
    }
    let provider = if mode == Mode::Assistant {
        resolve_provider(&settings, &inner.availability)
    } else {
        Ok("local")
    };
    let provider = match provider {
        Ok(p) => p,
        Err(message) => {
            crate::whisper::cancel(app, Some(id));
            inner.job.finish(id);
            inner.status = json!({"id":id,"phase":"error","mode":mode,"message":message});
            publish(app, &inner.status);
            return;
        }
    };
    if settings.assistant.transcription_engine == "whisper" {
        if let Err(message) = crate::whisper::start(app, id, &settings.assistant) {
            inner.job.finish(id);
            inner.status = json!({"id":id,"phase":"error","mode":mode,"message":message});
            publish(app, &inner.status);
            return;
        }
    }
    inner.provider = provider.into();
    command["provider"] = json!(provider);
    command["mode"] = json!(mode);
    command["key"] = json!(key);
    command["modifiers"] = json!(modifiers);
    inner.status = json!({"id":id,"phase":"processing","mode":mode,"message":"Starting microphone…","text":"","provider":provider});
    // Wait for the native context snapshot before showing any new UI.
    if let Err(error) = send(app, &mut inner, command) {
        crate::whisper::cancel(app, Some(id));
        inner.job.finish(id);
        inner.status =
            json!({"id":id,"phase":"error","mode":mode,"provider":provider,"message":error});
        publish(app, &inner.status);
    }
}
pub fn released(app: &AppHandle, mode: Mode) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if let Some(id) = inner.job.release(mode) {
        if let Err(error) = send(app, &mut inner, json!({"command":"stop","id":id})) {
            inner.status = json!({"id":id,"phase":"error","mode":mode,"provider":inner.provider,"message":error});
            publish(app, &inner.status);
        }
    }
}
pub fn cancel(app: &AppHandle) {
    let Some(state) = app.try_state::<AssistantState>() else {
        return;
    };
    let mut inner = state.inner.lock().unwrap();
    cancel_locked(app, &mut inner);
}
fn cancel_locked(app: &AppHandle, inner: &mut Inner) {
    crate::whisper::cancel(app, None);
    cancel_cloud(inner);
    inner.job.cancel();
    stop_assistant_speech(app, inner);
    inner.status = json!({"phase":"idle","text":""});
    if inner.native.is_some() {
        let restore = app
            .get_webview_window("assistant")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        let _ = send(
            app,
            inner,
            json!({"command":"cancel","id":0,"restore":restore}),
        );
    }
    publish(app, &inner.status);
}
pub fn shutdown(app: &AppHandle) {
    crate::whisper::shutdown(app);
    if let Some(state) = app.try_state::<AssistantState>() {
        let mut inner = state.inner.lock().unwrap();
        cancel_cloud(&mut inner);
        inner.job.cancel();
        inner.native = None;
    }
}

#[tauri::command]
pub fn assistant_state(app: AppHandle) -> Value {
    let state = app.state::<AssistantState>();
    let inner = state.inner.lock().unwrap();
    let mut availability = inner.availability.clone();
    cloud_availability(&app, &mut availability);
    let preferences = app
        .state::<AppState>()
        .settings
        .lock()
        .unwrap()
        .assistant
        .clone();
    availability["whisperLanguages"] = crate::whisper_core::languages(&preferences.whisper_model);
    availability["whisperReady"] = json!(crate::whisper_models::installed(
        &app,
        &preferences.whisper_model
    ));
    json!({"status":inner.status,"availability":availability})
}

pub(crate) fn assistant_error_overlay_height(app: &AppHandle) -> Option<f64> {
    let state = app.try_state::<AssistantState>()?;
    let inner = state.inner.try_lock().ok()?;
    is_assistant_error(&inner.status).then(|| assistant_error_height(&inner.status))
}

fn assistant_error_height(status: &Value) -> f64 {
    if status["text"].as_str().is_some_and(|text| !text.is_empty()) {
        360.0
    } else {
        190.0
    }
}

fn is_assistant_error(status: &Value) -> bool {
    status["phase"] == "error" && status["mode"] == "assistant"
}
fn result_id(status: &Value, request_id: Option<u64>) -> Result<u64, String> {
    let id = status["id"]
        .as_u64()
        .ok_or("No assistant result available")?;
    if request_id != Some(id) {
        return Err("This result is no longer current".into());
    }
    let recoverable_error =
        status["phase"] == "error" && status["text"].as_str().is_some_and(|s| !s.is_empty());
    if status["phase"] != "result" && !recoverable_error {
        return Err("Wait for the current request or cancel it first.".into());
    }
    Ok(id)
}
#[tauri::command]
pub fn assistant_control(
    app: AppHandle,
    command: String,
    text: Option<String>,
    context: Option<String>,
    request_id: Option<u64>,
    preview_height: Option<f64>,
) -> Result<(), String> {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if command == "resize_preview" {
        set_preview_height(&mut inner.status, request_id, preview_height)?;
        publish(&app, &inner.status);
        return Ok(());
    }
    if command == "expand_preview" || command == "collapse_preview" {
        set_preview_expanded(&mut inner.status, request_id, command == "expand_preview")?;
        publish(&app, &inner.status);
        return Ok(());
    }
    if command == "cancel" {
        if request_id.is_some_and(|id| !inner.job.current(id)) {
            return Err("This result is no longer current".into());
        }
        cancel_locked(&app, &mut inner);
        return Ok(());
    }
    if matches!(
        command.as_str(),
        "status"
            | "permissions"
            | "permission_microphone"
            | "permission_speech"
            | "permission_screen"
            | "settings_microphone"
            | "settings_speech"
            | "settings_screen"
            | "hold_suspend"
            | "hold_resume"
    ) {
        return send(&app, &mut inner, json!({"command":command,"id":0}));
    }
    if command == "ask" || command == "open" {
        let text = if command == "ask" {
            text.filter(|s| !s.trim().is_empty())
                .ok_or("Enter a request first")?
        } else {
            String::new()
        };
        if text.chars().count() > 4000 {
            return Err("Use a shorter request (up to 4,000 characters).".into());
        }
        cancel_cloud(&mut inner);
        stop_assistant_speech(&app, &mut inner);
        inner.job.cancel();
        inner.mode = Some(Mode::Assistant);
        let id = inner.job.press(Mode::Assistant).unwrap();
        inner.job.release(Mode::Assistant);
        let settings = app.state::<AppState>().settings.lock().unwrap().clone();
        inner.speech_settings = Some(settings.clone());
        if settings.assistant.provider == "automatic"
            && inner.availability["model"].as_str().is_none()
        {
            let _ = send(&app, &mut inner, json!({"command":"status","id":0}));
        }
        let provider = match resolve_provider(&settings, &inner.availability) {
            Ok(provider) => provider,
            Err(message) => {
                inner.status =
                    json!({"id":id,"phase":"error","mode":"assistant","message":message});
                publish(&app, &inner.status);
                return Ok(());
            }
        };
        inner.provider = provider.into();
        inner.usage = Value::Null;
        let mut value = configuration(&command, id, &settings);
        value["provider"] = json!(provider);
        value["text"] = json!(text);
        inner.status = json!({"id":id,"phase":"processing","mode":"assistant","message":if provider == "cloud" { "Thinking with cloud Assistant…" } else { "Thinking on this Mac…" },"provider":provider});
        publish(&app, &inner.status);
        if let Err(message) = send(&app, &mut inner, value) {
            inner.status = json!({"id":id,"phase":"error","mode":"assistant","provider":provider,"message":message});
            publish(&app, &inner.status);
        }
        return Ok(());
    }
    let id = result_id(&inner.status, request_id)?;
    match command.as_str() {
        "speak" => {
            let text = inner.status["text"]
                .as_str()
                .filter(|t| !t.is_empty())
                .ok_or("No text to speak")?;
            let text = text.to_string();
            inner.speech_id = Some(speak_generated(&app, text));
            inner.status["action"] = json!("speak");
            Ok(())
        }
        "copy" | "undo" | "screen" => {
            if command == "screen" {
                if !matches!(context.as_deref(), Some("window" | "region")) {
                    return Err("Choose window or region capture".into());
                }
                if let Some(window) = app.get_webview_window("assistant") {
                    let _ = window.hide();
                }
            }
            let restore = command == "undo"
                && app
                    .get_webview_window("assistant")
                    .is_some_and(|w| w.is_focused().unwrap_or(false));
            send(
                &app,
                &mut inner,
                json!({"command":command,"id":id,"restore":restore,"context":context}),
            )
        }
        _ => Err("Unknown assistant action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_never_assumes_cloud_before_native_availability() {
        let mut settings = Settings::default();
        settings.assistant.provider = "automatic".into();
        settings.assistant.cloud_consent_version = Some(1);
        assert!(resolve_provider(&settings, &json!({}))
            .unwrap_err()
            .contains("Checking"));
    }
    #[test]
    fn finalized_billing_cannot_be_downgraded_by_late_reconciliation() {
        assert!(!billing_update_allowed(
            &json!({"billingStatus":"settled"}),
            &json!({"billingStatus":"reserved"})
        ));
        assert!(!billing_update_allowed(
            &json!({"billingStatus":"released"}),
            &json!({"billingStatus":"pending"})
        ));
        assert!(billing_update_allowed(
            &json!({"billingStatus":"reserved"}),
            &json!({"billingStatus":"settled"})
        ));
    }
    #[test]
    fn stopped_or_replaced_narration_loses_only_its_own_overlay() {
        assert_eq!(narration_ownership(Some(4), 4), NarrationOwnership::Current);
        assert_eq!(
            narration_ownership(Some(4), 5),
            NarrationOwnership::Replaced
        );
        assert_eq!(narration_ownership(None, 5), NarrationOwnership::None);
    }
    #[test]
    fn recovery_copy_does_not_resurrect_the_speech_error() {
        let copied = json!({"id":7,"phase":"result","action":"preview","text":"Partial answer", "message":"Copied."});
        let recovery = audio_presentation(&copied, &Phase::Error, Some("Old speech error"));
        assert_eq!(recovery["phase"], "result");
        assert_eq!(recovery["message"], "Copied.");
        assert_eq!(recovery["assistantSpeech"], true);
    }
    #[test]
    fn owned_audio_uses_one_widget_through_generation_playback_and_completion() {
        let streaming = json!({"id":7,"phase":"streaming","action":"speak","text":"Hello."});
        let speaking = audio_presentation(&streaming, &Phase::Speaking, None);
        assert_eq!(speaking["phase"], "streaming");
        assert_eq!(speaking["message"], "Speaking · generating…");
        let final_text = json!({"id":7,"phase":"result","action":"speak","text":"Hello."});
        let playing = audio_presentation(&final_text, &Phase::Speaking, None);
        assert_eq!(playing["phase"], "speaking");
        assert_eq!(playing["text"], "Hello.");
        assert!(presentation(&playing).show);
        let done = audio_presentation(&playing, &Phase::Idle, None);
        assert!(!presentation(&done).show);
        let failed = audio_presentation(&streaming, &Phase::Error, Some("Network interrupted"));
        assert_eq!(failed["phase"], "error");
        assert_eq!(failed["text"], "Hello.");
        assert!(presentation(&failed).show);
    }
    #[test]
    fn live_preview_keeps_the_assistant_visible_without_a_focusable_result() {
        let recording = presentation(&json!({"phase":"recording","text":"Hello there"}));
        assert!(recording.show && recording.compact && recording.height == 76.0);
        let audio = presentation(
            &json!({"phase":"streaming","action":"speak","text":"A narrated answer."}),
        );
        assert!(audio.show && audio.compact && audio.height == 76.0);
    }
    #[test]
    fn live_preview_expands_without_changing_the_recording_or_generation() {
        let mut status = json!({"id":7,"phase":"recording","text":"First passage"});
        set_preview_expanded(&mut status, Some(7), true).unwrap();
        assert_eq!(status["phase"], "recording");
        assert_eq!(presentation(&status).height, 76.0);
        set_preview_height(&mut status, Some(7), Some(112.0)).unwrap();
        assert_eq!(presentation(&status).height, 112.0);
        let mut update = json!({"id":7,"phase":"streaming","text":"An answer keeps arriving"});
        preserve_preview(&status, &mut update);
        assert_eq!(presentation(&update).height, 112.0);
        set_preview_expanded(&mut update, Some(7), false).unwrap();
        assert_eq!(presentation(&update).height, 76.0);
        assert!(update.get("previewHeight").is_none());
        assert!(set_preview_height(&mut update, Some(7), Some(112.0)).is_err());
        assert!(set_preview_expanded(&mut update, Some(6), true).is_err());
        assert!(set_preview_expanded(&mut update, None, true).is_err());
        let mut replacement = json!({"id":8,"phase":"recording","text":"New request"});
        preserve_preview(&status, &mut replacement);
        assert_eq!(presentation(&replacement).height, 76.0);
        replacement["phase"] = json!("result");
        assert!(set_preview_expanded(&mut replacement, Some(8), true).is_err());
    }
    #[test]
    fn expanded_preview_resizing_is_scoped_and_bounded() {
        let mut status =
            json!({"id":7,"phase":"recording","text":"Growing text","previewExpanded":true});
        for height in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(5000.0),
        ] {
            assert!(set_preview_height(&mut status, Some(7), height).is_err());
        }
        assert!(set_preview_height(&mut status, Some(6), Some(100.0)).is_err());
        set_preview_height(&mut status, Some(7), Some(130.4)).unwrap();
        assert_eq!(presentation(&status).height, 131.0);
        assert_eq!(status["text"], "Growing text");
        status["phase"] = json!("result");
        assert!(set_preview_height(&mut status, Some(7), Some(100.0)).is_err());
    }
    #[test]
    fn generation_configuration_preserves_user_preferences_without_a_context_mode() {
        let mut settings = Settings::default();
        settings.assistant.instruction = "Keep replies brief and friendly".into();
        for command in ["start", "ask"] {
            let payload = configuration(command, 12, &settings);
            assert_eq!(payload["instruction"], "Keep replies brief and friendly");
            assert!(payload.get("context").is_none());
            assert_eq!(payload["id"], 12);
        }
    }
    #[test]
    fn hold_configuration_includes_the_native_request_envelope() {
        let mut settings = Settings::default();
        settings.shortcuts.transcribe = "Hold:LeftOption".into();
        settings.shortcuts.assistant = "Ctrl+D".into();
        let payload = hold_configuration(&settings);
        assert_eq!(
            payload["id"],
            json!(0),
            "Swift Request requires id even for configuration commands"
        );
        assert_eq!(payload["command"], "hold_configure");
        assert_eq!(
            payload["holdBindings"],
            json!({"transcribe":"Hold:LeftOption"})
        );
    }
    #[test]
    fn completed_insertion_closes_the_overlay() {
        let applied =
            presentation(&json!({"phase":"result","action":"applied","message":"Text inserted"}));
        assert!(!applied.show);
        let processing = presentation(&json!({"phase":"processing","message":"Inserting text…"}));
        assert!(processing.show && processing.compact && processing.height == 44.0);
        let fallback = presentation(
            &json!({"phase":"result","text":"Keep this text","message":"Could not insert"}),
        );
        assert!(fallback.show && !fallback.compact && fallback.height == 360.0);
    }
    #[test]
    fn failed_partial_answer_can_be_copied_or_undone_but_empty_error_cannot() {
        let partial = json!({"id":7,"phase":"error","mode":"assistant","text":"Partial answer"});
        assert_eq!(result_id(&partial, Some(7)).unwrap(), 7);
        assert!(is_assistant_error(&partial));
        assert_eq!(assistant_error_height(&partial), 360.0);
        assert_eq!(
            assistant_error_height(&json!({"phase":"error","mode":"assistant"})),
            190.0
        );
        assert!(result_id(&json!({"id":7,"phase":"error"}), Some(7)).is_err());
        assert!(result_id(&json!({"id":7,"phase":"error","text":"Partial"}), Some(6)).is_err());
    }
    #[test]
    fn stale_or_busy_results_cannot_be_undone_or_copied() {
        let ready = json!({"id":7,"phase":"result"});
        assert_eq!(result_id(&ready, Some(7)).unwrap(), 7);
        assert!(result_id(&ready, Some(6)).is_err());
        assert!(result_id(&ready, None).is_err());
        assert!(result_id(&json!({"id":7,"phase":"processing"}), Some(7)).is_err());
        assert!(result_id(&json!({"id":7,"phase":"idle"}), Some(7)).is_err());
    }
    // Losing narration ownership would allow a canceled assistant result to play later.
    #[test]
    fn cancel_invalidates_owned_speech_but_not_newer_selection_speech() {
        let mut speech = crate::speech_job::SpeechJob::default();
        let owned = speech.begin(Phase::Preparing);
        let mut ownership = Some(owned);
        assert!(cancel_speech_lease(&mut ownership, &mut speech));
        assert!(!speech.is_current(owned));
        assert!(ownership.is_none());
        let old = speech.begin(Phase::Preparing);
        let newer = speech.begin(Phase::Capturing);
        ownership = Some(old);
        assert!(!cancel_speech_lease(&mut ownership, &mut speech));
        assert!(speech.is_current(newer));
    }
}

fn history_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("transcription-history.json"))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn transcription_history(
    app: AppHandle,
    command: String,
    id: Option<String>,
) -> Result<Vec<crate::transcription_history::Entry>, String> {
    let state = app.state::<AssistantState>();
    let _guard = state.inner.lock().unwrap();
    let path = history_path(&app)?;
    match command.as_str() {
        "list" => (),
        "delete" => crate::transcription_history::delete(
            &path,
            Some(id.as_deref().ok_or("Missing transcript ID")?),
        )?,
        "clear" => crate::transcription_history::delete(&path, None)?,
        _ => return Err("Unknown history action".into()),
    }
    crate::transcription_history::list(&path)
}

fn resolve_provider(settings: &Settings, availability: &Value) -> Result<&'static str, String> {
    if settings.assistant.provider == "automatic" && availability["model"].as_str().is_none() {
        return Err(
            "Checking Apple Intelligence availability. Try the voice shortcut again in a moment."
                .into(),
        );
    }
    crate::hosted_assistant::resolve(
        &settings.assistant.provider,
        settings.assistant.cloud_consent_version,
        availability["model"] == "available",
        crate::hosted::service_configured(),
        crate::hosted::is_signed_in(),
    )
}
fn cloud_availability(app: &AppHandle, value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
    value["cloudConfigured"] = json!(crate::hosted::service_configured());
    value["cloudSignedIn"] = json!(crate::hosted::is_signed_in());
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap();
    value["provider"] = json!(settings.assistant.provider);
    value["effectiveProvider"] = json!(resolve_provider(&settings, value).ok());
}
fn cancel_cloud(inner: &mut Inner) {
    if let Some((_, _, cancel)) = inner.cloud.take() {
        cancel.send_replace(true);
    }
}
pub(crate) fn hosted_delivery(
    app: &AppHandle,
    id: u64,
    request: &str,
    command: &str,
    action: &str,
    text: &str,
    message: Option<&str>,
) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if !inner.job.current(id)
        || !inner
            .cloud
            .as_ref()
            .is_some_and(|(owned, key, _)| *owned == id && key == request)
    {
        return;
    }
    // Never restart a dead helper for a network completion.
    if inner
        .native
        .as_mut()
        .is_none_or(|native| native.child.try_wait().ok().flatten().is_some())
    {
        cancel_cloud(&mut inner);
        return;
    }
    let payload = json!({"command":command,"id":id,"action":action,"text":text,"message":message});
    if let Err(message) = send(app, &mut inner, payload) {
        cancel_cloud(&mut inner);
        inner.job.cancel();
        inner.status = json!({"id":id,"phase":"error","mode":"assistant","message":message,"text":text,"provider":"cloud"});
        publish(app, &inner.status);
    }
}
fn billing_update_allowed(previous: &Value, next: &Value) -> bool {
    !matches!(
        previous["billingStatus"].as_str(),
        Some("settled" | "released")
    ) || matches!(next["billingStatus"].as_str(), Some("settled" | "released"))
}
pub(crate) fn hosted_usage(app: &AppHandle, id: u64, request: &str, usage: Value) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    let owns_billing = inner
        .billing_request
        .as_ref()
        .is_some_and(|(owned, key)| *owned == id && key == request);
    if owns_billing && !billing_update_allowed(&inner.usage, &usage) {
        return;
    }
    let _ = app.emit(
        "texthalo:assistant-usage",
        json!({"id":id,"requestId":request,"usage":usage}),
    );
    // Billing updates can outlive generation, but cannot revive or replace the presentation.
    if inner.status["id"] != id || !owns_billing {
        return;
    }
    inner.usage = usage.clone();
    inner.status["usage"] = usage;
    let _ = app.emit("texthalo:assistant", &inner.status);
}
