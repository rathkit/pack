#!/usr/bin/env node
// Differential fuzzing of --minify (its syntax rewrites in particular): random programs built
// from the constructs the rewrites touch (booleans, undefined, odd number spellings, quoted keys,
// bracket access, `new`, braces and the dangling else, declaration runs, arrows with one return,
// literal conditions, line breaks standing for semicolons) are run as they are and minified,
// and must print the same.
//
//   node tools/fuzz-syntax.mjs [--seed N] [--programs N] [--keep]
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bin = process.env.RATHKIT_PACK_BIN ?? join(root, "target", "release", "rkp");
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
const chance = (n) => rand(n) === 0;

const numbers = ["0", "1", "2", "10", "100", "1000", "1.0", "0.5", ".5", "5.", "0x10", "0b11", "0o7", "1e3", "1_000", "2.50", "0.00015", "120000", "1.5e3"];
const strings = ['"a"', "'b'", '"it\'s"', "'say \"hi\"'", '""'];
const keys = ["k", "m", "if", "x-y", "7", "class"];

function expr(scope, depth) {
  const leaf = () =>
    pick([
      () => pick(["true", "false", "undefined", "null", "void 0"]),
      () => pick(numbers),
      () => pick(strings),
      () => (scope.length ? pick(scope) : "1"),
      () => (scope.length ? pick(scope) : "2"),
    ])();
  if (depth <= 0) return leaf();
  const sub = () => expr(scope, depth - 1);
  return pick([
    leaf,
    () => `(${sub()} + ${sub()})`,
    () => `(${sub()} === ${sub()})`,
    () => `(${sub()} ?? ${sub()})`,
    () => `(${sub()} || ${sub()})`,
    () => `(${sub()} && ${sub()})`,
    () => `(!${sub()})`,
    () => `typeof ${sub()}`,
    () => `(${pick(["true", "false", sub()])} ? ${sub()} : ${sub()})`,
    () => `({ ${chance(2) ? '"k"' : "k"}: ${sub()}, ${pick(['"m"', "m", '"x-y"', '"if"', "7", '"7"'])}: ${sub()} })`,
    () => `({ k: ${sub()}, m: ${sub()} })[${pick(['"k"', '"m"', "'k'"])}]`,
    () => `({ k: ${sub()}, "m": ${sub()} }).${pick(["k", "m"])}`,
    () => `(${sub()})?.[${pick(['"toString"', '"length"'])}]`,
    () => `new C(${sub()})${pick(["", "", ".v", ".get()"])}`,
    () => `(new C(${sub()})).v`,
    () => `String(${pick(["true", "false", "undefined"])}.toString?.())`,
    () => `(${pick(["", "async "]) === "" ? "" : ""}(x) => { return ${expr([...scope, "x"], depth - 1)}; })(${sub()})`,
    () => `((x) => { return { v: x }; })(${sub()}).v`,
    () => `((x, y) => { return x, y; })(${sub()}, ${sub()})`,
    () => `[${sub()}, ${sub()}]`,
    () => `\`t\${${sub()}}\``,
    () => `(${pick(numbers)}).toString()`,
  ])();
}

let counter = 0;
function statement(scope, depth, inLoop) {
  const name = () => `v${counter++}`;
  const sub = (d = 2) => expr(scope, d);
  const terminated = (text) => (chance(3) ? text + "\n" : text + ";");
  const choices = [
    () => {
      const n = name();
      const kind = pick(["var", "let", "const"]);
      scope.push(n);
      return terminated(`${kind} ${n} = ${sub()}`);
    },
    () => terminated(`r += String(${sub()})`),
    () => terminated(`r += String(${sub()}) + ","`),
    () => `return ${sub()};`,
    () => `return ${pick(["undefined", "void 0", sub()])};`,
  ];
  if (depth > 0) {
    const body = () => block(scope, depth - 1, inLoop);
    choices.push(
      () => `if (${pick(["true", "false", "a", "!a", "b > 1", sub(1)])}) ${body()}`,
      () => `if (${pick(["true", "false", "a", "b > 1", "(a)"])}) ${body()} else ${body()}`,
      () => `if (${pick(["true", "false", "a"])}) ${body()} else if (${pick(["b", "false", "true"])}) ${body()}`,
      () => `if (a) if (${pick(["b", "true", "false"])}) ${single(scope, depth - 1, inLoop)} else ${single(scope, depth - 1, inLoop)}`,
      () => `if (a) { if (b) ${single(scope, depth - 1, inLoop)} } else ${single(scope, depth - 1, inLoop)}`,
      () => `if (a) for (let i = 0; i < 2; i++) { if (b > i) ${single(scope, depth - 1, true)} } else ${single(scope, depth - 1, inLoop)}`,
      () => `for (let i = 0; i < 3; i++) ${body().replace(/^\{/, "{ ")}`,
      () => `{ let n = 3; while (n-- > 0) ${block(scope, depth - 1, true)} }`,
      () => `lab: for (const i of [1, 2, 3]) { if (i === 2) ${pick(["continue lab;", "break lab;"])} r += i; }`,
      () => `try { ${statement(scope, depth - 1, inLoop)} } catch (e) { r += "E"; }`,
      () => `switch (b) { case 1: ${single(scope, depth - 1, false)} break; default: r += "d"; }`,
      () => `do { r += "o"; } while (false)`,
      () => `var ${name()} = ${sub(1)}\n  var ${name()} = ${sub(1)}`,
      () => `let ${name()} = ${sub(1)};\n  let ${name()} = ${sub(1)};\n  const ${name()} = ${sub(1)}`,
    );
  }
  if (inLoop) choices.push(() => `if (${pick(["a", "b > 1"])}) ${pick(["break;", "continue;"])}`);
  return pick(choices)();
}

// A statement that can stand alone after `if (...)`, with or without braces.
function single(scope, depth, inLoop) {
  const s = statement([...scope], depth, inLoop);
  // Declarations cannot stand alone.
  if (/^(let|const)\b/.test(s)) return `{ ${s} }`;
  return chance(2) ? s : `{ ${s} }`;
}

function block(scope, depth, inLoop) {
  if (chance(3)) return single(scope, depth, inLoop);
  const inner = [...scope];
  const count = 1 + rand(3);
  const parts = [];
  for (let i = 0; i < count; i++) parts.push(statement(inner, depth, inLoop));
  // A statement ending without `;` needs the line break after it.
  return `{\n    ${parts.join("\n    ")}\n  }`;
}

function program() {
  counter = 0;
  const scope = ["a", "b"];
  const fns = [];
  const count = 2 + rand(3);
  for (let f = 0; f < count; f++) {
    const body = [];
    const inner = [...scope];
    for (let i = 0; i < 2 + rand(4); i++) body.push(statement(inner, 2, false));
    fns.push(`function f${f}(a, b) {\n  let r = "";\n  ${body.join("\n  ")}\n  return r;\n}`);
    if (chance(2)) fns.push(`const g${f} = (a, b) => { return f${f}(a, b) };`);
    if (chance(3)) fns.push(`const h${f} = (a, b) => { return [f${f}(a, b), ${expr(scope, 2)}] };`);
  }
  let out = `class C { constructor(v) { this.v = v; } get() { return this.v; } }\nconst show = (f, ...args) => { try { const r = f(...args); console.log(typeof r === "string" ? r : JSON.stringify(r)); } catch (e) { console.log("threw", e && e.constructor && e.constructor.name); } };\n`;
  out += fns.join("\n") + "\n";
  const names = fns.map((f) => /^(?:function|const) (\w+)/.exec(f)[1]);
  for (const n of names) for (const args of ["1, 0", "0, 2", "true, 3", "undefined, 1", '"x", 0']) out += `show(${n}, ${args});\n`;
  return out;
}

const dir = mkdtempSync(join(tmpdir(), "fuzz-syntax-"));
let failures = 0, invalid = 0;
const programs = arg("--programs", 300);
for (let p = 0; p < programs; p++) {
  const source = program();
  const file = join(dir, `p${p}.mjs`);
  writeFileSync(file, source);
  const expected = spawnSync("node", ["--no-warnings", file], { encoding: "utf8", timeout: 20000 });
  if (expected.status !== 0) {
    invalid++; // the generator wrote something that does not run
    continue;
  }
  for (const flags of [["--minify"], ["--minify", "--keep-names"], ["--minify-syntax"]]) {
    const out = join(dir, `p${p}.min.mjs`);
    const b = spawnSync(bin, ["bundle", file, "--outfile", out, ...flags], { encoding: "utf8" });
    let problem = null;
    if (b.status !== 0) problem = `bundler failed: ${b.stderr.trim().slice(0, 300)}`;
    else {
      const got = spawnSync("node", ["--no-warnings", out], { encoding: "utf8", timeout: 20000 });
      if (got.status !== 0 || got.stdout !== expected.stdout) problem = got.status !== 0 ? `crashed: ${got.stderr.split("\n").slice(0, 4).join(" | ")}` : "output differs";
    }
    if (problem) {
      failures++;
      console.log(`FAIL seed ${arg("--seed", 1)} program ${p} (${flags.join(" ")}): ${problem}`);
      console.log(`  kept ${file}`);
      break;
    }
  }
  if (failures > 5) break;
}
console.log(`${programs - invalid} programs, ${failures} disagreements (${invalid} generated programs did not run and were skipped)`);
if (!failures && !process.argv.includes("--keep")) rmSync(dir, { recursive: true, force: true });
process.exit(failures ? 1 : 0);
