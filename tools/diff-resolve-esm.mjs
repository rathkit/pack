// Compares the resolver's `import` resolution of bare specifiers with Node's
// `import.meta.resolve` (run with RESOLVE_KIND=import for the dump).
//   node --experimental-import-meta-resolve tools/diff-resolve-esm.mjs dump.tsv
import { readFileSync, realpathSync } from 'node:fs';
import { pathToFileURL, fileURLToPath } from 'node:url';

const rows = readFileSync(process.argv[2], 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
const counts = { same: 0, bothFail: 0, differ: 0, looser: 0, onlyNodeFails: 0, onlyWeFail: 0 };
const shown = { differ: 0, onlyNodeFails: 0, onlyWeFail: 0 };
for (const [file, specifier, ours] of rows) {
  if (/^[./]/.test(specifier)) continue; // relative ESM paths need full file names; bundlers are looser
  let node = null;
  try {
    node = realpathSync(fileURLToPath(import.meta.resolve(specifier, pathToFileURL(file).href)));
  } catch {}
  const weFail = ours.startsWith('ERROR');
  let kind;
  if (node === null && weFail) kind = 'bothFail';
  else if (node === null) kind = 'onlyNodeFails';
  else if (weFail) kind = 'onlyWeFail';
  // Node's ESM resolver never probes extensions or directory indexes; a bundler does, so
  // finding `node + .js` or `node/index.js` where Node stops at `node` is expected.
  else kind = node === ours ? 'same' : /^(\.[a-z]+|\/index\.[a-z]+)$/.test(ours.slice(node.length)) && ours.startsWith(node) ? 'looser' : 'differ';
  counts[kind]++;
  if (kind === 'onlyNodeFails') {
    // With `exports` present Node's refusal is the rule, and we must refuse too; without it,
    // Node refuses extensionless subpaths that a bundler accepts.
    const m = /^(@[^/]+\/[^/]+|[^/]+)(\/.*)?$/.exec(specifier);
    const pkg = ours.slice(0, ours.lastIndexOf('node_modules/') + 13) + m[1] + '/package.json';
    let hasExports = false;
    try { hasExports = 'exports' in JSON.parse(readFileSync(pkg, 'utf8')); } catch {}
    if (!hasExports) { counts.onlyNodeFails--; counts.looser++; continue; }
  }
  if (kind !== 'same' && kind !== 'bothFail' && shown[kind]++ < 12) console.log(`${kind}: ${file} ${JSON.stringify(specifier)}\n   node: ${node}\n   ours: ${ours}`);
}
console.log(counts);
