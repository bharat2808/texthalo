//! Checks run before the updater downloads or replaces an installed bundle.
use std::path::{Component, Path};

#[derive(Default)]
pub struct UpdateRecovery(std::sync::Mutex<Option<RecoveryCopy>>);

struct RecoveryCopy {
    bundle: std::path::PathBuf,
    directory: std::path::PathBuf,
}

fn restore_if_missing(copy: &RecoveryCopy) -> Result<bool, String> {
    if !copy.bundle.exists() {
        std::fs::rename(copy.directory.join("TextHalo.app"), &copy.bundle).map_err(|e| {
            format!(
                "The previous app is saved at {}. Restore it using Finder: {e}",
                copy.directory.display()
            )
        })?;
        std::fs::remove_dir(&copy.directory).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
pub async fn prepare_update_installation(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<UpdateRecovery>();
        let mut recovery = state.0.lock().unwrap();
        if recovery.is_some() { return Err("An update installation is already in progress.".into()); }
        check_update_installation()?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let bundle = bundle_path(&executable)?.to_path_buf();
        let directory = bundle.parent().unwrap().join(format!(".texthalo-update-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).map_err(|e| format!("Cannot create update recovery copy: {e}"))?;
        // Keep an independent copy: the upstream updater removes its temporary
        // backup even when its final rename fails. ditto preserves bundle metadata.
        let result = std::process::Command::new("/usr/bin/ditto")
            .arg(&bundle).arg(directory.join("TextHalo.app")).output();
        match result {
            Ok(output) if output.status.success() => {
                *recovery = Some(RecoveryCopy { bundle, directory });
                Ok(())
            }
            result => {
                let _ = std::fs::remove_dir_all(&directory);
                Err(format!("Could not make an update recovery copy; installation was not started: {result:?}"))
            }
        }
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn finish_update_installation(
    app: tauri::AppHandle,
    success: bool,
) -> Result<String, String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<UpdateRecovery>();
        let mut recovery = state.0.lock().unwrap();
        let Some(copy) = recovery.take() else {
            return Ok(String::new());
        };
        if success {
            std::fs::remove_dir_all(&copy.directory).map_err(|e| {
                format!(
                    "Update installed; recovery copy retained at {}: {e}",
                    copy.directory.display()
                )
            })?;
            Ok(String::new())
        } else if restore_if_missing(&copy)? {
            Ok("The previous app was restored. You can reopen it and try again.".into())
        } else {
            Ok(format!(
                "A recovery copy of your previous app is saved at {}.",
                copy.directory.join("TextHalo.app").display()
            ))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn check_error(error: &tauri_plugin_updater::Error) -> String {
    match error {
        tauri_plugin_updater::Error::TargetNotFound(_)
        | tauri_plugin_updater::Error::TargetsNotFound(_) => format!(
            "This release has no update for the {} build of TextHalo. Please wait for a compatible release. An update for the other Mac architecture cannot be installed.",
            if cfg!(target_arch = "x86_64") { "Intel" } else { "Apple Silicon" }
        ),
        _ => format!("Could not check for updates: {error}"),
    }
}

fn bundle_path(executable: &Path) -> Result<&Path, String> {
    let macos = executable
        .parent()
        .ok_or("Cannot locate the running app.")?;
    let contents = macos.parent().ok_or("Cannot locate the running app.")?;
    let bundle = contents.parent().ok_or("Cannot locate the running app.")?;
    if macos.file_name() != Some("MacOS".as_ref())
        || contents.file_name() != Some("Contents".as_ref())
        || bundle.extension() != Some("app".as_ref())
    {
        return Err("Automatic updates require the installed TextHalo.app. Open the app from Applications and try again.".into());
    }
    Ok(bundle)
}

fn check_location(bundle: &Path) -> Result<(), String> {
    if bundle
        .components()
        .any(|part| part == Component::Normal("AppTranslocation".as_ref()))
    {
        return Err("macOS is running a temporary copy of TextHalo. Quit TextHalo, drag it into Applications using Finder, eject the installer, and open the Applications copy.".into());
    }
    if bundle.starts_with("/Volumes") {
        return Err("TextHalo is running from a mounted disk or external volume. Quit TextHalo, copy it into Applications using Finder, eject the installer, and reopen it before updating.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn check_update_installation() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = bundle_path(&executable)?;
    check_location(bundle)?;
    #[cfg(target_os = "macos")]
    {
        use std::os::darwin::fs::MetadataExt as DarwinMetadataExt;
        use std::os::unix::fs::MetadataExt;
        // Tauri 2.12 stages and backs up in temp_dir with rename(), so a
        // different device must be rejected before it touches the app.
        let metadata = std::fs::metadata(bundle).map_err(|e| e.to_string())?;
        let temp = std::fs::metadata(std::env::temp_dir()).map_err(|e| e.to_string())?;
        if metadata.dev() != temp.dev() {
            return Err("TextHalo is on a different volume from the updater's temporary files. Move it into Applications on your startup disk, or install the new version manually using Finder.".into());
        }
        if metadata.st_flags() & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE) != 0 {
            return Err("TextHalo is locked. Quit the app, open Finder → Get Info for TextHalo.app, and clear Locked before updating.".into());
        }
    }
    let parent = bundle
        .parent()
        .ok_or("Cannot locate the app's install folder.")?;
    // Probe effective permissions (including ACLs), without modifying the app.
    let probe = parent.join(format!(".texthalo-update-check-{}", uuid::Uuid::new_v4()));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)
        .map_err(|e| format!("TextHalo cannot write to its install folder ({e}). Ask the Mac's administrator to replace TextHalo using Finder, or install it in your own Applications folder. Accessibility permission does not grant update access."))?;
    drop(file);
    std::fs::remove_file(&probe)
        .map_err(|e| format!("Cannot remove the update permission check: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn record_update_failure(
    app: tauri::AppHandle,
    stage: String,
    detail: String,
) -> Result<(), String> {
    use std::io::Write;
    use tauri::Manager;
    let directory = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("updater.log"))
        .map_err(|e| e.to_string())?;
    let entry = serde_json::json!({
        "time": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "version": app.package_info().version.to_string(),
        "architecture": std::env::consts::ARCH,
        "executable": std::env::current_exe().ok(),
        "stage": stage,
        "error": detail,
    });
    writeln!(file, "{entry}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_installed_bundle_layout_is_accepted() {
        assert_eq!(
            bundle_path(Path::new(
                "/Applications/TextHalo.app/Contents/MacOS/kiegen"
            ))
            .unwrap(),
            Path::new("/Applications/TextHalo.app")
        );
        assert!(bundle_path(Path::new("/work/target/debug/kiegen")).is_err());
        assert!(bundle_path(Path::new("/work/fake.app/bin/kiegen")).is_err());
    }

    #[test]
    fn mounted_and_translocated_apps_are_blocked() {
        assert!(check_location(Path::new("/Volumes/TextHalo/TextHalo.app")).is_err());
        assert!(check_location(Path::new(
            "/private/var/folders/abc/AppTranslocation/uuid/d/TextHalo.app"
        ))
        .is_err());
        assert!(check_location(Path::new("/Applications/TextHalo.app")).is_ok());
        assert!(check_location(Path::new("/Users/sam/Applications/TextHalo.app")).is_ok());
    }

    #[test]
    fn failed_replacement_restores_missing_app_without_overwriting_existing_app() {
        let root =
            std::env::temp_dir().join(format!("texthalo-recovery-test-{}", uuid::Uuid::new_v4()));
        let backup = root.join("backup");
        std::fs::create_dir_all(backup.join("TextHalo.app")).unwrap();
        std::fs::write(backup.join("TextHalo.app/version"), "old").unwrap();
        let copy = RecoveryCopy {
            bundle: root.join("TextHalo.app"),
            directory: backup,
        };
        std::fs::create_dir(&copy.bundle).unwrap();
        std::fs::write(copy.bundle.join("version"), "new").unwrap();
        assert!(!restore_if_missing(&copy).unwrap());
        assert_eq!(
            std::fs::read_to_string(copy.bundle.join("version")).unwrap(),
            "new"
        );
        std::fs::remove_dir_all(&copy.bundle).unwrap();
        assert!(restore_if_missing(&copy).unwrap());
        assert_eq!(
            std::fs::read_to_string(copy.bundle.join("version")).unwrap(),
            "old"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
