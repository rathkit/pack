import { readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => { const h = readFileSync(join(out, 'index.html'), 'utf8'); return /integrity="sha384-/.test(h) ? null : h; };
