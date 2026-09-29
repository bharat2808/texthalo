# TextHalo

**Select text. Press a shortcut. Hear it read aloud.**

TextHalo is a macOS menu bar text-to-speech app with Apple system voices, local AI speech through Kokoro and Chatterbox, and hosted Fish Audio speech. Built with Rust, Tauri, React, and TypeScript, it keeps playback controls close without taking focus away from your work.

**Website:** [texthalo.app](https://texthalo.app)

The website pricing page is available at `/pricing/`. It uses Neon Auth sign-in before starting Stripe Checkout. Desktop sign-in opens the branded website in the system browser, then returns through a short-lived PKCE handoff; the app stores its revocable session in macOS Keychain. The desktop Account tab also shows the signed-in account, balance, billing, and top-ups. Set `VITE_NEON_AUTH_URL`, `VITE_TEXTHALO_API_URL`, and `VITE_TEXTHALO_WEBSITE_URL` for the website/app builds, and allow the website origin in the server's `CORS_ALLOWED_ORIGINS`.

## Demo


https://github.com/user-attachments/assets/f33d93bc-57c8-4afb-8db1-70b9fac76eca


## Features

- **Read selected text** from other apps with a configurable global shortcut.
- **Choose between four speech engines:** Apple system voices, Kokoro 82M, Chatterbox Multilingual, and hosted Fish Audio voices.
- **Preview voices** directly in settings before using them.
- **Start listening sooner:** Kokoro and Chatterbox play short phrases while the next phrase is generated.
- **Control playback from a floating overlay** at the top center of the screen, with speech status and a Stop button.
- **Use reference voices with Chatterbox** by importing a local WAV clip.
- **Keep models loaded** between requests to avoid repeated model initialization.
- **Choose how text is captured:** Accessibility, copying, or Accessibility with a copy fallback.
- **Browse hosted voices, manage your account and credits, and open plan or top-up checkout** from Settings → Account.

## Get started

1. Launch TextHalo and open **Settings…** from its menu bar icon.
2. Enable TextHalo in **System Settings → Privacy & Security → Accessibility**.
3. Open **Voice**, choose an engine, and preview a voice. Apple system voices work without downloading a model; Kokoro and Chatterbox require a model download through the app. Fish Audio requires signing in from **Account** and accepting its hosted-text privacy notice.
4. Select text in another app and press **Cmd+Shift+S**.
5. Press **Cmd+Shift+X** or click **Stop** in the overlay to stop playback.

| Action | Default control |
| --- | --- |
| Read selected text | `Cmd+Shift+S` |
| Stop speech | `Cmd+Shift+X` |
| Open settings | Menu bar icon → **Settings…** |
| Preview a voice | **Voice** → **Preview** |

Change the keyboard shortcuts in **Shortcuts**. Selection length is limited to 5,000 characters by default and can be adjusted in **Capture**.

## Speech engines

| Engine | Voices and languages | Setup |
| --- | --- | --- |
| **Apple system voices** | Voices installed in macOS, with language selection and speaking-rate controls | Available immediately; the default engine |
| **Kokoro 82M** | American and British English; additional supported languages through espeak-ng | Download the model and voices in the app |
| **Chatterbox Multilingual** | 23 languages, a built-in reference voice, and imported WAV reference clips | Download the model in the app |
| **Fish Audio** | Hosted voice catalog, streamed speech, optional semantic text enhancement | Sign in, select Fish Audio, and accept the hosted-text notice |

Kokoro supports Spanish, French, Hindi, Italian, and Brazilian Portuguese through an installed espeak-ng executable. Japanese and Mandarin Kokoro voices are currently unavailable because their text front ends are not implemented.

Chatterbox has controls for emotion intensity and keeping the model loaded. Use **Add voice…** to import a WAV reference clip; TextHalo validates and converts the clip for the model.

### Optional espeak-ng support

TextHalo invokes `espeak-ng` as a separate CLI process and does not bundle or link its library. If it is missing, the in-app **Add voices** action uses Homebrew when available or installs a checksum-verified runtime under Application Support on a fresh Mac.

For English Kokoro voices, dictionary pronunciations are tried first. Unknown words use espeak-ng only when it is detected. If it is absent or cannot produce a pronunciation, TextHalo retains its letter-spelling behavior. Known words and acronyms keep their dictionary handling.

Detection includes standard Homebrew locations and `PATH`. For a custom installation, set `KIEGEN_ESPEAK_NG` to the executable's full path in the environment used to launch TextHalo.

## Text capture and privacy

Apple, Kokoro, and Chatterbox speech synthesis runs locally. Fish Audio is a hosted service: when selected, the captured text is sent to the TextHalo backend and Fish Audio to generate and stream speech. Optional semantic text enhancement also sends the text to the configured semantic cue service. Voice clone recordings are uploaded to Fish Audio when the user submits them. Review the hosted speech notice in Voice settings before enabling Fish Audio.

TextHalo requires Accessibility permission to capture another app's selection. In **Capture**, choose Accessibility-only capture to avoid using the clipboard, or use copying for apps that do not expose their selection through Accessibility. Copy-based capture can restore the previous clipboard contents; restoration is enabled by default.

Models and imported reference voices are stored locally. Both local engines keep their continuous playback samples in memory.

| Data | Default macOS location |
| --- | --- |
| Settings | `~/Library/Application Support/com.kiegen.app/settings.json` |
| Models and reference voices | `~/Library/Application Support/kiegen/models/` |
| Generated audio cache | `~/Library/Application Support/kiegen/cache/` |

These existing Kiegen-named locations are retained so the TextHalo rebrand preserves
Accessibility access, settings, models, and cached audio.

## Build from source

Use macOS with Node.js 22+, npm, Rust stable, and Xcode Command Line Tools.

```bash
git clone https://github.com/bharat2808/texthalo.git
cd texthalo
npm ci
npm run tauri dev
```

To build the macOS application:

```bash
VITE_TEXTHALO_API_URL=https://your-deployed-backend.example \
VITE_NEON_AUTH_URL=https://YOUR-NEON-AUTH-HOST/neondb/auth \
npm run tauri build -- --bundles app
```

The API origin is compiled into the native app; release builds do not fall back to localhost or a temporary tunnel. Set it to the deployed TextHalo backend when building. Debug builds default to `http://localhost:8788` for local development.

The application is written to:

```text
src-tauri/target/release/bundle/macos/TextHalo.app
```

Copy `TextHalo.app` to **Applications**, launch it, and enable Accessibility access. Rebuilding can invalidate the previous permission grant. If capture stops working after a rebuild, remove the stale TextHalo entry in Accessibility settings and enable the rebuilt app again.

## Website

The standalone TextHalo marketing site is a separate React/Vite project in `website/`; it does not replace the desktop app interface.

```bash
npm run website:dev
npm run website:build
```

The production static site is written to `dist-website/` and can be deployed to `texthalo.app` through a static hosting provider.

## Development checks

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# Use an empty model directory for the standard suite.
KIEGEN_MODELS_DIR="$(mktemp -d)" cargo test --manifest-path src-tauri/Cargo.toml

./scripts/check-licenses.sh
```

Real-model and download tests require additional assets and are not all run by the standard suite. To exercise Kokoro streaming with the models installed in their default location:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib kokoro_pcm_latency_and_stop -- --ignored --nocapture
```

This test plays audio and measures cold/warm playback startup, buffer underruns, and Stop. To measure synthesis at different sentence lengths, run the ignored `benchmark_kokoro_sentence_lengths` test. The equivalent Chatterbox measurement is `chatterbox_pcm_latency_and_stop` and requires its installed model.

## Project layout

| Path | Purpose |
| --- | --- |
| `src/` | React settings interface and speech overlay |
| `src-tauri/src/` | Native app, selection capture, speech engines, downloads, and playback |
| `src-tauri/tests/` | Integration and model verification tests |
| `scripts/` | Development and dependency-license checks |
| `docs/` | Development notes and design history |
| `spikes/` | Experimental implementations and investigation notes |

## License

TextHalo is licensed under [Apache-2.0](LICENSE-APACHE). Downloaded models and separately installed tools retain their own licenses. Dependency-license checks are defined in [`deny.toml`](deny.toml) and [`scripts/check-licenses.sh`](scripts/check-licenses.sh).

Maintained by **bharat2808**.
