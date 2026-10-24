import copy from 'rollup-plugin-copy';
export default { plugins: [copy({ targets: [{ src: 'pic.png', dest: 'copied' }], hook: 'writeBundle' })] };
