# Verifying a release

Every GitHub Release of rathkit-pack has, for each version:

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
