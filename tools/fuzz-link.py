#!/usr/bin/env python3
"""Fuzzes the linker against Node's: random small module graphs with re-exports, `export *`,
cycles and name clashes. Usage: tools/fuzz-link.py [--seed N] [--count N] [--acyclic]

With cycles, V8 can disagree with the specification (and with this linker) about `export *`
names: it caches a name resolved from inside a cycle before the cycle is complete. Acyclic
programs must agree exactly.

Writes programs under a temp directory, dumps the linker's view with `--link-dump`, and
compares it with tools/diff-link.mjs (Node links but never evaluates)."""
import argparse, os, random, subprocess, sys, tempfile

NAMES = ["a", "b", "c", "d"]
BIN = os.path.join(os.path.dirname(__file__), "..", "target", "release", "rkp")


def module(rng, index, count, strict, acyclic):
    lines, exported = [], set()

    def target():
        # Acyclic: a module only reaches later ones (the last one can only name itself).
        if acyclic:
            return "./m%d.js" % rng.randrange(min(index + 1, count - 1), count)
        return "./m%d.js" % rng.randrange(count)

    def export_name():
        free = [n for n in NAMES + ["default"] if n not in exported]
        if not free:
            return None
        name = rng.choice(free)
        exported.add(name)
        return name

    for _ in range(rng.randint(1, 5)):
        kind = rng.randrange(8)
        if not strict and kind in (4, 7):
            kind = 0
        if kind == 0:
            n = export_name()
            if n and n != "default":
                lines.append("export const %s = %d;" % (n, index))
            elif n:
                lines.append("export default %d;" % index)
        elif kind == 1:
            lines.append("export * from '%s';" % target())
        elif kind == 2:
            n = export_name()
            if n:
                src = rng.choice(NAMES + ["default"])
                lines.append("export { %s as %s } from '%s';" % (src, n if n != "default" else "default", target()))
        elif kind == 3:
            n = export_name()
            if n and n != "default":
                lines.append("export * as %s from '%s';" % (n, target()))
        elif kind == 4:
            n = export_name()
            if n:
                src = rng.choice(NAMES)
                lines.append("import { %s as imp%d } from '%s'; export { imp%d as %s };" % (src, len(lines), target(), len(lines), n))
        elif kind == 5:
            n = export_name()
            if n:
                lines.append("import * as ns%d from '%s'; export { ns%d as %s };" % (len(lines), target(), len(lines), n))
        elif kind == 6:
            n = export_name()
            if n and n != "default":
                lines.append("const l%d = 1; export { l%d as %s };" % (len(lines), len(lines), n))
        else:
            lines.append("import d%d from '%s'; use(d%d);" % (len(lines), target(), len(lines)))
    if index == 0:
        picks = rng.sample(NAMES, rng.randint(1, 3 if strict else 1))
        lines.append("import { %s } from './m%d.js'; use(%s);" % (", ".join(picks), rng.randrange(count), ", ".join(picks)))
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--count", type=int, default=2000)
    ap.add_argument("--acyclic", action="store_true", help="imports only point forward; Node and the specification must agree exactly")
    args = ap.parse_args()
    rng = random.Random(args.seed)
    root = tempfile.mkdtemp(prefix="fuzzlink")
    entries = []
    for p in range(args.count):
        d = os.path.join(root, "p%d" % p)
        os.makedirs(d)
        open(os.path.join(d, "package.json"), "w").write('{"type": "module"}')
        n = rng.randint(2, 4)
        # Most programs avoid the forms that fail often, so the accepting path gets exercised.
        strict = rng.random() < 0.3
        for i in range(n):
            open(os.path.join(d, "m%d.js" % i), "w").write(module(rng, i, n, strict, args.acyclic))
        entries.append(os.path.join(d, "m0.js"))
    dump = os.path.join(root, "dump.txt")
    with open(dump, "w") as out:
        subprocess.run([BIN, "--link-dump", *entries], stdout=out, check=True)
    result = subprocess.run(["node", "--no-warnings", "--experimental-vm-modules", os.path.join(os.path.dirname(__file__), "diff-link.mjs"), dump],
                            capture_output=True, text=True)
    print(result.stdout[-3000:] + result.stderr[-1000:])
    print("programs kept in", root)


main()
