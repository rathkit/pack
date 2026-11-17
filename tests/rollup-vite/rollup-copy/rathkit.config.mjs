import copy from 'rollup-plugin-copy';
export default { plugins: [copy({ targets: [{ src: 'a.txt', dest: 'dist/copied' }] })] };
