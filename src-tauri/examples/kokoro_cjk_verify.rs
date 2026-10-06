//! Render real Japanese/Mandarin fixtures through the production speech path.
//! Pass --download to repair the selected Kokoro plan before verification.
use kiegen_lib::{
    config::{Engine, Settings},
    download, engine_paths,
    spoken::Spoken,
};
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let output = PathBuf::from(
        args.get(1)
            .ok_or("usage: kokoro_cjk_verify OUTPUT_DIR [--download]")?,
    );
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    if args.iter().any(|arg| arg == "--download") {
        let dir = engine_paths::kokoro_dir().ok_or("missing app support directory")?;
        download::install_kokoro_variant_into(&dir, "fp32", &mut |file, _, _| {
            println!("Downloading {file}")
        })?;
    }
    let spoken = Spoken::new();
    for (voice, name, text) in [
        (
            "jf_alpha",
            "japanese",
            "こんにちは。今日は2026年10月6日です。日本語を自然に読み上げます。",
        ),
        (
            "zf_xiaobei",
            "mandarin",
            "你好，今天是2026年10月6日。银行行长说，价格是13.5元。",
        ),
        (
            "zf_xiaobei",
            "traditional",
            "歡迎使用語音。臺灣的銀行提供中文服務。",
        ),
        ("jf_alpha", "japanese-mixed", "TextHaloで日本語を読みます。"),
        ("zf_xiaobei", "mandarin-mixed", "欢迎使用TextHalo。"),
    ] {
        let mut settings = Settings {
            engine: Engine::Kokoro,
            ..Settings::default()
        };
        settings.kokoro.voice = voice.into();
        let path = output.join(format!("{name}.wav"));
        let report = spoken.render(&settings, text, &path)?;
        if !report.dropped.is_empty() {
            return Err(format!(
                "{name} dropped model phonemes: {:?}",
                report.dropped
            ));
        }
        if report.seconds < 1.0 {
            return Err(format!("{name} produced too little audio"));
        }
        let samples: Vec<f32> = hound::WavReader::open(&path)
            .map_err(|e| e.to_string())?
            .samples::<i16>()
            .map(|sample| sample.map(|s| s as f32).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        if !samples.iter().any(|s| s.abs() > 300.) {
            return Err(format!("{name} produced silent audio"));
        }
        println!("{}: {}", path.display(), report.summary());
    }
    Ok(())
}
