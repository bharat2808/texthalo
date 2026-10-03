//! Bounded text transport shared by local and hosted speech.
use std::sync::mpsc::{self, Receiver, SyncSender};

pub const MAX_CHARS: usize = 20_000;
pub enum SpeechEvent {
    Delta(String),
    Finish,
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
