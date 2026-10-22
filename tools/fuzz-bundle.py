#!/usr/bin/env python3
"""Differential fuzzing of the bundler against Node.

Usage: tools/fuzz-bundle.py [--seed N] [--count N] [--jobs N] [--keep]

Generates small multi-module programs that print values, runs each with Node as written and
as bundled (with and without tree-shaking), and requires the same stdout and exit status.
The programs are built to provoke the hard parts: top-level names that collide across
modules, parameters and locals that shadow imported names, shorthand properties, class and
function exports, default exports, namespaces, re-exports, and live bindings that change.
Imports only point to later modules, so the programs have no cycles (cycle handling has
fixtures of its own).
"""
import argparse, os, random, shutil, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
BIN = os.path.join(ROOT, "target", "release", "rathkit-pack")
POOL = ["a", "b", "c", "d", "e", "f", "g", "h"]


class Module:
    def __init__(self, index):
        self.index = index
        self.lines = []
        self.declared = set()      # top-level names in scope (declarations and imports)
        self.values = []           # names known to hold numbers
        self.funcs = []            # names of functions taking one number and returning a number
        self.exports = {}          # exported name -> kind ("value" | "func" | "class" | "ns")
        self.local_of = {}         # exported name -> local expression to read it


def pick_new_name(rng, mod):
    free = [n for n in POOL if n not in mod.declared]
    if not free:
        return None
    return rng.choice(free)


def expr(rng, mod, depth=0):
    """A number-valued expression using names visible at module level."""
    r = rng.random()
    if r < 0.4 or not mod.values:
        return str(rng.randint(0, 9))
    if r < 0.7:
        return rng.choice(mod.values)
    if r < 0.85 and mod.funcs:
        return "%s(%s)" % (rng.choice(mod.funcs), expr(rng, mod, depth + 1) if depth < 2 else "1")
    return "(%s + %s)" % (expr(rng, mod, depth + 1) if depth < 2 else "2", rng.choice(mod.values))


def gen_module(rng, index, count, modules):
    mod = Module(index)
    later = modules  # already generated modules (higher indexes)
    # Imports from later modules.
    for _ in range(rng.randint(0, 3)):
        if not later:
            break
        src = rng.choice(later)
        style = rng.random()
        if style < 0.2 and src.exports:
            ns = pick_new_name(rng, mod)
            if ns is None:
                continue
            mod.declared.add(ns)
            mod.lines.append("import * as %s from './m%d.js';" % (ns, src.index))
            mod.lines.append("console.log('m%d ns', Object.keys(%s).sort().join());" % (index, ns))
            for name, kind in sorted(src.exports.items()):
                if name == "default":
                    continue
                if kind == "value":
                    mod.lines.append("console.log('m%d %s.%s', %s.%s);" % (index, ns, name, ns, name))
                elif kind == "func":
                    mod.lines.append("console.log('m%d %s.%s()', %s.%s(1));" % (index, ns, name, ns, name))
                break
            continue
        names = [n for n in src.exports if n != "default"]
        rng.shuffle(names)
        specs = []
        for name in names[: rng.randint(1, 3)]:
            alias = name if rng.random() < 0.5 and name not in mod.declared else pick_new_name(rng, mod)
            if alias is None or alias in mod.declared:
                continue
            mod.declared.add(alias)
            kind = src.exports[name]
            if kind == "value":
                mod.values.append(alias)
            elif kind == "func":
                mod.funcs.append(alias)
            specs.append(name if alias == name else "%s as %s" % (name, alias))
        if "default" in src.exports and rng.random() < 0.5:
            alias = pick_new_name(rng, mod)
            if alias is not None:
                mod.declared.add(alias)
                if src.exports["default"] == "value":
                    mod.values.append(alias)
                elif src.exports["default"] == "func":
                    mod.funcs.append(alias)
                mod.lines.append("import %s%s from './m%d.js';" % (alias, (", { %s }" % ", ".join(specs)) if specs else "", src.index))
                continue
        if specs:
            mod.lines.append("import { %s } from './m%d.js';" % (", ".join(specs), src.index))
    # Declarations.
    for _ in range(rng.randint(2, 7)):
        kind = rng.randrange(8)
        name = pick_new_name(rng, mod)
        if name is None:
            break
        export = rng.random() < 0.6
        prefix = "export " if export else ""
        if kind <= 1:  # constant
            mod.lines.append("%sconst %s = %s;" % (prefix, name, expr(rng, mod)))
            mod.values.append(name)
            if export: mod.exports[name] = "value"
        elif kind == 2:  # function with a possibly-shadowing parameter
            param = rng.choice(POOL)
            body_refs = [v for v in mod.values if v != param] or ["0"]
            mod.lines.append("%sfunction %s(%s) { const q = %s + %s; return q + %d; }" % (prefix, name, param, param, rng.choice(body_refs), rng.randint(0, 5)))
            mod.funcs.append(name)
            if export: mod.exports[name] = "func"
        elif kind == 3:  # arrow function
            param = rng.choice(POOL)
            mod.lines.append("%sconst %s = (%s) => %s * 2 + %s;" % (prefix, name, param, param, rng.choice([v for v in mod.values if v != param] or ["1"])))
            mod.funcs.append(name)
            if export: mod.exports[name] = "func"
        elif kind == 4:  # class
            ref = rng.choice(mod.values) if mod.values else "1"
            mod.lines.append("%sclass %s { static id = %s; constructor() { this.v = %s; } get() { return this.v + %s.id; } }" % (prefix, name, ref, ref, name))
            mod.lines.append("console.log('m%d class %s', new %s().get(), %s.name);" % (index, name, name, name))
            if export: mod.exports[name] = "class"
            mod.declared.add(name)
            continue
        elif kind == 5:  # object with shorthand
            names = [v for v in mod.values][:3]
            mod.lines.append("%sconst %s = { %s };" % (prefix, name, ", ".join(names + ["k: 1"])))
            mod.lines.append("console.log('m%d obj %s', JSON.stringify(%s));" % (index, name, name))
            if export: mod.exports[name] = "obj"
            mod.declared.add(name)
            continue
        elif kind == 6:  # mutable binding
            mod.lines.append("%slet %s = %s;" % (prefix, name, expr(rng, mod)))
            helper = "bump_%s" % name
            if helper in mod.declared:
                continue
            mod.declared.add(helper)
            mod.lines.append("%sfunction %s() { %s++; return %s; }" % (prefix, helper, name, name))
            mod.declared.add(name)
            mod.values.append(name)
            if export:
                mod.exports[name] = "value"
                mod.exports[helper] = "func0"
            continue
        else:  # destructuring
            n2 = pick_new_name(rng, mod)
            if n2 is None or n2 == name:
                continue
            mod.lines.append("%sconst { x: %s, y: %s = %s } = { x: %s };" % (prefix, name, n2, expr(rng, mod), expr(rng, mod)))
            mod.declared.add(n2)
            mod.values += [name, n2]
            if export:
                mod.exports[name] = "value"
                mod.exports[n2] = "value"
        mod.declared.add(name)
    # Re-exports.
    if later and rng.random() < 0.5:
        src = rng.choice(later)
        names = [n for n in src.exports if n != "default" and n not in mod.exports]
        if names:
            n = rng.choice(names)
            as_name = n if rng.random() < 0.6 else rng.choice(POOL)
            if as_name not in mod.exports and as_name != "default":
                mod.lines.append("export { %s%s } from './m%d.js';" % (n, "" if as_name == n else " as " + as_name, src.index))
                mod.exports[as_name] = src.exports[n]
    if later and rng.random() < 0.15:
        src = rng.choice(later)
        mod.lines.append("export * from './m%d.js';" % src.index)
        for n, k in src.exports.items():
            if n != "default" and n not in mod.exports:
                mod.exports[n] = k
    # Default export.
    if rng.random() < 0.3 and "default" not in mod.exports:
        form = rng.randrange(3)
        if form == 0:
            mod.lines.append("export default %s;" % expr(rng, mod))
            mod.exports["default"] = "value"
        elif form == 1:
            mod.lines.append("export default function (x) { return x + %s; }" % expr(rng, mod))
            mod.exports["default"] = "func"
        else:
            mod.lines.append("export default class { m() { return %s; } }" % expr(rng, mod))
            mod.exports["default"] = "class"
    # Observable behavior.
    for _ in range(rng.randint(1, 3)):
        mod.lines.append("console.log('m%d', %s);" % (index, expr(rng, mod)))
    for name, kind in list(mod.exports.items()):
        if kind == "func0" and rng.random() < 0.7:
            pass
    # Exported mutators are called by importers; check live bindings in the entry.
    return mod


def write_program(rng, directory):
    count = rng.randint(2, 6)
    modules = []
    for index in range(count - 1, -1, -1):
        modules.insert(0, gen_module(rng, index, count, list(modules)))
    # The entry exercises mutators so live bindings are visible.
    entry = modules[0]
    extra = []
    for m in modules[1:]:
        for name, kind in m.exports.items():
            if kind == "func0" and rng.random() < 0.6:
                alias = "z_%s_%d" % (name, m.index)
                extra.append("import { %s as %s } from './m%d.js'; console.log('call', %s());" % (name, alias, m.index, alias))
    entry.lines += extra
    os.makedirs(directory, exist_ok=True)
    with open(os.path.join(directory, "package.json"), "w") as f:
        f.write('{"type": "module"}')
    for m in modules:
        name = "main.js" if m.index == 0 else "m%d.js" % m.index
        # main.js is m0; other modules import it by index so expose it under both names.
        with open(os.path.join(directory, name), "w") as f:
            f.write("\n".join(m.lines) + "\n")
    # Modules never import m0, so no alias file is needed.


def run_node(path, cwd):
    r = subprocess.run(["node", "--no-warnings", path], cwd=cwd, capture_output=True, text=True, timeout=30)
    return r.stdout, r.returncode, r.stderr


def check(seed, base, keep):
    rng = random.Random(seed)
    directory = os.path.join(base, "p%d" % seed)
    write_program(rng, directory)
    expected = run_node("main.js", directory)
    if "SyntaxError" in expected[2]:
        shutil.rmtree(directory, ignore_errors=True)
        return seed, directory, [], 0   # the generator made an invalid program
    problems = []
    for flags in ([], ["--no-treeshake"], ["--minify", "--keep-names"], ["--minify", "--keep-names", "--sourcemap=inline"]):
        out = os.path.join(directory, "out%s.mjs" % (("_" + flags[0].strip("-")) if flags else ""))
        b = subprocess.run([BIN, "bundle", "main.js", "--outfile", out] + flags, cwd=directory, capture_output=True, text=True)
        if b.returncode != 0:
            problems.append("bundler failed %s: %s" % (flags, b.stderr.strip()[:300]))
            continue
        got = run_node(out, directory)
        if got[0] != expected[0] or got[1] != expected[1]:
            problems.append("output differs %s (exit %d vs %d)" % (flags, expected[1], got[1]))
    if not problems and not keep:
        shutil.rmtree(directory, ignore_errors=True)
    return seed, directory, problems, expected[1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--count", type=int, default=300)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()
    base = tempfile.mkdtemp(prefix="fuzzbundle")
    bad, thrown = 0, 0
    with ThreadPoolExecutor(args.jobs) as pool:
        for seed, directory, problems, code in pool.map(lambda s: check(s, base, args.keep), range(args.seed * 100000, args.seed * 100000 + args.count)):
            if code != 0:
                thrown += 1
            if problems:
                bad += 1
                if bad <= 8:
                    print("FAIL", directory)
                    for p in problems:
                        print("   ", p)
    print("%d programs, %d disagreements (%d of the originals exit non-zero)" % (args.count, bad, thrown))
    if bad == 0 and not args.keep:
        shutil.rmtree(base, ignore_errors=True)
    else:
        print("programs kept in", base)
    sys.exit(1 if bad else 0)


main()
