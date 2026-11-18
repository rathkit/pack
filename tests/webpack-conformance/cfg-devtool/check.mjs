import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'main.js.map')) ? null : 'main.js.map was not written');
