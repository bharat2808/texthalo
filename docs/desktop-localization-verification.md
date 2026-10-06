# Desktop localization verification

Branch: `codex/desktop-localization`. Scope: desktop UI, native tray, and packaged macOS privacy descriptions. The website is outside this change.

## Behavior

English, Canadian French, and Latin American Spanish are available in Settings → General → Interface language. System mode resolves the Mac’s ordered preferred languages, with English fallback. Manual choices persist and update open windows and the tray. Speech/dictation languages, user transcripts, instructions, and custom voice names remain independent.

Native status and audit values retain their source text; translation occurs when displayed. Dates and numbers follow the selected interface language. Both catalogs share source keys and preserve interpolation parameters. Existing settings without a language preference migrate to System.

## Verification

- `npm test`: 64 passed, zero failed.
- `npm run build`: passed (TypeScript and Vite).
- `cargo test --manifest-path src-tauri/Cargo.toml`: 248 passed; 19 ignored tests require optional models/hardware/network fixtures.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`, `git diff --check`, and `plutil -lint` on the two plists and three localized privacy files: passed.
- Localization browser scenarios: preferred-language startup, persistent override, onboarding, live speech/Assistant/updater windows, custom voice names, and audit-history labels. 7 passed.
- Existing Home/permissions and Assistant history browser checks: 2 passed in isolation.
- Unsigned debug `.app` build: passed; all three privacy-resource files and `CFBundleLocalizations` were verified in `src-tauri/target/debug/bundle/macos/TextHalo.app`. Distribution signing/notarization and production deployment were not performed.

## Existing browser-suite failures

The full run before the final audit-label adjustment reported 31 passed and 22 failed. A clean temporary snapshot containing the repository HEAD plus the user’s original uncommitted frontend changes reported 26 passed and 21 failed. All 21 baseline failures also occurred in the feature run. The additional Home startup timeout had an unfinished `/src/main.tsx?e2e` network request and an empty page at the five-second assertion deadline; it passed when run in isolation. A subsequent browser run during concurrent Rust builds also timed out in two cold-start localization checks before the frontend rendered. The localization fixture now waits up to 15 seconds for Vite’s initial module graph to mount before making behavior assertions. The browser suite is therefore not globally green.

Baseline failures reproduced in both snapshots:

- `assistant-settings.spec.ts`: cloud Assistant preserves the voice flow and saves explicit cloud consent.
- `assistant-settings.spec.ts`: hold keys can be assigned independently and duplicate holds are rejected.
- `assistant-settings.spec.ts`: shortcut recording distinguishes a quick modifier tap, a hold, and a combination.
- `assistant-settings.spec.ts`: transcription history is separate and language changes persist.
- `assistant-settings.spec.ts`: assistant instructions persist with automatic private context and no chat launcher.
- `assistant-settings.spec.ts`: Dictate changes and cancels shortcuts without leaving the feature.
- `assistant-settings.spec.ts`: Assistant changes and cancels shortcuts without leaving the feature.
- `assistant-settings.spec.ts`: inline shortcut rejects a duplicate and lets the user retry.
- `assistant-settings.spec.ts`: failed shortcut save stays on the feature with an error.
- `assistant-settings.spec.ts`: optional recording shortcuts can be cleared directly.
- `assistant-settings.spec.ts`: section readiness explains missing setup and updates when ready.
- `assistant-settings.spec.ts`: section readiness only requires permissions needed by each feature.
- `assistant-settings.spec.ts`: Assistant requires screen permission for native image context and explains privacy.
- `assistant-settings.spec.ts`: Assistant remains usable when this build has no native image input.
- `assistant.spec.ts`: dictation and generation share the compact activity overlay.
- `assistant.spec.ts`: fallback results use the TextHalo dark palette.
- `assistant.spec.ts`: fallback header drags the window without stealing button interactions.
- `assistant.spec.ts`: recording shows a capped live preview and request-owned cancellation.
- `assistant.spec.ts`: streaming shows a capped live preview and request-owned cancellation.
- `assistant.spec.ts`: speaking shows a capped live preview and request-owned cancellation.
- `assistant.spec.ts`: processing shows a capped live preview and request-owned cancellation.

## Practical limits

Browser scenarios use mocked native IPC. Rust tests validate preference migration, resolution and tray translation; bundle inspection validates packaged localization resources. System-owned permission dialog chrome follows macOS rather than the in-app override. Live permission prompts and every translation have not received hands-on native-speaker acceptance testing.

The feature is local and uncommitted; pre-existing user edits are preserved. No branch was merged or pushed.
