import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const seen = [];

const envPlugin = {
  name: 'env',
  setup(build) {
    build.onStart(() => { seen.push('start'); });
    build.onResolve({ filter: /^env$/ }, (args) => ({ path: args.path, namespace: 'env-ns' }));
    build.onLoad({ filter: /.*/, namespace: 'env-ns' }, () => ({ contents: JSON.stringify({ mode: 'plugin', node: typeof process }), loader: 'json' }));
    build.onResolve({ filter: /^virtual:(.*)$/ }, (args) => ({ path: args.path.slice(8), namespace: 'virtual', pluginData: { from: args.importer.split(/[\\/]/).pop() } }));
    build.onLoad({ filter: /.*/, namespace: 'virtual' }, (args) => ({ contents: `import tail from './tail.js'; export default ${JSON.stringify(args.path + ':' + args.pluginData.from)} + tail;`, resolveDir: here, loader: 'js' }));
    build.onResolve({ filter: /\.glob$/ }, (args) => ({ path: path.join(args.resolveDir, args.path), namespace: 'glob' }));
    build.onLoad({ filter: /.*/, namespace: 'glob' }, (args) => ({ contents: 'plain text from a loader', loader: 'text' }));
    build.onEnd((result) => { seen.push('end:' + result.errors.length); fs.writeFileSync(path.join(process.env.OUT_DIR ?? here, 'esbuild.log'), seen.join(',') + '\n'); });
  },
};
export default { plugins: [envPlugin] };
