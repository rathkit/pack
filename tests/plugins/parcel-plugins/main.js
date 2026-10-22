import fs from 'node:fs';
import path from 'node:path';
import text from './note.txt';
import answer from 'virtual:answer';
// TODO: the validator reads this

console.log(text, answer, globalThis.__parcelRuntime);
const self = fs.readFileSync(process.env.BUNDLE, 'utf8');
console.log(/\/\* optimized (mjs|cjs|js) \*\/\s*$/.test(self));
const out = process.env.OUT_DIR;
console.log(fs.readFileSync(path.join(out, path.basename(process.env.BUNDLE) + '.size'), 'utf8').replace(/\d+/, 'N'));
console.log(fs.readFileSync(path.join(out, 'hooks.log'), 'utf8').trim());
console.log(fs.readFileSync(path.join(out, 'validated.txt'), 'utf8').trim());
