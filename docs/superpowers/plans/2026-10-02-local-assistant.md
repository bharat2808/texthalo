# Hold-to-talk assistant implementation plan

> Implement inline using executing-plans and test-driven-development.

Goal: configurable hold-to-talk dictation and spoken assistant requests, connected to existing speech, with safe focus handling.
Architecture: a bundled Swift native helper owns microphone, on-device recognition, Foundation Models, Accessibility targets, and local screenshot OCR. Rust owns shortcut transitions, job identities, cancellation, helper lifecycle, and speech dispatch. React renders settings and a non-activating result panel.
Tech stack: Tauri/Rust, React/TypeScript, Swift AVFoundation/Speech/FoundationModels/Vision.
Spec: ../specs/2026-10-02-local-assistant-design.md

## Constraints and review focus
- Three entry actions: Transcribe and Assistant hold/release; Speak selection unchanged; preserve Stop.
- No cloud recognition/generation fallback. Fish speech remains visibly hosted.
- Repeated presses, early releases, Escape, cancellation, stale completions and competing shortcuts cannot produce duplicate actions or stuck recording.
- Dictation inserts only into an unchanged original focused editable target; assistant insertion is explicit.
- No UI focus stealing on completion. Screen capture is explicit; this SDK supports OCR context only.

## Task 1 — shortcut and job contracts
- [x] Add failing Rust tests for new shortcut migration/collisions and a standalone hold lifecycle (repeat, wrong release, cancellation, stale result).
- [x] Implement settings and assistant lifecycle in config.rs, shortcuts.rs, assistant_job.rs.
- [x] Run focused Rust tests.

## Task 2 — native bridge
- [x] Add executable protocol tests for availability, empty requests, cancel/early release, and unavailable microphone state without recording.
- [x] Implement native/Assistant.swift with structured replies, on-device Speech, Foundation Models, target validation, and Vision OCR.
- [x] Build helper in build.rs; bundle as external binary and add usage descriptions/audio entitlements.
- [x] Compile and run native protocol checks and model smoke tests when available.

## Task 3 — app integration and interface
- [x] Implement assistant.rs process lifecycle, commands and non-activating panel; route structured speech through existing run_speech with enhancement disabled.
- [x] Connect key-down/up in lib.rs, cancel on Stop/settings changes/exit, retain current Speak behavior.
- [x] Add AssistantPanel.tsx and settings component with language, prompts, outputs, permissions, screen context, preview/copy/insert/speak/follow-up.
- [x] Add browser tests for settings and result interactions; run typecheck/build.

## Task 4 — end-to-end verification and review
- [x] Run Rust suite, npm test, browser tests, Swift checks and unsigned app bundle build.
- [x] Inspect bundle helper/permissions; test native model and speech using available fixtures without pretending mocked microphone is live input.
- [x] Obtain independent final code review; fix material findings and rerun affected checks.
- [x] Document exact results and any live microphone/TCC limitations; leave branch ready for review, no release/deployment.

## Execution notes
- User explicitly requested full implementation and testing; proceed inline.
- Installed FoundationModels Swift interface has no image Attachment API. Use explicitly labeled screenshot OCR rather than unsupported multimodal calls. Visual image understanding remains unavailable in this build.

Validation details and material limits: [verification report](2026-10-02-local-assistant-verification.md).
