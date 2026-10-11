# Website third-party notices

The website’s `/licenses/` page lists third-party licenses and attribution only. It does not publish or change TextHalo’s own proprietary terms. Page navigation and explanatory copy support all eight interface languages; original license texts are preserved.

## Inventory and regeneration

`website/src/third-party-licenses.json` and the downloadable `website/public/licenses/dependencies.json` snapshot 842 unique locked Rust/npm package versions for release 0.1.19. The generator excludes the application’s own packages, records each dependency’s declared license, and copies available LICENSE, COPYING, COPYRIGHT, and NOTICE files without alteration. Upstream links cover additional details and packages whose distribution does not include a notice file.

After installing the locked dependencies, run `npm run licenses:generate` from the repository root and review the generated changes. Cargo metadata identifies normal macOS dependencies; other resolved Rust packages are listed under build tools and other locked packages. Vite’s emitted module metadata distinguishes actual shipped npm modules from build tools and unused optional packages. npm entries cover every package in package-lock.json, including platform-specific packages.

Three Triplit optional peers lack license metadata in their npm packages. Their AGPL notices come from the upstream repository root at each published package’s npm gitHead. Production desktop/website bundles do not emit modules from these peers. They remain visible in the other-locked-packages inventory; no AGPL runtime module is shipped by this change.

## Additional assets

The page separately identifies bundled ONNX Runtime, native pronunciation adaptations and dictionaries, optional Kokoro/Chatterbox/Whisper model downloads, separately installed eSpeak NG/pcaudiolib, and the Google Fonts used by the website.

CJK notices are copied from `src-tauri/licenses/`, including upstream revisions and adaptation notices. Model links use the same pinned Hugging Face revisions as `src-tauri/src/download.rs` and `whisper_models.rs`. Kokoro’s pinned model card declares Apache-2.0; its linked license text is the official Apache license. MIT model notices come from OpenAI Whisper and Resemble AI Chatterbox. eSpeak NG and pcaudiolib COPYING texts are from their configured 1.52.0/1.3 tags. ONNX Runtime’s LICENSE and ThirdPartyNotices come from Microsoft’s v1.28.0 tag, matching ort-sys’s pinned runtime distribution. Website font notices come from the Google Fonts `ofl/dmsans`, `ofl/dmmono`, and `ofl/manrope` directories. These additional notices were checked on October 6, 2026.

## Verification

The inventory test verifies every locked npm version is listed, notice files exist and contain text, the public/downloaded inventories agree, TextHalo’s own packages are excluded, and unused Triplit peers remain outside the runtime group. Website localization tests verify all authored page labels and translated server-rendered routes. Browser scenarios verify footer discovery, translated navigation, narrow-screen layout, inventory/notice HTTP responses, and accessibility without JavaScript.

The dependency audit reports bincode 2.0.1 as unmaintained (RUSTSEC-2025-0141), with no patched version. The existing advisory policy now includes a documented exception for JPreprocess/Lindera’s bundled dictionary decoding; TextHalo does not deserialize user-supplied bincode dictionaries. Vulnerability checks remain enabled, and an upstream serializer replacement remains a maintenance item.
