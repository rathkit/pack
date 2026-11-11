import path from 'node:path';

// A style block of a component is a module whose id ends in `?vue&type=style&index=0&lang.scss`; its text is Sass although its file is not.
const block = path.join(import.meta.dirname, 'Card.vue') + '?vue&type=style&index=0&lang.scss';
export default {
  plugins: [{
    name: 'style-block',
    resolveId(source) { if (source === 'virtual:block') return block; },
    load(id) { if (id === block) return "@use 'ui-kit/styles/base';\n.block { margin: 1px; }\n"; },
  }],
};
