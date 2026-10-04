# Hosted voice Assistant Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the independent backend/native/desktop tasks and review their integration.

**Goal:** Route the existing voice Assistant's fulfillment to a metered OpenAI-compatible endpoint without adding typing.
**Architecture:** Rust owns authenticated hosted streaming and provider selection. Swift captures voice/context and applies both providers' answers through the same delivery code. The backend authenticates, reserves existing credits, streams bounded action/text events, and settles/reconciles durable usage.
**Tech Stack:** Swift, Rust/Tauri, React, TypeScript/Fastify/Cloudflare Workers, Neon/Postgres.
**Spec:** `docs/hosted-assistant-design.md`, with its approved voice-only amendment.

## Global constraints

- No typing UI or fallback. Existing voice recording, original target, insertion, undo, speaking and local mode remain.
- Local default; Cloud and Automatic require explicit cloud consent. OpenAI-compatible endpoint and separate configured token rates.
- One account credit wallet, bounded output, no double dispatch for duplicate IDs, cancellation and stale-event suppression.
- Same source selection/window capture, cloud OCR locally when supported; never call FoundationModels for cloud context or generation.
- Feature off without valid server configuration. No production deployment or secret changes.
- Desktop worktree `/Users/home/.codex/worktrees/hosted-assistant/kiegen` on `codex/feat-hosted-assistant-desktop`; separate backend repository `/Users/home/Desktop/kiegen/texthalo-server` on `codex/feat-hosted-assistant-backend`. Only coordinator changes branches or commits.

## Shared contract (binding for parallel tasks)

Settings: `assistant.provider: local|cloud|automatic` default local; `assistant.cloud_consent_version: number|null` default null.
Resolved `provider: local|cloud` travels in native configuration. Rust resolves automatic using native availability; lack of consent blocks cloud.

Swift emits `phase: generation_input` with local `id` and fields `request`, `preferences`, `selection`, `source` ({app,windowTitle,focusedRole,focusedLabel}), `previousOutput`, `screenText` (string, empty if none). Rust must intercept without publishing content as UI status.
Rust sends native commands `hosted_snapshot` and `hosted_complete` with `id`, `action` (`text|speak|read-selection|clarify`), `text` (cumulative), plus `hosted_error` with `id`, `text` (partial), `message`. Swift validates current pending cloud request, stable action, cancellation/terminal state and uses common delivery. Read-selection resolves to frozen native selection.

Backend:
- Authenticated GET `/v1/assistant/capabilities`: `enabled`, `modelAlias`, `rateVersion`, `maxRequestCredits`, `maxOutputTokens`; unavailable config returns enabled false.
- WS `/v1/assistant/stream`: start `{type:"start",token,requestId:<uuid>,interactionId:<uuid>,modelAlias:"assistant-standard",rateVersion,maxCredits,request,preferences,selection,source,previousOutput,screenText}`. Same flat content fields as native. Client stop `{type:"stop"}`.
- Events `{type,requestId,sequence,...}`: accepted(reservedCredits,modelAlias,rateVersion); action(action); text_delta(text); generation_done; usage(billingStatus,chargedCredits,refundedCredits,availableCredits); done; error(error/message,billingStatus). For read-selection no model body required; action then generation_done suffices.
- GET `/v1/assistant/requests/:requestId` for owned status/usage; POST same path `/cancel` idempotent.
- Dedicated provider adapter, server prompt owns four existing marker semantics. No tool calls in v1.

## Review focus

1. Cancel/replacement followed by late cloud events cannot mutate or speak (native/Rust tests).
2. Two devices or transport retries cannot duplicate inference/debit (DB/session tests).
3. Missing FoundationModels never blocks cloud fulfillment/context; voice recognition failure remains explicit (Swift tests).
4. Malformed/truncated action frames never get inserted; split Unicode/network frames decode correctly (backend/Rust tests).
5. Network/runtime death cannot leave credits reserved forever or make partial output look successful (recovery/session integration tests).

### Task 1: Backend inference and credit lifecycle

Owner: backend agent, all `texthalo-server/**` only.
- [x] Write/run failing tests for protocol, provider marker streaming, auth/config, charging/cancel/duplicate/recovery semantics.
- [x] Implement dedicated Assistant modules, additive DB migration/repository and memory test implementation; atomic admission/reservation via credit transaction support; status, cancel and recover.
- [x] Wire config/runtime and both Node/Worker stream registration, preserve TTS, document example environment and recovery deployment.
- [x] Run backend tests, build, Worker typecheck/router tests; report results and limitations. Do not commit.

### Task 2: Native capture and common delivery

Owner: native agent, `src-tauri/native/**`, native test harness scripts as necessary, `src-tauri/build.rs` only if needed.
- [x] Write/run failing native tests for cloud eligibility and late/invalid cloud response delivery.
- [x] Add provider to Request, split local check/context/generation from common delivery, emit generation_input and handle hosted_* commands.
- [x] Preserve streaming insertion, speech, target validation, undo and cancellation; local OCR cannot require FoundationModels. No typing additions.
- [x] Compile helper and run native tests; report protocol details and results. Do not commit.

### Task 3: Rust hosted routing

Owner: desktop transport agent, `src-tauri/src/**` except `config.rs` (coordinator owns settings), `Cargo.toml` only if needed.
- [x] Add failing tests for hosted event decoder, cumulative snapshots, fixed action, terminal sequence and provider routing.
- [x] Add `hosted_assistant.rs`, reuse secure auth/refresh in hosted.rs, intercept generation_input, spawn network task without holding mutex, send native snapshots/completion.
- [x] Cancel on replacement, escape/stop, signout, helper failure and shutdown; publish provider and late billing metadata without replaying output. Recover usage via owned request status after disconnect.
- [x] Run targeted Rust tests/checks; report results. Do not commit.

### Task 4: Settings and UI parity

Owner: coordinator, `config.rs`, `src/**`, root UI tests and docs.
- [x] Add defaulted provider/consent settings and relevant migration/routing tests.
- [x] Add provider selection and cloud disclosure; readiness uses selected provider and auth, but voice permissions remain required. Do not add typing.
- [x] Show provider/credits/pending settlement in existing overlay and history, preserve local default and account UX.
- [x] Run frontend tests/typecheck/build and native/Rust/backend cross-contract review.

### Task 5: Integration, review and branches

- [x] Review each task's changes and run mock-provider end-to-end integration including streamed answer and existing-wallet debit, cancellation/recovery and local regression.
- [x] Commit backend on `codex/feat-hosted-assistant-backend` in the server repository; commit desktop/docs on `codex/feat-hosted-assistant-desktop` in the app repository and update its existing backend gitlink to the tested server commit.
- [x] Fresh branch review, fix actionable defects, rerun appropriate checks.
- [x] Record exact tested coverage and external activation requirements. No claim of paid-live or older-hardware validation without evidence.

## Execution decisions

- Repository discovery: server is an embedded independent Git repository recorded as a gitlink without .gitmodules, so branches are separate repositories, not stacked desktop branches. Preserve that layout.
- User confirmed existing server external-provider wiring: reuse existing OpenAI-compatible transport and credential configuration, including TEXT_ENHANCER fallback values, with a distinct Assistant feature switch.
- User approval explicitly requests end-to-end execution; proceed continuously rather than introduce another plan approval round.
- Baseline root tests: 25 pass, one unrelated updater archive test fails because /usr/bin/clang reads the newer CommandLineTools SDK using an incompatible linker. Native contracts passed. Diagnose using a matching SDK before attributing this to feature changes.

## Final verification (2026-10-03)

- Server full Vitest suite: 32 files, 229 tests passed. Includes localhost mock-provider streaming through authenticated WebSocket and the SQL credit ledger.
- Server Node build, Worker typecheck, Worker router test and bundle build passed.
- Desktop root tests: 35 passed with the matching Xcode SDK selected through command-local SDKROOT.
- Desktop production build/typecheck passed. Browser end-to-end suite: 45 passed; both Cloud cases rerun after usage-label changes passed.
- Final Rust library suite: 210 passed, 12 ignored, 0 failed. Ignored tests require installed model weights or real playback and were not claimed as validated.
- Swift helper compiled for ARM64 and x86_64 targeting macOS 14. Physical older-Mac voice-to-insertion and paid-provider/deployed-service validation remain external qualification work.
- Cross-component reviews fixed recovery timing, empty read-selection charging, output-limit alignment, duplicate response handling, definitive rejection reconciliation, unknown Automatic availability and late billing metadata overwrites. Regression tests cover those fixes.
- The server uses a conservative fixed configurable request hold rather than a tokenizer estimate. Both repositories retain separate feature branches; desktop gitlink references the tested server revision.
- Test-first workflow was used for review regressions; the initial Rust decoder implementation and tests were developed together rather than strictly red/green.
- Activation requires migration 011 and the documented server feature flag. No deployment, real secret changes or paid inference performed.
