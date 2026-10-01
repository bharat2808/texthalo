import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createManifest, updaterExtractionPaths } from "./write-updater-manifest.mjs";

const entries = () => [
  { target: "darwin-aarch64", architectures: ["arm64"], bundleVersion: "1.2.3", url: "https://example.com/arm.app.tar.gz", signature: "arm-signature" },
];

test("Apple Silicon alone produces a complete release manifest", () => {
  const manifest = createManifest("1.2.3", entries());
  assert.equal(manifest.platforms["darwin-aarch64"].signature, "arm-signature");
  assert.deepEqual(Object.keys(manifest.platforms), ["darwin-aarch64"]);
});
test("refuses a release without an Apple Silicon archive", () => {
  assert.throws(() => createManifest("1.2.3", []), /Missing signed archive/);
});
test("refuses an Intel binary mislabeled as Apple Silicon", () => {
  const input = entries(); input[0].architectures = ["x86_64"];
  assert.throws(() => createManifest("1.2.3", input), /Wrong binary architecture/);
});
test("refuses mismatched versions, missing signatures, duplicate targets, and insecure URLs", () => {
  for (const change of [{ bundleVersion: "1.2.2" }, { signature: " " }, { target: "darwin-x86_64" }, { url: "http://example.com/app.app.tar.gz" }]) {
    const input = entries(); Object.assign(input[0], change);
    assert.throws(() => createManifest("1.2.3", input));
  }
  assert.throws(() => createManifest("1.2.3", [...entries(), ...entries()]), /duplicate/);
});

test("updater archive strips the app-name component and rejects AppleDouble sidecars", () => {
  assert.deepEqual(updaterExtractionPaths([
    "TextHalo.app/",
    "TextHalo.app/Contents/",
    "TextHalo.app/Contents/Info.plist",
  ]), ["", "Contents/", "Contents/Info.plist"]);
  assert.throws(() => updaterExtractionPaths([
    "._TextHalo.app",
    "TextHalo.app/",
    "TextHalo.app/Contents/Info.plist",
  ]), /AppleDouble/);
});

test("CLI inspects real Mach-O archives and rejects swapped architectures", { skip: process.platform !== "darwin" }, async () => {
  const root = await mkdtemp(join(tmpdir(), "texthalo-manifest-test-"));
  try {
    const artifacts = [];
    for (const arch of ["arm64", "x86_64"]) {
      const folder = join(root, arch);
      const contents = join(folder, "TextHalo.app", "Contents");
      await mkdir(join(contents, "MacOS"), { recursive: true });
      execFileSync("/usr/bin/clang", ["-arch", arch, "-mmacosx-version-min=11.0", "-x", "c", "-o", join(contents, "MacOS", "texthalo"), "-"], { input: "int main(void) { return 0; }" });
      await writeFile(join(contents, "Info.plist"), `<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>texthalo</string><key>CFBundleIdentifier</key><string>com.kiegen.app</string><key>CFBundleShortVersionString</key><string>1.2.3</string></dict></plist>`);
      const archive = join(root, `${arch}.app.tar.gz`);
      execFileSync("tar", ["-czf", archive, "-C", folder, "TextHalo.app"], { env: { ...process.env, COPYFILE_DISABLE: "1" } });
      const archiveEntries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" }).trim().split("\n");
      assert.doesNotThrow(() => updaterExtractionPaths(archiveEntries));
      await writeFile(`${archive}.sig`, `${arch}-test-signature`);
      artifacts.push([`https://example.com/${arch}.app.tar.gz`, archive, `${archive}.sig`]);
    }
    const script = fileURLToPath(new URL("./write-updater-manifest.mjs", import.meta.url));
    const output = join(root, "latest.json");
    const args = (arm) => [script, "1.2.3", output, "--platform", "darwin-aarch64", ...arm];
    execFileSync(process.execPath, args(artifacts[0]));
    const manifest = JSON.parse(await readFile(output, "utf8"));
    assert.equal(manifest.platforms["darwin-aarch64"].signature, "arm64-test-signature");
    const swapped = spawnSync(process.execPath, args(artifacts[1]), { encoding: "utf8" });
    assert.notEqual(swapped.status, 0);
    assert.match(swapped.stderr, /Wrong binary architecture/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
