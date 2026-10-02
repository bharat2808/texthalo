# Assistant direct output and settings revision

Accepted in conversation on October 2, 2026. The user requested test-driven implementation on the existing `codex/local-assistant-modes` branch.

## Product behavior

Keep the core General, Voice, Account, Capture, and audio History experience. Move Accessibility setup out of General into Permissions. Permission prompts require an explicit setup action. Permissions also owns Microphone, Speech Recognition, and Screen Recording, with independent statuses and macOS settings links.

Shortcuts are grouped by Speech, Transcription, Assistant, and shared Stop/cancel. Transcription and Assistant remain separate hold/release actions. Existing shortcuts and user settings are preserved.

Transcription has its own sidebar section with language and output preference, a generated-audio diagnostic, and the latest 50 local transcripts. Each entry has timestamp, source app, Copy, and Delete. Clear history removes all transcript entries. No microphone audio or screenshot context is stored in this history. The language also applies to spoken Assistant requests.

Assistant settings focus on saved instructions, context selection, and Apple model availability. Remove the conversational assistant widget. A compact, non-activating status window shows recording, processing, errors, or result feedback. Narration routes directly through the existing configured speech backend and overlay. Hosted voice disclosure remains in Assistant settings.

Generated text automatically replaces the original selection or inserts at the original caret. Transcription does the same unless configured to copy. Successful insertion exposes scoped Undo. An unavailable, read-only, secure, changed, or unfocused target retains a copyable result instead of inserting elsewhere. Clarification responses are displayed without insertion; the user can hold Assistant again with more detail.

## Focus and Undo

The keyboard-focused Accessibility element determines the target; pointer location does not select a destination. Capture its original text and UTF-16 range before showing UI. Before inserting, require the original application to be frontmost, the original element to be focused and editable, and its value and range to be unchanged.

Record the exact expected post-edit value and the observed post-edit selection. Undo requires the same focused editable element with that unchanged post-edit snapshot, then replaces only TextHalo's inserted range with the original selected text. Never synthesize a generic Undo shortcut. If verification fails, preserve current text and explain that Undo is unavailable. The receipt is in memory and ends when a new request starts or the result is dismissed. Clicking Undo may restore the source app only when the assistant window owns focus.

Result controls include the request ID. Reject stale actions. Cancellation validation and mutation occur under one Rust mutex, and canceled generation cannot publish a later result or start speech.

## Context

Context options: selected text, selected text plus original-window OCR, or spoken request only. Window OCR is requested per Assistant invocation after release, before generation, against the window captured at shortcut press. Screen permission is requested only from Permissions, not mid-recording. This SDK build extracts visible text locally; it does not interpret pictures. Selection and OCR context remain separate for exact-selection narration. Disabling context also disables reading the selection.

## Reference

Freeflow Notes supplied interaction references: its non-activating recording overlay, note-editor review UI, and dedicated prompt settings. TextHalo follows the user's subsequent direct-application-with-Undo decision rather than copying Freeflow's preview confirmation or combined dictation/edit activation.
