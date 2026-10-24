// Differential test for --define: each program under tests/defines/ runs under Node with the
// defined globals set beforehand, then bundled with the same definitions, and the output must
// agree. Dead code must also be gone: a production bundle may not mention the marker `UNREACHABLE`.
//   node tools/run-defines.mjs [name...]
// A program directory holds main.js and one defines-NAME.txt per variant, KEY=VALUE per line.
import { spawnSync } from 'node:child_process';
import { readdirSync, statSync, mkdtempSync, rmSync, readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const base = join(root, 'tests', 'defines');
const only = process.argv.slice(2);
const names = readdirSync(base).filter((n) => statSync(join(base, n)).isDirectory() && (!only.length || only.includes(n)));
const tmp = mkdtempSync(join(tmpdir(), 'defines-'));
const run = (args, cwd) => {
  const r = spawnSync('node', ['--no-warnings', ...args], { cwd, encoding: 'utf8', timeout: 20000 });
  return { out: r.stdout, err: r.stderr.split('\n').find((l) => /Error/.test(l)) ?? '', code: r.status };
};
const entryOf = (dir) => readdirSync(dir).find((f) => /^main\.m?js$/.test(f));
let failed = 0, compared = 0;
for (const name of names) {
  const dir = join(base, name);
  for (const file of readdirSync(dir).filter((f) => /^defines-.*\.txt$/.test(f))) {
    const pairs = readFileSync(join(dir, file), 'utf8').split('\n').filter((l) => l.trim()).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]);
    // The reference: the same values put in place before the program runs.
    const prelude = join(tmp, `${name}-${file}.cjs`);
    writeFileSync(prelude, pairs.map(([k, v]) => `{ const path = ${JSON.stringify(k.split('.'))}; let o = globalThis; for (const p of path.slice(0, -1)) o = o[p]; o[path.at(-1)] = (0, eval)(${JSON.stringify('(' + v + ')')}); }`).join('\n'));
    const expected = run(['--require', prelude, entryOf(dir)], dir);
    if (expected.code !== 0 || expected.out.trim() === '') {
      failed++;
      console.log(`FAIL ${name}/${file}: the reference run fails (exit ${expected.code}) ${expected.err}`);
      continue;
    }
    const defineFlags = pairs.flatMap(([k, v]) => ['--define', `${k}=${v}`]);
    for (const flags of [[], ['--no-treeshake'], ['--minify'], ['--minify', '--keep-names', '--format', 'cjs'], ['--format', 'iife'], ['--minify-syntax']]) {
      const out = join(tmp, `${name}-${file}-${flags.join('')}.${flags.includes('cjs') || flags.includes('iife') ? 'cjs' : 'mjs'}`);
      const b = spawnSync(bin, ['bundle', entryOf(dir), '--outfile', out, ...defineFlags, ...flags], { cwd: dir, encoding: 'utf8' });
      let problem = null;
      if (b.status !== 0) {
        if (/Top-level await|needs a global/.test(b.stderr)) continue;
        problem = `bundler failed: ${b.stderr.trim()}`;
      } else {
        compared++;
        const got = run([out], dir);
        const text = readFileSync(out, 'utf8');
        if (got.out !== expected.out || got.code !== expected.code) problem = `output differs (${flags.join(' ')})\n--- expected\n${expected.out}${expected.err}\n--- got (exit ${got.code})\n${got.out}${got.err}`;
        else if (file === 'defines-production.txt' && !flags.includes('--no-treeshake') && /UNREACHABLE/.test(text)) problem = `dead code is still in the bundle (${flags.join(' ')})`;
        else if (name === 'env' && file === 'defines-production.txt' && /test module/.test(text)) problem = 'the test branch is still in a production bundle';
        else if (name === 'env' && file === 'defines-production.txt' && /(dev|dev-only) module/.test(text)) problem = 'a development module is in a production bundle';
        else if (name === 'env' && file === 'defines-development.txt' && /prod module/.test(text)) problem = 'the production module is in a development bundle';
      }
      if (problem) {
        failed++;
        console.log(`FAIL ${name}/${file}: ${problem}`);
        console.log(`     bundle kept at ${out}`);
      }
    }
  }
}
console.log(failed ? `${failed} failure(s)` : `all ${compared} define builds agree`);
if (!failed) rmSync(tmp, { recursive: true, force: true });
process.exit(failed ? 1 : 0);
