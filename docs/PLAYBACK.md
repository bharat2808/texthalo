# Floating audio playback

The reading widget stays visible while speech is preparing, playing, or paused. Hover or keyboard focus reveals back/forward 10 seconds, pause/resume, playback speed (0.5×–2×), and a seek bar. Stop stays visible in the status row. The window stays 350 points wide so its buttons remain in place when it expands.

The main-thread window tick checks the native cursor against the widget frame and emits hover changes, so controls also reveal while TextHalo is inactive. Hover never activates the app. The webview accepts the first control click.

Controls collapse 1.5 seconds after both pointer and focus leave. Paused readings keep Resume visible indefinitely. A completed reading shows Replay for five seconds; pointer or focus interaction postpones dismissal until interaction ends. Stop dismisses immediately. Errors retain the existing recovery panel until dismissed.

Apple selections render short WAV chunks through the system `say` command. Apple, Kokoro, Chatterbox, Fish streams, hosted voice previews, and saved recordings use the shared 24 kHz native PCM player. AudioQueue's spectral time/pitch processor changes playback rate without changing the selected synthesis settings. Seeking is limited to received audio; an unknown duration is shown as the available time followed by `+`. Seeking past available audio clamps to its boundary.

The current reading's PCM stays available for seeking and Replay. The producer still limits queued audio ahead of playback. Stop, replacement, and completion dismissal release retained audio. Fish continuation acknowledgements use a monotonic playback high-water mark so rewinding never requests the same provider chunk twice.

The Assistant's separate transcript widget retains its existing presentation; these controls belong to the floating reading player.

Verification: browser playback/localization regressions, native buffer and dismissal-policy tests, Apple WAV rendering, and the explicit native audio-device test `pcm::tests::native_controls_pause_seek_speed_drain_and_replay` (run with `--ignored`) cover the controls and lifecycle.
