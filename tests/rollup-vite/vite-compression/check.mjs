import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(out, 'bundle.mjs.gz')) === true ? null : 'missing: ' + "out, 'bundle.mjs.gz'");
