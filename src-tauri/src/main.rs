// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(target_os = "macos", debug_assertions))]
mod dev_bundle;

fn main() {
    #[cfg(all(target_os = "macos", debug_assertions))]
    match dev_bundle::launch() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("Could not launch TextHalo for development: {error}");
            std::process::exit(1);
        }
    }
    kiegen_lib::run()
}
