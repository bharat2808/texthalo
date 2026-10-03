//! Bounded text transport shared by local and hosted speech.
use std::sync::mpsc::{self, Receiver, SyncSender};

/// Normalize only at engine boundaries. Preserve spaces at streamed chunk edges.
pub fn normalize(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase)
        .map(|c| if c.is_whitespace() { ' ' } else { c }).collect()
}

pub const MAX_CHARS: usize = 20_000;
pub enum SpeechEvent {
    Delta(String),
    Finish,
    // Fault injection for consumer tests. Production generation failures stop
    // the speech job directly through begin_generated_stream's error path.
    #[cfg(test)]
    Error(String),
}
pub enum SpeechInput {
    Complete(String),
    Stream(Receiver<SpeechEvent>),
}
pub fn channel() -> (SyncSender<SpeechEvent>, SpeechInput) {
    let (tx, rx) = mpsc::sync_channel(32);
    (tx, SpeechInput::Stream(rx))
}

#[cfg(test)]
mod tests {
    #[test]
    fn audio_text_is_lowercase_and_has_no_line_breaks() {
        assert_eq!(super::normalize("HELLO\r\nWorld\u{2028}ÉCOLE"), "hello  world école");
    }
    #[test]
    fn streamed_normalization_preserves_word_boundaries() {
        let parts = ["HEL", "LO\n", "WORLD"];
        let text: String = parts.iter().map(|part| super::normalize(part)).collect();
        assert_eq!(text, "hello world");
    }
}
