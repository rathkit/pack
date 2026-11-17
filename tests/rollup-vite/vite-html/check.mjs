import { readFileSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => { const html = readFileSync(join(out, 'index.html'), 'utf8'); for (const t of ["<title>Titled</title>"]) if (!html.includes(t)) return 'index.html lacks ' + t; return null; };
