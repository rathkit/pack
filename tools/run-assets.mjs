// Tests for the plugins that deal with images, fonts, SVG and other files, run for real from the
// bench folder's packages. Each case bundles one entry with one config and runs the result with
// Node; the lines it prints must match what is listed here, and files a plugin writes must exist.
//   node tools/run-assets.mjs
import { spawnSync } from 'node:child_process';
import { readFileSync, copyFileSync, cpSync, existsSync, mkdtempSync, readdirSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
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
  {
    name: 'vite-imagetools', entry: 'use-imagetools.js', config: 'imagetools.config.mjs', out: ['url photo-HASH.webp', 'meta 16 png', 'srcset 2'],
    // The resized copy is a real WebP, 16 pixels wide.
    verify: async (out) => {
      const file = filesUnder(out).find((f) => f.endsWith('.webp'));
      const info = file && (await sharp(file).metadata());
      return info?.format === 'webp' && info.width === 16 ? null : `the .webp file is ${JSON.stringify(info && { format: info.format, width: info.width })}`;
    },
  },
  {
    name: 'vite-plugin-image-optimizer', entry: 'use-optimizer.js', config: 'optimizer.config.mjs', out: ['string string'],
    // The PNG that was copied is smaller than the one in the project.
    verify: async (out, dir) => {
      const file = filesUnder(out).find((f) => /photo-.*\.png$/.test(f));
      return file && statSync(file).size < statSync(join(dir, 'photo.png')).size ? null : 'the copied PNG was not made smaller';
    },
  },
  // A font plugin that makes a style sheet of @font-face rules naming the project's font files (`/fonts/...` is the project's).
  { name: 'unplugin-fonts', entry: 'use-unfonts.js', config: 'unfonts.config.mjs', out: ['ok'], files: [/^Mono-Regular-.*\.woff2$/, /\.css$/] },
  // The page of a build: the tags a font plugin adds to index.html (links for Google Fonts, a preload of the copied font, and the font faces inlined).
  {
    name: 'unplugin-fonts (page)', page: 'index.html', config: 'unfonts-page.config.mjs',
    verify: (out) => {
      const html = readFileSync(join(out, 'index.html'), 'utf8');
      const wanted = [/rel="preconnect" href="https:\/\/fonts\.gstatic\.com\/"/, /href="https:\/\/fonts\.googleapis\.com\/css2\?family=Roboto/, /rel="preload" as="font" type="font\/woff2" href="\/Mono-Regular-[a-z0-9]+\.woff2"/, /<script type="module" src="\.\/use-unfonts\.js">/, /<link rel="stylesheet" href="\.\/page\.css">/, /<link rel="stylesheet" href="\.\/use-unfonts\.css">/];
      const missing = wanted.find((w) => !w.test(html));
      if (missing) return `index.html lacks ${missing}\n${html}`;
      // The linked sheet is built: its image is copied and named in it, and the script made for it is gone.
      const sheet = readFileSync(join(out, 'page.css'), 'utf8');
      if (!/url\(\.\/pic-[a-z0-9]+\.png\)/.test(sheet)) return `page.css was not built: ${sheet}`;
      if (existsSync(join(out, 'page.js'))) return 'the script made for page.css was left in the output';
      // The image the page names three ways (an icon, an `<img>`, a `url()` in a `<style>`) is copied once, and each says where.
      const named = html.match(/\.\/pic-[a-z0-9]+\.png/g) ?? [];
      if (named.length !== 3 || new Set(named).size !== 1 || !existsSync(join(out, named[0]))) return `the image was named ${JSON.stringify(named)}\n${html}`;
      return null;
    },
  },
  {
    name: 'unplugin-fonts (inlined font faces)', page: 'index.html', config: 'unfonts-inline.config.mjs',
    verify: (out) => {
      const html = readFileSync(join(out, 'index.html'), 'utf8');
      return /<style>[^<]*@font-face[^<]*font-family:\s*'?Mono'?/.test(html) ? null : `no inlined @font-face\n${html}`;
    },
  },
  // A page in the other formats: one file holds every script and sheet, and the page loads it once.
  {
    name: 'page as an iife', page: 'two-scripts.html', flags: ['--format', 'iife'], config: 'unfonts-inline.config.mjs',
    verify: (out) => {
      const html = readFileSync(join(out, 'two-scripts.html'), 'utf8');
      const scripts = html.match(/<script[^>]*><\/script>/g) ?? [];
      if (scripts.length !== 1 || !/src="\.\/bundle\.js"/.test(scripts[0])) return `the page loads ${JSON.stringify(scripts)}`;
      if ((html.match(/<link rel="stylesheet"/g) ?? []).length !== 1 || !/href="\.\/bundle\.css"/.test(html)) return `the page links ${html}`;
      return /^\(\(\) => \{/.test(readFileSync(join(out, 'bundle.js'), 'utf8')) ? null : 'bundle.js is not an iife';
    },
  },
  // CommonJS cannot run in a page.
  { name: 'page as commonjs', page: 'index.html', flags: ['--format', 'cjs'], config: 'unfonts-inline.config.mjs', failure: /cannot load CommonJS/ },
  // A font plugin that downloads the fonts of a Google Fonts style sheet and adds them to the output (needs the network).
  { name: 'vite-plugin-webfont-dl', entry: 'use-webfont.js', config: 'webfont-dl.config.mjs', out: ['ok'], network: 'https://fonts.googleapis.com/css2?family=Press+Start+2P', files: [/\.woff2$/, /^webfonts-.*\.css$/] },
  // No plugin: a font package's style sheet names its font files, which are copied and the addresses rewritten.
  { name: 'font package style sheet', entry: 'use-font.js', out: ['ok'], files: [/^roboto-mono-latin-400-normal-.*\.woff2$/, /\.css$/] },
  // No plugin: workers are scripts built on their own, with the files they import copied beside them.
  { name: 'workers', entry: 'worker-main.js', out: ['plain 42 logo-HASH.svg', 'constructor 8 logo-HASH.svg', 'inline function url worker-script-HASH.js'], files: [/^logo-.*\.svg$/, /^worker-script-.*\.js$/] },
  { name: 'rollup-plugin-copy', entry: 'nothing.js', config: 'copy.config.mjs', out: ['copied'], files: ['copied/pic.png'] },
];
const sharp = createRequire(join(bench, 'x.js'))('sharp');
// All the files under a folder.
const filesUnder = (d) => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? filesUnder(join(d, e.name)) : [join(d, e.name)]));
const tmp = mkdtempSync(join(tmpdir(), 'assets-'));
let failed = 0;
for (const c of cases) {
  if (c.network && !(await fetch(c.network, { signal: AbortSignal.timeout(5000) }).then((r) => r.ok, () => false))) {
    console.log(`skip ${c.name}: ${c.network} cannot be reached`);
    continue;
  }
  const dir = join(tmp, c.name.replace(/\W+/g, '-'));
  cpSync(join(root, 'tests', 'assets'), dir, { recursive: true });
  symlinkSync(bench, join(dir, 'node_modules'), 'dir');
  if (c.config) copyFileSync(join(dir, c.config), join(dir, 'rathkit.config.mjs'));
  const out = join(dir, 'dist');
  const file = join(out, 'bundle.mjs');
  const b = spawnSync(bin, ['bundle', c.page ?? c.entry, ...(c.page ? ['--outdir', out, ...(c.flags ?? [])] : ['--outfile', file, '--external', 'react', '--external', 'react-dom/server'])], { cwd: dir, encoding: 'utf8' });
  let problem = null;
  if (c.failure) problem = b.status !== 0 && c.failure.test(b.stderr) ? null : `expected the build to fail with ${c.failure}: ${b.status} ${b.stderr}`;
  else if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
  else if (c.page) {
    problem = c.verify ? await c.verify(out, dir) : null;
  } else if (!c.failure) {
    const r = spawnSync('node', ['--no-warnings', file], { cwd: dir, encoding: 'utf8' });
    const got = r.stdout.trim().split('\n').map((l) => l.replace(/(?<=[-.\/ ])[0-9A-Za-z]{8,32}(?=\.\w+$)/g, 'HASH'));
    if (r.status !== 0) problem = `run failed: ${r.stderr.trim()}`;
    else if (JSON.stringify(got) !== JSON.stringify(c.out)) problem = `printed ${JSON.stringify(got)}, expected ${JSON.stringify(c.out)}`;
    if (!problem && c.verify) problem = await c.verify(out, dir);
    for (const f of c.files ?? []) {
      const found = f instanceof RegExp ? existsSync(out) && filesUnder(out).some((n) => f.test(n.replace(/^.*\//, ''))) : existsSync(join(out, f)) || existsSync(join(dir, f));
      if (!problem && !found) problem = `missing file ${f}`;
    }
  }
  console.log(problem ? `FAIL ${c.name}: ${problem}` : `ok   ${c.name}`);
  if (problem) failed++;
}
console.log(failed ? `${failed} failure(s)` : `all ${cases.length} asset plugin builds agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
else console.log('kept', tmp);
process.exit(failed ? 1 : 0);
