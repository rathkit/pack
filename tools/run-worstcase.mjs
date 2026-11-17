// Worst-case inputs for time and memory (roadmap 3.4): graphs of 50,000 modules in four shapes, one 16 MB module and one barrel of
// 5,000 re-exports. Each is built plain, minified and minified with a source map; the output runs under Node and prints what the
// generator computed, and the peak resident size of the build stays under the bound written in docs/memory.md.
//   node tools/run-worstcase.mjs [--keep] [--modules N] [case...]
// The peak resident size is read from /usr/bin/time (Linux); where it is not there only the output is checked.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const args = process.argv.slice(2);
const keep = args.includes('--keep');
const at = args.indexOf('--modules');
const N = at >= 0 ? Number(args[at + 1]) : 50000;
const only = args.filter((a, i) => !a.startsWith('--') && args[i - 1] !== '--modules');

// The bound: peak resident size in MB for a build of N modules of about 150 bytes each. The same formula is in docs/memory.md.
// Measured with a release build at 0.6 GB for 50,000 modules; the bound leaves room for a slower allocator and a bigger machine.
const boundMB = (modules) => 64 + Math.ceil((modules * 16) / 1024);
// One module is bound by its size instead: the syntax tree, the scan and the minifier's second tree are each a multiple of the text.
// A 28 MB module measured at 1.2 GB with a source map.
const hugeBoundMB = (bytes) => 64 + Math.ceil((bytes / 1048576) * 48);

const MOD = 1000003;
// Every module exports `v<i>`, computed from the modules it imports, so the answer is the same however the bundler orders and
// hoists the code, and no call is nested deeper than one module.
const writeGraph = (dir, n, depsOf) => {
  mkdirSync(dir, { recursive: true });
  const value = new Array(n);
  const order = [];
  const seen = new Uint8Array(n);
  // Iterative depth-first post-order from module 0, so a chain of 50,000 does not overflow the generator's own stack.
  const stack = [[0, 0]];
  seen[0] = 1;
  while (stack.length) {
    const top = stack[stack.length - 1];
    const deps = depsOf(top[0]);
    if (top[1] < deps.length) {
      const d = deps[top[1]++];
      if (!seen[d]) {
        seen[d] = 1;
        stack.push([d, 0]);
      }
    } else {
      order.push(top[0]);
      stack.pop();
    }
  }
  for (const i of order) {
    const deps = depsOf(i);
    // A cycle's back edge reads a module that has not run: its value is `undefined` there, so ring cases do not use `value` of a later one.
    value[i] = deps.reduce((sum, d) => sum + (value[d] ?? 0), i) % MOD;
  }
  for (let i = 0; i < n; i++) {
    if (!seen[i]) continue;
    const deps = depsOf(i);
    let text = '';
    for (const d of deps) text += `import { v${d} } from './m${d}.js';\n`;
    text += `export const v${i} = (${[String(i), ...deps.map((d) => `v${d}`)].join(' + ')}) % ${MOD};\n`;
    writeFileSync(join(dir, `m${i}.js`), text);
  }
  writeFileSync(join(dir, 'main.js'), "import { v0 } from './m0.js';\nconsole.log(v0);\n");
  return String(value[0]);
};

const cases = {
  // Each module imports up to three later ones.
  wide: (dir, n) => writeGraph(dir, n, (i) => [i * 3 + 1, i * 3 + 2, i * 3 + 3].filter((d) => d < n)),
  // One long chain: module i imports module i + 1. Anything that recurses once per module overflows a stack here.
  chain: (dir, n) => writeGraph(dir, n, (i) => (i + 1 < n ? [i + 1] : [])),
  // Every module imports the next, and the last imports the first (a cycle of the whole graph is one strongly connected component).
  // The values are not defined across the cycle's back edge, so this one only checks that the build finishes and the output runs.
  ring: null,
  // One module that imports every other: a fan-out of N - 1 from a single file.
  star: (dir, n) => writeGraph(dir, n, (i) => (i === 0 ? Array.from({ length: n - 1 }, (_, k) => k + 1) : [])),
  // A single module of about 16 MB: 200,000 functions and one call of each.
  huge: (dir, n) => {
    mkdirSync(dir, { recursive: true });
    const count = Math.max(1000, Math.round((n / 50000) * 200000));
    const parts = [];
    let sum = 0;
    for (let i = 0; i < count; i++) {
      parts.push(`function f${i}(a, b) { if (a > ${i % 7}) { return (a * ${(i % 13) + 1} + b) % ${MOD}; } return (b + ${i}) % ${MOD}; }`);
      sum = (((sum * 31) % MOD) + (((i % 7) < 3 ? ((3 * ((i % 13) + 1) + 5) % MOD) : (5 + i) % MOD))) % MOD;
    }
    let calls = 'let acc = 0;\n';
    for (let i = 0; i < count; i++) calls += `acc = (acc * 31 + f${i}(3, 5)) % ${MOD};\n`;
    writeFileSync(join(dir, 'main.js'), `${parts.join('\n')}\n${calls}console.log(acc);\n`);
    return String(sum);
  },
  // A barrel: index.js re-exports 5,000 modules with `export *`, and the entry uses two of them.
  barrel: (dir, n) => {
    mkdirSync(dir, { recursive: true });
    const count = Math.min(5000, n);
    let index = '';
    for (let i = 0; i < count; i++) {
      writeFileSync(join(dir, `b${i}.js`), `export const b${i} = ${i};\nexport function g${i}(x) { return x + ${i}; }\n`);
      index += `export * from './b${i}.js';\n`;
    }
    writeFileSync(join(dir, 'index.js'), index);
    writeFileSync(join(dir, 'main.js'), `import { b3, g${count - 1} } from './index.js';\nconsole.log(b3 + g${count - 1}(1));\n`);
    return String(3 + count);
  },
};
// The ring is built by hand: a function of the cycle needs no value from across the back edge.
cases.ring = (dir, n) => {
  mkdirSync(dir, { recursive: true });
  for (let i = 0; i < n; i++) {
    const next = (i + 1) % n;
    writeFileSync(join(dir, `m${i}.js`), `import { w${next} } from './m${next}.js';\nexport function w${i}() { return ${i} + (${i} < ${n - 1} ? w${next}() : 0); }\n`);
  }
  // A recursive call per module would nest 50,000 deep, so the entry only asks for the first few steps.
  writeFileSync(join(dir, 'main.js'), `import { w${n - 1} } from './m${n - 1}.js';\nconsole.log(w${n - 1}());\n`);
  return String(n - 1);
};

const tmp = mkdtempSync(join(tmpdir(), 'worstcase-'));
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n' + detail : ''}`);
  }
};
const haveTime = existsSync('/usr/bin/time');
const modes = { plain: [], minify: ['--minify'], 'minify+map': ['--minify', '--sourcemap'] };
const rows = [];
for (const [name, make] of Object.entries(cases)) {
  if (only.length && !only.includes(name)) continue;
  const dir = join(tmp, name);
  const expected = make(dir, N);
  const modules = name === 'huge' ? 1 : name === 'barrel' ? Math.min(5000, N) + 2 : N;
  for (const [mode, flags] of Object.entries(modes)) {
    const out = join(tmp, `${name}-${mode.replace('+', '-')}.js`);
    const cmd = ['bundle', join(dir, 'main.js'), '--outfile', out, ...flags];
    const started = process.hrtime.bigint();
    const r = haveTime ? spawnSync('/usr/bin/time', ['-f', 'RSS %M', bin, ...cmd], { encoding: 'utf8', maxBuffer: 1 << 28 }) : spawnSync(bin, cmd, { encoding: 'utf8', maxBuffer: 1 << 28 });
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    const label = `${name} ${mode}`;
    check(r.status === 0, `${label}: the build succeeds`, (r.stderr ?? '').split('\n').slice(-5).join('\n'));
    if (r.status !== 0) continue;
    const rss = haveTime ? Number(/RSS (\d+)/.exec(r.stderr)?.[1]) / 1024 : NaN;
    const ran = spawnSync(process.execPath, ['--stack-size=4000', out], { encoding: 'utf8' });
    check(ran.status === 0 && ran.stdout.trim() === expected, `${label}: the output runs and prints ${expected}`, `${ran.stdout.trim()} ${ran.stderr.split('\n')[0]}`);
    const bound = name === 'huge' ? hugeBoundMB(statSync(join(dir, 'main.js')).size) : boundMB(modules);
    if (haveTime) check(rss <= bound, `${label}: peak ${rss.toFixed(0)} MB is under the bound of ${bound} MB`);
    rows.push(`${label.padEnd(20)} ${String(modules).padStart(6)} modules  ${ms.toFixed(0).padStart(6)} ms  ${Number.isNaN(rss) ? 'n/a' : rss.toFixed(0).padStart(5)} MB (bound ${bound})`);
  }
}
console.log(rows.join('\n'));
if (!keep) rmSync(tmp, { recursive: true, force: true });
else console.log(`kept in ${tmp}`);
console.log(failed ? `${failed} failed` : 'all passed');
process.exit(failed ? 1 : 0);
