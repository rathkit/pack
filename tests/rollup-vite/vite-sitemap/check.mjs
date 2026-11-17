import { existsSync } from 'node:fs'; import { join } from 'node:path';
export default (out, dir) => (existsSync(join(out, 'sitemap.xml')) === true ? null : 'missing: ' + "out, 'sitemap.xml'");
