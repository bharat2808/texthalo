import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/assistantUsage.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { assistantUsageLabel } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
test('pending accounting never presents placeholder zero as a settled charge', () => {
  assert.equal(assistantUsageLabel({ billingStatus: 'reserved', chargedCredits: 0 }), 'Checking generation usage…');
  assert.equal(assistantUsageLabel({ billingStatus: 'pending', chargedCredits: 0 }), 'Checking generation usage…');
});
test('reservation and settled usage have distinct labels', () => {
  assert.equal(assistantUsageLabel({ billingStatus: 'reserved', reservedCredits: 1000 }), '1,000 credits reserved');
  assert.equal(assistantUsageLabel({ billingStatus: 'settled', chargedCredits: 12 }), '12 credits for generation');
  assert.equal(assistantUsageLabel({ billingStatus: 'released', chargedCredits: 0 }), '0 credits for generation');
});
