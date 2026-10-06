# Expanded TextHalo interface localization

Implemented on `codex/desktop-localization` for desktop and website.

## Coverage

Adds Brazilian Portuguese (`pt-BR`), Italian (`it`), Japanese (`ja`), Simplified Chinese (`zh-Hans`), and Traditional Chinese (`zh-Hant`). Existing English, Canadian French, and Latin American Spanish remain available. Hindi is outside interface-localization scope; this change does not remove any speech voices.

Each new locale contains all 761 desktop source keys and all website source keys (441 before the third-party notices page). The native tray uses the same desktop catalogs. macOS microphone and speech-recognition permission descriptions are included in release and development bundles, with matching `CFBundleLocalizations` declarations.

Desktop and website share language identifiers and picker names. Desktop follows the Mac's preferred language unless a manual choice is saved. Website follows browser preferences unless a localized route or saved manual choice overrides them. Unsupported preferences retain the existing English fallback. Interface changes leave speech, recognition, user content, and backend plan values independent.

Chinese matching checks explicit scripts first. `zh-Hans-TW` stays Simplified, and `zh-Hant-CN` stays Traditional. Without a script, Taiwan, Hong Kong, and Macao resolve to Traditional; other Chinese locales resolve to Simplified. Portuguese regional variants resolve to the Brazilian catalog.

Website pre-rendering generates all new localized routes, canonical/alternate links, structured-data language tags, Open Graph locales, localized 404 pages, and sitemap entries. Connect query strings and fragments remain intact when switching languages.

## Verification

- Desktop and website production builds pass.
- JavaScript suite: 67 tests passed before the final product-name/spacing regression test was added. Final focused localization suite: 10 tests pass.
- Native localization tests: 5 pass, including persistence serialization, all new manual choices, regional/script matching, and actual native catalog selection.
- Desktop localization browser tests: 12 pass, including each new locale, persisted selection, speech settings independence, and live overlay updates.
- Website localization browser tests: 14 pass, including each new browser locale, manual selection, narrow layouts, static editorial/SEO output, live backend plan values, and storage-disabled fallback.
- Native Clippy with warnings denied, Rust formatting, plist validation, and Git whitespace checks pass.
- All new catalogs match existing source-key coverage and preserve interpolation parameters. Desktop product-name and intentional fragment-spacing checks pass. Primary desktop Home screenshots were generated at 780 × 640; Portuguese and Japanese layouts were visually inspected.

The initial desktop browser run overlapped full JavaScript/Vite checks and hit two startup timeouts. Its new overlay tests also referenced `Preparing voice…` instead of the actual source key `Preparing speech…`; the fixture was corrected. Subsequent complete desktop localization runs passed.

## Translation review and remaining limits

Catalogs began as machine-assisted drafts of static authored interface strings. Reviewed terminology and copy include primary navigation/actions, Home and website hero copy, language settings, brand names, voice setup/download guidance, key native recovery messages, and Chinese billing terminology. Interpolation values, user text, diagnostic details, and unknown backend-authored strings retain existing behavior.

Automated coverage and layout checks do not establish native-speaker editorial quality for every help paragraph, blog post, or privacy-policy translation. A full native-speaker review has not been performed.

Release 0.1.16 includes this work. Publication and deployment are recorded by the release receipts.
