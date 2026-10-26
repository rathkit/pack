'use strict';
// The programmatic side of Rathkit Pack: the same program as the `rkp` command, started with options in place of flags.
// It has no dependencies and starts no process but the program itself (and nothing at install time).
const { spawn, spawnSync } = require('node:child_process');
const { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } = require('node:fs');
const { dirname, join, resolve } = require('node:path');
const { tmpdir } = require('node:os');

// On Linux the package depends on the C library: glibc reports its version, musl (Alpine) does not.
function platformName() {
  const libc = process.platform !== 'linux' ? '' : process.report?.getReport?.().header?.glibcVersionRuntime ? '-gnu' : '-musl';
  return `${process.platform}-${process.arch}${libc}`;
}

/** The path of the program for this platform; `RATHKIT_PACK_BIN` overrides it. Throws when there is none. */
function binaryPath() {
  if (process.env.RATHKIT_PACK_BIN) return process.env.RATHKIT_PACK_BIN;
  const platform = platformName();
  const exe = process.platform === 'win32' ? 'rkp.exe' : 'rkp';
  let bin = null;
  try {
    bin = join(dirname(require.resolve(`@rathkit/pack-${platform}/package.json`)), 'bin', exe);
  } catch {}
  if (!bin || !existsSync(bin)) {
    throw new Error(
      `rkp: there is no build for ${platform}.\n` +
        'Builds exist for linux-x64 and linux-arm64 (glibc: -gnu, Alpine: -musl), darwin-x64, darwin-arm64 and win32-x64.\n' +
        `If the platform is one of those, the package @rathkit/pack-${platform} was not installed: install again without omitting optional dependencies. ` +
        'Otherwise build from source: https://github.com/rathkit/pack',
    );
  }
  return bin;
}

/** What `rkp --version` prints, without its name. */
function version() {
  const r = spawnSync(binaryPath(), ['--version'], { encoding: 'utf8' });
  return r.stdout.trim().replace(/^rkp /, '');
}

class BuildError extends Error {
  constructor(result) {
    super(result.errors.length ? result.errors.join('\n') : `rkp exited with ${result.status}`);
    this.name = 'BuildError';
    this.status = result.status;
    this.errors = result.errors;
    this.warnings = result.warnings;
    this.stdout = result.stdout;
    this.stderr = result.stderr;
  }
}

const KEYS = new Set(['entries', 'cwd', 'env', 'signal', 'outfile', 'outdir', 'format', 'globalName', 'globals', 'minify', 'keepNames', 'sourcemap', 'sourcesContent', 'splitting', 'treeShaking', 'platform', 'external', 'conditions', 'mainFields', 'alias', 'define', 'loader', 'assetNames', 'publicPath', 'jsx', 'jsxImportSource', 'jsxFactory', 'jsxFragment', 'decoratorMetadata', 'experimentalDecorators', 'strictNullChecks', 'tsconfig', 'config', 'mode', 'envDir', 'envPrefix', 'envFiles', 'banner', 'footer', 'drop', 'packages', 'resolveExtensions', 'preserveSymlinks', 'metafile', 'pure', 'entryNames', 'chunkNames', 'outExtension', 'legalComments', 'inject', 'federation', 'originalPaths', 'corsOrigins', 'port', 'host']);

/** The command line for options. A key that is not an option is an error, so a typo is not ignored. */
function toArgs(options, cleanups) {
  for (const key of Object.keys(options)) if (!KEYS.has(key)) throw new TypeError(`rkp: unknown option "${key}"`);
  const args = [];
  const entries = [].concat(options.entries ?? []);
  const add = (flag, value) => value === undefined || value === null || args.push(flag, String(value));
  const each = (flag, list) => (list ?? []).forEach((v) => args.push(flag, String(v)));
  const pairs = (flag, object, join = '=') => Object.entries(object ?? {}).forEach(([k, v]) => args.push(flag, `${k}${join}${v}`));
  add('--outfile', options.outfile);
  add('--outdir', options.outdir);
  add('--format', options.format);
  add('--global-name', options.globalName);
  pairs('--global', options.globals);
  const m = options.minify;
  if (m === true) args.push('--minify');
  else if (m) {
    if (m.whitespace) args.push('--minify-whitespace');
    if (m.identifiers) args.push('--minify-identifiers');
    if (m.syntax) args.push('--minify-syntax');
  }
  if (options.keepNames === true) args.push('--keep-names');
  if (options.keepNames === false) args.push('--no-keep-names');
  if (options.sourcemap === true) args.push('--sourcemap');
  else if (options.sourcemap) args.push(`--sourcemap=${options.sourcemap}`);
  if (options.sourcesContent === false) args.push('--no-sources-content');
  if (options.splitting) args.push('--splitting');
  if (options.treeShaking === false) args.push('--no-treeshake');
  if (options.platform === 'browser') args.push('--platform-browser');
  each('--external', options.external);
  each('--condition', options.conditions);
  each('--main-field', options.mainFields);
  Object.entries(options.alias ?? {}).forEach(([name, target]) => args.push('--alias', `${name}=${resolve(options.cwd ?? process.cwd(), target)}`));
  pairs('--define', options.define);
  Object.entries(options.loader ?? {}).forEach(([ext, name]) => args.push('--loader', `${ext}=${name}`));
  add('--asset-names', options.assetNames);
  add('--public-path', options.publicPath);
  add('--jsx', options.jsx);
  add('--jsx-import-source', options.jsxImportSource);
  add('--jsx-factory', options.jsxFactory);
  add('--jsx-fragment', options.jsxFragment);
  if (options.decoratorMetadata) args.push('--decorator-metadata');
  if (options.experimentalDecorators) args.push('--experimental-decorators');
  if (options.strictNullChecks) args.push('--strict-null-checks');
  if (options.tsconfig === false) args.push('--no-tsconfig');
  else add('--tsconfig', options.tsconfig);
  if (options.config === false) args.push('--no-config');
  else add('--config', options.config);
  // A text for scripts, or one for each kind of file.
  for (const [flag, value] of [['--banner', options.banner], ['--footer', options.footer]]) {
    if (typeof value === 'string') args.push(`${flag}:js=${value}`);
    else for (const [kind, text] of Object.entries(value ?? {})) args.push(`${flag}:${kind}=${text}`);
  }
  each('--drop', options.drop);
  each('--inject', options.inject);
  add('--legal-comments', options.legalComments);
  add('--entry-names', options.entryNames);
  add('--chunk-names', options.chunkNames);
  Object.entries(options.outExtension ?? {}).forEach(([from, to]) => args.push(`--out-extension:${from}=${to}`));
  (options.pure ?? []).forEach((name) => args.push(`--pure:${name}`));
  add('--packages', options.packages);
  if (options.resolveExtensions) args.push('--resolve-extensions', options.resolveExtensions.join(','));
  if (options.preserveSymlinks) args.push('--preserve-symlinks');
  if (options.metafile) args.push(`--metafile=${resolve(options.cwd ?? process.cwd(), options.metafile)}`);
  add('--mode', options.mode);
  add('--env-dir', options.envDir);
  each('--env-prefix', [].concat(options.envPrefix ?? []));
  if (options.envFiles === false) args.push('--no-env-files');
  if (options.originalPaths) args.push('--original-paths');
  each('--cors-origin', options.corsOrigins);
  add('--port', options.port);
  add('--host', options.host);
  if (options.federation !== undefined) {
    let file = options.federation;
    if (typeof file !== 'string') {
      // Options given as an object are handed over in a file of their own, removed afterwards.
      const dir = mkdtempSync(join(tmpdir(), 'rkp-federation-'));
      file = join(dir, 'federation.json');
      writeFileSync(file, JSON.stringify(options.federation));
      cleanups.push(() => rmSync(dir, { recursive: true, force: true }));
    }
    args.push('--federation', resolve(options.cwd ?? process.cwd(), file));
  }
  return { entries, args };
}

function lines(text, prefix) {
  return text.split('\n').filter((l) => l.startsWith(prefix)).map((l) => l.slice(prefix.length).trim());
}

/** Starts the program with these arguments. Never throws for a failed run: look at `status`. */
function run(args, { cwd, env, signal } = {}) {
  return new Promise((done, fail) => {
    let child;
    try {
      child = spawn(binaryPath(), args, { cwd, env: env ? { ...process.env, ...env } : process.env, signal, stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (error) {
      return fail(error);
    }
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    child.on('error', fail);
    child.on('close', (status, termSignal) => done({ status, signal: termSignal, stdout, stderr }));
  });
}

/**
 * Builds. Resolves with the files written, or rejects with a BuildError that carries the errors.
 * `entries` are files or html pages; the rest is as the command's flags.
 */
async function build(options = {}) {
  const cleanups = [];
  try {
    const { entries, args } = toArgs(options, cleanups);
    if (!entries.length) throw new TypeError('rkp: build needs entries');
    const r = await run(['bundle', ...entries, ...args], options);
    const result = {
      ok: r.status === 0,
      status: r.status,
      stdout: r.stdout,
      stderr: r.stderr,
      errors: lines(r.stderr, 'error: '),
      warnings: lines(r.stderr, 'warning: '),
      // What the program lists: `path  1.5 KB` for each file and `path  page` for each page.
      files: r.stdout.split('\n').map((l) => /^(.*?)  (?:[\d.]+ KB|page)$/.exec(l)?.[1]).filter(Boolean),
    };
    if (!result.ok) throw new BuildError(result);
    // A single output file is not listed by the program; the page and style sheets beside it are.
    if (options.outfile) result.files.unshift(resolve(options.cwd ?? process.cwd(), options.outfile));
    // The metafile is read back, so a program can use it without opening the file.
    if (options.metafile) result.metafile = JSON.parse(readFileSync(resolve(options.cwd ?? process.cwd(), options.metafile), 'utf8'));
    return result;
  } finally {
    cleanups.forEach((c) => c());
  }
}

/** Starts the development server. Resolves once it is listening, with its address and a way to stop it. */
async function serve(options = {}) {
  const cleanups = [];
  const { entries, args } = toArgs(options, cleanups);
  const child = spawn(binaryPath(), ['serve', ...entries, ...args], { cwd: options.cwd, env: options.env ? { ...process.env, ...options.env } : process.env, signal: options.signal, stdio: ['ignore', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (d) => (stderr += d));
  const url = await new Promise((done, fail) => {
    let out = '';
    child.stdout.on('data', (d) => {
      out += d;
      const m = /serving at (\S+)/.exec(out);
      if (m) done(m[1]);
    });
    child.on('error', fail);
    child.on('close', (status) => fail(new BuildError({ status, errors: lines(stderr, 'error: '), warnings: [], stdout: out, stderr })));
  }).catch((error) => {
    cleanups.forEach((c) => c());
    throw error;
  });
  return {
    url,
    /** Stops the server and waits for it to end. */
    close: () =>
      new Promise((done) => {
        cleanups.forEach((c) => c());
        if (child.exitCode !== null || child.signalCode !== null) return done();
        child.once('close', () => done());
        child.kill('SIGINT');
      }),
  };
}

exports.binaryPath = binaryPath;
exports.version = version;
exports.run = run;
exports.build = build;
exports.serve = serve;
exports.BuildError = BuildError;
