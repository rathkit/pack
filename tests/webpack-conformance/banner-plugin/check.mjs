import { readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (readFileSync(join(out, 'main.js'), 'utf8').startsWith('/*! built by conformance */') || /^\/\*[^]*built by conformance[^]*\*\//.test(readFileSync(join(out, 'main.js'), 'utf8')) ? null : 'no banner at the top');
