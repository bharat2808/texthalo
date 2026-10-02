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
        "native/SpeechSupport.swift",
        "native/Assistant.swift",
        "native/Info.plist",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let output = format!("binaries/texthalo-assistant-{target}");
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-O",
            "-target",
            &format!("{arch}-apple-macos14.0"),
            "native/Contracts.swift",
            "native/SpeechSupport.swift",
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
        ])
        .status()
        .expect("Xcode with FoundationModels is required to build the assistant");
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
