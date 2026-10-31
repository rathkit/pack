# Verifying a release

Every GitHub Release of Rathkit Pack has, for each version:

| File | What it is |
| --- | --- |
| `rathkit-pack-VERSION-PLATFORM.tar.gz` | the binary, `LICENSE` and `README.md` |
| `SHA256SUMS` | the SHA-256 of every file above and of the SBOM |
| `rathkit-pack-VERSION.sbom.cdx.json` | the software bill of materials (CycloneDX); it lists **zero** components |
| `*.sigstore.json` | a Sigstore (keyless) signature of each file above, made by this repository's release workflow |

The binaries are **not** signed with an Apple Developer ID or a Windows code-signing certificate.
macOS and Windows may warn about an unsigned program; the checks below are what to rely on.

## Checksums

```
sha256sum --check --ignore-missing SHA256SUMS      # shasum -a 256 -c on macOS
```

## Signatures

The signature proves the file was made by the `release` workflow of `rathkit/pack` for a version tag, and not
by anyone else. With [cosign](https://docs.sigstore.dev/cosign/):

```
cosign verify-blob \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github.com/rathkit/pack/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

Verify `SHA256SUMS` this way, then the tarball against `SHA256SUMS`. The same command works for any file with its own
`.sigstore.json`.

## npm

`@rathkit/pack` is the same program for npm: a launcher with one optional dependency per platform, and the package for
your platform: `@rathkit/pack-linux-x64-gnu`, `-linux-arm64-gnu`, `-linux-x64-musl`, `-linux-arm64-musl`,
`-darwin-arm64`, `-darwin-x64` and `-win32-x64`. None of them has an install script or any dependency but those. Each is staged by the `release` workflow with a
provenance statement and goes live when the owner approves it:

```
npm install --save-dev @rathkit/pack@next
npm audit signatures
```

Versions with a suffix (`0.1.0-alpha.0`) are on the `next` tag only. The first versions of a package are published by
hand and have no provenance statement (the release notes say which); later ones do.

## crates.io

`cargo install rathkit-pack --version <version>` builds the program from the crate on crates.io. crates.io has no provenance
statements, so nothing there says which commit a crate came from. The tag does: the signed `v<version>` tag, and the
checksums and signatures of the release files above, are the record of what that version is, and the crate's source is the
`src` folder of that tag.

## Zero dependencies

```
node -e "const s=require('./rathkit-pack-VERSION.sbom.cdx.json'); console.log(s.components.length)"     # 0
```

The release is one executable. There is nothing to install and no install script. To see the same from source:

```
git clone https://github.com/rathkit/pack && cd pack
git checkout vVERSION
node tools/check-zero-deps.mjs                   # empty dependency tables, no build script, a lock file with one package
cargo build --release --locked
```

The plugin host (`node` is used when a config lists plugins) and the federation runtime are scripts inside the binary.
Packages that the tests need (`bench/package.json`) are never part of a release.
