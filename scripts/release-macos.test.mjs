import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { buildPlan, executePlan, validateAssetSet, verifyUpdaterSignature, productionBranch, publishRelease } from "./release-macos.mjs";

test("build plan produces the Apple Silicon DMG and signs updater archives only after stapling", () => {
  const steps = buildPlan({ root: "/repo", output: "/out", version: "1.2.3", identity: "Developer ID Application: Test (TEAM)", profile: "test-notary" });
  assert.equal(steps.filter(s => s.args.includes("build")).length, 1);
  for (const [arch, target] of [["aarch64", "aarch64-apple-darwin"]]) {
    const build = steps.findIndex(s => s.args.includes(target));
    assert.ok(build >= 0);
    const staple = steps.findIndex((s, i) => i > build && s.args[0] === "stapler" && s.args[1] === "staple" && s.args[2].endsWith(".app"));
    const tar = steps.findIndex(s => s.command === "tar" && s.args.includes(`/out/TextHalo-${arch}.app.tar.gz`));
    const sign = steps.findIndex(s => s.args.includes("signer") && s.args.includes(`/out/TextHalo-${arch}.app.tar.gz`));
    assert.ok(build < staple && staple < tar && tar < sign);
    assert.ok(steps.some(s => s.args.includes(`/out/TextHalo-macOS-${arch}.dmg`) && s.args.includes("create")));
  }
});

test("notarization rejection stops the pipeline even when the command exits zero", async () => {
  let called = 0;
  await assert.rejects(executePlan([
    { command: "xcrun", args: ["notarytool"], acceptedNotarization: true },
    { command: "should-not-run", args: [] },
  ], async () => { called++; return JSON.stringify({ status: "Invalid" }); }), /notarization/i);
  assert.equal(called, 1);
});

test("publication refuses a partial or tampered asset set", async () => {
  const dir = await mkdtemp(join(tmpdir(), "texthalo-release-test-"));
  try {
    await assert.rejects(validateAssetSet(dir, { version: "1.2.3", files: {} }), /Missing/);
    await writeFile(join(dir, "TextHalo-macOS-aarch64.dmg"), "tampered");
    await assert.rejects(validateAssetSet(dir, { version: "1.2.3", files: { "TextHalo-macOS-aarch64.dmg": "0".repeat(64) } }), /checksum|Missing/i);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test("production deployment uses the actual Pages production branch, not the Git branch", () => {
  assert.equal(productionBranch([{ name: "texthalo", production_branch: "voiceovers" }]), "voiceovers");
  assert.throws(() => productionBranch([{ name: "another-project", production_branch: "master" }]), /texthalo/);
});

test("a failed upload stays a draft; a verified upload alone can become latest", async () => {
  const dir = await mkdtemp(join(tmpdir(), "texthalo-publish-test-"));
  try {
    const names = ["TextHalo-macOS-aarch64.dmg", "TextHalo-aarch64.app.tar.gz", "TextHalo-aarch64.app.tar.gz.sig", "latest.json"];
    const receipt = { version: "1.2.3", commit: "a".repeat(40), files: {} };
    for (const name of names) {
      await writeFile(join(dir, name), name);
      receipt.files[name] = createHash("sha256").update(name).digest("hex");
    }
    for (const corrupt of [true, false]) {
      const actions = [];
      let publicVerified = false;
      const execute = async (command, args) => {
        actions.push([command, ...args]);
        if (command === "git") return "";
        if (args[1] === "download") {
          const destination = args[args.indexOf("--dir") + 1];
          for (const name of names) await writeFile(join(destination, name), corrupt && name.includes("aarch64.dmg") ? "wrong binary" : name);
        }
        return "";
      };
      const publish = publishRelease(dir, receipt, execute, async () => { publicVerified = true; });
      if (corrupt) await assert.rejects(publish, /checksum/); else await publish;
      assert.ok(actions.find(a => a[2] === "create").includes("--draft"));
      assert.equal(actions.some(a => a.includes("--latest")), !corrupt);
      assert.equal(publicVerified, !corrupt);
    }
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test("updater signatures verify against the trusted key and reject changed archives", async () => {
  const dir = await mkdtemp(join(tmpdir(), "texthalo-signature-test-"));
  const cli = join(process.cwd(), "node_modules/.bin/tauri");
  try {
    const key = join(dir, "key");
    let result = spawnSync(cli, ["signer", "generate", "--ci", "-w", key, "-p", ""], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const archive = join(dir, "app.tar.gz");
    await writeFile(archive, "original archive");
    result = spawnSync(cli, ["signer", "sign", "--app-version", "1.2.3", archive], { encoding: "utf8", env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: await readFile(key, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "" } });
    assert.equal(result.status, 0, result.stderr);
    const publicKey = await readFile(`${key}.pub`, "utf8");
    await verifyUpdaterSignature(archive, await readFile(`${archive}.sig`, "utf8"), publicKey);
    await writeFile(archive, "changed archive");
    await assert.rejects(verifyUpdaterSignature(archive, await readFile(`${archive}.sig`, "utf8"), publicKey), /signature/i);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
