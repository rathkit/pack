// `test` as a function of the module picks vendor/util.js; lazy.js matches the `shared` test too, but a module `import()` loads keeps its own chunk.
// `name` as a function is not followed: the group makes a file named after its key. A function `chunks` of splitChunks is not followed either;
// the group names its own.
export default { mode: 'production', target: 'node', optimization: { splitChunks: { chunks: (chunk) => chunk.name !== 'skip', name: (module, chunks, key) => key + '-' + chunks.map((c) => c.name).join('-'), cacheGroups: { shared: { test: (module) => /lazy|vendor/.test(module.resource || ''), name(module, chunks, key) { return 'shared'; }, chunks: 'all', minSize: 0 } } } } };
