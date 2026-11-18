import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const read = (f) => (existsSync(join(out, f)) ? readFileSync(join(out, f), 'utf8') : null);
  const files = readdirSync(out).filter((f) => f.endsWith('.js'));
  const where = (text) => files.filter((f) => read(f).includes(text));
  if (where('INITIAL-ONLY').join() !== 'first.js') return `INITIAL-ONLY is in ${where('INITIAL-ONLY')} (want first.js)`;
  if (where('ASYNC-ONLY').join() !== 'later.js') return `ASYNC-ONLY is in ${where('ASYNC-ONLY')} (want later.js)`;
  // reached by the entry too, so an 'async' group does not take it
  const shared = where('SHARED-BOTH');
  if (shared.length !== 1 || shared[0] === 'wrong-kind.js' || shared[0] === 'wrong-kind-2.js') return `SHARED-BOTH is in ${shared}`;
  if (files.some((f) => f.startsWith('wrong-kind'))) return `a group of the wrong kind made a file: ${files}`;
  return null;
};
