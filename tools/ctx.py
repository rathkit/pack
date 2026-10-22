#!/usr/bin/env python3
"""Shows the source around a parser error: tools/ctx.py FILE OFFSET [CONTEXT]"""
import sys
text = open(sys.argv[1], encoding='utf-8', errors='replace').read().encode('utf-8')
pos = int(sys.argv[2]); n = int(sys.argv[3]) if len(sys.argv) > 3 else 120
print(text[max(0, pos - n):pos].decode('utf-8', 'replace') + '⟦HERE⟧' + text[pos:pos + n].decode('utf-8', 'replace'))
