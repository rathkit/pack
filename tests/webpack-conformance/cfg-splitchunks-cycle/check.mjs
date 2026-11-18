import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out, dir) => {
  if (existsSync(join(out, 'loop.js'))) return 'loop.js exists: the group that makes a cycle was not given up';
  if (!existsSync(join(out, 'good.js'))) return 'good.js is missing: the other group should still apply';
  const stderr = readFileSync(join(dir, 'build-stderr.txt'), 'utf8');
  if (!/chunk group 'loop'/.test(stderr)) return `no warning that names the group; stderr: ${stderr}`;
  return null;
};
