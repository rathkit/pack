import replace from '@rollup/plugin-replace';
export default { plugins: [replace({ preventAssignment: true, values: { __V__: '1', 'process.env.NODE_ENV': JSON.stringify('production') } })] };
