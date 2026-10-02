> Updated interaction and settings design: [Direct output with Undo](2026-10-02-assistant-direct-output.md). The accepted revision below supersedes preview/accept and chat-panel behavior in this original design.

# Local assistant modes

Status: implemented on `codex/local-assistant-modes`; see the implementation verification report for SDK and live-device test limits.

## Purpose

Let users invoke TextHalo from another application, generate or transform text with Apple's on-device model, and optionally send the result through the existing speech pipeline. Users can configure shortcuts, instructions, context sources, and output behavior. Focus and insertion behavior must be predictable.

## Existing integration points

- `src-tauri/src/shortcuts.rs` registers configurable Speak and Stop actions and rejects collisions.
- `src-tauri/src/capture.rs` captures selected text through Accessibility or clipboard copying and retains the source application instance. It does not retain an editable element or insertion range.
- `src-tauri/src/overlay.rs` provides a non-focusing speech status window.
- `src-tauri/src/lib.rs` exposes `speak_text` and routes supplied text through `run_speech`.
- `src-tauri/src/config.rs` defines local Apple, Kokoro, and Chatterbox engines and hosted Fish speech.
- No Foundation Models bridge or screenshot assistant exists in the inspected app code.

## Interaction choices

User decision: three primary entry shortcuts, each configurable: Transcribe, Assistant, and the existing Speak selection. Preserve the existing Stop control separately. Do not assign global shortcuts to individual assistant modes.

- Transcribe is an explicit microphone action. User decision: hold the shortcut to record; release it to stop and finalize. Escape cancels. It transcribes speech without interpreting it as assistant instructions.
- Assistant is also push-to-talk: on shortcut press, capture the original selection and target before recording the spoken request; on release, finalize transcription and submit that request to the assistant. Keep the source application focused during recording and processing. Modes are inferred from the spoken request or selected in the result panel. A result panel supports typed follow-up requests.
- Speak selection keeps the existing direct text-to-speech behavior.

Transcription is newly included in scope by the user's correction. No microphone/transcription implementation was found in the inspected app code. Its native API, on-device support, language availability, and permissions must be verified during implementation planning; it must not silently upload audio.

## Assistant modes

| Mode | Source | Behavior | Focus and destination |
| --- | --- | --- | --- |
| Ask / Generate | Spoken request or typed follow-up; optional captured selection | Interpret request, generate a draft, or ask a clarification | Capture original target on shortcut press; show result without stealing focus; preview by default |
| Rewrite selection | Selected text plus spoken, saved, or typed instruction | Enhance, shorten, translate, or change tone | Preview; Replace only for a verified editable selection; otherwise Copy |
| Summarize and speak | Selected text plus assistant request | Summarize, then synthesize once | Requested by voice or in the assistant panel; playback and progress preserve source focus |
| Generate voiceover | Request and optional selection | Generate a narration script | Preview with Speak action; an explicit request to narrate can start speech directly |
| Screen context | Explicit active-window or selected-region capture | Explain, summarize, or generate from visible content | Capture before displaying assistant UI; no insertion destination inferred from an image |

General assistant requests may select generation or speech. Explicit mode and output choices override model inference. Ambiguous requests return a clarification. Text inside captured content is source material, not authority to change output destinations or perform actions.

## Cursor and focus contract

The keyboard-focused editable element determines an insertion target. Mouse position only influences panel placement; hovering does not choose a text field or application. Place the panel near the caret when bounds are available, otherwise on the source window's display, clamped to visible screen bounds.

Before showing any focusable UI, capture the source application instance, window identity, focused Accessibility element, selected range and text where available. An empty caret is an insertion target, not permission to read the entire field. No selection means no selection context; do not silently capture the screen or clipboard.

The existing Speak shortcut and Transcribe recording use non-focusing status UI. Holding Assistant captures the original context and records a spoken instruction without focusing a panel. Release starts recognition and assistant processing. Results and clarification prompts appear without stealing focus; clicking the panel deliberately focuses it for typed follow-up. Summarize-and-speak is requested through Assistant, not through a separate shortcut. Escape dismisses it; restore the original application only when the assistant still owns focus. A user who switched to another app keeps that app focused. Finishing a background generation never steals focus or pastes automatically.

Insert and Replace are explicit result actions. Revalidate the original application, window, editable element, and range/content before writing. If the target closed, changed, cannot be verified, is read-only, or is secure, keep the result in preview and offer Copy. Never blindly paste into whichever app is currently frontmost. A screenshot does not establish an editable target. Unsupported editors still get Preview, Copy, and Speak.

## Transcription cursor behavior

Capture the focused target when the shortcut is pressed and recording starts and keep the source app focused. On finalization, insert the transcript at the captured caret, or replace the captured selection, only if that same editable target and range remain verifiably unchanged and focused. Do not stream partial transcripts into the external editor. If focus or target content changes, retain the transcript in a non-focusing result view with Copy; do not activate or type into another app. With no editable target, produce a copyable transcript. Secure fields cannot be insertion targets.

This explicit transcription action permits automatic final insertion into a verified original target. Assistant-generated text still requires an explicit Insert or Replace result action. Starting transcription does not capture screenshots, read surrounding text, invoke the assistant, or start narration. Unavailable recognition or microphone permission shows a clear actionable state.

## Settings and output

Expose configurable hold-to-talk Transcribe and Assistant shortcuts, and the existing Speak selection shortcut, with the existing Stop binding retained separately. Preserve Speak/Stop defaults; new bindings begin unassigned. Assistant settings include saved writing instructions and selection context. The request determines preview or speech output; result controls provide Copy, Insert/Replace, and Speak. All assistant modes share one shortcut. Transcription settings include language and output preference (insert into verified original target, or copy). Assistant Insert and Replace remain explicit result actions.

Generation uses the on-device model with runtime availability checks. Speech uses the user's configured voice engine. Show when hosted Fish speech sends generated text to the service. No silent cloud fallback for generation. This SDK exposes text prompting only: screenshot support uses local Vision OCR, explicitly labeled as screen text, and requires screen capture permission. Visual image interpretation is unavailable in this build. Denial leaves text modes usable.

## Architecture

Add a Swift helper for Foundation Models, built and bundled through the Rust/Tauri build and release flow. Communicate using a versioned structured request/response protocol. Check actual SDK declarations before choosing minimum supported OS versions. Keep existing speech available on machines without the model.

Rust owns assistant jobs, cancellation, captured context, destination validation, and speech dispatch. The model returns a constrained action (show text, speak text, or clarify) and generated text; it never receives arbitrary command execution. UI renders a dedicated assistant panel and settings controls. Reuse speech lifecycle and stop behavior, ensuring canceled or superseded generations cannot begin playback or update newer results. Avoid applying hosted text enhancement a second time to a deliberately generated script.

Share microphone capture and transcription infrastructure between two distinct destinations: dictation insertion and assistant instructions. Add a recording lifecycle for recording, finalizing, completion, cancellation, and permission errors. Start once on shortcut key-down, ignore key repeat, and finalize once on release. Latch the destination and original context at key-down; do not switch destinations mid-recording. Only one microphone session can be active. Cancel safely on Escape, permission failure, or app shutdown. Verify release handling for configured shortcut chords, including modifiers released first, to prevent stuck recording. Empty or canceled recordings perform no insertion or assistant action. Permission setup must not automatically start recording after the user has released the shortcut. Prevent duplicate finalization and discard late transcription callbacks after cancellation. Keep recorded audio transient.

Keep screenshot images and context transient, outside normal logs and persisted settings. Existing speech history rules apply when users generate audio. Do not add continuous screen monitoring, autonomous clicking, or background clipboard reading in this feature.

## Validation

Automated checks cover shortcut collisions and settings migration, request/response validation, mode precedence, missing context, stale target refusal, and cancellation preventing delayed playback. Run frontend typecheck/build and relevant Rust tests. Validate Swift helper compilation and actual local model availability; report unavailable hardware/model states accurately.

Manual checks cover an editable field with a caret, editable selected text, read-only browser selection, an unsupported editor, a password field, switching apps during generation, closing the source window, multiple displays, Escape restoration, denied permissions, and screenshots excluding the assistant panel. Verify local and hosted voice routing separately. Test hold/release behavior for both shortcuts, ignored key repeat, modifier release order, empty recordings, competing shortcuts, Escape cancellation, late callbacks, microphone denial, missing language assets, focus changes during recording, and single final insertion at an unchanged caret or selection.

## References

- https://developer.apple.com/documentation/foundationmodels/analyzing-images-with-multimodal-prompting
- https://developer.apple.com/documentation/foundationmodels/expanding-generation-with-tool-calling

## Confirmed interaction contract

Confirmed entry points: Transcribe, Assistant, and existing Speak selection, with no per-mode assistant shortcuts. Confirmed activation: hold to record and release to process for both Transcribe and Assistant. Transcribe produces literal text; Assistant treats the transcript as the user request. Final dictation insertion requires an unchanged focused target; assistant-generated text uses preview with explicit Insert/Replace. Recording, processing, and result arrival preserve source focus.
