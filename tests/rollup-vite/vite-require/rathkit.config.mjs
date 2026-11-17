import pluginRequire from 'vite-plugin-require';
export default { plugins: [(pluginRequire.default ?? pluginRequire)()] };
