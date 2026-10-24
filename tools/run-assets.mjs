// Tests for the plugins that deal with images, fonts, SVG and other files, run for real from the
// bench folder's packages. Each case bundles one entry with one config and runs the result with
// Node; the lines it prints must match what is listed here, and files a plugin writes must exist.
//   node tools/run-assets.mjs
import { spawnSync } from 'node:child_process';
import { copyFileSync, cpSync, existsSync, mkdtempSync, readdirSync, rmSync, symlinkSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rathkit-pack');
const bench = join(root, 'bench', 'node_modules');
if (!existsSync(join(bench, '@rollup', 'plugin-url'))) {
  console.log('assets: skipped (run npm install in bench first)');
  process.exit(0);
}
const cases = [
  { name: '@rollup/plugin-url', entry: 'use-assets.js', config: 'url.config.mjs', out: ['svg file logo-HASH.svg', 'png file pic-HASH.png', 'font inlined'] },
  { name: 'url-loader and file-loader', entry: 'use-assets.js', config: 'webpack.config.mjs', out: ['svg inlined', 'png file HASH.png', 'font file f.HASH.woff2'] },
  { name: '@svgr/rollup', entry: 'use-component-default.jsx', config: 'svgr-rollup.config.mjs', out: ['true'] },
  { name: '@svgr/webpack', entry: 'use-component-default.jsx', config: 'webpack-svgr.config.mjs', out: ['true'] },
  { name: 'vite-plugin-svgr (?react)', entry: 'use-component.jsx', config: 'svgr-vite.config.mjs', out: ['true'] },
  { name: 'rollup-plugin-copy', entry: 'nothing.js', config: 'copy.config.mjs', out: ['copied'], files: ['copied/pic.png'] },
];
const tmp = mkdtempSync(join(tmpdir(), 'assets-'));
let failed = 0;
for (const c of cases) {
  const dir = join(tmp, c.name.replace(/\W+/g, '-'));
  cpSync(join(root, 'tests', 'assets'), dir, { recursive: true });
  symlinkSync(bench, join(dir, 'node_modules'), 'dir');
  copyFileSync(join(dir, c.config), join(dir, 'rathkit.config.mjs'));
  const out = join(dir, 'dist');
  const file = join(out, 'bundle.mjs');
  const b = spawnSync(bin, ['bundle', c.entry, '--outfile', file, '--external', 'react', '--external', 'react-dom/server'], { cwd: dir, encoding: 'utf8' });
  let problem = null;
  if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
  else {
    const r = spawnSync('node', ['--no-warnings', file], { cwd: dir, encoding: 'utf8' });
    const got = r.stdout.trim().split('\n').map((l) => l.replace(/(?<=[-.\/ ])[0-9A-Za-z]{8,32}(?=\.\w+$)/g, 'HASH'));
    if (r.status !== 0) problem = `run failed: ${r.stderr.trim()}`;
    else if (JSON.stringify(got) !== JSON.stringify(c.out)) problem = `printed ${JSON.stringify(got)}, expected ${JSON.stringify(c.out)}`;
    for (const f of c.files ?? []) if (!problem && !existsSync(join(out, f)) && !existsSync(join(dir, f))) problem = `missing file ${f}`;
  }
  console.log(problem ? `FAIL ${c.name}: ${problem}` : `ok   ${c.name}`);
  if (problem) failed++;
}
console.log(failed ? `${failed} failure(s)` : `all ${cases.length} asset plugin builds agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
else console.log('kept', tmp);
process.exit(failed ? 1 : 0);
