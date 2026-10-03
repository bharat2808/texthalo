//! Bounded, ordered producer/consumer playback for models that return audio buffers.
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::time::Duration;

/// Split at natural boundaries, with a short first phrase for early playback.
#[cfg(test)]
pub fn chatterbox_chunks(text: &str) -> Vec<String> {
    split(text, 100, 160, true)
}

/// Provisional Kokoro limits: preserve complete sentences up to the chunk budget.
/// These are separate from Chatterbox's shorter synthesis budget.
#[cfg(test)]
pub fn kokoro_chunks(text: &str) -> Vec<String> {
    split(text, 180, 240, true)
}

#[cfg(test)]
fn split(text: &str, first: usize, later: usize, natural: bool) -> Vec<String> {
    split_limit(text, first, later, natural, usize::MAX)
}

fn split_limit(
    text: &str,
    first: usize,
    later: usize,
    natural: bool,
    max_chunks: usize,
) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() && out.len() < max_chunks {
        let limit = if out.is_empty() { first } else { later };
        let window: Vec<(usize, char)> = rest.char_indices().take(limit).collect();
        let hard_end = window.last().map(|(i, c)| i + c.len_utf8()).unwrap();
        let sentence = window.iter().find_map(|(i, c)| {
            let mut end = i + c.len_utf8();
            for closing in rest[end..].chars().take_while(|c| "\"'”’)]}".contains(*c)) {
                if end + closing.len_utf8() > hard_end {
                    break;
                }
                end += closing.len_utf8();
            }
            let next = rest[end..].chars().next();
            let boundary = "。！？\n".contains(*c)
                || (".!?".contains(*c) && next.is_none_or(char::is_whitespace));
            let abbreviation = natural && *c == '.' && {
                let word = rest[..*i]
                    .split_whitespace()
                    .last()
                    .unwrap_or("")
                    .trim_start_matches(['“', '"', '(']);
                [
                    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g", "i.e",
                ]
                .contains(&word.to_lowercase().as_str())
                    || (word.len() == 1 && word.chars().all(char::is_uppercase))
            };
            (boundary && !abbreviation).then_some(end)
        });
        let end = if let Some(end) = sentence {
            end
        } else if hard_end == rest.len() {
            hard_end
        } else {
            let clause = natural
                .then(|| {
                    window
                        .iter()
                        .rev()
                        .find(|(i, c)| *i > 0 && ",;:，；：".contains(*c))
                        .map(|(i, c)| i + c.len_utf8())
                })
                .flatten();
            clause.unwrap_or_else(|| {
                window
                    .iter()
                    .rev()
                    .find(|(i, c)| {
                        *i > 0 && (c.is_whitespace() || (!natural && ",;:，；：".contains(*c)))
                    })
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(hard_end)
            })
        };
        out.push(rest[..end].to_string());
        rest = &rest[end..];
        // Keep whitespace with the preceding chunk when it fits the same budget.
        let room = limit - out.last().unwrap().chars().count();
        let spaces = rest
            .char_indices()
            .take(room)
            .take_while(|(_, c)| c.is_whitespace())
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        out.last_mut().unwrap().push_str(&rest[..spaces]);
        rest = &rest[spaces..];
    }
    out
}

#[cfg(test)]
pub fn run<T: Send, R, P, C>(
    chunks: Vec<String>,
    mut render: R,
    mut play: P,
    cancelled: C,
) -> Result<(), String>
where
    R: FnMut(&str) -> Result<T, String> + Send,
    P: FnMut(T) -> Result<(), String>,
    C: Fn() -> bool + Sync,
{
    std::thread::scope(|scope| {
        // One ready chunk may wait while the next is synthesized and the current
        // chunk plays. Backpressure bounds memory independently of selection length.
        let (tx, rx) = sync_channel(1);
        let cancel = &cancelled;
        let producer = scope.spawn(move || {
            for chunk in chunks {
                if cancel() {
                    break;
                }
                let audio = render(&chunk);
                let failed = audio.is_err();
                if cancel() || tx.send(audio).is_err() || failed {
                    break;
                }
            }
        });
        let result = loop {
            if cancelled() {
                break Ok(());
            }
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(audio)) => {
                    if cancelled() {
                        break Ok(());
                    }
                    if let Err(error) = play(audio) {
                        break Err(error);
                    }
                }
                Ok(Err(error)) => break Err(error),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        // Unblock a producer waiting on a full queue after Stop or a player error.
        drop(rx);
        producer
            .join()
            .map_err(|_| "speech synthesis worker failed".to_string())?;
        result
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn kokoro_keeps_sentence_context_and_abbreviations() {
        let text = "Dr. Smith checked the first result before deciding whether the next sentence needed more context for the voice to sound natural.";
        assert_eq!(super::kokoro_chunks(text), vec![text]);
        let long = "word ".repeat(150);
        let chunks = super::kokoro_chunks(&long);
        assert_eq!(chunks.concat(), long);
        assert!(chunks[0].chars().count() <= 180);
        assert!(chunks.iter().all(|c| c.chars().count() <= 240));
    }

    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    #[test]
    fn first_audio_plays_before_later_audio_finishes_rendering() {
        let (played, wait_for_play) = mpsc::channel();
        let mut heard = Vec::new();
        run(
            vec!["first".into(), "second".into(), "third".into()],
            move |text| {
                if text == "second" {
                    wait_for_play
                        .recv_timeout(Duration::from_secs(1))
                        .map_err(|_| "waited for all synthesis before playback".to_string())?;
                }
                Ok(text.to_string())
            },
            |audio| {
                if audio == "first" {
                    played.send(()).unwrap();
                }
                heard.push(audio);
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert_eq!(heard, ["first", "second", "third"]);
    }

    #[test]
    fn backpressure_bounds_synthesis_ahead_of_a_slow_player() {
        use std::sync::{atomic::AtomicUsize, Arc};
        let rendered = Arc::new(AtomicUsize::new(0));
        let count = rendered.clone();
        let (third, waiting_for_third) = mpsc::channel();
        let error = run(
            (0..100).map(|i| i.to_string()).collect(),
            move |text| {
                let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 3 {
                    third.send(()).unwrap();
                }
                Ok(text.to_string())
            },
            |_| {
                waiting_for_third
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                // Playing one, one ready, and one waiting to enter the bounded queue.
                assert_eq!(rendered.load(Ordering::SeqCst), 3);
                Err("player stopped".to_string())
            },
            || false,
        )
        .unwrap_err();
        assert_eq!(error, "player stopped");
        assert_eq!(rendered.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn cancellation_discards_queued_audio() {
        let stopped = AtomicBool::new(false);
        let mut heard = Vec::new();
        run(
            vec!["first".into(), "second".into(), "third".into()],
            |text| Ok(text.to_string()),
            |audio| {
                heard.push(audio);
                stopped.store(true, Ordering::SeqCst);
                Ok(())
            },
            || stopped.load(Ordering::SeqCst),
        )
        .unwrap();
        assert_eq!(heard, ["first"]);
    }

    #[test]
    fn a_later_synthesis_failure_preserves_already_played_audio_and_reports_failure() {
        let mut heard = Vec::new();
        let error = run(
            vec!["first".into(), "broken".into()],
            |text| {
                if text == "broken" {
                    Err("decoder failed".to_string())
                } else {
                    Ok(text.to_string())
                }
            },
            |audio| {
                heard.push(audio);
                Ok(())
            },
            || false,
        )
        .unwrap_err();
        assert_eq!(heard, ["first"]);
        assert_eq!(error, "decoder failed");
    }

    #[test]
    fn text_is_preserved_and_the_first_sentence_can_start_early() {
        let text = "Hello there. This is the next sentence. And another one follows.";
        let parts = chatterbox_chunks(text);
        assert!(parts.len() > 1);
        assert_eq!(parts.concat(), text);
        assert_eq!(parts[0].trim(), "Hello there.");
    }

    #[test]
    fn long_unpunctuated_and_unicode_text_is_bounded_without_lost_characters() {
        for text in ["word ".repeat(100), "世界你好".repeat(100)] {
            let parts = chatterbox_chunks(&text);
            assert!(parts.iter().all(|part| part.chars().count() <= 160));
            assert!(parts[0].chars().count() <= 100);
            assert_eq!(parts.concat(), text);
        }
    }

    #[test]
    fn decimals_are_not_sentence_boundaries() {
        assert_eq!(
            chatterbox_chunks("It costs 3.50 dollars. Next sentence.")[0].trim(),
            "It costs 3.50 dollars."
        );
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use crate::speech_input::{channel, SpeechEvent};
    use std::sync::atomic::{AtomicBool, Ordering};
    #[test]
    fn speaks_sentence_before_eof_and_flushes_unpunctuated_tail() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("Hello. ".into())).unwrap();
        let mut heard = Vec::new();
        let text = run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |audio, _stopped| {
                if let Some(audio) = audio {
                    if heard.is_empty() {
                        tx.send(SpeechEvent::Delta("Final words".into())).unwrap();
                        tx.send(SpeechEvent::Finish).unwrap();
                    }
                    heard.push(audio);
                }
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert_eq!(text, "Hello. Final words");
        assert_eq!(heard.concat(), text);
    }
    #[test]
    fn complete_readings_preserve_the_existing_configured_character_limit() {
        let text = "Long reading. ".repeat(1600);
        let mut heard = String::new();
        let full = run_input(
            crate::speech_input::SpeechInput::Complete(text.clone()),
            100,
            160,
            |s| Ok(s.to_owned()),
            |audio, _stopped| {
                if let Some(audio) = audio {
                    heard.push_str(&audio);
                }
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert_eq!(full, text);
        assert_eq!(heard, text);
    }
    #[test]
    fn quoted_first_sentence_renders_before_following_input_arrives() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("“Hi!” ".into())).unwrap();
        let began = std::time::Instant::now();
        let mut heard = Vec::new();
        run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |audio, _stopped| {
                if let Some(audio) = audio {
                    if heard.is_empty() {
                        tx.send(SpeechEvent::Delta("Final words".into())).unwrap();
                        tx.send(SpeechEvent::Finish).unwrap();
                    }
                    heard.push(audio);
                } else if began.elapsed() > Duration::from_millis(300) {
                    return Err("quoted opening waited for more input".into());
                }
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert_eq!(heard, ["“Hi!” ", "Final words"]);
    }
    #[test]
    fn queued_input_error_discards_pending_sentences_before_synthesis() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("Pending sentence. ".repeat(100)))
            .unwrap();
        tx.send(SpeechEvent::Error("generation failed".into()))
            .unwrap();
        let rendered = std::sync::atomic::AtomicUsize::new(0);
        let mut played = 0;
        let result = run_input(
            input,
            100,
            160,
            |_| {
                rendered.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            |audio, _stopped| {
                if audio.is_some() {
                    played += 1;
                }
                Ok(())
            },
            || false,
        );
        assert_eq!(result.unwrap_err(), "generation failed");
        assert_eq!(rendered.load(Ordering::SeqCst), 0);
        assert_eq!(played, 0);
    }
    #[test]
    fn input_error_interrupts_producer_backpressure_and_player_wait() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("Pending sentence. ".repeat(100)))
            .unwrap();
        let (ready, third_rendered) = std::sync::mpsc::channel();
        let mut count = 0;
        let result = run_input(
            input,
            100,
            160,
            |_| {
                count += 1;
                if count == 3 {
                    ready.send(()).unwrap();
                }
                Ok(())
            },
            |audio, stopped| {
                if audio.is_some() {
                    third_rendered.recv_timeout(Duration::from_secs(1)).unwrap();
                    tx.send(SpeechEvent::Error("generation failed".into()))
                        .unwrap();
                    let until = std::time::Instant::now() + Duration::from_millis(300);
                    while !stopped() && std::time::Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    assert!(stopped(), "input error did not interrupt the player wait");
                }
                Ok(())
            },
            || false,
        );
        assert_eq!(result.unwrap_err(), "generation failed");
    }
    #[test]
    fn quoted_abbreviations_keep_their_sentence_context() {
        assert_eq!(
            chatterbox_chunks("“Dr. Smith is ready.” Next sentence.")[0],
            "“Dr. Smith is ready.” "
        );
    }
    #[test]
    fn token_boundary_does_not_turn_a_decimal_into_a_sentence() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("It costs 3.".into())).unwrap();
        tx.send(SpeechEvent::Delta("50 dollars. Next".into()))
            .unwrap();
        tx.send(SpeechEvent::Finish).unwrap();
        let mut heard = Vec::new();
        run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |audio, _stopped| {
                if let Some(audio) = audio {
                    heard.push(audio);
                }
                Ok(())
            },
            || false,
        )
        .unwrap();
        assert_eq!(heard, ["It costs 3.50 dollars. ", "Next"]);
    }
    #[test]
    fn player_failure_unblocks_open_text_producer() {
        let (_tx, input) = channel();
        assert_eq!(
            run_input(
                input,
                100,
                160,
                |s| Ok(s.to_owned()),
                |_, _stopped| Err("player failed".into()),
                || false
            )
            .unwrap_err(),
            "player failed"
        );
    }
    #[test]
    fn text_limit_rejects_oversized_unicode_input() {
        let (tx, input) = channel();
        tx.send(SpeechEvent::Delta("界".repeat(20_001))).unwrap();
        tx.send(SpeechEvent::Finish).unwrap();
        assert!(run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |_, _stopped| Ok(()),
            || false
        )
        .is_err());
    }
    #[test]
    fn cancellation_unblocks_producer_waiting_for_text() {
        let (_tx, input) = channel();
        let cancelled = AtomicBool::new(false);
        run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |_, _| {
                cancelled.store(true, Ordering::SeqCst);
                Ok(())
            },
            || cancelled.load(Ordering::SeqCst),
        )
        .unwrap();
    }
    #[test]
    fn disconnect_and_explicit_error_never_mean_success() {
        let (tx, input) = channel();
        drop(tx);
        assert!(run_input(
            input,
            100,
            160,
            |s| Ok(s.to_owned()),
            |_, _stopped| Ok(()),
            || false
        )
        .is_err());
        let (tx, input) = channel();
        tx.send(SpeechEvent::Error("generation failed".into()))
            .unwrap();
        assert_eq!(
            run_input(
                input,
                100,
                160,
                |s| Ok(s.to_owned()),
                |_, _stopped| Ok(()),
                || false
            )
            .unwrap_err(),
            "generation failed"
        );
    }
}

fn drain_ready_input(
    receiver: &std::sync::mpsc::Receiver<crate::speech_input::SpeechEvent>,
    full: &mut String,
    pending: &mut String,
    finished: &mut bool,
) -> Result<(), String> {
    use crate::speech_input::{SpeechEvent, MAX_CHARS};
    while !*finished {
        match receiver.try_recv() {
            Ok(SpeechEvent::Delta(text)) => {
                if full.chars().count() + text.chars().count() > MAX_CHARS {
                    return Err("Speech input exceeds 20000 characters".into());
                }
                full.push_str(&text);
                pending.push_str(&text);
            }
            Ok(SpeechEvent::Finish) => *finished = true,
            #[cfg(test)]
            Ok(SpeechEvent::Error(error)) => return Err(error),
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("Speech input ended without EOF".into())
            }
        }
    }
    Ok(())
}

/// Render an open text source with one ready audio chunk. `None` ticks let the
/// consumer start a short opening buffer even while generation is still pending.
/// The returned text is complete only after explicit EOF; disconnect is an error.
pub fn run_input<T: Send, R, P, C>(
    input: crate::speech_input::SpeechInput,
    first: usize,
    later: usize,
    mut render: R,
    mut play: P,
    cancelled: C,
) -> Result<String, String>
where
    R: FnMut(&str) -> Result<T, String> + Send,
    P: FnMut(Option<T>, &dyn Fn() -> bool) -> Result<(), String>,
    C: Fn() -> bool + Sync,
{
    use crate::speech_input::{SpeechEvent, SpeechInput, MAX_CHARS};
    use std::sync::atomic::{AtomicBool, Ordering};
    let aborted = AtomicBool::new(false);
    let stop = || cancelled() || aborted.load(Ordering::Acquire);
    std::thread::scope(|scope| {
        let (tx, rx) = sync_channel(1);
        let stop_ref = &stop;
        let producer_aborted = &aborted;
        let producer = scope.spawn(move || -> Result<String, String> {
            let result = (|| {
                let (mut pending, receiver, mut finished) = match input {
                    SpeechInput::Complete(text) => (text, None, true),
                    SpeechInput::Stream(rx) => (String::new(), Some(rx), false),
                };
                let mut full = pending.clone();
                let mut first_chunk = true;
                loop {
                    if stop_ref() {
                        return Ok(full);
                    }
                    if let Some(receiver) = &receiver {
                        drain_ready_input(receiver, &mut full, &mut pending, &mut finished)?;
                    }
                    if receiver.is_some() && full.chars().count() > MAX_CHARS {
                        return Err("Speech input exceeds 20000 characters".into());
                    }
                    let budget = if first_chunk { first } else { later };
                    // A sentinel distinguishes an actual complete sentence from split's EOF flush.
                    let probe = if finished {
                        pending.clone()
                    } else {
                        format!("{pending}sentinel")
                    };
                    let chunk = split_limit(&probe, budget, budget, true, 1)
                        .into_iter()
                        .next();
                    if let Some(chunk) = chunk.filter(|chunk| {
                        finished
                            || (chunk.len() <= pending.len()
                                && (pending.chars().count() >= budget
                                    || (chunk.contains('\n')
                                        || chunk
                                            .trim_end()
                                            .trim_end_matches(['\"', '\'', '”', '’', ')', ']', '}'])
                                            .ends_with(['.', '!', '?', '。', '！', '？']))))
                    }) {
                        pending.drain(..chunk.len());
                        if !chunk.trim().is_empty() {
                            first_chunk = false;
                            let mut audio = render(&chunk)?;
                            loop {
                                if let Some(receiver) = &receiver {
                                    drain_ready_input(
                                        receiver,
                                        &mut full,
                                        &mut pending,
                                        &mut finished,
                                    )?;
                                }
                                if stop_ref() {
                                    return Ok(full);
                                }
                                match tx.try_send(audio) {
                                    Ok(()) => break,
                                    Err(std::sync::mpsc::TrySendError::Full(waiting)) => {
                                        audio = waiting;
                                        std::thread::sleep(Duration::from_millis(10));
                                    }
                                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                                        return Ok(full)
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    if finished {
                        if full.trim().is_empty() {
                            return Err("nothing to say: the selection is empty".into());
                        }
                        return Ok(full);
                    }
                    match receiver
                        .as_ref()
                        .unwrap()
                        .recv_timeout(Duration::from_millis(20))
                    {
                        Ok(SpeechEvent::Delta(text)) => {
                            full.push_str(&text);
                            pending.push_str(&text);
                        }
                        Ok(SpeechEvent::Finish) => finished = true,
                        #[cfg(test)]
                        Ok(SpeechEvent::Error(error)) => return Err(error),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => {
                            return Err("Speech input ended without EOF".into())
                        }
                    }
                }
            })();
            if result.is_err() {
                producer_aborted.store(true, Ordering::Release);
            }
            result
        });
        let result = loop {
            if stop() {
                break Ok(());
            }
            let audio = match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(audio) => Some(audio),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            };
            if stop() {
                break Ok(());
            }
            if let Err(error) = play(audio, &stop) {
                break Err(error);
            }
        };
        aborted.store(true, Ordering::Release);
        drop(rx);
        let full = producer
            .join()
            .map_err(|_| "speech synthesis worker failed".to_string())?;
        result?;
        full
    })
}

pub fn should_start(buffered: f64, target: f64, waited: Duration, eof: bool) -> bool {
    buffered > 0.0 && (buffered >= target || eof || waited >= Duration::from_millis(500))
}
#[cfg(test)]
mod startup_tests {
    use super::*;
    #[test]
    fn short_first_audio_starts_on_timeout_without_waiting_for_more_text() {
        assert!(!should_start(0.1, 3.0, Duration::from_millis(100), false));
        assert!(should_start(0.1, 3.0, Duration::from_millis(500), false));
        assert!(!should_start(0.0, 3.0, Duration::from_secs(2), false));
    }
}
