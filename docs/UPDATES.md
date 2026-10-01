# Application updates

TextHalo uses Tauri's signed updater with a static manifest attached to GitHub Releases.
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
updates. On the release Mac, the local key is stored at `~/.tauri/kiegen-updater.key` with
owner-only file permissions. Never attach it to a release or commit it.

## Build and publish Apple Silicon updates

TextHalo releases support Apple Silicon (Apple M-series) Macs only. Intel builds,
Intel CI jobs, architecture detection, and custom Intel runtime compilation are
not part of the release workflow. The signed updater manifest contains only
`darwin-aarch64`.

### Automated workflow

`scripts/release-macos.mjs` uses the existing Developer ID identity and a
notarytool Keychain profile. Prerequisites are Xcode Command Line Tools, the
`aarch64-apple-darwin` Rust target, Node dependencies, and authenticated gh.
Wrangler authentication is required only for Cloudflare deployment.

For a single local command that builds, signs, notarizes, verifies, and publishes
the GitHub release:

```sh
export NOTARY_KEYCHAIN_PROFILE="your-existing-profile-name"
npm run release:mac -- release
# Also deploy the updated website to Cloudflare Pages afterward:
npm run release:mac -- release --deploy-cloudflare
```

The website is built/tested during preparation. Its stable download URL follows
the latest GitHub release automatically; publishing the updated website itself
requires `--deploy-cloudflare`. Publication and deployment run sequentially, not
atomically: a failed website deployment does not roll back the GitHub release.
Resolve the deployment error and retry with `npm run release:mac -- deploy`,
without rebuilding or republishing. If you already prepared the artifacts, use
`npm run release:mac -- publish --deploy-cloudflare`.

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
publication over an existing tag. Push the source commit before publishing so
GitHub can resolve the release target. It never switches branches, commits, or
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
to Apple but does not publish to GitHub or deploy the website.

The updater key is read from `TAURI_SIGNING_PRIVATE_KEY`, or the Keychain service
named by `TAURI_SIGNING_KEYCHAIN_SERVICE`, or `TAURI_SIGNING_PRIVATE_KEY_PATH`
(default `~/.tauri/kiegen-updater.key`). Set
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if encrypted. Secret values are never
printed or passed as command arguments. Failed output folders remain available
for inspection; move one aside before retrying.

`publish` rechecks the receipt and signatures and uploads these four assets as
a draft release:

- `TextHalo-macOS-aarch64.dmg`
- `TextHalo-aarch64.app.tar.gz`
- `TextHalo-aarch64.app.tar.gz.sig`
- `latest.json`

It downloads and compares the uploaded checksums before making the release
public/latest. Failed upload verification leaves a draft for inspection.
It then verifies the public latest-download URLs against the same checksums.

`deploy` verifies the public artifacts first, reads the existing texthalo Pages
project's actual production branch using Wrangler's active credentials, builds
and deploys the website, and checks the live direct download links and legacy redirect. Set
`CLOUDFLARE_ACCOUNT_ID` if the authenticated user has multiple accounts.
The script never creates a project or changes its production branch.

All website download buttons link directly to the stable Apple Silicon DMG URL.
The website states the Apple Silicon requirement. `/download` and `/download/`
remain as HTTP 302 redirects for existing links; there is no separate download
page, Intel download, or browser architecture detection.

### Manual manifest generation

After signing and notarizing the app and refreshing its updater archive/signature:

```sh
node scripts/write-updater-manifest.mjs \
  0.1.9 latest.json \
  --platform darwin-aarch64 \
  https://github.com/bharat2808/texthalo/releases/download/v0.1.9/TextHalo-aarch64.app.tar.gz \
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
