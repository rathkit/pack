import legacy from '@rollup/plugin-legacy';
import path from 'node:path';
export default { plugins: [legacy({ [path.resolve(import.meta.dirname, 'lib.js')]: 'theThing' })] };
