#!/usr/bin/env python3
"""Differential test of redeclaration rules against V8.

Builds every pair of declaration forms inside every kind of scope, asks V8 and
`rathkit-pack` whether each program is valid (as a module or as a script), and reports
disagreements.

    cargo build --release
    python3 tools/scope-matrix.py
"""
import collections
import itertools
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(HERE, "..", "target", "release", "rathkit-pack")

DECLS = {
    "var": "var x;",
    "let": "let x;",
    "const": "const x = 1;",
    "class": "class x {}",
    "function": "function x() {}",
    "generator": "function* x() {}",
    "async": "async function x() {}",
    "nested-var": "{ var x; }",
    "nested-let": "{ let x; }",
    "nested-function": "{ function x() {} }",
    "for-var": "for (var x;;) break;",
    "for-let": "for (let x;;) break;",
    "for-of-var": "for (var x of []) ;",
    "if-function": "if (1) function x() {}",
    "label": "x: ;",
    "destructure-var": "var { x } = {};",
    "destructure-let": "let [x] = [];",
}

CONTEXTS = {
    "top": "@@",
    "function": "function f() { @@ }",
    "block": "{ @@ }",
    "arrow": "(a) => { @@ }",
    "switch": "switch (0) { case 0: @@ }",
    "static-block": "class A { static { @@ } }",
    "catch": "try {} catch (x) { @@ }",
    "catch-pattern": "try {} catch ({x}) { @@ }",
    "param": "function f(x) { @@ }",
    "param-default": "function f(x = 1) { @@ }",
    "for-let-body": "for (let x of []) { @@ }",
    "function-expression-name": "(function x() { @@ })",
    "method": "class A { m() { @@ } }",
    "arrow-param": "(x) => { @@ }",
    "async-arrow-param": "async (x) => { @@ }",
}


def build():
    programs = {}
    for ctx_name, ctx in CONTEXTS.items():
        before, after = ctx.split("@@")
        for (n1, d1), (n2, d2) in itertools.product(DECLS.items(), repeat=2):
            programs[f"{ctx_name}/{n1}+{n2}"] = f"{before}{d1} {d2}{after}"
        for n1, d1 in DECLS.items():
            programs[f"{ctx_name}/{n1}"] = f"{before}{d1}{after}"
    return programs


def main():
    if not os.path.exists(BIN):
        sys.exit("build first: cargo build --release")
    programs = build()
    with tempfile.TemporaryDirectory() as work:
        paths, names = [], {}
        for i, (name, src) in enumerate(programs.items()):
            path = os.path.join(work, f"p{i}.js")
            with open(path, "w") as f:
                f.write(src)
            paths.append(path)
            names[path] = name
        listing = os.path.join(work, "list.txt")
        with open(listing, "w") as f:
            f.write("\n".join(paths))
        failed = False
        # Once as "module, else script", once as sloppy script only.
        for label, flag, env in (("module-or-script", "--parse-verdicts", {}), ("script only", "--script-verdicts", {"MODE": "script"})):
            theirs = dict(
                line.split("\t")
                for line in subprocess.run(
                    ["node", "--experimental-vm-modules", "--no-warnings", os.path.join(HERE, "diff-v8.mjs"), listing],
                    capture_output=True, text=True, check=True, env={**os.environ, **env},
                ).stdout.split("\n")
                if "\t" in line
            )
            ours = dict(
                line.split("\t")
                for line in subprocess.run([BIN, flag, *paths], capture_output=True, text=True, check=True).stdout.split("\n")
                if "\t" in line
            )
            counts = collections.Counter()
            problems = []
            for path in paths:
                pair = (theirs[path], ours[path])
                counts[pair] += 1
                if pair[0] != pair[1]:
                    problems.append((names[path], open(path).read(), pair))
            print(f"{len(paths)} programs, {label}")
            for pair, n in sorted(counts.items()):
                print(f"  V8={pair[0]:6} ours={pair[1]:6}: {n}")
            for name, src, (v, o) in problems[:40]:
                kind = "TOO LENIENT" if o != "fail" else "FALSE REJECT"
                print(f"  {kind}: {src}    [{name}] V8={v} ours={o}")
            if len(problems) > 40:
                print(f"  ... and {len(problems) - 40} more")
            failed |= bool(problems)
        sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
