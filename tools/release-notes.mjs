// Prints the CHANGELOG.md section of one version, for the notes of its GitHub Release.
//   node tools/release-notes.mjs 0.1.0-alpha.0
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = process.argv[2];
if (!version) {
  console.error('usage: node tools/release-notes.mjs VERSION');
  process.exit(2);
}
const text = readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md'), 'utf8');
const heading = new RegExp(`^## \\[?${version.replace(/[.+-]/g, '\\$&')}\\]?(?: .*)?$`, 'm').exec(text);
if (!heading) {
  console.error(`CHANGELOG.md has no section for ${version}`);
  process.exit(1);
}
const rest = text.slice(heading.index + heading[0].length);
const next = rest.search(/^## /m);
process.stdout.write(rest.slice(0, next === -1 ? undefined : next).trim() + '\n');
