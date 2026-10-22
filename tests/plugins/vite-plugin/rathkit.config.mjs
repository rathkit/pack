import path from 'node:path';

function versionPlugin() {
  let root = '';
  return {
    name: 'vite:version',
    enforce: 'pre',
    config(config, env) {
      return { define: { __APP_VERSION__: JSON.stringify(`v1 ${env.command}`) } };
    },
    configResolved(resolved) {
      root = resolved.root;
    },
    resolveId(source) {
      if (source === 'virtual:root') return '\0virtual:root';
    },
    load(id) {
      if (id === '\0virtual:root') return `export const root = ${JSON.stringify(path.basename(root))};`;
    },
  };
}

const servePlugin = { name: 'only-serve', apply: 'serve', transform: () => { throw new Error('must not run in a build'); } };
const post = { name: 'post', enforce: 'post', transform(code, id) { return id.endsWith('main.js') ? code.replace('PLACEHOLDER', 'post-transformed') : null; } };
const first = { name: 'first', transform(code, id) { return id.endsWith('main.js') ? code.replace('PLACEHOLDER', 'normal-transformed') : null; } };

export default { plugins: [post, versionPlugin(), [servePlugin, [first]]] };
