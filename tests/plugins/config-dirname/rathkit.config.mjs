import path from 'node:path';

// A Vite configuration written for CommonJS reads __dirname when it is loaded (a configuration is bundled, as Vite does, so it has a value).
const where = path.basename(__dirname) + ':' + path.basename(__filename);

export default {
  plugins: [{
    name: 'dirname',
    resolveId(source) { if (source === 'virtual:where') return '\0virtual:where'; },
    load(id) { if (id === '\0virtual:where') return `export const where = ${JSON.stringify(where)};`; },
  }],
};
