# One very large module

Decision for [#130](https://github.com/rathkit/pack/issues/130): Pack does not cut the parse of one module between threads. It is revisited when a second kind of workload needs it.

## What is measured

The TypeScript 5.9 compiler is one 9 MB file with about 200,000 lines. Its top level is flat (about 10,000 column-0 statements, not one big wrapper), so statement boundaries exist. On the benchmark machine the graph phase takes 110 ms and is one parse, followed by a scan of 40 ms and the minifier pieces (62 and 24 ms). The same file parsed alone, with the scope analysis, takes 160 to 235 ms on a loaded machine (`parser::parse`, five runs).

## Why not now

- Boundaries cannot be found without reading the text the way the parser does. A statement can start after a regular expression, a template string with a nested template, or a line break where no semicolon was written. A cut found by a cheaper scan that is wrong gives a different tree, so every cut needs the lexer's state (regular expression or division, template depth, ASI) and a check that the neighbor agrees. That is a second lexing pass over the whole file on one thread before any thread can start.
- The parser builds scopes while it reads, and the arena of nodes is numbered in reading order. Pieces parsed apart need their node numbers and symbol numbers shifted when joined, and a scope analysis that crosses pieces: `var` and function declarations are hoisted to the whole file, and a later statement can use a name an earlier piece declares. Joining is serial and touches every identifier.
- Everything after the parse (scan, link, the minifier pieces) sees one tree. Output bytes must stay identical to the serial parse; the proof needs the tree compared node by node for every fixture, which is a test suite of its own.
- The bound on the gain: with a perfect eight-way split and no join cost, 110 ms of the graph phase becomes about 15 ms for this one workload; the boundary scan and the join are not free, so a realistic gain is a part of that. No other benchmark workload has a module over 1 MB (three.js is a few classes), so the work serves one input.

## What would change this

A second workload with one module that is more than a few megabytes (generated code, a bundled library shipped as one file), or a parser that already keeps its scope analysis apart from the tree. The cheapest first step then is a parse that only finds the top-level statement boundaries, run in parallel with the read, with the cuts checked against the serial parse in a test.
