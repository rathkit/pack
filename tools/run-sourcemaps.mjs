#!/usr/bin/env node
// Runtime check of source maps. Generates programs in which many functions create an Error at a
// known place in a known file, bundles them in every output mode, runs the result with
// `node --enable-source-maps`, and requires every stack frame to point at the `new Error` it
// came from: same file, same line, same column.
//
//   node tools/run-sourcemaps.mjs [--seed N] [--programs N]
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, "target", "release", "rathkit-pack");
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? fallback : Number(process.argv[i + 1]);
};
let state = arg("--seed", 1) >>> 0 || 1;
const rand = (n) => {
  state ^= state << 13;
  state >>>= 0;
  state ^= state >>> 17;
  state ^= state << 5;
  state >>>= 0;
  return state % n;
};
const pick = (list) => list[rand(list.length)];

// ---- programs -----------------------------------------------------------------------------

const padding = () => pick(["", " ", "  ", "\t", "    ", " /* note */ ", " /* é😀 */ "]);
const wide = () => pick(["", "", "const w = \"é😀\u4e2d\";", "const w = '\u{1F600}\u{1F600}';", "// \u00e9\u00e9\n"]);
const types = () => pick(["", ": number", ": string | undefined", ": Array<{ a: number }>"]);

let site = 0;
const sites = []; // { id, file }
function errorSite(file, ts) {
  const id = site++;
  sites.push({ id, file });
  return { id, text: `${padding()}new Error("@${id}")` };
}

function esmModule(file, ts, count) {
  const eol = pick(["\n", "\n", "\r\n"]);
  let out = ts ? `import type { Unused } from "./types";${eol}` : "";
  const names = [];
  for (let i = 0; i < count; i++) {
    const name = `fn_${file.replace(/\W/g, "_")}_${i}`;
    names.push(name);
    const s = errorSite(file, ts);
    const a = ts ? `a${types() || ": number"}` : "a";
    const ret = ts ? pick(["", ": string", ": string | undefined"]) : "";
    const shape = rand(ts ? 8 : 6);
    if (shape === 0) out += `export function ${name}(${a}, b${ts ? ": string[]" : ""})${ret} {${eol}${padding()}${wide()}${eol}  return ${s.text}.stack;${eol}}${eol}`;
    else if (shape === 1) out += `export const ${name} = (${a})${ret} => ${s.text}.stack;${eol}`;
    else if (shape === 2) out += `export const ${name} = ${ts ? "<T,>" : ""}(${a}) => {${eol}  ${wide()} const v = "é😀" + a; return v ? ${s.text}.stack : undefined;${eol}};${eol}`;
    else if (shape === 3) out += `export class C_${name} {${eol}  ${ts ? "private x = 1;" : "x = 1;"}${eol}  run(${a})${ret} {${eol}    return \`\${this.x} \${a}\` && ${s.text}.stack;${eol}  }${eol}}${eol}export function ${name}(${a}) { return new C_${name}().run(a); }${eol}`;
    else if (shape === 4) out += `export async function ${name}(${a}) {${eol}  await 0;${eol}  return [1, 2].map((q) => ${s.text}.stack)[0];${eol}}${eol}`;
    else if (shape === 6) out += `export namespace NS_${name} {${eol}  export const k = 1;${eol}  export function f(${a}) { return ${s.text}.stack; }${eol}}${eol}export const ${name} = (${a}) => NS_${name}.f(a);${eol}`;
    else if (shape === 7) out += `function dec_${name}(v: any, c: any) { return v; }${eol}export class D_${name} {${eol}  constructor(private readonly q: number = 1) {}${eol}  @dec_${name}${eol}  run(${a}) {${eol}    return ${s.text}.stack;${eol}  }${eol}}${eol}export enum E_${name} { A, B = 4 }${eol}export function ${name}(${a}) { return new D_${name}().run(a); }${eol}`;
    else out += `export function ${name}(${a}) {${eol}${ts ? "  const t = a as unknown as number;" + eol : ""}  if (a${ts ? "!" : ""}) {${eol}    return ${s.text}.stack;${eol}  }${eol}  return undefined;${eol}}${eol}`;
  }
  return { text: out, names };
}

function commonJs(file, count) {
  let out = "";
  const names = [];
  for (let i = 0; i < count; i++) {
    const name = `cjs_${i}`;
    names.push(name);
    const s = errorSite(file, false);
    out += `exports.${name} = function (a) {${padding()}\n  ${wide()}\n  return ${s.text}.stack;\n};\n`;
  }
  return { text: out, names };
}

function program(dir) {
  const a = esmModule("a.ts", true, 4 + rand(5));
  const b = esmModule("b.js", false, 3 + rand(4));
  const c = commonJs("c.cjs", 2 + rand(3));
  const lazy = esmModule("lazy.ts", true, 2 + rand(3));
  const files = { "a.ts": a.text, "b.js": b.text, "c.cjs": c.text, "lazy.ts": lazy.text, "types.ts": "export type Unused = number;\n" };
  let main = `import * as A from "./a.ts";\nimport * as B from "./b.js";\nimport C from "./c.cjs";\nconst out = [];\nconst frame = (stack) => String(stack).split("\\n")[1] ?? "";\n`;
  for (const n of a.names) main += `out.push(frame(await Promise.resolve(A.${n}(1, []))));\n`;
  for (const n of b.names) main += `out.push(frame(await Promise.resolve(B.${n}(1, []))));\n`;
  for (const n of c.names) main += `out.push(frame(C.${n}(1)));\n`;
  main += `const L = await import("./lazy.ts");\n`;
  for (const n of lazy.names) main += `out.push(frame(await Promise.resolve(L.${n}(1, []))));\n`;
  main += `console.log(out.join("\\n"));\n`;
  files["main.ts"] = main;
  // The sites in the order main.ts reports them.
  const order = [...a.names.map((_, i) => 0), ...b.names, ...c.names, ...lazy.names];
  for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, name), text);
  return files;
}

// ---- checking -----------------------------------------------------------------------------

function expectedPosition(text, id) {
  const at = text.indexOf(`new Error("@${id}")`);
  const before = text.slice(0, at);
  const lines = before.split(/\r\n|[\n\r\u2028\u2029]/);
  return { line: lines.length, column: [...lines[lines.length - 1]].reduce((n, ch) => n + ch.length, 0) + 1 };
}

const modes = [
  [],
  ["--no-treeshake"],
  ["--minify"],
  ["--minify", "--keep-names"],
  ["--minify", "--format", "cjs"],
  ["--format", "iife"],
  ["--splitting", "--minify"],
];

const tmp = mkdtempSync(join(tmpdir(), "sourcemaps-"));
let failures = 0, checked = 0;
const programs = arg("--programs", 12);
for (let p = 0; p < programs; p++) {
  const dir = join(tmp, `p${p}`);
  mkdirSync(dir);
  site = 0;
  sites.length = 0;
  const files = program(dir);
  // Which file each site lives in, in the order main.ts prints them.
  const order = [];
  for (const [file, text] of Object.entries(files)) {
    if (file === "main.ts" || file === "types.ts") continue;
    for (let i = 0; ; i++) {
      const re = /new Error\("@(\d+)"\)/g;
      let m;
      while ((m = re.exec(text))) order.push({ id: Number(m[1]), file });
      break;
    }
  }
  const reportOrder = [];
  // main.ts prints a.ts's, b.js's, c.cjs's then lazy.ts's, each in the order it declares them.
  for (const file of ["a.ts", "b.js", "c.cjs", "lazy.ts"]) for (const s of order.filter((o) => o.file === file)) reportOrder.push(s);
  for (const mode of modes) {
    for (const kind of ["--sourcemap", "--sourcemap=inline"]) {
      const split = mode.includes("--splitting");
      const cjs = mode.includes("cjs") || mode.includes("iife");
      const outName = split ? "dist/main.js" : cjs ? "dist/main.cjs" : "dist/main.mjs";
      rmSync(join(dir, "dist"), { recursive: true, force: true });
      const args = ["bundle", "main.ts", ...(split ? ["--outdir", "dist"] : ["--outfile", outName]), ...mode, kind];
      const label = `program ${p} (${args.slice(2).join(" ")})`;
      const b = spawnSync(bin, args, { cwd: dir, encoding: "utf8" });
      if (b.status !== 0) {
        // Top-level await only exists in the esm format.
        if (/Top-level await/.test(b.stderr)) continue;
        failures++;
        console.log(`FAIL ${label}: bundler failed: ${b.stderr.trim()}`);
        continue;
      }
      const r = spawnSync("node", ["--no-warnings", "--enable-source-maps", join(dir, outName)], { cwd: dir, encoding: "utf8", timeout: 30000 });
      if (r.status !== 0) {
        failures++;
        console.log(`FAIL ${label}: run failed: ${r.stderr.split("\n").slice(0, 4).join(" | ")}`);
        continue;
      }
      const frames = r.stdout.trimEnd().split("\n");
      if (frames.length !== reportOrder.length) {
        failures++;
        console.log(`FAIL ${label}: expected ${reportOrder.length} frames, got ${frames.length}`);
        continue;
      }
      for (let i = 0; i < frames.length; i++) {
        const want = reportOrder[i];
        const pos = expectedPosition(files[want.file], want.id);
        const m = /([^/\\( ]+):(\d+):(\d+)\)?$/.exec(frames[i]);
        checked++;
        if (!m || m[1] !== want.file || Number(m[2]) !== pos.line || Number(m[3]) !== pos.column) {
          failures++;
          console.log(`FAIL ${label}: site @${want.id} should be ${want.file}:${pos.line}:${pos.column}, frame is "${frames[i].trim()}"`);
          if (failures > 12) process.exit(1);
        }
      }
    }
  }
}
// A dependency that ships its own map: frames must go on to the files it was built from.
{
  const dir = join(tmp, "prebuilt");
  mkdirSync(dir);
  const lib = esmModule("lib.ts", true, 6);
  const sitesHere = [...lib.text.matchAll(/new Error\("@(\d+)"\)/g)].map((m) => Number(m[1]));
  writeFileSync(join(dir, "lib.ts"), lib.text);
  for (const buildFlags of [[], ["--minify", "--keep-names"]]) {
    const lb = spawnSync(bin, ["bundle", "lib.ts", "--outfile", "lib.mjs", "--sourcemap=inline", ...buildFlags], { cwd: dir, encoding: "utf8" });
    if (lb.status !== 0) { failures++; console.log(`FAIL prebuilt: ${lb.stderr}`); continue; }
    let main = 'import * as L from "./lib.mjs";\nconst frame = (s) => String(s).split("\\n")[1] ?? "";\n';
    for (const n of lib.names) main += `console.log(frame(await Promise.resolve(L.${n}(1, []))));\n`;
    writeFileSync(join(dir, "main.mjs"), main);
    for (const mode of [[], ["--minify"]]) {
      const label = `prebuilt (${buildFlags.join(" ")} then ${mode.join(" ")})`;
      const b = spawnSync(bin, ["bundle", "main.mjs", "--outfile", "dist/main.mjs", "--sourcemap", ...mode], { cwd: dir, encoding: "utf8" });
      if (b.status !== 0) { failures++; console.log(`FAIL ${label}: ${b.stderr}`); continue; }
      const r = spawnSync("node", ["--no-warnings", "--enable-source-maps", "dist/main.mjs"], { cwd: dir, encoding: "utf8" });
      const frames = r.stdout.trimEnd().split("\n");
      const byOrder = [...lib.text.matchAll(/new Error\("@(\d+)"\)/g)].map((m) => Number(m[1]));
      // Sites are in declaration order, and main calls the functions in that order.
      frames.forEach((f, i) => {
        const pos = expectedPosition(lib.text, byOrder[i]);
        const m = /([^/\\( ]+):(\d+):(\d+)\)?$/.exec(f);
        checked++;
        if (!m || m[1] !== "lib.ts" || Number(m[2]) !== pos.line || Number(m[3]) !== pos.column) { failures++; console.log(`FAIL ${label}: site @${byOrder[i]} should be lib.ts:${pos.line}:${pos.column}, frame is "${f.trim()}"`); }
      });
    }
  }
}
console.log(failures ? `${failures} failure(s) in ${checked} frames` : `all ${checked} stack frames map back to their source`);
if (!failures) rmSync(tmp, { recursive: true, force: true });
else console.log(`kept ${tmp}`);
process.exit(failures ? 1 : 0);
