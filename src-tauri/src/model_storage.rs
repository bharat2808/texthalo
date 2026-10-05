#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Engine;

    #[test]
    fn deleting_a_variant_preserves_other_models_shared_files_and_voice_clips() {
        for (engine, quant, files, others) in [
            (Engine::Kokoro, "fp16", vec!["onnx/model_fp16.onnx", "onnx/model_fp16.part"], vec!["onnx/model.onnx"]),
            (Engine::Chatterbox, "q4", vec!["onnx/language_model_q4.onnx", "onnx/language_model_q4.onnx_data", "onnx/language_model_q4.part"], vec!["onnx/language_model_q4f16.onnx", "onnx/language_model_q4f16.onnx_data", "onnx/speech_encoder.onnx_data"]),
        ] {
            let dir = std::env::temp_dir().join(format!("texthalo-delete-{engine:?}-{}", std::process::id()));
            for file in files.iter().chain(others.iter()).copied().chain(["tokenizer.json", "lexicon/us_gold.json", "voices/my-voice.wav"]) {
                let path = dir.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"keep or remove").unwrap();
            }
            assert!(has_variant(&dir, engine, quant));
            remove_variant(&dir, engine, quant).unwrap();
            assert!(!has_variant(&dir, engine, quant));
            for file in files { assert!(!dir.join(file).exists(), "{file}"); }
            for file in others.into_iter().chain(["tokenizer.json", "lexicon/us_gold.json", "voices/my-voice.wav"]) {
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
