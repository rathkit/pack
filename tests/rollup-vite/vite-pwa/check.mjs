import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(out, 'sw.js')) === true ? null : 'missing: ' + "out, 'sw.js'");
