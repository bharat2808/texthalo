# Browser audiobooks

TextHalo owns the library, narration, buffering, playback and bookmarks. The browser extension is a relay: its popup reports support, platform, book title, page position and connection status, and offers Listen from this page and Build complete audiobook. Currently supported: Kindle web on `read.amazon.ca` and `read.amazon.com`.

## Reader discovery and capture

`browser-extension/kindle.js` reports changed reader presence. The background worker connects a ready, selected book only when the user chooses **Connect to TextHalo**. It then refreshes the app's reader metadata while polling for commands. Switching tabs or closing the book expires/disconnects its suggestion. The adapter verifies book identity, layout, visible page surfaces and page changes before acknowledging navigation; it refuses stale or covered pages.

Chrome's `activeTab` permission requires an extension gesture before screenshot capture. Open the popup for the tab and choose **Connect to TextHalo**. Automatic metadata detection works without that grant; narration cannot start capturing until it is granted. No broad `<all_urls>` access is requested. The selected reader tab can remain in a background browser window; an inactive tab within that window cannot be captured.

The native helper runs local Apple Vision OCR using `src-tauri/native/ScreenText.swift`. It forwards app messages to a random-port loopback listener authenticated with a user-private token. Reader commands are bound to the connected tab/book, expire, and are removed on timeout. Page scripts cannot submit app commands. The installed launcher no longer saves screenshot/OCR diagnostics by default.

## App UX

`src/Audiobooks.tsx` lists saved books, library playback controls and checkpoints, and links directly to the official TextHalo Reader Chrome Web Store listing. `src/SpeechOverlay.tsx` retains its playback, preparation and checkpoint controls while the extension is connected. The audiobook overlay hides when the extension is disconnected. Dismiss hides the notice for that book for the app session. Audiobook playback without a reader connection does not open the floating popup. Normal selected-text speech keeps its existing overlay controls.

The extension offers exactly two audiobook actions: Listen from this page and Build complete audiobook. Playback and chapter controls remain in the connected app overlay and saved library. Native control requests use a separate helper process so they cannot block the persistent relay's capture/navigation pump. The app accepts actions only for the current relay and connected book, validates playback targets, and reports asynchronous action errors on the next control-state poll.

Chapter checkpoints are recognized from captured chapter/part/prologue/epilogue headings, not from an inferred table of contents; checkpoints stay disabled for saved playback until both chapter boundaries are verified and all their audio commits. The first captured page is also a checkpoint.

## Durable recording

`src-tauri/src/audiobooks.rs` owns generation independently of the webview. Each book has a private directory under `~/Library/Application Support/kiegen/audiobooks/<UUID>/` containing:

- `audio.wav` initially, or the UUID-named WAV selected by `book.json` after a new page session: the active growing 24 kHz mono PCM recording.
- `book.json`: source pages, stable sentence and chapter IDs, sentence text/source byte ranges, committed sample boundaries, narration settings and complete-book preparation.
- `bookmark.json`: stable sentence ID, sample offset within that sentence, listening time and speed. The app updates it roughly once per second and flushes it immediately on Stop or tray Quit.
- `chapters/<chapter UUID>.wav`: derived audio for each saved chapter, refreshed after a captured page finishes and when a worker exits, including cancellation or synthesis failure. Partial audio is retained.

A sentence is rendered to a transient scratch WAV, appended and synced, then published by atomically replacing the sentence commit manifest. Failed/interrupted appends roll back to the manifest's committed prefix. Only committed sentences become playable. Startup checks contiguous sentence/sample boundaries, repairs the WAV header and discards an uncommitted suffix. The scratch file is removed after synthesis and on recovery.

Playback maps the committed PCM file instead of copying a multi-hour recording into RAM. Each sentence commit extends the player's accessible mapped prefix. The same native player supplies pause, resume, seek and speed controls. Bookmarks are updated only for the audiobook's own speech job; another reading cannot overwrite its progress.

Local Kokoro is currently the audiobook narrator. The book retains its Kokoro voice/model/synthesis speed when preparation resumes; playback speed is independent. Missing models produce an actionable error. Hosted speech and model downloading are not automatic.

**Listen from this page** begins at the selected visible passage and prepares forward. It never searches backward to complete the imported chapter. Internal sentence commits and cross-page sentence matching keep capture and audio safe; they do not make partial chapters resumable. A changed page layout starts a new page session while retaining the previous recording.

Chapters remain grouped under their book as **Preparing**, **Partial**, **Needs attention**, or **Complete**. Completion requires a verified beginning, a captured next chapter or verified book end, continuous coverage, and all chapter audio committed. Imported fragments stay session-only even after reaching their end. Following chapters become resumable once recorded completely. Both the library and connected overlay offer Continue only for complete chapters; the native playback command rejects partial chapter requests. Saved playback starts at the selected completed chapter and continues like a playlist through the complete chapters that follow it, stopping after the last consecutive complete chapter. Seeking stays within the playing chapters, and playback does not automatically restart reader preparation.

**Build complete audiobook** first scans from the verified beginning to the verified end, saving each captured page. Before any new synthesis, it writes `full-<range UUID>/transcript.json` containing the complete text, source pages and chapter boundaries. The second stage generates audio from that saved transcript and reuses only matching complete chapter audio. Once capture finishes, rendering and interrupted-rendering recovery no longer require a connected reader. The UI distinguishes transcript capture from audio generation. Missing controls never imply a verified book boundary. Saved complete chapters can play offline. Opening a library book does not automatically reopen browser tabs.

## Validation and remaining work

Earlier live verification exercised the relay popup and page narration. That outdated audiobook was removed from the active library at the user's request. A subsequent live mid-chapter run saved Chapter One across Kindle pages 11–13, closed by Chapter Two on page 14. Its chapter WAV and sentence index both contain 240.875 seconds across 49 speech segments; earlier-page publication preserved the imported sentence bookmark. Full-book generation and whole-process resume still remain to be exercised live.

Rust tests cover chapter completion eligibility, rejection of imported fragments after reopening, bounded saved playback, page reflow starting a new session without discarding old audio, cross-page sentence conservation, full-book capture and cache reuse, durable bookmarks, and interrupted/failed sentence commits. Relay tests cover caller identity, re-registration, expired requests, outdated workers and popup timeout handling.

OCR text is preserved rather than rewritten. Decorative initials, printed headers/footers and OCR character/spacing errors still need broader evaluation. Chapter headings are recognized conservatively; unresolved text before a chapter stops generation for review. Rendering a full sentence at a time may change prosody compared with page-sized synthesis. Reader layout changes can invalidate stored page numbers/text matches.

Install [TextHalo Reader from the Chrome Web Store](https://chromewebstore.google.com/detail/texthalo-reader/jafnpokahhbldophkeogljemkblfmgmo). The Mac app installs and registers the native helper on launch. Launch TextHalo once, add the extension to Chrome, then open a supported Kindle book and choose **Connect to TextHalo** in the popup. Manual unpacked installation is only needed for local development. Store-installed extension testing with the distributed app and broader full-book verification remain to be exercised.

Official references:
- https://developer.chrome.com/docs/extensions/develop/concepts/activeTab
- https://developer.chrome.com/docs/extensions/reference/api/tabs
- https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging


### Live validation — October 9, 2026

Installed unpacked TextHalo Reader 0.4.0 in Chrome and registered its native helper. A Tauri dev build connected to a real Kindle book after the explicit extension gesture. Creation produced approximately 3½ minutes of committed audio across two pages; local capture, verified page turns, pause/resume, speed selection, seeking and stopping preparation were exercised from the extension. Disconnect hid the app connection notice, and the saved library/bookmark survived a dev app restart. This was a short buffer test, not full-book validation.

Live testing repaired the empty-book/playback popup case, polling that hid controls or reset an active speed/seek interaction, and the app’s missing Save-dialog capability. Regression checks cover the first-time connected state and preservation of controls/focused speed selection. Seven Rust reader-bridge tests and sixteen JavaScript reader-relay tests passed.
