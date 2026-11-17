import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(dir, 'stats.html')) === true ? null : 'missing: ' + "dir, 'stats.html'");
