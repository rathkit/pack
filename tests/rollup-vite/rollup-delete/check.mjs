import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(dir, 'old', 'x.txt')) === false ? null : 'still there: ' + "dir, 'old', 'x.txt'");
