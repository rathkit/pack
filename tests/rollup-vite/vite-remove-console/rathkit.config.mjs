import removeConsole from 'vite-plugin-remove-console';
export default { plugins: [removeConsole({ includes: ['log'] })] };
