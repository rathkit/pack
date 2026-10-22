#!/usr/bin/env python3
"""For each distinct parser error message, shows one example in context.
usage: ts-samples.py files.txt verdicts.txt [limit-per-message] [only-ext]"""
import sys, re, collections
files = open(sys.argv[1]).read().split('\n'); verdicts = open(sys.argv[2]).read().split('\n')
limit = int(sys.argv[3]) if len(sys.argv) > 3 else 1
only = sys.argv[4] if len(sys.argv) > 4 else None
seen = collections.Counter()
for f, v in zip(files, verdicts):
    if not v.startswith('ERR'): continue
    if only and not f.endswith(only): continue
    m = re.match(r'ERR (.*) @(\d+)$', v)
    msg = m.group(1); pos = int(m.group(2))
    if seen[msg] >= limit: continue
    seen[msg] += 1
    data = open(f, 'rb').read()
    print(f"=== {msg}  ({f.split('node_modules/')[-1][:90]})")
    print((data[max(0, pos - 100):pos].decode('utf-8', 'replace') + '⟦HERE⟧' + data[pos:pos + 80].decode('utf-8', 'replace')).replace('\n', '↵\n'))
