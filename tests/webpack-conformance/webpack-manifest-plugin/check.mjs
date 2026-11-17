import { readFileSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  const manifest = JSON.parse(readFileSync(join(out, 'asset-manifest.json'), 'utf8'));
  return manifest['main.js'] ? null : `manifest: ${JSON.stringify(manifest)}`;
};
