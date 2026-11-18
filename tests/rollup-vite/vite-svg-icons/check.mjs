import { readFileSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => (readFileSync(join(out, 'bundle.mjs'), 'utf8').includes("icon-a") === true ? null : 'output lacks ' + "icon-a");
