//! Push-to-talk lifecycle: one recording and no work from stale completions.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Transcribe,
    Assistant,
}

#[derive(Default)]
pub struct HoldJob {
    serial: u64,
    held: Option<Mode>,
}
impl HoldJob {
    pub fn press(&mut self, mode: Mode) -> Option<u64> {
        if self.held.is_some() {
            return None;
        }
        self.serial += 1;
        self.held = Some(mode);
        Some(self.serial)
    }
    pub fn release(&mut self, mode: Mode) -> Option<u64> {
        if self.held != Some(mode) {
            return None;
        }
        self.held = None;
        Some(self.serial)
    }
    pub fn current(&self, id: u64) -> bool {
        id == self.serial
    }
    pub fn cancel(&mut self) {
        self.serial += 1;
        self.held = None;
    }
    pub fn finish(&mut self, id: u64) {
        if self.current(id) {
            self.held = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeat_and_competing_presses_do_not_restart_recording() {
        let mut job = HoldJob::default();
        let id = job.press(Mode::Transcribe).unwrap();
        assert_eq!(job.press(Mode::Transcribe), None);
        assert_eq!(job.press(Mode::Assistant), None);
        assert_eq!(job.release(Mode::Assistant), None);
        assert_eq!(job.release(Mode::Transcribe), Some(id));
        assert_eq!(job.release(Mode::Transcribe), None);
    }
    #[test]
    fn cancel_and_new_request_reject_old_results() {
        let mut job = HoldJob::default();
        let old = job.press(Mode::Assistant).unwrap();
        job.cancel();
        assert!(!job.current(old));
        assert_eq!(job.release(Mode::Assistant), None);
        let new = job.press(Mode::Transcribe).unwrap();
        assert_ne!(old, new);
        assert!(!job.current(old));
        assert!(job.current(new));
    }
    #[test]
    fn finish_does_not_release_another_recording() {
        let mut job = HoldJob::default();
        let old = job.press(Mode::Assistant).unwrap();
        job.release(Mode::Assistant);
        let new = job.press(Mode::Transcribe).unwrap();
        job.finish(old);
        assert_eq!(job.release(Mode::Transcribe), Some(new));
    }
}
