import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const read = (f) => (existsSync(join(out, f)) ? readFileSync(join(out, f), 'utf8') : null);
  const [special, everything, main] = [read('special.js'), read('everything.js'), read('main.js')];
  if (special === null || everything === null) return 'special.js and everything.js should both exist';
  if (!special.includes('PKG-A') || special.includes('PKG-B')) return 'special.js should hold vendor/a.js alone';
  if (!everything.includes('PKG-B') || everything.includes('PKG-A')) return 'everything.js should hold vendor/b.js alone';
  if (main.includes('PKG-')) return 'main.js still holds package code';
  return null;
};
