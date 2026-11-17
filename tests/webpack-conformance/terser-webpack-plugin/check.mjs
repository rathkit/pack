import { readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => (/argumentNumberOne/.test(readFileSync(join(out, 'main.js'), 'utf8')) ? 'the output was not minified' : null);
