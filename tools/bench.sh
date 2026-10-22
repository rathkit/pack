#!/bin/sh
# Best-of-N throughput for each phase, using the profiler binary (no sampling).
#   tools/bench.sh CORPUS.txt [RUNS]
# Build the binary first:
#   RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR=target/prof cargo build --profile profiling --features profiling --bin prof
corpus=$1
runs=${2:-5}
for phase in lex parse; do
  best=0
  i=0
  while [ $i -lt $runs ]; do
    mbs=$(target/prof/profiling/prof $phase 1 $(cat "$corpus") | sed -n 's/.*, \([0-9.]*\) MB\/s.*/\1/p')
    best=$(echo "$mbs $best" | awk '{print ($1 > $2) ? $1 : $2}')
    i=$((i + 1))
  done
  printf '%-8s best of %s: %6.1f MB/s\n' "$phase" "$runs" "$best"
done
