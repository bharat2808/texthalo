// Snapshot third-party metadata and notices from the locked dependency graph.
// TextHalo's own package/license is deliberately excluded.
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir, readdir, copyFile, rm } from 'node:fs/promises';
import { dirname, join, relative, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { build } from 'vite';
const root = resolve('.');
const output = join(root, 'website/public/licenses/dependencies');
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml', '--filter-platform', 'aarch64-apple-darwin'], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
function reachable(normalOnly) {
  const visited = new Set();
  function visit(id) {
    if (visited.has(id)) return;
    visited.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      if (!normalOnly || dep.dep_kinds.some(kind => kind.kind === null)) visit(dep.pkg);
    }
  }
  visit(metadata.resolve.root);
  return visited;
}
const runtime = reachable(true);
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
const entries = [];
async function notices(directory, slug) {
  const files = [];
  async function scan(folder, depth) {
    let children;
    try { children = await readdir(folder, { withFileTypes: true }); } catch { return; }
    for (const child of children.sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(folder, child.name);
      if (child.isFile() && /^(licen[cs]e|copying|copyright|notice)([._-]|$)/i.test(child.name)) files.push(path);
      else if (child.isDirectory() && depth < 2 && /^(licen[cs]es?|whisper\.cpp|vendor|third.party)$/i.test(child.name)) await scan(path, depth + 1);
    }
  }
  await scan(directory, 0);
  const result = [];
  for (const file of files) {
    const name = relative(directory, file).replaceAll('/', '-');
    const target = join(output, slug, `${name}.txt`);
    await mkdir(dirname(target), { recursive: true });
    await copyFile(file, target);
    result.push({ name: relative(directory, file), href: `/licenses/dependencies/${slug}/${name}.txt` });
  }
  return result;
}
for (const pkg of metadata.packages.filter(pkg => pkg.source).sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))) {
  const slug = `rust-${pkg.name}-${pkg.version}`;
  entries.push({ name: pkg.name, version: pkg.version, license: pkg.license || 'See upstream license', ecosystem: 'Rust', scope: runtime.has(pkg.id) ? 'runtime' : 'build', url: pkg.repository || `https://crates.io/crates/${pkg.name}/${pkg.version}`, notices: await notices(dirname(pkg.manifest_path), slug) });
}
const lock = JSON.parse(await readFile('package-lock.json', 'utf8'));
// Installed optional peers are not necessarily shipped. Inspect actual emitted modules.
const emitted = new Set();
for (const configFile of ['vite.config.ts', 'vite.website.config.ts']) {
  const result = await build({ configFile, logLevel: 'silent', build: { write: false } });
  for (const bundle of Array.isArray(result) ? result : [result]) {
    for (const chunk of bundle.output.filter(item => item.type === 'chunk')) {
      for (const [id, module] of Object.entries(chunk.modules)) if (module.renderedLength > 0) emitted.add(id);
    }
  }
}
const shipped = path => [...emitted].some(id => id.startsWith(`${resolve(path)}/`));
const upstreamLicenses = {
  '@triplit/db': { license: 'AGPL-3.0', revision: 'd119bc39ed0ca5dd99c149fb5bd54825dc2899a0', notice: 'triplit-db-AGPL.txt' },
  '@triplit/react': { license: 'AGPL-3.0', revision: 'd119bc39ed0ca5dd99c149fb5bd54825dc2899a0', notice: 'triplit-react-AGPL.txt' },
  '@triplit/logger': { license: 'AGPL-3.0', revision: 'd969ac61f5a2d666f9fc1b7284f253270bf4a6e0', notice: 'triplit-logger-AGPL.txt' },
};
const npm = new Map();
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path) continue;
  const name = path.split('node_modules/').at(-1);
  const id = `${name}@${pkg.version}`;
  const existing = npm.get(id);
  if (existing) { if (shipped(path)) existing.scope = 'runtime'; continue; }
  const slug = `npm-${name.replaceAll('/', '-')}-${pkg.version}-${createHash('sha256').update(id).digest('hex').slice(0, 8)}`;
  let installed = {};
  try { installed = JSON.parse(await readFile(join(path, 'package.json'), 'utf8')); } catch { /* Platform-only optional packages may not be installed. */ }
  npm.set(id, { name, version: pkg.version, license: pkg.license || installed.license || upstreamLicenses[name]?.license || 'See upstream license', ecosystem: 'npm', scope: shipped(path) ? 'runtime' : 'build', url: upstreamLicenses[name] ? `https://github.com/aspen-cloud/triplit/tree/${upstreamLicenses[name].revision}/packages/${name.split('/')[1]}` : `https://www.npmjs.com/package/${name}/v/${pkg.version}`, notices: upstreamLicenses[name] ? [{ name: 'Upstream LICENSE', href: `/licenses/attributions/${upstreamLicenses[name].notice}` }] : await notices(resolve(path), slug) });
}
entries.push(...[...npm.values()].sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`)));
const inventory = { version: lock.version, entries };
await writeFile('website/src/third-party-licenses.json', `${JSON.stringify(inventory, null, 2)}\n`);
await writeFile('website/public/licenses/dependencies.json', `${JSON.stringify(inventory, null, 2)}\n`);
console.log(`Recorded ${entries.length} locked third-party dependencies and their available license/notice files.`);
