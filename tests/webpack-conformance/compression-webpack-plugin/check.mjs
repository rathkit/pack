import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'main.js.gz')) ? null : 'main.js.gz was not written');
