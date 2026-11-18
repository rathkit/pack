import inject from '@rollup/plugin-inject';
import path from 'node:path';
export default { plugins: [inject({ greet: path.resolve(import.meta.dirname, 'greet.js') })] };
