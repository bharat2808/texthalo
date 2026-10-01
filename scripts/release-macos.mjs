// Production macOS release orchestration. No publication or deployment by default.
import { createHash, createPublicKey, verify } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile, writeFile, mkdir, mkdtemp, rm, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createManifest, inspectArchive } from "./write-updater-manifest.mjs";

const REPO = "bharat2808/texthalo";
const ARCHITECTURES = ["aarch64"];
const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const assetNames = () => [...ARCHITECTURES.flatMap(a => [`TextHalo-macOS-${a}.dmg`, `TextHalo-${a}.app.tar.gz`, `TextHalo-${a}.app.tar.gz.sig`]), "latest.json"];

function run(command, args, { env = process.env, capture = false } = {}) {
  // Never log environment values (the updater key is supplied only via env).
  console.log(`→ ${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, { cwd: ROOT, env, encoding: "utf8", stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit", maxBuffer: 16 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`${command} failed (${result.status ?? result.error?.code}).${capture ? " Check the command separately for diagnostics." : ""}`);
  return result.stdout?.trim() ?? "";
}

export function buildPlan({ root, output, version, identity, profile }) {
  const steps = [];
  const step = (command, args, extra = {}) => steps.push({ command, args, ...extra });
  for (const arch of ARCHITECTURES) {
    const target = `${arch}-apple-darwin`;
    const app = join(root, "src-tauri/target", target, "release/bundle/macos/TextHalo.app");
    const zip = join(output, `${arch}-notarization.zip`);
    const staging = join(output, `${arch}-dmg`);
    const dmg = join(output, `TextHalo-macOS-${arch}.dmg`);
    const archive = join(output, `TextHalo-${arch}.app.tar.gz`);
    step("npm", ["run", "tauri", "build", "--", "--target", target, "--bundles", "app", "--config", JSON.stringify({ bundle: { createUpdaterArtifacts: false, macOS: { signingIdentity: identity } } })]);
    step("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
    step("/usr/bin/ditto", ["-c", "-k", "--keepParent", app, zip]);
    step("xcrun", ["notarytool", "submit", zip, "--keychain-profile", profile, "--wait", "--timeout", "30m", "--output-format", "json"], { acceptedNotarization: true });
    step("xcrun", ["stapler", "staple", app]);
    step("xcrun", ["stapler", "validate", app]);
    step("/usr/sbin/spctl", ["--assess", "--type", "execute", "--verbose", app]);
    // Tauri updater 2.12 strips the first archive path component, so the app
    // must be the root component and its Contents/ becomes the installed app
    // root. Disable Apple's implicit ._ sidecar entries; those have no child
    // path to strip and make the updater fail while unpacking.
    step("tar", ["-czf", archive, "-C", resolve(app, ".."), "TextHalo.app"], { env: { COPYFILE_DISABLE: "1" } });
    step("npm", ["run", "tauri", "signer", "sign", "--", "--app-version", version, archive]);
    step("/bin/mkdir", [staging]);
    step("/usr/bin/ditto", [app, join(staging, "TextHalo.app")]);
    step("/bin/ln", ["-s", "/Applications", join(staging, "Applications")]);
    step("hdiutil", ["create", "-volname", "TextHalo", "-srcfolder", staging, "-format", "UDZO", dmg]);
    step("/usr/bin/codesign", ["--sign", identity, "--timestamp", dmg]);
    step("xcrun", ["notarytool", "submit", dmg, "--keychain-profile", profile, "--wait", "--timeout", "30m", "--output-format", "json"], { acceptedNotarization: true });
    step("xcrun", ["stapler", "staple", dmg]);
    step("xcrun", ["stapler", "validate", dmg]);
    step("/usr/sbin/spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", "--verbose", dmg]);
  }
  return steps;
}

export async function executePlan(steps, execute = run, env = process.env) {
  for (const step of steps) {
    const result = await execute(step.command, step.args, { env: { ...env, ...step.env }, capture: Boolean(step.acceptedNotarization) });
    if (step.acceptedNotarization && JSON.parse(result).status !== "Accepted") {
      throw new Error("Apple notarization was not Accepted. No release will be published; inspect the notarytool log.");
    }
  }
}

async function digestFile(path, algorithm = "sha256") {
  const hash = createHash(algorithm);
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest();
}

// Tauri wraps the standard Minisign text format in base64. Verify with Node's
// Ed25519 primitive, including the global signature covering the trusted comment.
export async function verifyUpdaterSignature(archive, signature, publicKey) {
  const decode = value => Buffer.from(value.trim(), "base64").toString("utf8").trim().split(/\r?\n/);
  const key = Buffer.from(decode(publicKey)[1] ?? "", "base64");
  const lines = decode(signature);
  const packet = Buffer.from(lines[1] ?? "", "base64");
  const globalSignature = Buffer.from(lines[3] ?? "", "base64");
  if (key.length !== 42 || packet.length !== 74 || globalSignature.length !== 64 || !lines[2]?.startsWith("trusted comment: ") || !key.subarray(2, 10).equals(packet.subarray(2, 10))) throw new Error("Invalid updater signature or signing key");
  const algorithm = packet.subarray(0, 2).toString();
  if (!["ED", "Ed"].includes(algorithm)) throw new Error("Unsupported updater signature algorithm");
  const publicObject = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10)]), format: "der", type: "spki" });
  const message = algorithm === "ED" ? await digestFile(archive, "blake2b512") : await readFile(archive);
  const signatureBytes = packet.subarray(10);
  if (!verify(null, message, publicObject, signatureBytes) || !verify(null, Buffer.concat([signatureBytes, Buffer.from(lines[2].slice(17))]), publicObject, globalSignature)) throw new Error("Updater signature verification failed");
}

export async function validateAssetSet(directory, receipt) {
  for (const name of assetNames()) {
    if (!/^[a-f0-9]{64}$/.test(receipt.files?.[name] ?? "")) throw new Error(`Missing verified asset: ${name}`);
    if (!(await stat(join(directory, name))).isFile()) throw new Error(`Missing file: ${name}`);
    if ((await digestFile(join(directory, name))).toString("hex") !== receipt.files[name]) throw new Error(`Asset checksum changed: ${name}`);
  }
}

export function productionBranch(projects) {
  const project = projects.find(p => p.name === "texthalo");
  if (!project?.production_branch) throw new Error("Cannot determine texthalo Pages production branch");
  return project.production_branch;
}

export function validateReleaseBuildEnvironment(env) {
  for (const name of ["VITE_TEXTHALO_API_URL", "VITE_TURNSTILE_SITE_KEY"]) {
    if (!env[name]?.trim()) throw new Error(`Set ${name} before building the production app and website.`);
  }
  for (const name of ["VITE_TEXTHALO_API_URL", "VITE_NEON_AUTH_URL", "VITE_TEXTHALO_WEBSITE_URL"]) {
    const value = env[name]?.trim();
    if (!value) continue;
    let url;
    try { url = new URL(value); } catch { throw new Error(`${name} must be an absolute HTTPS URL.`); }
    if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) {
      throw new Error(`${name} must be an absolute HTTPS URL without credentials, query, or fragment.`);
    }
  }
}

async function pagesProject() {
  // Wrangler's formatted project list omits production_branch. Read the project
  // API with the CLI's active credentials, kept in memory and never printed.
  const who = JSON.parse(run("npx", ["--no-install", "wrangler", "whoami", "--json"], { capture: true }));
  const account = process.env.CLOUDFLARE_ACCOUNT_ID ?? (who.accounts?.length === 1 ? who.accounts[0].id : null);
  if (!account || !/^[a-f0-9]{32}$/.test(account)) throw new Error("Set CLOUDFLARE_ACCOUNT_ID to the account containing the existing texthalo Pages project.");
  const auth = JSON.parse(run("npx", ["--no-install", "wrangler", "auth", "token", "--json"], { capture: true }));
  const headers = auth.type === "api_key" ? { "X-Auth-Key": auth.key, "X-Auth-Email": auth.email } : { Authorization: `Bearer ${auth.token}` };
  const response = await fetch(`https://api.cloudflare.com/client/v4/accounts/${account}/pages/projects/texthalo`, { headers, signal: AbortSignal.timeout(30_000) });
  const data = await response.json();
  if (!response.ok || !data.success) throw new Error("Could not read the existing texthalo Pages project; refusing deployment.");
  return { branch: productionBranch([data.result]), account };
}

async function configuration() {
  const pkg = JSON.parse(await readFile(join(ROOT, "package.json"), "utf8"));
  const config = JSON.parse(await readFile(join(ROOT, "src-tauri/tauri.conf.json"), "utf8"));
  const cargo = await readFile(join(ROOT, "src-tauri/Cargo.toml"), "utf8");
  const lock = JSON.parse(await readFile(join(ROOT, "package-lock.json"), "utf8"));
  const cargoLock = await readFile(join(ROOT, "src-tauri/Cargo.lock"), "utf8");
  if (!/^\d+\.\d+\.\d+$/.test(pkg.version) || pkg.version !== config.version || pkg.version !== cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1] || pkg.version !== lock.version || pkg.version !== lock.packages[""].version || pkg.version !== cargoLock.match(/name = "kiegen"\nversion = "([^"]+)"/)?.[1]) throw new Error("Release versions must agree in package.json, lockfiles, Cargo.toml and tauri.conf.json");
  if (!config.bundle.macOS.signingIdentity?.startsWith("Developer ID Application:")) throw new Error("A Developer ID signing identity is required");
  return { version: pkg.version, config };
}

async function verifiedManifest(directory, version, config) {
  const entries = [];
  for (const arch of ARCHITECTURES) {
    const archive = join(directory, `TextHalo-${arch}.app.tar.gz`);
    const signature = await readFile(`${archive}.sig`, "utf8");
    await verifyUpdaterSignature(archive, signature, config.plugins.updater.pubkey);
    entries.push(await inspectArchive(`darwin-${arch}`, `https://github.com/${REPO}/releases/download/v${version}/TextHalo-${arch}.app.tar.gz`, archive, `${archive}.sig`, async app => {
      run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
      run("xcrun", ["stapler", "validate", app]);
      run("/usr/sbin/spctl", ["--assess", "--type", "execute", app]);
    }));
    const dmg = join(directory, `TextHalo-macOS-${arch}.dmg`);
    run("/usr/bin/codesign", ["--verify", "--strict", dmg]);
    run("xcrun", ["stapler", "validate", dmg]);
    run("/usr/sbin/spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", dmg]);
  }
  return createManifest(version, entries, process.env.RELEASE_NOTES ?? "");
}

function cleanCommit() {
  if (run("git", ["status", "--porcelain", "--untracked-files=normal"], { capture: true })) throw new Error("Commit or isolate the intended release changes first; release builds require a clean worktree.");
  return run("git", ["rev-parse", "HEAD"], { capture: true });
}

async function prepare(directory, version, config) {
  const commit = cleanCommit();
  const profile = process.env.NOTARY_KEYCHAIN_PROFILE;
  if (!profile) throw new Error("Set NOTARY_KEYCHAIN_PROFILE to your existing notarytool Keychain profile name.");
  const installed = run("rustup", ["target", "list", "--installed"], { capture: true }).split(/\s+/);
  for (const arch of ARCHITECTURES) if (!installed.includes(`${arch}-apple-darwin`)) throw new Error(`Install the Rust target first: rustup target add ${arch}-apple-darwin`);
  const env = { ...process.env, APPLE_SIGNING_IDENTITY: config.bundle.macOS.signingIdentity };
  for (const key of ["ORT_LIB_PATH", "ORT_LIB_LOCATION", "ORT_PREFER_DYNAMIC_LINK"]) delete env[key];
  if (!env.TAURI_SIGNING_PRIVATE_KEY) {
    env.TAURI_SIGNING_PRIVATE_KEY = env.TAURI_SIGNING_KEYCHAIN_SERVICE
      ? run("security", ["find-generic-password", "-s", env.TAURI_SIGNING_KEYCHAIN_SERVICE, "-w"], { capture: true })
      : await readFile(env.TAURI_SIGNING_PRIVATE_KEY_PATH ?? join(homedir(), ".tauri/kiegen-updater.key"), "utf8");
  }
  env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= "";
  // Notarization is explicit below, using the selected Keychain profile.
  for (const key of ["APPLE_ID", "APPLE_PASSWORD", "APPLE_API_KEY", "APPLE_API_KEY_PATH", "APPLE_API_ISSUER"]) delete env[key];
  await mkdir(resolve(directory, ".."), { recursive: true });
  await mkdir(directory); // Refuse reuse: partial artifacts must never masquerade as a fresh build.
  run("npm", ["test"]);
  run("npm", ["run", "website:build"]);
  run("cargo", ["test", "--locked", "--manifest-path", "src-tauri/Cargo.toml"]);
  await executePlan(buildPlan({ root: ROOT, output: directory, version, identity: config.bundle.macOS.signingIdentity, profile }), run, env);
  const manifest = await verifiedManifest(directory, version, config);
  await writeFile(join(directory, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  if (cleanCommit() !== commit) throw new Error("Source changed during the release build; no receipt written.");
  const files = {};
  for (const name of assetNames()) files[name] = (await digestFile(join(directory, name))).toString("hex");
  await writeFile(join(directory, "release-receipt.json"), `${JSON.stringify({ version, commit, files }, null, 2)}\n`);
  console.log(`Prepared Apple Silicon release in ${directory}. Nothing published.`);
}

async function receiptFor(directory, version, config) {
  const receipt = JSON.parse(await readFile(join(directory, "release-receipt.json"), "utf8"));
  if (receipt.version !== version || receipt.commit !== cleanCommit()) throw new Error("Prepared release does not match the current clean source commit/version.");
  await validateAssetSet(directory, receipt);
  const expected = await verifiedManifest(directory, version, config);
  const actual = JSON.parse(await readFile(join(directory, "latest.json"), "utf8"));
  if (actual.version !== expected.version || JSON.stringify(actual.platforms) !== JSON.stringify(expected.platforms)) throw new Error("Manifest does not match verified release archives");
  return receipt;
}

async function verifyPublicAssets(receipt) {
  const latest = JSON.parse(run("gh", ["api", `repos/${REPO}/releases/latest`], { capture: true }));
  if (latest.tag_name !== `v${receipt.version}` || latest.draft || latest.prerelease) throw new Error("The prepared version is not the latest public stable release");
  for (const name of assetNames()) {
    const response = await fetch(`https://github.com/${REPO}/releases/latest/download/${name}`, { signal: AbortSignal.timeout(300_000) });
    if (!response.ok || !response.body) throw new Error(`Public download unavailable: ${name} (${response.status})`);
    const hash = createHash("sha256");
    for await (const chunk of response.body) hash.update(chunk);
    if (hash.digest("hex") !== receipt.files[name]) throw new Error(`Public download checksum mismatch: ${name}`);
  }
}

export async function publishRelease(directory, receipt, execute = run, verifyPublic = verifyPublicAssets) {
  await validateAssetSet(directory, receipt);
  // An existing tag could point at different code. Never reuse or overwrite it.
  const refs = await execute("git", ["ls-remote", "--tags", `https://github.com/${REPO}.git`, `refs/tags/v${receipt.version}`], { capture: true });
  if (refs) throw new Error("This version already has a remote tag. Bump the version; do not overwrite a release.");
  await execute("gh", ["release", "create", `v${receipt.version}`, ...assetNames().map(name => join(directory, name)), "--repo", REPO, "--target", receipt.commit, "--title", `TextHalo v${receipt.version}`, "--generate-notes", "--draft"]);
  const downloaded = await mkdtemp(join(tmpdir(), "texthalo-release-upload-"));
  try {
    await execute("gh", ["release", "download", `v${receipt.version}`, "--repo", REPO, "--dir", downloaded]);
    await validateAssetSet(downloaded, receipt);
  } finally { await rm(downloaded, { recursive: true, force: true }); }
  await execute("gh", ["release", "edit", `v${receipt.version}`, "--repo", REPO, "--draft=false", "--latest"]);
  await verifyPublic(receipt);
}

async function deploy(receipt) {
  // Verify the published artifacts before deploying the download page.
  await verifyPublicAssets(receipt);
  const { branch, account } = await pagesProject();
  run("npm", ["run", "website:build"]);
  run("npx", ["--no-install", "wrangler", "pages", "deploy", "dist-website", "--project-name", "texthalo", "--branch", branch, "--commit-hash", receipt.commit], { env: { ...process.env, CLOUDFLARE_ACCOUNT_ID: account } });
  const response = await fetch(`https://texthalo.app/?release=${receipt.version}`, { signal: AbortSignal.timeout(30_000), cache: "no-store" });
  const html = await response.text();
  if (!response.ok || !ARCHITECTURES.every(arch => html.includes(`TextHalo-macOS-${arch}.dmg`))) throw new Error("Deployment submitted, but the live download page has not verified. Check Pages status before retrying.");
  const redirect = await fetch("https://texthalo.app/download/", { redirect: "manual", signal: AbortSignal.timeout(30_000) });
  if (redirect.status !== 302 || redirect.headers.get("location") !== `https://github.com/${REPO}/releases/latest/download/TextHalo-macOS-aarch64.dmg`) throw new Error("Legacy download redirect did not verify.");
  console.log("Verified production download links and legacy redirect: https://texthalo.app/");
}

export function parseReleaseArgs(args) {
  const [mode, ...flags] = args;
  if (!["plan", "prepare", "verify", "publish", "deploy", "release"].includes(mode)
    || flags.some(flag => flag !== "--deploy-cloudflare") || flags.length > 1
    || (flags.length && !["release", "publish"].includes(mode))) {
    throw new Error("Usage: npm run release:mac -- <plan|prepare|verify|publish|deploy|release> [--deploy-cloudflare (release or publish only)]");
  }
  return { mode, deployCloudflare: flags.includes("--deploy-cloudflare") };
}

export async function runReleaseWorkflow({ mode, deployCloudflare }, operations) {
  if (mode === "prepare" || mode === "release") await operations.prepare();
  if (mode === "prepare") return;
  const receipt = await operations.verify();
  if (mode === "publish" || mode === "release") await operations.publish(receipt);
  if (mode === "deploy" || deployCloudflare) {
    try { await operations.deploy(receipt); }
    catch (error) {
      throw new Error(`Website deployment failed; the GitHub release is not rolled back. After resolving the error, retry with npm run release:mac -- deploy. ${error.message}`, { cause: error });
    }
  }
}

async function main() {
  const options = parseReleaseArgs(process.argv.slice(2));
  const { mode } = options;
  const { version, config } = await configuration();
  const directory = join(ROOT, "release-artifacts", version);
  if (mode === "plan") return console.log(JSON.stringify(buildPlan({ root: ROOT, output: directory, version, identity: config.bundle.macOS.signingIdentity, profile: process.env.NOTARY_KEYCHAIN_PROFILE ?? "<keychain-profile>" }), null, 2));
  validateReleaseBuildEnvironment(process.env);
  if (process.platform !== "darwin") throw new Error("Signed macOS releases must run on macOS");
  await runReleaseWorkflow(options, {
    prepare: () => prepare(directory, version, config),
    verify: () => receiptFor(directory, version, config),
    publish: receipt => publishRelease(directory, receipt),
    deploy,
  });
  console.log(`${mode} completed for ${version}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
