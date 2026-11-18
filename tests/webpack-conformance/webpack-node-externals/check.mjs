import { readFileSync } from 'node:fs';
import { join } from 'node:path';
// webpack-merge is in node_modules, so it stays a require() and its code is not in the bundle.
export default (out) => { const s = readFileSync(join(out, 'main.js'), 'utf8'); return /webpack-merge/.test(s) && s.length < 4000 ? null : `webpack-merge was bundled (${s.length} bytes)`; };
