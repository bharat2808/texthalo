//! Convert cumulative model snapshots into a non-repeating committed speech prefix.

#[derive(Clone, Default)]
pub struct SnapshotBuffer {
    committed: String,
    finished: bool,
}
impl SnapshotBuffer {
    pub fn update(&mut self, snapshot: &str, finished: bool) -> Result<String, String> {
        if snapshot.chars().count() > 20_000 {
            return Err("The generated answer exceeds the 20,000-character speech limit.".into());
        }
        if !snapshot.starts_with(&self.committed) || (self.finished && snapshot != self.committed) {
            return Err(
                "Generation revised words already sent to speech. Playback was stopped.".into(),
            );
        }
        if self.finished {
            return Ok(String::new());
        }
        let tail = &snapshot[self.committed.len()..];
        let mut end = if finished { tail.len() } else { 0 };
        if !finished {
            for (i, ch) in tail.char_indices() {
                if !".!?。！？".contains(ch) {
                    continue;
                }
                if ch == '.' {
                    let word = tail[..i]
                        .split_whitespace()
                        .last()
                        .unwrap_or("")
                        .trim_start_matches(['“', '"', '(']);
                    if [
                        "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g",
                        "i.e",
                    ]
                    .contains(&word.to_lowercase().as_str())
                        || (word.len() == 1 && word.chars().all(char::is_uppercase))
                    {
                        continue;
                    }
                }
                let mut boundary = i + ch.len_utf8();
                for closing in tail[boundary..]
                    .chars()
                    .take_while(|c| "\"'”’)]}".contains(*c))
                {
                    boundary += closing.len_utf8();
                }
                let next = tail[boundary..].chars().next();
                if "。！？".contains(ch) || next.is_some_and(char::is_whitespace) {
                    for space in tail[boundary..].chars().take_while(|c| c.is_whitespace()) {
                        boundary += space.len_utf8();
                    }
                    end = boundary;
                }
            }
            // Bound latency/memory for long unpunctuated narration too. The engine
            // applies its smaller synthesis budgets after this commit boundary.
            if end == 0 && tail.chars().count() >= 3000 {
                let window: Vec<_> = tail.char_indices().take(3000).collect();
                end = window
                    .iter()
                    .rev()
                    .find(|(_, c)| c.is_whitespace())
                    .or_else(|| window.last())
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
            }
        }
        let delta = tail[..end].to_owned();
        self.committed.push_str(&delta);
        self.finished = finished;
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ocr_line_wraps_wait_for_sentence_punctuation() {
        let mut buffer = SnapshotBuffer::default();
        assert_eq!(buffer.update("A sentence wraps\nacross lines", false).unwrap(), "");
        assert_eq!(buffer.update("A sentence wraps\nacross lines. Next", false).unwrap(),
                   "A sentence wraps\nacross lines. ");
    }

    #[test]
    fn waits_for_complete_sentence_and_never_repeats_snapshots() {
        let mut buffer = SnapshotBuffer::default();
        assert_eq!(buffer.update("Hello", false).unwrap(), "");
        assert_eq!(
            buffer.update("Hello there. Next", false).unwrap(),
            "Hello there. "
        );
        assert_eq!(buffer.update("Hello there. Next", false).unwrap(), "");
        assert_eq!(
            buffer.update("Hello there. Next sentence.", true).unwrap(),
            "Next sentence."
        );
        assert_eq!(
            buffer.update("Hello there. Next sentence.", true).unwrap(),
            ""
        );
    }
    #[test]
    fn permits_tail_corrections_but_rejects_rewriting_spoken_words() {
        let mut buffer = SnapshotBuffer::default();
        buffer.update("First sentence. A blue", false).unwrap();
        assert_eq!(
            buffer.update("First sentence. A red apple.", true).unwrap(),
            "A red apple."
        );
        let mut buffer = SnapshotBuffer::default();
        buffer.update("First sentence. ", false).unwrap();
        assert!(buffer.update("Changed sentence. ", false).is_err());
    }
    #[test]
    fn abbreviations_decimals_and_quoted_sentences_remain_whole() {
        let mut buffer = SnapshotBuffer::default();
        assert_eq!(buffer.update("Dr. Lee paid 3.50", false).unwrap(), "");
        assert_eq!(
            buffer
                .update("Dr. Lee paid 3.50 dollars. “Hello!” Next", false)
                .unwrap(),
            "Dr. Lee paid 3.50 dollars. “Hello!” "
        );
    }
    #[test]
    fn short_unpunctuated_answers_flush_only_at_eof() {
        let mut buffer = SnapshotBuffer::default();
        assert_eq!(buffer.update("Yes", false).unwrap(), "");
        assert_eq!(buffer.update("Yes", true).unwrap(), "Yes");
        assert!(buffer.update("Yes indeed", false).is_err());
    }
    #[test]
    fn very_long_sentences_flush_at_a_bounded_word_boundary() {
        let text = "word ".repeat(700);
        let mut buffer = SnapshotBuffer::default();
        let first = buffer.update(&text, false).unwrap();
        assert!(!first.is_empty());
        assert!(text.starts_with(&first));
        assert!(first.ends_with(' '));
        let tail = buffer.update(&text, true).unwrap();
        assert_eq!(first + &tail, text);
        assert!(SnapshotBuffer::default()
            .update(&"x".repeat(20001), true)
            .is_err());
    }
}

/// Forward a coalescing snapshot mailbox into the bounded speech input.
pub fn forward_snapshots(
    mut latest: tokio::sync::watch::Receiver<(String, bool)>,
    tx: std::sync::mpsc::SyncSender<crate::speech_input::SpeechEvent>,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    use crate::speech_input::SpeechEvent;
    use std::sync::mpsc::TrySendError;
    use std::time::Duration;
    let mut buffer = SnapshotBuffer::default();
    loop {
        if cancelled() {
            return Ok(());
        }
        // Inspect closure before reading: a sender can publish its final value and
        // drop while the speech queue is full. Always consume that retained value.
        let closed = latest.has_changed().is_err();
        let (text, finished) = latest.borrow_and_update().clone();
        let delta = buffer.update(&text, finished)?;
        let mut events = Vec::with_capacity(2);
        if !delta.is_empty() {
            events.push(SpeechEvent::Delta(delta));
        }
        if finished {
            events.push(SpeechEvent::Finish);
        }
        for mut event in events {
            loop {
                if cancelled() {
                    return Ok(());
                }
                // A revised committed prefix must abort even while backpressured.
                // Validate a copy so coalesced suffixes remain pending for delivery.
                let now_closed = latest.has_changed().is_err();
                let current = latest.borrow().clone();
                buffer.clone().update(&current.0, current.1)?;
                if now_closed && !current.1 {
                    return Err("Generation ended before the answer was complete.".into());
                }
                match tx.try_send(event) {
                    Ok(()) => break,
                    Err(TrySendError::Disconnected(_)) => {
                        return Err("Speech input closed before generation completed.".into())
                    }
                    Err(TrySendError::Full(waiting)) => {
                        event = waiting;
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
        if finished {
            return Ok(());
        }
        if closed {
            return Err("Generation ended before the answer was complete.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod forward_tests {
    use super::*;
    use crate::speech_input::SpeechEvent;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;

    #[test]
    fn saturated_queue_keeps_final_coalesced_snapshot_after_sender_closes() {
        let (feed, latest) = tokio::sync::watch::channel(("First. ".to_string(), false));
        let (tx, rx) = mpsc::sync_channel(32);
        for _ in 0..32 {
            tx.send(SpeechEvent::Delta(String::new())).unwrap();
        }
        let (waiting, blocked) = mpsc::channel();
        let polls = AtomicUsize::new(0);
        let worker = std::thread::spawn(move || {
            forward_snapshots(latest, tx, || {
                if polls.fetch_add(1, Ordering::SeqCst) == 1 {
                    waiting.send(()).unwrap();
                }
                false
            })
        });
        blocked
            .recv_timeout(Duration::from_secs(1))
            .expect("forwarder reached full queue");
        feed.send(("First. Intermediate".into(), false)).unwrap();
        feed.send(("First. Final words".into(), true)).unwrap();
        drop(feed);
        for _ in 0..32 {
            rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        let mut text = String::new();
        let mut eof = 0;
        while let Ok(event) = rx.recv_timeout(Duration::from_secs(1)) {
            match event {
                SpeechEvent::Delta(delta) => text.push_str(&delta),
                SpeechEvent::Finish => eof += 1,
                SpeechEvent::Error(error) => panic!("{error}"),
            }
        }
        worker.join().unwrap().unwrap();
        assert_eq!(text, "First. Final words");
        assert_eq!(eof, 1);
    }

    #[test]
    fn cancellation_unblocks_a_full_speech_queue() {
        let (_feed, latest) = tokio::sync::watch::channel(("First. ".to_string(), false));
        let (tx, _rx) = mpsc::sync_channel(32);
        for _ in 0..32 {
            tx.send(SpeechEvent::Delta(String::new())).unwrap();
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (done, result) = mpsc::channel();
        let (waiting, blocked) = mpsc::channel();
        let polls = AtomicUsize::new(0);
        std::thread::spawn(move || {
            done.send(forward_snapshots(latest, tx, || {
                if polls.fetch_add(1, Ordering::SeqCst) == 2 {
                    waiting.send(()).unwrap();
                }
                stop.load(Ordering::SeqCst)
            }))
            .unwrap();
        });
        blocked
            .recv_timeout(Duration::from_secs(1))
            .expect("forwarder is retrying full queue");
        cancelled.store(true, Ordering::SeqCst);
        assert!(result.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
    }

    #[test]
    fn revision_of_committed_text_returns_error() {
        let (feed, latest) = tokio::sync::watch::channel(("First. ".to_string(), false));
        let (tx, rx) = mpsc::sync_channel(32);
        let (done, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = done.send(forward_snapshots(latest, tx, || false));
        });
        assert!(
            matches!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), SpeechEvent::Delta(text) if text == "First. ")
        );
        feed.send(("Revised. ".into(), false)).unwrap();
        assert!(result
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap_err()
            .contains("revised"));
        worker.join().unwrap();
    }
    #[test]
    fn closed_unfinished_generation_returns_error_without_eof() {
        let (feed, latest) = tokio::sync::watch::channel(("Partial answer".to_string(), false));
        drop(feed);
        let (tx, rx) = mpsc::sync_channel(32);
        assert!(forward_snapshots(latest, tx, || false)
            .unwrap_err()
            .contains("before the answer was complete"));
        assert!(rx.try_recv().is_err());
    }
}
