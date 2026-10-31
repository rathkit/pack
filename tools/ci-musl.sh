#!/usr/bin/env bash
# Runs the checks of the ci job on Alpine (musl) in a container, for one Node major: tools/ci-musl.sh 24
# GitHub's runner cannot run JavaScript actions (checkout, pnpm/setup) in an Alpine container on arm64, so these legs do not use
# `container:`; the steps run here, inside `docker run`, the same ones on x64 and arm64. Every step is run and reported, and the
# script fails at the end if any failed. Keep the list in step with the check job in .github/workflows/ci.yml.
set -u
if [ "${1:-}" != --inside ]; then
  node=${1:?usage: tools/ci-musl.sh NODE_MAJOR}
  root=$(cd "$(dirname "$0")/.." && pwd)
  exec docker run --rm -v "$root":/w node:"$node"-alpine sh -c 'apk add --no-cache bash >/dev/null && bash /w/tools/ci-musl.sh --inside'
fi

cd /w
apk add --no-cache git rust cargo python3 bash curl >/dev/null || { echo "apk failed"; exit 1; }
git config --global --add safe.directory /w
# pnpm, the musl build, at a pinned version and checksum (pnpm/setup is an action, and actions cannot run here on arm64).
PNPM_VERSION=v12.14.0
case "$(uname -m)" in
  x86_64) arch=x64; sha=d7115e2f2791d79bf9e7e448609537595bb7787de1ef5292ce91ff3d1be2629e ;;
  aarch64) arch=arm64; sha=511a54809bdfd28270d30011d1697f582c8130d555b4fa0e2856b1a7534c98bf ;;
  *) echo "unsupported CPU $(uname -m)"; exit 1 ;;
esac
curl -fsSL -o /tmp/pnpm.tar.gz "https://github.com/pnpm/pnpm/releases/download/$PNPM_VERSION/pnpm-linux-$arch-musl.tar.gz"
echo "$sha  /tmp/pnpm.tar.gz" | sha256sum -c - >/dev/null || { echo "pnpm checksum mismatch"; exit 1; }
mkdir -p /opt/pnpm && tar -xzf /tmp/pnpm.tar.gz -C /opt/pnpm
ln -sf "$(find /opt/pnpm -type f -name pnpm | head -1)" /usr/local/bin/pnpm
export TS_MODULE=/w/bench/node_modules/ts5 RUNNER_TEMP=/tmp GITHUB_WORKSPACE=/w CI=true
node --version; pnpm --version; cargo --version

failed=0
step() {
  local name=$1; shift
  echo "::group::$name"
  if "$@"; then echo "::endgroup::"; echo "ok   $name"; else echo "::endgroup::"; echo "::error::FAIL $name"; failed=1; fi
}
step "install bench" bash -c 'cd bench && pnpm install --frozen-lockfile --ignore-scripts'
step zero-deps node tools/check-zero-deps.mjs
step sbom bash -c 'node tools/make-sbom.mjs /tmp/sbom.cdx.json && node -e "process.exit(require(process.argv[1]).components.length)" /tmp/sbom.cdx.json'
step build cargo build --release
step version ./target/release/rkp --version
step cargo-test cargo test --release
for t in run-fixtures run-defines run-loaders run-plugins run-assets run-target test-npm-wrapper check-promises run-split run-sourcemaps run-watch run-serve run-jsx test-federation-runtime diff-semver; do
  step "$t" node "tools/$t.mjs"
done
step fuzz-bundle python3 tools/fuzz-bundle.py --count 300
step diff-real node tools/diff-real.mjs /w/bench
exit $failed
