import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("generation history retains 50 complete private audit records", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-audit-tests-"));
  try {
    const binary = join(directory, "audit");
    const compiled = spawnSync("xcrun", ["swiftc", "-parse-as-library", "src-tauri/native/GenerationHistory.swift", "src-tauri/native/GenerationHistoryTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:15000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});

test("native hold events preserve taps, combinations, and release lifecycle", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-holds-"));
  try {
    const binary = join(directory, "holds");
    const compiled = spawnSync("xcrun", ["swiftc", "src-tauri/native/Contracts.swift", "src-tauri/native/HoldShortcuts.swift", "src-tauri/native/HoldShortcutTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:10000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});

test("native insertion, hold-release, and scoped Undo contracts", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-contracts-"));
  try {
    const binary = join(directory, "contracts");
    const compiled = spawnSync("xcrun", ["swiftc", "src-tauri/native/Contracts.swift", "src-tauri/native/ContractTests.swift", "-o", binary], {encoding:"utf8",timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:10000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});


test("development app declares the same privacy purposes as the release app", {skip:process.platform !== "darwin"}, () => {
  const readPlist = (path) => {
    const result = spawnSync("/usr/bin/plutil", ["-convert", "json", "-o", "-", path], {encoding:"utf8"});
    assert.equal(result.status, 0, result.stdout + result.stderr);
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


test("single-stream generation strips model-owned action markers", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-generation-tests-"));
  try {
    const binary = join(directory, "generation-tests");
    const localModel = process.env.TEXTHALO_TEST_LOCAL_MODEL === "1";
    const cltSwift = "/Library/Developer/CommandLineTools/usr/bin/swiftc";
    const cltSdk = "/Library/Developer/CommandLineTools/SDKs/MacOSX27.0.sdk";
    const useImageSdk = localModel && existsSync(cltSwift) && existsSync(cltSdk);
    const compiler = useImageSdk ? cltSwift : "xcrun";
    const args = [...(useImageSdk ? [] : ["swiftc"]), "-parse-as-library", "-target", "arm64-apple-macos14.0",
      ...(useImageSdk ? ["-sdk", cltSdk, "-D", "FOUNDATION_MODELS_IMAGE_ATTACHMENTS"] : []),
      "src-tauri/native/Contracts.swift", "src-tauri/native/ScreenText.swift", "src-tauri/native/AssistantGeneration.swift", "src-tauri/native/AssistantGenerationTests.swift", "-o", binary];
    const compiled = spawnSync(compiler, args, {encoding:"utf8",timeout:useImageSdk ? 180000 : 60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8",timeout:process.env.TEXTHALO_TEST_LOCAL_MODEL === "1" ? 120000 : 15000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});


test("native generation errors retain partial output for Copy and scoped Undo", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-events-"));
  try {
    // Compile the real Assistant class with this test's entry point; do not run
    // its application/event-tap main while checking emitted protocol payloads.
    const source = join(directory, "Assistant.swift");
    writeFileSync(source, readFileSync("src-tauri/native/Assistant.swift", "utf8").split("@main struct Main {")[0]);
    const main = join(directory, "Events.swift");
    writeFileSync(main, `import CoreGraphics
import Foundation
@main struct Events {
  @MainActor static func main() {
    let assistant = Assistant()
    assistant.id = 7
    assistant.result = "A partial answer."
    assistant.event("error", "Generation timed out.")
    assistant.event("error", "Generation failed.")
    assistant.event("error", "Specific answer", text: "Explicit output")
    assistant.event("idle")
    assistant.insertResult()
    assistant.insertion?.cancel()
    let ownedTarget = Target()
    assistant.target = ownedTarget
    let generator = Task<Void, Never> { try? await Task.sleep(for: .seconds(60)) }
    let inserter = Task<Void, Never> { try? await Task.sleep(for: .seconds(60)) }
    assistant.generation = generator
    assistant.insertion = inserter
    assistant.recording = true
    assistant.finishing = true
    let image = CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!.makeImage()!
    assistant.screenPreparation = PreparedScreenContext(requestID: 7, windowID: 42) { _ in image }
    assistant.handle(Request(command: "halt", id: 8))
    assert(assistant.generation != nil && assistant.result == "A partial answer.")
    assistant.handle(Request(command: "halt", id: 7))
    assert(assistant.generation == nil && assistant.insertion == nil && assistant.screenPreparation == nil)
    assert(generator.isCancelled && inserter.isCancelled)
    assert(!assistant.recording && !assistant.finishing)
    assert(assistant.target === ownedTarget)
    assert(assistant.id == 7 && assistant.result == "A partial answer.")
  }
}`);
    const binary = join(directory, "events");
    const sources = ["Contracts", "HoldShortcuts", "SpeechSupport", "ScreenCapture", "ScreenText", "GenerationHistory", "AssistantGeneration"].map(name => `src-tauri/native/${name}.swift`);
    const compiled = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-target", "arm64-apple-macos14.0", ...sources, source, main, "-o", binary], {encoding:"utf8", timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8", timeout:10000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const events = result.stdout.trim().split("\n").map(line => JSON.parse(line));
    assert.deepEqual(events.map(event => event.text), ["A partial answer.", "A partial answer.", "Explicit output", "", "A partial answer."]);
    assert.equal(events.at(-1).message, "Inserting text…");
    assert.ok(events.every(event => event.id === 7));
  } finally { rmSync(directory, {recursive:true,force:true}); }
});

test("cloud native fulfillment rejects late and malformed frames and bypasses local generation", {skip:process.platform !== "darwin"}, () => {
  const directory = mkdtempSync(join(tmpdir(), "texthalo-cloud-native-"));
  try {
    const source = join(directory, "Assistant.swift");
    writeFileSync(source, readFileSync("src-tauri/native/Assistant.swift", "utf8").split("@main struct Main {")[0]);
    const binary = join(directory, "cloud");
    const sources = ["Contracts", "HoldShortcuts", "SpeechSupport", "ScreenCapture", "ScreenText", "GenerationHistory", "AssistantGeneration"].map(name => `src-tauri/native/${name}.swift`);
    const compiled = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-target", "arm64-apple-macos14.0", ...sources, source, "src-tauri/native/HostedAssistantTests.swift", "-o", binary], {encoding:"utf8", timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:"utf8", timeout:10000});
    assert.equal(result.status, 0, result.stdout + result.stderr);
    const events = result.stdout.trim().split("\n").map(line => JSON.parse(line));
    const input = events.find(event => event.phase === "generation_input");
    assert.ok(input, "cloud dispatch must emit generation_input without running FoundationModels");
    assert.equal(input.request, "Write a greeting");
    assert.equal(input.preferences, "Brief");
    assert.equal(input.previousOutput, "Earlier answer");
    assert.equal(input.screenText, "");
    assert.deepEqual(input.source, {app:"", windowTitle:"", focusedRole:"", focusedLabel:""});
    assert.equal(input.selection, "");
    assert.equal(events.filter(event => event.phase === "result" && event.action === "clarify").length, 1);
    assert.equal(events.filter(event => event.phase === "error").length, 3);
    assert.equal(typeof input.auditId, "string");
    assert.ok(!events.some(event => event.text === "Wrong" || event.text === "Late"));
  } finally { rmSync(directory, {recursive:true,force:true}); }
});
