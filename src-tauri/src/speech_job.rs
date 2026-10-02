use crate::{Phase, StatusEvent};
use std::time::Instant;

pub struct SpeechJob {
    pub id: u64,
    pub streaming: bool,
    pub status: StatusEvent,
    pub changed: Instant,
    generation: tokio::sync::watch::Sender<u64>,
}

impl Default for SpeechJob {
    fn default() -> Self {
        Self {
            id: 0,
            streaming: false,
            status: StatusEvent {
                phase: Phase::Idle,
                message: None,
                chars: None,
            },
            changed: Instant::now(),
            generation: tokio::sync::watch::channel(0).0,
        }
    }
}

impl SpeechJob {
    pub fn begin(&mut self, phase: Phase) -> u64 {
        self.id += 1;
        self.generation.send_replace(self.id);
        self.streaming = false;
        self.set(phase, None, None);
        self.id
    }
    pub fn is_current(&self, id: u64) -> bool {
        self.id == id
    }
    /// Subscribe while holding the job lock, but wait without retaining that lock.
    /// The stored generation also catches cancellation before the future is polled.
    pub fn cancellation(&self, id: u64) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut receiver = self.generation.subscribe();
        async move {
            while *receiver.borrow_and_update() == id {
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        }
    }
    pub fn cancel(&mut self) {
        self.id += 1;
        self.generation.send_replace(self.id);
        self.streaming = false;
        self.set(Phase::Idle, None, None);
    }
    pub fn cancel_if_source_terminated(&mut self, id: u64, terminated: bool) -> bool {
        if !terminated
            || !self.is_current(id)
            || matches!(self.status.phase, Phase::Idle | Phase::Error)
        {
            return false;
        }
        self.cancel();
        true
    }
    /// A gap in streamed playback means more audio is being prepared, not completion.
    pub fn observe_playback(&mut self, playing: bool) -> bool {
        if !playing && matches!(self.status.phase, Phase::Speaking) {
            self.set(
                if self.streaming {
                    Phase::Preparing
                } else {
                    Phase::Idle
                },
                None,
                None,
            );
            return true;
        }
        false
    }

    pub fn set(&mut self, phase: Phase, message: Option<String>, chars: Option<usize>) {
        self.status = StatusEvent {
            phase,
            message,
            chars,
        };
        self.changed = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_wakes_a_waiting_hosted_request() {
        let mut job = SpeechJob::default();
        let id = job.begin(Phase::Preparing);
        let cancelled = job.cancellation(id);
        tokio::pin!(cancelled);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut cancelled)
                .await
                .is_err()
        );
        job.cancel();
        tokio::time::timeout(std::time::Duration::from_millis(100), cancelled)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn replacement_cancels_only_the_previous_hosted_request() {
        let mut job = SpeechJob::default();
        let old = job.begin(Phase::Preparing);
        let cancelled = job.cancellation(old);
        let current = job.begin(Phase::Preparing);
        tokio::time::timeout(std::time::Duration::from_millis(100), cancelled)
            .await
            .unwrap();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(10),
            job.cancellation(current)
        )
        .await
        .is_err());
    }
    #[test]
    fn source_termination_cancels_pending_audio() {
        let mut job = SpeechJob::default();
        let id = job.begin(Phase::Preparing);
        assert!(job.cancel_if_source_terminated(id, true));
        assert!(!job.is_current(id));
        assert!(matches!(job.status.phase, Phase::Idle));
    }

    #[test]
    fn running_source_does_not_interrupt_playback() {
        let mut job = SpeechJob::default();
        let id = job.begin(Phase::Speaking);
        assert!(!job.cancel_if_source_terminated(id, false));
        assert!(job.is_current(id));
        assert!(matches!(job.status.phase, Phase::Speaking));
    }

    #[test]
    fn old_source_termination_cannot_cancel_a_new_request() {
        let mut job = SpeechJob::default();
        let old = job.begin(Phase::Speaking);
        let current = job.begin(Phase::Preparing);
        assert!(!job.cancel_if_source_terminated(old, true));
        assert!(job.is_current(current));
    }
    #[test]
    fn stop_during_processing_invalidates_pending_playback() {
        let mut job = SpeechJob::default();
        let pending = job.begin(Phase::Preparing);
        job.cancel();
        assert!(!job.is_current(pending));
        assert!(matches!(job.status.phase, Phase::Idle));
    }
    #[test]
    fn a_gap_between_chunks_keeps_the_job_preparing_until_stopped() {
        let mut job = SpeechJob::default();
        job.begin(Phase::Preparing);
        job.streaming = true;
        job.set(Phase::Speaking, None, None);
        assert!(job.observe_playback(false));
        assert!(matches!(job.status.phase, Phase::Preparing));
        job.cancel();
        assert!(!job.streaming);
        assert!(matches!(job.status.phase, Phase::Idle));
    }

    #[test]
    fn completed_non_streaming_playback_becomes_idle() {
        let mut job = SpeechJob::default();
        job.begin(Phase::Speaking);
        assert!(job.observe_playback(false));
        assert!(matches!(job.status.phase, Phase::Idle));
    }

    #[test]
    fn a_new_request_invalidates_the_previous_request() {
        let mut job = SpeechJob::default();
        let old = job.begin(Phase::Capturing);
        let new = job.begin(Phase::Preparing);
        assert!(!job.is_current(old));
        assert!(job.is_current(new));
    }
}
