import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const log = [];

const virtual = {
  name: 'virtual-modules',
  resolveId(source, importer) {
    if (source === 'virtual:greeting') return '\0virtual:greeting';
    if (source === '#alias/message') return this.resolve('./message.js', importer, { skipSelf: true });
    if (source === 'left-out') return { id: 'left-out', external: true };
    return null;
  },
  load(id) {
    if (id === '\0virtual:greeting') return 'export default "hello from virtual"; export const extra = 1;';
    return null;
  },
};

const replace = {
  name: 'replace-tokens',
  transform: {
    filter: { id: /\.js$/ },
    handler(code, id) {
      if (!code.includes('%%NAME%%')) return null;
      return { code: code.replaceAll('%%NAME%%', 'rathkit'), map: null, meta: { replaced: true } };
    },
  },
};

const wrap = {
  name: 'wrap-output',
  buildStart() { log.push('buildStart'); },
  buildEnd() { log.push('buildEnd'); },
  renderChunk(code, chunk, options) {
    return { code: `/* chunk ${chunk.fileName} format=${options.format} */\n${code}`, map: null };
  },
  generateBundle(options, bundle) {
    const names = Object.keys(bundle).sort();
    this.emitFile({ type: 'asset', fileName: 'manifest.json', source: JSON.stringify({ files: names, chunk: bundle[names[0]].type }) });
  },
  writeBundle(options, bundle) {
    log.push('writeBundle');
  },
  closeBundle() {
    log.push('closeBundle');
    fs.writeFileSync(path.join(process.env.OUT_DIR ?? here, 'hooks.log'), log.join(',') + '\n');
  },
};

export default { plugins: [virtual, replace, wrap], define: { __BUILD__: '"test-build"' }, external: ['left-out'] };
