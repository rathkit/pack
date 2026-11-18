import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  if (existsSync(join(out, 'skipped.js'))) return 'skipped.js exists although its modules are below minSize';
  if (!existsSync(join(out, 'forced.js'))) return 'forced.js is missing (enforce)';
  if (!readFileSync(join(out, 'forced.js'), 'utf8').includes('SMALL-B')) return 'forced.js does not hold vendor/b.js';
  if (!readFileSync(join(out, 'main.js'), 'utf8').includes('SMALL-A')) return 'vendor/a.js should have stayed in main.js';
  return null;
};
