//! Selection capture — the risky part of the app.
//!
//! Two mechanisms, one entry point:
//!   1. the Accessibility API (non-destructive; unreliable in Electron/Chrome/terminals)
//!   2. synthetic ⌘C into the pasteboard (universal; briefly owns the clipboard)
//!
//! Everything here requires the Accessibility TCC grant. A denied grant shows up
//! as `NoPermission`, never as a silent empty string.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    // Keep the actual app instance: a PID can be reused after an app quits.
    // Runtime ownership must never be persisted in audio history.
    #[cfg(target_os = "macos")]
    #[serde(skip)]
    pub application: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureError {
    /// Accessibility not granted (or revoked since launch).
    NoPermission,
    /// Permission is fine; there was nothing selected / the app exposes no selection.
    NoSelection,
    /// Posted ⌘C but the pasteboard never changed.
    Timeout,
    /// Failed to create or dispatch the synthetic ⌘C event.
    Keystroke,
    /// Failed to run clipboard access on macOS's main thread.
    MainThread,
    /// A copy happened but the pasteboard held no usable text.
    Empty,
    /// The user is typing into a secure field (password); we refuse before posting anything.
    SecureInput,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::NoPermission => {
                "TextHalo needs Accessibility permission (System Settings → Privacy & Security → Accessibility)"
            }
            Self::NoSelection => "no selected text found",
            Self::Timeout => "nothing was copied — is text selected?",
            Self::Keystroke => "TextHalo could not send ⌘C to the selected app",
            Self::MainThread => "TextHalo could not access the macOS clipboard",
            Self::Empty => "the selection contained no text",
            Self::SecureInput => "not available in password fields",
        };
        f.write_str(msg)
    }
}

/// Capture source labels on a best-effort basis alongside the selected text.
pub fn capture_with_source(
    app: &tauri::AppHandle,
    mode: crate::config::CaptureMode,
    timeout_ms: u64,
    restore: bool,
) -> Result<(String, SourceMetadata), CaptureError> {
    let source = platform::source_metadata(app);
    platform::capture(mode, timeout_ms, restore, Some(app)).map(|text| (text, source))
}

/// Is the Accessibility grant in place right now? Re-checked per invocation, never cached:
/// users revoke it, and every rebuild of an unsigned dev build invalidates it.
pub fn is_trusted() -> bool {
    platform::is_trusted()
}

/// Show macOS's first-run Accessibility permission prompt when access is missing.
pub fn request_accessibility() -> bool {
    platform::request_accessibility()
}

/// A copy-mode capture while secure input is on would leak nothing (the OS blocks it)
/// but would also silently fail; detecting it lets us say something useful instead.
pub fn secure_input_active() -> bool {
    platform::secure_input_active()
}

// ─────────────────────────────── macOS ───────────────────────────────
#[cfg(target_os = "macos")]
mod platform {
    use super::{CaptureError, SourceMetadata};
    use accessibility_sys::{
        kAXErrorSuccess, kAXFocusedUIElementAttribute, kAXSelectedTextAttribute,
        kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
        AXUIElementCopyAttributeValue, AXUIElementCreateSystemWide, AXUIElementRef,
    };
    use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use objc2_app_kit::{NSPasteboard, NSPasteboardType, NSPasteboardTypeString, NSWorkspace};
    use objc2_foundation::NSString;
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    fn on_main_thread<T: Send + 'static>(
        app: Option<&tauri::AppHandle>,
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, CaptureError> {
        let (sender, receiver) = sync_channel(1);
        let task = move || {
            let result = objc2::rc::autoreleasepool(|_| operation());
            let _ = sender.send(result);
        };
        if let Some(app) = app {
            app.run_on_main_thread(task)
                .map_err(|_| CaptureError::MainThread)?;
        } else {
            task();
        }
        receiver.recv().map_err(|_| CaptureError::MainThread)
    }

    pub fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Ask macOS to show its Accessibility permission prompt.
    pub fn request_accessibility() -> bool {
        let prompt_key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let options: CFDictionary<CFString, CFBoolean> =
            CFDictionary::from_CFType_pairs(&[(prompt_key, CFBoolean::true_value())]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
    }

    pub fn source_metadata(app: &tauri::AppHandle) -> SourceMetadata {
        on_main_thread(Some(app), read_source_metadata).unwrap_or_default()
    }

    fn read_source_metadata() -> SourceMetadata {
        unsafe {
            // A focused app can have no AXFocusedUIElement (for example an Electron
            // editor with Accessibility support disabled). App attribution must not
            // depend on that control being exposed.
            let Some(frontmost) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
                return SourceMetadata::default();
            };
            let pid = frontmost.processIdentifier();
            let app_name = frontmost
                .localizedName()
                .map(|name| name.to_string())
                .filter(|name| !name.trim().is_empty());

            let app = accessibility_sys::AXUIElementCreateApplication(pid);
            if app.is_null() {
                return SourceMetadata {
                    app_name,
                    window_title: None,
                    application: Some(frontmost),
                };
            }
            let window = copy_attribute(app, "AXFocusedWindow");
            CFRelease(app as CFTypeRef);
            let window_title = window.and_then(|window| {
                let title = copy_attribute(window as AXUIElementRef, "AXTitle");
                CFRelease(window);
                title.and_then(|title| {
                    if CFGetTypeID(title) != CFString::type_id() {
                        CFRelease(title);
                        return None;
                    }
                    let value = CFString::wrap_under_create_rule(title as CFStringRef).to_string();
                    let value = value.trim().to_string();
                    (!value.is_empty()).then_some(value)
                })
            });
            SourceMetadata {
                app_name,
                window_title,
                application: Some(frontmost),
            }
        }
    }

    /// `IsSecureEventInputEnabled()` lives in HIToolbox. Resolve it at runtime rather than
    /// linking the framework: we only need a yes/no, and a missing symbol must not be fatal.
    ///
    /// `b"…\0"` rather than a C-string literal because this crate is edition 2021
    /// (clippy::manual_c_str_literals only knows about `c"…"`).
    pub fn secure_input_active() -> bool {
        #[allow(clippy::manual_c_str_literals)] // c"..." needs edition 2024
        const SYMBOL: &[u8] = b"IsSecureEventInputEnabled\0";
        unsafe {
            let sym = libc::dlsym(libc::RTLD_DEFAULT, SYMBOL.as_ptr() as *const libc::c_char);
            if sym.is_null() {
                return false;
            }
            let f: extern "C" fn() -> bool = std::mem::transmute(sym);
            f()
        }
    }

    /// Read one attribute off an AX element, taking ownership of the returned CFTypeRef.
    unsafe fn copy_attribute(
        element: AXUIElementRef,
        attribute: &'static str,
    ) -> Option<CFTypeRef> {
        let name = CFString::from_static_string(attribute);
        let mut value: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value);
        if err == kAXErrorSuccess && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    /// Accessibility path: system-wide element → focused element → AXSelectedText.
    pub fn ax_selected_text() -> Option<String> {
        unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }
            let focused = copy_attribute(system_wide, kAXFocusedUIElementAttribute);
            CFRelease(system_wide as CFTypeRef);
            let focused = focused?;

            let text = copy_attribute(focused as AXUIElementRef, kAXSelectedTextAttribute);
            CFRelease(focused);
            let text = text?;

            // Defensive: an app may return a non-string for this attribute.
            if CFGetTypeID(text) != CFString::type_id() {
                CFRelease(text);
                return None;
            }
            let cf = CFString::wrap_under_create_rule(text as CFStringRef);
            let s = cf.to_string();
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        }
    }

    /// Post a synthetic ⌘C to the focused app.
    fn post_copy_key(down: bool) -> Result<(), CaptureError> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| CaptureError::Keystroke)?;
        let event = CGEvent::new_keyboard_event(source, KeyCode::ANSI_C, down)
            .map_err(|_| CaptureError::Keystroke)?;
        event.set_flags(CGEventFlags::CGEventFlagCommand);
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    /// Copy path: snapshot the pasteboard text → ⌘C → wait for changeCount → read → restore.
    ///
    /// v0 snapshots and restores *text only*. Full-fidelity restore needs
    /// `NSPasteboard.pasteboardItems()` + `writeObjects:` with `NSPasteboardWriting`
    /// objects (see docs/DESIGN.md §2) — deferred to v1, and the reason
    /// `restore_clipboard` is a setting rather than an assumption.
    pub fn copy_selected_text(
        app: Option<&tauri::AppHandle>,
        timeout_ms: u64,
        restore: bool,
    ) -> Result<String, CaptureError> {
        let (before, previous) = on_main_thread(app, || {
            let pasteboard = NSPasteboard::generalPasteboard();
            let previous: Option<String> = unsafe {
                pasteboard
                    .stringForType(NSPasteboardTypeString)
                    .map(|s| s.to_string())
            };
            (pasteboard.changeCount(), previous)
        })?;

        on_main_thread(app, || post_copy_key(true))??;
        // Let the target app process the key-down without blocking Cocoa's main thread.
        std::thread::sleep(Duration::from_millis(20));
        on_main_thread(app, || post_copy_key(false))??;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut observed_change = false;
        let captured = loop {
            let (changed, text) = on_main_thread(app, move || {
                let pasteboard = NSPasteboard::generalPasteboard();
                let changed = pasteboard.changeCount() != before;
                let text = if changed {
                    unsafe {
                        pasteboard
                            .stringForType(NSPasteboardTypeString)
                            .map(|s| s.to_string())
                    }
                } else {
                    None
                };
                (changed, text)
            })?;
            if changed {
                observed_change = true;
                if text.as_ref().is_some_and(|text| !text.trim().is_empty()) {
                    break text;
                }
            }
            if Instant::now() >= deadline {
                break if observed_change {
                    Some(String::new())
                } else {
                    None
                };
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        if restore {
            if let Some(previous) = previous {
                on_main_thread(app, move || {
                    let pasteboard = NSPasteboard::generalPasteboard();
                    let ty: &NSPasteboardType = unsafe { NSPasteboardTypeString };
                    pasteboard.clearContents();
                    pasteboard.setString_forType(&NSString::from_str(&previous), ty);
                })?;
            }
        }

        match captured {
            Some(text) if !text.trim().is_empty() => Ok(text),
            Some(_) => Err(CaptureError::Empty),
            None => Err(CaptureError::Timeout),
        }
    }

    pub fn capture(
        mode: crate::config::CaptureMode,
        timeout_ms: u64,
        restore: bool,
        app: Option<&tauri::AppHandle>,
    ) -> Result<String, CaptureError> {
        use crate::config::CaptureMode;
        if !is_trusted() {
            return Err(CaptureError::NoPermission);
        }
        match mode {
            CaptureMode::AxOnly => ax_selected_text().ok_or(CaptureError::NoSelection),
            CaptureMode::CopyOnly => {
                if secure_input_active() {
                    return Err(CaptureError::SecureInput);
                }
                copy_selected_text(app, timeout_ms, restore)
            }
            CaptureMode::AxThenCopy => {
                if let Some(text) = ax_selected_text() {
                    return Ok(text);
                }
                if secure_input_active() {
                    return Err(CaptureError::SecureInput);
                }
                copy_selected_text(app, timeout_ms, restore)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CaptureMode;

    /// Without the Accessibility grant, capture must fail *loudly and cheaply*:
    /// a typed error, no synthetic keystroke, no clipboard mutation.
    #[test]
    fn without_permission_capture_refuses_instead_of_guessing() {
        if is_trusted() {
            // Developer machine with the grant in place — nothing to assert here.
            return;
        }
        for mode in [
            CaptureMode::AxThenCopy,
            CaptureMode::AxOnly,
            CaptureMode::CopyOnly,
        ] {
            let result = platform::capture(mode, 150, true, None);
            assert!(
                matches!(result, Err(CaptureError::NoPermission)),
                "expected NoPermission for {mode:?}, got {result:?}"
            );
        }
    }
}

// ─────────────────────────── other platforms ───────────────────────────
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{CaptureError, SourceMetadata};

    pub fn source_metadata(_app: &tauri::AppHandle) -> SourceMetadata {
        SourceMetadata::default()
    }

    pub fn is_trusted() -> bool {
        false
    }

    pub fn request_accessibility() -> bool {
        false
    }

    pub fn secure_input_active() -> bool {
        false
    }

    pub fn capture(
        _mode: crate::config::CaptureMode,
        _timeout_ms: u64,
        _restore: bool,
        _app: Option<&tauri::AppHandle>,
    ) -> Result<String, CaptureError> {
        Err(CaptureError::NoPermission)
    }
}
