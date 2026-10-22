import fs from 'node:fs';
import path from 'node:path';
import { greet } from './shared.js';

const workerUrl = __WORKER__;
console.log(workerUrl.startsWith('file://'), workerUrl.endsWith('/worker.mjs'));
console.log(greet('main'));
const worker = await import(workerUrl);
console.log(worker.default);
const meta = JSON.parse(fs.readFileSync(path.join(process.env.OUT_DIR, 'meta.json'), 'utf8'));
console.log(JSON.stringify(meta));
