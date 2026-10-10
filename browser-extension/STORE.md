# Chrome Web Store preparation

This is TextHalo's macOS browser relay, version 0.5.0. It has not been submitted or published. Narration, page buffering, audio files and listening bookmarks belong to the TextHalo desktop app. The extension provides two connected-book actions: Listen from this page and Build complete audiobook; generation, audio storage and the saved library remain in the desktop app.

## Draft listing

Name: TextHalo Reader

Single purpose: Connect the user's selected Kindle web book to the TextHalo desktop app by identifying reading content and relaying verified page capture and navigation requests.

Description: Connect a supported Kindle web reader tab to TextHalo. After the user chooses Connect to TextHalo, the extension identifies the displayed book, reading position and page-image bounds, captures the selected tab when requested, and verifies requested page turns. Choose Listen from this page or Build complete audiobook in the extension popup. Use the connected desktop overlay for playback controls. TextHalo performs local OCR and manages narration, audio files and saved listening progress. Requires macOS, the updated TextHalo app and its separately installed native relay helper. Supported hosts: read.amazon.ca and read.amazon.com. This is an experimental Kindle adapter; OCR and reader-layout compatibility are still being tested.

No affiliation with or endorsement by Amazon. Kindle is a trademark of its respective owner.

## Privacy disclosure draft

The extension processes the selected supported reader's URL, book identity/title, reading position, page image bounds and visible page screenshots. It communicates with an extension-ID-allowlisted native helper on the same Mac. The helper performs local OCR using TextHalo's shared code and relays reader information and command results to the desktop app through a private authenticated loopback connection. The authentication token stays in a user-private local file and is not exposed to websites or the extension's content script.

The desktop app saves book text, sentence source positions, a single growing audio file and playback bookmarks under `~/Library/Application Support/kiegen/audiobooks/`. The helper does not retain screenshot or OCR diagnostics by default. Removing the extension does not remove desktop-app files. No reading content is sent to TextHalo servers or a remote speech service in the current local Kokoro pipeline. Kindle continues its normal network activity independently.

There is no telemetry, advertising or remote executable code. Before submission, publish an accurate extension-specific privacy policy at a public URL and supply that URL in the dashboard.

Permission explanations:
- `activeTab`: capture the user-selected Kindle tab after invoking the extension.
- `nativeMessaging`: communicate with TextHalo's ID-allowlisted local reader helper.
- `https://read.amazon.ca/*` and `https://read.amazon.com/*`: inspect the selected reader and operate verified page controls. No other reader hosts are currently supported.

Reviewers need the updated macOS app and helper. Supply reproducible test instructions and authorized test material; do not include personal Kindle captures, account data or native binaries in the extension ZIP.

## Package and publication follow-up

Run `node browser-extension/package.mjs`. The allowlisted ZIP in `output/store/` contains only extension relay/runtime files and icons. The helper and app are distributed separately.

Before submitting: validate the actual Store-assigned extension ID and update the helper origin check, native host allowed origin and setup ID together if needed. Finish signed app/helper installation, listing screenshots, public privacy/support URLs and broader live testing. Submit the finished extension for review when publication is requested. Add the actual Store URL to TextHalo after an item exists; do not create a placeholder installation link.

Official references (checked October 9, 2026):
- https://developer.chrome.com/docs/webstore/publish
- https://developer.chrome.com/docs/webstore/cws-dashboard-privacy
- https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging


## Readiness audit — October 10, 2026

The MV3 package has narrow Kindle host access, bundled executable code, icons and an allowlisted archive. Native messaging is supported by Chrome; requiring a Mac desktop companion is not itself a publishing blocker. Store acceptance remains subject to review.

Completed October 10, 2026:
- The Mac app now registers the native host itself on every launch (`src-tauri/src/reader_host.rs`): it installs the `reader-host` binary (compiled by `build.rs`, bundled via `externalBin`) to `~/Library/Application Support/TextHalo Reader/` and writes the `com.texthalo.reader.json` manifests for Chrome, Edge, Brave and Chromium. No Xcode or manual setup needed on user machines.
- The extension detects a missing app install: the popup probes the native host once per session (`bridge-status`); when the host is absent it shows a download banner with a direct link to the latest Mac DMG (`https://storage.texthalo.app/TextHalo-macOS-aarch64.dmg`). No new extension permissions were required.
- The website privacy policy (`https://texthalo.app/privacy/`) now has a “Browser extension” section covering the reader integration; effective date October 10, 2026. Supply this URL as the store listing's privacy policy.

Still required before submission:
- Create a Store draft, verify its assigned ID against `extension-id.txt`, manifest key and helper origin/registration, and test the Store-installed extension with the distributed app (fresh Mac without the app: banner appears; install DMG, launch app once, reopen popup: banner clears).
- Prepare listing screenshots, support details, and reviewer instructions/test content. Verify capture, page turns, connection loss, app restarts and audiobook playback with a real supported Kindle book.
- Configure `VITE_READER_EXTENSION_URL` only after a real listing exists. Until then the app downloads its bundled ZIP for manual unpacked installation and links to setup instructions.

No Store submission or public publication has been performed. The development ZIP is suitable for local testing, not a claim that review requirements are complete.
