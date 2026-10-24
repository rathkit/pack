#!/usr/bin/env node
// Starts the rathkit-pack program for this platform. The program itself comes in a package of its own
// (@rathkit/pack-<platform>-<cpu>) that npm installs because its `os` and `cpu` fit; nothing runs at install time.
'use strict';
const { spawnSync } = require('node:child_process');
const { existsSync } = require('node:fs');
const { dirname, join } = require('node:path');

// On Linux the package depends on the C library: glibc reports its version, musl (Alpine) does not.
const libc = process.platform !== 'linux' ? '' : process.report?.getReport?.().header?.glibcVersionRuntime ? '-gnu' : '-musl';
const platform = `${process.platform}-${process.arch}${libc}`;
const exe = process.platform === 'win32' ? 'rkp.exe' : 'rkp';

function find() {
  if (process.env.RATHKIT_PACK_BIN) return process.env.RATHKIT_PACK_BIN;
  try {
    return join(dirname(require.resolve(`@rathkit/pack-${platform}/package.json`)), 'bin', exe);
  } catch {
    return null;
  }
}

const bin = find();
if (!bin || !existsSync(bin)) {
  console.error(
    `rkp: there is no build for ${platform}.\n` +
      'Builds exist for linux-x64 and linux-arm64 (glibc: -gnu, Alpine: -musl), darwin-x64, darwin-arm64 and win32-x64.\n' +
      'If the platform is one of those, the package @rathkit/pack-' + platform + ' was not installed:\n' +
      'install again without --omit=optional. Otherwise build from source: https://github.com/rathkit/pack',
  );
  process.exit(1);
}
const result = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  console.error(`rkp: cannot start ${bin}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) process.kill(process.pid, result.signal);
process.exit(result.status ?? 1);
