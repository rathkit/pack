import strip from '@rollup/plugin-strip';
export default { plugins: [strip({ functions: ['console.debug', 'assert.*'] })] };
