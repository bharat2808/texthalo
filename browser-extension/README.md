# TextHalo Reader

A relay for TextHalo's desktop audiobook library. Currently supports Kindle web on `read.amazon.ca` and `read.amazon.com`. The popup shows website support, book title, page position and app connection, with two actions: **Listen from this page** and **Build complete audiobook**. TextHalo performs generation and playback locally.

## Local setup

1. Install the native helper: `node browser-extension/native/setup.mjs --install`.
2. Load this folder as an unpacked extension in `chrome://extensions`.
3. Reload an open Kindle book. Detection does not connect automatically.
4. Open the extension popup and choose **Connect to TextHalo** for that tab; Chrome requires this gesture for screenshot access.
5. Choose **Listen from this page** or **Build complete audiobook** in the extension. Playback and chapter controls remain in the app overlay while the extension is connected. Only chapters recorded completely from beginning to end offer Continue in the **Audiobooks** library. A mid-chapter page session can play now without searching backward; its initial fragment is not resumable later. Choose **Disconnect** to end the reader connection.

The app’s **Audiobooks** section can save a bundled extension ZIP and open these installation instructions. Once published, set `VITE_READER_EXTENSION_URL` to the verified Store listing when building the app to switch the download button to Store installation.

After updating source files, reload the installed extension and the reader tab. The popup reports a timeout or outdated worker rather than remaining on “Checking”. The native helper uses the stable extension ID in `extension-id.txt`; helper origin and Chrome registration must stay aligned.

The app owns one growing audio file, sentence commits, chapter checkpoints, local Kokoro narration and saved listening time/speed. Generation runs in Rust and does not depend on the library webview staying open. See `docs/BROWSER_READING.md` for persistence, reader requirements and validation limits.

Capture requires the book tab to be selected in its own browser window. Its window may be in the background. Unsupported websites are not captured. App requests expire and are bound to the selected book and unchanged page. OCR and narration stay local. The helper does not save diagnostic images/text by default.

## Checks and packaging

- `node --test scripts/reader-relay.test.mjs`
- `cargo test --lib` in `src-tauri` with the matching Xcode SDK.
- `fixtures/reader.html` and `fixtures/check-reader.js` are isolated adapter regression fixtures.
- `node browser-extension/package.mjs` builds an allowlisted ZIP excluding native binaries, fixtures, captures and book data.

The extension has not been published. Signed app/helper distribution, Store review and broader full-book verification remain before release; see `STORE.md`.
