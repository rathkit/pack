// The package's code is in vendors.js, and not in the entry's file.
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  if (!existsSync(join(out, 'vendors.js'))) return `no vendors.js; files: ${readdirSync(out).join(' ')}`;
  const marker = 'SEMVER_SPEC_VERSION';
  if (!readFileSync(join(out, 'vendors.js'), 'utf8').includes(marker)) return 'vendors.js does not hold the semver code';
  if (readFileSync(join(out, 'main.js'), 'utf8').includes(marker)) return 'main.js holds semver code';
  return null;
};
