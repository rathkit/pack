// Compares bundlers on the same inputs: wall time of the whole command (start-up included), peak memory, output
// size, and whether the output runs and prints the same thing.
//
//   pnpm install && node generate.mjs && node run.mjs [options] [workload...]
//
//   --runs N          timed runs per case, after one warm-up run (default 7)
//   --tools a,b,c     rathkit, esbuild, rolldown, rspack, vite (default rathkit; the others are for development only)
//   --env production  define process.env.NODE_ENV as "production" in every tool (the default), or `none` for no define
//   --modes a,b       plain, minify, minify+map (default all three)
//   --markdown        also print the tables as Markdown
//
// RATHKIT_PACK_BIN names the rathkit-pack binary (default ../target/release/rkp).
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync, readdirSync, statSync, rmSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { gzipSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { builtinModules } from 'node:module';

const here = dirname(fileURLToPath(import.meta.url));
const bin = (n) => join(here, 'node_modules/.bin', n);
const RK = process.env.RATHKIT_PACK_BIN ?? join(here, '..', 'target', 'release', 'rkp');

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(name);
  if (at < 0) return fallback;
  const [, value] = args.splice(at, 2);
  return value;
};
const runs = Number(option('--runs', 7));
const env = option('--env', 'production');
const markdown = args.includes('--markdown') && args.splice(args.indexOf('--markdown'), 1).length > 0;
const allModes = { plain: { minify: false, map: false }, minify: { minify: true, map: false }, 'minify+map': { minify: true, map: true } };
const modeNames = option('--modes', Object.keys(allModes).join(',')).split(',');
const production = env === 'production';

const workloads = {
  ts5: 'inputs/ts5.js',
  three: 'inputs/three.js',
  lodash: 'inputs/lodash.js',
  react: 'inputs/react.js',
  rxjs: 'inputs/rxjs.js',
  app1500: 'inputs/generated/app1500/main.js',
  tsapp500: 'inputs/generated/tsapp500/main.ts',
};
// three needs a browser to run; the others print something every tool must agree on.
const runnable = new Set(['ts5', 'react', 'rxjs', 'app1500', 'tsapp500']);
const externals = [...builtinModules, ...builtinModules.map((m) => 'node:' + m)];
const NODE_ENV = '"production"';

const tools = {
  rathkit: ({ entry, out, minify, map }) => ({ cmd: RK, args: ['bundle', entry, '--outfile', out, '--no-config', ...(production ? ['--define', `process.env.NODE_ENV=${NODE_ENV}`] : []), ...(minify ? ['--minify'] : []), ...(map ? ['--sourcemap'] : [])] }),
  esbuild: ({ entry, out, minify, map }) => ({ cmd: bin('esbuild'), args: [entry, '--bundle', '--format=esm', '--platform=node', `--outfile=${out}`, '--log-level=error', ...(production ? [`--define:process.env.NODE_ENV=${NODE_ENV}`] : []), ...(minify ? ['--minify'] : []), ...(map ? ['--sourcemap'] : [])] }),
  rolldown: ({ entry, out, minify, map }) => ({ cmd: bin('rolldown'), args: [entry, '--file', out, '--format', 'esm', '--platform', 'node', ...(production ? ['--transform.define', `process.env.NODE_ENV=${NODE_ENV}`] : []), ...(minify ? ['--minify'] : []), ...(map ? ['--sourcemap'] : [])] }),
  rspack: ({ entry, out, minify, map, dir }) => {
    const config = join(dir, 'rspack.config.mjs');
    writeFileSync(
      config,
      `export default { mode: 'production', context: ${JSON.stringify(here)}, entry: ${JSON.stringify(join(here, entry))}, target: 'node', externalsPresets: { node: true }, devtool: ${map ? "'source-map'" : 'false'}, optimization: { minimize: ${minify}, nodeEnv: ${production ? "'production'" : 'false'} }, experiments: { outputModule: true }, resolve: { extensions: ['.ts', '.js', '.mjs', '.json'] }, module: { rules: [{ test: /\\.ts$/, loader: 'builtin:swc-loader', options: { jsc: { parser: { syntax: 'typescript' } } }, type: 'javascript/auto' }] }, output: { path: ${JSON.stringify(dirname(out))}, filename: ${JSON.stringify(out.split('/').pop())}, module: true, library: { type: 'module' }, chunkFormat: 'module' }, stats: 'errors-only' };\n`,
    );
    return { cmd: bin('rspack'), args: ['build', '--config', config] };
  },
  vite: ({ entry, out, minify, map, dir }) => {
    const config = join(dir, 'vite.config.mjs');
    writeFileSync(
      config,
      `export default { root: ${JSON.stringify(here)}, logLevel: 'error', configFile: false, define: ${production ? `{ 'process.env.NODE_ENV': ${JSON.stringify(NODE_ENV)} }` : '{}'}, build: { outDir: ${JSON.stringify(dirname(out))}, emptyOutDir: false, minify: ${minify ? "'oxc'" : 'false'}, sourcemap: ${map}, target: 'esnext', lib: { entry: ${JSON.stringify(join(here, entry))}, formats: ['es'], fileName: () => ${JSON.stringify(out.split('/').pop())} }, rollupOptions: { external: ${JSON.stringify(externals)} } } };\n`,
    );
    return { cmd: bin('vite'), args: ['build', '--config', config] };
  },
};
const selected = option('--tools', 'rathkit').split(',');
// What is left after the options are the workloads to run (all of them when none are named).
const only = args.filter((a) => !a.startsWith('--'));

for (const need of [existsSync(RK) ? null : `the rathkit-pack binary (cargo build --release, or RATHKIT_PACK_BIN): ${RK}`, existsSync(join(here, 'node_modules')) ? null : 'pnpm install (in bench/)', existsSync(join(here, 'inputs/generated')) ? null : 'node generate.mjs']) {
  if (need) {
    console.error(`missing: ${need}`);
    process.exit(2);
  }
}

const run = (spec, cwd) => {
  const t = process.hrtime.bigint();
  const r = spawnSync('/usr/bin/time', ['-f', '%M', '--', spec.cmd, ...spec.args], { cwd, encoding: 'utf8', maxBuffer: 1 << 28 });
  const ms = Number(process.hrtime.bigint() - t) / 1e6;
  return { ms, rss: Number((r.stderr.trim().split('\n').pop() ?? '').trim()), ok: r.status === 0, err: r.stderr };
};
const median = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
const sizeOf = (dir) => {
  const files = readdirSync(dir).filter((f) => /\.m?js$/.test(f));
  const big = files.map((f) => join(dir, f)).sort((a, b) => statSync(b).size - statSync(a).size)[0];
  const data = readFileSync(big);
  return { bytes: data.length, gzip: gzipSync(data).length };
};

const rows = [];
for (const [name, entry] of Object.entries(workloads)) {
  if (only.length && !only.includes(name)) continue;
  for (const mode of modeNames) {
    const { minify, map } = allModes[mode];
    let reference = null;
    for (const tool of selected) {
      const dir = join(here, 'out', `${name}-${mode}-${tool}`);
      rmSync(dir, { recursive: true, force: true });
      mkdirSync(dir, { recursive: true });
      const out = join(dir, 'out.mjs');
      const spec = tools[tool]({ entry, out, minify, map, dir });
      const times = [], rss = [];
      let failed = null;
      for (let i = -1; i < runs; i++) {
        rmSync(out, { force: true });
        const r = run(spec, here);
        if (!r.ok) {
          failed = r.err.trim().split('\n').slice(-2).join(' | ').slice(0, 160);
          break;
        }
        if (i >= 0) {
          times.push(r.ms);
          rss.push(r.rss);
        }
      }
      let runs_ = '-';
      if (!failed && runnable.has(name)) {
        const r = spawnSync(process.execPath, [out], { encoding: 'utf8', timeout: 60000 });
        const printed = r.status === 0 ? r.stdout : null;
        if (printed !== null && reference === null) reference = printed;
        runs_ = printed === null ? 'fails' : printed === reference ? 'same' : 'DIFFERENT';
      }
      const size = failed ? null : sizeOf(dir);
      const row = { name, mode, tool, min: failed ? null : Math.min(...times), med: failed ? null : median(times), rss: failed ? null : Math.max(...rss) / 1024, size, runs: runs_, failed };
      rows.push(row);
      console.log(`${name.padEnd(9)} ${mode.padEnd(10)} ${tool.padEnd(8)} ${failed ? 'FAILED ' + failed : `med ${row.med.toFixed(0).padStart(5)} ms  min ${row.min.toFixed(0).padStart(5)} ms  rss ${row.rss.toFixed(0).padStart(4)} MB  out ${(size.bytes / 1024).toFixed(0).padStart(6)} KB  gz ${(size.gzip / 1024).toFixed(0).padStart(5)} KB  runs ${runs_}`}`);
    }
  }
}
writeFileSync(join(here, 'results.json'), JSON.stringify({ env, runs, node: process.version, rows }, null, 1));

// ---- tables ----
const names = Object.keys(workloads).filter((w) => rows.some((r) => r.name === w));
const get = (w, m, t) => rows.find((r) => r.name === w && r.mode === m && r.tool === t);
const table = (title, cell, unit) => {
  const out = [];
  out.push(markdown ? `\n#### ${title}\n` : `\n== ${title}`);
  const head = ['', ...selected];
  out.push(markdown ? `| ${head.join(' | ')} |\n|${head.map(() => '---').join('|')}|` : head.map((h, i) => (i ? h.padStart(10) : h.padEnd(10))).join(''));
  for (const w of names) {
    const cells = selected.map((t) => {
      const r = get(w, cell.mode, t);
      return !r || r.failed ? 'failed' : cell.of(r);
    });
    out.push(markdown ? `| ${[w, ...cells].join(' | ')} |` : [w.padEnd(10), ...cells.map((c) => c.padStart(10))].join(''));
  }
  return out.join('\n');
};
for (const mode of modeNames) {
  console.log(table(`${mode}: median ms (${env})`, { mode, of: (r) => r.med.toFixed(0) }));
  if (selected.includes('rathkit')) {
    const lines = selected
      .filter((t) => t !== 'rathkit')
      .map((t) => {
        const ratios = names.map((w) => [get(w, mode, t), get(w, mode, 'rathkit')]).filter(([a, b]) => a && b && !a.failed && !b.failed).map(([a, b]) => a.med / b.med);
        return `${t} ${(Math.exp(ratios.reduce((s, x) => s + Math.log(x), 0) / ratios.length)).toFixed(2)}x`;
      });
    console.log(`  geometric mean time relative to rathkit: ${lines.join(', ')}`);
  }
}
for (const mode of modeNames.filter((m) => m.startsWith('minify'))) {
  console.log(table(`${mode}: output KB / gzip KB (${env})`, { mode, of: (r) => `${(r.size.bytes / 1024).toFixed(0)}/${(r.size.gzip / 1024).toFixed(0)}` }));
  break;
}
console.log(table(`${modeNames.at(-1)}: peak memory MB`, { mode: modeNames.at(-1), of: (r) => r.rss.toFixed(0) }));
console.log(table(`${modeNames[0]}: does the output run and print the same?`, { mode: modeNames[0], of: (r) => r.runs }));
