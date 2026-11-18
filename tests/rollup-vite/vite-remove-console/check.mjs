import { readFileSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => (readFileSync(join(out, 'bundle.mjs'), 'utf8').includes("gone") === false ? null : 'output has ' + "gone");
