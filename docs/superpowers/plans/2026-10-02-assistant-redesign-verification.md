# Assistant UI revision verification

## Implemented

- Dedicated Permissions, Transcription, and Assistant sections; categorized shortcuts.
- Preserved core speech settings and audio history, removed Accessibility card and automatic request from General.
- Immediate generated-text insertion/replacement with scoped, snapshot-checked Undo.
- Compact status/result surface without chat composer; copy fallback and direct narration.
- Optional original-window OCR during Assistant requests.
- Persistent newest-first transcript history limited to 50, copy/delete/clear controls.

## Test-driven checks

New browser tests were run against the old UI first: they failed on missing Undo, the old manual insertion action, the General permission card, and missing dedicated pages. Separate regressions reproduced automatic Accessibility prompts and incorrect denied Speech Recognition handling before fixes.

Native contract and Rust history tests initially failed to compile because the new receipt/history APIs did not exist, then passed after implementation. Native contracts cover UTF-16 replacement/insertion, unchanged-target requirements, later text/selection changes, wrong element, non-editable fields, invalid ranges, and hold/release single delivery. The native contract executable now runs as part of `npm test` on macOS.

Validation commands:

- `SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk npm test`
- `npm run test:e2e`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run build`
- `python3 scripts/assistant/native-smoke.py src-tauri/binaries/texthalo-assistant-aarch64-apple-darwin`
- Ad hoc signed debug `.app` build with isolated `com.kiegen.assistant-test` identifier.
- `git diff --check`

The SDKROOT override addresses the existing command-line compiler/SDK mismatch in the Mach-O packaging tests. Existing download/hardware-dependent Rust tests remain ignored.

Verified totals: 21 JavaScript tests (including the Swift contract executable), 10 browser tests, and 163 Rust tests passed. Fifteen existing Rust tests remain ignored. Frontend build, debug app bundling, and whitespace checks passed.

## Native evidence and limits

Real on-device generation, narration intent classification, and cancellation passed using synthetic prompts without recording the microphone. The rebuilt test app's Assistant and Transcription pages were inspected through native accessibility and screenshots. Code review findings about exact-selection narration, permission enums, and stale cancellation were fixed and reviewed again.

A synthetic TextEdit cross-app test was attempted. The native helper observed ChatGPT as frontmost rather than TextEdit after UI automation, so it correctly used the non-editable fallback. This does not verify actual insertion or Undo. Live microphone holds, release timing, source focus restoration, successful cross-app AX insertion/Undo, multi-monitor placement, and screen-context capture still require manual validation. The existing generated-audio transcription diagnostic remains available in Transcription settings; no ambient microphone audio was recorded for this revision.

No release, production deployment, permission grant, or hosted narration request was made for this revision.
