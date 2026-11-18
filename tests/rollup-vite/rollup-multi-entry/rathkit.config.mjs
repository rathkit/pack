import multiEntry from '@rollup/plugin-multi-entry';
export default { input: ['a.js', 'b.js'], plugins: [multiEntry()] };
