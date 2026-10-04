//! One app owns the global shortcuts, clipboard, and Assistant helper.
use std::{
    fs::{File, OpenOptions},
    os::fd::AsRawFd,
    path::Path,
    time::Duration,
};

fn lock(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

pub fn acquire() -> Result<File, Box<dyn std::error::Error>> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let identifier = config["identifier"]
        .as_str()
        .ok_or("Missing app identifier")?;
    let directory =
        std::path::PathBuf::from(std::env::var_os("HOME").ok_or("Missing home directory")?)
            .join("Library/Application Support")
            .join(identifier);
    std::fs::create_dir_all(&directory)?;
    let guard = lock(&directory.join("instance.lock"))
        .map_err(|_| "TextHalo is already running. Quit that copy before opening another.")?;

    // Older installed builds do not hold this lock. Ask them to quit before
    // registering shortcuts; their stdin-owned native helpers exit with them.
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        identifier,
    ));
    let others: Vec<_> = apps
        .iter()
        .filter(|app| app.processIdentifier() != std::process::id() as i32)
        .collect();
    for app in &others {
        if !app.isTerminated() && !app.terminate() {
            return Err("Quit the other TextHalo copy before opening this one.".into());
        }
    }
    for _ in 0..100 {
        if others.iter().all(|app| app.isTerminated()) {
            return Ok(guard);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("The other TextHalo copy has not quit; this copy will not register shortcuts.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_owner_and_exit_releases_ownership() {
        let path =
            std::env::temp_dir().join(format!("texthalo-instance-{}.lock", uuid::Uuid::new_v4()));
        let first = lock(&path).unwrap();
        assert!(lock(&path).is_err());
        drop(first);
        let next = lock(&path).unwrap();
        drop(next);
        std::fs::remove_file(path).unwrap();
    }
}
