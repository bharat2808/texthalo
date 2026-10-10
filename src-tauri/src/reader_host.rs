//! Registers the TextHalo Reader native-messaging host so the Chrome Web Store
//! extension can detect the installed Mac app.
//!
//! On every launch (best-effort, idempotent) the app ensures:
//! - the bundled `reader-host` binary is installed to
//!   `~/Library/Application Support/TextHalo Reader/`
//! - a `com.texthalo.reader.json` manifest exists in each supported
//!   Chromium browser's `NativeMessagingHosts` directory, allowlisting only
//!   the TextHalo Reader extension ID.
//!
//! When the extension is installed without the Mac app, its `hello` probe
//! fails with "host not found" and the popup offers a direct app download.

#[cfg(target_os = "macos")]
use std::path::PathBuf;

/// Extension ID pinned by `browser-extension/manifest.json` (`key`) and
/// `browser-extension/extension-id.txt`; must match the origin check in
/// `browser-extension/native/ReaderHost.swift`.
#[cfg(target_os = "macos")]
fn extension_id() -> &'static str {
    include_str!("../../browser-extension/extension-id.txt").trim()
}

#[cfg(target_os = "macos")]
const HOST_NAME: &str = "com.texthalo.reader";

#[cfg(target_os = "macos")]
fn install_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|e| format!("HOME unavailable: {e}"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/TextHalo Reader"))
}

/// Locate the bundled reader-host binary: next to the app executable in a
/// bundled build, or the build.rs-compiled binary when running unbundled.
#[cfg(target_os = "macos")]
fn host_binary_path() -> Result<PathBuf, String> {
    let bundled = std::env::current_exe()
        .map_err(|e| format!("current executable unavailable: {e}"))?
        .with_file_name("reader-host");
    if bundled.exists() || !cfg!(debug_assertions) {
        return Ok(bundled);
    }
    Ok(PathBuf::from(env!("TEXTHALO_READER_HOST_DEV_PATH")))
}
#[cfg(target_os = "macos")]
fn write_if_differ(path: &PathBuf, content: &[u8], mode: u32) -> Result<bool, String> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == content {
            return Ok(false);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    Ok(true)
}

#[cfg(target_os = "macos")]
fn browser_host_dirs() -> Result<Vec<PathBuf>, String> {
    let home = std::env::var("HOME").map_err(|e| format!("HOME unavailable: {e}"))?;
    let base = PathBuf::from(home).join("Library/Application Support");
    Ok(vec![
        base.join("Google/Chrome/NativeMessagingHosts"),
        base.join("Microsoft Edge/NativeMessagingHosts"),
        base.join("BraveSoftware/Brave-Browser/NativeMessagingHosts"),
        base.join("Chromium/NativeMessagingHosts"),
    ])
}
#[cfg(target_os = "macos")]
fn register() -> Result<(), String> {
    let dir = install_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;

    let source = host_binary_path()?;
    let binary = std::fs::read(&source)
        .map_err(|e| format!("read reader-host at {}: {e}", source.display()))?;
    // Refresh the installed copy when the bundled host changes (app updates).
    write_if_differ(&dir.join("reader-host"), &binary, 0o700)?;

    // Same launcher shape as browser-extension/native/setup.mjs: Chrome passes
    // the requesting origin as argv[1], which the host validates.
    let launcher = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", dir.join("reader-host").display());
    write_if_differ(&dir.join("launch-host"), launcher.as_bytes(), 0o700)?;

    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "TextHalo browser reader relay",
        "path": dir.join("launch-host").to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", extension_id())],
    });
    let manifest_text = format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
    );
    for browser_dir in browser_host_dirs()? {
        write_if_differ(
            &browser_dir.join(format!("{HOST_NAME}.json")),
            manifest_text.as_bytes(),
            0o600,
        )?;
    }
    Ok(())
}

/// Ensure the reader native-messaging host is registered. Best-effort and
/// idempotent: failures are logged and never block app startup.
pub fn ensure_registered(_app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(error) = register() {
        eprintln!("[TextHalo] reader host registration: {error}");
    }
}