import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import vm from "node:vm";

// Exercise the real UI handler with the native/network boundary replaced.
const source = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
const handler = source.slice(source.indexOf("  const uploadClone ="), source.indexOf("  const deleteHostedClone ="));
async function run(invoke) {
  const state = { clonePath: "/recording.wav", cloneName: "My voice", cloneConsent: true, hostedClones: [], accountBusy: false, cloneUploadError: "", cloneUploadSuccess: "" };
  const context = { ...state, invoke, Error };
  for (const key of [...Object.keys(state), "accountError"]) {
    context[`set${key[0].toUpperCase()}${key.slice(1)}`] = value => { state[key] = typeof value === "function" ? value(state[key]) : value; };
  }
  await vm.runInNewContext(ts.transpile(handler + "\nuploadClone();"), context);
  return state;
}

test("rejected upload preserves the form and sets a clone-local error", async () => {
  const state = await run(async () => { throw new Error("Fish Audio: Title contains sensitive words"); });
  assert.match(state.cloneUploadError, /Title contains sensitive words/);
  assert.equal(state.clonePath, "/recording.wav");
  assert.equal(state.cloneUploadSuccess, "");
  assert.equal(state.accountBusy, false);
});

test("successful upload clears the form and confirms creation even if list refresh fails", async () => {
  const state = await run(async command => {
    if (command === "desktop_upload_clone") return { id: "clone-1", name: "My voice", status: "training", createdAt: "2026-09-30T00:00:00Z" };
    throw new Error("List unavailable");
  });
  assert.equal(state.clonePath, "");
  assert.equal(state.cloneName, "");
  assert.equal(state.cloneConsent, false);
  assert.match(state.cloneUploadSuccess, /My voice/);
  assert.equal(state.hostedClones[0].id, "clone-1");
  assert.equal(state.cloneUploadError, "");
  assert.equal(state.accountBusy, false);
});
