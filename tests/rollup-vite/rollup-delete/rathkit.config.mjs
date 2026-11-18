import del from 'rollup-plugin-delete';
export default { plugins: [del({ targets: 'old/*', hook: 'buildStart' })] };
