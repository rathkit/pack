// Compares the linker's namespace names with Node's own module linker.
//   rkp --link-dump ENTRY... > dump.txt; node --experimental-vm-modules tools/diff-link.mjs dump.txt
// Each entry's import closure is loaded as vm.SourceTextModule and linked but never
// evaluated, so no package code runs. A link error in Node must be a link error in ours, and
// otherwise the sorted `Object.keys` of the namespace must equal our export names.
import vm from 'node:vm';
import { readFileSync } from 'node:fs';

const entries = [];
let current = null;
for (const line of readFileSync(process.argv[2], 'utf8').split('\n')) {
  const [tag, a, b, c] = line.split('\t');
  if (tag === 'ENTRY') entries.push((current = { path: a, edges: new Map(), names: [], errors: 0 }));
  else if (tag === 'EDGE') current.edges.set(a + '\0' + b, c);
  else if (tag === 'NAMES') current.names = a ? a.split('\x1f') : [];
  else if (tag === 'ERRORS') current.errors = Number(a);
}
const counts = { same: 0, differ: 0, bothReject: 0, nodeRejects: 0, weReject: 0 };
let shown = 0;
for (const entry of entries) {
  const cache = new Map();
  const load = (path) => {
    if (!cache.has(path)) cache.set(path, new vm.SourceTextModule(readFileSync(path, 'utf8'), { identifier: path }));
    return cache.get(path);
  };
  let result;
  try {
    const root = load(entry.path);
    await root.link((specifier, referrer) => {
      const target = entry.edges.get(referrer.identifier + '\0' + specifier);
      if (!target) throw new Error('no edge ' + specifier);
      return load(target);
    });
    result = Reflect.ownKeys(root.namespace).filter((k) => typeof k === 'string').sort();
  } catch (e) {
    result = null;
    entry.nodeError = String(e.message).split('\n')[0];
  }
  const ours = [...entry.names].sort();
  let kind;
  if (result === null) kind = entry.errors > 0 ? 'bothReject' : 'nodeRejects';
  else if (entry.errors > 0) kind = 'weReject';
  else kind = JSON.stringify(result) === JSON.stringify(ours) ? 'same' : 'differ';
  counts[kind]++;
  if ((kind === 'differ' || kind === 'weReject' || kind === 'nodeRejects') && shown++ < 12) {
    console.log(kind, entry.path, entry.nodeError ?? '');
    if (kind === 'differ') console.log('  node-only:', result.filter((n) => !ours.includes(n)).slice(0, 8), ' ours-only:', ours.filter((n) => !result.includes(n)).slice(0, 8));
  }
}
console.log(counts);
