import { here } from './theme/where';

// A file the configuration imports has its own `__dirname`, not the configuration's.
export default {
  plugins: [{
    name: 'dirname-import',
    resolveId(source) { if (source === 'virtual:where') return '\0virtual:where'; },
    load(id) { if (id === '\0virtual:where') return `export const where = ${JSON.stringify(here)};`; },
  }],
};
