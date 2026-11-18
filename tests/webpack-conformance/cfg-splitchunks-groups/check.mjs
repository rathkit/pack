// The group's `test` takes the files of vendor/ out of the chunks they were in, into one file named by the group.
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  if (!existsSync(join(out, 'vendors.js'))) return `no vendors.js; files: ${readdirSync(out).join(' ')}`;
  if (!readFileSync(join(out, 'vendors.js'), 'utf8').includes('VENDOR-LIB')) return 'vendors.js does not hold the package code';
  for (const f of readdirSync(out).filter((n) => n.endsWith('.js') && n !== 'vendors.js')) {
    if (readFileSync(join(out, f), 'utf8').includes('VENDOR-LIB')) return `${f} holds package code that vendors.js should have`;
  }
  return null;
};
