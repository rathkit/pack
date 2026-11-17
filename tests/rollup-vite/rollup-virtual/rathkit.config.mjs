import virtual from '@rollup/plugin-virtual';
export default { plugins: [virtual({ virt: 'export default 5;' })] };
