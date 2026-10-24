#!/usr/bin/env node
// Starts the rkp program for this platform. The program itself comes in a package of its own
// (@rathkit/pack-<platform>) that the package manager installs because its `os`, `cpu` and `libc` fit; nothing runs at install time.
'use strict';
const { spawnSync } = require('node:child_process');
const { binaryPath } = require('../index.js');

let bin;
try {
  bin = binaryPath();
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
const result = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  console.error(`rkp: cannot start ${bin}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) process.kill(process.pid, result.signal);
process.exit(result.status ?? 1);
