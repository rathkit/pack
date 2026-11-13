// Prints the names a declaration file exports, as TypeScript sees them.
//   node declared-names.cjs <typescript folder> file <file.d.ts> <cwd>
//   node declared-names.cjs <typescript folder> resolve <specifier> <folder to resolve from>
const path = require('node:path');
const [typescriptDir, mode, target, from] = process.argv.slice(2);
const ts = require(typescriptDir);
let file = target;
if (mode === 'resolve') {
  const hit = ts.resolveModuleName(target, path.join(from, 'x.ts'), { moduleResolution: ts.ModuleResolutionKind.Bundler, module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ESNext }, ts.sys).resolvedModule;
  if (!hit || !/\.d\.[mc]?ts$/.test(hit.resolvedFileName)) { console.error('no declarations'); process.exit(3); }
  file = hit.resolvedFileName;
}
const program = ts.createProgram([file], { noEmit: true, skipLibCheck: true, noResolve: false, moduleResolution: ts.ModuleResolutionKind.Bundler, module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ESNext, types: [] });
const source = program.getSourceFile(file);
const checker = program.getTypeChecker();
const symbol = checker.getSymbolAtLocation(source);
console.log(JSON.stringify(symbol ? checker.getExportsOfModule(symbol).map((s) => s.name).sort() : []));
