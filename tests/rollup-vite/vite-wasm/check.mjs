import { readdirSync } from 'node:fs';
export default (out) => (readdirSync(out).some((f) => f.endsWith('.wasm')) ? null : 'the wasm file was not written');
