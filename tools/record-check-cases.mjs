// Records TypeScript 5.9.3's diagnostics for the small programs below into check/tests/oracle-cases.tsv, which the checker's tests read
// without Node. Run it after changing the checker or the list:
//   node tools/record-check-cases.mjs --check target/release/rkp-check --ts test-deps/node_modules/ts5
// A line is: name, text (escaped: \\ and \u{HEX} for everything outside printable ASCII), the compiler's diagnostics of the codes the
// checker implements (code:start:length, space separated), and `match` when the checker reports exactly those, `gap` when it does not
// (the test then only requires that the checker reports nothing the compiler does not).
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const opt = (n) => { const i = args.indexOf(n); return i < 0 ? undefined : args[i + 1]; };
const ts = createRequire(import.meta.url)(resolve(opt('--ts')));
if (ts.version !== '5.9.3') { console.error(`expected TypeScript 5.9.3, got ${ts.version}`); process.exit(2); }
const CHECK = resolve(opt('--check'));
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'check', 'tests', 'oracle-cases.tsv');

const cases = {
  clean_basic: 'let a: number = 1;\nconst s = "x" + `y${a}z`;\n',
  clean_division: 'let a = 4 / 2 / 1;\nlet b = (a) / 2;\n',
  clean_regex: 'let r = /a[/]b/gi;\nlet q = a.match(/x+/);\n',
  clean_template_nesting: 'let a = `x${`y${1}`}z`; let b = `${ {a:1}.a }`;\n',
  clean_numbers: 'let a = 0x1F + 0b101 + 0o17 + 1_000 + 1e3 + .5 + 5. + 10n + 0xFFn;\n',
  clean_string_escapes: 'let s = "\\n\\t\\x41\\u0041\\u{1F600}\\0\\\n";\n',
  clean_unicode_text: 'let s = "é😀"; // é\nlet t = `😀`;\n',
  clean_shebang: '#!/usr/bin/env node\nlet a = 1;\n',
  clean_spread_zero: 'function f(...rest: any[]) {} f(...[0x1]);\n',
  unterminated_string: 'let a = "abc\nlet b = 1;\n',
  unterminated_string_eof: "let a = 'abc",
  unterminated_string_crlf: 'let a = "abc\r\nlet b;\n',
  unterminated_string_after_ls: 'let a = "ab\u2028cd\nlet b;\n',
  backslash_at_eof_in_string: 'let a = "x\\',
  unterminated_template: 'let a = `abc',
  unterminated_comment: 'let a = 1; /* abc',
  unterminated_regex: 'let a = /abc\nlet b;\n',
  unterminated_regex_trailing: 'let a = /abc  \nlet b;\n',
  unterminated_regex_group: 'x = /a(b);\n',
  unterminated_regex_unbalanced: 'x = /ab);\n',
  invalid_char_hash: 'let a = #;\n',
  invalid_char_control: 'let a = 1;\u0001\n',
  invalid_char_backslash: 'let a = \\ 1;\n',
  invalid_char_bom_mid: 'let a = 1;\u0000\n',
  digit_expected: 'let a = 1e;\nlet b = 1e+;\n',
  hex_expected: 'let a = 0x;\n',
  binary_expected: 'let a = 0b2;\n',
  octal_expected: 'let a = 0o9;\n',
  separator_double: 'let a = 1__0;\n',
  separator_trailing: 'let a = 1_;\n',
  separator_leading_fraction: 'let a = 5._5;\n',
  separator_after_zero: 'let a = 0_1;\n',
  separator_hex: 'let a = 0x_1;\n',
  ident_after_number: 'let a = 3in x;\nlet b = 1abc;\n',
  ident_after_bigint: 'let a = 1nOptions;\n',
  bigint_fraction: 'let a = 1.5n;\n',
  bigint_exponent: 'let a = 1e3n;\n',
  legacy_octal: 'let a = 0777;\n',
  legacy_octal_after_minus: 'let a = 1 - 0777;\n',
  leading_zero_decimal: 'let a = 08;\nlet b = 09.5;\n',
  escape_hex_short: 'let s = "\\xZ";\n',
  escape_unicode_short: 'let s = "\\u12";\n',
  escape_extended_range: 'let s = "\\u{110000}";\n',
  escape_extended_unterminated: 'let s = "\\u{12";\n',
  escape_extended_empty: 'let s = "\\u{}";\n',
  escape_octal: 'let s = "\\1" + "\\07" + "\\08" + "\\377";\n',
  escape_decimal: 'let s = "\\8" + "\\9";\n',
  escape_in_template_untagged: 'let s = `\\1`;\n',
  escape_in_template_tagged: 'let s = tag`\\1 \\xZ`;\n',
  merge_conflict: '<<<<<<< HEAD\nlet a;\n=======\nlet a2;\n>>>>>>> x\n',
  non_ascii_identifier: 'let é = 1; let a = 08;\n',
  after_regex_decision: 'let a = (1) / 2 / 3;\nlet b = "x\nlet c;\n',
  after_template_decision: 'let a = `${1}`;\nlet b = 0x;\n',
  two_errors: 'let a = "x\nlet b = 0b;\nlet c = 1__0;\n',
  non_bmp_before_error: 'let s = "😀"; let a = 0x;\n',
  crlf_lines_error_second: 'let a = 1;\r\nlet b = "x\r\nlet c;\r\n',
  dts_like: 'declare const a: 1_;\n',
  regex_flags_unknown: 'let r = /a/zz;\n',
  hash_bang_midfile: 'let a;\n#!x\n',
};
const enc = (t) => [...t].map((c) => (c === '\\' ? '\\\\' : c.codePointAt(0) >= 0x20 && c.codePointAt(0) < 0x7f ? c : `\\u{${c.codePointAt(0).toString(16)}}`)).join('');
const codes = new Set(spawnSync(CHECK, ['--codes'], { encoding: 'utf8' }).stdout.split('\n').map((l) => /^TS(\d+):/.exec(l)?.[1]).filter(Boolean).map(Number));
const lines = [];
for (const [name, text] of Object.entries(cases)) {
  const sf = ts.createSourceFile('a.ts', text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const expected = sf.parseDiagnostics.filter((d) => codes.has(d.code)).map((d) => `${d.code}:${d.start}:${d.length}`);
  const tmp = join(dirname(out), `.rec-${process.pid}.ts`);
  writeFileSync(tmp, text);
  const ours = JSON.parse(spawnSync(CHECK, ['--json', tmp], { encoding: 'utf8' }).stdout);
  const got = ours.diagnostics.map((d) => `${d.code}:${d.start}:${d.length}`);
  const kind = JSON.stringify(got) === JSON.stringify(expected) ? 'match' : 'gap';
  if (got.some((g) => !expected.includes(g))) { console.error(`FALSE POSITIVE in ${name}: ours ${got} compiler ${expected}`); process.exitCode = 1; }
  lines.push([name, enc(text), expected.join(' '), kind].join('\t'));
}
import('node:fs').then(({ rmSync, readdirSync }) => { for (const f of readdirSync(dirname(out))) if (f.startsWith('.rec-')) rmSync(join(dirname(out), f)); });
writeFileSync(out, `# Generated by tools/record-check-cases.mjs from TypeScript 5.9.3; do not edit by hand.\n# name, text, the compiler's diagnostics of implemented codes (code:start:length), match|gap\n${lines.join('\n')}\n`);
console.log(`recorded ${lines.length} cases (${lines.filter((l) => l.endsWith('match')).length} match, ${lines.filter((l) => l.endsWith('gap')).length} gap)${process.exitCode ? ' with FALSE POSITIVES' : ''}`);
