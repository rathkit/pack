# Changelog

All notable changes to rathkit-pack are listed here, newest first. The notes of each GitHub
Release are the section of its version.

## Versions

rathkit-pack follows [Semantic Versioning](https://semver.org) with the usual 0.x reading:
**while the major version is 0, any release may change options, output and plugin support**.
A change that breaks a documented option or flag is listed under "Changed" or "Removed" with the
version in which it happened, and the minor number is raised for it (0.1 to 0.2); patch releases
fix bugs only. Versions with a suffix (`-alpha.N`) are published as pre-releases, never as the
latest release, and promise nothing but what the README says is tested.

## 0.1.0-alpha.0

First release.

### Added

- A bundler for JavaScript, TypeScript, JSX, JSON and CSS with no dependencies (not even crates):
  module graph, linking, tree shaking, code splitting, minifying, source maps, defines, loaders
  (text, base64, data URL, binary, file, empty, CSS and local CSS) and `?raw`, `?url` and `?inline`
  import suffixes.
- Plugins: Rollup and Vite plugins, esbuild plugins, webpack loaders and compiler plugins, and
  Parcel plugins, run by a Node 24 or later process. What is covered by tests is listed in the README.
- Module federation: containers, remotes, shared packages, the manifest, generated and consumed
  types, runtime plugins and development servers, compatible with the containers rspack and
  Vite build (tested in Chromium).
- `--watch` and `serve` (a development server with live reload).
- Release files: one tarball per platform, `SHA256SUMS`, an SBOM that lists zero components, and
  Sigstore signatures. The binaries are not signed with an operating-system key; see
  [docs/verifying.md](docs/verifying.md).
- `rkp --version`.
- npm packages: `@rathkit/pack` (a launcher) and one package per platform, published with provenance on the `next` tag.
