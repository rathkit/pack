import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(out, 'copied', 'a.txt')) === true ? null : 'missing: ' + "out, 'copied', 'a.txt'");
