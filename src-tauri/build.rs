struct SwiftToolchain {
    compiler: String,
    sdk: String,
    sdk_major: u32,
}

fn xcrun_value(developer_dir: Option<&str>, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("/usr/bin/xcrun");
    if let Some(path) = developer_dir {
        command.env("DEVELOPER_DIR", path);
    }
    let output = command.args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}

fn swift_toolchain() -> SwiftToolchain {
    let candidates = [None, Some("/Library/Developer/CommandLineTools")];
    candidates
        .into_iter()
        .filter_map(|developer_dir| {
            let compiler = xcrun_value(developer_dir, &["--find", "swiftc"])?;
            let sdk = xcrun_value(developer_dir, &["--sdk", "macosx", "--show-sdk-path"])?;
            let sdk_major = xcrun_value(developer_dir, &["--sdk", "macosx", "--show-sdk-version"])?
                .split('.')
                .next()?
                .parse()
                .ok()?;
            Some(SwiftToolchain {
                compiler,
                sdk,
                sdk_major,
            })
        })
        .max_by_key(|candidate| candidate.sdk_major)
        .expect("A macOS Swift toolchain is required to build the assistant")
}

fn build_assistant() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let target = std::env::var("TARGET").unwrap();
    let arch = if target.starts_with("aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    std::fs::create_dir_all("binaries").expect("create native binary directory");
    for file in [
        "native/Contracts.swift",
        "native/FocusedPaste.swift",
        "native/HoldShortcuts.swift",
        "native/SpeechSupport.swift",
        "native/ScreenCapture.swift",
        "native/ScreenText.swift",
        "native/GenerationHistory.swift",
        "native/AssistantGeneration.swift",
        "native/Assistant.swift",
        "native/Info.plist",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let output = format!("binaries/texthalo-assistant-{target}");
    let deployment_target = format!("{arch}-apple-macos14.0");
    let toolchain = swift_toolchain();
    let mut args = vec![
        "-parse-as-library",
        "-O",
        "-target",
        &deployment_target,
        "-sdk",
        &toolchain.sdk,
        "native/Contracts.swift",
        "native/FocusedPaste.swift",
        "native/HoldShortcuts.swift",
        "native/SpeechSupport.swift",
        "native/ScreenCapture.swift",
        "native/ScreenText.swift",
        "native/GenerationHistory.swift",
        "native/AssistantGeneration.swift",
        "native/Assistant.swift",
        "-o",
        &output,
        "-Xlinker",
        "-sectcreate",
        "-Xlinker",
        "__TEXT",
        "-Xlinker",
        "__info_plist",
        "-Xlinker",
        "native/Info.plist",
    ];
    if toolchain.sdk_major >= 27 {
        args.splice(0..0, ["-D", "FOUNDATION_MODELS_IMAGE_ATTACHMENTS"]);
    }
    let status = std::process::Command::new(&toolchain.compiler)
        .args(args)
        .status()
        .expect("A Swift toolchain with FoundationModels is required to build the assistant");
    assert!(status.success(), "native assistant compilation failed");
    println!(
        "cargo:rustc-env=TEXTHALO_ASSISTANT_DEV_PATH={}/{}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap(),
        output
    );
}

fn main() {
    build_assistant();
    println!("cargo:rerun-if-env-changed=VITE_TEXTHALO_API_URL");
    println!("cargo:rerun-if-env-changed=VITE_TEXTHALO_WEBSITE_URL");
    if let Ok(origin) = std::env::var("VITE_TEXTHALO_API_URL") {
        println!("cargo:rustc-env=TEXTHALO_API_BASE_URL={origin}");
    }
    let website = std::env::var("VITE_TEXTHALO_WEBSITE_URL")
        .unwrap_or_else(|_| "https://texthalo.app".into());
    println!("cargo:rustc-env=TEXTHALO_WEBSITE_BASE_URL={website}");
    tauri_build::build()
}
