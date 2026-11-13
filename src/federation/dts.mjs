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
let typescriptProblem = 'the typescript package is not installed (--dts writes declarations with it; install it in the project)';

function loadTypeScript() {
  let ts;
  try {
    ts = createRequire(path.join(cwd, 'noop.js'))('typescript');
  } catch {
    return null;
  }
  // TypeScript 7 is the native compiler: its package has no JavaScript API (no `ts.sys`, no `createProgram`) to write declarations with.
  if (!ts || !ts.sys || typeof ts.createProgram !== 'function') {
    let version = '';
    try {
      version = ` ${createRequire(path.join(cwd, 'noop.js'))('typescript/package.json').version}`;
    } catch {}
    typescriptProblem = `the typescript package${version} has no JavaScript API (TypeScript 7 is the native compiler), which this needs. Install typescript 5 or 6 in the project`;
    return null;
  }
  return ts;
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

// ---- one declaration file for an entry (--dts-bundle) -----------------------------------------------------

// The declarations of an entry and of everything it reaches by relative imports, in one file: names that two files share are told apart, the
// imports and exports between the files are gone, what comes from a package stays an import, and the entry's exports are listed at the end.
function bundleDeclarations(ts, entryFile) {
  const options = { noEmit: true, noLib: true, skipLibCheck: true, types: [], module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, allowJs: false };
  const program = ts.createProgram([entryFile], options);
  const checker = program.getTypeChecker();
  const entry = program.getSourceFile(entryFile);
  if (!entry) throw new Error('cannot read ' + entryFile);
  const root = path.dirname(entryFile);

  // ---- the modules of the bundle: the entry and what it reaches by relative imports and exports
  const isRelative = (spec) => spec.startsWith('.') || spec.startsWith('/');
  const internal = (file) => !file.isDeclarationFile ? false : file.fileName.startsWith(root + path.sep) || file.fileName === entryFile;
  const resolveInternal = (from, spec) => {
    if (!isRelative(spec)) return null;
    if (/\.d\.[mc]?ts$/.test(spec)) return program.getSourceFile(path.resolve(path.dirname(from.fileName), spec)) ?? null;
    const base = path.resolve(path.dirname(from.fileName), spec).replace(/\.(m|c)?[jt]s$/, '');
    for (const candidate of [base + '.d.ts', base + '.d.mts', base + '.d.cts', path.join(base, 'index.d.ts')]) {
      const found = program.getSourceFile(candidate);
      if (found) return found;
    }
    return null;
  };
  const order = [];
  const seen = new Set();
  const edges = new Map();
  const visit = (file) => {
    if (seen.has(file.fileName)) return;
    seen.add(file.fileName);
    const targets = [];
    for (const statement of file.statements) {
      const spec = (ts.isImportDeclaration(statement) || ts.isExportDeclaration(statement)) && statement.moduleSpecifier && ts.isStringLiteral(statement.moduleSpecifier) ? statement.moduleSpecifier.text : null;
      if (spec === null) continue;
      const target = resolveInternal(file, spec);
      if (target) {
        targets.push(target);
        visit(target);
      }
    }
    // `import("./x").T` in a type also reaches a module.
    const walk = (node) => {
      if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument) && ts.isStringLiteral(node.argument.literal)) {
        const target = resolveInternal(file, node.argument.literal.text);
        if (target) {
          targets.push(target);
          visit(target);
        }
      }
      ts.forEachChild(node, walk);
    };
    walk(file);
    edges.set(file.fileName, targets);
    order.push(file);
  };
  visit(entry);
  const modules = order;
  const moduleOf = new Map(modules.map((m) => [m.fileName, m]));

  // ---- names: what is declared at the top of each module, and the external imports
  const topLevelSymbols = new Map(); // ts.Symbol -> { file, name }
  const used = new Set();
  const finalName = new Map(); // ts.Symbol -> final name
  const uniqueName = (name) => {
    let candidate = name;
    for (let n = 1; used.has(candidate); n++) candidate = `${name}$${n}`;
    used.add(candidate);
    return candidate;
  };
  const declName = (statement) => {
    if ((ts.isFunctionDeclaration(statement) || ts.isClassDeclaration(statement) || ts.isInterfaceDeclaration(statement) || ts.isTypeAliasDeclaration(statement) || ts.isEnumDeclaration(statement) || ts.isModuleDeclaration(statement)) && statement.name && ts.isIdentifier(statement.name)) return [statement.name];
    if (ts.isVariableStatement(statement)) return statement.declarationList.declarations.map((d) => d.name).filter(ts.isIdentifier);
    return [];
  };
  // Names the bundle's own declarations keep when they can; the first module in order gets them first.
  for (const file of modules) {
    for (const statement of file.statements) {
      for (const name of declName(statement)) {
        const symbol = checker.getSymbolAtLocation(name);
        if (!symbol || topLevelSymbols.has(symbol)) continue;
        topLevelSymbols.set(symbol, { file, name: name.text });
        finalName.set(symbol, uniqueName(name.text));
      }
    }
  }
  // External imports: one local name for each thing imported from a package.
  const externals = new Map(); // key -> { spec, kind, imported, local }
  const externalKey = (spec, kind, imported) => `${spec}\u0000${kind}\u0000${imported}`;
  const externalFor = (spec, kind, imported, preferred) => {
    const key = externalKey(spec, kind, imported);
    let found = externals.get(key);
    if (!found) {
      found = { spec, kind, imported, local: uniqueName(preferred) };
      externals.set(key, found);
    }
    return found;
  };
  const importBinding = (declaration) => {
    // The specifier, kind and imported name of an import binding declaration.
    if (ts.isImportSpecifier(declaration)) {
      const clause = declaration.parent.parent;
      const spec = clause.parent.moduleSpecifier.text;
      return { spec, kind: 'named', imported: (declaration.propertyName ?? declaration.name).text, local: declaration.name.text };
    }
    if (ts.isImportClause(declaration)) return { spec: declaration.parent.moduleSpecifier.text, kind: 'default', imported: 'default', local: declaration.name.text };
    if (ts.isNamespaceImport(declaration)) return { spec: declaration.parent.parent.moduleSpecifier.text, kind: 'namespace', imported: '*', local: declaration.name.text };
    return null;
  };

  // ---- namespaces standing for modules that are imported or re-exported as a whole
  const namespaces = new Map(); // file name -> name
  const namespaceBlocks = [];
  const exportsOf = (file) => {
    const moduleSymbol = checker.getSymbolAtLocation(file);
    return moduleSymbol ? checker.getExportsOfModule(moduleSymbol) : [];
  };
  const resolveAlias = (symbol) => {
    let current = symbol;
    for (let i = 0; i < 20 && current.flags & ts.SymbolFlags.Alias; i++) {
      const next = checker.getAliasedSymbol(current);
      if (!next || next === current || next.name === 'unknown') return { external: current };
      current = next;
    }
    return { symbol: current };
  };
  const namespaceFor = (file, preferred) => {
    let name = namespaces.get(file.fileName);
    if (name) return name;
    name = uniqueName(preferred);
    namespaces.set(file.fileName, name);
    const members = [];
    for (const exported of exportsOf(file)) {
      const text = publicBinding(exported, `${name}_${exported.name}`);
      if (text) members.push(text === exported.name ? text : `${text} as ${exported.name}`);
    }
    namespaceBlocks.push(`declare namespace ${name} {\n    export { ${members.join(', ')} };\n}`);
    return name;
  };
  // The name an export has in the bundle, or null when it cannot be named (a type-only thing from nowhere).
  function publicBinding(exported, preferredNamespace) {
    const resolved = resolveAlias(exported);
    if (resolved.external) {
      const declaration = resolved.external.declarations?.[0];
      const binding = declaration && importBinding(declaration);
      if (binding) return externalFor(binding.spec, binding.kind, binding.imported, binding.local).local;
      return null;
    }
    const target = resolved.symbol;
    if (finalName.has(target)) return finalName.get(target);
    if (target.flags & ts.SymbolFlags.ValueModule && target.declarations?.some(ts.isSourceFile)) {
      const file = target.declarations.find(ts.isSourceFile);
      if (moduleOf.has(file.fileName)) return namespaceFor(file, preferredNamespace);
    }
    return null;
  }

  // ---- the text of each module, with names changed and module statements gone
  const finalFor = (identifier) => {
    const symbol = checker.getSymbolAtLocation(identifier);
    if (!symbol) return null;
    if (symbol.flags & ts.SymbolFlags.Alias) {
      const declaration = symbol.declarations?.[0];
      const binding = declaration && importBinding(declaration);
      if (binding && !isRelative(binding.spec)) return externalFor(binding.spec, binding.kind, binding.imported, binding.local).local;
      if (binding && declaration) {
        if (ts.isNamespaceImport(declaration)) {
          const file = resolveInternal(declaration.getSourceFile(), binding.spec);
          return file ? namespaceFor(file, binding.local) : null;
        }
        const resolved = resolveAlias(symbol);
        if (resolved.symbol && finalName.has(resolved.symbol)) return finalName.get(resolved.symbol);
        if (resolved.external) return publicBinding(symbol, binding.local);
      }
      return null;
    }
    return finalName.get(symbol) ?? null;
  };
  const header = [];
  const bodies = [];
  const emitted = new Set();
  for (const file of modules) {
    const text = file.text;
    const edits = [];
    const removed = [];
    // A reference to another file of the library is a reference to what is in this file now.
    for (const reference of file.referencedFiles) if (!resolveInternal(file, reference.fileName.startsWith('.') ? reference.fileName : './' + reference.fileName)) header.push(`/// <reference path="${reference.fileName}" />`);
    for (const reference of file.typeReferenceDirectives) header.push(`/// <reference types="${reference.fileName}" />`);
    for (const statement of file.statements) {
      if (ts.isImportDeclaration(statement) || ts.isExportDeclaration(statement) || ts.isExportAssignment(statement) || ts.isImportEqualsDeclaration(statement)) {
        if (ts.isExportAssignment(statement) && statement.isExportEquals) throw new Error(`export = is not supported in ${file.fileName}`);
        removed.push([statement.getFullStart(), statement.getEnd()]);
        continue;
      }
      // `export` / `export default` in front of a declaration
      const modifiers = ts.canHaveModifiers(statement) ? ts.getModifiers(statement) ?? [] : [];
      for (const modifier of modifiers) {
        if (modifier.kind === ts.SyntaxKind.ExportKeyword || modifier.kind === ts.SyntaxKind.DefaultKeyword) edits.push([modifier.getStart(), modifier.getEnd() + 1, '']);
      }
      // `export default function f()` has no `declare`, which the same thing needs once it is not exported.
      const needsDeclare = (ts.isFunctionDeclaration(statement) || ts.isClassDeclaration(statement) || ts.isEnumDeclaration(statement) || ts.isVariableStatement(statement) || (ts.isModuleDeclaration(statement) && !(statement.flags & ts.NodeFlags.GlobalAugmentation))) && modifiers.some((m) => m.kind === ts.SyntaxKind.ExportKeyword) && !modifiers.some((m) => m.kind === ts.SyntaxKind.DeclareKeyword);
      if (needsDeclare) {
        const firstModifier = modifiers[0];
        edits.push([firstModifier.getStart(), firstModifier.getStart(), 'declare ']);
      }
    }
    const walk = (node) => {
      if (removed.some(([a, b]) => node.getStart() >= a && node.getEnd() <= b)) return;
      if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument) && ts.isStringLiteral(node.argument.literal)) {
        const spec = node.argument.literal.text;
        const targetFile = resolveInternal(file, spec);
        if (targetFile && node.qualifier) {
          // `import("./x").T<A>` is the name T has in the bundle (A is rewritten on its own).
          const left = ts.isIdentifier(node.qualifier) ? node.qualifier : leftmost(node.qualifier);
          const exported = exportsOf(targetFile).find((e) => e.name === left.text);
          const name = exported && publicBinding(exported, left.text);
          if (name) {
            const tail = node.typeArguments ? node.typeArguments.pos - 1 : node.getEnd();
            const qualifierEnd = node.qualifier.getEnd();
            const rest = text.slice(left.getEnd(), qualifierEnd);
            edits.push([node.getStart(), Math.min(tail, qualifierEnd), name + rest]);
            if (node.typeArguments) node.typeArguments.forEach(walk);
            return;
          }
        } else if (!targetFile && !isRelative(spec)) {
          // an external `import("pkg").T` stays as it is
        } else if (targetFile) throw new Error(`typeof import("${spec}") is not supported in ${file.fileName}`);
        return;
      }
      if (ts.isIdentifier(node)) {
        const parent = node.parent;
        const isMemberName = (ts.isQualifiedName(parent) && parent.right === node) || (ts.isPropertyAccessExpression(parent) && parent.name === node);
        if (!isMemberName && !(ts.isPropertySignature(parent) && parent.name === node) && !(ts.isMethodSignature(parent) && parent.name === node) && !(ts.isPropertyDeclaration(parent) && parent.name === node) && !(ts.isMethodDeclaration(parent) && parent.name === node) && !(ts.isParameter(parent) && parent.name === node) && !(ts.isEnumMember(parent) && parent.name === node)) {
          const renamed = finalFor(node);
          if (renamed && renamed !== node.text) edits.push([node.getStart(), node.getEnd(), renamed]);
        }
        return;
      }
      ts.forEachChild(node, walk);
    };
    const leftmost = (qualified) => (ts.isQualifiedName(qualified) ? leftmost(qualified.left) : qualified);
    for (const statement of file.statements) {
      if (removed.some(([a]) => a === statement.getFullStart())) continue;
      walk(statement);
    }
    // Apply: remove the module statements and make the edits, from the end.
    const all = [...edits, ...removed.map(([a, b]) => [a, b, ''])].sort((x, y) => y[0] - x[0]);
    let out = text;
    for (const [a, b, replacement] of all) out = out.slice(0, a) + replacement + out.slice(b);
    bodies.push(out.replace(/^\s*\/\/\/ <reference [^\n]*\n/gm, '').trim());
  }

  // ---- what the entry exports
  const exportList = [];
  const externalExports = [];
  for (const exported of exportsOf(entry)) {
    const resolved = resolveAlias(exported);
    if (resolved.external) {
      // A re-export from a package: kept as it was.
      const declaration = resolved.external.declarations?.[0];
      if (declaration && ts.isExportSpecifier(declaration)) {
        const decl = declaration.parent.parent;
        const spec = decl.moduleSpecifier.text;
        externalExports.push(`export { ${declaration.propertyName ? `${declaration.propertyName.text} as ` : ''}${exported.name} } from '${spec}';`);
        continue;
      }
      const binding = declaration && importBinding(declaration);
      if (binding) {
        const local = externalFor(binding.spec, binding.kind, binding.imported, binding.local).local;
        exportList.push(local === exported.name ? local : `${local} as ${exported.name}`);
      }
      continue;
    }
    const name = publicBinding(exported, exported.name);
    if (name) exportList.push(name === exported.name ? name : `${name} as ${exported.name}`);
  }
  // `export * from "pkg"` anywhere in the chain of star exports
  for (const file of modules) {
    for (const statement of file.statements) {
      if (ts.isExportDeclaration(statement) && !statement.exportClause && statement.moduleSpecifier && ts.isStringLiteral(statement.moduleSpecifier) && !isRelative(statement.moduleSpecifier.text)) {
        externalExports.push(`export * from '${statement.moduleSpecifier.text}';`);
      }
    }
  }

  // ---- put it together
  const imports = [];
  const bySpec = new Map();
  for (const e of externals.values()) {
    if (!bySpec.has(e.spec)) bySpec.set(e.spec, []);
    bySpec.get(e.spec).push(e);
  }
  for (const [spec, list] of bySpec) {
    const named = list.filter((e) => e.kind === 'named').map((e) => (e.imported === e.local ? e.local : `${e.imported} as ${e.local}`));
    const def = list.find((e) => e.kind === 'default');
    const ns = list.find((e) => e.kind === 'namespace');
    if (ns) imports.push(`import * as ${ns.local} from '${spec}';`);
    if (def || named.length) imports.push(`import ${[def ? def.local : null, named.length ? `{ ${named.join(', ')} }` : null].filter(Boolean).join(', ')} from '${spec}';`);
  }
  const unique = (list) => [...new Set(list)];
  const parts = [...unique(header), ...imports, ...bodies, ...namespaceBlocks, `export { ${unique(exportList).join(', ')} };`, ...unique(externalExports)];
  return parts.filter(Boolean).join('\n\n') + '\n';
}

// ---- TypeScript 7 -------------------------------------------------------------------------------------------------

// TypeScript 7 has no JavaScript API to build a program with, but its command line writes the same declarations. Its `tsc` is found in
// the project's `typescript` package.
function nativeTypeScript() {
  try {
    const require = createRequire(path.join(cwd, 'noop.js'));
    const manifest = require('typescript/package.json');
    if (Number(String(manifest.version).split('.')[0]) < 7) return null;
    const bin = path.join(path.dirname(require.resolve('typescript/package.json')), typeof manifest.bin === 'string' ? manifest.bin : manifest.bin?.tsc ?? 'bin/tsc');
    return fs.existsSync(bin) ? { version: manifest.version, bin } : null;
  } catch {
    return null;
  }
}

function findConfigFile(tsConfigPath) {
  if (tsConfigPath) return path.resolve(cwd, tsConfigPath);
  for (let dir = cwd; ; dir = path.dirname(dir)) {
    const file = path.join(dir, 'tsconfig.json');
    if (fs.existsSync(file)) return file;
    if (dir === path.dirname(dir)) return null;
  }
}

// Declarations for `entries` written by the TypeScript 7 command line, to the same layout the API route gives: from the folder the sources
// share. A tsconfig of ours (the project's, with what --dts needs on top) is written for the run, because the command line takes either
// files or a tsconfig, not both.
function emitNative(native, { entries, outDir, tsConfigPath }) {
  const out = path.resolve(cwd, outDir);
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'rathkit-dts-native-'));
  try {
    const defaults = path.join(work, 'defaults.json');
    fs.writeFileSync(defaults, JSON.stringify({ compilerOptions: { target: 'es2020', module: 'esnext', moduleResolution: 'bundler', jsx: 'react-jsx', esModuleInterop: true, allowJs: true, skipLibCheck: true } }));
    const project = findConfigFile(tsConfigPath);
    const config = (rootDir) => ({
      extends: project ? [defaults, project] : [defaults],
      files: entries.map((e) => path.resolve(cwd, e)),
      include: [],
      compilerOptions: { declaration: true, emitDeclarationOnly: true, noEmit: false, outDir: out, declarationDir: out, ...(rootDir ? { rootDir } : {}), declarationMap: false, sourceMap: false, composite: false, incremental: false, noEmitOnError: false, skipLibCheck: true, allowJs: true },
    });
    const run = (extra, rootDir) => {
      const file = path.join(work, 'tsconfig.json');
      fs.writeFileSync(file, JSON.stringify(config(rootDir)));
      const result = spawnSync(process.execPath, [native.bin, '--project', file, '--pretty', 'false', ...extra], { cwd, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
      if (result.error) throw new Error(`the TypeScript compiler could not be run: ${result.error.message}`);
      return result;
    };
    // Version 7 does not guess the folder the sources share, so it is found first: the files the program reads, less libraries and declarations.
    const listed = run(['--listFilesOnly']).stdout.split(/\r?\n/).filter((line) => path.isAbsolute(line));
    const sources = listed.filter((f) => !/\.d\.[mc]?ts$/.test(f) && !f.split(path.sep).includes('node_modules')).map((f) => path.dirname(f));
    const common = sources.reduce((a, b) => {
      let dir = a;
      while (!(b + path.sep).startsWith(dir + path.sep) && dir !== path.dirname(dir)) dir = path.dirname(dir);
      return dir;
    }, sources[0] ?? cwd);
    const result = run(['--listEmittedFiles'], common);
    let errors = 0;
    for (const line of result.stdout.split(/\r?\n/)) {
      const m = /^(.+?)\((\d+),\d+\): error TS\d+: (.*)$/.exec(line);
      const bare = /^error TS\d+: (.*)$/.exec(line);
      if (!m && !bare) continue;
      errors++;
      if (errors <= 20) note(m ? `${m[1]}:${m[2]}: ${m[3]}` : bare[1]);
    }
    const written = result.stdout.split(/\r?\n/).filter((l) => l.startsWith('TSFILE: ')).map((l) => l.slice(8)).filter((f) => /\.d\.[mc]?ts$/.test(f)).map((f) => path.relative(out, f).split(path.sep).join('/')).sort();
    if (!written.length) return { error: 'no declarations were written' };
    return { written, errors, common };
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

// ---- a reader of declaration files, for TypeScript 7 -------------------------------------------------------------------

// TypeScript 7 is the native compiler and its package has no JavaScript API to read a program with. The declaration files that --dts-bundle
// merges are what `tsc --declaration` writes, a plain and regular subset of the language, so they are read here: what `bundleDeclarations`
// asks of the `typescript` package (statements, the names in them and which declaration each names, the exports of a file and what an import
// or re-export stands for) is answered from a scan of the text. The answers have the shape of the TypeScript API, so the merge is the same code.
function declarationReader() {
  const K = { SourceFile: 1, Identifier: 2, StringLiteral: 3, ImportDeclaration: 4, ExportDeclaration: 5, ExportAssignment: 6, ImportEqualsDeclaration: 7, FunctionDeclaration: 8, ClassDeclaration: 9, InterfaceDeclaration: 10, TypeAliasDeclaration: 11, EnumDeclaration: 12, ModuleDeclaration: 13, VariableStatement: 14, VariableDeclaration: 15, VariableDeclarationList: 16, ImportClause: 17, NamespaceImport: 18, NamedImports: 19, ImportSpecifier: 20, NamedExports: 21, ExportSpecifier: 22, NamespaceExport: 23, QualifiedName: 24, ImportType: 25, LiteralType: 26, OtherStatement: 27, ExportKeyword: 28, DefaultKeyword: 29, DeclareKeyword: 30, OtherModifier: 31, TypeArguments: 32, TypeReference: 33 };
  const SymbolFlags = { Alias: 1 << 21, ValueModule: 1 << 9 };
  const NodeFlags = { GlobalAugmentation: 1 << 11 };
  const keywords = new Set('string number boolean any unknown never void object symbol bigint null undefined true false this typeof keyof unique readonly infer extends implements new in is asserts out declare export default abstract static public private protected override accessor function class interface enum import var let const namespace module global as from satisfies of delete super require'.split(' '));
  const modifierWords = new Set(['readonly', 'static', 'public', 'private', 'protected', 'abstract', 'override', 'declare', 'async', 'accessor', 'export', 'default', 'get', 'set']);
  const isModuleKeyword = new Set(['interface', 'class', 'namespace', 'module', 'enum', 'global']);

  function tokenize(text) {
    const tokens = [];
    const n = text.length;
    let i = 0;
    let braces = 0;
    const substitutions = [];
    const push = (type, start, end) => tokens.push({ type, start, end, value: text.slice(start, end) });
    const template = () => {
      while (i < n) {
        const c = text[i];
        if (c === '\\') i += 2;
        else if (c === '`') { i++; return false; }
        else if (c === '$' && text[i + 1] === '{') { i += 2; return true; }
        else i++;
      }
      return false;
    };
    while (i < n) {
      const c = text[i];
      if (c === ' ' || c === '\n' || c === '\r' || c === '\t' || c === '﻿' || c === ' ' || c === ' ' || c === ' ') { i++; continue; }
      if (c === '/' && text[i + 1] === '/') { while (i < n && text[i] !== '\n') i++; continue; }
      if (c === '/' && text[i + 1] === '*') { const end = text.indexOf('*/', i + 2); i = end < 0 ? n : end + 2; continue; }
      const start = i;
      if (c === '"' || c === "'") {
        i++;
        while (i < n && text[i] !== c) i += text[i] === '\\' ? 2 : 1;
        i++;
        push('string', start, i);
      } else if (c === '`') {
        i++;
        if (template()) substitutions.push(braces);
        push('template', start, i);
      } else if (c === '}' && substitutions.length && substitutions[substitutions.length - 1] === braces) {
        substitutions.pop();
        i++;
        if (template()) substitutions.push(braces);
        push('template', start, i);
      } else if (/[A-Za-z_$\u0080-￿]/.test(c)) {
        i++;
        while (i < n && /[\w$\u0080-￿]/.test(text[i])) i++;
        push('id', start, i);
      } else if (/[0-9]/.test(c) || (c === '.' && /[0-9]/.test(text[i + 1] ?? ''))) {
        i++;
        while (i < n && /[\w.]/.test(text[i])) i++;
        push('num', start, i);
      } else if (c === '=' && text[i + 1] === '>') { i += 2; push('p', start, i); }
      else if (c === '.' && text[i + 1] === '.' && text[i + 2] === '.') { i += 3; push('p', start, i); }
      else {
        i++;
        if (c === '{') braces++;
        else if (c === '}') braces--;
        push('p', start, i);
      }
    }
    return tokens;
  }

  const files = new Map();
  const loadFile = (fileName) => {
    const name = path.resolve(fileName);
    if (files.has(name)) return files.get(name);
    let text;
    try {
      text = fs.readFileSync(name, 'utf8');
    } catch {
      return null; // missing, or a folder
    }
    return parseFile(name, text);
  };

  const tools = (file) => ({
    getStart() { return this.start; },
    getEnd() { return this.end; },
    getFullStart() { return this.fullStart ?? this.start; },
    getSourceFile() { return file; },
  });

  function parseFile(fileName, text) {
    const file = { kind: K.SourceFile, fileName, text, isDeclarationFile: true, statements: [], referencedFiles: [], typeReferenceDirectives: [], symbols: new Map(), exportTable: new Map(), stars: [], flags: 0, start: 0, end: text.length, fullStart: 0 };
    files.set(fileName, file);
    Object.assign(file, tools(file));
    const head = text.slice(0, (text.search(/^[^\s/]|\n\s*[^\s/*]/m) + 1) || 0);
    for (const m of text.matchAll(/^\s*\/\/\/\s*<reference\s+path="([^"]*)"\s*\/>/gm)) file.referencedFiles.push({ fileName: m[1] });
    for (const m of text.matchAll(/^\s*\/\/\/\s*<reference\s+types="([^"]*)"\s*\/>/gm)) file.typeReferenceDirectives.push({ fileName: m[1] });
    void head;
    const tokens = tokenize(text);
    const make = (kind, start, end, extra = {}) => Object.assign({ kind, start, end, parent: undefined }, tools(file), extra);
    const ident = (tok, parent, extra = {}) => make(K.Identifier, tok.start, tok.end, { text: tok.value, parent, ...extra });
    const stringLiteral = (tok, parent) => make(K.StringLiteral, tok.start, tok.end, { text: tok.value.slice(1, -1), parent });
    let t = 0;
    let previousEnd = 0;
    while (t < tokens.length) {
      const s = t;
      // The words in front: modifiers, then what kind of statement it is.
      let k = s;
      const modifiers = [];
      for (;;) {
        const tok = tokens[k];
        if (!tok || tok.type !== 'id') break;
        const next = tokens[k + 1];
        if (tok.value === 'export' && next && (next.value === '{' || next.value === '*' || next.value === '=' || (next.value === 'type' && (tokens[k + 2]?.value === '{' || tokens[k + 2]?.value === '*')) || next.value === 'as' || next.value === 'import')) break;
        if (tok.value === 'export') modifiers.push(make(K.ExportKeyword, tok.start, tok.end));
        else if (tok.value === 'default' && next && !['function', 'class', 'interface', 'abstract', 'enum'].includes(next.value)) break;
        else if (tok.value === 'default') modifiers.push(make(K.DefaultKeyword, tok.start, tok.end));
        else if (tok.value === 'declare') modifiers.push(make(K.DeclareKeyword, tok.start, tok.end));
        else if ((tok.value === 'abstract' || tok.value === 'async') && next?.type === 'id') modifiers.push(make(K.OtherModifier, tok.start, tok.end));
        else if (tok.value === 'const' && next?.value === 'enum') modifiers.push(make(K.OtherModifier, tok.start, tok.end));
        else break;
        k++;
      }
      const word = tokens[k]?.type === 'id' ? tokens[k].value : '';
      const braceDeclaration = isModuleKeyword.has(word);
      // Where the statement ends: a `;` outside brackets, or for a declaration with a body its closing brace.
      let depth = 0;
      let e = s;
      for (let j = s; j < tokens.length; j++) {
        e = j;
        const tok = tokens[j];
        if (tok.type === 'p') {
          if (tok.value === '(' || tok.value === '[' || tok.value === '{') depth++;
          else if (tok.value === ')' || tok.value === ']' || tok.value === '}') depth--;
          if (tok.value === ';' && depth === 0) break;
          if (tok.value === '}' && depth === 0 && braceDeclaration) {
            const next = tokens[j + 1];
            if (!(next && (['>', ',', '{', '&', '|', '[', ']', ')', '=', '.'].includes(next.value) || (next.type === 'id' && (next.value === 'extends' || next.value === 'implements'))))) {
              if (next && next.value === ';') e = j + 1;
              break;
            }
          }
        }
        if (j === tokens.length - 1) e = j;
      }
      t = e + 1;
      if (tokens[s].value === ';' && e === s) { continue; }
      const statement = make(K.OtherStatement, tokens[s].start, tokens[e].end, { fullStart: previousEnd, modifiers, flags: 0, _kids: [] });
      previousEnd = statement.end;
      statement.parent = file;
      let nameToken = null;
      const bindings = [];
      const first = tokens[k];
      if (word === 'import' && !modifiers.length) {
        // import [type] [default][, * as ns | { a as b }] from '...'  /  import '...'  /  import a = require('...')
        let j = k + 1;
        if (tokens[j]?.value === 'type' && (tokens[j + 1]?.type === 'id' ? tokens[j + 1].value !== 'from' : tokens[j + 1]?.value === '{' || tokens[j + 1]?.value === '*')) j++;
        const equals = (() => { for (let q = j; q <= e; q++) if (tokens[q].value === '=') return true; else if (tokens[q].value === 'from' || tokens[q].type === 'string') return false; return false; })();
        if (equals) statement.kind = K.ImportEqualsDeclaration;
        else {
          statement.kind = K.ImportDeclaration;
          const clause = make(K.ImportClause, tokens[j].start, tokens[e].end, { parent: statement });
          if (tokens[j].type === 'string') {
            statement.moduleSpecifier = stringLiteral(tokens[j], statement);
          } else {
            if (tokens[j].type === 'id' && tokens[j].value !== 'from') {
              clause.name = ident(tokens[j], clause);
              bindings.push({ local: tokens[j].value, flags: SymbolFlags.Alias, declaration: clause });
              j++;
              if (tokens[j]?.value === ',') j++;
            }
            if (tokens[j]?.value === '*') {
              const ns = make(K.NamespaceImport, tokens[j].start, tokens[j + 2].end, { parent: clause });
              ns.name = ident(tokens[j + 2], ns);
              clause.namedBindings = ns;
              bindings.push({ local: tokens[j + 2].value, flags: SymbolFlags.Alias, declaration: ns });
              j += 3;
            } else if (tokens[j]?.value === '{') {
              const named = make(K.NamedImports, tokens[j].start, tokens[e].end, { parent: clause, elements: [] });
              clause.namedBindings = named;
              j++;
              while (tokens[j] && tokens[j].value !== '}') {
                if (tokens[j].value === ',') { j++; continue; }
                if (tokens[j].value === 'type' && tokens[j + 1]?.type === 'id' && tokens[j + 1].value !== 'as') j++;
                const spec = make(K.ImportSpecifier, tokens[j].start, tokens[j].end, { parent: named });
                if (tokens[j + 1]?.value === 'as') {
                  spec.propertyName = ident(tokens[j], spec);
                  spec.name = ident(tokens[j + 2], spec);
                  j += 3;
                } else {
                  spec.name = ident(tokens[j], spec);
                  j++;
                }
                named.elements.push(spec);
                bindings.push({ local: spec.name.text, flags: SymbolFlags.Alias, declaration: spec });
              }
              j++;
            }
            if (tokens[j]?.value === 'from') j++;
            statement.moduleSpecifier = tokens[j]?.type === 'string' ? stringLiteral(tokens[j], statement) : undefined;
          }
          if (clause.name || clause.namedBindings) statement.importClause = clause;
        }
      } else if (word === 'export' && !modifiers.length && tokens[k + 1]) {
        let j = k + 1;
        if (tokens[j].value === 'type' && (tokens[j + 1]?.value === '{' || tokens[j + 1]?.value === '*')) j++;
        const next = tokens[j];
        if (next.value === '=') {
          statement.kind = K.ExportAssignment;
          statement.isExportEquals = true;
        } else if (next.value === 'default') {
          statement.kind = K.ExportAssignment;
          statement.isExportEquals = false;
          statement.expression = tokens[j + 1]?.type === 'id' && tokens[j + 2]?.value === ';' ? tokens[j + 1].value : null;
        } else if (next.value === 'as') {
          statement.kind = K.OtherStatement;
        } else if (next.value === 'import') {
          statement.kind = K.ImportEqualsDeclaration;
        } else {
          statement.kind = K.ExportDeclaration;
          if (next.value === '*') {
            j++;
            if (tokens[j]?.value === 'as') {
              const ns = make(K.NamespaceExport, tokens[j].start, tokens[j + 1].end, { parent: statement });
              ns.name = ident(tokens[j + 1], ns);
              statement.exportClause = ns;
              j += 2;
            }
          } else {
            const named = make(K.NamedExports, next.start, tokens[e].end, { parent: statement, elements: [] });
            statement.exportClause = named;
            j++;
            while (tokens[j] && tokens[j].value !== '}') {
              if (tokens[j].value === ',') { j++; continue; }
              if (tokens[j].value === 'type' && tokens[j + 1]?.type === 'id' && tokens[j + 1].value !== 'as') j++;
              const spec = make(K.ExportSpecifier, tokens[j].start, tokens[j].end, { parent: named });
              if (tokens[j + 1]?.value === 'as') {
                spec.propertyName = ident(tokens[j], spec);
                spec.name = ident(tokens[j + 2], spec);
                j += 3;
              } else {
                spec.name = ident(tokens[j], spec);
                j++;
              }
              named.elements.push(spec);
            }
            j++;
          }
          if (tokens[j]?.value === 'from') statement.moduleSpecifier = stringLiteral(tokens[j + 1], statement);
        }
      } else if (word === 'default') {
        statement.kind = K.ExportAssignment;
        statement.isExportEquals = false;
        statement.expression = tokens[k + 1]?.type === 'id' && tokens[k + 2]?.value === ';' ? tokens[k + 1].value : null;
      } else if (word === 'export' && modifiers.length === 0) {
        statement.kind = K.OtherStatement;
      } else if (word === 'function') {
        statement.kind = K.FunctionDeclaration;
        nameToken = tokens[k + 1]?.type === 'id' ? tokens[k + 1] : tokens[k + 2]?.type === 'id' ? tokens[k + 2] : null;
      } else if (word === 'class') {
        statement.kind = K.ClassDeclaration;
        nameToken = tokens[k + 1]?.type === 'id' && !['extends', 'implements'].includes(tokens[k + 1].value) ? tokens[k + 1] : null;
      } else if (word === 'interface') { statement.kind = K.InterfaceDeclaration; nameToken = tokens[k + 1]; }
      else if (word === 'type' && tokens[k + 1]?.type === 'id') { statement.kind = K.TypeAliasDeclaration; nameToken = tokens[k + 1]; }
      else if (word === 'enum' || (word === 'const' && tokens[k + 1]?.value === 'enum')) { statement.kind = K.EnumDeclaration; nameToken = tokens[word === 'enum' ? k + 1 : k + 2]; }
      else if (word === 'namespace' || word === 'module' || word === 'global') {
        statement.kind = K.ModuleDeclaration;
        if (word === 'global') { statement.flags |= NodeFlags.GlobalAugmentation; nameToken = first; }
        else if (tokens[k + 1]?.type === 'string') statement.name = stringLiteral(tokens[k + 1], statement);
        else nameToken = tokens[k + 1];
      } else if (word === 'var' || word === 'let' || word === 'const') {
        statement.kind = K.VariableStatement;
        const list = make(K.VariableDeclarationList, tokens[k].start, tokens[e].end, { parent: statement, declarations: [] });
        statement.declarationList = list;
        let d = 0;
        for (let j = k + 1; j <= e; j++) {
          const tok = tokens[j];
          if (tok.type === 'p' && '([{'.includes(tok.value)) d++;
          else if (tok.type === 'p' && ')]}'.includes(tok.value)) d--;
          if (d === 0 && tok.type === 'id' && (j === k + 1 || tokens[j - 1].value === ',') && !keywords.has(tok.value)) {
            const declaration = make(K.VariableDeclaration, tok.start, tok.end, { parent: list });
            declaration.name = ident(tok, declaration);
            list.declarations.push(declaration);
          }
        }
      }
      if (nameToken && statement.kind !== K.ModuleDeclaration) statement.name = ident(nameToken, statement);
      if (nameToken && statement.kind === K.ModuleDeclaration) statement.name = ident(nameToken, statement);
      statement.nameToken = nameToken;
      statement.bindings = bindings;
      if (statement.kind >= K.FunctionDeclaration && statement.kind <= K.VariableStatement) scanBody(file, tokens, statement, s, k, e, text);
      file.statements.push(statement);
      statement.parent = file;
      for (const child of statement._kids ?? []) child.parent ??= statement;
    }
    file._kids = file.statements;
    index(file);
    return file;
  }

  // The identifiers of a declaration that stand for something declared elsewhere, and the `import("...")` types, in the order they are written.
  function scanBody(file, tokens, statement, s, k, e, text) {
    const make = (kind, start, end, extra = {}) => Object.assign({ kind, start, end, parent: undefined }, tools(file), extra);
    const locals = new Set();
    const nameIndex = statement.nameToken ? tokens.indexOf(statement.nameToken) : -1;
    // Names bound inside: type parameters, `infer`, mapped types' keys, and what a namespace declares for itself.
    const match = new Map();
    const open = [];
    for (let i = k; i <= e; i++) {
      const tok = tokens[i];
      if (tok.type !== 'p') continue;
      if (tok.value === '<') open.push({ ch: '<', i });
      else if (tok.value === '>') { if (open.length && open[open.length - 1].ch === '<') match.set(open.pop().i, i); }
      else if (tok.value === '(' || tok.value === '[' || tok.value === '{') open.push({ ch: tok.value, i });
      else if (tok.value === ')' || tok.value === ']' || tok.value === '}') { while (open.length && open.pop().ch === '<'); }
    }
    for (let i = k; i <= e; i++) {
      const tok = tokens[i];
      if (tok.type === 'id' && tok.value === 'infer' && tokens[i + 1]?.type === 'id') locals.add(tokens[i + 1].value);
      if (tok.value === '[' && tokens[i + 1]?.type === 'id' && tokens[i + 2]?.value === 'in') locals.add(tokens[i + 1].value);
      if (tok.value === '<' && match.has(i)) {
        const close = match.get(i);
        const after = tokens[close + 1];
        const previous = tokens[i - 1];
        const nested = statement.kind === K.ModuleDeclaration && previous?.type === 'id' && ['interface', 'class', 'type', 'function'].includes(tokens[i - 2]?.value);
        if ((after && after.value === '(') || i - 1 === nameIndex || nested) {
          let depth = 0;
          let wantsName = true;
          for (let j = i + 1; j < close; j++) {
            const inner = tokens[j];
            if (inner.type === 'p' && '([{<'.includes(inner.value)) depth++;
            else if (inner.type === 'p' && ')]}>'.includes(inner.value)) depth--;
            if (depth === 0 && inner.value === ',') { wantsName = true; continue; }
            if (wantsName && depth === 0 && inner.type === 'id') {
              if (['const', 'in', 'out'].includes(inner.value) && tokens[j + 1]?.type === 'id') continue;
              locals.add(inner.value);
              wantsName = false;
            }
          }
        }
      }
      if (statement.kind === K.ModuleDeclaration && tok.type === 'id' && i > nameIndex + 1) {
        if (['interface', 'class', 'function', 'enum', 'namespace', 'module', 'var', 'let', 'const'].includes(tok.value) && tokens[i + 1]?.type === 'id') locals.add(tokens[i + 1].value);
        if (tok.value === 'type' && tokens[i + 1]?.type === 'id' && (tokens[i + 2]?.value === '=' || tokens[i + 2]?.value === '<')) locals.add(tokens[i + 1].value);
      }
    }
    statement.locals = locals;
    // The leaves.
    const declared = new Set();
    if (statement.name?.kind === K.Identifier) declared.add(statement.name);
    const nameLeaves = new Set();
    const leaves = [];
    const nameOfDeclarations = statement.declarationList ? statement.declarationList.declarations.map((d) => d.name) : [];
    for (const n of nameOfDeclarations) nameLeaves.add(n.start);
    if (statement.name?.kind === K.Identifier && statement.kind !== K.ModuleDeclaration) nameLeaves.add(statement.name.start);
    if (statement.name?.kind === K.Identifier && statement.kind === K.ModuleDeclaration && statement.nameToken?.value !== 'global') nameLeaves.add(statement.name.start);
    const scan = (from, to, stack, out) => {
      for (let i = from; i <= to; i++) {
        const tok = tokens[i];
        if (tok.type === 'p') {
          if (tok.value === '(' || tok.value === '[' || tok.value === '{' || tok.value === '<') stack.push(tok.value);
          else if (tok.value === ')' || tok.value === ']' || tok.value === '}') { while (stack.length && stack.pop() === '<'); }
          else if (tok.value === '>') { if (stack[stack.length - 1] === '<') stack.pop(); }
          continue;
        }
        if (tok.type !== 'id') continue;
        // import("...").A.B<C>
        if (tok.value === 'import' && tokens[i + 1]?.value === '(' && tokens[i + 2]?.type === 'string' && tokens[i + 3]?.value === ')') {
          const typeOf = tokens[i - 1]?.value === 'typeof';
          const node = make(K.ImportType, typeOf ? tokens[i - 1].start : tok.start, tokens[i + 3].end, { _kids: [] });
          const literal = make(K.StringLiteral, tokens[i + 2].start, tokens[i + 2].end, { text: tokens[i + 2].value.slice(1, -1) });
          node.argument = make(K.LiteralType, literal.start, literal.end, { literal });
          let j = i + 4;
          let qualifier;
          while (tokens[j]?.value === '.' && tokens[j + 1]?.type === 'id') {
            const part = make(K.Identifier, tokens[j + 1].start, tokens[j + 1].end, { text: tokens[j + 1].value });
            qualifier = qualifier ? make(K.QualifiedName, qualifier.start, part.end, { left: qualifier, right: part }) : part;
            node.end = part.end;
            j += 2;
          }
          if (qualifier) node.qualifier = qualifier;
          if (tokens[j]?.value === '<' && match.has(j)) {
            const close = match.get(j);
            const inside = [];
            scan(j + 1, close - 1, [...stack, '<'], inside);
            const pseudo = make(K.TypeArguments, tokens[j].end, tokens[close].start, { _kids: inside });
            node.typeArguments = Object.assign([pseudo], { pos: tokens[j].end });
            node._kids = inside;
            node.end = tokens[close].end;
            j = close + 1;
          }
          out.push(node);
          i = j - 1;
          continue;
        }
        const value = tok.value;
        const previous = tokens[i - 1];
        const next = tokens[i + 1];
        const top = stack[stack.length - 1];
        if (nameLeaves.has(tok.start)) {
          out.push(statement.name?.start === tok.start ? statement.name : nameOfDeclarations.find((n) => n.start === tok.start));
          continue;
        }
        if (previous?.value === '.' && previous.type === 'p') continue;
        if (keywords.has(value)) continue;
        if (modifierWords.has(value) && (next?.type === 'id' || next?.type === 'string' || next?.value === '[' || next?.value === '*') && !['=', ':', '?', ',', ';', ')', '>'].includes(next?.value)) continue;
        if (value === 'type' && next?.type === 'id') continue;
        // A name that is a member or a parameter, not a reference.
        const startsMember = previous && (['{', ';', ','].includes(previous.value) || (previous.type === 'id' && modifierWords.has(previous.value)) || previous.value === '(' || previous.value === '[' || previous.value === '...');
        const optional = next?.value === '?';
        const after = optional ? tokens[i + 2] : next;
        if (startsMember && (top === '{' || top === '(' || top === '[')) {
          if (after?.value === ':') continue;
          if (top === '{' && (after?.value === '(' || after?.value === '<' || [';', ',', '}', '='].includes(after?.value ?? ''))) continue;
        }
        if (next?.type === 'id' && next.value === 'is' && previous?.value === ':') continue;
        if (previous?.type === 'id' && previous.value === 'asserts') continue;
        if (locals.has(value)) continue;
        out.push(make(K.Identifier, tok.start, tok.end, { text: value, parent: undefined, ref: true }));
      }
    };
    scan(k, e, [], leaves);
    // A name that follows a dot is a member, and a qualified name's first part is a reference.
    statement._kids = leaves;
    void text;
  }

  // The names a file declares, imports and exports.
  function index(file) {
    const symbol = (name, flags, declaration) => {
      const found = file.symbols.get(name);
      if (found) { found.declarations.push(declaration); return found; }
      const made = { name, flags, declarations: [declaration], file };
      file.symbols.set(name, made);
      return made;
    };
    file.isModule = file.statements.some((s) => [K.ImportDeclaration, K.ExportDeclaration, K.ExportAssignment, K.ImportEqualsDeclaration].includes(s.kind) || s.modifiers?.some((m) => m.kind === K.ExportKeyword));
    for (const statement of file.statements) {
      for (const b of statement.bindings ?? []) {
        const made = { name: b.local, flags: b.flags, declarations: [b.declaration], file };
        file.symbols.set(b.local, made);
      }
      const exported = statement.modifiers?.some((m) => m.kind === K.ExportKeyword);
      const byDefault = statement.modifiers?.some((m) => m.kind === K.DefaultKeyword);
      const names = [];
      if (statement.kind === K.VariableStatement) names.push(...statement.declarationList.declarations.map((d) => d.name.text));
      else if (statement.name?.kind === K.Identifier && !(statement.flags & NodeFlags.GlobalAugmentation) && statement.kind >= K.FunctionDeclaration && statement.kind <= K.ModuleDeclaration) names.push(statement.name.text);
      for (const name of names) {
        const made = symbol(name, 0, statement);
        if (exported) {
          const key = byDefault ? 'default' : name;
          if (byDefault) made.exportName = 'default';
          if (!file.exportTable.has(key)) file.exportTable.set(key, byDefault ? Object.assign(made, { name: 'default' }) : made);
        }
      }
      if (byDefault && exported && !names.length && statement.kind !== K.OtherStatement) file.exportTable.set('default', { name: 'default', flags: 0, declarations: [statement], file });
      if (statement.kind === K.ExportDeclaration) {
        const clause = statement.exportClause;
        if (!clause) { if (statement.moduleSpecifier) file.stars.push(statement); }
        else if (clause.kind === K.NamespaceExport) file.exportTable.set(clause.name.text, { name: clause.name.text, flags: SymbolFlags.Alias, declarations: [clause], file });
        else for (const spec of clause.elements) file.exportTable.set(spec.name.text, { name: spec.name.text, flags: SymbolFlags.Alias, declarations: [spec], file });
      }
      if (statement.kind === K.ExportAssignment && !statement.isExportEquals && statement.expression) {
        file.exportTable.set('default', { name: 'default', flags: SymbolFlags.Alias, declarations: [statement], file });
      }
    }
  }

  const resolveFile = (from, spec) => {
    if (!spec.startsWith('.') && !spec.startsWith('/')) return null;
    if (/\.d\.[mc]?ts$/.test(spec)) return loadFile(path.resolve(path.dirname(from.fileName), spec));
    const base = path.resolve(path.dirname(from.fileName), spec).replace(/\.(m|c)?[jt]s$/, '');
    for (const candidate of [base + '.d.ts', base + '.d.mts', base + '.d.cts', path.join(base, 'index.d.ts')]) {
      const found = loadFile(candidate);
      if (found) return found;
    }
    return null;
  };
  const unknown = { name: 'unknown', flags: 0, declarations: [] };
  const moduleSymbols = new Map();
  const moduleSymbol = (file) => {
    if (!file.isModule) return undefined;
    if (!moduleSymbols.has(file)) moduleSymbols.set(file, { name: `"${file.fileName}"`, flags: SymbolFlags.ValueModule, declarations: [file], file });
    return moduleSymbols.get(file);
  };
  const exportsOf = (file, seen = new Set()) => {
    if (seen.has(file)) return new Map();
    seen.add(file);
    const table = new Map(file.exportTable);
    for (const star of file.stars) {
      const target = resolveFile(file, star.moduleSpecifier.text);
      if (!target) continue;
      for (const [name, symbol] of exportsOf(target, seen)) if (name !== 'default' && !table.has(name)) table.set(name, symbol);
    }
    return table;
  };
  const aliased = (alias) => {
    const declaration = alias.declarations[0];
    const file = alias.file;
    const inFile = (statementFile, spec, name) => {
      const target = resolveFile(statementFile, spec);
      if (!target) return unknown;
      return exportsOf(target).get(name) ?? unknown;
    };
    switch (declaration.kind) {
      case K.ImportSpecifier: return inFile(file, declaration.parent.parent.parent.moduleSpecifier.text, (declaration.propertyName ?? declaration.name).text);
      case K.ImportClause: return inFile(file, declaration.parent.moduleSpecifier.text, 'default');
      case K.NamespaceImport: { const target = resolveFile(file, declaration.parent.parent.moduleSpecifier.text); return target ? moduleSymbol(target) ?? unknown : unknown; }
      case K.NamespaceExport: { const target = resolveFile(file, declaration.parent.moduleSpecifier.text); return target ? moduleSymbol(target) ?? unknown : unknown; }
      case K.ExportSpecifier: {
        const statement = declaration.parent.parent;
        const name = (declaration.propertyName ?? declaration.name).text;
        if (statement.moduleSpecifier) return inFile(file, statement.moduleSpecifier.text, name);
        return file.symbols.get(name) ?? unknown;
      }
      case K.ExportAssignment: return file.symbols.get(declaration.expression) ?? unknown;
      default: return unknown;
    }
  };
  const checker = {
    getSymbolAtLocation(node) {
      if (node.kind === K.SourceFile) return moduleSymbol(node);
      if (node.kind !== K.Identifier) return undefined;
      const file = node.getSourceFile();
      return file.symbols.get(node.text);
    },
    getExportsOfModule(symbol) {
      const file = symbol.declarations[0];
      return [...exportsOf(file).values()];
    },
    getAliasedSymbol: aliased,
  };
  const isKind = (kind) => (node) => node?.kind === kind;
  const ts = {
    SyntaxKind: K,
    SymbolFlags,
    NodeFlags,
    ScriptTarget: { ESNext: 99 },
    ModuleKind: { ESNext: 99 },
    ModuleResolutionKind: { Bundler: 100 },
    createProgram(roots) {
      for (const root of roots) loadFile(root);
      return { getTypeChecker: () => checker, getSourceFile: (name) => loadFile(name) };
    },
    forEachChild(node, callback) {
      for (const child of node._kids ?? []) { const result = callback(child); if (result) return result; }
      return undefined;
    },
    getModifiers: (node) => node.modifiers,
    canHaveModifiers: (node) => Array.isArray(node.modifiers),
    isSourceFile: isKind(K.SourceFile),
    isIdentifier: isKind(K.Identifier),
    isStringLiteral: isKind(K.StringLiteral),
    isImportDeclaration: isKind(K.ImportDeclaration),
    isExportDeclaration: isKind(K.ExportDeclaration),
    isExportAssignment: isKind(K.ExportAssignment),
    isImportEqualsDeclaration: isKind(K.ImportEqualsDeclaration),
    isFunctionDeclaration: isKind(K.FunctionDeclaration),
    isClassDeclaration: isKind(K.ClassDeclaration),
    isInterfaceDeclaration: isKind(K.InterfaceDeclaration),
    isTypeAliasDeclaration: isKind(K.TypeAliasDeclaration),
    isEnumDeclaration: isKind(K.EnumDeclaration),
    isModuleDeclaration: isKind(K.ModuleDeclaration),
    isVariableStatement: isKind(K.VariableStatement),
    isImportSpecifier: isKind(K.ImportSpecifier),
    isImportClause: isKind(K.ImportClause),
    isNamespaceImport: isKind(K.NamespaceImport),
    isExportSpecifier: isKind(K.ExportSpecifier),
    isQualifiedName: isKind(K.QualifiedName),
    isImportTypeNode: isKind(K.ImportType),
    isLiteralTypeNode: isKind(K.LiteralType),
    isPropertyAccessExpression: () => false,
    isPropertySignature: () => false,
    isMethodSignature: () => false,
    isPropertyDeclaration: () => false,
    isMethodDeclaration: () => false,
    isParameter: () => false,
    isEnumMember: () => false,
  };
  return ts;
}

// ---- writing declarations for entry points (--dts) --------------------------------------------------

// One declaration file for each source file the entries reach, as `tsc --declaration --emitDeclarationOnly` writes them, laid out
// from the folder the sources have in common. Files in node_modules are not described.
function emit({ entries, outDir, tsConfigPath, bundle = false }) {
  const ts = loadTypeScript();
  let native = null;
  if (!ts) {
    native = nativeTypeScript();
    if (!native) return { error: typescriptProblem };
    if (!bundle) return emitNative(native, { entries, outDir, tsConfigPath });
  }
  const out = path.resolve(cwd, outDir);
  // To bundle, the declarations of every file are written to a folder of ours first, and one file for each entry goes to `out`. With
  // TypeScript 7 its command line writes them, and the merge reads them with the reader of declaration files above.
  const target = bundle ? fs.mkdtempSync(path.join(os.tmpdir(), 'rathkit-dts-bundle-')) : out;
  let written = [];
  let errors = 0;
  let common;
  if (native) {
    const done = emitNative(native, { entries, outDir: target, tsConfigPath });
    if (done.error) {
      fs.rmSync(target, { recursive: true, force: true });
      return done;
    }
    errors = done.errors;
    common = done.common;
  } else {
    const options = compilerOptions(ts, tsConfigPath);
    const program = ts.createProgram(entries, { ...options, declaration: true, emitDeclarationOnly: true, noEmit: false, outDir: target, declarationDir: undefined, declarationMap: false, sourceMap: false, composite: false, incremental: false, noEmitOnError: false, skipLibCheck: true, allowJs: true });
    const result = program.emit(undefined, (file, text) => {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, text);
      written.push(path.relative(target, file).split(path.sep).join('/'));
    });
    const problems = ts.getPreEmitDiagnostics(program).filter((d) => d.category === ts.DiagnosticCategory.Error);
    errors = problems.length;
    for (const d of problems.slice(0, 20)) {
      const where = d.file ? `${path.relative(cwd, d.file.fileName)}:${d.file.getLineAndCharacterOfPosition(d.start).line + 1}: ` : '';
      note(`${where}${ts.flattenDiagnosticMessageText(d.messageText, '\n').split('\n')[0]}`);
    }
    if (result.emitSkipped && !written.length) {
      if (bundle) fs.rmSync(target, { recursive: true, force: true });
      return { error: 'no declarations were written' };
    }
    if (bundle) {
      // Where the files were put follows the folder the sources have in common.
      const sources = program.getSourceFiles().filter((f) => !f.isDeclarationFile && !program.isSourceFileFromExternalLibrary(f)).map((f) => path.dirname(f.fileName));
      common = sources.reduce((a, b) => {
        let dir = a;
        while (!(b + path.sep).startsWith(dir + path.sep) && dir !== path.dirname(dir)) dir = path.dirname(dir);
        return dir;
      }, sources[0] ?? cwd);
    }
  }
  if (bundle) {
    try {
      const names = new Set();
      const bundled = [];
      const reader = ts ?? declarationReader();
      fs.mkdirSync(out, { recursive: true });
      for (const entry of entries) {
        const base = path.join(target, path.relative(common, entry).replace(/\.(tsx?|jsx?|mjs|cjs|mts|cts)$/, ''));
        const emitted = ['.d.ts', '.d.mts', '.d.cts'].map((e) => base + e).find((f) => fs.existsSync(f));
        const name = path.basename(emitted ?? base + '.d.ts');
        if (!emitted) {
          note(`no declarations were written for ${path.relative(cwd, entry)}`);
          continue;
        }
        if (names.has(name)) return { error: `two entries would both be written as ${name}; give them different names` };
        names.add(name);
        fs.writeFileSync(path.join(out, name), bundleDeclarations(reader, emitted));
        bundled.push(name);
      }
      written = bundled;
    } catch (error) {
      return { error: `the declarations could not be bundled: ${error.message}` };
    } finally {
      fs.rmSync(target, { recursive: true, force: true });
    }
  }
  written.sort();
  return { written, errors };
}

async function generate({ name, exposes, outDir, tsConfigPath, typesFolder = '@mf-types', compiledTypesFolder = 'compiled-types', additionalFiles = [], generateApi = true, compilerInstance = 'tsc', extractThirdParty = false, extractRemoteTypes = false, hostTypesFolder = '@mf-types', afterGenerate = '' }) {
  const ts = loadTypeScript();
  // TypeScript 7 has no JavaScript API; its command line writes the same declarations.
  const native = ts ? null : nativeTypeScript();
  if (!ts && !native) {
    note(`types were not written for the exposed modules: ${typescriptProblem}`);
    return { written: false };
  }
  const options = ts ? compilerOptions(ts, tsConfigPath) : {};
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
  if (native && (compilerInstance.trim() === 'tsc' || !compilerInstance.trim())) {
    const defaults = path.join(work, 'defaults.json');
    fs.writeFileSync(defaults, JSON.stringify({ compilerOptions: { target: 'es2020', module: 'esnext', moduleResolution: 'bundler', jsx: 'react-jsx', esModuleInterop: true, allowJs: true, skipLibCheck: true } }));
    const project = findConfigFile(tsConfigPath);
    const tsconfig = path.join(work, 'tsconfig.mf.json');
    fs.writeFileSync(tsconfig, JSON.stringify({ extends: project ? [defaults, project] : [defaults], compilerOptions: { ...program_options, noEmit: false }, files: rootFiles, include: [] }));
    const result = spawnSync(process.execPath, [native.bin, '--project', tsconfig, '--pretty', 'false'], { cwd, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
    if (result.error) throw new Error(`the TypeScript compiler could not be run: ${result.error.message}`);
    external = (`${result.stdout}${result.stderr}`.match(/error TS\d+/g) ?? []).length;
    if (!fs.existsSync(compiled)) {
      note(`no types were written: the TypeScript ${native.version} compiler wrote none${result.stdout ? ': ' + result.stdout.split('\n')[0] : ''}`);
      fs.rmSync(work, { recursive: true, force: true });
      return { written: false };
    }
  } else if (compilerInstance.trim() === 'tsc' || !compilerInstance.trim()) {
    const program = ts.createProgram(rootFiles, { ...options, ...program_options });
    const result = program.emit();
    diagnostics = ts.getPreEmitDiagnostics(program).filter((d) => d.category === ts.DiagnosticCategory.Error);
    if (result.emitSkipped && !fs.existsSync(compiled)) {
      note('no types were written: ' + diagnostics.slice(0, 3).map((d) => ts.flattenDiagnosticMessageText(d.messageText, '\n')).join('; '));
      fs.rmSync(work, { recursive: true, force: true });
      return { written: false };
    }
  } else {
    const configPath = ts ? findConfig(ts, tsConfigPath) : findConfigFile(tsConfigPath);
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
  if (!Object.keys(paths).length) return;
  let raw;
  try {
    raw = fs.readFileSync(file, 'utf8');
  } catch {
    return; // no tsconfig.json
  }
  let config;
  try {
    config = JSON.parse(raw);
  } catch {
    // Comments would be lost by writing the file again; it is left as it is.
    note(`tsconfig.json has comments, so it was not changed; add to "compilerOptions": "paths": ${JSON.stringify(paths)}`);
    return;
  }
  config.compilerOptions ||= {};
  const before = JSON.stringify(config.compilerOptions.paths || {});
  config.compilerOptions.paths = { ...(config.compilerOptions.paths || {}), ...paths };
  if (JSON.stringify(config.compilerOptions.paths) === before) return;
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
      let stamped;
      try {
        stamped = fs.readFileSync(stamp, 'utf8');
      } catch {}
      if (stamped !== sha) {
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
