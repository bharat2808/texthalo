import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

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
