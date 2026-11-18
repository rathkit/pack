import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const text = (f) => readFileSync(join(out, f), 'utf8');
  if (!existsSync(join(out, 'common.js'))) return `no common.js: ${readdirSync(out)}`;
  if (!text('common.js').includes('USED-BY-TWO') || text('common.js').includes('USED-BY-ONE')) return 'common.js should hold vendor/two.js only';
  const withOne = readdirSync(out).filter((f) => f.endsWith('.js') && text(f).includes('USED-BY-ONE'));
  if (withOne.length !== 1 || withOne[0] === 'common.js') return `vendor/one.js is in ${withOne}`;
  return null;
};
