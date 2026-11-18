import path from 'node:path';
import { nodeResolve } from '@rollup/plugin-node-resolve';
import commonjs from '@rollup/plugin-commonjs';
import replace from '@rollup/plugin-replace';
import terser from '@rollup/plugin-terser';

const text = { name: 'text', transform: (code, id) => (id.endsWith('.txt') ? `export default ${JSON.stringify(code)};` : null) };
const at = { name: 'at', resolveId: (source) => (source.startsWith('@/') ? path.resolve('src', source.slice(2)) : null) };

export default {
  input: 'src/main.js',
  output: { file: 'dist/main.js', format: 'es', sourcemap: true },
  plugins: [
    at,
    text,
    nodeResolve(),
    commonjs(),
    replace({ preventAssignment: true, values: { __APP_VERSION__: '"1.2.3"', 'process.env.NODE_ENV': '"production"' } }),
    terser(),
  ],
};
