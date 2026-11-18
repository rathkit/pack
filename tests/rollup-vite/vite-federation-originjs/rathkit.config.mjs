import federation from '@originjs/vite-plugin-federation';
export default { plugins: [federation({ name: 'host', remotes: {}, shared: [] })] };
