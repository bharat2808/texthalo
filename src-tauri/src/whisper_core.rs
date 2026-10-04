//! Platform-independent persistent inference and overlapping transcript assembly.
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
pub const RATE: usize = 16_000;
pub const WINDOW: usize = 20 * RATE;
pub const STEP: usize = 16 * RATE;
pub const MAX_AUDIO: usize = 90 * RATE;
pub fn languages(model: &str) -> serde_json::Value {
    serde_json::json!((0..=if model.starts_with("large-v3") { whisper_rs::get_lang_max_id() } else { 98 }).filter_map(|id| {
        Some(serde_json::json!({"code":whisper_rs::get_lang_str(id)?,"name":whisper_rs::get_lang_str_full(id)?}))
    }).collect::<Vec<_>>())
}
pub struct Engine {
    context: WhisperContext,
}
impl Engine {
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = path
            .to_str()
            .ok_or("The model path is not valid Unicode.")?;
        let context = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| format!("Could not load Whisper: {e}"))?;
        Ok(Self { context })
    }
    pub fn transcribe(
        &self,
        samples: &[f32],
        language: &str,
        abort: Arc<AtomicBool>,
    ) -> Result<String, String> {
        if samples.is_empty() || !samples.iter().all(|s| s.is_finite()) {
            return Err("Invalid microphone audio.".into());
        }
        // Do not turn digital silence into invented text.
        if samples.iter().map(|s| s * s).sum::<f32>() / (samples.len() as f32) < 0.0000001 {
            return Ok(String::new());
        }
        let mut state = self.context.create_state().map_err(|e| e.to_string())?;
        let language = language.split(['-', '_']).next().unwrap_or("auto");
        if language.contains('\0')
            || language != "auto" && whisper_rs::get_lang_id(language).is_none()
        {
            return Err("Whisper does not support the selected language.".into());
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(4);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_language(Some(language));
        params.set_detect_language(false);
        // whisper-rs 0.16's safe trampoline casts a boxed trait object to F.
        // Pass the atomic directly instead. `abort` stays alive through synchronous full().
        unsafe extern "C" fn should_abort(data: *mut std::ffi::c_void) -> bool {
            (*(data as *const AtomicBool)).load(Ordering::SeqCst)
        }
        unsafe {
            params.set_abort_callback(Some(should_abort));
            params.set_abort_callback_user_data(Arc::as_ptr(&abort) as *mut std::ffi::c_void);
        }
        state
            .full(params, samples)
            .map_err(|e| format!("Transcription failed: {e}"))?;
        let mut text = String::new();
        for segment in state.as_iter() {
            text.push_str(segment.to_str().map_err(|e| e.to_string())?);
        }
        Ok(text.trim().to_owned())
    }
    pub fn finish(
        &self,
        samples: &[f32],
        language: &str,
        abort: Arc<AtomicBool>,
    ) -> Result<String, String> {
        let mut text = String::new();
        let mut start = 0;
        while start < samples.len() {
            if abort.load(Ordering::SeqCst) {
                return Err("Transcription canceled.".into());
            }
            let end = (start + WINDOW).min(samples.len());
            let next = self.transcribe(&samples[start..end], language, abort.clone())?;
            text = merge(&text, &next);
            if end == samples.len() {
                break;
            }
            start += STEP;
        }
        Ok(text)
    }
}
/// Only remove a matching suffix/prefix at an overlapping audio boundary. Never replace
/// the earlier passage with a rolling preview, even if the new preview collapses.
pub fn merge(prefix: &str, next: &str) -> String {
    if prefix.is_empty() {
        return next.into();
    }
    if next.is_empty() {
        return prefix.into();
    }
    let left: Vec<_> = prefix.split_whitespace().collect();
    let right: Vec<_> = next.split_whitespace().collect();
    // Strip boundary punctuation, preserving vowel marks in languages such as Hindi.
    let norm = |s: &str| {
        s.trim_matches(|c: char| {
            matches!(
                c,
                '.' | ','
                    | '!'
                    | '?'
                    | ':'
                    | ';'
                    | '\"'
                    | '\''
                    | '('
                    | ')'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '।'
                    | '…'
            )
        })
        .to_lowercase()
    };
    let overlap = (2..=left.len().min(right.len()).min(80))
        .rev()
        .find(|&n| {
            left[left.len() - n..]
                .iter()
                .zip(&right[..n])
                .all(|(a, b)| norm(a) == norm(b))
        })
        .unwrap_or(0);
    let tail = right[overlap..].join(" ");
    if tail.is_empty() {
        prefix.into()
    } else {
        format!("{prefix} {tail}")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires WHISPER_TEST_MODEL and WHISPER_TEST_PCM (16 kHz mono f32 LE)"]
    fn real_model_smoke() {
        let model = std::env::var("WHISPER_TEST_MODEL").unwrap();
        let raw = std::fs::read(std::env::var("WHISPER_TEST_PCM").unwrap()).unwrap();
        let samples: Vec<_> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let engine = Engine::load(Path::new(&model)).unwrap();
        let text = engine
            .finish(&samples, "en-US", Arc::new(AtomicBool::new(false)))
            .unwrap();
        println!("Transcript: {text}");
        assert!(engine
            .transcribe(&samples, "en", Arc::new(AtomicBool::new(true)))
            .is_err());
        assert_eq!(
            engine
                .transcribe(&vec![0.0; RATE], "en", Arc::new(AtomicBool::new(false)))
                .unwrap(),
            ""
        );
        assert!(text.to_lowercase().contains("today we are testing"));
        assert!(text.to_lowercase().contains("purple elephant"));
        assert!(text.to_lowercase().contains("silver bicycle"));
        assert!(engine
            .finish(&samples, "en", Arc::new(AtomicBool::new(true)))
            .is_err());
    }
    #[test]
    fn rolling_previews_keep_earlier_passages() {
        assert_eq!(
            merge(
                "First passage. Keep these final words.",
                "these final words, then continue."
            ),
            "First passage. Keep these final words. then continue."
        );
    }
    #[test]
    fn collapse_does_not_erase_text() {
        assert_eq!(
            merge("The whole earlier passage.", "Next."),
            "The whole earlier passage. Next."
        );
    }
    #[test]
    fn unicode_overlap_and_new_content_survive() {
        assert_eq!(
            merge("नमस्ते मेरे सभी शब्द", "सभी शब्द सही लिखें"),
            "नमस्ते मेरे सभी शब्द सही लिखें"
        );
    }
    #[test]
    fn distinct_vowel_marks_do_not_create_false_overlap() {
        assert_eq!(merge("कि कि", "की की आगे"), "कि कि की की आगे");
    }
    #[test]
    fn unrelated_repeated_words_are_not_deleted() {
        assert_eq!(merge("Yes", "Yes again"), "Yes Yes again");
    }
}
