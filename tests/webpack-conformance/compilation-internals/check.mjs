import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
export default (out) => {
  if (!existsSync(join(out, 'seen.json'))) return 'the plugin wrote nothing';
  const seen = JSON.parse(readFileSync(join(out, 'seen.json'), 'utf8'));
  const bad = [];
  for (const k of ['chunkCount', 'modules', 'chunkModules', 'groups', 'namedChunks', 'assetInfo', 'hash']) if (seen[k] !== true) bad.push(k);
  for (const k of ['assets', 'chunks', 'entrypoints', 'modules']) if (seen.stats?.[k] !== true) bad.push('stats.' + k);
  if (!seen.entrypoints?.includes('main')) bad.push('entrypoints');
  if (!existsSync(join(out, 'extra.txt'))) bad.push('emitAsset');
  return bad.length ? 'not as webpack has it: ' + bad.join(', ') : null;
};
