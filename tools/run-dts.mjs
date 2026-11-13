// Tests for --dts: a small TypeScript project is bundled with declarations, and a consumer file is type-checked against them (tsc from the
// test-deps folder: pnpm install --dir test-deps).
//   node tools/run-dts.mjs
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync, realpathSync, cpSync, readdirSync, copyFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { testModules, installHint } from './lib/test-deps.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, 'target', 'release', 'rkp');
const typescript = join(testModules, 'typescript');
if (!existsSync(typescript)) {
  console.log('dts: typescript is not installed (' + installHint + '); skipped');
  process.exit(0);
}
const tmp = mkdtempSync(join(tmpdir(), 'dts-'));
let failed = 0;
const outerCheck = (ok, what, detail = '') => {
  if (!ok) {
    failed++;
    console.log(`FAIL ${what}${detail ? '\n' + detail : ''}`);
  }
};
const check = outerCheck;
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


// Declarations are merged with TypeScript 5's API, and with a reader of our own when the project's typescript is version 7 (it has no API).
const ts7 = join(testModules, 'ts7');
for (const [tag, compiler] of [['', typescript], [' (TypeScript 7)', ts7]]) {
  if (!existsSync(compiler)) continue;
  const check = (ok, what, detail = '') => outerCheck(ok, what + tag, detail);
  {
    const lib = join(tmp, 'lib' + tag.length);
    cpSync(join(root, 'tests', 'dts-bundle', 'lib'), lib, { recursive: true });
    symlinkSync(realpathSync(compiler), join(lib, 'node_modules', 'typescript'), 'dir');
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


  // Re-exports written with the .ts extension (allowImportingTsExtensions) are followed.
  {
    const lib = join(tmp, 'tsext' + tag.length);
    cpSync(join(root, 'tests', 'dts-bundle', 'ts-ext'), lib, { recursive: true });
    mkdirSync(join(lib, 'node_modules'), { recursive: true });
    symlinkSync(realpathSync(compiler), join(lib, 'node_modules', 'typescript'), 'dir');
    const built = run(lib, ['bundle', 'src/index.ts', '--outdir', 'dist', '--dts-bundle']);
    check(built.status === 0, 'a build with --dts-bundle of a library that imports with .ts extensions', built.stderr + built.stdout);
    const merged = existsSync(join(lib, 'dist', 'index.d.ts')) ? readFileSync(join(lib, 'dist', 'index.d.ts'), 'utf8') : '';
    check(/\bgreet\b/.test(merged) && /\banswer\b/.test(merged) && /\bGreeting\b/.test(merged) && !/from '\.\//.test(merged), 'what a .ts re-export names is merged in', merged);
  }

  // And a library with the awkward corners: default exports, a default re-export, `declare global`, a module augmentation, a const enum.
  {
    const lib = join(tmp, 'edge' + tag.length);
    cpSync(join(root, 'tests', 'dts-bundle', 'edge'), lib, { recursive: true });
    symlinkSync(realpathSync(compiler), join(lib, 'node_modules', 'typescript'), 'dir');
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


}

// TypeScript 7 is the native compiler: --dts and --dts-dir write the same declarations through its command line.
if (existsSync(ts7)) {
  const native = join(tmp, 'native7');
  mkdirSync(join(native, 'node_modules'), { recursive: true });
  symlinkSync(realpathSync(ts7), join(native, 'node_modules', 'typescript'), 'dir');
  cpSync(join(project, 'src'), join(native, 'src'), { recursive: true });
  cpSync(join(project, 'tsconfig.json'), join(native, 'tsconfig.json'));
  const r7 = run(native, ['bundle', 'src/index.ts', 'src/plain.js', '--outdir', 'dist', '--dts']);
  check(r7.status === 0, 'TypeScript 7 writes declarations', r7.stderr);
  const names = existsSync(join(native, 'dist')) ? readdirSync(join(native, 'dist')).filter((f) => f.endsWith('.d.ts')).sort() : [];
  check(names.join() === 'index.d.ts,plain.d.ts,util.d.ts', 'with the layout TypeScript 5 gives (from the folder the sources share)', names.join());
  const index7 = existsSync(join(native, 'dist', 'index.d.ts')) ? readFileSync(join(native, 'dist', 'index.d.ts'), 'utf8') : '';
  check(/export declare function add\(a: number, b: number\): number;/.test(index7) && /class Greeter/.test(index7), 'and the declarations are the ones TypeScript 5 writes', index7.slice(0, 300));
  write(native, 'src/util.ts', 'export interface Point { x: number; y: number }\nexport const origin: Point = { x: "no", y: 0 };\n');
  const bad = run(native, ['bundle', 'src/index.ts', 'src/plain.js', '--outdir', 'dist2', '--dts']);
  check(bad.status === 0 && /util\.ts:2: .*not assignable/.test(bad.stderr + bad.stdout), 'a type error is reported and the declarations are written all the same', bad.stderr);
} else {
  console.log('dts: ts7 is not installed; the TypeScript 7 checks are skipped');
}

// A typescript package of version 7 without its command line has no way to write declarations: --dts says so and names the version (issue #71).
{
  const native = join(tmp, 'native');
  write(native, 'node_modules/typescript/package.json', JSON.stringify({ name: 'typescript', version: '7.0.2', main: 'index.js' }));
  write(native, 'node_modules/typescript/index.js', 'module.exports = {};\n');
  write(native, 'src/greet.ts', 'export const greet = (name: string): string => "hi " + name;\n');
  const r = run(native, ['bundle', 'src/greet.ts', '--outdir', 'dist', '--dts']);
  check(r.status !== 0 && /typescript package 7\.0\.2 has no JavaScript API/.test(r.stderr), 'TypeScript 7 gets a clear message, not a crash', r.stderr);
  check(!/fileExists|TypeError/.test(r.stderr), 'and no TypeError');
}

try { rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {} // Windows may still hold a file of a process that just ended
if (failed) {
  console.log(`${failed} failure(s)`);
  process.exit(1);
}
console.log('dts: all checks pass');
