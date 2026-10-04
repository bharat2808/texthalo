# Hosted voice Assistant implementation

The implementation keeps the existing hold-to-talk Assistant flow. Local and hosted answers use the same Swift insertion, original-target validation, narration, clarification and Undo paths. No typing interface or fallback was added.

## Repositories and branches

- Desktop: `codex/feat-hosted-assistant-desktop`, based on `streaming-and-generation` (`9001467`).
- Separate server repository: `codex/feat-hosted-assistant-backend`, based on `streaming-and-generation-for-text` (`7fd2a9f`). Backend implementation commit: `33cfc2d7a406f2186b024cdd2bd49c534eaddc2b`.
- The desktop repository already records the server as a gitlink without `.gitmodules`. This change preserves that layout and updates the recorded backend revision; it does not invent a new monorepo or submodule checkout setup.

## How it works

1. The user selects On this Mac, TextHalo Cloud, or Automatic in Assistant settings. Existing installations remain local. Cloud requires explicit consent and the existing TextHalo sign-in.
2. The same shortcut records speech and transcribes locally. Swift captures the original target/selection and locally extracts window text. Cloud fulfillment never invokes Foundation Models for context selection or answer generation.
3. Rust sends the bounded request to `/v1/assistant/stream`, authenticated with the existing securely stored desktop credentials. Provider credentials stay on the server.
4. The backend reuses the existing `TEXT_ENHANCER_*` OpenAI-compatible endpoint configuration, credential helper, and shared SSE transport. Jev remains the TTS semantic-cue provider.
5. Assistant reserves the configured maximum request hold from the existing credit wallet, streams a validated action and text, then settles confirmed usage and refunds the remainder. The UI distinguishes temporary holds from final charges.
6. Rust converts deltas into native cumulative snapshots. Swift validates the current request/action and applies the answer just as it does for local generation. Late billing metadata cannot restart delivery.

The first release uses a fixed conservative `ASSISTANT_MAX_REQUEST_CREDITS` hold (default 1,000), not a tokenizer-based exact estimate. Users need that balance to begin; the final charge uses configured token rates and never exceeds the hold. This deliberately avoids an unverified cross-provider tokenizer estimate. Unused credits return to the original eligible grants through existing ledger logic.

## Activation

No production deployment, paid-provider request, real `.env` edit, or secret change was performed.

On the server, apply `migrations/011_assistant_requests.sql` before deploying the code. The new table and recovery discovery role are used by status/account recovery even when new cloud generation is disabled. Existing migrations must already be applied. Keep the minute recovery schedule enabled when cloud admissions are disabled.

Set `ASSISTANT_ENABLED=true` after verifying the existing `TEXT_ENHANCER_BASE_URL`, `TEXT_ENHANCER_MODEL`, authentication mode and credential configuration. An existing `DESKTOP_SESSION_SECRET` supplies the request fingerprint key unless `ASSISTANT_FINGERPRINT_KEY` is set. Verify Assistant tariff overrides or the inherited `ENHANCEMENT_*` rates against the actual selected model before offering paid usage; change `ASSISTANT_RATE_VERSION` with tariff/model policy changes. The server README and `.env.example` document all settings and recovery behavior.

The desktop uses its existing hosted API origin. Once the matching server is deployed, select Cloud and enable cloud processing in Assistant settings. Microphone, local Speech Recognition, Accessibility and applicable Screen Recording permissions are still required. This change does not provide hosted transcription if local recognition is unavailable.

## Verification and limits

- Backend: 229 tests passed, including real localhost HTTP SSE provider → authenticated WebSocket → production SQL repository → existing-wallet debit. Additional cases cover duplicate requests, RLS, atomic rollback, cancellation, malformed responses, missing selection, output bounds, lease recovery, stale owners and configuration.
- Backend Node build, Worker typecheck, router tests and Worker bundle build passed. Actual deployed Worker behavior and multi-instance production PostgreSQL contention remain deployment qualification tasks.
- Desktop browser suite: 45 tests passed; the two Cloud-specific tests also passed after final usage-label changes. Existing no-composer, cancellation, shortcut, local readiness, insertion-result and history UI tests remain intact.
- Swift helper compiled for both `arm64-apple-macos14.0` and `x86_64-apple-macos14.0`; native contract tests exercise cloud preparation and hosted delivery without local generation. Compilation does not establish runtime compatibility on a physical Intel or older-macOS machine.
- Rust includes real localhost WebSocket tests for the start contract, Unicode stream delivery and cancellation, plus provider-routing, rejection-billing, bounded-output and recovery-schedule tests. Final aggregate counts are recorded in the execution plan.
- The root release-manifest test initially failed because the local linker read an incompatible macOS 27 CommandLineTools SDK. Selecting the matching Xcode SDK through command-local `SDKROOT` made it pass; no system settings or unrelated source files were changed.

Cloud inference and hosted speech have separate charges; if narration cannot start, the answer remains available. Model pricing/retention terms, paid live inference, full physical microphone-to-insertion testing on unsupported-Apple-Intelligence hardware, and live deployment are not claimed by the automated tests.

Future Shortcuts tools, hosted transcription, image uploads and Windows support remain outside this voice-fulfillment change.
