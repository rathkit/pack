import { readFileSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => (readFileSync(join(out, 'bundle.mjs'), 'utf8').includes("color: red") === true ? null : 'output lacks ' + "color: red");
