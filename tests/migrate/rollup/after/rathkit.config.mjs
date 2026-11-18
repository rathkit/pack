import path from 'node:path';

// Only what rkp does not do itself: node-resolve, commonjs, replace and terser are built in.
const text = { name: 'text', transform: (code, id) => (id.endsWith('.txt') ? `export default ${JSON.stringify(code)};` : null) };
const at = { name: 'at', resolveId: (source) => (source.startsWith('@/') ? path.resolve('src', source.slice(2)) : null) };

export default { plugins: [at, text] };
