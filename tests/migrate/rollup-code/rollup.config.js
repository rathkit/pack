// A config like the ones of libraries that build several outputs from a helper: no value is a plain string at the place it is used.
import path from 'node:path';

const input = path.join('src', 'index.js');
const name = 'Sample';
const create = (file, format) => ({ input, output: { file: path.join('dist', file), format, name, sourcemap: format === 'cjs' } });

export default [create('sample.js', 'es'), create('sample.cjs', 'cjs'), create('sample.umd.js', 'umd'), { input: { a: 'src/a.js', b: 'src/b.js' }, output: { dir: 'dist/split' } }];
