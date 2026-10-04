# Local Whisper transcription

Branch: `codex/local-whisper-transcription`.

Dictate → Transcription source uses the same selection rows, readiness badges,
model dropdown and download progress styles as Voices. Whisper defaults to the
normal multilingual Small model (488 MB). Tiny, Base, Medium, Large v3 Turbo and
Large v3 are optional downloads. Existing settings migrate to macOS speech
recognition; new installations select Whisper. No API key or environment variable
is required.

Microphone permission is required for either source. Apple's Speech Recognition
permission is required only for macOS speech recognition. Accessibility still
controls insertion and hold shortcuts. Dictate's source, model and language also
apply to Assistant's spoken requests. Cloud Assistant generation is a separate
setting; Whisper does not upload microphone audio.

The shared Rust engine embeds whisper.cpp through `whisper-rs` 0.16.0, with Metal
on macOS and the CPU backend elsewhere. Build machines need CMake and a C++
compiler. The existing Swift adapter captures and resamples microphone audio;
Windows will need its own capture and insertion adapter. The inference/model
code is reusable, but this change does not implement the Windows app.

Models download from a pinned Hugging Face revision in four bounded ranges.
The server must return exact ranges and sizes; a SHA-256 check must pass before
the model is marked installed. Downloads can be canceled or retried. Models live
in the app data directory's `whisper-models` folder and can be deleted through
Advanced model settings. Model files are not bundled in the installer.

One worker retains the selected model between requests, unloading after five
idle minutes. Previews run on coalesced audio snapshots, approximately every
three seconds when inference keeps up. Recordings are bounded to 90 seconds.
Final transcription uses overlapping 20-second windows covering the entire
recording, rather than replacing earlier text with the most recent window.
Cancellation aborts inference and ignores stale results. Transcription quality,
preview latency, and mixed-language detection depend on the audio and model;
choosing a specific language is more predictable than automatic detection.

`whisper-rs` 0.16.0's safe abort trampoline miscasts its closure allocation. The
engine instead passes a live `Arc<AtomicBool>` through a documented raw callback;
the owner stays alive until synchronous inference returns.

## Validation

```sh
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib whisper
```

An optional real-model smoke test takes an explicit model path and a synthetic
16 kHz mono little-endian float32 PCM fixture. The fixture must include an opening
“Today we are testing” and ending “purple elephant and silver bicycle”. It checks
that both ends survive final assembly and verifies cancellation and silence.

```sh
WHISPER_TEST_MODEL=/absolute/path/ggml-small.bin \
WHISPER_TEST_PCM=/absolute/path/live-english.f32 \
cargo test --manifest-path src-tauri/Cargo.toml --lib real_model_smoke -- --ignored --nocapture
```

The native capture test checks resampling, final flushing, and cancellation using
synthetic PCM without accessing the microphone. An interactive microphone and
insertion test still needs the running desktop app.
