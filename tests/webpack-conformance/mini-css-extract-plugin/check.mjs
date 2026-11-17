import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const css = readdirSync(out).filter((f) => f.endsWith('.css'));
  return css.length === 1 && /\.box\s*\{\s*color:\s*green/.test(readFileSync(join(out, css[0]), 'utf8')) ? null : `style sheets: ${css}`;
};
