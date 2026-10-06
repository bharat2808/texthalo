# Focused editor paste delivery

Branch: `codex/focused-editor-paste`, based on `master` (`9ce002c`).

## Reference and behavior

Freeflow Notes delivers transcripts through `Sources/ClipboardController.swift` and the completion path in `Sources/AppState+TranscriptionPipeline.swift`. It writes the transcript to the pasteboard, waits for the shortcut to release, then posts Cmd+V using the current keyboard layout. It does not require Accessibility text or caret attributes to deliver a paste.

TextHalo now uses this delivery pattern for dictation and completed Assistant answers that have not already been inserted by streaming. Missing `AXValue`, `AXSelectedTextRange`, or an AX editable role no longer prevents paste. The receiving app handles insertion at its currently focused editor and selection. The source app must remain active, Accessibility must remain granted, and identifiable secure fields are refused. No app activation or automatic Return is performed.

The release polling uses 25 ms intervals, up to 24 waits, then a 30 ms settling delay. A held shortcut after the budget expires cancels delivery rather than posting modified keystrokes. Paste uses the current keyboard layout with keycode 9 as fallback. Optional clipboard restoration runs after one second and preserves all readable pasteboard types; a newer clipboard generation always wins. Restoration also occurs if a write or event dispatch fails.

Accessibility snapshots remain available for context, streaming insertion and scoped Undo. A dispatched paste does not create an Undo receipt or promise that the receiving editor consumed it. A partly streamed answer never falls back to pasting the complete answer again.

The existing metadata-only insertion trace now creates its parent directory before writing. No new text, clipboard contents, audio or transcripts are logged by that trace.

## Verification

- Focused-paste regression tests use a real, isolated pasteboard and substitute the physical event dispatch and clock boundaries. They cover delivery without AX snapshots, held shortcuts, app switches during the settling delay, cancellation, rich clipboard data, newer clipboard copies, disabled restoration, event failures and failed clipboard writes.
- Full JavaScript test suite: 58 passed.
- Full Rust test suite: 243 passed, 19 optional model/hardware/network tests ignored.
- TypeScript/frontend build: passed.
- Rust formatting, diff whitespace checks, Clippy and unsigned debug app bundle: passed. The bundle is at `src-tauri/target/debug/bundle/macos/TextHalo.app`.
- Independent review found a clipboard-write failure regression; the test failed before the restoration fix and passed afterward. Re-review found no remaining blocker.

## Manual acceptance

Live delivery through the signed app into Codex, native editors and browser textareas/contenteditable fields remains pending. Automated tests do not prove compatibility with every receiving app or permission configuration. Test a caret and a selection, held shortcut release, an app switch while recording, clipboard restoration, and a newer copy before restoration. The installed app and macOS permissions have not been changed. No release, push or merge was performed.
