# Memory

A build's peak resident size is bounded, and `tools/run-worstcase.mjs` fails when it is not.

| Input | Bound (peak resident size) |
| --- | --- |
| A graph of N modules of about 150 bytes each | 64 MB + 16 KB per module (846 MB for 50,000) |
| One module of S bytes | 64 MB + 48 x S (1,398 MB for 16 MB of source) |

Measured with a release build on Linux, plain, minified and minified with a source map:

| Case | Modules | Peak |
| --- | --- | --- |
| wide, chain, ring, star graphs | 50,000 | 355 to 479 MB |
| one 16 MB module | 1 | 421 MB plain, 1.2 GB minified with a map |
| a barrel of 5,000 re-exports | 5,002 | 35 MB |

What keeps it down: once the chunks' text is made, the per-module scans (edits, namespace lists, key references) are dropped before
the minifier runs, unless the build is kept for a rebuild (watch, serve); what a module uses to name things is held only until the
names are made in a one-shot build; and a module with many requests finds repeats through an index instead of a scan.
The main thread runs on a 1 GB stack (mapped when touched) so deeply nested generated files do not overflow it.

`RATHKIT_TIMING=1` prints the resident and peak size after each phase.

Run the check with `node tools/run-worstcase.mjs [--keep] [--modules N] [case...]`; set `TMPDIR` to put the generated graphs elsewhere.
