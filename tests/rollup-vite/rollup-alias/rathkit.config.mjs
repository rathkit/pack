import alias from '@rollup/plugin-alias';
import path from 'node:path';
export default { plugins: [alias({ entries: [{ find: '@', replacement: path.resolve(import.meta.dirname, 'src') }] })] };
