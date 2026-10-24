import { Worker as NodeWorker } from 'node:worker_threads';
import Make from './worker-script.js?worker';
import inlined from './worker-script.js?worker&inline';
import url from './worker-script.js?worker&url';
// Node's workers take the address and nothing else; the browser's take options too.
globalThis.Worker = class extends NodeWorker {
  constructor(address) { super(address); }
};
const ask = (w, n) => new Promise((resolve) => { w.once('message', resolve); w.postMessage(n); });
const plain = new Worker(new URL('./worker-script.js', import.meta.url), { type: 'module' });
const made = new Make();
console.log('plain', await ask(plain, 21));
console.log('constructor', await ask(made, 4));
console.log('inline', typeof inlined, 'url', url.replace(/^.*\//, '').replace(/-[a-z0-9]+\./, '-HASH.'));
plain.terminate();
made.terminate();
