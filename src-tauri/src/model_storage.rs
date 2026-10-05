//! Remove only allowlisted variant weights, retaining shared assets and voice clips.
use crate::config::Engine;
use std::path::{Path, PathBuf};

fn variant_files(engine: Engine, quant: &str) -> Result<Vec<PathBuf>, String> {
    let files = match engine {
        Engine::Kokoro => vec![crate::kokoro_models::get(quant)?.file],
        Engine::Chatterbox => {
            let model = crate::chatterbox_models::get(quant)?;
            vec![model.graph, model.weights]
        }
        _ => return Err("This voice source has no downloaded local model.".into()),
    };
    let mut paths = Vec::new();
    for file in files {
        let path = PathBuf::from(file);
        let partial = path.with_extension("part");
        paths.push(path);
        if !paths.contains(&partial) {
            paths.push(partial);
        }
    }
    Ok(paths)
}

pub fn has_variant(dir: &Path, engine: Engine, quant: &str) -> bool {
    variant_files(engine, quant).is_ok_and(|files| {
        files
            .iter()
            .any(|file| dir.join(file).symlink_metadata().is_ok())
    })
}

pub fn remove_variant(dir: &Path, engine: Engine, quant: &str) -> Result<(), String> {
    for file in variant_files(engine, quant)? {
        let path = dir.join(file);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not delete {}: {error}", path.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Engine;

    #[test]
    fn deleting_a_variant_preserves_other_models_shared_files_and_voice_clips() {
        for (engine, quant, files, others) in [
            (
                Engine::Kokoro,
                "fp16",
                vec!["onnx/model_fp16.onnx", "onnx/model_fp16.part"],
                vec!["onnx/model.onnx"],
            ),
            (
                Engine::Chatterbox,
                "q4",
                vec![
                    "onnx/language_model_q4.onnx",
                    "onnx/language_model_q4.onnx_data",
                    "onnx/language_model_q4.part",
                ],
                vec![
                    "onnx/language_model_q4f16.onnx",
                    "onnx/language_model_q4f16.onnx_data",
                    "onnx/speech_encoder.onnx_data",
                ],
            ),
        ] {
            let dir = std::env::temp_dir()
                .join(format!("texthalo-delete-{engine:?}-{}", std::process::id()));
            for file in files.iter().chain(others.iter()).copied().chain([
                "tokenizer.json",
                "lexicon/us_gold.json",
                "voices/my-voice.wav",
            ]) {
                let path = dir.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"keep or remove").unwrap();
            }
            assert!(has_variant(&dir, engine, quant));
            remove_variant(&dir, engine, quant).unwrap();
            assert!(!has_variant(&dir, engine, quant));
            for file in files {
                assert!(!dir.join(file).exists(), "{file}");
            }
            for file in others.into_iter().chain([
                "tokenizer.json",
                "lexicon/us_gold.json",
                "voices/my-voice.wav",
            ]) {
                assert!(dir.join(file).exists(), "{file} must survive");
            }
            remove_variant(&dir, engine, quant).unwrap(); // Already absent is harmless.
            assert!(remove_variant(&dir, engine, "../tokenizer.json").is_err());
            assert!(dir.join("tokenizer.json").exists());
            assert!(remove_variant(&dir, Engine::Apple, quant).is_err());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
