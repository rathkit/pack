import fs from 'node:fs';
import path from 'node:path';
import ignored from './ignored.js';
import env from './env.dev.js';

console.log(Object.keys(ignored).length === 0 ? 'ignored is empty' : 'ignored is not empty');
console.log(env.name, process.env.STAGE, $('x'));
const out = process.env.OUT_DIR;
console.log(fs.readFileSync(path.join(out, 'stamp.txt'), 'utf8'));
console.log(fs.readFileSync(path.join(out, 'child.txt'), 'utf8'));
console.log(fs.readFileSync(process.env.BUNDLE, 'utf8').split('\n')[0]);
console.log(fs.readFileSync(path.join(out, 'hooks.log'), 'utf8').trim());
