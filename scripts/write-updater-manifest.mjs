import { readFile, writeFile, mkdtemp, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const architectures = { "darwin-aarch64": "arm64" };

export function createManifest(version, entries, notes = "") {
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version)) throw new Error("Invalid release version");
  const platforms = {};
  for (const entry of entries) {
    const expected = architectures[entry.target];
    if (!expected || platforms[entry.target]) throw new Error(`Unsupported or duplicate target: ${entry.target}`);
    if (!entry.architectures.includes(expected)) throw new Error(`Wrong binary architecture for ${entry.target}`);
    if (entry.bundleVersion !== version) throw new Error(`Bundle version mismatch for ${entry.target}`);
    if (!entry.signature.trim()) throw new Error(`Empty signature for ${entry.target}`);
    const url = new URL(entry.url);
    if (url.protocol !== "https:" || !url.pathname.endsWith(".app.tar.gz")) {
      throw new Error("Updater URLs must be HTTPS .app.tar.gz archives");
    }
    platforms[entry.target] = { url: entry.url, signature: entry.signature.trim() };
  }
  for (const target of Object.keys(architectures)) {
    if (!platforms[target]) throw new Error(`Missing signed archive for ${target}`);
  }
  return { version, notes, pub_date: new Date().toISOString(), platforms };
}

// Tauri updater 2.12 removes the first component from every tar entry. The
// app bundle must therefore be the root component so Contents/ is installed
// directly at the app root. AppleDouble ._ entries are unsafe: stripping
// their only component maps a file onto the existing temp directory.
export function updaterExtractionPaths(entries) {
  const plist = entries.find((entry) => /^[^/]+\.app\/Contents\/Info\.plist$/.test(entry));
  if (!plist) throw new Error("Archive must contain a top-level macOS app bundle");
  const appName = plist.split("/")[0];
  if (entries.some((entry) => entry.split("/").some((part) => part.startsWith("._")))) {
    throw new Error("Updater archive must not contain AppleDouble ._ metadata entries");
  }
  if (entries.some((entry) => entry !== `${appName}/` && !entry.startsWith(`${appName}/`))) {
    throw new Error("Updater archive entries must all be inside the app bundle");
  }
  return entries.map((entry) => entry.split("/").slice(1).join("/"));
}

export async function inspectArchive(target, url, archivePath, signaturePath, verifyBundle) {
  // Inspect the actual executable in local release artifacts, not their filenames.
  const directory = await mkdtemp(join(tmpdir(), "texthalo-manifest-"));
  try {
    const entries = execFileSync("tar", ["-tzf", resolve(archivePath)], { encoding: "utf8" }).trim().split("\n");
    if (entries.some((entry) => entry.startsWith("/") || entry.split("/").includes(".."))) throw new Error("Unsafe archive path");
    updaterExtractionPaths(entries);
    const plist = entries.find((entry) => /^[^/]+\.app\/Contents\/Info\.plist$/.test(entry));
    const appName = plist.split("/")[0];
    execFileSync("tar", ["-xzf", resolve(archivePath), "-C", directory]);
    const bundle = join(directory, appName);
    const readPlist = (key) => execFileSync("/usr/bin/plutil", ["-extract", key, "raw", "-o", "-", join(bundle, "Contents/Info.plist")], { encoding: "utf8" }).trim();
    const executable = readPlist("CFBundleExecutable");
    if (executable.includes("/") || executable === "..") throw new Error("Invalid bundle executable");
    const binary = join(bundle, "Contents", "MacOS", executable);
    const archs = execFileSync("/usr/bin/lipo", ["-archs", binary], { encoding: "utf8" }).trim().split(/\s+/);
    if (readPlist("CFBundleIdentifier") !== "com.kiegen.app") throw new Error("Wrong bundle identifier");
    if (verifyBundle) await verifyBundle(bundle);
    return { target, url, architectures: archs, bundleVersion: readPlist("CFBundleShortVersionString"),
      signature: await readFile(signaturePath, "utf8") };
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function main() {
  const [version, output, ...args] = process.argv.slice(2);
  if (!version || !output || args.length !== 5 || args[0] !== "--platform") {
    throw new Error("Usage: node scripts/write-updater-manifest.mjs <version> <output> --platform darwin-aarch64 <url> <archive> <sig>");
  }
  const entries = [];
  for (let i = 0; i < args.length; i += 5) entries.push(await inspectArchive(...args.slice(i + 1, i + 5)));
  const manifest = createManifest(version, entries, process.env.RELEASE_NOTES ?? "");
  await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Wrote ${output} for ${version}: Apple Silicon.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
