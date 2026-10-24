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

// ---- writing a remote's types ----------------------------------------------------------------------

function generate({ name, exposes, outDir, tsConfigPath, typesFolder = '@mf-types', compiledTypesFolder = 'compiled-types' }) {
  let ts;
  try {
    ts = createRequire(path.join(cwd, 'noop.js'))('typescript');
  } catch {
    note('types were not written for the exposed modules: the typescript package is not installed');
    return { written: false };
  }
  const configPath = tsConfigPath ? path.resolve(cwd, tsConfigPath) : ts.findConfigFile(cwd, ts.sys.fileExists, 'tsconfig.json');
  let options = { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, jsx: ts.JsxEmit.ReactJSX, esModuleInterop: true, allowJs: true, skipLibCheck: true };
  if (configPath) {
    const read = ts.readConfigFile(configPath, ts.sys.readFile);
    if (read.config) options = { ...options, ...ts.parseJsonConfigFileContent(read.config, ts.sys, path.dirname(configPath)).options };
  }
  const files = exposes.map((e) => e.file);
  const rootDir = files.reduce((common, file) => {
    const dir = path.dirname(file);
    while (!(dir + path.sep).startsWith(common + path.sep) && common !== path.dirname(common)) common = path.dirname(common);
    return common;
  }, path.dirname(files[0]));
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'rathkit-types-'));
  const compiled = path.join(work, compiledTypesFolder);
  const program = ts.createProgram(files, { ...options, declaration: true, emitDeclarationOnly: true, noEmit: false, outDir: compiled, rootDir, declarationMap: false, sourceMap: false, composite: false, incremental: false, noEmitOnError: false, skipLibCheck: true, allowJs: true });
  const result = program.emit();
  const diagnostics = ts.getPreEmitDiagnostics(program).filter((d) => d.category === ts.DiagnosticCategory.Error);
  if (result.emitSkipped && !fs.existsSync(compiled)) {
    note('no types were written: ' + diagnostics.slice(0, 3).map((d) => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('; '));
    fs.rmSync(work, { recursive: true, force: true });
    return { written: false };
  }
  const collect = (dir, base = '') => fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? collect(path.join(dir, e.name), `${base}${e.name}/`) : [[`${compiledTypesFolder}/${base}${e.name}`, fs.readFileSync(path.join(dir, e.name))]]));
  const entries = collect(compiled);
  const typeFile = (file) => path.relative(rootDir, file).replace(/\.(tsx?|jsx?|mjs|cjs|mts|cts)$/, '').split(path.sep).join('/');
  const keys = exposes.map((e) => `${name}/${e.key.replace(/^\.\//, '')}`);
  const apis = `\n    export type RemoteKeys = ${keys.map((k) => `'${k}'`).join(' | ')};\n    type PackageType<T> = ${exposes.map((e, i) => `T extends '${keys[i]}' ? typeof import('./${compiledTypesFolder}/${typeFile(e.file)}')`).join(' :\n') || 'any'} : any;`;
  entries.push(['apis.d.ts', Buffer.from(apis)]);
  const folder = path.join(outDir, typesFolder);
  fs.rmSync(folder, { recursive: true, force: true });
  for (const [file, data] of entries) {
    fs.mkdirSync(path.dirname(path.join(folder, file)), { recursive: true });
    fs.writeFileSync(path.join(folder, file), data);
  }
  fs.writeFileSync(path.join(outDir, `${typesFolder}.zip`), zipWrite(entries));
  fs.writeFileSync(path.join(outDir, `${typesFolder}.d.ts`), apis);
  fs.rmSync(work, { recursive: true, force: true });
  if (diagnostics.length) note(`${diagnostics.length} type error(s) in the exposed modules; the types were written anyway`);
  return { written: true, zip: `${typesFolder}.zip`, api: `${typesFolder}.d.ts`, files: entries.length };
}

// ---- reading the types of remotes ---------------------------------------------------------------------

async function get(url, timeout = 5000) {
  const response = await fetch(url, { signal: AbortSignal.timeout(timeout) });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return response;
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
    fs.writeFileSync(marker, JSON.stringify({ name, private: true, 'rathkit-types': true, type: 'module', exports: { './*': { types: './*.d.ts' } } }, null, 2) + '\n');
    for (const [expose, { file, hasDefault }] of Object.entries(entries)) {
      const target = path.join(folder, `${expose}.d.ts`);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      const from = path.relative(path.dirname(target), file).split(path.sep).join('/');
      // With the `.js` that an ES module's import has under node16 / nodenext (it names the .d.ts all the same).
      const spec = JSON.stringify((from.startsWith('.') ? from : './' + from) + '.js');
      fs.writeFileSync(target, `export * from ${spec};\n${hasDefault ? `export { default } from ${spec};\n` : ''}`);
    }
  }
}

async function consume({ remotes, typesFolder = '@mf-types', updateTsConfig = false }) {
  const root = path.resolve(cwd, typesFolder);
  const paths = {};
  const modules = {};
  const fetched = [];
  const relative = (target) => './' + path.relative(cwd, target).split(path.sep).join('/');
  for (const remote of remotes) {
    try {
      const manifestUrl = /\.json(?:$|[?#])/.test(remote.entry) ? remote.entry : new URL('mf-manifest.json', remote.entry).href;
      const manifest = await (await get(manifestUrl)).json();
      const types = manifest.metaData?.types;
      if (!types?.zip) continue;
      const publicPath = manifest.metaData.publicPath && manifest.metaData.publicPath !== 'auto' ? manifest.metaData.publicPath : manifestUrl;
      const zipUrl = new URL(`${types.path ? types.path.replace(/\/?$/, '/') : ''}${types.zip}`, publicPath).href;
      const zip = Buffer.from(await (await get(zipUrl, 15000)).arrayBuffer());
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
      note(`types of the remote ${remote.name} were not read: ${error.message}`);
    }
  }
  linkModules(modules);
  if (updateTsConfig) mapPaths(paths);
  return { fetched };
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
  answer = request.command === 'generate' ? generate(request) : request.command === 'link' ? link(request) : await consume(request);
} catch (error) {
  answer = { error: String(error?.stack ?? error) };
}
process.stdout.write(JSON.stringify({ ...answer, messages }));
