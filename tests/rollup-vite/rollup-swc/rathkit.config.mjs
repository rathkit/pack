import swc from '@rollup/plugin-swc';
export default { plugins: [swc({ swc: { jsc: { parser: { syntax: 'typescript' }, target: 'es2022' } } })] };
