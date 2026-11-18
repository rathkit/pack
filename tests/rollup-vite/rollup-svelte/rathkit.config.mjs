import svelte from 'rollup-plugin-svelte';
import nodeResolve from '@rollup/plugin-node-resolve';
export default { plugins: [svelte(), nodeResolve({ browser: true, exportConditions: ['svelte'], extensions: ['.svelte', '.js'] })] };
