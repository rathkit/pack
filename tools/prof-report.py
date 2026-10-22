#!/usr/bin/env python3
"""Turns raw samples from `prof` (PROF_OUT=samples.txt) into a readable profile.

    python3 tools/prof-report.py target/prof/profiling/prof samples.txt [--top 25]

Symbolizes the program with `addr2line -i`, so code the optimizer inlined is attributed to
the source function it came from. Shared libraries (libc) are named from their dynamic
symbol table; a stripped libc has no private symbols, so internal helpers show up as
`libc.so.6:<nearest exported symbol>+`. Reports:
  self       samples where the function was executing (innermost frame, inlined included)
  inclusive  samples where the function was anywhere on the stack
"""
import argparse
import bisect
import collections
import os
import re
import subprocess


def shorten(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    for prefix in ("rathkit_pack::", "core::", "alloc::", "std::"):
        name = name.replace(prefix, "")
    return name if len(name) <= 96 else name[:93] + "..."


class Mapping:
    def __init__(self, start, end, offset, path):
        self.start, self.end, self.offset, self.path = start, end, offset, path


def read_samples(path):
    maps, stacks = [], []
    for line in open(path):
        if line.startswith("# "):
            _, rng, offset, file = line.rstrip("\n").split(" ", 3)
            start, end = (int(x, 16) for x in rng.split("-"))
            maps.append(Mapping(start, end, int(offset, 16), file))
        elif line.strip():
            stacks.append([int(x, 16) for x in line.split()])
    return maps, stacks


def load_bases(maps):
    """Load base of each file: where its offset-0 page is mapped."""
    bases = {}
    for m in maps:
        if m.offset == 0:
            bases.setdefault(m.path, m.start)
    return bases


def library_symbols(path):
    out = subprocess.run(["nm", "-D", "-S", "--defined-only", path], capture_output=True, text=True).stdout
    entries = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 4:
            entries.append((int(parts[0], 16), int(parts[1], 16), parts[3].split("@")[0]))
    entries.sort()
    return entries


lines = {}


def symbolize_program(binary, addresses):
    table = {}
    addresses = sorted(addresses)
    for start in range(0, len(addresses), 5000):
        chunk = addresses[start : start + 5000]
        out = subprocess.run(
            ["addr2line", "-a", "-f", "-C", "-i", "-e", binary, *[hex(a) for a in chunk]],
            capture_output=True, text=True, check=True,
        ).stdout.split("\n")
        current, i = None, 0
        while i < len(out):
            line = out[i]
            if re.fullmatch(r"0x[0-9a-f]+", line):
                current = int(line, 16)
                table[current] = []
                lines[current] = None
                i += 1
            elif current is not None and i + 1 < len(out) and line:
                if line != "??":
                    table[current].append(shorten(line))
                    if lines[current] is None:
                        lines[current] = out[i + 1].split(" (")[0].replace(os.getcwd() + "/", "")
                i += 2
            else:
                i += 1
    return table


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("samples")
    ap.add_argument("--top", type=int, default=25)
    ap.add_argument("--lines", action="store_true", help="list the hottest source lines instead")
    ap.add_argument("--libc-callers", action="store_true", help="attribute time spent inside libc to the program function that called it")
    args = ap.parse_args()

    maps, stacks = read_samples(args.samples)
    bases = load_bases(maps)
    exe = os.path.realpath(args.binary)
    libs = {}

    def locate(pc, is_return):
        probe = pc - 1 if is_return else pc
        for m in maps:
            if m.start <= probe < m.end:
                return m.path, probe - bases[m.path]
        return None, probe

    # Resolve every distinct address once.
    probes = {}
    program = set()
    for stack in stacks:
        for depth, pc in enumerate(stack):
            key = (pc, depth > 0)
            if key not in probes:
                path, rel = locate(pc, depth > 0)
                probes[key] = (path, rel)
                if path and os.path.realpath(path) == exe:
                    program.add(rel)
    table = symbolize_program(args.binary, program)

    def names_for(path, rel):
        if path is None:
            return ["??"]
        if os.path.realpath(path) == exe:
            return table.get(rel) or ["??"]
        base = os.path.basename(path)
        if path not in libs:
            libs[path] = library_symbols(path)
        entries = libs[path]
        i = bisect.bisect_right([e[0] for e in entries], rel) - 1
        if i >= 0:
            start, size, name = entries[i]
            if rel < start + max(size, 1):
                return [f"{base}:{name}"]
            return [f"{base}:{name}+ (internal)"]
        return [f"{base}:??"]

    cache = {key: names_for(*loc) for key, loc in probes.items()}
    self_count, incl_count = collections.Counter(), collections.Counter()
    for stack in stacks:
        frames = [cache[(pc, depth > 0)] for depth, pc in enumerate(stack)]
        self_count[frames[0][0]] += 1
        seen = set()
        for names in frames:
            for name in names:
                if name not in seen:
                    seen.add(name)
                    incl_count[name] += 1

    total = len(stacks)
    if args.libc_callers:
        callers = collections.Counter()
        in_libc = 0
        for stack in stacks:
            frames = [cache[(pc, depth > 0)] for depth, pc in enumerate(stack)]
            if not frames[0][0].startswith("libc.so"):
                continue
            in_libc += 1
            owner = next((names[0] for names in frames if not names[0].startswith("libc.so")), "(none)")
            callers[owner] += 1
        print(f"{in_libc} of {total} samples ({100 * in_libc / total:.1f}%) are inside libc; who called it:\n")
        for name, n in callers.most_common(args.top):
            print(f"{n:7} {100 * n / total:5.1f}%   {name}")
        return
    if args.lines:
        by_line = collections.Counter()
        for stack in stacks:
            path, rel = probes[(stack[0], False)]
            if path and os.path.realpath(path) == exe and lines.get(rel):
                by_line[lines[rel]] += 1
            else:
                by_line["(libc / unknown)"] += 1
        print(f"{total} samples, hottest source lines (innermost, so inlined code is credited to its own line)\n")
        for where, n in by_line.most_common(args.top):
            print(f"{n:7} {100 * n / total:5.1f}%   {where}")
        return
    print(f"{total} samples\n")
    print(f"{'self':>7} {'%':>6}   function")
    for name, n in self_count.most_common(args.top):
        print(f"{n:7} {100 * n / total:5.1f}%   {name}")
    print(f"\n{'incl':>7} {'%':>6}   function")
    for name, n in incl_count.most_common(args.top):
        print(f"{n:7} {100 * n / total:5.1f}%   {name}")


if __name__ == "__main__":
    main()
