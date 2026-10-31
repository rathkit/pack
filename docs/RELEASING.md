# Releasing

How a release of Rathkit Pack is made, and the steps only the owner can take. Nothing here publishes by itself: the
`release` workflow builds, tests, lists and signs on every version tag, and stages the npm packages only when the repository
variable `NPM_PUBLISH` is `true`. A staged version is not on npm until the owner approves it.

## Versions and tags

- A release is a git tag `v<version>`, and `<version>` is the one in `Cargo.toml`. The workflow refuses a tag that is not.
- While the project is in alpha, every version has a suffix (`-alpha.N`). The GitHub release is a pre-release, and the npm
  packages go to the `next` dist-tag, never `latest`.
- The notes of a release are its section of `CHANGELOG.md` (`tools/release-notes.mjs`), so write the section first.
- **The release is immutable the moment it is created.** Its tag can never be moved, replaced or reused, and its files can
  never change. Check the tag before you push it.

## Making a release

1. Put the version in `Cargo.toml` (and `Cargo.lock`, which `cargo build` updates), and add the `## <version>` section to
   `CHANGELOG.md` with its date. Commit that as `chore(release): <version>` (signed, with the DCO sign-off).
2. Make the tag signed, on that commit: `git tag -s v<version> -m v<version>`.
3. Check it: `git tag -v v<version>` must say "Good signature", and `git cat-file -t v<version>` must say `tag`.
4. Push the commit, then the tag: `git push origin main` and `git push origin v<version>`.
5. The `release` workflow builds seven platforms, runs the tests and a smoke build on each, writes `SHA256SUMS` and the SBOM,
   signs every file with Sigstore, and creates the GitHub pre-release.
6. The `npm` job (only when `NPM_PUBLISH` is `true`) waits for the owner to approve the `npm` environment, then stages the
   eight packages with provenance through npm trusted publishing: no npm token is stored anywhere.
7. The owner approves the staged versions (below).

By hand, `release` is a dry run: **Actions, release, Run workflow**, with a tag such as `v0.1.0-alpha.0`. Everything is
built, tested and signed and kept as a workflow artifact; nothing is released or published.

## Approving a release

```
npm stage list
npm stage download <stage-id>     # the exact tarball, to compare with the release
npm stage approve <stage-id>      # asks for the two-factor code
npm stage reject <stage-id>
```

Approve all eight. A version that is still `validating` cannot be approved yet. Then check
`npm view @rathkit/pack dist-tags` (`next` is the new version and `latest` has not moved) and install it in an empty project.

## The first release of each package

Trusted publishing can only be configured for a package that exists, so the first version of each of the eight packages
is published by hand from the release's own tarballs, with the `next` tag and no provenance:

1. Download the tarballs of the release and run `node tools/make-npm-packages.mjs --out npm-out --from <folder>`.
2. For each folder in `npm-out`: `npm pack`, then `npm publish ./<file>.tgz --access public --tag next --provenance=false`.
   (The `./` matters: without it npm reads `dir/name.tgz` as a GitHub repository.)
3. npm gives a first version the `latest` tag whatever `--tag` says, and cannot remove `latest`. Move it to the
   placeholder npm creates: `npm dist-tag add @rathkit/<package>@0.0.0-stage latest`.
4. On each package's npm page, set the trusted publisher: organization `rathkit`, repository `pack`, workflow `release.yml`,
   environment `npm`; leave "Allow npm publish" and "Allow npm dist-tag" **unchecked** (so every version is staged), and
   set publishing access to "Require two-factor authentication and disallow bypass 2fa tokens".
5. npm says a trusted publisher is "not yet validated" until a publish through the workflow happens. Release the next
   alpha through the workflow soon after.

The release notes of such a version say that it was published by hand and carries no provenance statement.
