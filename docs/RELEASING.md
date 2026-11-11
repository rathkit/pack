# Releasing

How a release of Rathkit Pack is made, and the steps only the owner can take. Nothing here publishes by itself: the
`release` workflow builds, tests, lists and signs on every version tag, and stages the npm packages only when the repository
variable `NPM_PUBLISH` is `true`, and publishes the crate to crates.io only when `CRATES_PUBLISH` is `true`. A staged npm
version is not on npm until the owner approves it; crates.io has no staging, so approving the `crates-io` environment is the
one step that publishes there.

## Versions and tags

- A release is a git tag `v<version>`, and `<version>` is the one in `Cargo.toml`. The workflow refuses a tag that is not.
- While the project is before 1.0, every version has a suffix (`-alpha.N`, `-beta.N`, `-rc.N`). The GitHub release is a pre-release, and the npm
  packages go to the `next` dist-tag, never `latest`.
- The notes of a release are its section of `CHANGELOG.md` (`tools/release-notes.mjs`), so write the section first.
- **The release is immutable the moment it is created.** Its tag can never be moved, replaced or reused, and its files can
  never change. Check the tag before you push it.

## Making a release

1. Put the version in `Cargo.toml` (and `Cargo.lock`, which `cargo build` updates), and add the `## <version>` section to
   `CHANGELOG.md` with its date. Commit that as `chore(release): <version>` (signed, with the DCO sign-off).
2. Check the time on your machine, since CI only gates size and peak memory (`tools/check-baseline.mjs`, run on every change): run the
   benchmark of [rathkit/pack-bench](https://github.com/rathkit/pack-bench) in its container on the release commit and compare it
   with the last published run. No case that Pack won on time may have become a loss or a tie, and no median may be slower by
   more than the run-to-run noise of that case (the bench repository's `report.mjs` prints both runs). A slower case is fixed first
   or named in the `CHANGELOG.md` section with the reason.
3. Push the commit, then run the corpus on it: **Actions, corpus, Run workflow**, on `main`. All of the projects must build and
   check out (the summary of the run starts with the count, for example "60 of 60 built and checked out"). A project that
   fails is either fixed first or named in the `CHANGELOG.md` section with the reason. The corpus fetches other projects from
   the network, so it never blocks a release by itself; it is a step here instead.
4. Make the tag signed, on that commit: `git tag -s v<version> -m v<version>`.
5. Check it: `git tag -v v<version>` must say "Good signature", and `git cat-file -t v<version>` must say `tag`.
6. Push the tag: `git push origin v<version>`.
7. The `release` workflow builds seven platforms, runs the tests and a smoke build on each, writes `SHA256SUMS` and the SBOM,
   signs every file with Sigstore, and creates the GitHub pre-release.
8. The `npm` job (only when `NPM_PUBLISH` is `true`) waits for the owner to approve the `npm` environment, then stages the
   eight packages from the release (checked against `SHA256SUMS`) with provenance through npm trusted publishing: no npm token is stored anywhere.
9. The owner approves the staged versions (below).
10. The `crates` job (only when `CRATES_PUBLISH` is `true`) waits for the owner to approve the `crates-io` environment, gets a
   30-minute token from crates.io through trusted publishing, runs `cargo publish --locked` and revokes the token. **That is
   permanent**: a version on crates.io can be yanked but never removed, and its number can never be reused.

The `release` workflow also calls the corpus workflow for the tag, so every release has a run of its own beside it (it does not hold
the release up). The corpus runs every Monday as well. Each run keeps `corpus.json` (the Rathkit Pack version and the result for each
project) and `corpus.txt` (the log) as the workflow artifact `corpus-<run id>` for 90 days, and its summary begins with the count.

By hand, `release` is a dry run: **Actions, release, Run workflow**, with a tag such as `v0.1.0-alpha.0`. Everything is
built, tested and signed and kept as a workflow artifact; nothing is released or published.

## Approving a release

The `release` job packs the eight npm packages and the crate once, and `SHA256SUMS` (signed) lists them with the binaries. The `npm`
job stages those very files (it stops unless each matches `SHA256SUMS`), and the `crates` job packs the crate again and stops
before it asks crates.io for a token unless the hash is the one in `SHA256SUMS`. Before approving anything, run the check:

```
node tools/approve-release.mjs v<version>             # check only; nothing is approved
node tools/approve-release.mjs v<version> --approve   # check, then approve what matches
```

It needs `npm login` and `gh auth login` as the owner. It downloads `SHA256SUMS` from the release, then:

- **npm**: lists what is staged (`npm stage list --json`; only `staged` entries count), downloads each tarball
  (`npm stage download`) and compares its hash. A package that is not staged is skipped when `npm view` shows that version is
  already published (a run that stopped half way); it stops if a package is neither, or is staged twice.
- **crates.io**: compares the release's `.crate` with `SHA256SUMS`, checks that the version is not on crates.io yet, and finds the
  run waiting at the `crates-io` environment.

Only if everything matches, and only with `--approve`, it asks for one typed `yes`, then once for the six-digit code from the
authenticator app (leave it empty for the browser sign-in that a passkey or security key needs). It runs
`npm stage approve <id> --otp <code>` for each package, and asks for a fresh code if one expires part way. Then it approves the
`crates-io` environment, which lets the `crates` job publish. `--npm` or `--crates` limits it to one registry. Run it again after
an interruption: what is already live is skipped.

The registries differ. npm stages, so nothing is live until approval, and approval is per package. crates.io has no staging and a
version can never be removed, so the check comes before the gate, and approving the environment is what publishes. Neither uses a
stored token: npm uses trusted publishing for staging and your login for approval, and crates.io gives the job a 30-minute token.

Then check `npm view @rathkit/pack dist-tags` (`next` is the new version and `latest` has not moved) and install it in an empty
project. By hand, `npm stage list`, `npm stage download <stage-id>` and `npm stage reject <stage-id>` do the same one at a time.
A version that is still `validating` cannot be approved yet.

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

## crates.io

The crate is `rathkit-pack` (the `rkp` program and the `rathkit_pack` library), built from source by whoever installs it:
`cargo install rathkit-pack --version <version>`. Cargo skips versions with a suffix unless the version is named.
`Cargo.toml` lists what goes in the crate (`include`: `src`, `Cargo.toml`, `Cargo.lock`, the README, LICENSE, NOTICE and
CHANGELOG), and every run of the `release` workflow packs and builds the crate from those files without uploading it.

The first version is published by hand, because trusted publishing can only be set up for a crate that exists:

1. On crates.io, sign in with GitHub and verify your email. Create an API token (Account Settings, API Tokens) named
   `pack first publish` with only the `publish-new` scope, limited to the crate pattern `rathkit-pack`, and a short expiry.
2. In a clean checkout of the release tag: `cargo publish --dry-run --locked`.
3. Then `read -rs CARGO_REGISTRY_TOKEN; export CARGO_REGISTRY_TOKEN; cargo publish --locked` and paste the token. This keeps it
   out of `~/.cargo/credentials.toml`.
4. Revoke the token on crates.io.
5. In the crate's settings, Trusted Publishing, add a GitHub publisher: owner `rathkit`, repository `pack`, workflow
   `release.yml`, environment `crates-io`.
6. In the repository, create the `crates-io` environment (required reviewer, tag rule `v*`, no admin bypass) and set the variable
   `CRATES_PUBLISH` to `true`. Release the next version through the workflow.

crates.io has no provenance statement for crates. What ties a crate to its source is the signed tag and the signed release
files; see [verifying.md](verifying.md).
