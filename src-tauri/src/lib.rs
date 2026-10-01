//! TextHalo — a menu-bar app that speaks selected text.
//!
//! Shape (see docs/DESIGN.md §1): no Dock icon; a first-run setup and permission guidance
//! appear when needed, then the tray is the persistent UI and settings open on demand. The
//! real app is the Rust service below; the webview is a config editor and setup wizard.

mod capture;
pub mod chatterbox;
pub mod config;
pub mod download;
mod engine_locks;
pub mod engine_paths;
pub mod engines;
pub mod espeak;
pub mod g2p;
mod history;
mod hosted;
pub mod kokoro;
pub mod lexicon;
pub mod numbers;
mod overlay;
mod pcm;
mod shortcuts;
mod speech;
mod speech_job;
pub mod spoken;
mod streaming;
mod update_safety;
pub mod voices;

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_updater::UpdaterExt;

use config::Settings;
use shortcuts::Action;
use speech::Voice;

/// Copy-mode capture is bounded: an app that never touches the pasteboard should
/// cost a blink, not a hang.
const COPY_TIMEOUT_MS: u64 = 150;
const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Deep-link to the Accessibility pane in System Settings for users who need to enable
/// the toggle after the one-time AX permission prompt.
const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

pub struct AppState {
    pub settings: Mutex<Settings>,
    job: Mutex<speech_job::SpeechJob>,
    synthesis: engine_locks::EngineLocks,
    /// Every engine the app can speak with, routed by the settings. The Apple path lives
    /// inside it rather than beside it so there is one place that decides what speaks.
    pub spoken: spoken::Spoken,
    pub voices: Vec<Voice>,
    pub bindings: Mutex<Vec<shortcuts::Binding>>,
    update_available_version: Mutex<Option<String>>,
    update_overlay_status: Mutex<UpdateOverlayStatus>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
struct UpdateOverlayStatus {
    visible: bool,
    version: Option<String>,
    installable: bool,
    installing: bool,
    message: Option<String>,
    error: bool,
}

fn history_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("locate app data directory: {e}"))?
        .join("audio-history");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create audio history: {e}"))?;
    Ok(dir)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Capturing,
    Preparing,
    Speaking,
    Error,
}

#[derive(Serialize, Clone)]
struct StatusEvent {
    phase: Phase,
    message: Option<String>,
    chars: Option<usize>,
}

#[derive(Serialize)]
struct UiState {
    settings: Settings,
    voices: Vec<Voice>,
    /// Every engine with its voices and its readiness. The picker renders from this, so
    /// adding an engine never means editing TypeScript.
    engines: Vec<engines::EngineInfo>,
    trusted: bool,
    /// True while a password field has secure input on — a copy-mode capture will
    /// refuse rather than fail mysteriously, and the UI says so up front.
    secure_input: bool,
    speaking: bool,
    refused_shortcuts: Vec<String>,
    /// `xx_YY` from the OS, so the voice browser can open on the user's own language.
    system_language: String,
    /// Shown in Settings so the file is findable without digging through Library.
    config_path: String,
}

fn ui_state(app: &AppHandle, refused_shortcuts: Vec<String>) -> UiState {
    use tauri_plugin_autostart::ManagerExt;
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap().clone();
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        settings.launch_at_login = enabled;
    }
    // Built before the struct literal: the literal moves `settings` into its first field,
    // so borrowing it later in the same expression is a borrow of a moved value.
    let engines = engines::catalog(&settings);
    UiState {
        settings,
        voices: state.voices.clone(),
        engines,
        trusted: capture::is_trusted(),
        secure_input: capture::secure_input_active(),
        speaking: state.spoken.is_speaking(),
        refused_shortcuts,
        system_language: speech::system_language(),
        config_path: config::settings_path(app).display().to_string(),
    }
}

#[derive(Serialize, Clone)]
struct BackgroundUpdateStatus {
    version: Option<String>,
}

fn set_update_menu_status(app: &AppHandle, version: Option<&str>) {
    let version = version.map(str::to_string);
    *app.state::<AppState>()
        .update_available_version
        .lock()
        .unwrap() = version.clone();

    if app.tray_by_id("kiegen").is_some() {
        match build_tray_menu(app, version.as_deref()) {
            Ok(menu) => {
                if let Some(tray) = app.tray_by_id("kiegen") {
                    if let Err(error) = tray.set_menu(Some(menu)) {
                        eprintln!("[TextHalo] could not refresh tray update menu: {error}");
                    }
                }
            }
            Err(error) => eprintln!("[TextHalo] could not rebuild tray update menu: {error}"),
        }
    }
}

#[tauri::command]
fn set_update_menu_status_from_settings(app: AppHandle, version: Option<String>) {
    set_update_menu_status(&app, version.as_deref());
}

#[tauri::command]
fn get_update_overlay_status(app: AppHandle) -> UpdateOverlayStatus {
    app.state::<AppState>()
        .update_overlay_status
        .lock()
        .unwrap()
        .clone()
}

#[tauri::command]
fn publish_update_status(app: AppHandle, mut status: UpdateOverlayStatus) {
    let state = app.state::<AppState>();
    let mut current = state.update_overlay_status.lock().unwrap();
    status.visible = current.visible;
    *current = status.clone();
    drop(current);
    let _ = app.emit("texthalo:update-overlay", status);
}

#[tauri::command]
fn close_update_overlay(app: AppHandle) {
    app.state::<AppState>()
        .update_overlay_status
        .lock()
        .unwrap()
        .visible = false;
    if let Some(window) = app.get_webview_window("update-overlay") {
        let _ = window.hide();
    }
}

async fn check_for_updates_in_background(app: &AppHandle, show_result: bool) {
    if app
        .state::<AppState>()
        .update_overlay_status
        .lock()
        .unwrap()
        .installing
    {
        return;
    }
    match app.updater() {
        Ok(updater) => {
            let result = updater.check().await;
            if app
                .state::<AppState>()
                .update_overlay_status
                .lock()
                .unwrap()
                .installing
            {
                return;
            }
            match result {
                Ok(Some(update)) => {
                    let version = update.version;
                    set_update_menu_status(app, Some(&version));
                    if show_result {
                        overlay::show_update_result(app, Some(version.clone()));
                    }
                    let _ = app.emit(
                        "texthalo:background-update-status",
                        BackgroundUpdateStatus {
                            version: Some(version),
                        },
                    );
                }
                Ok(None) => {
                    set_update_menu_status(app, None);
                    if show_result {
                        overlay::show_update_result(app, None);
                    }
                    let _ = app.emit(
                        "texthalo:background-update-status",
                        BackgroundUpdateStatus { version: None },
                    );
                }
                Err(error) => {
                    eprintln!("[TextHalo] background update check failed: {error}");
                    let _ = update_safety::record_update_failure(
                        app.clone(),
                        "check".into(),
                        error.to_string(),
                    );
                    if show_result {
                        overlay::show_update_error(app, update_safety::check_error(&error));
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("[TextHalo] could not initialize updater: {error}");
            if show_result {
                overlay::show_update_error(app, update_safety::check_error(&error));
            }
        }
    }
}

fn start_background_update_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check_for_updates_in_background(&app, false).await;
            tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
        }
    });
}

#[tauri::command]
fn get_speech_status(app: AppHandle) -> StatusEvent {
    app.state::<AppState>().job.lock().unwrap().status.clone()
}

fn begin_speech(app: &AppHandle, phase: Phase) -> u64 {
    let state = app.state::<AppState>();
    let mut job = state.job.lock().unwrap();
    state.spoken.stop();
    let id = job.begin(phase);
    let _ = app.emit("kiegen:status", &job.status);
    id
}

fn job_status(
    app: &AppHandle,
    id: u64,
    phase: Phase,
    message: Option<String>,
    chars: Option<usize>,
) {
    let state = app.state::<AppState>();
    let mut job = state.job.lock().unwrap();
    if job.is_current(id) {
        job.set(phase, message, chars);
        let _ = app.emit("kiegen:status", &job.status);
    }
}

/// Serialize synthesis per engine, keeping Stop and other engines independent. A canceled
/// render may finish computing; only the current job is allowed to start playback.
fn run_speech(
    app: &AppHandle,
    id: u64,
    settings: Settings,
    text: String,
    truncated: bool,
    archive: bool,
    source: capture::SourceMetadata,
) {
    let state = app.state::<AppState>();
    let _synthesis = state.synthesis.for_engine(settings.engine).lock().unwrap();
    if !state.job.lock().unwrap().is_current(id) {
        return;
    }
    let chars = text.chars().count();
    job_status(app, id, Phase::Preparing, None, Some(chars));
    if let Some(reason) = selected_engine_refusal(&settings) {
        job_status(app, id, Phase::Error, Some(reason), Some(chars));
        return;
    }
    if settings.engine == config::Engine::Fish {
        {
            let mut job = state.job.lock().unwrap();
            if !job.is_current(id) {
                return;
            }
            // Fish Audio can pause between PCM packets while it prepares the next
            // chunk. Keep those playback gaps in Preparing, as for local PCM streams,
            // instead of letting the overlay mistake them for completion.
            job.streaming = true;
        }
        let app = app.clone();
        let voice = settings.fish.voice_id.clone();
        let model_id = settings.fish.model_id.clone();
        let enhance = settings.fish.enhance_text;
        tauri::async_runtime::spawn(async move {
            let result =
                hosted::stream_speech(app.clone(), id, text.clone(), voice, model_id, enhance, archive, source.clone())
                    .await;
            let state = app.state::<AppState>();
            let mut job = state.job.lock().unwrap();
            if job.is_current(id) {
                job.streaming = false;
                match result {
                    Ok(()) => job.set(
                        Phase::Idle,
                        Some("Hosted speech complete".into()),
                        Some(text.chars().count()),
                    ),
                    Err(message) => {
                        state.spoken.stop();
                        job.set(Phase::Error, Some(message), Some(text.chars().count()));
                    }
                }
                let _ = app.emit("kiegen:status", &job.status);
            }
        });
        return;
    }
    if settings.engine == config::Engine::Apple {
        let archive_path = archive
            .then(|| history_dir(app).ok())
            .flatten()
            .and_then(|dir| history::reserve_audio_path(&dir, "aiff").ok());
        let result = {
            let job = state.job.lock().unwrap();
            if !job.is_current(id) {
                return;
            }
            state
                .spoken
                .speak(&settings, &text, archive_path.as_deref())
        };
        match result {
            Ok(report) => {
                job_status(
                    app,
                    id,
                    Phase::Speaking,
                    Some(report.summary()),
                    Some(chars),
                );
                while state.spoken.is_speaking() && state.job.lock().unwrap().is_current(id) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if state.job.lock().unwrap().is_current(id) {
                    if let Some(path) = state.spoken.finish_apple_archive() {
                        if let Ok(dir) = history_dir(app) {
                            if let Err(error) = history::save_entry(
                                &dir,
                                &path,
                                "Apple",
                                settings.voice.as_deref().unwrap_or("System default"),
                                &text,
                                None,
                                &source,
                            ) {
                                eprintln!("[TextHalo] could not save Apple audio history: {error}");
                                let _ = std::fs::remove_file(path);
                            }
                        }
                    }
                    job_status(
                        app,
                        id,
                        Phase::Idle,
                        Some(if truncated {
                            format!("truncated to {} characters", settings.max_chars)
                        } else {
                            report.summary()
                        }),
                        Some(chars),
                    );
                }
            }
            Err(error) => job_status(app, id, Phase::Error, Some(error), Some(chars)),
        }
        return;
    }
    let history_root = archive.then(|| history_dir(app).ok()).flatten();
    let mut recorder = history_root
        .as_deref()
        .and_then(|dir| history::PcmRecorder::new(dir).ok());
    {
        let mut job = state.job.lock().unwrap();
        if !job.is_current(id) {
            return;
        }
        job.streaming = true;
    }
    let result = state
        .spoken
        .stream_pcm(
            &settings,
            &text,
            recorder.as_mut(),
            || !state.job.lock().unwrap().is_current(id),
            |player| {
                let mut job = state.job.lock().unwrap();
                if !job.is_current(id) {
                    return Ok(());
                }
                state.spoken.activate_pcm(player.clone());
                player.start()?;
                job.set(Phase::Speaking, None, Some(chars));
                let _ = app.emit("kiegen:status", &job.status);
                Ok(())
            },
        )
        .map(|(report, _, _)| report);
    let mut job = state.job.lock().unwrap();
    if !job.is_current(id) {
        return;
    }
    job.streaming = false;
    match result {
        Ok(report) => {
            if let (Some(recorder), Some(dir)) = (recorder.take(), history_root.as_deref()) {
                if let Some(path) = recorder.finish() {
                    let (engine, voice) = match settings.engine {
                        config::Engine::Kokoro => ("Kokoro", settings.kokoro.voice.clone()),
                        config::Engine::Chatterbox => {
                            let speaker = settings
                                .chatterbox
                                .ref_audio
                                .as_deref()
                                .unwrap_or("Built-in speaker");
                            (
                                "Chatterbox",
                                format!("{speaker} · {}", settings.chatterbox.voice),
                            )
                        }
                        config::Engine::Apple | config::Engine::Fish => unreachable!(),
                    };
                    if let Err(error) = history::save_entry(
                        dir,
                        &path,
                        engine,
                        &voice,
                        &text,
                        Some(report.seconds as f64),
                        &source,
                    ) {
                        eprintln!("[TextHalo] could not save audio history: {error}");
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            job.set(
                Phase::Idle,
                Some(if truncated {
                    format!("truncated to {} characters", settings.max_chars)
                } else {
                    report.summary()
                }),
                Some(chars),
            )
        }
        Err(error) => {
            state.spoken.stop();
            job.set(Phase::Error, Some(error), Some(chars));
        }
    }
    let _ = app.emit("kiegen:status", &job.status);
}

/// Why the selected engine cannot speak, or `None` if it can.
///
/// A user who picks Kokoro must never hear Samantha and conclude that is what Kokoro
/// sounds like, so there is no fallback here — the shortcut reports the reason instead.
fn selected_engine_refusal(settings: &Settings) -> Option<String> {
    if settings.engine == config::Engine::Apple {
        return None;
    }
    engine_refusal(settings.engine, engines::catalog(settings))
}

fn engine_refusal(engine: config::Engine, catalog: Vec<engines::EngineInfo>) -> Option<String> {
    catalog
        .into_iter()
        .find(|info| info.id == engine)
        .filter(|info| !info.can_speak)
        .map(|info| match info.blocked_reason {
            // The user asked for speech and got silence: give them the reason and the way
            // out, rather than the terse status line the settings pane shows.
            Some(reason) => format!(
                "{}: {reason}. Switch to the Apple system voices to read the selection now.",
                info.label
            ),
            None => format!("{} cannot speak right now.", info.label),
        })
}

/// The whole point of the app: capture → speak. Runs off the main thread because the
/// AX read plus a possible ⌘C round-trip blocks for up to `COPY_TIMEOUT_MS`.
fn speak_selection(app: &AppHandle, id: u64) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if let Some(reason) = selected_engine_refusal(&settings) {
        job_status(app, id, Phase::Error, Some(reason), None);
        return;
    }
    let (text, source) = match capture::capture_with_source(
        settings.capture_mode,
        COPY_TIMEOUT_MS,
        settings.restore_clipboard,
    ) {
        Ok(text) => text,
        Err(error) => {
            job_status(app, id, Phase::Error, Some(error.to_string()), None);
            return;
        }
    };
    let truncated = text.chars().count() > settings.max_chars;
    let text = text.chars().take(settings.max_chars).collect();
    run_speech(app, id, settings, text, truncated, true, source);
}

fn speak_given(app: &AppHandle, id: u64, text: String) {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let truncated = text.chars().count() > settings.max_chars;
    let text = text.chars().take(settings.max_chars).collect();
    run_speech(app, id, settings, text, truncated, true, capture::SourceMetadata::default());
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn show_setup_wizard(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut current = state.settings.lock().unwrap();
    let mut settings = current.clone();
    settings.onboarding_completed = false;
    match config::save(app, &settings) {
        Ok(()) => *current = settings,
        Err(error) => eprintln!("[TextHalo] could not save setup state: {error}"),
    }
    drop(current);
    show_settings(app);
    let _ = app.emit("texthalo:open-setup", ());
}

// ─────────────────────────────── commands ───────────────────────────────

#[tauri::command]
fn get_state(app: AppHandle) -> UiState {
    ui_state(&app, Vec::new())
}

/// Persist, then re-apply shortcuts. Registration failures come back to the UI so it
/// can say "macOS refused Cmd+Shift+S" instead of pretending the chord works.
#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<UiState, String> {
    shortcuts::bindings(&settings)?; // validate before writing anything
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let launch_changed = manager
        .is_enabled()
        .unwrap_or_else(|_| app.state::<AppState>().settings.lock().unwrap().launch_at_login)
        != settings.launch_at_login;
    if launch_changed {
        if settings.launch_at_login {
            manager.enable().map_err(|e| format!("enable launch at login: {e}"))?;
        } else {
            manager.disable().map_err(|e| format!("disable launch at login: {e}"))?;
        }
    }
    config::save(&app, &settings)?;
    let changed_engine = {
        let state = app.state::<AppState>();
        let mut current = state.settings.lock().unwrap();
        let changed = current.engine != settings.engine;
        *current = settings.clone();
        changed
    };
    if changed_engine {
        stop_speaking(app.clone());
    }
    let refused = shortcuts::apply(&app)?;
    Ok(ui_state(&app, refused))
}

/// Speak the current selection right now (the settings window's "try it" button).
#[tauri::command]
fn speak_selection_now(app: AppHandle) {
    let id = begin_speech(&app, Phase::Capturing);
    std::thread::spawn(move || speak_selection(&app, id));
}

#[tauri::command]
fn speak_text(app: AppHandle, text: String) {
    let id = begin_speech(&app, Phase::Preparing);
    std::thread::spawn(move || speak_given(&app, id, text));
}

/// Audition a voice without committing to it. The voice browser previews rows this
/// way, so clicking through the list never silently rewrites the saved setting.
#[tauri::command]
fn preview_voice(
    app: AppHandle,
    engine: config::Engine,
    voice: Option<String>,
    rate: u32,
    text: Option<String>,
) {
    let mut settings = app.state::<AppState>().settings.lock().unwrap().clone();
    if settings.engine != engine {
        return;
    }
    let id = begin_speech(&app, Phase::Preparing);
    settings.rate = rate;
    match settings.engine {
        config::Engine::Apple => settings.voice = voice,
        config::Engine::Kokoro => {
            if let Some(voice) = voice {
                settings.kokoro.voice = voice;
            }
        }
        config::Engine::Chatterbox => {
            if let Some(voice) = voice {
                settings.chatterbox.voice = voice;
            }
        }
        config::Engine::Fish => settings.fish.voice_id = voice.unwrap_or_default(),
    }
    let sample =
        text.unwrap_or_else(|| "This is how I sound when reading your selection.".to_string());
    std::thread::spawn(move || run_speech(&app, id, settings, sample, false, false, capture::SourceMetadata::default()));
}

#[tauri::command]
fn get_audio_history(app: AppHandle) -> Result<Vec<history::Entry>, String> {
    history::list(&history_dir(&app)?)
}

#[tauri::command]
fn delete_audio_history(app: AppHandle, id: String) -> Result<(), String> {
    history::delete(&history_dir(&app)?, &id)
}

#[tauri::command]
fn clear_audio_history(app: AppHandle) -> Result<(), String> {
    history::clear(&history_dir(&app)?)
}

#[tauri::command]
fn play_audio_history(app: AppHandle, id: String) -> Result<(), String> {
    let path = history::audio_path(&history_dir(&app)?, &id)?;
    std::process::Command::new("/usr/bin/afplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not play saved audio: {e}"))
}

#[tauri::command]
fn stop_speaking(app: AppHandle) {
    let state = app.state::<AppState>();
    let mut job = state.job.lock().unwrap();
    job.cancel();
    state.spoken.stop();
    let _ = app.emit("kiegen:status", &job.status);
}

// ─────────────────────────── weight downloads ───────────────────────────

/// Progress of a weight download, so the window can show it rather than a frozen button.
/// `phase` is `downloading`, `done` or `error`.
#[derive(Serialize, Clone)]
struct InstallEvent {
    engine: config::Engine,
    phase: &'static str,
    file: String,
    done: u64,
    total: u64,
    message: Option<String>,
}

fn emit_install(
    app: &AppHandle,
    engine: config::Engine,
    phase: &'static str,
    file: &str,
    done: u64,
    total: u64,
    message: Option<String>,
) {
    let _ = app.emit(
        "kiegen:install",
        InstallEvent {
            engine,
            phase,
            file: file.to_string(),
            done,
            total,
            message,
        },
    );
}

/// Copy a user-chosen WAV into the app's own voice store, then report the new catalogue.
///
/// `path` is resolved by the window's file dialog, so this never prompts: a Rust command that
/// blocks on a modal dialog is a command the UI cannot show progress for.
#[tauri::command]
fn add_chatterbox_voice(app: AppHandle, path: String) -> Result<UiState, String> {
    voices::add(std::path::Path::new(&path))?;
    Ok(ui_state(&app, Vec::new()))
}

/// Delete a stored reference clip, clearing the selection if it was the chosen one — a config
/// naming a file that is gone would otherwise read as "the user picked something invalid".
#[tauri::command]
fn delete_chatterbox_voice(app: AppHandle, file: String) -> Result<UiState, String> {
    voices::remove(&file)?;
    {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().unwrap();
        if settings.chatterbox.ref_audio.as_deref() == Some(file.as_str()) {
            settings.chatterbox.ref_audio = None;
            let snapshot = settings.clone();
            drop(settings);
            config::save(&app, &snapshot)?;
        }
    }
    Ok(ui_state(&app, Vec::new()))
}

/// Kokoro's and Chatterbox's weights are plain files the app fetches and verifies itself.
/// Nothing here is installed by another runtime: the app owns every file it needs, which is
/// what makes the local engines work with no Python on the machine at all.
fn install_local_engine(app: &AppHandle, engine: config::Engine) -> Result<(), String> {
    let dir = match engine {
        config::Engine::Kokoro => engine_paths::kokoro_dir(),
        config::Engine::Chatterbox => engine_paths::chatterbox_dir(),
        config::Engine::Apple => return Ok(()),
        config::Engine::Fish => {
            return Err("Fish Audio voices are hosted and need no model download.".into())
        }
    }
    .ok_or("could not locate the app support directory")?;

    // Emitting on every read would flood the IPC channel. A megabyte, or a new file, is
    // plenty to keep a progress bar honest.
    let mut emitted: u64 = 0;
    let mut emitted_path = String::new();
    let mut actual: u64 = 0;
    let mut last_total: u64 = 0;

    let mut on_progress = |path: &str, done: u64, total: u64| {
        actual = done;
        last_total = total;
        if done.saturating_sub(emitted) >= 1 << 20 || path != emitted_path {
            emitted = done;
            emitted_path = path.to_string();
            emit_install(app, engine, "downloading", path, done, total, None);
        }
    };

    match engine {
        config::Engine::Kokoro => download::install_kokoro_into(&dir, &mut on_progress)?,
        _ => download::install_chatterbox_into(&dir, &mut on_progress)?,
    }
    emit_install(app, engine, "done", "", actual, last_total, None);
    Ok(())
}

fn install_engine_blocking(app: &AppHandle, engine: config::Engine) {
    match engine {
        // Already on the machine; nothing to fetch.
        config::Engine::Apple => {}
        config::Engine::Kokoro | config::Engine::Chatterbox => {
            if let Err(error) = install_local_engine(app, engine) {
                emit_install(app, engine, "error", "", 0, 0, Some(error));
            }
        }
        config::Engine::Fish => emit_install(
            app,
            engine,
            "error",
            "",
            0,
            0,
            Some("Fish Audio is hosted and needs no model download.".into()),
        ),
    }
}

#[tauri::command]
fn install_engine(app: AppHandle, engine: config::Engine) {
    std::thread::spawn(move || install_engine_blocking(&app, engine));
}

/// Install espeak-ng — the extra back end for Kokoro's five non-English languages.
///
/// Threaded and event-reported rather than returning a value, because a package-manager run
/// takes long enough that blocking the command would freeze the pane, and because the
/// catalogue has to be re-read afterwards either way.
#[tauri::command]
fn install_espeak_ng(app: AppHandle) {
    std::thread::spawn(move || {
        let _ = app.emit("kiegen:espeak", "Downloading and verifying espeak-ng…");
        let message = match espeak::install() {
            Ok(message) => message,
            Err(error) => error,
        };
        let _ = app.emit("kiegen:espeak", message);
    });
}

#[tauri::command]
fn open_settings_window(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg(ACCESSIBILITY_PANE)
        .status()
        .map(|_| ())
        .map_err(|e| format!("could not open System Settings: {e}"))
}

#[tauri::command]
fn request_accessibility(app: AppHandle) -> Result<UiState, String> {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    settings.accessibility_prompted = true;
    config::save(&app, &settings)?;
    drop(settings);

    capture::request_accessibility();
    Ok(ui_state(&app, Vec::new()))
}

#[tauri::command]
fn permission_status() -> bool {
    capture::is_trusted()
}

// ──────────────────────────────── setup ────────────────────────────────

fn build_tray_menu(
    app: &AppHandle,
    update_version: Option<&str>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let speak = MenuItem::with_id(app, "speak", "Speak selection", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let setup = MenuItem::with_id(app, "setup", "Setup Wizard…", true, None::<&str>)?;
    let check_updates = MenuItem::with_id(
        app,
        "check_updates",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit TextHalo", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let update_separator = PredefinedMenuItem::separator(app)?;
    let quit_separator = PredefinedMenuItem::separator(app)?;
    let update_available = update_version
        .map(|version| {
            MenuItem::with_id(
                app,
                "update_available",
                format!("Update available — {version}"),
                true,
                None::<&str>,
            )
        })
        .transpose()?;

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![
        &speak,
        &stop,
        &separator,
        &settings,
        &setup,
        &update_separator,
        &check_updates,
    ];
    if let Some(update_available) = update_available.as_ref() {
        items.push(update_available);
    }
    items.push(&quit_separator);
    items.push(&quit);
    Menu::with_items(app, &items)
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let update_version = app
        .state::<AppState>()
        .update_available_version
        .lock()
        .unwrap()
        .clone();
    let menu = build_tray_menu(app, update_version.as_deref())?;

    let mut builder = TrayIconBuilder::with_id("kiegen")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("TextHalo — speak the selection")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "speak" => {
                let app = app.clone();
                let id = begin_speech(&app, Phase::Capturing);
                std::thread::spawn(move || speak_selection(&app, id));
            }
            "stop" => {
                stop_speaking(app.clone());
            }
            "settings" => show_settings(app),
            "setup" => show_setup_wizard(app),
            "check_updates" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    check_for_updates_in_background(&app, true).await;
                });
            }
            "update_available" => {
                if let Some(version) = app
                    .state::<AppState>()
                    .update_available_version
                    .lock()
                    .unwrap()
                    .clone()
                {
                    overlay::show_update_result(app, Some(version.clone()));
                    let _ = app.emit(
                        "texthalo:background-update-status",
                        BackgroundUpdateStatus {
                            version: Some(version),
                        },
                    );
                }
            }
            "quit" => {
                app.state::<AppState>().spoken.stop();
                app.exit(0);
            }
            _ => {}
        });

    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    builder = builder.icon(tray_icon).icon_as_template(true);

    builder.build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .on_window_event(|window, event| {
            // A menu-bar app keeps its settings window alive between visits. Destroying
            // it makes get_webview_window("main") return None when Settings is clicked.
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_secure_storage::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    // Press only: `Released` would double-trigger every chord.
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    match shortcuts::action_for(app, shortcut) {
                        Some(Action::Speak) => {
                            let app = app.clone();
                            let id = begin_speech(&app, Phase::Capturing);
                            std::thread::spawn(move || speak_selection(&app, id));
                        }
                        Some(Action::Stop) => {
                            stop_speaking(app.clone());
                        }
                        None => {}
                    }
                })
                .build(),
        )
        .setup(|app| {
            // Menu-bar agent, not a windowed app: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(if cfg!(debug_assertions) {
                tauri::ActivationPolicy::Regular
            } else {
                tauri::ActivationPolicy::Accessory
            });

            let handle = app.handle().clone();
            hosted::initialize_secure_storage(&handle);
            let settings = config::load(&handle);
            let first_run = !settings.onboarding_completed;
            let voices = speech::list_voices();

            app.manage(AppState {
                settings: Mutex::new(settings),
                job: Mutex::new(speech_job::SpeechJob::default()),
                synthesis: engine_locks::EngineLocks::default(),
                spoken: spoken::Spoken::new(),
                voices,
                bindings: Mutex::new(Vec::new()),
                update_available_version: Mutex::new(None),
                update_overlay_status: Mutex::new(UpdateOverlayStatus::default()),
            });
            app.manage(update_safety::UpdateRecovery::default());

            if let Err(error) = shortcuts::apply(&handle) {
                eprintln!("[TextHalo] shortcut setup failed: {error}");
            }
            install_tray(&handle)?;
            if !cfg!(debug_assertions) {
                start_background_update_checks(handle.clone());
            }
            overlay::setup(&handle)?;

            // Show setup on a fresh install, and show permission guidance whenever access
            // is missing. Existing installs keep their saved onboarding state.
            if cfg!(debug_assertions) || first_run || !capture::is_trusted() {
                show_settings(&handle);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            update_safety::check_update_installation,
            update_safety::prepare_update_installation,
            update_safety::finish_update_installation,
            update_safety::record_update_failure,
            get_state,
            get_speech_status,
            get_update_overlay_status,
            publish_update_status,
            close_update_overlay,
            save_settings,
            speak_selection_now,
            speak_text,
            preview_voice,
            get_audio_history,
            delete_audio_history,
            clear_audio_history,
            play_audio_history,
            stop_speaking,
            install_engine,
            install_espeak_ng,
            add_chatterbox_voice,
            delete_chatterbox_voice,
            open_settings_window,
            open_accessibility_settings,
            request_accessibility,
            permission_status,
            set_update_menu_status_from_settings,
            hosted::begin_desktop_signin,
            hosted::cancel_desktop_signin,
            hosted::desktop_is_signed_in,
            hosted::desktop_sign_out,
            hosted::desktop_account,
            hosted::desktop_billing_plans,
            hosted::desktop_voices,
            hosted::desktop_voice_languages,
            hosted::desktop_voice_preview,
            hosted::desktop_clones,
            hosted::desktop_clone_status,
            hosted::desktop_delete_clone,
            hosted::desktop_upload_clone,
            hosted::desktop_checkout,
            hosted::desktop_topup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TextHalo");
}

#[cfg(test)]
mod readiness_tests {
    use super::*;

    #[test]
    fn ready_local_engines_are_allowed_to_speak() {
        for engine in [config::Engine::Kokoro, config::Engine::Chatterbox] {
            let mut catalog = engines::catalog_with(&config::Settings::default(), Vec::new());
            let info = catalog.iter_mut().find(|info| info.id == engine).unwrap();
            info.can_speak = true;
            info.blocked_reason = None;
            assert_eq!(engine_refusal(engine, catalog), None);
        }
    }

    #[test]
    fn unavailable_engine_explains_what_is_missing() {
        let mut catalog = engines::catalog_with(&config::Settings::default(), Vec::new());
        let info = catalog
            .iter_mut()
            .find(|info| info.id == config::Engine::Kokoro)
            .unwrap();
        info.can_speak = false;
        info.blocked_reason = Some("Weights missing".to_string());
        let reason = engine_refusal(config::Engine::Kokoro, catalog).unwrap();
        assert!(reason.contains("Weights missing"));
    }

    #[test]
    fn apple_speech_remains_available() {
        assert_eq!(selected_engine_refusal(&config::Settings::default()), None);
    }
}
