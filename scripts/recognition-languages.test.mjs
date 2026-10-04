import assert from 'node:assert/strict';
import test from 'node:test';
import { loadTestModule } from './load-test-module.mjs';
const { groupRecognitionLanguages } = await loadTestModule(new URL('../src/recognitionLanguages.ts', import.meta.url));

test('regional variants are grouped by language without dropping or changing locales', () => {
  const languages = [{code:'fr-CA', name:'French (Canada)'}, {code:'en-US', name:'English (US)'}, {code:'fr-FR', name:'French (France)'}, {code:'en-GB', name:'English (UK)'}];
  const groups = groupRecognitionLanguages(languages);
  assert.deepEqual(groups.map(group => group.name), ['English', 'French']);
  assert.deepEqual(groups[1].languages.map(language => language.code), ['fr-CA', 'fr-FR']);
  assert.equal(languages[0].code, 'fr-CA');
});
