import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'assets.json')) ? null : 'assets.json was not written');
