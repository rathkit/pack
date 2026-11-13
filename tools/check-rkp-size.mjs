// The rkp binary is the same with and without the checker's source in the repository (roadmap 2.12).
//
//   node tools/check-rkp-size.mjs
//
// The optional checker is a separate crate of the workspace (check/), not linked into rkp. This builds rkp in release mode with the
// workspace as it is, then again with the checker removed from the workspace (the `check` entry of Cargo.toml and the directory moved
// aside; both are put back at the end, also when a build fails; each build starts from a clean release build of rkp), and fails unless the two binaries are the same size. It prints both
// sizes and SHA-256 hashes: with the same toolchain and the same source path the files are expected to be byte for byte identical, and
// a difference in bytes at the same size is reported as a note, not a failure.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const exe = join(root, 'target', 'release', process.platform === 'win32' ? 'rkp.exe' : 'rkp');
const manifestPath = join(root, 'Cargo.toml');
const lockPath = join(root, 'Cargo.lock');
const aside = join(root, '.check-aside');

function build() {
  // Cargo does not treat the workspace's member list as an input of the binary, so without this the second build would be the first
  // one's file again and the comparison would prove nothing.
  const clean = spawnSync('cargo', ['clean', '--release', '--package', 'rathkit-pack'], { cwd: root, stdio: 'inherit' });
  if (clean.status !== 0) throw new Error(`cargo clean failed (${clean.status})`);
  const r = spawnSync('cargo', ['build', '--release', '--bin', 'rkp'], { cwd: root, stdio: 'inherit' });
  if (r.status !== 0) throw new Error(`cargo build failed (${r.status})`);
  const bytes = readFileSync(exe);
  return { size: statSync(exe).size, sha: createHash('sha256').update(bytes).digest('hex') };
}

if (!existsSync(join(root, 'check', 'Cargo.toml'))) {
  console.error('check/Cargo.toml not found: nothing to compare against');
  process.exit(2);
}
const manifest = readFileSync(manifestPath, 'utf8');
const lock = existsSync(lockPath) ? readFileSync(lockPath) : null;
const withMember = /members\s*=\s*\[[^\]]*"check"[^\]]*\]/.test(manifest);
if (!withMember) {
  console.error('Cargo.toml does not list "check" as a workspace member');
  process.exit(2);
}
let withChecker, without;
let moved = false;
try {
  withChecker = build();
  renameSync(join(root, 'check'), aside);
  moved = true;
  writeFileSync(manifestPath, manifest.replace(/members\s*=\s*\[[^\]]*\]/, 'members = ["."]'));
  without = build();
} catch (e) {
  console.error(String(e.message ?? e));
  process.exitCode = 2;
} finally {
  if (moved) renameSync(aside, join(root, 'check'));
  writeFileSync(manifestPath, manifest);
  if (lock) writeFileSync(lockPath, lock);
}
if (!withChecker || !without) process.exit(process.exitCode ?? 2);
console.log(`rkp with the checker in the workspace:    ${withChecker.size} bytes  sha256 ${withChecker.sha}`);
console.log(`rkp without the checker in the workspace: ${without.size} bytes  sha256 ${without.sha}`);
if (withChecker.size !== without.size) {
  console.error(`FAIL: the rkp binary changed size by ${withChecker.size - without.size} bytes because of the checker`);
  process.exit(1);
}
console.log(withChecker.sha === without.sha ? 'ok: the two binaries are byte for byte identical' : 'ok: the same size (the bytes differ; see the note in the header of this tool)');
