import greeting, { extra } from 'virtual:greeting';
import message from '#alias/message';
import fs from 'node:fs';
import path from 'node:path';

console.log(greeting, extra, message, __BUILD__);
const out = process.env.OUT_DIR;
console.log(JSON.parse(fs.readFileSync(path.join(out, 'manifest.json'), 'utf8')).files.length >= 1);
console.log(fs.readFileSync(process.env.BUNDLE, 'utf8').startsWith('/* chunk '));
