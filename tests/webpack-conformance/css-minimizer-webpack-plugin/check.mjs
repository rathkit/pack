import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const css = readdirSync(out).filter((f) => f.endsWith('.css'));
  const text = css.length === 1 ? readFileSync(join(out, css[0]), 'utf8').trim() : '';
  return text && !text.includes('\n') ? null : `not minified: ${JSON.stringify(text)}`;
};
