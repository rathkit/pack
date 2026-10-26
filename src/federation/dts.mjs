// Types for module federation, written and read the way the other bundlers do it:
//
//   a remote publishes @mf-types.zip (compiled-types/*.d.ts of what it exposes, and apis.d.ts) and @mf-types.d.ts;
//   a host downloads the zip of every remote that has one into @mf-types/<remote>/ and makes `remote/expose` find
//   those files (a folder in node_modules, or the `paths` of tsconfig.json when asked).
//
// It runs in Node with the project's own `typescript` package, which is needed to write types, not to read them.
// The request is JSON on stdin; the answer is JSON on stdout.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import http from 'node:http';
import https from 'node:https';
import { spawnSync } from 'node:child_process';
import { builtinModules } from 'node:module';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';

const request = JSON.parse(fs.readFileSync(0, 'utf8'));
const { cwd } = request;
const messages = [];
const note = (text) => messages.push(text);

// ---- zip ---------------------------------------------------------------------------------------------

function zipWrite(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, data] of files) {
    const nameBytes = Buffer.from(name, 'utf8');
    const packed = zlib.deflateRawSync(data);
    const crc = zlib.crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, packed);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(8, 10);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(packed.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBytes);
    offset += local.length + nameBytes.length + packed.length;
  }
  const directory = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

function zipRead(buffer) {
  let end = buffer.length - 22;
  while (end >= 0 && buffer.readUInt32LE(end) !== 0x06054b50) end--;
  if (end < 0) throw new Error('not a zip file');
  const count = buffer.readUInt16LE(end + 10);
  let at = buffer.readUInt32LE(end + 16);
  const files = [];
  for (let i = 0; i < count; i++) {
    if (buffer.readUInt32LE(at) !== 0x02014b50) throw new Error('damaged zip directory');
    const method = buffer.readUInt16LE(at + 10);
    const packedSize = buffer.readUInt32LE(at + 20);
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    const local = buffer.readUInt32LE(at + 42);
    const name = buffer.toString('utf8', at + 46, at + 46 + nameLength);
    const start = local + 30 + buffer.readUInt16LE(local + 26) + buffer.readUInt16LE(local + 28);
    const packed = buffer.subarray(start, start + packedSize);
    if (!name.endsWith('/')) files.push([name, method === 0 ? Buffer.from(packed) : zlib.inflateRawSync(packed)]);
    at += 46 + nameLength + extraLength + commentLength;
  }
  return files;
}

// ---- type packages and remotes' types that travel in the zip ---------------------------------------------

const SPECIFIER = /(?:\bfrom\s*|\bimport\s*\(\s*|\bimport\s+|\brequire\s*\(\s*|\breference\s+types\s*=\s*)['"]([^'"]+)['"]/g;
const builtins = new Set(builtinModules);
const packageOf = (spec) => {
  if (spec.startsWith('.') || spec.startsWith('/') || spec.startsWith('node:')) return null;
  const parts = spec.split('/');
  const name = spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
  return builtins.has(name) ? null : name;
};
const findInNodeModules = (name) => {
  for (let dir = cwd; ; dir = path.dirname(dir)) {
    const found = path.join(dir, 'node_modules', name);
    if (fs.existsSync(path.join(found, 'package.json'))) return found;
    if (dir === path.dirname(dir)) return null;
  }
};
const hasTypes = (dir) => {
  const json = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  return Boolean(json.types || json.typings || fs.existsSync(path.join(dir, 'index.d.ts')) || JSON.stringify(json.exports ?? '').includes('"types"'));
};
const walkTypes = (dir, base = '') => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
  if (e.name === 'node_modules') return [];
  if (e.isDirectory()) return walkTypes(path.join(dir, e.name), `${base}${e.name}/`);
  return /\.d\.[cm]?ts$/.test(e.name) || (base === '' && e.name === 'package.json') ? [[`${base}${e.name}`, path.join(dir, e.name)]] : [];
});

// The declarations of the packages the exposed modules' declarations import, copied beside them (`node_modules` of the zip), so a host
// that does not have the packages still resolves them. `exclude` lists packages to leave out.
function thirdPartyEntries(entries, exclude = []) {
  const out = [];
  const done = new Set();
  const skip = (name) => exclude.some((e) => name === e || name.startsWith(e + '/'));
  const queue = entries.filter(([f]) => /\.d\.[cm]?ts$/.test(f)).map(([, data]) => data.toString('utf8'));
  while (queue.length) {
    const text = queue.pop();
    for (const [, spec] of text.matchAll(SPECIFIER)) {
      const name = packageOf(spec);
      if (!name || done.has(name) || skip(name)) continue;
      done.add(name);
      let dir = findInNodeModules(name);
      let target = name;
      if (!dir || !hasTypes(dir)) {
        const typed = findInNodeModules(`@types/${name.replace(/^@/, '').replace('/', '__')}`);
        if (!typed) {
          if (!dir) note(`the types of ${name} were not copied: the package was not found`);
          continue;
        }
        dir = typed;
        target = `@types/${name.replace(/^@/, '').replace('/', '__')}`;
      }
      for (const [rel, file] of walkTypes(dir)) {
        const data = fs.readFileSync(file);
        out.push([`node_modules/${target}/${rel}`, data]);
        if (rel.endsWith('.ts')) queue.push(data.toString('utf8'));
      }
    }
  }
  return out;
}

// The types this application has read of its own remotes, copied into the zip (`node_modules/<remote>`), so a host of this application
// can use what it re-exports.
function remoteTypeEntries(hostTypesFolder) {
  const root = path.resolve(cwd, hostTypesFolder);
  if (!fs.existsSync(root)) return [];
  const out = [];
  for (const remote of fs.readdirSync(root, { withFileTypes: true })) {
    if (!remote.isDirectory()) continue;
    const walk = (dir, base) => fs.readdirSync(dir, { withFileTypes: true }).forEach((e) => {
      if (e.isDirectory()) walk(path.join(dir, e.name), `${base}${e.name}/`);
      else if (e.name !== '.zip-sha256') out.push([`node_modules/${remote.name}/${base}${e.name}`, fs.readFileSync(path.join(dir, e.name))]);
    });
    walk(path.join(root, remote.name), '');
  }
  return out;
}

// A compiler other than `tsc` (`vue-tsc`, `tspc`, or a command with arguments) is run on a tsconfig written for the purpose.
function runCompiler(command, tsconfig) {
  const words = command.match(/"[^"]*"|'[^']*'|\S+/g)?.map((w) => w.replace(/^["']|["']$/g, '')) ?? [command];
  const [name, ...args] = words;
  let executable = name;
  for (let dir = cwd; ; dir = path.dirname(dir)) {
    const shim = path.join(dir, 'node_modules', '.bin', process.platform === 'win32' ? `${name}.cmd` : name);
    if (fs.existsSync(shim)) {
      executable = shim;
      break;
    }
    if (dir === path.dirname(dir)) break;
  }
  const result = spawnSync(executable, [...args, '--project', tsconfig], { cwd, encoding: 'utf8', shell: process.platform === 'win32' });
  if (result.error) throw new Error(`the compiler ${name} could not be run: ${result.error.message}`);
  return result;
}

// ---- writing a remote's types ----------------------------------------------------------------------

// The project's own TypeScript, and the compiler options it would use (a tsconfig.json found, or the one asked for).
function loadTypeScript() {
  try {
    return createRequire(path.join(cwd, 'noop.js'))('typescript');
  } catch {
    return null;
  }
}

function findConfig(ts, tsConfigPath) {
  return tsConfigPath ? path.resolve(cwd, tsConfigPath) : ts.findConfigFile(cwd, ts.sys.fileExists, 'tsconfig.json');
}

function compilerOptions(ts, tsConfigPath) {
  const configPath = findConfig(ts, tsConfigPath);
  let options = { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, jsx: ts.JsxEmit.ReactJSX, esModuleInterop: true, allowJs: true, skipLibCheck: true };
  if (configPath) {
    const read = ts.readConfigFile(configPath, ts.sys.readFile);
    if (read.config) options = { ...options, ...ts.parseJsonConfigFileContent(read.config, ts.sys, path.dirname(configPath)).options };
  }
  return options;
}

// ---- writing declarations for entry points (--dts) --------------------------------------------------

// One declaration file for each source file the entries reach, as `tsc --declaration --emitDeclarationOnly` writes them, laid out
// from the folder the sources have in common. Files in node_modules are not described.
function emit({ entries, outDir, tsConfigPath }) {
  const ts = loadTypeScript();
  if (!ts) return { error: 'the typescript package is not installed (--dts writes declarations with it; install it in the project)' };
  const options = compilerOptions(ts, tsConfigPath);
  const out = path.resolve(cwd, outDir);
  const program = ts.createProgram(entries, { ...options, declaration: true, emitDeclarationOnly: true, noEmit: false, outDir: out, declarationDir: undefined, declarationMap: false, sourceMap: false, composite: false, incremental: false, noEmitOnError: false, skipLibCheck: true, allowJs: true });
  const written = [];
  const result = program.emit(undefined, (file, text) => {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, text);
    written.push(path.relative(out, file).split(path.sep).join('/'));
  });
  const errors = ts.getPreEmitDiagnostics(program).filter((d) => d.category === ts.DiagnosticCategory.Error);
  for (const d of errors.slice(0, 20)) {
    const where = d.file ? `${path.relative(cwd, d.file.fileName)}:${d.file.getLineAndCharacterOfPosition(d.start).line + 1}: ` : '';
    note(`${where}${ts.flattenDiagnosticMessageText(d.messageText, '\n').split('\n')[0]}`);
  }
  if (result.emitSkipped && !written.length) return { error: 'no declarations were written' };
  written.sort();
  return { written, errors: errors.length };
}

async function generate({ name, exposes, outDir, tsConfigPath, typesFolder = '@mf-types', compiledTypesFolder = 'compiled-types', additionalFiles = [], generateApi = true, compilerInstance = 'tsc', extractThirdParty = false, extractRemoteTypes = false, hostTypesFolder = '@mf-types', afterGenerate = '' }) {
  const ts = loadTypeScript();
  if (!ts) {
    note('types were not written for the exposed modules: the typescript package is not installed');
    return { written: false };
  }
  const options = compilerOptions(ts, tsConfigPath);
  const files = exposes.map((e) => e.file);
  const rootFiles = [...files, ...additionalFiles];
  const rootDir = rootFiles.reduce((common, file) => {
    const dir = path.dirname(file);
    while (!(dir + path.sep).startsWith(common + path.sep) && common !== path.dirname(common)) common = path.dirname(common);
    return common;
  }, path.dirname(files[0]));
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'rathkit-types-'));
  const compiled = path.join(work, compiledTypesFolder);
  const program_options = { declaration: true, emitDeclarationOnly: true, noEmit: false, outDir: compiled, rootDir, declarationMap: false, sourceMap: false, composite: false, incremental: false, noEmitOnError: false, skipLibCheck: true, allowJs: true };
  let diagnostics = [];
  let external = 0;
  if (compilerInstance.trim() === 'tsc' || !compilerInstance.trim()) {
    const program = ts.createProgram(rootFiles, { ...options, ...program_options });
    const result = program.emit();
    diagnostics = ts.getPreEmitDiagnostics(program).filter((d) => d.category === ts.DiagnosticCategory.Error);
    if (result.emitSkipped && !fs.existsSync(compiled)) {
      note('no types were written: ' + diagnostics.slice(0, 3).map((d) => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('; '));
      fs.rmSync(work, { recursive: true, force: true });
      return { written: false };
    }
  } else {
    const configPath = findConfig(ts, tsConfigPath);
    const tsconfig = path.join(work, 'tsconfig.mf.json');
    fs.writeFileSync(tsconfig, JSON.stringify({ ...(configPath ? { extends: configPath } : {}), compilerOptions: program_options, files: rootFiles, include: [] }));
    const result = runCompiler(compilerInstance, tsconfig);
    external = (`${result.stdout}${result.stderr}`.match(/error TS\d+/g) ?? []).length;
    if (!fs.existsSync(compiled)) {
      note(`no types were written: the compiler ${compilerInstance} wrote none${result.stdout ? ': ' + result.stdout.split('\n')[0] : ''}`);
      fs.rmSync(work, { recursive: true, force: true });
      return { written: false };
    }
  }
  const collect = (dir, base = '') => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? collect(path.join(dir, e.name), `${base}${e.name}/`) : [[`${compiledTypesFolder}/${base}${e.name}`, fs.readFileSync(path.join(dir, e.name))]]));
  const entries = collect(compiled);
  if (extractRemoteTypes) entries.push(...remoteTypeEntries(hostTypesFolder));
  if (extractThirdParty) entries.push(...thirdPartyEntries(entries, Array.isArray(extractThirdParty?.exclude) ? extractThirdParty.exclude : []));
  const typeFile = (file) => path.relative(rootDir, file).replace(/\.(tsx?|jsx?|mjs|cjs|mts|cts)$/, '').split(path.sep).join('/');
  const keys = exposes.map((e) => `${name}/${e.key.replace(/^\.\//, '')}`);
  const apis = `\n    export type RemoteKeys = ${keys.map((k) => `'${k}'`).join(' | ')};\n    type PackageType<T> = ${exposes.map((e, i) => `T extends '${keys[i]}' ? typeof import('./${compiledTypesFolder}/${typeFile(e.file)}')`).join(' :\n') || 'any'} : any;`;
  if (generateApi) entries.push(['apis.d.ts', Buffer.from(apis)]);
  const folder = path.join(outDir, typesFolder);
  fs.rmSync(folder, { recursive: true, force: true });
  for (const [file, data] of entries) {
    fs.mkdirSync(path.dirname(path.join(folder, file)), { recursive: true });
    fs.writeFileSync(path.join(folder, file), data);
  }
  fs.writeFileSync(path.join(outDir, `${typesFolder}.zip`), zipWrite(entries));
  if (generateApi) fs.writeFileSync(path.join(outDir, `${typesFolder}.d.ts`), apis);
  fs.rmSync(work, { recursive: true, force: true });
  const errors = diagnostics.length + external;
  if (errors) note(`${errors} type error(s) in the exposed modules; the types were written anyway`);
  if (afterGenerate) {
    // The function came from the configuration as its source text; it can use only what it defines itself.
    globalThis.require ??= createRequire(path.join(cwd, 'noop.js'));
    const hook = (0, eval)(`(${afterGenerate})`);
    await hook({ zipTypesPath: path.join(outDir, `${typesFolder}.zip`), apiTypesPath: generateApi ? path.join(outDir, `${typesFolder}.d.ts`) : '', zipName: `${typesFolder}.zip`, apiFileName: generateApi ? `${typesFolder}.d.ts` : '' });
  }
  return { written: true, zip: `${typesFolder}.zip`, api: generateApi ? `${typesFolder}.d.ts` : '', files: entries.length, typeErrors: errors };
}

// ---- reading the types of remotes ---------------------------------------------------------------------

// `family` 4 or 6 asks for one kind of address, which fetch cannot; the request is made with the http module then.
function viaHttp(url, timeout, family, redirects = 5) {
  return new Promise((resolve, reject) => {
    const lib = url.startsWith('https:') ? https : http;
    const req = lib.get(url, { family, timeout }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location && redirects > 0) {
        res.resume();
        resolve(viaHttp(new URL(res.headers.location, url).href, timeout, family, redirects - 1));
        return;
      }
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const body = Buffer.concat(chunks);
        resolve({ ok: res.statusCode >= 200 && res.statusCode < 300, status: res.statusCode, json: async () => JSON.parse(body.toString('utf8')), arrayBuffer: async () => body.buffer.slice(body.byteOffset, body.byteOffset + body.length) });
      });
    });
    req.on('timeout', () => req.destroy(new Error('timed out')));
    req.on('error', reject);
  });
}

let addressFamily = 0;
async function get(url, timeout = 5000, retries = 0) {
  let last;
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const response = addressFamily ? await viaHttp(url, timeout, addressFamily) : await fetch(url, { signal: AbortSignal.timeout(timeout) });
      if (!response.ok) throw new Error(`${url}: ${response.status}`);
      return response;
    } catch (error) {
      last = error;
    }
  }
  throw last;
}

const hasDefault = (text) => /export\s+default\b|export\s*\{[^}]*\bas\s+default\b[^}]*\}|export\s*\{\s*default\b/.test(text);

// `import "remote/expose"` is found through `paths` in tsconfig.json, one exact entry for each exposed module.
function mapPaths(paths) {
  const file = path.join(cwd, 'tsconfig.json');
  if (!fs.existsSync(file) || !Object.keys(paths).length) return;
  let config;
  try {
    config = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    // Comments would be lost by writing the file again; it is left as it is.
    note(`tsconfig.json has comments, so it was not changed; add to "compilerOptions": "paths": ${JSON.stringify(paths)}`);
    return;
  }
  config.compilerOptions ||= {};
  const before = JSON.stringify(config.compilerOptions.paths || {});
  config.compilerOptions.paths = { ...(config.compilerOptions.paths || {}), ...paths };
  if (JSON.stringify(config.compilerOptions.paths) === before) return;
  const raw = fs.readFileSync(file, 'utf8');
  const indent = /^\t/m.test(raw) ? '\t' : /^( +)\S/m.exec(raw)?.[1].length ?? 2;
  fs.writeFileSync(file, JSON.stringify(config, null, indent) + '\n');
}

// The declarations of a remote, as the editor finds them for `import "remote/expose"`: a folder of the remote's name in
// node_modules, with a file for each exposed module that passes on the real declarations, which stay in @mf-types. No
// setting of the project has to change. (It is not done where the folder is a package someone installed.)
function linkModules(modules) {
  for (const [name, entries] of Object.entries(modules)) {
    const folder = path.join(cwd, 'node_modules', name);
    const marker = path.join(folder, 'package.json');
    if (fs.existsSync(folder) && !(fs.existsSync(marker) && JSON.parse(fs.readFileSync(marker, 'utf8'))['rathkit-types'])) {
      note(`the remote ${name} shares its name with an installed package, so its types were put in @mf-types only; map them in tsconfig.json "paths" or rename the remote`);
      continue;
    }
    fs.rmSync(folder, { recursive: true, force: true });
    fs.mkdirSync(folder, { recursive: true });
    // `exports` so that node16 / nodenext resolution, which needs it for `remote/Button`, finds the files too.
    fs.writeFileSync(marker, JSON.stringify({ name, private: true, 'rathkit-types': true, type: 'module', exports: { './*': { import: { types: './*.d.ts' }, require: { types: './*.d.cts' } } } }, null, 2) + '\n');
    for (const [expose, { file, hasDefault }] of Object.entries(entries)) {
      const target = path.join(folder, `${expose}.d.ts`);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      const from = path.relative(path.dirname(target), file).split(path.sep).join('/');
      // With the `.js` that an ES module's import has under node16 / nodenext (it names the .d.ts all the same).
      const spec = JSON.stringify((from.startsWith('.') ? from : './' + from) + '.js');
      fs.writeFileSync(target, `export * from ${spec};\n${hasDefault ? `export { default } from ${spec};\n` : ''}`);
      // A CommonJS file under node16 / nodenext reads the same types through a file of its own kind.
      fs.writeFileSync(target.replace(/\.d\.ts$/, '.d.cts'), `export * from ${spec};\n${hasDefault ? `import __default from ${spec};\nexport default __default;\n` : ''}`);
    }
  }
}

// `index.d.ts` of the types folder: `loadRemote` of the runtime packages, typed by the `apis.d.ts` of each remote that has one.
function writeApiTypes(root, names, runtimePkgs) {
  const aliases = names.filter((n) => fs.existsSync(path.join(root, n, 'apis.d.ts'))).sort();
  if (!aliases.length) return;
  const imports = aliases.map((alias, i) => `import type { PackageType as PackageType_${i}, RemoteKeys as RemoteKeys_${i} } from './${alias}/apis.d.ts';`).join('\n');
  const keys = `type RemoteKeys = ${aliases.map((_, i) => `RemoteKeys_${i}`).join(' | ')};`;
  const types = `type PackageType<T, Y=any> = ${[...aliases.map((_, i) => `T extends RemoteKeys_${i} ? PackageType_${i}<T>`), 'Y'].join(' :\n')} ;`;
  const packages = new Set(['@module-federation/runtime', '@module-federation/enhanced/runtime', '@module-federation/runtime-tools', ...runtimePkgs]);
  const declarations = [...packages].map((pkg) => `declare module "${pkg}" {\n  ${keys}\n  ${types}\n  export function loadRemote<T extends RemoteKeys,Y>(packageName: T): Promise<PackageType<T, Y>>;\n  export function loadRemote<T extends string,Y>(packageName: T): Promise<PackageType<T, Y>>;\n}`).join('\n');
  fs.writeFileSync(path.join(root, 'index.d.ts'), `${imports}\n${declarations}\n`);
}

async function consume({ remotes, typesFolder = '@mf-types', updateTsConfig = false, timeout = 5000, maxRetries = 0, typeUrls = {}, family = 0, remoteTypesFolder = '', consumeApiTypes = false, runtimePkgs = [] }) {
  addressFamily = family;
  const root = path.resolve(cwd, typesFolder);
  const paths = {};
  const modules = {};
  const fetched = [];
  const failed = [];
  const relative = (target) => './' + path.relative(cwd, target).split(path.sep).join('/');
  for (const remote of remotes) {
    try {
      let zipUrl;
      if (typeUrls[remote.name]?.zip) {
        // The address of the types is given: no manifest is read.
        zipUrl = new URL(typeUrls[remote.name].zip, remote.entry).href;
      } else {
        const manifestUrl = /\.json(?:$|[?#])/.test(remote.entry) ? remote.entry : new URL('mf-manifest.json', remote.entry).href;
        let types;
        let publicPath = manifestUrl;
        try {
          const manifest = await (await get(manifestUrl, timeout, maxRetries)).json();
          types = manifest.metaData?.types;
          if (manifest.metaData?.publicPath && manifest.metaData.publicPath !== 'auto') publicPath = manifest.metaData.publicPath;
        } catch (error) {
          if (!remoteTypesFolder) throw error;
        }
        if (types?.zip) zipUrl = new URL(`${types.path ? types.path.replace(/\/?$/, '/') : ''}${types.zip}`, publicPath).href;
        // With no manifest to say where, the archive is named after `remoteTypesFolder`, beside the remote's entry.
        else if (remoteTypesFolder) zipUrl = new URL(`${remoteTypesFolder}.zip`, remote.entry).href;
        else continue;
      }
      const zip = Buffer.from(await (await get(zipUrl, Math.max(timeout, 15000), maxRetries)).arrayBuffer());
      const files = zipRead(zip);
      const folder = path.join(root, remote.name);
      // The same zip as last time: the files stay as they are (an editor watching them has nothing to read again).
      const stamp = path.join(folder, '.zip-sha256');
      const sha = createHash('sha256').update(zip).digest('hex');
      if (!(fs.existsSync(stamp) && fs.readFileSync(stamp, 'utf8') === sha)) {
        fs.rmSync(folder, { recursive: true, force: true });
        for (const [file, data] of files) {
          const target = path.join(folder, file);
          if (!target.startsWith(folder + path.sep)) continue;
          fs.mkdirSync(path.dirname(target), { recursive: true });
          fs.writeFileSync(target, data);
        }
        // The declarations are ES modules, whatever the project around them is.
        fs.writeFileSync(path.join(folder, 'package.json'), JSON.stringify({ private: true, type: 'module' }) + '\n');
        fs.writeFileSync(stamp, sha);
      }
      const apis = files.find(([f]) => f === 'apis.d.ts')?.[1].toString() ?? '';
      modules[remote.name] = {};
      for (const [, key, file] of apis.matchAll(/T extends '([^']+)' \? typeof import\('\.\/([^']+)'\)/g)) {
        const real = path.join(folder, file);
        paths[key] = [relative(real)];
        const text = files.find(([f]) => f === `${file}.d.ts`)?.[1].toString() ?? '';
        modules[remote.name][key.slice(remote.name.length + 1)] = { file: real, hasDefault: hasDefault(text) };
      }
      fetched.push(remote.name);
    } catch (error) {
      failed.push(remote.name);
      note(`types of the remote ${remote.name} were not read: ${error.message}`);
    }
  }
  linkModules(modules);
  if (updateTsConfig) mapPaths(paths);
  if (consumeApiTypes) writeApiTypes(root, fetched, runtimePkgs);
  return { fetched, failed };
}

// Puts the shims for the remotes' modules in node_modules again from the declarations already in @mf-types, with no network:
// installing packages removes them.
function link({ typesFolder = '@mf-types' }) {
  const root = path.resolve(cwd, typesFolder);
  const modules = {};
  const linked = [];
  for (const name of fs.existsSync(root) ? fs.readdirSync(root) : []) {
    const folder = path.join(root, name);
    const apisFile = path.join(folder, 'apis.d.ts');
    if (!fs.existsSync(apisFile)) continue;
    modules[name] = {};
    for (const [, key, file] of fs.readFileSync(apisFile, 'utf8').matchAll(/T extends '([^']+)' \? typeof import\('\.\/([^']+)'\)/g)) {
      const real = path.join(folder, file);
      const text = fs.existsSync(`${real}.d.ts`) ? fs.readFileSync(`${real}.d.ts`, 'utf8') : '';
      modules[name][key.slice(name.length + 1)] = { file: real, hasDefault: hasDefault(text) };
    }
    linked.push(name);
  }
  linkModules(modules);
  return { linked };
}

let answer;
try {
  answer = request.command === 'generate' ? await generate(request) : request.command === 'emit' ? emit(request) : request.command === 'link' ? link(request) : await consume(request);
} catch (error) {
  answer = { error: String(error?.stack ?? error) };
}
process.stdout.write(JSON.stringify({ ...answer, messages }));
