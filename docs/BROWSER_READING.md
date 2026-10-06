# Browser audiobooks

TextHalo owns the library, narration, buffering, playback and bookmarks. The browser extension is a relay: its popup reports support, platform, book title, page position and connection status. Currently supported: Kindle web on `read.amazon.ca` and `read.amazon.com`.

## Reader discovery and capture

`browser-extension/kindle.js` reports changed reader presence. The background worker connects ready, selected book tabs automatically and refreshes the app's reader metadata while polling for commands. Switching tabs or closing the book expires/disconnects its suggestion. The adapter verifies book identity, layout, visible page surfaces and page changes before acknowledging navigation; it refuses stale or covered pages.

Chrome's `activeTab` permission requires an extension gesture before screenshot capture. Open the popup for the tab and choose **Enable page capture**. Automatic metadata detection works without that grant; narration cannot start capturing until it is granted. No broad `<all_urls>` access is requested. The selected reader tab can remain in a background browser window; an inactive tab within that window cannot be captured.

The native helper runs local Apple Vision OCR using `src-tauri/native/ScreenText.swift`. It forwards app messages to a random-port loopback listener authenticated with a user-private token. Reader commands are bound to the connected tab/book, expire, and are removed on timeout. Page scripts cannot submit app commands. The installed launcher no longer saves screenshot/OCR diagnostics by default.

## App UX

`src/Audiobooks.tsx` lists imported books and opens their growing audio, playback controls, current sentence and checkpoints. `src/SpeechOverlay.tsx` offers **Create** or **Continue** when the app detects a ready book. Hover or keyboard focus expands the tray; leaving collapses it after 1.5 seconds. Paused playback keeps controls visible. Dismiss hides that book's suggestion for the app session; its library entry remains accessible.

The tray disappears when the connected book is no longer ready/available, or when dismissed. Ongoing playback retains the existing speech player's completion/error policies. Suggestions do not activate or steal focus from the browser. Chapter checkpoints are recognized from captured chapter/part/prologue/epilogue headings, not from an inferred table of contents; unavailable checkpoints stay disabled until their audio commits. The first captured page is also a checkpoint.

## Durable recording

`src-tauri/src/audiobooks.rs` owns generation independently of the webview. Each book has a private directory under `~/Library/Application Support/kiegen/audiobooks/<UUID>/` containing:

- `audio.wav`: the single growing 24 kHz mono PCM recording.
- `book.json`: source pages, sentence text, source byte ranges in normalized OCR text, chapter checkpoints, committed sample boundaries and the book's narration settings.
- `bookmark.json`: listening time and speed, updated independently of the source index roughly once per second.

A sentence is rendered to a transient scratch WAV, appended and synced, then published by atomically replacing the sentence commit manifest. Failed/interrupted appends roll back to the manifest's committed prefix. Only committed sentences become playable. Startup checks contiguous sentence/sample boundaries, repairs the WAV header and discards an uncommitted suffix. The scratch file is removed after synthesis and on recovery.

Playback maps the committed PCM file instead of copying a multi-hour recording into RAM. Each sentence commit extends the player's accessible mapped prefix. The same native player supplies pause, resume, seek and speed controls. Bookmarks are updated only for the audiobook's own speech job; another reading cannot overwrite its progress.

Local Kokoro is currently the audiobook narrator. The book retains its Kokoro voice/model/synthesis speed when preparation resumes; playback speed is independent. Missing models produce an actionable error. Hosted speech and model downloading are not automatic.

**Buffer as I listen** captures a whole page at a time and commits its sentences individually, then pauses page fetching once roughly two minutes of audio are ahead of the listening bookmark. An unfinished sentence carries source ranges across the page boundary. Generation persists across app section changes, stops safely after the current operation, and can resume from disk after reopening the app. Resuming generation restores the final captured page (up to 100 verified turns) and compares its OCR text before advancing.

**Create complete audiobook** generates to a verified final page and is enabled for imports that began at a positively identified start of the book. Mid-book imports remain excerpts from that source position and cannot be relabeled complete. A visible, explicitly disabled previous/next control is required to establish start/end; missing controls do not imply completion. The adapter may need updates for other Kindle layouts. Cached audio can be played offline; opening a library book does not automatically reopen browser tabs or synchronize the visible page to every sentence.

## Validation and remaining work

The new relay popup was verified live with the correct Kindle title, page 11, support `true` and app connection. The app's new durable library generated 37 sentences across two pages into a single 172.5-second WAV. Full-book generation and whole-process resume have not yet been exercised end to end with the new pipeline.

Rust tests cover cross-page sentence conservation, abbreviations, chapter checkpoint availability, repeat-page refusal, interrupted append recovery, failed commit rollback, and playback restricted to a committed disk prefix. Relay tests cover caller identity, re-registration, expired requests, outdated workers and popup timeout handling. The former React experiment and its generated artifacts have been removed.

OCR text is preserved rather than rewritten. Decorative initials, printed headers/footers and OCR character/spacing errors still need broader evaluation. Chapter headings are recognized conservatively; unresolved text before a chapter stops generation for review. Rendering a full sentence at a time may change prosody compared with page-sized synthesis. Reader layout changes can invalidate stored page numbers/text matches.

Store publication, app-bundled native-host installation and a signed production release remain deployment work. The unpacked relay helper can be installed with `node browser-extension/native/setup.mjs --install`; it is not yet a complete end-user installer. Do not publish a Store URL before an actual listing exists.

Official references:
- https://developer.chrome.com/docs/extensions/develop/concepts/activeTab
- https://developer.chrome.com/docs/extensions/reference/api/tabs
- https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
