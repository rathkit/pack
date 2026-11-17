import esbuild from 'rollup-plugin-esbuild';
export default { plugins: [esbuild({ target: 'es2020' })] };
