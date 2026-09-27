use kiegen_lib::{
    config::{Engine, Settings},
    spoken::Spoken,
};
use std::{path::Path, sync::Arc};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let destination = Path::new(args.get(1).expect("output directory argument"));
    std::fs::create_dir_all(destination).unwrap();
    let lines = [
        "Select a passage.",
        "Press your shortcut.",
        "Then settle in and listen.",
        "TextHalo. A little more room to listen.",
    ];
    let mut settings = Settings::default();
    settings.engine = Engine::Kokoro;
    settings.kokoro.voice = "af_heart".to_string();
    settings.kokoro.speed = 1.0;
    settings.kokoro.keep_warm = true;
    let spoken = Arc::new(Spoken::new());
    for (index, line) in lines.iter().enumerate() {
        let path = destination.join(format!("line-{}.wav", index + 1));
        let report = spoken.render(&settings, line, &path).unwrap();
        println!("{} | {} | {}", path.display(), report.summary(), line);
    }
}
