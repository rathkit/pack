// Stand-ins for the four plugins of @vitejs/plugin-legacy: Pack skips them (it builds one pass), so none of these hooks may run.
const boom = (what) => () => { throw new Error(`${what} must not run`); };
export default {
  plugins: [
    { name: 'vite:legacy-config', apply: 'build', config: boom('config') },
    { name: 'vite:legacy-generate-polyfill-chunk', apply: 'build', generateBundle: boom('generateBundle') },
    { name: 'vite:legacy-post-process', enforce: 'post', apply: 'build', renderChunk: boom('renderChunk') },
    { name: 'vite:legacy-env', config: boom('env config') },
  ],
};
