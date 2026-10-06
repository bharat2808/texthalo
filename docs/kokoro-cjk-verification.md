# Native Japanese and Mandarin Kokoro support

Implemented on `codex/desktop-localization`. TextHalo now routes Kokoro's five Japanese and eight Mandarin voices through native pronunciation front ends. Japanese uses JPreprocess 0.15.0 with an embedded NAIST dictionary and Misaki's kana phone mappings. Mandarin uses embedded pinyin-pro phrase readings, Jieba word boundaries, and Misaki's tone/IPA mappings. These are native adaptations, not the upstream Python runtime; dictionary segmentation may differ from Python Misaki.

Both paths work without Python, eSpeak NG, or runtime dictionary installation. Japanese handles kanji, kana, full-width text and number readings. Mandarin handles simplified/traditional text, phrase-dependent readings, cardinal numbers, decimals and years. Mixed English uses TextHalo's existing English pronunciation path. Unknown unsupported readings return an error rather than silently dropping characters. This adds speech support; interface language choices remain English, French and Spanish.

New model downloads include all 13 Japanese/Mandarin voice tables automatically. Existing installs show a compact download action for missing tables. The installer reuses the existing graph and verified files. Japanese/Mandarin previews use native sample sentences. The two language converters and embedded dictionaries increase the native application size; the Japanese dictionary artifacts are approximately 76 MiB before app compression.

The phone mapping generator pins Misaki revision `fba1236595f2d2bf21d414ba6e57d25256afada3` and verifies SHA-256 source digests. Dependency and dictionary licenses/attributions are retained under `src-tauri/licenses` and configured for inclusion in the macOS app's `Contents/Resources/licenses` directory. The license check reports no new copyleft blockers; its five existing MPL dependencies remain unchanged.

Verification:

- Rust library suite: 246 passed, 13 optional tests ignored. Coverage includes kana/tone mappings, embedded dictionary conversion, mixed English, unsupported input, and repairing an older English installation with exactly 13 missing style tables.
- Desktop JavaScript suite: 66 passed.
- Kokoro setup browser scenarios: 4 passed, including downloading/selecting Japanese and Mandarin voices without requesting eSpeak.
- Desktop and website production builds passed.
- Rust Clippy with warnings denied passed; formatting and diff checks passed.
- Real FP32 ONNX generation through `Spoken::render`: Japanese (7.2 s), Mandarin (7.9 s), traditional Mandarin (5.4 s), mixed Japanese/English (2.8 s), mixed Mandarin/English (3.0 s). Each WAV contains non-silent audio with no dropped model phoneme symbols. Samples were written under `/tmp/texthalo-cjk-audio`.

Reproduce actual synthesis with `cargo run --manifest-path src-tauri/Cargo.toml --example kokoro_cjk_verify -- OUTPUT_DIR`. `--download` repairs the installed FP32 model plan first. The native checks on this Mac require command-local `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk` because its default linker cannot read the installed macOS 27 SDK. No system SDK settings were changed.

Audio generation checks do not establish native-speaker pronunciation quality. The application was not released or deployed as part of this change.
