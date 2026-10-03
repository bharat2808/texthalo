# Streamed assistant implementation plan

Approved design: stream generation into a live preview and one request-owned output session. Hosted cue classification waits for five complete sentences (or EOF, character cap, bounded timeout), with rolling preceding/following context separate from synthesis text. Local voices start from the first complete sentence. Preserve cancellation, original-field ownership, history, and existing buffers. No release or deployment.

## Constraints and interfaces
- Work in the existing development checkout so the user can test it; preserve existing work and nested server repository.
- Native emits `phase: streaming`, `action: preview|speak|clarify`, `text`: cumulative answer. `result/speak` is the final snapshot and end-of-input. Clarify never executes audio. Partial transcription is `recording` with cumulative `text`.
- Rust owns one speech job and bounded text input per assistant request; cumulative snapshots must extend committed text. EOF explicit; failure/cancel abort all output. Apple voice may retain final-only output.
- Hosted v2: start includes `inputMode: incremental`, empty text; after ready send ordered `append_text {sequence,text}` and `finish_input {sequence}`. Existing full-text clients unchanged. Server retains 20k total character bound and current per-chunk accounting/playback acknowledgements. Five-sentence context window bounded to 3000 chars, 2 second maximum wait from pending text. Empty EOF rejects.
- No production deploy; local server integration verifies protocol with provider boundary fixtures. Real provider/model tests reported separately.

## Tasks (each RED -> GREEN -> regression)
- [x] Server: failing semantic rolling-buffer tests; incremental input protocol tests; implement buffer and SpeechSession async chunk source; socket E2E checks ordering, EOF, cancellation, billing and context-only evidence.
- [x] Native/UI: failing snapshot/preview and cancellation tests; stream Foundation Models snapshots with existing routing; partial transcript preview; overlap source OCR where safe; guarded incremental insertion only for verified editable fields; final paste fallback. UI shows capped preview without focus theft.
- [x] Rust audio: failing input-stream/EOF/cancel tests; extend local buffered speech to accept open text input in one PCM player; bridge native snapshots to a speech lease; hosted v2 client concurrent text upload and PCM receive; keep single history item and pinned settings.
- [x] Integrate and verify: native tests and actual local-model smoke, Rust tests/check, server full suite + E2E + build, desktop scripts/typecheck/Playwright. Review changes and fix regressions. Start/reload development only.

## Review focus
- Short answer/no punctuation must flush; incomplete source is not EOF.
- Duplicate snapshots/sequence IDs must not duplicate speech or billing.
- Cancellation while waiting for more text or playback must unblock every worker.
- User editing/focus changes must never redirect or overwrite output.
- Hosted context is never synthesized twice; server errors never silently restart paid output.

## Execution record
- Initial checkout clean except nested texthalo-server repository. Existing app branch codex/local-assistant-modes.
- User explicitly requested implementation and E2E using TDD; proceed without another approval gate. No automatic commits or releases.

## Verified result
- Server: 212 tests across 28 files including real local WebSocket end-to-end sessions; provider boundaries use fixtures (no live paid provider calls). Server TypeScript build and worker typecheck pass.
- Rust: 198 tests pass, 16 opt-in/environment-dependent tests ignored by the default suite. Real opt-in Kokoro progressive PCM test additionally passed, including a quoted opening, exactly one player/start and one recording. Last run: 3.4 seconds audio, 0.051 seconds underrun under concurrent test load.
- Desktop: all 40 Playwright tests pass; TypeScript check and frontend build pass. Native contracts/protocol tests pass. Actual local Foundation Models streaming/audio routing/transcription smoke passed on a rerun; an earlier run hit the existing screen-description semantic assertion, so model wording remains nondeterministic.
- Default npm test: 25/26 pass; the unrelated real-Mach-O updater fixture fails because /usr/bin/clang cannot read the installed macOS27 SDK. Its entire six-test suite passes when run alone with SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk. Do not globally use that older SDK for Foundation Models Swift tests.
- Independent scoped reviews of server, native/UI, audio and integration passed after regression fixes. Fixes cover whitespace billing, split decimals/quotes, pending-error cancellation, long-reading compatibility, partial recovery, stale playback leases and coalesced EOF.
- Development app restarted with npm run tauri dev and native helper running. No production deployment or release build; existing development signing preserves permission identity.

## Compatibility decisions
- Existing SFSpeechRecognizer partial results are displayed but only finalized transcription is inserted; no unstable dictation text is committed. Apple say voice continues to wait for the full answer. Local PCM engines and hosted v2 accept progressive text.
- Unsupported/unobservable insertion fields keep the final-paste fallback; any dispatched incremental edit disables duplicate final paste. Selection replacement remains final-only.
- Hosted cue buffering defaults to five complete sentences of rolling context, bounded by EOF, 2 seconds or 3000 characters. Surrounding context is never spoken twice.
- All edits remain uncommitted for review. texthalo-server is a separate nested repository and its changes must be retained separately from the desktop diff.
