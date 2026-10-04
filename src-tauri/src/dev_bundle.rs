//! Launch dev builds through Launch Services so TCC attributes requests to TextHalo,
//! rather than the terminal/editor that ran Cargo. Release bundles bypass this module.
use std::{fs, path::Path, process::Command, time::Duration};

const PARENT_ARGUMENT: &str = "--texthalo-dev-parent=";

fn signing_identity(config: &serde_json::Value) -> Result<&str, &'static str> {
    config["bundle"]["macOS"]["signingIdentity"]
        .as_str()
        .map(str::trim)
        .filter(|identity| !identity.is_empty() && *identity != "-")
        .ok_or("Configure bundle.macOS.signingIdentity to preserve TextHalo permissions in development")
}

fn inside_bundle(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "MacOS")
        && executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_some_and(|n| n == "Contents")
}

pub fn launch() -> Result<bool, Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    if inside_bundle(&executable) {
        if let Some(parent) = std::env::args().find_map(|arg| {
            arg.strip_prefix(PARENT_ARGUMENT)?
                .parse::<i32>()
                .ok()
                .filter(|pid| *pid > 1)
        }) {
            // Launch Services reparents the app. Follow the Cargo-owned launcher so
            // a native rebuild or Ctrl-C also closes this development instance.
            std::thread::spawn(move || loop {
                if unsafe { libc::kill(parent, 0) } != 0 {
                    std::process::exit(0);
                }
                std::thread::sleep(Duration::from_millis(250));
            });
        }
        return Ok(false);
    }
    let bundle = executable
        .parent()
        .ok_or("No executable directory")?
        .join("TextHalo.app");
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(contents.join("Resources"))?;
    fs::write(
        contents.join("Info.plist"),
        include_str!("../DevInfo.plist"),
    )?;
    fs::copy(&executable, macos.join("kiegen"))?;
    fs::copy(
        env!("TEXTHALO_ASSISTANT_DEV_PATH"),
        macos.join("texthalo-assistant"),
    )?;
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.icns"),
        contents.join("Resources/icon.icns"),
    )?;
    // Match the installed app's signing identity and identifiers. Ad-hoc signing
    // ties TCC grants to a build's hash, so every rebuild loses that identity.
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let identity = signing_identity(&config)?;
    // Sign nested code first, then the containing bundle, as in release builds.
    for path in [macos.join("texthalo-assistant"), bundle.clone()] {
        let signed = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", identity, "--timestamp=none"])
            .arg(&path)
            .status()?;
        if !signed.success() {
            return Err(format!(
                "Could not sign {} with {identity}; check the signing certificate in Keychain",
                path.display()
            )
            .into());
        }
    }
    if !Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&bundle)
        .status()?
        .success()
    {
        return Err("TextHalo development signature verification failed".into());
    }
    let status = Command::new("/usr/bin/open")
        .args(["-n", "-W"])
        .arg(&bundle)
        .arg("--args")
        .arg(format!("{PARENT_ARGUMENT}{}", std::process::id()))
        .args(std::env::args_os().skip(1))
        .status()?;
    if !status.success() {
        return Err("Could not launch the TextHalo development bundle".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn development_uses_the_configured_release_identity() {
        let config = serde_json::json!({"bundle":{"macOS":{"signingIdentity":"Developer ID Application: Test (TEAM)"}}});
        assert_eq!(
            signing_identity(&config).unwrap(),
            "Developer ID Application: Test (TEAM)"
        );
        for identity in ["", "-", "   "] {
            let config = serde_json::json!({"bundle":{"macOS":{"signingIdentity":identity}}});
            assert!(signing_identity(&config).is_err());
        }
        assert!(signing_identity(&serde_json::json!({})).is_err());
    }
    #[test]
    fn only_bundle_executables_skip_launch_services() {
        assert!(!inside_bundle(Path::new("/project/target/debug/kiegen")));
        assert!(inside_bundle(Path::new(
            "/project/target/debug/TextHalo.app/Contents/MacOS/kiegen"
        )));
    }
}
