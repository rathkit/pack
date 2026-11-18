import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const files = readdirSync(out).filter((f) => f.endsWith('.js'));
  const shared = files.filter((f) => f.startsWith('shared-'));
  if (shared.length !== 1 || !readFileSync(join(out, shared[0]), 'utf8').includes('FN-TEST-PKG')) return `the function test should make one shared-*.js with vendor/util.js; files: ${files}`;
  const lazy = files.filter((f) => f.startsWith('lazy-'));
  if (lazy.length !== 1 || !readFileSync(join(out, lazy[0]), 'utf8').includes('lazy')) return `lazy.js should keep its own chunk; files: ${files}`;
  return null;
};
