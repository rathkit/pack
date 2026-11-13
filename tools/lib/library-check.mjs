// Library mode against the real thing: a corpus project's own build is the package its authors publish, so the library is built by rkp in
// both formats (bundled, and with one file for each module) and what `import` and `require` of the result give is compared with what they
// give for the published package, subpath by subpath. Only the formats the published package ships are compared (a CommonJS-only package
// has no ES module build to match). Used by tools/run-corpus.mjs for a project with a `library` block:
//   package   the name on npm (default: the name in the project's package.json)
//   version   the release to compare with (default: the version in the project's package.json)
//   entries   { "<subpath>": "<source entry>" }
//   root      the folder --preserve-modules starts from (default: the entry's folder)
//   peers     packages the published one needs installed beside it
//   typesIgnore  names the published declarations export that the source does not (helper types their declaration bundler exports)
//   expectFail   a known gap and its reason: it is expected to fail, and a pass says to remove it
//   flags     more options for rkp
//   types     also build with --dts-bundle and compare the names the declarations export (TypeScript from the test-deps folder)
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { testModules } from './test-deps.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const probe = path.join(here, 'probe-exports.mjs');
const sh = (cmd, argv, cwd, limit = 120) => {
  const r = spawnSync(cmd, argv, { cwd, encoding: 'utf8', timeout: limit * 1000, maxBuffer: 64 << 20 });
  return { ok: r.status === 0, out: (r.stdout || '').trim(), err: `${r.stdout || ''}${r.stderr || ''}`.trim() };
};
const lastLine = (text) => text.split('\n').pop();
const without = (names) => Object.fromEntries(Object.entries(names).filter(([k, t]) => k !== '__esModule' && k !== 'module.exports' && t !== 'undefined'));
const lacking = (ours, published) => Object.entries(without(published)).filter(([k, t]) => ours[k] !== t).map(([k, t]) => `${k}:${t} (ours: ${ours[k]})`);

export function libraryCheck(p, where, rkp, scratchRoot) {
  const result = libraryRun(p, where, rkp, scratchRoot);
  if (!p.library.expectFail) return result;
  return result.ok ? { ok: false, step: 'library', detail: `this passes now, so remove its expectFail (${p.library.expectFail})` } : { ok: true, expected: p.library.expectFail };
}

function libraryRun(p, where, rkp, scratchRoot) {
  const lib = p.library;
  const manifest = JSON.parse(fs.readFileSync(path.join(where, 'package.json'), 'utf8'));
  const name = lib.package ?? manifest.name;
  const version = lib.version ?? manifest.version;
  const scratch = path.join(scratchRoot, `published-${p.name}`);
  fs.rmSync(scratch, { recursive: true, force: true });
  fs.mkdirSync(scratch, { recursive: true });
  fs.writeFileSync(path.join(scratch, 'package.json'), '{"name":"scratch","private":true}\n');
  let r = sh('pnpm', ['add', `${name}@${version}`, ...(lib.peers ?? []), '--ignore-scripts'], scratch, 300);
  if (!r.ok) return { ok: false, step: 'published', detail: `pnpm add ${name}@${version}: ${r.err.split('\n').slice(-4).join('\n')}` };
  const entries = Object.entries(lib.entries);
  const sources = entries.map(([, e]) => e);
  const problems = [];
  const variants = [
    { label: 'bundled', dir: 'rkp-lib', args: [], file: (src) => path.basename(src).replace(/\.[^.]+$/, '') },
    {
      label: 'preserved',
      dir: 'rkp-lib-pm',
      args: ['--preserve-modules', ...(lib.root ? ['--preserve-modules-root', lib.root] : [])],
      file: (src) => path.relative(lib.root ?? path.dirname(sources[0]), src).replace(/\.[^.]+$/, '').split(path.sep).join('/'),
    },
  ];
  for (const v of variants) {
    fs.rmSync(path.join(where, v.dir), { recursive: true, force: true });
    r = sh(rkp, ['bundle', ...sources, '--format', 'esm,cjs', '--outdir', v.dir, ...v.args, ...(lib.flags ?? [])], where, 300);
    if (!r.ok) return { ok: false, step: `build (${v.label})`, detail: r.err.split('\n').slice(-6).join('\n') };
    for (const [subpath, source] of entries) {
      const spec = subpath === '.' ? name : `${name}/${subpath.replace(/^\.\//, '')}`;
      const base = path.join(where, v.dir, v.file(source));
      for (const [format, extension, flag] of [['esm', '.mjs', 'esm'], ['cjs', '.cjs', 'cjs']]) {
        const shipped = sh(process.execPath, ['--experimental-import-meta-resolve', probe, `format-${flag}`, spec, scratch], scratch);
        if (!shipped.ok) { problems.push(`${v.label} ${spec}: ${format}: the published package does not resolve (${lastLine(shipped.err)})`); continue; }
        if (lastLine(shipped.out) !== format) continue;
        const theirs = sh(process.execPath, ['--experimental-import-meta-resolve', probe, `shape-${flag}`, spec, scratch], scratch);
        const ours = sh(process.execPath, ['--experimental-import-meta-resolve', probe, `shape-${flag}`, base + extension, where], where);
        if (!theirs.ok) { problems.push(`${v.label} ${spec}: ${format}: the published package does not load (${lastLine(theirs.err)})`); continue; }
        if (!ours.ok) { problems.push(`${v.label} ${spec}: ${format}: our file does not load (${lastLine(ours.err)})`); continue; }
        const missing = lacking(JSON.parse(lastLine(ours.out)).names, JSON.parse(lastLine(theirs.out)).names);
        if (missing.length) problems.push(`${v.label} ${spec}: ${format}: missing ${missing.slice(0, 6).join(', ')}${missing.length > 6 ? ` and ${missing.length - 6} more` : ''}`);
      }
    }
  }
  if (lib.types) {
    const typescript = path.join(testModules, 'typescript');
    if (fs.existsSync(typescript)) {
      const result = declarations(lib, where, rkp, scratch, name, entries, typescript);
      problems.push(...result);
    }
  }
  return problems.length ? { ok: false, step: 'library', detail: problems.join('\n') } : { ok: true };
}

// `--dts-bundle`, and the names the declarations export against the published ones (TypeScript's own program, from the test-deps folder).
function declarations(lib, where, rkp, scratch, name, entries, typescript) {
  const problems = [];
  const out = path.join(where, 'rkp-lib-dts');
  fs.rmSync(out, { recursive: true, force: true });
  const modules = path.join(where, 'node_modules');
  const linked = !fs.existsSync(path.join(modules, 'typescript'));
  if (linked) { fs.mkdirSync(modules, { recursive: true }); fs.symlinkSync(fs.realpathSync(typescript), path.join(modules, 'typescript'), 'dir'); }
  const r = sh(rkp, ['bundle', ...entries.map(([, e]) => e), '--outdir', 'rkp-lib-dts', '--dts-bundle', ...(lib.flags ?? [])], where, 300);
  if (linked) fs.rmSync(path.join(modules, 'typescript'), { force: true });
  if (!r.ok) return [`--dts-bundle: ${r.err.split('\n').slice(-4).join('\n')}`];
  const script = path.join(here, 'declared-names.cjs');
  for (const [subpath, source] of entries) {
    const spec = subpath === '.' ? name : `${name}/${subpath.replace(/^\.\//, '')}`;
    const ours = path.join(out, path.basename(source).replace(/\.[^.]+$/, '') + '.d.ts');
    if (!fs.existsSync(ours)) { problems.push(`types ${spec}: no ${path.basename(ours)} was written`); continue; }
    const published = sh(process.execPath, [script, typescript, 'resolve', spec, scratch], scratch);
    if (!published.ok) { continue; }   // the package ships no declarations for it
    const theirNames = JSON.parse(lastLine(published.out));
    const ourNames = JSON.parse(lastLine(sh(process.execPath, [script, typescript, 'file', ours, where], where).out || '[]'));
    const missing = theirNames.filter((n) => !ourNames.includes(n) && !(lib.typesIgnore ?? []).includes(n));
    if (missing.length) problems.push(`types ${spec}: missing ${missing.slice(0, 6).join(', ')}${missing.length > 6 ? ` and ${missing.length - 6} more` : ''}`);
  }
  return problems;
}
