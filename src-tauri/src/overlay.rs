//! A non-focusing status panel, following FreeFlow's top-center black overlay.
use crate::{AppState, Phase};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        "speech-overlay",
        WebviewUrl::App("index.html?overlay".into()),
    )
    .title("TextHalo speech")
    .inner_size(350.0, 44.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .focused(false)
    .focusable(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    WebviewWindowBuilder::new(
        app,
        "update-overlay",
        WebviewUrl::App("index.html?update-overlay".into()),
    )
    .title("TextHalo update")
    .inner_size(430.0, 250.0)
    .decorations(false)
    .transparent(true)
    .resizable(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let handle = app.clone();
        if app.run_on_main_thread(move || tick(&handle)).is_err() {
            break;
        }
    });
    Ok(())
}

pub fn show_update_result(app: &AppHandle, version: Option<String>) {
    let state = app.state::<AppState>();
    let mut current = state.update_overlay_status.lock().unwrap();
    let status = if current.installing {
        let mut status = current.clone();
        status.visible = true;
        status
    } else {
        crate::UpdateOverlayStatus {
            visible: true,
            version,
            installable: false,
            installing: false,
            message: None,
            error: false,
        }
    };
    *current = status.clone();
    drop(current);

    if let Some(window) = app.get_webview_window("update-overlay") {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("texthalo:update-overlay", status);
}

pub fn show_update_error(app: &AppHandle, message: String) {
    let status = crate::UpdateOverlayStatus {
        visible: true,
        version: None,
        installable: false,
        installing: false,
        message: Some(message),
        error: true,
    };
    *app.state::<AppState>()
        .update_overlay_status
        .lock()
        .unwrap() = status.clone();

    if let Some(window) = app.get_webview_window("update-overlay") {
        let _ = window.center();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("texthalo:update-overlay", status);
}

fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut job = state.job.lock().unwrap();
    let playing = state.spoken.is_speaking();
    let changed = job.observe_playback(playing);
    if changed {
        let _ = app.emit("kiegen:status", &job.status);
    }
    let error = matches!(job.status.phase, Phase::Error);
    let (expanded, interacting) = *state.playback_overlay.lock().unwrap();
    let playback = state.spoken.playback();
    let completed = matches!(job.status.phase, Phase::Idle) && playback.available;
    let visible = !matches!(job.status.phase, Phase::Idle)
        || keep_completed_visible(completed, interacting, job.changed.elapsed());
    drop(job);
    if completed && !visible {
        state.spoken.stop();
    }
    let owned = crate::assistant::sync_speech(app);
    let assistant_error = crate::assistant::assistant_error_overlay_height(app);
    let visible = (visible && !owned) || assistant_error.is_some();
    let Some(window) = app.get_webview_window("speech-overlay") else {
        return;
    };
    if visible {
        if let Some(height) = assistant_error {
            let _ = window.set_focusable(height > 190.0);
            position_sized(&window, 430.0, height);
        } else {
            let _ = window.set_focusable(expanded || playback.paused || completed);
            if !error && (expanded || playback.paused) {
                position_sized(&window, 350.0, 112.0);
            } else {
                position(&window, error);
            }
        }
        let _ = window.show();
    } else if !visible && window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    }
}

pub(crate) fn position(window: &tauri::WebviewWindow, error: bool) {
    let width = if error { 430.0 } else { 350.0 };
    let height = if error { 190.0 } else { 44.0 };
    position_sized(window, width, height);
}

pub(crate) fn position_sized(window: &tauri::WebviewWindow, width: f64, height: f64) {
    // AppKit coordinates are logical points with the origin at the bottom left.
    // Stay below the menu bar/notch so the label and Stop remain visible on every Mac.
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSScreen, NSWindow, NSWindowCollectionBehavior};
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(screen) = NSScreen::mainScreen(mtm) else {
            return;
        };
        let frame = screen.visibleFrame();
        if let Ok(raw) = window.ns_window() {
            // Tauri owns the NSWindow; this borrow is only used on the main thread.
            let native = unsafe { &*(raw as *const NSWindow) };
            native.setLevel(1000); // NSScreenSaverWindowLevel, matching FreeFlow.
            native.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            let current = native.frame();
            let visible = native.isVisible();
            // Live updates must not snap a dragged widget back to its default
            // position. When its height changes, keep its top edge in place.
            if visible && current.size.width == width && current.size.height == height {
                return;
            }
            let origin = if visible {
                NSPoint::new(
                    current.origin.x,
                    current.origin.y + current.size.height - height,
                )
            } else {
                NSPoint::new(
                    frame.origin.x + (frame.size.width - width) / 2.0,
                    frame.origin.y + frame.size.height - height - 6.0,
                )
            };
            native.setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        }
    }
}

fn keep_completed_visible(completed: bool, interacting: bool, elapsed: Duration) -> bool {
    completed && (interacting || elapsed < Duration::from_secs(5))
}

#[cfg(test)]
mod playback_tests {
    use super::*;
    #[test]
    fn completion_waits_five_seconds_and_never_hides_during_interaction() {
        assert!(keep_completed_visible(
            true,
            false,
            Duration::from_millis(4999)
        ));
        assert!(!keep_completed_visible(true, false, Duration::from_secs(5)));
        assert!(keep_completed_visible(true, true, Duration::from_secs(60)));
        assert!(!keep_completed_visible(false, true, Duration::ZERO));
    }
}
