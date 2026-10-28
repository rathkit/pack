// Tests for --dts: a small TypeScript project is bundled with declarations, and a consumer file is type-checked against them (tsc from the
// bench folder: pnpm install --dir bench).
//   node tools/run-dts.mjs
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync, realpathSync, cpSync, readdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const typescript = join(root, 'bench', 'node_modules', 'typescript');
if (!existsSync(typescript)) {
  console.log('dts: typescript is not installed (pnpm install --dir bench); skipped');
  process.exit(0);
}
const tmp = mkdtempSync(join(tmpdir(), 'dts-'));
let failed = 0;
const check = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n' + detail : ''}`);
  }
};
const write = (dir, file, text) => {
  mkdirSync(dirname(join(dir, file)), { recursive: true });
  writeFileSync(join(dir, file), text);
};
const run = (cwd, args) => spawnSync(bin, args, { cwd, encoding: 'utf8' });

// A project with TypeScript in its node_modules.
const project = join(tmp, 'app');
mkdirSync(join(project, 'node_modules'), { recursive: true });
symlinkSync(realpathSync(typescript), join(project, 'node_modules', 'typescript'), 'dir');
write(project, 'tsconfig.json', JSON.stringify({ compilerOptions: { target: 'es2020', module: 'esnext', moduleResolution: 'bundler', strict: true } }));
write(project, 'src/index.ts', "export * from './util';\nexport function add(a: number, b: number): number { return a + b; }\nexport default class Greeter { constructor(public name: string) {} greet(): string { return 'hi ' + this.name; } }\n");
write(project, 'src/util.ts', 'export interface Point { x: number; y: number }\nexport const origin: Point = { x: 0, y: 0 };\n');
write(project, 'src/plain.js', '/** @param {number} n @returns {string} */\nexport function label(n) { return "n" + n; }\n');

// Declarations next to the output, from the folder the sources share.
let r = run(project, ['bundle', 'src/index.ts', 'src/plain.js', '--outdir', 'dist', '--dts']);
check(r.status === 0, 'build with --dts', r.stderr + r.stdout);
for (const f of ['index.d.ts', 'util.d.ts', 'plain.d.ts']) check(existsSync(join(project, 'dist', f)), `dist/${f} is written`, r.stdout);
check(/declare function add\(a: number, b: number\): number/.test(readFileSync(join(project, 'dist', 'index.d.ts'), 'utf8')), 'the exported function is described');
check(/label\(n: number\): string/.test(readFileSync(join(project, 'dist', 'plain.d.ts'), 'utf8')), 'JSDoc types of a JavaScript entry are used');

// A consumer is checked against them: the right use passes, the wrong one fails.
const consumer = (body) => {
  write(project, 'consumer/main.ts', `import Greeter, { add, origin } from '../dist/index.js';\nimport { label } from '../dist/plain.js';\n${body}\n`);
  return spawnSync('node', [join(typescript, 'bin', 'tsc'), '--noEmit', '--strict', '--module', 'esnext', '--moduleResolution', 'bundler', '--target', 'es2020', 'consumer/main.ts'], { cwd: project, encoding: 'utf8' });
};
check(consumer("const n: number = add(1, 2); const s: string = new Greeter('a').greet() + label(origin.x);").status === 0, 'a correct consumer type-checks');
check(consumer("const bad: string = add(1, 2);").status !== 0, 'a wrong consumer is rejected');

// --dts-dir puts them elsewhere.
r = run(project, ['bundle', 'src/index.ts', '--outfile', 'out/main.js', '--dts-dir', 'types']);
check(r.status === 0 && existsSync(join(project, 'types', 'index.d.ts')) && !existsSync(join(project, 'out', 'index.d.ts')), '--dts-dir', r.stderr + r.stdout);

// Without TypeScript the build says so, rather than writing nothing.
const bare = join(tmp, 'bare');
write(bare, 'src/index.ts', 'export const x: number = 1;\n');
r = run(bare, ['bundle', 'src/index.ts', '--outdir', 'dist', '--dts']);
check(r.status !== 0 && /typescript package is not installed/.test(r.stderr), 'a missing typescript is an error', r.stderr);
check(existsSync(join(bare, 'dist', 'bundle.js')), 'the bundle itself is still written');


// --dts-bundle: one declaration file for the entry, with everything it reaches merged in.
{
  const lib = join(tmp, 'lib');
  cpSync(join(root, 'tests', 'dts-bundle', 'lib'), lib, { recursive: true });
  symlinkSync(realpathSync(typescript), join(lib, 'node_modules', 'typescript'), 'dir');
  const built = run(lib, ['bundle', 'src/index.ts', '--outdir', 'dist', '--dts-bundle', '--external', 'ext-pkg']);
  check(built.status === 0, 'a build with --dts-bundle', built.stderr + built.stdout);
  const files = existsSync(join(lib, 'dist')) ? readdirSync(join(lib, 'dist')).filter((f) => f.endsWith('.d.ts')).sort() : [];
  check(files.join() === 'index.d.ts', 'one declaration file for the entry', files.join());
  const merged = existsSync(join(lib, 'dist', 'index.d.ts')) ? readFileSync(join(lib, 'dist', 'index.d.ts'), 'utf8') : '';
  check(!/from '\.\.?\//.test(merged) && !/import\("\.\.?\//.test(merged), 'with no import of another file of the library', merged.slice(0, 300));
  check(/^import .* from 'ext-pkg';/m.test(merged) && /export \{ makeExt \} from 'ext-pkg';/.test(merged), 'and what comes from a package stays an import and a re-export');
  check((merged.match(/\binterface Item\b|\binterface Item\$\d\b/g) ?? []).length === 3, 'the three things named Item are told apart', (merged.match(/interface Item\S*/g) ?? []).join());
  check(/export \{[^}]*\bdescribe\b[^}]*\};/.test(merged), 'and the exports are listed');
  mkdirSync(join(lib, 'consumer'), { recursive: true });
  copyFileSync(join(root, 'tests', 'dts-bundle', 'consumer.ts'), join(lib, 'consumer', 'main.ts'));
  const compile = spawnSync('node', [join(typescript, 'bin', 'tsc'), '--noEmit', '--strict', '--module', 'esnext', '--moduleResolution', 'bundler', '--target', 'es2020', 'consumer/main.ts'], { cwd: lib, encoding: 'utf8' });
  check(compile.status === 0, 'a strict consumer type-checks against it, and its wrong uses are errors', compile.stdout);
}


// And a library with the awkward corners: default exports, a default re-export, `declare global`, a module augmentation, a const enum.
{
  const lib = join(tmp, 'edge');
  cpSync(join(root, 'tests', 'dts-bundle', 'edge'), lib, { recursive: true });
  symlinkSync(realpathSync(typescript), join(lib, 'node_modules', 'typescript'), 'dir');
  const built = run(lib, ['bundle', 'src/index.ts', '--outdir', 'dist', '--dts-bundle', '--external', 'ext-pkg']);
  check(built.status === 0, 'a build with --dts-bundle of a library with augmentations', built.stderr + built.stdout);
  const merged = existsSync(join(lib, 'dist', 'index.d.ts')) ? readFileSync(join(lib, 'dist', 'index.d.ts'), 'utf8') : '';
  check(/declare global \{/.test(merged) && /declare module 'ext-pkg' \{/.test(merged), 'declare global and a module augmentation are kept', merged.slice(0, 200));
  check(/export \{[^}]*\bThing as default\b[^}]*\}|export default Thing/.test(merged), 'and so is the default export');
  mkdirSync(join(lib, 'consumer'), { recursive: true });
  copyFileSync(join(lib, 'consumer.ts'), join(lib, 'consumer', 'main.ts'));
  const compile = spawnSync('node', [join(typescript, 'bin', 'tsc'), '--noEmit', '--strict', '--module', 'esnext', '--moduleResolution', 'bundler', '--target', 'es2020', 'consumer/main.ts'], { cwd: lib, encoding: 'utf8' });
  check(compile.status === 0, 'a strict consumer type-checks against it', compile.stdout);
}

rmSync(tmp, { recursive: true, force: true });
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
console.log('dts: all checks pass');
