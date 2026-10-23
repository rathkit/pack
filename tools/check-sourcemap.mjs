#!/usr/bin/env node
// Checks a source map against the text on both ends, with no help from the bundler: at every
// mapped position the token in the generated file must be the token in the original file, or
// an identifier that the map says was renamed (and then the map's name must be the original).
//
//   node tools/check-sourcemap.mjs GENERATED.js [--lenient] [--quiet]
//
// The map is read from the file's `sourceMappingURL` comment (a file next to it, or inline).
// Tokens that a lowering rewrote (TypeScript enums, decorators, parameter properties) legitimately
// map to other text, so `--lenient` only reports the numbers; without it any mismatch fails.
import fs from "node:fs";
import path from "node:path";

const file = process.argv[2];
const lenient = process.argv.includes("--lenient");
const quiet = process.argv.includes("--quiet");
const code = fs.readFileSync(file, "utf8");
const m = /\n\/\/# sourceMappingURL=(.+)\s*$/.exec(code);
if (!m) {
  console.error("no sourceMappingURL comment");
  process.exit(2);
}
const raw = m[1].startsWith("data:") ? Buffer.from(m[1].split(",")[1], "base64").toString("utf8") : fs.readFileSync(path.resolve(path.dirname(file), m[1]), "utf8");
const map = JSON.parse(raw);

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function* decode(mappings) {
  let line = 0, col = 0, src = 0, oline = 0, ocol = 0, name = 0;
  for (const group of mappings.split(";")) {
    col = 0;
    if (group) {
      for (const seg of group.split(",")) {
        const nums = [];
        let value = 0, shift = 0;
        for (const ch of seg) {
          const d = B64.indexOf(ch);
          if (d < 0) throw new Error("bad base64 digit in mappings");
          value += (d & 31) * 2 ** shift;
          if (d & 32) shift += 5;
          else {
            nums.push(value % 2 ? -(value - 1) / 2 : value / 2);
            value = 0;
            shift = 0;
          }
        }
        if (![1, 4, 5].includes(nums.length)) throw new Error(`segment with ${nums.length} fields`);
        col += nums[0];
        if (nums.length === 1) yield { line, col };
        else {
          src += nums[1];
          oline += nums[2];
          ocol += nums[3];
          const e = { line, col, src, oline, ocol };
          if (nums.length === 5) {
            name += nums[4];
            e.name = name;
          }
          yield e;
        }
      }
    }
    line++;
  }
}

const LINE_BREAK = new RegExp("\\r\\n|[\\n\\r" + String.fromCharCode(0x2028, 0x2029) + "]");
const split = (text) => text.split(LINE_BREAK);
const genLines = split(code);
const srcLines = map.sourcesContent ? map.sourcesContent.map((t) => (t == null ? null : split(t))) : map.sources.map((s) => split(fs.readFileSync(path.resolve(path.dirname(file), s), "utf8")));

const JOINERS = String.fromCharCode(0x200c, 0x200d);
const IDENT = new RegExp("^[\\p{ID_Start}$_\\\\][\\p{ID_Continue}$" + JOINERS + "\\\\]*", "u");
function token(lines, line, col) {
  const text = lines[line];
  if (text === undefined || col > text.length) return null;
  const rest = text.slice(col);
  const id = IDENT.exec(rest);
  if (id) return { kind: "id", text: id[0] };
  const num = /^[0-9][0-9a-zA-Z_.]*/.exec(rest);
  if (num) return { kind: "num", text: num[0] };
  return { kind: "punct", text: rest[0] ?? "" };
}

// What --minify-syntax writes in place of other tokens: `!0` for `true`, `void 0` for `undefined`,
// another spelling of a number, an enum member's value, a bare name for a quoted one, `.` for `[`, `(` for `{`.
function isSyntaxRewrite(g, s) {
  if (["true", "false", "undefined"].includes(s.text)) return ["!", "void", "("].includes(g.text);
  if (g.kind === "num" && s.kind === "num") return true;
  // An enum member written as its value: `Color.Red` becomes `0`, `"S"` or `(-3)`.
  if (s.kind === "id" && (g.kind === "num" || ['"', "(", "-"].includes(g.text))) return true;
  // `.5` for `0.5`, and an operator for the punctuation it replaces (`)` becomes `?`, `}` becomes `:` or `;`).
  if (s.kind === "num" && g.text === ".") return true;
  if (g.kind === "punct" && s.kind === "punct") return true;
  if (s.kind === "punct" && ['"', "'"].includes(s.text)) return g.kind === "id";
  if (s.text === "[") return g.text === ".";
  if (s.text === "{" || s.text === "return") return g.text === "(" || g.kind === "id" || g.kind === "punct";
  return false;
}

let entries = 0, rewritten = 0, unmapped = 0, same = 0, renamed = 0, bad = 0, badName = 0;
const examples = [];
let prev = null;
let outOfOrder = 0;
for (const e of decode(map.mappings)) {
  if (prev && (e.line < prev.line || (e.line === prev.line && e.col < prev.col))) outOfOrder++;
  prev = e;
  entries++;
  if (e.src === undefined) {
    unmapped++;
    continue;
  }
  const g = token(genLines, e.line, e.col);
  const s = srcLines[e.src] && token(srcLines[e.src], e.oline, e.ocol);
  if (!g || !s) {
    bad++;
    if (examples.length < 8) examples.push(`out of range: gen ${e.line + 1}:${e.col + 1} -> ${map.sources[e.src]}:${e.oline + 1}:${e.ocol + 1}`);
    continue;
  }
  if (g.text === s.text) same++;
  else if (g.kind === "id" && s.kind === "id" && e.name !== undefined) {
    if (map.names[e.name] === s.text) renamed++;
    else {
      badName++;
      if (examples.length < 8) examples.push(`name ${JSON.stringify(map.names[e.name])} but source has ${JSON.stringify(s.text)} at ${map.sources[e.src]}:${e.oline + 1}:${e.ocol + 1}`);
    }
  } else if (g.kind === "punct" && s.kind === "punct" && g.text[0] === s.text[0]) same++;
  else if (isSyntaxRewrite(g, s)) rewritten++;
  else {
    bad++;
    if (examples.length < 8) examples.push(`gen ${e.line + 1}:${e.col + 1} ${JSON.stringify(g.text)} -> ${map.sources[e.src]}:${e.oline + 1}:${e.ocol + 1} ${JSON.stringify(s.text)}`);
  }
}
const mapped = entries - unmapped;
if (!quiet || bad || badName || outOfOrder) {
  console.log(`${file}: ${entries} segments, ${mapped} mapped (${same} same text, ${renamed} renamed, ${rewritten} rewritten), ${bad} mismatched, ${badName} wrong names, ${outOfOrder} out of order`);
  for (const x of examples) console.log("  " + x);
}
// Rewritten references (`x` to `(init(), x2)`) map to the text they replaced; they are a small
// share of a real program, and a map that was shifted would show far more.
const tooMany = lenient && bad > 40 && bad / Math.max(mapped, 1) > 0.03;
process.exit(outOfOrder || badName || tooMany || (bad && !lenient) ? 1 : 0);
