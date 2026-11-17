import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(out, 'main.d.ts')) === true ? null : 'missing: ' + "out, 'main.d.ts'");
