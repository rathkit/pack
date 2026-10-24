#!/usr/bin/env python3
"""Differential fuzzing of the parser against V8.

Mutates real JavaScript files (inserting, deleting, swapping and replacing tokens), asks
both V8 (through Node) and `rathkit-pack` whether each mutant parses, and reports where
they disagree:

  * "false reject": we refuse code V8 accepts. These are parser bugs; the files are kept.
  * "lenient": we accept code V8 refuses. Grouped by V8's error message, with the known,
    deliberately deferred categories counted separately.

    cargo build --release
    python3 tools/fuzz-diff.py CORPUS.txt --seeds 1 2 3

CORPUS.txt lists one JavaScript file per line (for example the output of `find`). Only
files of 150 to 5000 bytes that V8 accepts as a module are used as mutation sources.
Requires Node with --experimental-vm-modules support (any recent version).
"""
import argparse
import collections
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(HERE, "..", "target", "release", "rkp")

TOKENS = [
    "(", ")", "{", "}", "[", "]", ";", ",", "=>", "...", "?.", "??", "#", "@", "`", "'", '"', "/",
    "async", "await", "yield", "let", "static", "class", "function", "=", "**", "in", "of", "get",
    "set", "new", "import", "export", "default", "*", "<", ">", "!", "?", ":", ".", "0", "1n", "0x",
    "super", "this", "return", "break", "continue", "var", "const", "if", "else", "for", "while",
    "do", "try", "catch", "finally", "throw", "delete", "typeof", "void", "with", "enum", "null",
    "true", "/*", "*/", "//", "\n", "%", "|", "&", "^", "~", "++", "--", "+=", "&&=", "??=", "**=",
    "extends", "from", "as", "private", "#x", "\\", "\\u0061", "constructor", "prototype",
    "__proto__", "arguments", "eval",
]

# Early errors we have not implemented yet. Counted, not listed.
DEFERRED = [
    (re.compile(r"^Invalid regular expression"), "regular expression syntax (regex validator)"),
    (re.compile(r"^HTML comments are not allowed in modules"), "HTML comment in module"),
]


def mutate(src, rng):
    pos = rng.randrange(len(src) + 1)
    roll = rng.random()
    if roll < 0.35:
        return src[:pos] + rng.choice(TOKENS) + src[pos:]
    if roll < 0.65:
        return src[:pos] + src[pos + rng.randint(1, 3):]
    if roll < 0.8 and pos + 1 < len(src):
        return src[:pos] + src[pos + 1] + src[pos] + src[pos + 2:]
    return src[:pos] + rng.choice(TOKENS) + src[pos + rng.randint(1, 4):]


SCRIPT_MODE = False


def v8_verdicts(list_file):
    env = {**os.environ, "MODE": "script"} if SCRIPT_MODE else os.environ
    out = subprocess.run(
        ["node", "--experimental-vm-modules", "--no-warnings", os.path.join(HERE, "diff-v8.mjs"), list_file],
        capture_output=True, text=True, check=True, env=env,
    ).stdout
    return dict(line.split("\t") for line in out.split("\n") if "\t" in line)


def our_verdicts(files):
    verdicts = {}
    for i in range(0, len(files), 4000):
        flag = "--script-verdicts" if SCRIPT_MODE else "--parse-verdicts"
        out = subprocess.run([BIN, flag, *files[i:i + 4000]], capture_output=True, text=True, check=True).stdout
        verdicts.update(line.split("\t") for line in out.split("\n") if "\t" in line)
    return verdicts


def v8_messages(paths, workdir):
    script = os.path.join(workdir, "messages.mjs")
    with open(script, "w") as f:
        f.write(
            'import {readFileSync} from "node:fs"; import vm from "node:vm";\n'
            'for (const p of JSON.parse(readFileSync(process.argv[2], "utf8"))) {\n'
            '  let m = "ok"; try { new vm.' + ("Script" if SCRIPT_MODE else "SourceTextModule") + '(readFileSync(p, "utf8")); } catch (e) { m = e.message; }\n'
            '  console.log(p + "\\t" + m);\n}\n'
        )
    listing = os.path.join(workdir, "messages.json")
    with open(listing, "w") as f:
        json.dump(paths, f)
    out = subprocess.run(["node", "--experimental-vm-modules", "--no-warnings", script, listing], capture_output=True, text=True, check=True).stdout
    return dict(line.split("\t", 1) for line in out.split("\n") if "\t" in line)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus")
    ap.add_argument("--seeds", type=int, nargs="+", default=[1])
    ap.add_argument("--files", type=int, default=2500, help="source files per seed")
    ap.add_argument("--per-file", type=int, default=8, help="mutants per source file")
    ap.add_argument("--script", action="store_true", help="compare as sloppy scripts instead of modules")
    ap.add_argument("--keep", default="fuzz-false-rejects", help="directory for false-reject reproductions")
    args = ap.parse_args()
    global SCRIPT_MODE
    SCRIPT_MODE = args.script
    if not os.path.exists(BIN):
        sys.exit("build first: cargo build --release")

    corpus = [l.strip() for l in open(args.corpus) if l.strip()]
    totals = collections.Counter()
    lenient_messages = collections.Counter()
    deferred = collections.Counter()
    kept = 0

    for seed in args.seeds:
        rng = random.Random(seed)
        with tempfile.TemporaryDirectory() as work:
            pool = corpus[:]
            rng.shuffle(pool)
            sources = []
            sizes_ok = [p for p in pool if "\t" not in p and 150 <= (os.path.getsize(p) if os.path.exists(p) else 0) <= 5000]
            base_list = os.path.join(work, "base.txt")
            with open(base_list, "w") as f:
                f.write("\n".join(sizes_ok[: args.files * 2]))
            base = v8_verdicts(base_list)
            wanted = "script" if SCRIPT_MODE else "module"
            sources = [p for p, v in base.items() if v == wanted][: args.files]

            mutants = []
            for i, path in enumerate(sources):
                try:
                    text = open(path, encoding="utf8").read()
                except (OSError, UnicodeDecodeError):
                    continue
                for k in range(args.per_file):
                    out = os.path.join(work, f"m{i}_{k}.js")
                    with open(out, "w", encoding="utf8", newline="") as f:
                        f.write(mutate(text, rng))
                    mutants.append(out)

            listing = os.path.join(work, "mutants.txt")
            with open(listing, "w") as f:
                f.write("\n".join(mutants))
            theirs, ours = v8_verdicts(listing), our_verdicts(mutants)
            pairs = collections.Counter()
            lenient = []
            for path in mutants:
                if path not in theirs or path not in ours:
                    continue
                pairs[(theirs[path], ours[path])] += 1
                if ours[path] == "fail" and theirs[path] != "fail":
                    os.makedirs(args.keep, exist_ok=True)
                    shutil.copy(path, os.path.join(args.keep, f"seed{seed}-{os.path.basename(path)}"))
                    kept += 1
                elif theirs[path] == "fail" and ours[path] != "fail":
                    lenient.append(path)
            totals.update(pairs)
            for path, message in v8_messages(lenient, work).items():
                for pattern, label in DEFERRED:
                    if pattern.match(message):
                        deferred[label] += 1
                        break
                else:
                    lenient_messages[re.sub(r"'[^']*'", "'X'", message)] += 1
            print(f"seed {seed}: {sum(pairs.values())} mutants, {len(lenient)} lenient", file=sys.stderr)

    print("\nverdicts (V8, ours):")
    for key, n in sorted(totals.items()):
        print(f"  {key}: {n}")
    print(f"\nFALSE REJECTS (we refuse what V8 accepts): {kept}" + (f"  -> reproductions in {args.keep}/" if kept else ""))
    print(f"\nlenient, deferred by design: {sum(deferred.values())}")
    for label, n in deferred.most_common():
        print(f"  {n:4}  {label}")
    print(f"\nlenient, NOT yet covered: {sum(lenient_messages.values())}")
    for message, n in lenient_messages.most_common():
        print(f"  {n:4}  {message}")


if __name__ == "__main__":
    main()
