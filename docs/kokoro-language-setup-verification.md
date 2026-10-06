# Kokoro additional voice setup

The Voices screen offers one action to install eSpeak NG and Kokoro's 13 additional voice tables for Spanish, French, Hindi, Italian, and Brazilian Portuguese. Japanese and Mandarin remain unavailable because their text front ends are not implemented.

The native command detects an existing runtime and skips its installation. It then runs the existing selected-model installer, which reuses completed files and fetches missing files. Catalog readiness checks each voice table's expected size, so older installations with eSpeak NG but missing tables still show the download button and explain why those voices cannot yet be selected. Partial failures retain a retry action and settle both language and model progress.

Model actions now fit their labels, share a wrapping row, and use a secondary delete button. The additional voice setup button also fits its label. The model action says “Download missing files” when the graph is installed but its download plan is incomplete. Setup labels and messages have French and Spanish translations.

Verification:

- JavaScript tests: 66 passed.
- Rust library tests: 240 passed, 13 ignored (optional hardware/model/network tests).
- Kokoro browser scenarios: 3 passed, including fresh setup, existing-runtime repair, and retry after failed file transfer. The existing-runtime scenario also checks compact button widths and a shared model-action row at a 780-pixel window width.
- Existing desktop localization browser scenarios: 7 passed.
- Desktop and website production builds passed.
- Rust Clippy with warnings denied passed; formatting and diff checks passed.

Browser scenarios mock native IPC. Runtime orchestration and file readiness have native unit coverage; this verification did not install packages or download production model files onto this Mac.
