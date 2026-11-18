import { existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (existsSync(join(out, 'note.txt')) ? null : 'dist/note.txt was not written');
