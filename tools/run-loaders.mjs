// Tests for loaders (text, base64, dataurl, binary, file, empty) and style sheets. Each program under
// tests/loaders/ is bundled in several output modes and run with Node; what it prints must equal
// expected.txt. Style sheet output is compared with expected.css / expected.min.css when present, and every
// file a sheet's url() points to must exist next to it.
//   node tools/run-loaders.mjs [name...]
// flags.txt holds extra command line flags for the program.
import { spawnSync } from 'node:child_process';
import { readdirSync, statSync, mkdtempSync, rmSync, readFileSync, existsSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

// Every segment of a style sheet's map must lead from a token in the output to the same token in its source
// (a local class name carries a prefix in the output, so one may end with the other).
const decodeMappings = (mappings) => {
  const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const segments = [];
  let source = 0, sline = 0, scol = 0;
  mappings.split(';').forEach((line, generatedLine) => {
    let col = 0;
    for (const raw of line.split(',')) {
      if (!raw) continue;
      const numbers = [];
      let value = 0, shift = 0;
      for (const ch of raw) {
        const digit = B64.indexOf(ch);
        value |= (digit & 31) << shift;
        if (digit & 32) shift += 5;
        else { numbers.push(value & 1 ? -(value >> 1) : value >> 1); value = 0; shift = 0; }
      }
      col += numbers[0];
      if (numbers.length >= 4) { source += numbers[1]; sline += numbers[2]; scol += numbers[3]; segments.push({ line: generatedLine, col, source, sline, scol }); }
    }
  });
  return segments;
};
const checkCssMap = (cssFile, dir) => {
  const mapFile = cssFile + '.map';
  if (!existsSync(mapFile)) return 'no .css.map was written';
  const map = JSON.parse(readFileSync(mapFile, 'utf8'));
  const out = readFileSync(cssFile, 'utf8').split('\n');
  if (!readFileSync(cssFile, 'utf8').includes('sourceMappingURL=')) return 'the sheet does not point to its map';
  const segments = decodeMappings(map.mappings);
  if (!segments.length) return 'the css map has no segments';
  const token = (line, col) => (line?.slice(col).match(/^[\w-]+/)?.[0] ?? line?.[col]);
  for (const s of segments) {
    const content = map.sourcesContent?.[s.source];
    if (content === undefined) return 'the css map has no sources content';
    const here = token(out[s.line], s.col), there = token(content.split('\n')[s.sline], s.scol);
    if (!here || !there || !(here.endsWith(there) || there.endsWith(here))) return `css map segment at ${s.line + 1}:${s.col + 1} says ${JSON.stringify(here)} came from ${JSON.stringify(there)} (${map.sources[s.source]}:${s.sline + 1}:${s.scol + 1})`;
  }
  return null;
};

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const base = join(root, 'tests', 'loaders');
const only = process.argv.slice(2);
const update = process.env.UPDATE_SNAPSHOTS === '1';
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const tmp = mkdtempSync(join(tmpdir(), 'loaders-'));
let failed = 0, builds = 0;
for (const name of names) {
  const dir = join(base, name);
  const extra = existsSync(join(dir, 'flags.txt')) ? readFileSync(join(dir, 'flags.txt'), 'utf8').split(/\s+/).filter(Boolean) : [];
  const expected = existsSync(join(dir, 'expected.txt')) ? readFileSync(join(dir, 'expected.txt'), 'utf8') : null;
  for (const flags of [[], ['--minify'], ['--format', 'cjs'], ['--format', 'iife'], ['--minify-whitespace', '--sourcemap'], ['--sourcemap'], ['--minify', '--sourcemap']]) {
    const out = join(tmp, `${name}-${flags.join('') || 'plain'}`);
    const file = join(out, flags.includes('cjs') || flags.includes('iife') ? 'bundle.cjs' : 'bundle.mjs');
    const b = spawnSync(bin, ['bundle', 'main.js', '--outfile', file, ...extra, ...flags], { cwd: dir, encoding: 'utf8' });
    builds++;
    let problem = null;
    if (b.status !== 0) {
      if (/Top-level await|needs a global/.test(b.stderr)) continue;
      problem = `bundler failed: ${b.stderr.trim()}`;
    } else {
      if (expected !== null) {
        const r = spawnSync('node', ['--no-warnings', file], { cwd: dir, encoding: 'utf8', env: { ...process.env, FIXTURE_DIR: dir, OUT_DIR: out } });
        if (r.stdout !== expected || r.status !== 0) problem = `output differs (${flags.join(' ')})\n--- expected\n${expected}--- got (exit ${r.status})\n${r.stdout}${r.stderr}`;
      }
      const css = file.replace(/\.[cm]js$/, '.css');
      for (const [snapshot, wanted] of [['expected.css', !flags.some((f) => f.startsWith('--minify')) && !flags.includes('--format') && !flags.includes('--sourcemap')], ['expected.min.css', flags.includes('--minify') && !flags.includes('--sourcemap')]]) {
        if (!wanted || problem || !readdirSync(dir).some((f) => f.endsWith('.css') && !f.startsWith('expected'))) continue;
        const path = join(dir, snapshot);
        if (!existsSync(css)) { problem = 'no .css file was written'; break; }
        const got = readFileSync(css, 'utf8');
        if (update) writeFileSync(path, got);
        else if (!existsSync(path)) problem = `${snapshot} is missing`;
        else if (readFileSync(path, 'utf8') !== got) problem = `${snapshot} differs\n--- expected\n${readFileSync(path, 'utf8')}--- got\n${got}`;
      }
      if (!problem && name === 'values' && readdirSync(out).some((f) => f.startsWith('unused-'))) problem = 'a copied file nobody uses was written';
      if (!problem && flags.includes('--sourcemap') && existsSync(css)) problem = checkCssMap(css, dir);
      if (!problem && existsSync(css)) {
        for (const m of readFileSync(css, 'utf8').matchAll(/url\(("?)(\.\/[^")]+)\1\)/g)) {
          if (!existsSync(join(out, m[2]))) problem = `the sheet points to ${m[2]}, which was not written`;
        }
      }
    }
    if (problem) {
      failed++;
      console.log(`FAIL ${name} [${flags.join(' ')}]: ${problem}`);
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${builds} loader builds agree`);
if (!failed) try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} // Windows may still hold a file of a process that just ended
process.exit(failed ? 1 : 0);
