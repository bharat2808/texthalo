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
    native: Option<Native>,
    job: HoldJob,
    status: Value,
    availability: Value,
    spoke: Option<u64>,
    speech_id: Option<u64>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AssistantState::default());
    WebviewWindowBuilder::new(
        app,
        "assistant",
        WebviewUrl::App("index.html?assistant".into()),
    )
    .title("TextHalo Assistant")
    .inner_size(460.0, 490.0)
    .min_inner_size(360.0, 300.0)
    .decorations(true)
    .resizable(true)
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
        let input = child.stdin.take().ok_or("Native input unavailable")?;
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

fn publish(app: &AppHandle, status: &Value) {
    let _ = app.emit("texthalo:assistant", status);
    let show = !matches!(
        status["phase"].as_str(),
        Some("idle" | "canceled" | "capturing_screen")
    );
    let anchor = status.get("bounds").cloned();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window("assistant") {
            if show {
                if !window.is_visible().unwrap_or(false) {
                    position_panel(&window, anchor.as_ref());
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
fn position_panel(window: &tauri::WebviewWindow, anchor: Option<&Value>) {
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
                    .min(origin.x + (size.width - 472.0).max(12.0));
                let panel_y = (y + anchor["height"].as_f64().unwrap_or(0.0) + 8.0)
                    .max(origin.y + 32.0)
                    .min(origin.y + (size.height - 510.0).max(32.0));
                let _ = window.set_position(tauri::LogicalPosition::new(panel_x, panel_y));
                return;
            }
        }
    }
    let _ = window.center();
}

fn receive(app: &AppHandle, mut event: Value) {
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if event["phase"] == "availability" {
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
        "output":settings.assistant.dictation_output})
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
    inner.job.cancel();
    stop_assistant_speech(app, &mut inner);
    inner.status = json!({"phase":"idle","text":""});
    if inner.native.is_some() {
        let restore = app
            .get_webview_window("assistant")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        let _ = send(
            app,
            &mut inner,
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
#[tauri::command]
pub fn assistant_control(
    app: AppHandle,
    command: String,
    text: Option<String>,
    context: Option<String>,
) -> Result<(), String> {
    if command == "cancel" {
        cancel(&app);
        return Ok(());
    }
    let state = app.state::<AssistantState>();
    let mut inner = state.inner.lock().unwrap();
    if matches!(command.as_str(), "status" | "permissions" | "diagnose") {
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
    let id = inner.status["id"]
        .as_u64()
        .ok_or("No assistant result available")?;
    if matches!(
        inner.status["phase"].as_str(),
        Some("recording" | "processing" | "capturing_screen")
    ) {
        return Err("Wait for the current request or cancel it first.".into());
    }
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
        "copy" | "insert" | "screen" => {
            if command == "screen" {
                if !matches!(context.as_deref(), Some("window" | "region")) {
                    return Err("Choose window or region capture".into());
                }
                if let Some(window) = app.get_webview_window("assistant") {
                    let _ = window.hide();
                }
            }
            send(
                &app,
                &mut inner,
                json!({"command":command,"id":id,"restore":command == "insert","context":context}),
            )
        }
        _ => Err("Unknown assistant action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
