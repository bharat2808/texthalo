import assert from 'node:assert/strict';
import test from 'node:test';
import { loadTestModule } from './load-test-module.mjs';

const { serviceReadiness } = await loadTestModule(new URL('../src/serviceReadiness.ts', import.meta.url));
const ready = () => ({
  availability: { model: 'unavailable', microphone: 3, recognition: 3, imageInput: false,
    cloudConfigured: true, cloudSignedIn: true },
  error: '', trusted: true, focusedPasswordField: false,
  shortcuts: { speak: 'Cmd+S', transcribe: 'Hold:Fn', assistant: 'Hold:RightOption' },
  refused: [], preferences: { language: 'en-US', instruction: '', dictation_output: 'insert', provider: 'cloud', cloud_consent_version: 1 },
  engine: { can_speak: true, status: 'Ready' },
});
test('cloud voice Assistant works without Apple Intelligence', () => {
  assert.deepEqual(serviceReadiness(ready()).assistant, []);
});
test('another app enabling global Secure Input does not mark features unavailable', () => {
  const input = ready(); input.secureInput = true;
  const readiness = serviceReadiness(input);
  for (const feature of ['voice', 'transcription', 'assistant']) {
    assert.deepEqual(readiness[feature], []);
  }
});
test('a focused password field shows the contextual warning', () => {
  const input = ready(); input.focusedPasswordField = true;
  const readiness = serviceReadiness(input);
  for (const feature of ['voice', 'transcription', 'assistant']) {
    assert.deepEqual(readiness[feature], ['Leave the password field to use shortcuts.']);
  }
});
test('cloud still requires voice recognition and microphone access', () => {
  const input = ready(); input.availability.microphone = 2; input.availability.recognition = 1;
  const reasons = serviceReadiness(input).assistant;
  assert.ok(reasons.some(value => value.includes('Microphone')));
  assert.ok(reasons.some(value => value.includes('Speech Recognition')));
});
test('cloud does not become ready without consent, sign-in or configured service', () => {
  const input = ready(); input.preferences.cloud_consent_version = null;
  input.availability.cloudSignedIn = false; input.availability.cloudConfigured = false;
  const reasons = serviceReadiness(input).assistant;
  assert.ok(reasons.some(value => value.includes('cloud processing')));
  assert.ok(reasons.some(value => value.includes('Sign in')));
  assert.ok(reasons.some(value => value.includes('configured')));
});
test('automatic chooses available local model without cloud prerequisites', () => {
  const input = ready(); input.preferences.provider = 'automatic'; input.availability.model = 'available';
  input.preferences.cloud_consent_version = null; input.availability.cloudSignedIn = false;
  assert.deepEqual(serviceReadiness(input).assistant, []);
});
test('legacy preferences remain local and require Apple Intelligence', () => {
  const input = ready(); delete input.preferences.provider; delete input.preferences.cloud_consent_version;
  assert.ok(serviceReadiness(input).assistant.some(value => value.includes('Apple Intelligence')));
});
test('cloud screen capability is independent of local model screen gating', () => {
  const input = ready(); input.availability.cloudScreenInput = true; input.availability.screen = false;
  assert.ok(serviceReadiness(input).assistant.some(value => value.includes('Screen Recording')));
  input.availability.screen = true;
  assert.deepEqual(serviceReadiness(input).assistant, []);
});
test('Whisper needs its model and microphone, without Speech Recognition permission', () => {
  const input = ready(); input.preferences.transcription_engine = 'whisper';
  input.availability.recognition = 1; input.availability.whisperReady = true;
  assert.deepEqual(serviceReadiness(input).transcription, []);
  assert.deepEqual(serviceReadiness(input).assistant, []);
  input.availability.whisperReady = false;
  assert.ok(serviceReadiness(input).transcription.some(reason => reason.includes('Download')));
  input.availability.whisperReady = true; input.availability.microphone = 2;
  assert.ok(serviceReadiness(input).transcription.some(reason => reason.includes('Microphone')));
});
