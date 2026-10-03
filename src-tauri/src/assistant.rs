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
    native: Option<Native>,
    job: HoldJob,
    status: Value,
    availability: Value,
    spoke: Option<u64>,
    speech_id: Option<u64>,
}

fn hold_configuration(settings: &Settings) -> Value {
    let mut bindings = serde_json::Map::new();
    for (mode, key) in [("transcribe", &settings.shortcuts.transcribe), ("assistant", &settings.shortcuts.assistant)] {
        if crate::shortcuts::is_hold(key) { bindings.insert(mode.into(), json!(key)); }
    }
    json!({"command":"hold_configure", "id":0, "holdBindings":bindings})
}

pub fn configure_holds(app: &AppHandle) -> Result<(), String> {
    let value = hold_configuration(&app.state::<AppState>().settings.lock().unwrap());
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    // Do not launch a sidecar solely for an empty hold configuration.
    if inner.native.is_none() && value["holdBindings"].as_object().is_some_and(|b| b.is_empty()) { return Ok(()); }
    send(app, &mut inner, value)
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AssistantState::default());
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
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start native assistant: {e}"))?;
        let mut input = child.stdin.take().ok_or("Native input unavailable")?;
        let holds = hold_configuration(&app.state::<AppState>().settings.lock().unwrap());
        writeln!(input, "{holds}").and_then(|_| input.flush()).map_err(|e| e.to_string())?;
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
                inner.job.cancel();
                inner.status = json!({"phase":"error","message":"The native assistant stopped. Hold the shortcut again to retry."});
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

fn presentation(status: &Value) -> Presentation {
    let show = !matches!(status["phase"].as_str(), Some("idle" | "canceled"))
        && !matches!(status["action"].as_str(), Some("speak" | "applied"));
    let compact = matches!(
        status["phase"].as_str(),
        Some("recording" | "processing" | "capturing_screen")
    ) || matches!(
        status["action"].as_str(),
        Some("copied" | "undone")
    );
    let expanded =
        status["text"].as_str().is_some_and(|s| !s.is_empty()) && status["action"] != "applied";
    let height = if compact {
        44.0
    } else if expanded {
        360.0
    } else {
        140.0
    };
    Presentation { show, compact, height }
}

fn publish(app: &AppHandle, status: &Value) {
    let _ = app.emit("texthalo:assistant", status);
    let Presentation { show, compact, height } = presentation(status);
    if matches!(
        status["action"].as_str(),
        Some("copied" | "undone")
    ) {
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
    let anchor = status.get("bounds").cloned();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window("assistant") {
            if show {
                let _ = window.set_focusable(!compact);
                if compact {
                    crate::overlay::position(&window, false);
                } else {
                    let _ = window.set_size(tauri::LogicalSize::new(380.0, height));
                    position_panel(&window, anchor.as_ref(), height);
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

fn receive(app: &AppHandle, mut event: Value) {
    if matches!(event["phase"].as_str(), Some("hold_pressed" | "hold_released" | "hold_cancel")) {
        let mode = match event["mode"].as_str() {
            Some("transcribe") => Mode::Transcribe,
            Some("assistant") => Mode::Assistant,
            _ => return,
        };
        let configured = {
            let state = app.state::<AppState>();
            let settings = state.settings.lock().unwrap();
            if mode == Mode::Transcribe { settings.shortcuts.transcribe.clone() } else { settings.shortcuts.assistant.clone() }
        };
        if !crate::shortcuts::is_hold(&configured) || event["key"] != configured { return; }
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
        if !inner.availability.is_object() { inner.availability = json!({}); }
        inner.availability["holdError"] = json!(inner.hold_error);
        let _ = app.emit("texthalo:assistant-availability", &inner.availability);
        return;
    }
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if event["phase"] == "availability" {
        event["holdError"] = json!(inner.hold_error);
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
    if event["phase"] == "canceled" {
        stop_assistant_speech(app, &mut inner);
        inner.job.cancel();
    } else if matches!(event["phase"].as_str(), Some("result" | "error")) {
        inner.job.finish(id);
    }
    if event["phase"] == "result" && event["action"] == "speak" && inner.spoke != Some(id) {
        if let Some(text) = event["text"].as_str().filter(|s| !s.trim().is_empty()) {
            inner.spoke = Some(id);
            inner.speech_id = Some(speak_generated(app, text.to_string()));
        }
    }
    event["hostedSpeech"] = json!(
        app.state::<AppState>().settings.lock().unwrap().engine == crate::config::Engine::Fish
    );
    inner.status = event;
    publish(app, &inner.status);
}

fn speak_generated(app: &AppHandle, text: String) -> u64 {
    let mut settings = app.state::<AppState>().settings.lock().unwrap().clone();
    settings.fish.enhance_text = false;
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
    let state = app.state::<AppState>();
    let mut speech = state.job.lock().unwrap();
    if cancel_speech_lease(&mut inner.speech_id, &mut speech) {
        state.spoken.stop();
        let _ = app.emit("kiegen:status", &speech.status);
    }
}

fn configuration(command: &str, id: u64, settings: &Settings) -> Value {
    json!({"command":command,"id":id,"language":settings.assistant.language,
        "instruction":settings.assistant.instruction,"context":settings.assistant.context,
        "output":settings.assistant.dictation_output,"restoreClipboard":settings.restore_clipboard})
}

pub fn pressed(app: &AppHandle, mode: Mode, key: String, modifiers: String) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    let Some(id) = inner.job.press(mode) else {
        return;
    };
    // Prevent existing narration being captured as the user's request.
    crate::stop_speaking(app.clone());
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let mut command = configuration("start", id, &settings);
    command["mode"] = json!(mode);
    command["key"] = json!(key);
    command["modifiers"] = json!(modifiers);
    inner.status = json!({"id":id,"phase":"processing","message":"Starting microphone…","text":""});
    // Wait for the native context snapshot before showing any new UI.
    if let Err(error) = send(app, &mut inner, command) {
        inner.job.finish(id);
        inner.status = json!({"id":id,"phase":"error","message":error});
        publish(app, &inner.status);
    }
}
pub fn released(app: &AppHandle, mode: Mode) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if let Some(id) = inner.job.release(mode) {
        if let Err(error) = send(app, &mut inner, json!({"command":"stop","id":id})) {
            inner.status = json!({"id":id,"phase":"error","message":error});
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
    if let Some(state) = app.try_state::<AssistantState>() {
        state.inner.lock().unwrap().native = None;
    }
}

#[tauri::command]
pub fn assistant_state(app: AppHandle) -> Value {
    let state = app.state::<AssistantState>();
    let inner = state.inner.lock().unwrap();
    json!({"status":inner.status,"availability":inner.availability})
}
fn result_id(status: &Value, request_id: Option<u64>) -> Result<u64, String> {
    let id = status["id"]
        .as_u64()
        .ok_or("No assistant result available")?;
    if request_id != Some(id) {
        return Err("This result is no longer current".into());
    }
    if status["phase"] != "result" {
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
) -> Result<(), String> {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
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
            | "diagnose"
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
        stop_assistant_speech(&app, &mut inner);
        inner.job.cancel();
        let id = inner.job.press(Mode::Assistant).unwrap();
        inner.job.release(Mode::Assistant);
        let settings = app.state::<AppState>().settings.lock().unwrap().clone();
        let mut value = configuration(&command, id, &settings);
        value["text"] = json!(text);
        inner.status = json!({"id":id,"phase":"processing","message":"Thinking on this Mac…"});
        publish(&app, &inner.status);
        return send(&app, &mut inner, value);
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
    fn generation_configuration_preserves_user_preferences_and_context_mode() {
        let mut settings = Settings::default();
        settings.assistant.instruction = "Keep replies brief and friendly".into();
        settings.assistant.context = "screen".into();
        for command in ["start", "ask"] {
            let payload = configuration(command, 12, &settings);
            assert_eq!(payload["instruction"], "Keep replies brief and friendly");
            assert_eq!(payload["context"], "screen");
            assert_eq!(payload["id"], 12);
        }
    }
    #[test]
    fn hold_configuration_includes_the_native_request_envelope() {
        let mut settings = Settings::default();
        settings.shortcuts.transcribe = "Hold:LeftOption".into();
        settings.shortcuts.assistant = "Ctrl+D".into();
        let payload = hold_configuration(&settings);
        assert_eq!(payload["id"], json!(0), "Swift Request requires id even for configuration commands");
        assert_eq!(payload["command"], "hold_configure");
        assert_eq!(payload["holdBindings"], json!({"transcribe":"Hold:LeftOption"}));
    }
    #[test]
    fn completed_insertion_closes_the_overlay() {
        let applied = presentation(&json!({"phase":"result","action":"applied","message":"Text inserted"}));
        assert!(!applied.show);
        let processing = presentation(&json!({"phase":"processing","message":"Inserting text…"}));
        assert!(processing.show && processing.compact && processing.height == 44.0);
        let fallback = presentation(&json!({"phase":"result","text":"Keep this text","message":"Could not insert"}));
        assert!(fallback.show && !fallback.compact && fallback.height == 360.0);
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
