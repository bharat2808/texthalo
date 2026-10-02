# Local assistant implementation and verification

Branch: `codex/local-assistant-modes`.

## Delivered

- Configurable hold/release Transcribe and Assistant shortcuts; existing Speak and Stop preserved. New shortcuts start unassigned.
- On-device microphone recognition with explicit permission setup, key-repeat suppression, physical key/modifier release watchdog, timeout, and cancellation.
- Local structured assistant generation, typed follow-ups, exact-selection reading, and generated-text speech through the existing engine pipeline. Generated scripts bypass hosted enhancement to avoid a second rewrite.
- Non-activating result window, explicit Copy/Insert/Speak, original target/range validation, secure-field refusal, conditional source-focus restoration, and cancellation of owned speech.
- Explicit screenshot/window/region capture with local OCR. The installed SDK lacks image attachments; this implementation does not claim visual image understanding.
- Swift helper compiled and bundled by Tauri, microphone/speech usage descriptions, audio-input entitlement, weak-linked Foundation Models availability checks.
- Settings include language, dictation output, selection context, writing preferences, availability, permission setup, and a generated-audio transcription diagnostic.

## Decisions made during implementation

- Use local screenshot OCR because the installed FoundationModels interface has no Attachment API. Cost: diagrams and photographs are not semantically interpreted.
- Insert only through a verified Accessibility target. No blind simulated paste fallback. Cost: editors that do not expose a settable selected-text attribute require Copy.
- Keep the requested feature branch in the existing checkout. The pre-existing untracked `texthalo-server/` is untouched.
- Test with an isolated app identity when the workspace's existing `tauri dev` process interfered with native UI targeting. The test identity uses separate preferences and local Apple speech.

## Verification evidence

- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: 150 unit tests and 10 integration tests passed; 15 existing tests ignored (download/model/hardware requirements).
- `SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk npm test`: 20 passed. Without the explicit SDK, an existing Mach-O release test failed because `/usr/bin/clang` selected an incompatible CommandLineTools SDK.
- `npx playwright test`: 7 passed, including result controls, read-only insertion refusal, explicit OCR capture, recording cancellation, and hosted-speech disclosure refresh.
- `npm run build`: passed.
- Swift contract executable: 12 assertions passed, covering changed targets, UTF-16 selection bounds, early recognizer finalization, duplicate release, and late final results.
- Native protocol smoke: real Foundation Models text generation, narration intent, empty-request rejection, unmatched release, and canceled-generation suppression passed.
- Real isolated app generation-to-speech: typed narration request returned a welcome sentence and produced a valid 3.610-second, 22,050 Hz mono AIFF through the Apple speech pipeline. Result controls were inspected in the native webview; Insert was disabled without an editable target.
- Real app transcription diagnostic: generated Samantha audio recognized on-device as “The quick brown fox jumps over the lazy dog.” No ambient microphone recording was used.
- Debug app bundle built with ad-hoc signing. Deep/strict signature check, sidecar audio entitlement, usage descriptions, and weak FoundationModels linking verified.
- Independent review identified cancellation/owned-speech, completed-result Escape, stale hosted disclosure, and focus-restoration issues; each was addressed. Regression checks cover owned speech cancellation and hosted disclosure.

## Practical limits

- Accessibility approval is required for live cross-app selection/insertion. Changed or unsupported targets fall back to a copyable result.
- Live microphone hold/release audio capture and the full matrix of external editors/multiple displays still require hands-on acceptance testing. The microphone was intentionally not opened during automated testing.
- Ad-hoc rebuilds may invalidate macOS permission grants. Distribution builds need the normal Developer ID signing/notarization workflow; no production release was performed.
- Hosted Fish synthesis was not invoked during these tests; existing hosted routing and disclosure were exercised through code and UI regression tests.
