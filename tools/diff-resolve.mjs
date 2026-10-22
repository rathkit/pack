// Compares the resolver with Node's own `require.resolve`.
//   rathkit-pack --resolve-dump FILE... > dump.tsv; node tools/diff-resolve.mjs dump.tsv
// Prints disagreements and a summary. Cases where Node itself throws and the resolver also
// fails agree; a request Node cannot satisfy but the resolver can (or the reverse) does not.
import { createRequire } from 'node:module';
import { readFileSync, realpathSync } from 'node:fs';

const rows = readFileSync(process.argv[2], 'utf8').split('\n').filter(Boolean).map((l) => l.split('\t'));
const counts = { same: 0, bothFail: 0, differ: 0, onlyNodeFails: 0, onlyWeFail: 0 };
const shown = { differ: 0, onlyNodeFails: 0, onlyWeFail: 0 };
for (const [file, specifier, ours] of rows) {
  let node;
  try {
    node = realpathSync(createRequire(file).resolve(specifier));
  } catch (e) {
    node = null;
  }
  const weFail = ours.startsWith('ERROR');
  let kind;
  if (node === null && weFail) kind = 'bothFail';
  else if (node === null) kind = 'onlyNodeFails';
  else if (weFail) kind = 'onlyWeFail';
  else kind = node === ours ? 'same' : 'differ';
  counts[kind]++;
  if (kind !== 'same' && kind !== 'bothFail' && shown[kind]++ < 15) console.log(`${kind}: ${file} ${JSON.stringify(specifier)}\n   node: ${node}\n   ours: ${ours}`);
}
console.log(counts);
