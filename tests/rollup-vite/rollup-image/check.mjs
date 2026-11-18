import { readFileSync } from 'node:fs'; import { join } from 'node:path';
export default (out) => (readFileSync(join(out, 'bundle.mjs'), 'utf8').includes('data:image/png;base64,') ? null : 'no data URL in the output');
