import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';

test('published third-party inventory covers npm locks and resolves every copied notice', async () => {
  const inventory = JSON.parse(await readFile('website/src/third-party-licenses.json', 'utf8'));
  const lock = JSON.parse(await readFile('package-lock.json', 'utf8'));
  assert.equal(inventory.version, lock.version);
  assert.deepEqual(inventory, JSON.parse(await readFile('website/public/licenses/dependencies.json', 'utf8')));
  const indexed = new Set(inventory.entries.map(e => `${e.ecosystem}:${e.name}@${e.version}`));
  assert.equal(indexed.size, inventory.entries.length);
  for (const [path, pkg] of Object.entries(lock.packages)) {
    if (path) assert.ok(indexed.has(`npm:${path.split('node_modules/').at(-1)}@${pkg.version}`), path);
  }
  for (const entry of inventory.entries) {
    assert.ok(!['kiegen', 'texthalo'].includes(entry.name));
    assert.notEqual(entry.license, 'See upstream license', entry.name);
    assert.ok(['runtime', 'build'].includes(entry.scope));
    for (const notice of entry.notices) {
      assert.match(notice.href, /^\/licenses\/(dependencies|attributions)\//);
      assert.ok((await readFile(`website/public${notice.href}`, 'utf8')).trim(), notice.href);
    }
  }
  for (const name of ['@triplit/db', '@triplit/react', '@triplit/logger']) {
    const entry = inventory.entries.find(e => e.name === name);
    assert.equal(entry.license, 'AGPL-3.0');
    assert.equal(entry.scope, 'build', `${name} is an unused optional peer, not shipped in either bundle`);
  }
  for (const name of ['react', 'react-dom', 'jpreprocess', 'lindera', 'jieba-rs']) {
    assert.ok(inventory.entries.some(e => e.name === name && e.scope === 'runtime'), name);
  }
});
