// Differential test for the bundler: runs every program under tests/fixtures/ with Node as it is,
// then bundled (with and without tree-shaking), and requires identical output and exit status.
//   node tools/run-fixtures.mjs [name...]
import { execFileSync, spawnSync } from 'node:child_process';
import { readdirSync, statSync, mkdtempSync, rmSync, existsSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const fixtures = join(root, 'tests', 'fixtures');
const only = process.argv.slice(2);
const names = readdirSync(fixtures).filter((n) => statSync(join(fixtures, n)).isDirectory() && (!only.length || only.includes(n)));
const run = (file, cwd) => {
  const r = spawnSync('node', ['--no-warnings', file], { cwd, encoding: 'utf8', timeout: 20000 });
  return { out: r.stdout, err: r.stderr.split('\n').find((l) => /Error/.test(l)) ?? '', code: r.status };
};
const tmp = mkdtempSync(join(tmpdir(), 'fixtures-'));
let failed = 0;
for (const name of names) {
  const dir = join(fixtures, name);
  const entry = ['main.ts', 'main.tsx', 'main.js'].find((f) => existsSync(join(dir, f)));
  // A fixture Node cannot run states its expected output instead.
  const stated = join(dir, 'expected.txt');
  const expected = existsSync(stated) ? { out: readFileSync(stated, 'utf8'), err: '', code: 0 } : run(entry, dir);
  // A reference that fails to run, or prints nothing, would make every comparison pass for
  // the wrong reason.
  if (!existsSync(join(dir, 'expect-failure')) && (expected.code !== 0 || expected.out.trim() === '')) {
    failed++;
    console.log(`FAIL ${name}: the unbundled program itself does not run cleanly (exit ${expected.code}): ${expected.err}`);
    continue;
  }
  for (const flags of [[], ['--no-treeshake'], ['--format', 'cjs'], ['--format', 'iife'], ['--splitting'], ['--minify'], ['--minify', '--keep-names'], ['--minify', '--keep-names', '--format', 'cjs'], ['--minify', '--keep-names', '--format', 'iife'], ['--minify', '--keep-names', '--splitting'], ['--sourcemap'], ['--sourcemap', '--minify', '--keep-names'], ['--sourcemap=inline', '--minify', '--keep-names', '--format', 'cjs'], ['--sourcemap', '--minify', '--keep-names', '--splitting']]) {
    // A program that prints function or class names only agrees when they are kept.
    if (flags.includes('--minify') && !flags.includes('--keep-names') && existsSync(join(dir, 'prints-names'))) continue;
    const label = flags.join('').replace(/--/g, '');
    const out = join(tmp, `${name}${label ? '-' + label : ''}.${flags.includes('cjs') || flags.includes('iife') ? 'cjs' : 'mjs'}`);
    const split = flags.includes('--splitting');
    const outDir = out.replace(/\.[a-z]+$/, '') + '-chunks';
    const b = spawnSync(bin, ['bundle', entry, ...(split ? ['--outdir', outDir] : ['--outfile', out]), ...flags], { cwd: dir, encoding: 'utf8' });
    // Some programs cannot be written in these formats at all.
    if (b.status !== 0 && /Top-level await|needs a global/.test(b.stderr)) continue;
    let problem = null;
    if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim()}`;
    else {
      const got = run(split ? join(outDir, entry.replace(/\.[a-z]+$/, '.js')) : out, dir);
      if (got.out !== expected.out || got.code !== expected.code) {
        problem = `output differs${flags.length ? ` (${flags.join(' ')})` : ''}\n--- expected (exit ${expected.code})\n${expected.out}${expected.err}\n--- got (exit ${got.code})\n${got.out}${got.err}`;
      }
    }
    // Every map must agree with the text on both ends (see check-sourcemap.mjs): exactly for
    // JavaScript, and for TypeScript apart from what lowering rewrote.
    if (!problem && flags.some((f) => f.startsWith('--sourcemap'))) {
      const files = split ? readdirSync(outDir).filter((f) => f.endsWith('.js')).map((f) => join(outDir, f)) : [out];
      for (const f of files) {
        const c = spawnSync('node', [join(root, 'tools', 'check-sourcemap.mjs'), f, '--quiet', '--lenient'], { encoding: 'utf8' });
        if (c.status !== 0) problem = `source map check failed for ${f}\n${c.stdout}${c.stderr}`;
      }
    }
    if (problem) {
      failed++;
      console.log(`FAIL ${name}: ${problem}`);
      console.log(`     bundle kept at ${out}`);
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${names.length} fixtures agree`);
if (!failed) try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} // Windows may still hold a file of a process that just ended
process.exit(failed ? 1 : 0);
