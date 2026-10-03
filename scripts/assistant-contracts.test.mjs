import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("native hold events preserve taps, combinations, and release lifecycle", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-holds-"));
  try {
    const binary = join(directory, "holds");
    const compiled = spawnSync("xcrun", ["swiftc", "src-tauri/native/Contracts.swift", "src-tauri/native/HoldShortcuts.swift", "src-tauri/native/HoldShortcutTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:10000});
    assert.equal(result.status, 0, result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});

test("native insertion, hold-release, and scoped Undo contracts", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-contracts-"));
  try {
    const binary = join(directory, "contracts");
    const compiled = spawnSync("xcrun", ["swiftc", "src-tauri/native/Contracts.swift", "src-tauri/native/ContractTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:10000});
    assert.equal(result.status, 0, result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});


test("development app declares the same privacy purposes as the release app", {skip:process.platform !== "darwin"}, () => {
  const readPlist = (path) => {
    const result = spawnSync("/usr/bin/plutil", ["-convert", "json", "-o", "-", path], {encoding:"utf8"});
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const dev = readPlist("src-tauri/DevInfo.plist");
  const release = readPlist("src-tauri/Info.plist");
  assert.equal(dev.CFBundleExecutable, "kiegen");
  const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
  assert.equal(dev.CFBundleIdentifier, config.identifier);
  assert.equal(dev.CFBundleName, config.productName);
  assert.equal(dev.CFBundleDisplayName, config.productName);
  for (const key of ["NSMicrophoneUsageDescription", "NSSpeechRecognitionUsageDescription"]) {
    assert.ok(dev[key]?.trim());
    assert.equal(dev[key], release[key]);
  }
});


test("screen OCR reads UI text and honors cancellation", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-screen-tests-"));
  try {
    const binary = join(directory, "screen-tests");
    const compiled = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-target", "arm64-apple-macos14.0", "src-tauri/native/ScreenText.swift", "src-tauri/native/ScreenTextTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:15000});
    assert.equal(result.status, 0, result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});
