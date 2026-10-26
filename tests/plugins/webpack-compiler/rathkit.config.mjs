import { fileURLToPath } from 'node:url';
// Plugins written against webpack's compiler: lifecycle hooks, assets, stats, a child compiler, and the plugins
// that ship with webpack (imported from 'webpack', which this project does not install).
import fs from 'node:fs';
import path from 'node:path';
import webpack from 'webpack';

const { Compilation, sources } = webpack;
const here = path.dirname(fileURLToPath(import.meta.url));
const order = [];

class TracePlugin {
  apply(compiler) {
    for (const name of ['environment', 'afterEnvironment', 'entryOption', 'afterPlugins', 'run', 'compile', 'thisCompilation', 'make', 'finishMake', 'afterCompile', 'emit', 'afterEmit']) {
      // Child compilers copy the taps of the parent, as in webpack; the first run of each hook is the parent's.
      compiler.hooks[name].tap('TracePlugin', () => { if (!order.includes(name)) order.push(name); });
    }
    compiler.hooks.thisCompilation.tap('TracePlugin', (compilation) => {
      for (const name of ['buildModule', 'finishModules', 'seal', 'processAssets', 'afterProcessAssets', 'afterSeal']) {
        compilation.hooks[name].tap({ name: 'TracePlugin', stage: 0 }, () => { if (!compilation.compiler.isChild() && !order.includes('c.' + name)) order.push('c.' + name); });
      }
    });
    // An async hook, tapped the three ways.
    compiler.hooks.emit.tapAsync('TracePlugin', (compilation, callback) => { order.push('emit:async'); callback(); });
    compiler.hooks.emit.tapPromise('TracePlugin', async () => { order.push('emit:promise'); });
    compiler.hooks.done.tapPromise('TracePlugin', async (stats) => {
      const json = stats.toJson({ all: false, assets: true });
      const names = json.assets.map((a) => a.name.replace(/^bundle\.\w+$/, 'bundle')).sort();
      const positions = ['environment', 'entryOption', 'run', 'compile', 'thisCompilation', 'make', 'c.seal', 'c.processAssets', 'emit', 'afterEmit'].map((n) => order.indexOf(n));
      const increasing = positions.every((p, i) => p >= 0 && (i === 0 || p > positions[i - 1]));
      fs.writeFileSync(path.join(process.env.OUT_DIR ?? here, 'hooks.log'), [`order ok: ${increasing}`, `emit taps: ${order.filter((n) => n.startsWith('emit:')).join(',')}`, `assets: ${names.join(',')}`, `errors: ${stats.hasErrors()} warnings: ${stats.hasWarnings()}`].join('\n') + '\n');
    });
  }
}

class StampPlugin {
  apply(compiler) {
    compiler.hooks.thisCompilation.tap('StampPlugin', (compilation) => {
      compilation.hooks.processAssets.tap({ name: 'StampPlugin', stage: Compilation.PROCESS_ASSETS_STAGE_ADDITIONAL }, (assets) => {
        compilation.emitAsset('stamp.txt', new sources.RawSource('stamp: ' + Object.keys(assets).map((n) => n.replace(/^bundle\.\w+$/, 'bundle')).sort().join(',')));
      });
      // Later stages see what earlier ones added.
      compilation.hooks.processAssets.tap({ name: 'StampPlugin', stage: Compilation.PROCESS_ASSETS_STAGE_OPTIMIZE }, () => {
        compilation.updateAsset('stamp.txt', (old) => new sources.RawSource(old.source() + ' +optimized'));
        compilation.emitAsset('tmp.txt', new sources.RawSource('temporary'));
        compilation.renameAsset('tmp.txt', 'renamed.txt');
        compilation.deleteAsset('renamed.txt');
      });
    });
  }
}

// html-webpack-plugin's way of working in miniature: compile a file with a child compiler, run its output.
class ChildPlugin {
  apply(compiler) {
    compiler.hooks.thisCompilation.tap('ChildPlugin', (compilation) => {
      compilation.hooks.processAssets.tapPromise({ name: 'ChildPlugin', stage: Compilation.PROCESS_ASSETS_STAGE_ADDITIONS }, async () => {
        const child = compilation.createChildCompiler('ChildPlugin', { filename: '__child-[name]', library: { type: 'var', name: 'CHILD_RESULT' } }, [new webpack.library.EnableLibraryPlugin('var')]);
        new webpack.EntryPlugin(compiler.context, path.join(here, 'child-template.js'), 'tpl').apply(child);
        const extracted = [];
        child.hooks.thisCompilation.tap('ChildPlugin', (childCompilation) => {
          childCompilation.hooks.processAssets.tap({ name: 'ChildPlugin', stage: Compilation.PROCESS_ASSETS_STAGE_ADDITIONS }, (assets) => {
            for (const name of Object.keys(assets)) if (name.startsWith('__child-')) { extracted.push(assets[name].source()); childCompilation.deleteAsset(name); }
          });
        });
        await new Promise((resolve, reject) => child.runAsChild((error, entries, childCompilation) => (error ? reject(error) : resolve())));
        const factory = new Function(extracted[0] + '\nreturn CHILD_RESULT;')();
        compilation.emitAsset('child.txt', new sources.RawSource(factory({ word: 'hello' })));
      });
    });
  }
}

export default {
  mode: 'production',
  plugins: [
    new TracePlugin(),
    new StampPlugin(),
    new ChildPlugin(),
    new webpack.BannerPlugin({ banner: 'built by webpack-compiler fixture' }),
    new webpack.IgnorePlugin({ resourceRegExp: /^\.\/ignored\.js$/ }),
    new webpack.NormalModuleReplacementPlugin(/env\.dev\.js$/, './env.prod.js'),
    new webpack.EnvironmentPlugin({ STAGE: 'fixture' }),
    new webpack.ProvidePlugin({ $: path.join(here, 'jq.js') }),
  ],
};
