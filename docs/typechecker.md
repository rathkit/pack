# The optional type checker (roadmap 2.12, issue #78)

Status: **stage 1 of 4 exists, and it is not a type checker.** It reports a handful of scanner errors with TypeScript's codes and positions.
A run that prints no diagnostics says only that this stage found none of those; it says nothing about the rest of the syntax, names,
declarations or types. The issue stays open; this page is where the staged progress is written down.

`rkp` itself contains no checking code and never will. The checker is a separate program in a separate crate of this workspace
(`check/`), with no dependencies, found and run by `rkp check`.

## Decisions

These were the three open questions of the issue.

**Command, and discovery.** `rkp check [ARGS...]` finds an executable named `rkp-check` and runs it with the same arguments and
standard streams, and returns its exit code, the way `cargo` finds `cargo-*` subcommands. The lookup order is: the `node_modules/.bin`
of the current directory and each parent (so a project pins its own checker version with its package manager), the directory of the
`rkp` executable, then `PATH`. When it finds none, `rkp check` prints how to install it and exits 2 (the code is in `src/checker.rs`, about
60 lines; the checker's code is not there). A flag on `bundle` that runs the same executable during a build is the planned second
entry point; it comes with stage 3, when there is something worth running next to a build, and it will hand the executable the
TypeScript files of the module graph. Nothing about it exists yet.

**TypeScript version tracked: 5.9.3.** Reasons: it is the version the issue and `docs/benchmarks.md` compare against, `test-deps` already
pins it, and it is the only recent version that can be driven as a library for per-file diagnostics (`ts.createSourceFile(...).parseDiagnostics`),
which is what the measurement below uses as its oracle. TypeScript 7.0.2 ships a native command line and its Node API is marked unstable, so
it cannot be used that way; as a spot check, its `tsc` printed the same codes and line and column positions as 5.9.3 on six scanner errors
(this is a spot check, not a measurement). How a project pins the version: one checker release series per TypeScript version, and
`rkp-check --version` says which one it follows (`rkp-check 0.0.1 (stage 1: scanner diagnostics; follows TypeScript 5.9.3)`). The project pins
the checker through its package manager like any other tool. There is no `--typescript` switch and no `tsconfig.json` handling yet; neither
is needed for the scanner.

**Staging.** Each stage says what it does not report, and its output says so too.

| Stage | Reports | Does not report |
| --- | --- | --- |
| 1 (here) | Scanner errors: unterminated strings, templates, comments and regular expressions, invalid characters, malformed numeric literals and escapes (the codes below), in `.ts`, `.mts` and `.cts` | Everything the parser reports (missing tokens, unexpected tokens, malformed declarations, 1005, 1109, 1128 and the rest), regular expression syntax and flag errors, JSX, JavaScript files, every binder and checker error: duplicate or undeclared names, types of any kind |
| 1b | The parser's syntax errors (a faithful TypeScript parser with its error recovery) | Binder and checker errors |
| 2 | Binder and declaration errors: duplicate identifiers, invalid modifiers and declaration shapes, unresolved imports | Types |
| 3 | Name resolution and simple types: assignability of primitives and literals, calls, property access on known shapes | Generics inference, conditional and mapped types, control-flow narrowing beyond the simple cases, declaration merging edge cases |
| 4 | The full checker | Whatever the measurement says it still misses, listed with counts |

The stage-1 row of the issue ("syntax and declaration errors first") is split in two here on purpose: the scanner is what exists. The parser's
syntax errors and the binder's declaration errors are not in this increment, and nothing in this increment reports a declaration error.
Stage 1b needs the parser, and is also what lets the checker follow TypeScript's parser-driven scanner decisions instead of guessing (see
"verified" below).

## What stage 1 reports

Run `rkp-check --codes` for the list; it is the set of `CODES` in `check/src/scanner.rs`:

| Code | Message (TypeScript 5.9) |
| --- | --- |
| 1002 | Unterminated string literal. |
| 1010 | '*/' expected. |
| 1121 | Octal literals are not allowed. Use the syntax '0o...'. |
| 1124 | Digit expected. |
| 1125 | Hexadecimal digit expected. |
| 1126 | Unexpected end of text. |
| 1127 | Invalid character. |
| 1160 | Unterminated template literal. |
| 1161 | Unterminated regular expression literal. |
| 1177 | Binary digit expected. |
| 1178 | Octal digit expected. |
| 1198 | An extended Unicode escape value must be between 0x0 and 0x10FFFF inclusive. |
| 1199 | Unterminated Unicode escape sequence. |
| 1351 | An identifier or keyword cannot immediately follow a numeric literal. |
| 1352 | A bigint literal cannot use exponential notation. |
| 1353 | A bigint literal must be an integer. |
| 1487 | Octal escape sequences are not allowed. Use the syntax '\x..'. |
| 1488 | Escape sequence '\8' is not allowed. |
| 1489 | Decimals with leading zeros are not allowed. |
| 6188 | Numeric separators are not allowed here. |
| 6189 | Multiple consecutive numeric separators are not permitted. |

Positions are TypeScript's: start and length in UTF-16 code units, printed as `file(line,column): error TS1125: message` like `tsc`.

The output of a run always ends with what the run did not do (a test pins the sentence), for example:

```
rkp-check stage 1: scanned 1 file(s), 0 diagnostic(s), 0 withheld as unverified (see --unverified), 0 file(s) stopped early, 0 not checked.
This is stage 1 of the checker: it reports only the scanner errors listed by `rkp-check --codes`. It did not check syntax beyond that, names, declarations or types, so no diagnostics does not mean the code is correct.
```

Files it does not scan (`.tsx`, `.jsx`, `.js`) are listed as "not checked", not passed over.

### Why some diagnostics are "unverified", and why a scan stops

TypeScript's scanner is steered by its parser: whether a `/` is a division or the start of a regular expression, and whether a `}` ends a
template substitution, are decided by where the parser is. Stage 1 has no parser, so it guesses from the previous token. For code that is
valid up to that point the guess is right (the measurement below has no false positive on valid files), but after a syntax error that stage 1
does not see, TypeScript's parser can be somewhere else, and then a guess produces a diagnostic TypeScript does not.

- A diagnostic reported before the first such decision in a file is **verified**: every token before it was found without guessing.
- A diagnostic reported after one is **unverified**. `rkp-check` prints verified diagnostics only; `--unverified` prints the others, marked,
  and the summary line says how many were withheld. The JSON form (`--json`, one object per file, used by the measurement) carries all of them
  with a `verified` flag.
- When even the guess is not possible (a `/` after `)`, `}`, `++`, `--`, `>` or `of` where a regular expression that ends on the line is also
  possible; a non-ASCII character in code, because the Unicode identifier tables are not carried; a merge conflict marker; an octal escape in
  a template, which is an error only when the template is untagged), the scan **stops** there, says where and why, and reports nothing after it.
  A stop is printed as a note; it is never silent.

## Measurement

Method, `tools/diff-check.mjs`: the oracle is `ts.createSourceFile(name, text, Latest, true, ScriptKind.TS).parseDiagnostics` of TypeScript
5.9.3, the syntactic diagnostics, which include the scanner's. Seed files: the repository's own TypeScript files (`tests/`) and the 103
`lib.*.d.ts` files of the TypeScript 5.9.3 package, 170 files. Each seed is checked as it is (the false-positive gate: the checker may report
nothing the compiler does not) and then in 60 mutated copies (truncated, characters removed, a character or an edge-case snippet such as `0x`,
`\u{`, `1__0`, `3in`, `/*` inserted), seeded and reproducible, where the compiler reports something. A report matches when code, start and
length are all equal.

Result of `node tools/diff-check.mjs --check target/release/rkp-check --ts <typescript 5.9.3> --seeds tests --seeds <typescript 5.9.3>/lib
--mutants 60 --rng 1` (169 seed files, 10,309 files checked, 10,140 of them mutated):

| Code | Found, verified | False, verified | Found, unverified | False, unverified | Missed |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1002 | 272 | 0 | 17 | 1 | 11 |
| 1010 | 1175 | 0 | 17 | 0 | 0 |
| 1124 | 20 | 0 | 2 | 0 | 0 |
| 1125 | 23 | 0 | 3 | 0 | 0 |
| 1127 | 155 | 0 | 8 | 0 | 0 |
| 1160 | 57 | 0 | 12 | 28 | 2 |
| 1161 | 0 | 0 | 209 | 49 | 8 |
| 1177, 1178 | 39 | 0 | 1 | 0 | 0 |
| 1198, 1199 | 3 | 0 | 1 | 0 | 0 |
| 1351 to 1353 | 160 | 0 | 9 | 0 | 0 |
| 1487, 1489 | 14 | 0 | 1 | 0 | 0 |
| 6188, 6189 | 40 | 0 | 4 | 0 | 0 |
| **Total** | **1958** | **0** | **284** | **78** | **21** |

- **False positives, verified (printed by default): 0**, and 0 on the 169 unmutated seed files (all of them valid TypeScript). **Unverified: 78**
  (28 of them 1160 and 49 of them 1161, after a syntax error the checker cannot see). These are counted separately from missed errors, as the
  issue asks, and are withheld by default.
- **Missed: 21** reports of implemented codes that were not reported and not after a stop (mostly 1002 and 1161 after a parser decision that
  went the other way). Another 454 compiler reports of implemented codes lie after a point where the scan stopped (380 of them 1127: the
  mutations insert `é` and an emoji, and stage 1 stops at non-ASCII characters in code rather than carry the Unicode identifier tables).
- Agreement: 2,242 of the 2,263 compiler reports of implemented codes outside stops were found (99.07%). Counting the reports after a stop as
  misses, 2,242 of 2,717 were found (**82.5%**), and 1,958 (**72.1%**) were found *and* verified, which are the ones printed by default.
- Of the 10,309 files, 5,246 have no diagnostic from either side; 1,624 have only diagnostics of implemented codes; 1,540 of those are matched
  exactly. The scan stopped early in 513 files (5.0%).
- What the compiler reports on the same files that stage 1 does not implement (code: count), the large part of what "syntax errors" means:
  1005: 6,919; 1109: 3,793; 1434: 2,457; 1128: 1,689; 1443: 953; 1435: 337; 1003: 308; 1146: 239; 1131: 182; 1011: 130; and 25 more codes.
  Stage 1 reports none of them: 17,510 of the 20,227 compiler diagnostics on these files (87%) are not seen at all, which is why the first
  line of this page says it is not a checker yet.

The same comparison, small and fast, runs in `cargo test -p rathkit-check` from `check/tests/oracle-cases.tsv` (59 programs recorded from
the compiler: 56 agree exactly, 3 are known gaps in which the checker reports nothing the compiler does not), and in CI on the repository's
own files with 20 mutants each, failing on any verified false positive.

What the numbers do not say:

- **TypeScript's own conformance baselines were not used, so the first done-when item of the issue is not met.** They live in the TypeScript
  repository (`tests/baselines/reference`), not in the `typescript` npm package, and the registry has no package that carries them. They were
  not fetched from GitHub for this increment. The comparison above is against the 5.9.3 compiler itself on files the compiler can be run on,
  which is the same reference, on different inputs; it has no claim about agreement on the conformance suite.
- Agreement is measured on the codes stage 1 implements only. Everything else the compiler reports on those same files is counted in the
  "compiler diagnostics this stage does not implement" line, because an "agreement rate" over only what the checker knows would read as more
  than it is.
- The mutants are programs with scanner errors injected at random, not programs people wrote. They find disagreements; they do not estimate
  how often a real project has each error.
- The verified false-positive count is zero on this corpus; that is a measurement, not a proof. The unverified false positives are the
  reason the unverified class exists.

### Known gaps (what stage 1 can miss)

- All parser, binder and checker errors (above).
- Malformed regular expressions: TypeScript 5.9 reports bad flags and bad pattern syntax (1499 to 1520); stage 1 only finds a regular
  expression with no end.
- Anything after a stop, and unverified positions that differ because the parser sat somewhere the guess did not expect.
- Octal and decimal escapes in untagged templates (1487, 1488), and escapes in a malformed `\u{...}` inside a template.
- Identifiers and characters outside ASCII in code (stage 1 stops at them), identifiers with `\u` escapes, merge conflict markers (1185).
- JSX, and JavaScript files.

## The rkp binary does not grow because of the checker

`check/` is a member of the workspace; `rkp` is the root package and the default member, so `cargo build` builds `rkp` only, and the checker
is not a dependency of anything. `tools/check-rkp-size.mjs` builds `rkp` in release mode twice, once with the workspace as it is and once with
`check/` removed from it (the member entry and the directory), and fails unless the two binaries are the same size; it prints both sizes and
hashes. CI runs it on one leg. The two builds both include the discovery code of `src/checker.rs`; its own cost is stated here:

| | Bytes |
| --- | ---: |
| `rkp` release binary, `origin/main` (7ee7a77), without the discovery code | 3,735,080 |
| `rkp` release binary, this branch, `check/` in the workspace | 3,740,376 |
| `rkp` release binary, this branch, `check/` removed from the workspace | 3,740,376 (SHA-256 identical to the line above) |

So the checker's source adds nothing to `rkp` (0 bytes, byte for byte identical), and `rkp check` itself, the discovery code and its message,
costs 5,296 bytes (0.14%) in `rkp`. That is what everyone pays, installed checker or not. (x86-64 Linux, Rust 1.99.1, `cargo build --release`;
a size is not a timing, so the loaded machine these were built on does not matter for it. Each of the two builds in the tool starts from a
clean release build of `rkp`: Cargo does not count the member list as an input, and without the clean the second build would have been the
first one's file again.)

`node tools/check-zero-deps.mjs` was extended to every workspace member (empty dependency tables, no build script, a lock file with only
the workspace's own packages and no source or dependency line); it still passes, and the release bill of materials still lists no
components. The checker's installed size is the size of its one executable:

`rkp-check` is one file with no runtime and no dependency: 408,784 bytes for the stage-1 release build (x86-64 Linux, same toolchain).
The budget at stage 1 is 1 MiB; it is a number written here, not yet a CI gate. Stages 1b to 4 will raise it, and each raise is
written here with the stage that causes it. Nothing is published to npm or crates.io yet, so there is no package install size.

## Where each done-when item of the issue stands

| Item | State |
| --- | --- |
| Same codes and positions as the tracked version, measured against TypeScript's conformance baselines, agreement rate and every gap published | **Not met.** Codes and positions are measured against the 5.9.3 compiler for the scanner codes (above); the conformance baselines were not obtainable from the registry. |
| Never reports an error the reference does not; says which errors it can miss | **Partly.** Zero verified false positives on the corpus; unverified diagnostics exist, are withheld by default and are counted. The misses are listed above. |
| Speed, peak memory and install size against `tsc` and TypeScript 7 on real projects, in pack-bench, in `docs/benchmarks.md` | **Not done.** Nothing here has been timed against them; at stage 1 there is nothing comparable to time. |
| The `rkp` binary does not grow; no new dependency; installed size stated and budgeted | **Met for what exists:** the size check is in CI, zero-deps passes, the installed size is stated. There is no size budget yet. |
| Pack column in the build-plus-type-check table | **Not done**; Pack still has no checker worth putting there. |
| Declaration output and federation types keep working; 2.11 not made harder | **Met trivially:** nothing in the build changes. |

## Reproduce

```sh
cargo build --release -p rathkit-check
# the oracle: TypeScript 5.9.3 (test-deps has it as `ts5`, or install it in a scratch directory with pnpm)
node tools/diff-check.mjs --check target/release/rkp-check --ts test-deps/node_modules/ts5 \
    --seeds tests --seeds path/to/typescript/lib --mutants 60 --rng 1
node tools/record-check-cases.mjs --check target/release/rkp-check --ts test-deps/node_modules/ts5   # rewrites check/tests/oracle-cases.tsv
cargo test --release -p rathkit-check
node tools/check-rkp-size.mjs
```
