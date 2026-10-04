import assert from 'node:assert/strict';
import test from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { loadTestModule } from './load-test-module.mjs';

const { default: Panel } = await loadTestModule(new URL('../src/SpeechErrorPanel.tsx', import.meta.url));
const base = { assistant: true, message: 'Network interrupted', actionError: null, busy: false,
  actionLabel: 'Assistant settings', dismiss() {}, recover() {} };
const render = props => renderToStaticMarkup(createElement(Panel, {...base,...props}));

test('partial Assistant errors retain full selectable text, Copy and scoped Undo', () => {
  const text = 'Partial <answer>\n' + 'x'.repeat(5000);
  const html = render({textRecovery:{text,canUndo:true,copy() {},undo() {}}});
  assert.match(html,/with-recovery/);
  assert.match(html,/aria-label="Recoverable Assistant text" tabindex="0"/);
  assert.ok(html.includes('Partial &lt;answer&gt;\n' + 'x'.repeat(5000)));
  assert.match(html,/>Copy<\/button>/);
  assert.match(html,/>Undo<\/button>/);
  assert.match(html,/>Assistant settings<\/button>/);
});
test('empty Assistant and voice errors retain the compact recovery widget', () => {
  const html = render({});
  assert.doesNotMatch(html,/with-recovery|>Copy<|>Undo</);
  assert.match(html,/Unable to answer/);
  const voice = render({assistant:false,actionLabel:'Voice settings',useLocal() {}});
  assert.match(voice,/Unable to speak/);
  assert.match(voice,/>Use local voice<\/button>/);
  assert.doesNotMatch(voice,/Recoverable Assistant text/);
});
test('Undo requires native availability, and pending actions disable recovery controls', () => {
  const html = render({busy:true,textRecovery:{text:'Keep this',canUndo:false,copy() {},undo() {}}});
  assert.doesNotMatch(html,/>Undo</);
  assert.match(html,/disabled=""[^>]*>Copy<\/button>/);
  assert.match(html,/disabled=""[^>]*>Assistant settings<\/button>/);
});
test('Copy and Undo invoke distinct recovery callbacks', () => {
  const calls = [];
  const tree = Panel({...base,textRecovery:{text:'Keep this',canUndo:true,copy:() => calls.push('copy'),undo:() => calls.push('undo')}});
  const buttons = [];
  const walk = node => {
    if (!node || typeof node !== 'object') return;
    if (node.type === 'button') buttons.push(node);
    for (const child of [node.props?.children].flat(Infinity)) walk(child);
  };
  walk(tree);
  buttons.find(button => button.props.children === 'Copy').props.onClick();
  buttons.find(button => button.props.children === 'Undo').props.onClick();
  assert.deepEqual(calls,['copy','undo']);
});
