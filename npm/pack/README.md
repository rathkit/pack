# @rathkit/pack

**Alpha.** rathkit-pack is a JavaScript and TypeScript bundler written in Rust with no dependencies. This package
starts the program for your platform; it has no dependencies of its own, runs nothing when installed, and only
lists the one platform package that fits as an optional dependency.

```
npm install --save-dev @rathkit/pack@next
npx rathkit-pack bundle src/main.ts --outfile dist/app.mjs
```

Versions with a suffix such as `0.1.0-alpha.0` are published under the `next` tag, never `latest`. Node 24 or later is
needed only when a config lists plugins. What works and what is tested: https://github.com/rathkit/pack#readme.
How to check a release: https://github.com/rathkit/pack/blob/main/docs/verifying.md.

Builds: linux-x64 and linux-arm64 (glibc and musl, so Alpine too), darwin-x64, darwin-arm64, win32-x64. They are not signed with an
operating-system key.
