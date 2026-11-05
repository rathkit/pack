// The federation runtime's version ranges against npm's `semver`, over every combination of a few hundred versions and ranges.
//   node tools/diff-semver.mjs        (needs `pnpm install --dir test-deps`)
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { join, dirname } from 'node:path';
import { existsSync } from 'node:fs';
import { testDeps, testModules, installHint } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
if (!existsSync(join(testModules, 'semver'))) {
  console.log('semver: not installed (' + installHint + '); skipped');
  process.exit(0);
}
const semver = createRequire(join(testDeps, 'package.json'))('semver');
const { satisfy } = await import(pathToFileURL(join(root, 'src', 'federation', 'semver.js')).href);

const versions = [];
for (const major of [0, 1, 2, 19]) for (const minor of [0, 1, 2, 5]) for (const patch of [0, 1, 3, 9]) versions.push(`${major}.${minor}.${patch}`);
for (const pre of ['alpha', 'beta.1', 'beta.2', 'rc.1', '0']) for (const base of ['1.2.3', '1.2.4', '2.0.0', '0.2.3', '19.0.0']) versions.push(`${base}-${pre}`);
const atoms = ['1', '1.2', '1.2.3', '0', '0.2', '0.0.3', '0.2.3', '2', '19', '1.x', '1.2.x', '1.*', '*', 'x', '', '1.2.3-beta.1', '1.2.3-rc.1', '2.0.0-alpha', '19.0.0-rc.1', '0.0'];
const ranges = new Set();
for (const op of ['', '^', '~', '>', '>=', '<', '<=', '=']) for (const a of atoms) ranges.add(op + a);
for (const a of atoms.slice(0, 12)) {
  for (const b of atoms.slice(0, 12)) {
    ranges.add(`${a} - ${b}`);
    ranges.add(`>=${a} <${b}`);
    ranges.add(`^${a} || ~${b}`);
    ranges.add(`>= ${a} < ${b}`);
  }
}
let total = 0;
const wrong = [];
for (const range of ranges) {
  for (const version of versions) {
    total++;
    const want = semver.satisfies(version, range);
    if (satisfy(version, range) !== want) wrong.push(`${version} in "${range}": semver says ${want}`);
  }
}
for (const line of wrong.slice(0, 20)) console.log('MISMATCH', line);
console.log(wrong.length ? `${wrong.length} of ${total} differ` : `all ${total} range checks agree with semver`);
process.exit(wrong.length ? 1 : 0);
