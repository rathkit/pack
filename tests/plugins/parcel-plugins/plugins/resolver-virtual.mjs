import { Resolver } from '@parcel/plugin';
import path from 'node:path';

export default new Resolver({
  async resolve({ specifier, dependency }) {
    if (specifier !== 'virtual:answer') return null;
    return { filePath: path.join(process.cwd(), 'virtual-answer.js'), code: `export default ${6 * 7};`, sideEffects: false };
  },
});
