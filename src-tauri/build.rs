fn main() {
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
