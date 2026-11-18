import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'sw.js')) ? null : 'sw.js was not written');
