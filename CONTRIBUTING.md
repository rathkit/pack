# Contributing to Rathkit Pack

Thank you for wanting to help. This page is the whole process, in the order you will meet it.

**Status: alpha.** This repository is public and 0.1.0-alpha.1 is on npm and crates.io, so outside pull requests are welcome for issues a maintainer has labeled `accepted`. Questions are welcome on Discord (see the end of this page).

## The short version

1. Read the [Code of Conduct](CODE_OF_CONDUCT.md) and the [AI policy](AI_POLICY.md).
2. Find an issue, or open one. Wait until a maintainer labels it `accepted`.
3. Fork the repository. Work in your fork, never in this one.
4. Sign every commit with a verified GPG key and add a DCO sign-off.
5. Open a pull request from your fork and fill in the template. CI must pass.
6. A maintainer reviews it. We merge when it is ready.

## Roles

Everyone starts with read access, and that is enough to fork, open issues and send pull requests.
You do not need to apply for anything to contribute code.

| Role | Access | How |
| --- | --- | --- |
| Everyone | Read | Nothing to do |
| Contributor | Triage: label and assign issues and pull requests | Apply with the "Apply for a role" issue form |
| Maintainer | Write: review and merge pull requests | By invitation |
| Owner | Admin | The project owner |

The people in each role are listed in [MEMBERS.md](MEMBERS.md). How roles are given and removed is
in [GOVERNANCE.md](GOVERNANCE.md).

## Fork only

Only the owner creates branches in this repository. Everyone else, maintainers included, works
from a fork and opens pull requests against `main`. The repository blocks other branches, so a
push to a branch here will fail.

## Issue first

Open an issue and describe the problem before you write code, then wait for the `accepted` label.
This saves you from writing a change we cannot take. A typo, a broken link or a one-line fix can
skip the issue.

## Sign your commits and sign off

Two separate things are required on every commit.

**A GPG signature** proves who made the commit. Set up a key and tell GitHub about it:

```sh
gpg --full-generate-key
git config --global user.signingkey <your key id>
git config --global commit.gpgsign true
```

Add the public key under Settings, SSH and GPG keys, and use an email address that is verified on
your GitHub account. GitHub shows a "Verified" badge on signed commits, and CI requires it.

**A DCO sign-off** is a line saying you have the right to submit the work, under the
[Developer Certificate of Origin](DCO). `git commit -s` adds it:

```text
Signed-off-by: Your Name <you@example.com>
```

The name and email must match the commit's author. To fix commits you already made:

```sh
git commit --amend --no-edit -s -S                 # the last commit
git rebase --exec 'git commit --amend --no-edit -s -S' origin/main   # all commits on your branch
git push --force-with-lease                        # to your fork
```

## Commit messages

- `type(scope): subject`, in the Conventional Commits style. Types: `feat`, `fix`, `docs`,
  `test`, `perf`, `refactor`, `chore`, `ci`, `build`, `style`.
- Imperative, lower case after the colon, no trailing period.
- The body explains why, not what. Write in American English.
- No other trailers. See the [AI policy](AI_POLICY.md).

## Before you open a pull request

```sh
cargo build --release
cargo test --release
```

CI runs more checks, listed in `.github/workflows/ci.yml`. Run what applies to your change.

## Pull requests

- One change per pull request, small enough to review in one sitting.
- Use the template. Say what changed and why, and how you checked it.
- Add tests for behavior changes and update the docs.
- Reply to review comments yourself (see the [AI policy](AI_POLICY.md)).

## Review and merging

Maintainers review every pull request, and the people in [CODEOWNERS](.github/CODEOWNERS) must
approve changes to their areas. Review is best-effort and there is no response-time promise. We may
close a pull request that does not follow this page.

## What we do not take

- **New runtime dependencies.** Rathkit packages ship with none, and CI enforces it.
- Code for the paid Rathkit Pro packages. Those do not accept outside contributions.
- Code copied from another project unless its license allows it and you say so in the pull
  request.
- Changes that skip signing or the sign-off.

## Security

Do not report security problems in issues. Follow the [security policy](https://github.com/rathkit/.github/blob/main/SECURITY.md).

## License

By contributing you certify the [DCO](DCO) and agree that your contribution is licensed under the
same license as the project, Apache-2.0 (see [LICENSE](LICENSE)). The copyright holder is named
in [NOTICE](NOTICE). You keep the copyright in your own
contribution.

## Questions

[Discord](https://discord.gg/A5VBxgPaex). It is best-effort and not a support commitment.
