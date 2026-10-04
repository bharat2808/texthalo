import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import react from '@vitejs/plugin-react';

/** Load the real TS/TSX graph without unresolved relative imports in a data URL. */
export async function loadTestModule(entry) {
  const result = await build({ configFile: false, logLevel: 'silent', plugins: [react()], build: {
    write: false, minify: false, lib: { entry: fileURLToPath(entry), formats: ['es'] },
  } });
  const output = Array.isArray(result) ? result.flatMap(bundle => bundle.output) : result.output;
  const compiled = output.find(chunk => chunk.type === 'chunk').code;
  return import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
}
