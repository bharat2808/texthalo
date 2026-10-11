import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { buildPlan, executePlan, validateAssetSet, verifyUpdaterSignature, productionBranch, publishRelease, verifyPublicFile, assetExists, verifyWebsite, parseReleaseArgs, runReleaseWorkflow, validateReleaseBuildEnvironment } from "./release-macos.mjs";

test("combined release accepts only explicit supported deployment flags", () => {
  assert.deepEqual(parseReleaseArgs(["release"]), { mode: "release", deployCloudflare: false });
  assert.deepEqual(parseReleaseArgs(["release", "--deploy-cloudflare"]), { mode: "release", deployCloudflare: true });
  assert.deepEqual(parseReleaseArgs(["publish", "--deploy-cloudflare"]), { mode: "publish", deployCloudflare: true });
  for (const args of [[], ["unknown"], ["release", "--deploy"], ["plan", "--deploy-cloudflare"], ["release", "--deploy-cloudflare", "--deploy-cloudflare"]]) {
    assert.throws(() => parseReleaseArgs(args), /Usage/);
  }
});

test("release workflow verifies before publishing and optionally deploys last", async () => {
  const receipt = { version: "1.2.3" };
  for (const [args, expected] of [
    [["release"], ["prepare", "verify", "publish"]],
    [["release", "--deploy-cloudflare"], ["prepare", "verify", "publish", "deploy"]],
    [["publish", "--deploy-cloudflare"], ["verify", "publish", "deploy"]],
    [["prepare"], ["prepare"]], [["verify"], ["verify"]], [["deploy"], ["verify", "deploy"]],
  ]) {
    const calls = [];
    await runReleaseWorkflow(parseReleaseArgs(args), Object.fromEntries(["prepare", "verify", "publish", "deploy"].map(stage => [stage, async value => {
      if (["publish", "deploy"].includes(stage)) assert.equal(value, receipt);
      calls.push(stage);
      return receipt;
    }])));
    assert.deepEqual(calls, expected);
  }
});

test("combined release stops on failures and gives a deployment-only retry command", async () => {
  const stages = ["prepare", "verify", "publish", "deploy"];
  for (const failed of stages) {
    const calls = [];
    const operations = Object.fromEntries(stages.map(stage => [stage, async () => {
      calls.push(stage);
      if (stage === failed) throw new Error(`failed ${stage}`);
      return {};
    }]));
    await assert.rejects(runReleaseWorkflow(parseReleaseArgs(["release", "--deploy-cloudflare"]), operations), failed === "deploy" ? /retry with npm run release:mac -- deploy/ : new RegExp(`failed ${failed}`));
    assert.deepEqual(calls, stages.slice(0, stages.indexOf(failed) + 1));
  }
});

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
    assert.ok(steps[tar].args.includes("TextHalo.app"));
    assert.equal(steps[tar].env.COPYFILE_DISABLE, "1");
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

test("production release requires API and Turnstile browser configuration", () => {
  assert.throws(() => validateReleaseBuildEnvironment({}), /VITE_TEXTHALO_API_URL/);
  assert.throws(() => validateReleaseBuildEnvironment({ VITE_TEXTHALO_API_URL: "https://api.example.com" }), /VITE_TURNSTILE_SITE_KEY/);
  assert.throws(() => validateReleaseBuildEnvironment({ VITE_TEXTHALO_API_URL: "http://api.example.com", VITE_TURNSTILE_SITE_KEY: "public-key" }), /HTTPS/);
  assert.doesNotThrow(() => validateReleaseBuildEnvironment({
    VITE_TEXTHALO_API_URL: "https://api.example.com",
    VITE_NEON_AUTH_URL: "https://auth.example.com/neondb/auth",
    VITE_TEXTHALO_WEBSITE_URL: "https://texthalo.app",
    VITE_TURNSTILE_SITE_KEY: "public-key",
  }));
});

test("R2 publication verifies versioned uploads before promoting downloads and latest manifest", async () => {
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
      const execute = async (command, args) => { actions.push([command, ...args]); return ""; };
      const verifyFile = async (url, hash) => {
        actions.push(["verify", url, hash]);
        if (corrupt && url.endsWith(".app.tar.gz")) throw new Error("Public download checksum mismatch");
      };
      const publish = publishRelease(dir, receipt, { execute, verifyFile, readLatest: async () => null, exists: async () => false });
      if (corrupt) await assert.rejects(publish, /checksum/); else await publish;
      const uploads = actions.filter(a => a[0] === "npx");
      assert.ok(uploads.every(a => a.includes("--remote")));
      assert.ok(uploads.every(a => a.some(arg => arg.startsWith("texthalo-storage/"))));
      assert.equal(uploads.some(a => a.includes("texthalo-storage/updates/v2/latest.json")), !corrupt);
      assert.ok(!uploads.some(a => a.includes("texthalo-storage/latest.json")), "legacy installations keep their original signing feed");
      assert.equal(uploads.some(a => a.includes("texthalo-storage/TextHalo-macOS-aarch64.dmg")), !corrupt);
      if (!corrupt) {
        const promotion = actions.findIndex(a => a.includes("texthalo-storage/updates/v2/latest.json"));
        for (const name of names) {
          assert.ok(actions.findIndex(a => a[0] === "verify" && a[1] === `https://storage.texthalo.app/releases/1.2.3/${name}`) < promotion);
        }
        assert.ok(uploads.find(a => a.includes("texthalo-storage/updates/v2/latest.json")).includes("no-store"));
      }
      assert.ok(!actions.some(a => a[0] === "gh" || a[0] === "git"));
    }
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test("R2 publication refuses downgrades and verifies existing versioned assets without overwriting", async () => {
  const dir = await mkdtemp(join(tmpdir(), "texthalo-publish-retry-"));
  try {
    const receipt = { version: "1.2.3", files: {} };
    for (const name of ["TextHalo-macOS-aarch64.dmg", "TextHalo-aarch64.app.tar.gz", "TextHalo-aarch64.app.tar.gz.sig", "latest.json"]) {
      await writeFile(join(dir, name), name);
      receipt.files[name] = createHash("sha256").update(name).digest("hex");
    }
    const actions = [];
    const options = { execute: async (_, args) => actions.push(args), exists: async () => true,
      verifyFile: async () => { throw new Error("checksum mismatch"); }, readLatest: async () => ({ version: "1.2.4" }) };
    await assert.rejects(publishRelease(dir, receipt, options), /newer|downgrade/i);
    assert.equal(actions.length, 0);
    await assert.rejects(publishRelease(dir, receipt, { ...options, readLatest: async () => ({version:"1.2.3"}) }), /checksum/);
    assert.equal(actions.length, 0);
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

test("public verification retries R2 propagation errors but stops on changed bytes", async () => {
  const bytes = "verified release";
  const hash = createHash("sha256").update(bytes).digest("hex");
  const statuses = [404, 503, 200];
  let attempts = 0;
  const pauses = [];
  await verifyPublicFile("https://storage.texthalo.app/test", hash, {
    fetchAsset: async () => new Response(bytes, { status: statuses[attempts++] }),
    pause: async delay => pauses.push(delay),
  });
  assert.equal(attempts, 3);
  assert.equal(pauses.length, 2);
  attempts = 0;
  await assert.rejects(verifyPublicFile("https://storage.texthalo.app/test", hash, {
    fetchAsset: async () => { attempts++; return new Response("tampered"); }, pause: async () => {},
  }), /checksum/);
  assert.equal(attempts, 1);
  attempts = 0;
  await assert.rejects(verifyPublicFile("https://storage.texthalo.app/test", hash, {
    fetchAsset: async () => { attempts++; return new Response(null, {status: 404}); }, pause: async () => {},
  }), /unavailable/);
  assert.equal(attempts, 8);
});

test("existence probes do not cache a miss at the canonical download URL", async () => {
  const canonical = "https://storage.texthalo.app/releases/1.2.3/TextHalo-aarch64.app.tar.gz";
  let requested;
  assert.equal(await assetExists(canonical, async (url, method) => {
    requested = new URL(url);
    assert.equal(method, "HEAD");
    return new Response(null, {status: 404});
  }), false);
  assert.equal(requested.origin + requested.pathname, canonical);
  assert.ok(requested.searchParams.has("probe"));
});

test("website verification waits for R2 links, the published version, and redirect propagation", async () => {
  let calls = 0;
  const url = "https://storage.texthalo.app/TextHalo-macOS-aarch64.dmg";
  await verifyWebsite({version: "0.1.12"}, {
    pause: async () => {},
    fetchPage: async target => {
      if (target.includes("/download/")) return new Response(null, {status:302,headers:{location: calls < 3 ? "https://github.com/old.dmg" : url}});
      calls++;
      return new Response(`<a href="${url}">Download</a>Current version <!-- -->0.1.${calls < 2 ? "13" : "12"}<!-- -->`);
    },
  });
  assert.equal(calls, 3);
});
