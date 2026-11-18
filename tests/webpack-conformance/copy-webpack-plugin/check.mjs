import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'public', 'a.txt')) && existsSync(join(out, 'renamed.txt')) ? null : 'copied files are missing');
