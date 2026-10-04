import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { loadTestModule } from './load-test-module.mjs';

const { default: Grip } = await loadTestModule(new URL('../src/OverlayGrip.tsx', import.meta.url));
const flush = () => new Promise(resolve => setImmediate(resolve));
function setup(invoke) {
  globalThis.window = { __TAURI_INTERNALS__: { metadata: { currentWindow: { label: 'assistant' } }, invoke } };
  return Grip({}).props.children[1].props;
}
test('the move handle starts native dragging only for the primary mouse button', async () => {
  const calls = [];
  const props = setup(async (command, args) => calls.push({command, args}));
  props.onMouseDown({button: 2, preventDefault() { assert.fail('Secondary click must pass through'); }});
  props.onMouseDown({button: 0, preventDefault() {}});
  await flush();
  assert.deepEqual(calls, [{command: 'plugin:window|start_dragging', args: {label: 'assistant'}}]);
  delete globalThis.window;
});
test('arrow keys move the widget by logical points at the current display scale', async () => {
  const calls = [];
  const props = setup(async (command, args) => {
    calls.push({command, args});
    if (command.endsWith('|outer_position')) return {x: 100, y: 200};
    if (command.endsWith('|scale_factor')) return 2;
  });
  props.onKeyDown({key: 'Enter', preventDefault() { assert.fail('Other keys must pass through'); }});
  props.onKeyDown({key: 'ArrowRight', shiftKey: false, preventDefault() {}});
  await flush();
  const position = calls.find(call => call.command.endsWith('|set_position')).args.value;
  assert.deepEqual(JSON.parse(JSON.stringify(position)), {Physical: {x: 120, y: 200}});
  calls.length = 0;
  props.onKeyDown({key: 'ArrowUp', shiftKey: true, preventDefault() {}});
  await flush();
  assert.deepEqual(JSON.parse(JSON.stringify(calls.find(call => call.command.endsWith('|set_position')).args.value)), {Physical: {x: 100, y: 100}});
  delete globalThis.window;
});
test('both floating windows permit the move controls', () => {
  for (const name of ['assistant', 'speech-overlay']) {
    const capability = JSON.parse(readFileSync(new URL(`../src-tauri/capabilities/${name}.json`, import.meta.url)));
    assert.ok(capability.permissions.includes('core:window:allow-start-dragging'));
    assert.ok(capability.permissions.includes('core:window:allow-set-position'));
  }
});
