// A plugin that reads what plugins read of the compilation: chunks, chunk groups, entrypoints, modules, the chunk graph, assets, stats.
const path = require('path');
const fs = require('fs');
class Inspect {
  apply(compiler) {
    const seen = {};
    compiler.hooks.thisCompilation.tap('Inspect', (compilation) => {
      compilation.hooks.processAssets.tap({ name: 'Inspect', stage: 1000 }, (assets) => {
        seen.assetNames = Object.keys(assets).sort();
        seen.entrypoints = [...compilation.entrypoints.keys()];
        seen.entryChunkFiles = [...compilation.entrypoints.values()].map((e) => e.getFiles());
        seen.chunkCount = compilation.chunks.size > 0;
        seen.chunkIds = [...compilation.chunks].map((c) => typeof c.name === 'string' || c.name === null);
        seen.chunkFiles = [...compilation.chunks].map((c) => [...c.files].length > 0);
        seen.modules = [...compilation.modules].some((m) => typeof m.resource === 'string' && m.resource.endsWith('a.js'));
        const main = [...compilation.chunks].find((c) => c.name === 'main') ?? [...compilation.chunks][0];
        seen.chunkModules = compilation.chunkGraph.getChunkModules(main).some((m) => String(m.resource || m.identifier()).endsWith('main.js'));
        seen.groups = compilation.chunkGroups.length > 0;
        seen.namedChunks = compilation.namedChunks instanceof Map;
        seen.assetInfo = compilation.getAssets().every((a) => a.name && a.info && typeof a.source.size() === 'number');
        seen.hash = typeof compilation.hash === 'string';
        compilation.emitAsset('extra.txt', new compiler.webpack.sources.RawSource('extra'));
      });
    });
    compiler.hooks.done.tap('Inspect', (stats) => {
      const json = stats.toJson({ all: false, assets: true, chunks: true, entrypoints: true, modules: true });
      seen.stats = { assets: json.assets.length > 0, chunks: json.chunks.length > 0, entrypoints: !!json.entrypoints.main, modules: json.modules.length > 0, hasErrors: stats.hasErrors() };
      fs.writeFileSync(path.join(compiler.options.output.path, 'seen.json'), JSON.stringify(seen));
    });
  }
}
module.exports = { mode: 'production', target: 'node', plugins: [new Inspect()] };
