import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('Whisper resamples every microphone frame, flushes the end, and rejects canceled frames', {skip:process.platform !== 'darwin'}, () => {
  const directory = mkdtempSync(join(tmpdir(), 'texthalo-whisper-capture-'));
  try {
    const binary = join(directory, 'capture-test');
    const compiled = spawnSync('xcrun', ['swiftc', '-parse-as-library', 'src-tauri/native/SpeechSupport.swift', 'src-tauri/native/WhisperCaptureTests.swift', '-o', binary], {encoding:'utf8',timeout:60000});
    assert.equal(compiled.status, 0, compiled.stderr);
    const result = spawnSync(binary, [], {encoding:'utf8',timeout:10000});
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /whisper capture passed/);
  } finally { rmSync(directory, {recursive:true,force:true}); }
});
