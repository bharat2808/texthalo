# Application updates

TextHalo uses Tauri's signed updater with a static manifest at `https://storage.texthalo.app/updates/v2/latest.json`.
Release artifacts are stored in the existing `texthalo-storage` R2 bucket.
The Rust updater plugin requests cache revalidation for both native/background
checks and Settings checks, and only offers versions newer than the installed app.
Release builds check at startup and every six hours. Users can also check from the tray
or Settings → General. Tray checks show a dedicated result panel; background checks
update the tray quietly. Installation always requires an explicit Install action.

Before downloading, TextHalo checks the actual bundle location, locked flag, install
folder permissions (including ACLs), and the volume used for temporary files. Mounted
installer disks, translocated apps, and cross-volume installs require moving the app
into Applications on the startup disk. If the user cannot write to the install folder,
Finder/admin-assisted replacement or installation into the user's own Applications
folder is required. Accessibility permission cannot grant filesystem update access.

Installation makes an independent recovery copy beside the app before calling Tauri's
installer. On failure it restores a missing app; it never overwrites an existing bundle.
If a bundle remains after a failure, the recovery copy is retained and its path shown.
Successful installation removes this recovery copy. Check/install/relaunch failures are
recorded in the app's macOS log directory as `updater.log`, including the build
architecture and executable path. This file stays on the Mac; it is not uploaded.

## Signing keys

The updater keypair is separate from Apple's Developer ID certificate. The public key is
in `src-tauri/tauri.conf.json`. The private key must stay outside the repository and must
be backed up securely: existing installations trust that key, so losing it prevents future
updates. On the release Mac, the replacement key is stored in the login Keychain under service `com.texthalo.updater.private-key` and at `~/.tauri/kiegen-updater.key` with owner-only file permissions. Never attach it to a release or commit it. Keep a secure backup outside the release Mac.

The original release Mac was erased and its private updater key was lost. Version 0.1.18 introduces the replacement key and a separate v2 update feed. Users of earlier releases must install 0.1.18 manually once; later updates use the replacement key. Do not publish a replacement-key signature to the legacy root feed: earlier apps would reject it.

## Build and publish Apple Silicon updates

TextHalo releases support Apple Silicon (Apple M-series) Macs only. Intel builds,
Intel CI jobs, architecture detection, and custom Intel runtime compilation are
not part of the release workflow. The signed updater manifest contains only
`darwin-aarch64`.

### Automated workflow

`scripts/release-macos.mjs` uses the existing Developer ID identity and a
notarytool Keychain profile. Prerequisites are Xcode Command Line Tools, the
`aarch64-apple-darwin` Rust target, Node dependencies, authenticated GitHub CLI (`gh auth login`) with repository write access, and authenticated Wrangler for R2 publication and Pages deployment.
Before any production build/deploy, also set `VITE_TEXTHALO_API_URL` to the
HTTPS API origin and `VITE_TURNSTILE_SITE_KEY` to the public widget site key.
The script refuses to make a production build if either is absent; these values
are embedded in the desktop app and static website, respectively.

For a single local command that builds, signs, notarizes, verifies, and publishes
the R2 and GitHub releases:

```sh
export NOTARY_KEYCHAIN_PROFILE="your-existing-profile-name"
export VITE_TEXTHALO_API_URL="https://your-api.example"
export VITE_TURNSTILE_SITE_KEY="your-public-site-key"
npm run release:mac -- release
# Also deploy the updated website to Cloudflare Pages afterward:
npm run release:mac -- release --deploy-cloudflare
```

The website is built/tested during preparation. Its stable download URL follows
the stable R2 download object automatically; publishing the updated website itself
requires `--deploy-cloudflare`. R2 publication, GitHub publication, and website deployment run sequentially, not
atomically: a failed website deployment does not roll back the R2 release.
Resolve the deployment error and retry with `npm run release:mac -- deploy`,
without rebuilding or republishing. If you already prepared the artifacts, use
`npm run release:mac -- publish --deploy-cloudflare`.

Both `release` and `publish` automatically push a `vVERSION` tag at the verified
source commit and publish the same four assets on GitHub. GitHub assets are first
uploaded to a draft, downloaded, and checksum-verified before publication as the
latest release. A conflicting remote tag is rejected. Published assets are never
overwritten; retries require identical checksums. If GitHub publication fails after
R2 succeeds, resolve the error and retry `npm run release:mac -- publish` (add
`--deploy-cloudflare` to also update the website). The tag makes the release source
available on GitHub; branch updates remain a separate `git push` operation.

The same stages remain available separately:

```sh
rustup target add aarch64-apple-darwin
npm ci
export NOTARY_KEYCHAIN_PROFILE="your-existing-profile-name"
npm run release:mac -- plan
npm run release:mac -- prepare
npm run release:mac -- verify
# After testing the prepared app and authorizing publication:
npm run release:mac -- publish
npm run release:mac -- deploy
```

Before preparation, bump all version fields in package.json, Cargo.toml,
tauri.conf.json and both lockfiles, then commit the intended release changes.
The script refuses dirty worktrees, mixed versions, reused output folders, and
changed bytes at an existing release path. Commit and preserve the intended
release source before publishing. It never switches branches, commits, or
bumps versions automatically.

`prepare` runs tests, builds the Apple Silicon app, signs it with Developer ID,
notarizes and staples it, then creates and signs the updater archive from that
stapled app. The updater archive roots entries at `TextHalo.app/Contents/`;
Tauri's macOS updater strips the first path component and installs `Contents/`
at the bundle root. The release disables and rejects AppleDouble `._` sidecars,
which otherwise cause extraction to fail. It creates a DMG with an Applications shortcut and signs, notarizes,
and staples the DMG. It verifies Gatekeeper, the actual Mach-O architecture,
bundle version, and updater signature against the existing trusted public key.
Apple notarization must return Accepted. Outputs and a source-commit/checksum
receipt are saved in `release-artifacts/VERSION/`. Preparation submits artifacts
to Apple but does not publish to R2 or deploy the website.

The updater key is read from `TAURI_SIGNING_PRIVATE_KEY`, or the Keychain service
named by `TAURI_SIGNING_KEYCHAIN_SERVICE`, or `TAURI_SIGNING_PRIVATE_KEY_PATH`
(default `~/.tauri/kiegen-updater.key`). Set
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if encrypted. Secret values are never
printed or passed as command arguments. Failed output folders remain available
for inspection; move one aside before retrying.

`publish` rechecks the receipt and signatures and uploads these four assets
under `releases/VERSION/` in `texthalo-storage`:

- `TextHalo-macOS-aarch64.dmg`
- `TextHalo-aarch64.app.tar.gz`
- `TextHalo-aarch64.app.tar.gz.sig`
- `latest.json`

Versioned files use immutable cache headers. Existence probes use a separate
query URL so a missing object cannot cache a 404 at its canonical download URL. Existing versioned objects are
verified against the prepared checksums and never overwritten; retries can reuse
identical files. A release older than the current R2 manifest is rejected.
The script verifies public checksums before updating the root DMG download,
then publishes `updates/v2/latest.json` last. Both mutable root objects use `no-store`.
The manifest always references the immutable versioned updater archive. The old root `latest.json` feed is preserved for installations that trust the lost original key.
An upload or verification failure before promotion leaves the current manifest
unchanged; publication and promotion of the two root objects are sequential.

`deploy` verifies the public artifacts first, reads the existing texthalo Pages
project's actual production branch using Wrangler's active credentials, builds
and deploys the website, and checks the live direct download links and legacy redirect. Set
`CLOUDFLARE_ACCOUNT_ID` if the authenticated user has multiple accounts.
The script never creates a project or changes its production branch.

All website download buttons link directly to
`https://storage.texthalo.app/TextHalo-macOS-aarch64.dmg`.
The website states the Apple Silicon requirement. Deployment sets
`VITE_TEXTHALO_DOWNLOAD_VERSION` to the published artifact version; it defaults
to package.json for normal development and release builds. `/download` and `/download/`
remain as HTTP 302 redirects for existing links; there is no separate download
page, Intel download, or browser architecture detection.

### Manual manifest generation

After signing and notarizing the app and refreshing its updater archive/signature:

```sh
node scripts/write-updater-manifest.mjs \
  0.1.13 latest.json \
  --platform darwin-aarch64 \
  https://storage.texthalo.app/releases/0.1.13/TextHalo-aarch64.app.tar.gz \
  release/arm64/TextHalo.app.tar.gz release/arm64/TextHalo.app.tar.gz.sig
```

Substitute the actual new version. The helper checks the archived executable
architecture, bundle identifier and version; it rejects missing, duplicate, or
unsupported platforms. The automated release command additionally verifies the
cryptographic signature. Never publish an ARM archive under an Intel platform key.

## Verification before release

CI runs checks and bundles on the Apple Silicon `macos-15` runner. Ad-hoc CI
bundles do not substitute for testing production-signed updater artifacts.
On a clean Apple Silicon Mac/VM, install a signed older DMG, copy the app into
Applications, and update through a staging manifest. Check relaunch/version,
settings preservation, tray results, failed network/signature handling, mounted
DMG launches, standard-user permissions, locked bundles, and external volumes.
Failed replacement must leave a launchable app or an accessible recovery copy.
Keep production latest.json unchanged during staging tests.

The bundle identifier and user-data paths retain their original Kiegen values
so existing Accessibility permission, settings, and downloaded models survive
updates. Keep those values stable.

## Migration from private GitHub Releases

On October 5, 2026, the existing signed v0.1.12 artifacts were retrieved from
GitHub using the repository owner's account and published to R2.
The installer, updater archive, and signature keep their original bytes; only
the manifest's archive URL changes. Preserve the existing updater signing key.
The source tree is v0.1.13; moving v0.1.12 does not publish a new app version.

Already distributed apps, including the migrated v0.1.12 installer, still embed
the GitHub updater endpoint. Those binaries need a manual installation of the
first app release built with the R2 configuration. Uploading their old artifacts
or changing website links cannot change the endpoint inside an installed binary.
Subsequent releases built from the migrated source check R2 automatically.
