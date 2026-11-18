import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'index.html')) ? null : 'index.html was not written');
