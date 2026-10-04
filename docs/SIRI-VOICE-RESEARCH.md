# Siri voice access and assistant capabilities

Research date: 2026-10-03. This is an investigation, not an implemented TextHalo integration.

## Result on this Mac

Built and ran `maximilianromer/siri-tts-cli` at commit
`092cb7944e095b89ce6ad3d76277e261791dcef1` in a temporary checkout.
Host: macOS 27.0.1 (26A434), arm64, Apple Swift 6.3.3.
Release build succeeded. Voice enumeration returned 13 installed entries.
Two exact voice asset identities successfully synthesized the same test sentence:

| Voice asset key | Output duration | First audio | Total synthesis | Word timings |
| --- | --- | --- | --- | --- |
| `en-US:natural:male:Aaron:premium:5030` | 3.762 s | 1.298 s | 1.854 s | Returned |
| `en-US:fmvoice:female:nora:premium:0` | 3.722 s | 0.169 s | 0.294 s | Unavailable |

These are single sequential observations, not benchmarks or guaranteed cold-start times.
Both commands used `--engine siri --no-prewarm --timeout 20 --json` and an output WAV path;
there was no automatic AV fallback. Python independently parsed both WAV files and
confirmed 48 kHz, mono, 16-bit PCM with nonzero samples. This proves non-silent
synthesis through this helper, not subjective voice quality or exact equivalence
to the current Siri UI voice. The remaining 11 entries were not synthesized.
No voice installation, removal, system setting change, or TextHalo code change was performed.

Evidence and audio are saved locally at:
`/Users/home/.codex/visualizations/2026/10/03/01a102f8-5d43-7270-b453-2284f11b384e/siri-research/`.
The JSON output paths retain their original temporary generation locations.

## End-to-end upstream implementation

1. `Package.swift` imports a reconstructed Swift interface and weak-links Apple's
   private `SiriTTSService` framework. This is not a public Apple SDK contract.
2. `SiriEngine.connect()` checks framework loading with `dlopen`, lazily creates
   `DaemonSession`, and calls `invokeDaemon()`.
3. `downloadedVoices(matching: nil)` queries the resident service. The local
   TextToSpeech/MobileAsset catalog helps filter stale entries whose assets have
   disappeared. A catalog listing and usable installed inventory are distinct.
4. Voice selection uses language and an asset key or name. TextHalo should use
   exact keys: this Mac has both `Aaron` and `aaron`, with different technologies.
5. `SynthesisRequest(text:voice:)` carries text and rate/pitch/volume. Optional
   prewarming and word-timing capability checks precede `session.synthesize`.
6. Audio callbacks include actual format metadata. `SiriAudioNormalizer` handles
   LPCM or Opus, including decoder state across callbacks, and produces consistent
   PCM. Do not assume the daemon itself always returns 48 kHz Int16 PCM.
7. Request tokens, serialized callbacks, timeouts, and cancellation prevent stale
   chunks from reaching a later request. Completed audio is validated before success.
8. Long input is divided at natural boundaries with a 4,000-byte target. The CLI
   spools output; streaming can emit PCM as it arrives. Word timing offsets use UTF-16.
9. The optional HTTP server and Swift client wrap this same engine; they are not
   necessary to access the local service from a separate helper process.

The project also contains private subscription/download and purge paths. They
were inspected but not exercised. An acknowledged installation request is not
installation completion; both local assets and daemon inventory must agree.

## Proposed TextHalo integration boundaries

Existing Apple speech uses `/usr/bin/say` in `src-tauri/src/speech.rs`.
`config.rs` stores that engine's voice as a name, unsuitable as a unique Siri identity.
`spoken.rs::stream_pcm_input` currently accepts only Kokoro and Chatterbox.
`assistant.rs` already supervises a Swift helper using stdin/stdout messages.

A separate Siri speech helper would isolate private-interface loading failures
from the app and its assistant. The existing helper transport pattern is reusable;
audio needs a separate pipe or framed binary messages, not unframed PCM mixed into JSON.
Route normalized PCM through TextHalo's playback, Stop, and history lifecycle.

Suggested capability states: unsupported, service unavailable, no installed voices,
listed but unverified, verified usable, and failed synthesis. OS version alone is
insufficient. Check helper launch, service connection, installed identity, and a
bounded live synthesis probe. Refresh after voice changes or relevant failures.
Record the actual engine, asset key, format, and error. Make fallback visible and
do not switch engines midway through already-playing audio.

Before integration is considered ready, verify cancellation during synthesis,
long input, repeated requests, helper crash/restart, absent assets, unsupported
systems, and the packaged signed application. Current evidence covers only a
locally built CLI and two successful file renders. Private interfaces can change
between macOS versions.

## Assistant capabilities are a separate layer

`native/AssistantGeneration.swift` creates `LanguageModelSession(instructions:)`
without tools. Its output protocol accepts only text, speak, read-selection, or
clarify. `Contracts.swift` explicitly says the model cannot click controls,
navigate apps, send messages, or execute commands. The host already supplies
selection/app/screenshot context and handles insertion, copying, undo, and narration.
Those host actions do not constitute a general model-driven tool executor.

Apple's public Foundation Models framework supports `Tool` and a session `tools:`
parameter; both were also verified in the locally installed SDK interface.
We can expose specific application operations to the model, such as listing voices,
choosing an exact voice, retrieving playback status, or stopping playback. A named
Shortcuts runner or app-specific integrations could provide broader actions later.
These require implemented tools and permission-aware execution; gaining access to
the Siri TTS service does not grant access to Siri's assistant or its app actions.

For tool execution, validate arguments and scope, return actual operation results,
serialize conflicting mutations, and preserve cancellation/request identity.
Treat selected text and screenshots as untrusted context, as the existing prompt does.
Report an action as completed only after its executor confirms completion.

## Sources

- [Pinned upstream source](https://github.com/maximilianromer/siri-tts-cli/tree/092cb7944e095b89ce6ad3d76277e261791dcef1)
- [SiriEngine](https://github.com/maximilianromer/siri-tts-cli/blob/092cb7944e095b89ce6ad3d76277e261791dcef1/Sources/SiriTTSCore/SiriEngine.swift)
- [Audio normalization](https://github.com/maximilianromer/siri-tts-cli/blob/092cb7944e095b89ce6ad3d76277e261791dcef1/Sources/SiriTTSCore/SiriAudioNormalizer.swift)
- [Apple: Expanding generation with tool calling](https://developer.apple.com/documentation/foundationmodels/expanding-generation-with-tool-calling)
- [Apple: Tool protocol](https://developer.apple.com/documentation/foundationmodels/tool)
