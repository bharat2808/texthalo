import assert from 'node:assert/strict';
import test from 'node:test';
import { loadTestModule } from './load-test-module.mjs';
const localization = await loadTestModule(new URL('../src/localization.ts', import.meta.url));

test('automatic language follows preferred languages, including regional variants', () => {
  assert.equal(localization.resolveLanguage('system', ['fr-CA', 'en-CA']), 'fr-CA');
  assert.equal(localization.resolveLanguage('system', ['es-MX']), 'es-419');
  assert.equal(localization.resolveLanguage('system', ['fr_FR']), 'fr-CA');
  assert.equal(localization.resolveLanguage('system', ['de-DE', 'es-US']), 'es-419');
  assert.equal(localization.resolveLanguage('system', ['de-DE']), 'en');
  assert.equal(localization.resolveLanguage('system', []), 'en');
});
test('manual language overrides the Mac and unsupported saved values fall back safely', () => {
  assert.equal(localization.resolveLanguage('en', ['fr-CA']), 'en');
  assert.equal(localization.resolveLanguage('fr-CA', ['es-MX']), 'fr-CA');
  assert.equal(localization.resolveLanguage('es-419', ['en-US']), 'es-419');
  assert.equal(localization.resolveLanguage('unknown', ['fr-CA']), 'fr-CA');
});
test('translation interpolates values without translating user content', () => {
  assert.equal(localization.translate('Settings', 'fr-CA'), 'Paramètres');
  assert.equal(localization.translate('Settings', 'es-419'), 'Configuración');
  assert.equal(localization.translate('Preview {0}', 'fr-CA', ['My English voice']), 'Écouter My English voice');
  assert.equal(localization.translate('unrecognized technical detail', 'fr-CA'), 'unrecognized technical detail');
  assert.equal(localization.translate('Speaking {0} characters', 'es-419', [42]), 'Leyendo 42 caracteres');
});
test('catalogs preserve all interpolation parameters and have complete matching coverage', () => {
  const { catalogs } = localization;
  assert.ok(Object.keys(catalogs['fr-CA']).length > 400);
  assert.deepEqual(Object.keys(catalogs['fr-CA']).sort(), Object.keys(catalogs['es-419']).sort());
  for (const [language, catalog] of Object.entries(catalogs)) {
    for (const [source, target] of Object.entries(catalog)) {
      assert.ok(target.trim(), `${language}: empty translation for ${source}`);
      assert.deepEqual(target.match(/\{\d+\}/g)?.sort() ?? [], source.match(/\{\d+\}/g)?.sort() ?? [], `${language}: ${source}`);
    }
  }
});
test('native messages translate parameters while preserving diagnostic details', () => {
  assert.equal(localization.translateMessage('Could not start microphone: device busy', 'fr-CA'), 'Impossible de démarrer le microphone : device busy');
  assert.equal(localization.translateMessage('Listening… Release to transcribe.', 'es-419'), 'Escuchando… Suelta para transcribir.');
  assert.equal(localization.translateMessage('unknown: diagnostic', 'fr-CA'), 'unknown: diagnostic');
});
test('unknown prototype keys and user names remain verbatim', () => {
  assert.equal(localization.translate('constructor', 'fr-CA'), 'constructor');
  assert.equal(localization.translateMessage('toString', 'es-419'), 'toString');
  assert.equal(localization.translateMessage('“Ready” was uploaded and saved. Ready to select in Voice → My hosted clones.', 'fr-CA'), '« Ready » a été téléversée et enregistrée. Prête à sélectionner dans Voix → Mes voix clonées hébergées.');
});
test('audit phases use localized display labels and unknown phases remain intact', () => {
  assert.equal(localization.translatePhase('result', 'fr-CA'), 'Résultat');
  assert.equal(localization.translatePhase('streaming', 'es-419'), 'Generando');
  assert.equal(localization.translatePhase('capturing_screen', 'fr-CA'), 'Capture de l’écran');
  assert.equal(localization.translatePhase('transcript', 'es-419'), 'Transcripción');
  assert.equal(localization.translatePhase('future_phase', 'fr-CA'), 'future_phase');
});
