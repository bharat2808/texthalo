import assert from 'node:assert/strict';
import test from 'node:test';
import { loadTestModule } from './load-test-module.mjs';

const { watchNativeHover } = await loadTestModule(new URL('../src/nativeHover.ts', import.meta.url));
const button = () => ({ isConnected: true, getBoundingClientRect: () => ({ left: 10, top: 20, right: 32, bottom: 42 }) });
function setup(t, invoke) {
  globalThis.isTauri = true;
  globalThis.window = { __TAURI_INTERNALS__: { metadata: { currentWindow: { label: 'assistant' } }, invoke } };
  t.after(() => { delete globalThis.isTauri; delete globalThis.window; });
}
test('native hover works without DOM pointer events on a scaled, offset display', async t => {
  let cursor = { x: 0, y: 0 };
  let entered = 0;
  const calls = [];
  setup(t, async (command, args) => {
    calls.push({command, args});
    if (command.endsWith('|cursor_position')) return cursor;
    if (command.endsWith('|inner_position')) return {x: -300, y: 40};
    if (command.endsWith('|scale_factor')) return 2;
    throw Error(command);
  });
  let resolveEntry;
  const entry = new Promise(resolve => { resolveEntry = resolve; });
  const stop = watchNativeHover(button(), () => { entered++; resolveEntry(); });
  t.after(stop);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(entered, 0, 'Pointer outside the icon does not expand the widget');
  cursor = {x: -260, y: 100};
  await Promise.race([entry, new Promise((_, reject) => {
    const timer = setTimeout(() => reject(Error('Native hover never fired')), 1500);
    timer.unref();
  })]);
  assert.equal(entered, 1);
  assert.ok(calls.some(call => call.command.endsWith('|inner_position') && call.args.label === 'assistant'));
  stop();
});
test('a cancelled preview ignores a late native cursor response', async t => {
  let resolveCursor;
  const cursor = new Promise(resolve => { resolveCursor = resolve; });
  setup(t, async command => {
    if (command.endsWith('|cursor_position')) return cursor;
    if (command.endsWith('|inner_position')) return {x: 0, y: 0};
    if (command.endsWith('|scale_factor')) return 1;
  });
  let entered = false;
  const stop = watchNativeHover(button(), () => { entered = true; });
  stop();
  resolveCursor({x: 20, y: 30});
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(entered, false);
});
test('a preview appearing under a stationary pointer stays collapsed', async t => {
  let cursor = {x: 20, y: 30};
  setup(t, async command => {
    if (command.endsWith('|cursor_position')) return cursor;
    if (command.endsWith('|inner_position')) return {x: 0, y: 0};
    if (command.endsWith('|scale_factor')) return 1;
  });
  let entries = 0;
  let resolveEntry;
  const entry = new Promise(resolve => { resolveEntry = resolve; });
  const stop = watchNativeHover(button(), () => { entries++; resolveEntry(); });
  t.after(stop);
  await new Promise(resolve => setTimeout(resolve, 160));
  assert.equal(entries, 0, 'Initial and repeated stationary samples must not expand');
  cursor = {x: 21, y: 30};
  await Promise.race([entry, new Promise((_, reject) => {
    const timer = setTimeout(() => reject(Error('Deliberate pointer movement never expanded')), 1500);
    timer.unref();
  })]);
  assert.equal(entries, 1);
});
test('native hover reports leaving the widget without DOM pointer events', async t => {
  let cursor = {x: 20, y: 30};
  setup(t, async command => {
    if (command.endsWith('|cursor_position')) return cursor;
    if (command.endsWith('|inner_position')) return {x: 0, y: 0};
    if (command.endsWith('|scale_factor')) return 1;
  });
  let leaves = 0;
  let resolveLeave;
  const left = new Promise(resolve => { resolveLeave = resolve; });
  const stop = watchNativeHover(button(), () => {}, () => { leaves++; resolveLeave(); });
  t.after(stop);
  await new Promise(resolve => setTimeout(resolve, 160));
  assert.equal(leaves, 0, 'Remaining over expanded text keeps the widget open');
  cursor = {x: 100, y: 100};
  await Promise.race([left, new Promise((_, reject) => {
    const timer = setTimeout(() => reject(Error('Native pointer leave never fired')), 1500);
    timer.unref();
  })]);
  await new Promise(resolve => setTimeout(resolve, 160));
  assert.equal(leaves, 1, 'Leaving collapses once, even while the pointer stays outside');
});
