// Writes the two generated workloads: a 1,500-module JavaScript app and a 500-module TypeScript app.
// They are made, not stored, so the repository stays small and anyone can see what they contain.
import { mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), 'inputs', 'generated');
rmSync(root, { recursive: true, force: true });

// Plain JavaScript: 1,500 modules of 40 small exported functions, each importing a few later ones.
const js = join(root, 'app1500');
mkdirSync(js, { recursive: true });
const n = 1500;
for (let i = 0; i < n; i++) {
  let body = '';
  for (let k = 0; k < 40; k++) body += `export function f${i}_${k}(a) { return a * ${k} + ${i}; }\n`;
  let deps = '';
  const used = [];
  for (let j = i + 1; j < Math.min(n, i + 4); j++) {
    deps += `import { f${j}_1 } from './m${j}.js';\n`;
    used.push(`f${j}_1(${i})`);
  }
  writeFileSync(join(js, `m${i}.js`), `${deps}${body}export const v${i} = ${used.join(' + ') || '0'};\n`);
}
writeFileSync(join(js, 'main.js'), "import { v0 } from './m0.js';\nconsole.log(v0);\n");

// TypeScript: 500 modules with an enum, an interface, a generic method, a getter and parameter properties.
const ts = join(root, 'tsapp500');
mkdirSync(ts, { recursive: true });
const m = 500;
for (let i = 0; i < m; i++) {
  let deps = '';
  const used = [];
  for (let j = i + 1; j < Math.min(m, i + 3); j++) {
    deps += `import { Thing${j}, make${j}, Kind${j} } from './m${j}';\n`;
    used.push(`make${j}(${i}).size`);
  }
  writeFileSync(
    join(ts, `m${i}.ts`),
    `${deps}export enum Kind${i} { A, B = ${i}, C }
export interface Props${i} { id: number; name: string; kind: Kind${i}; tags?: readonly string[] }
export class Thing${i} implements Props${i} {
  constructor(public id: number, public name: string, public kind: Kind${i} = Kind${i}.B, readonly tags: string[] = []) {}
  describe<T extends object>(extra: T): string { return \`\${this.name}#\${this.id}:\${Kind${i}[this.kind]}:\${JSON.stringify(extra)}\`; }
  get size(): number { return this.tags.length + ${i}; }
}
export function make${i}(id: number): Thing${i} { return new Thing${i}(id, 'thing${i}', Kind${i}.C, ['x', 'y']); }
export type Maker${i} = typeof make${i};
export const total${i}: number = ${used.join(' + ') || '0'} + ${i};
`,
  );
}
writeFileSync(join(ts, 'main.ts'), "import { total0, make0 } from './m0';\nconsole.log(total0, make0(1).describe({ ok: true }));\n");
console.log(`wrote ${n} + ${m} modules under ${root}`);
