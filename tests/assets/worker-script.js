import { parentPort } from 'node:worker_threads';
import { double } from './worker-util.js';
import logo from './logo.svg';
parentPort.on('message', (n) => parentPort.postMessage(`${double(n)} ${logo.replace(/^.*\//, '').replace(/-[a-z0-9]+\./, '-HASH.')}`));
