import { readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const html = readFileSync(join(out, 'page.html'), 'utf8');
  return /<title>Conformance page<\/title>/.test(html) && /<script[^>]*src="[^"]*main\.js"/.test(html) && /name="viewport"/.test(html) ? null : `page.html: ${html}`;
};
