/** Options for {@link build} and {@link serve}: what the `rkp` command takes as flags. */
export interface Options {
  /** Files to build, or html pages (their module scripts and style sheets are built and the page is written again). */
  entries: string | string[];
  /** Where relative paths are taken from. Default: the current directory. */
  cwd?: string;
  /** Added to the environment of the program (and of plugins). */
  env?: Record<string, string>;
  /** Stops the program when aborted. */
  signal?: AbortSignal;

  /** One output file. A single entry, or entries that share one file. */
  outfile?: string;
  /** A folder of output files (needed for splitting, pages and federation). */
  outdir?: string;
  /** Default `esm`. A page cannot be `cjs`. */
  format?: 'esm' | 'cjs' | 'iife';
  /** The global an `iife` assigns its exports to. */
  globalName?: string;
  /** For an `iife`: the global each external module is read from, `{ react: 'React' }`. */
  globals?: Record<string, string>;
  /** The version of JavaScript the output may use: `es2015` to `es2025`, or `esnext` (the default; nothing is rewritten). Newer syntax is rewritten, or the build stops saying what could not be: BigInt literals and top-level await cannot be, and some others are not yet. */
  target?: string;
  /** `true` is whitespace, identifiers and syntax together. */
  minify?: boolean | { whitespace?: boolean; identifiers?: boolean; syntax?: boolean };
  keepNames?: boolean;
  /** `true` is `linked`: a `.map` file and a comment pointing at it. */
  sourcemap?: boolean | 'linked' | 'inline' | 'external';
  /** `false` leaves the original text out of the map. */
  sourcesContent?: boolean;
  /** Code shared by entries and loaded with `import()` goes to files of its own (`esm` only). */
  splitting?: boolean;
  /** `false` keeps everything. Default `true`. */
  treeShaking?: boolean;
  /** `browser` does not treat Node's built-in modules as external. Default `node`. */
  platform?: 'node' | 'browser';
  /** Text put before the code of every file: for scripts (a string), or for each kind of file. */
  banner?: string | { js?: string; css?: string };
  /** Text put after the code of every file. */
  footer?: string | { js?: string; css?: string };
  /** Remove every `console.*(...)` call, and every `debugger` statement. */
  drop?: Array<'console' | 'debugger'>;
  /** Names whose calls have no effects (`'styled.div'`), as if each carried a `@__PURE__` comment: an unused call is removed, if what it is given has no effects either. */
  pure?: string[];
  /** `external` leaves every package import out of the output (paths and aliases are still bundled). Default `bundle`. */
  packages?: 'external' | 'bundle';
  /** The extensions tried for an import without one, with or without the dot. Default `.tsx .ts .jsx .js .mjs .cjs .mts .cts .json`. */
  resolveExtensions?: string[];
  /** Keep a symbolic link's own path instead of the real file's. */
  preserveSymlinks?: boolean;
  /** Plugins: objects with hooks (kept in this process; see `Plugin`), or `{ module }` to load one where the build runs. They come after those of the config file. */
  plugins?: Array<Plugin | PluginModule | Array<Plugin | PluginModule>>;
  /** `serve` only: serve over HTTPS. `true` makes (and keeps) a self-signed certificate for localhost that the browser has to be told to trust; give `{ cert, key }` (PEM files) to use your own, such as one made by mkcert. Needs Node.js. */
  https?: boolean | { cert?: string; key?: string };
  /** `serve` only: replace changed modules in the open page (`import.meta.hot.accept`) instead of reloading it. The modules are a table of functions, not one scope-hoisted program; it does not work with module federation. */
  hmr?: boolean;
  /** Write declaration files (`.d.ts`) for the entries with the project's own `typescript` package: one for each source file, laid out beside the output. */
  dts?: boolean;
  /** The folder for the declaration files, when not the output folder (implies `dts`). */
  dtsDir?: string;
  /** Write what went into the build and what came out, as JSON in the shape of esbuild's metafile, to this file (the result has it too). */
  metafile?: string;
  /** Specifiers left out of the output: exact names, or a prefix ending in `/*`. */
  external?: string[];
  /** Conditions of `exports` and `imports` maps, besides the usual ones. */
  conditions?: string[];
  mainFields?: string[];
  /** A name, and anything below it, resolves to a path. */
  alias?: Record<string, string>;
  /** Expressions replaced by values: `{ 'process.env.NODE_ENV': '"production"' }`. */
  define?: Record<string, string>;
  /** How files with an extension are loaded: `{ '.svg': 'dataurl' }` (text, base64, dataurl, binary, file, empty, css, local-css). */
  loader?: Record<string, 'text' | 'base64' | 'dataurl' | 'binary' | 'file' | 'empty' | 'css' | 'local-css'>;
  /** What becomes of the comments that say who wrote code and under what terms (`/*! ... *\/`, `@license`, `@preserve`), which minifying would drop: put after the code of the file that has it (`eof`, the default), left out (`none`), put in `NAME.LEGAL.txt` and named at the end of the file (`linked`), or put in that file without a mention (`external`). */
  legalComments?: 'eof' | 'none' | 'linked' | 'external';
  /** Files whose named exports stand in for the globals of the same names (`Buffer`, `process`) in every module that uses one; a file no module needs is not bundled. */
  inject?: string[];
  /** The names of entry files, with `[name]` and `[hash]` filled in (a hash of what is in the file and in the files it loads, so a name changes when anything the file runs does). Default `[name]`. No directories. */
  entryNames?: string;
  /** The names of the other files (shared code, `import()`ed code), with `[name]` and `[hash]`. Default `[name]-[hash]`. No directories. */
  chunkNames?: string;
  /** The extension of each kind of output file: `{ '.js': '.mjs' }`. */
  outExtension?: { '.js'?: string; '.css'?: string };
  /** The names of copied files; `[name]`, `[hash]` and `[ext]` (no dot) are filled in. */
  assetNames?: string;
  /** What copied files' addresses start with. */
  publicPath?: string;
  jsx?: 'automatic' | 'classic';
  /** Compile JSX to `jsxDEV` of `<jsxImportSource>/jsx-dev-runtime`, told where each element is written (React's development builds). `tsconfig.json` `"jsx": "react-jsxdev"` does the same. */
  jsxDev?: boolean;
  jsxImportSource?: string;
  jsxFactory?: string;
  jsxFragment?: string;
  decoratorMetadata?: boolean;
  experimentalDecorators?: boolean;
  strictNullChecks?: boolean;
  /** A tsconfig given as text or as an object, used in place of any `tsconfig.json`. */
  tsconfigRaw?: string | Record<string, unknown>;
  /** A `tsconfig.json` to use, or `false` for none. */
  tsconfig?: string | false;
  /** A config file that lists plugins (`rathkit.config.mjs`), or `false` to ignore the one next to the project. */
  config?: string | false;
  /** The mode: `import.meta.env.MODE`, which `.env.[mode]` files are read, and what plugins are told. Default `production` (`development` for `serve`). */
  mode?: string;
  /** Where the `.env` files are. Default: the current directory. */
  envDir?: string;
  /** Names of `.env` files and of the environment that reach `import.meta.env`: those that start with one of these. Default `VITE_`. */
  envPrefix?: string | string[];
  /** `false` reads no `.env` files: `import.meta.env` has only `MODE`, `DEV`, `PROD`, `SSR` and `BASE_URL`. */
  envFiles?: boolean;
  /** Module federation: a JSON file, or the options of a `ModuleFederationPlugin`. */
  federation?: string | Record<string, unknown>;
  /** Names of files in the output keep their original paths in maps and `import.meta.url`. */
  originalPaths?: boolean;
  /** `serve` only: other origins that may load this server's files (federation). */
  corsOrigins?: string[];
  /** `serve` only. */
  port?: number;
  /** `serve` only. */
  host?: string;
}

/** What went into a build and what came out: the shape of esbuild's metafile. `bytesInOutput` is an estimate (the original sizes, scaled to the file's size). */
/** What a hook of a plugin object can call. `resolve` and `load` are asynchronous; the rest answer at once, and the hook waits for the build while they do (the plugin object lives in another thread of the build, so a hook blocks on them). Methods that are not listed here are not available to objects passed to the API: put such a plugin in a config file. */
export interface PluginContext {
  meta: { rollupVersion: string; watchMode: boolean };
  resolve(source: string, importer?: string, options?: { skipSelf?: boolean; isEntry?: boolean; custom?: Record<string, unknown> }): Promise<{ id: string; external: boolean | 'absolute' | 'relative'; moduleSideEffects: boolean | 'no-treeshake'; meta: Record<string, unknown> } | null>;
  load(options: { id: string } | string): Promise<{ id: string; code: string | null }>;
  warn(message: string | { message: string }): void;
  info(message: string | { message: string }): void;
  debug(message: string | { message: string }): void;
  error(message: string | { message: string; code?: string }): never;
  addWatchFile(file: string): void;
  emitFile(file: { type: 'asset'; fileName?: string; name?: string; source: string | Uint8Array } | { type: 'chunk'; id: string; fileName?: string; name?: string; importer?: string }): string;
  getFileName(reference: string): string;
  setAssetSource(reference: string, source: string | Uint8Array): void;
  parse(code: string, options?: Record<string, unknown>): Record<string, unknown>;
  getModuleInfo(id: string): Record<string, unknown> | null;
  getModuleIds(): IterableIterator<string>;
  getWatchFiles(): string[];
}

type Hook<A extends unknown[], R> = ((this: PluginContext, ...args: A) => R | Promise<R>) | { order?: 'pre' | 'post' | null; handler: (this: PluginContext, ...args: A) => R | Promise<R> };

/**
 * A plugin written as an object (Rollup's hooks, Vite's `config`, `configResolved`, `transformIndexHtml`, `handleHotUpdate`), kept in this
 * process: the build calls its hooks across a local socket, so what goes in and out is JSON (bytes as `Buffer`), and a hook can keep state in
 * the plugin as usual. `generateBundle` and `writeBundle` see the files with their `code` and `source`, and what they change in those, or remove, is applied.
 */
export interface Plugin {
  name: string;
  enforce?: 'pre' | 'post';
  apply?: 'build' | 'serve';
  options?: Hook<[Record<string, unknown>], Record<string, unknown> | null | void>;
  buildStart?: Hook<[Record<string, unknown>], void>;
  resolveId?: Hook<[source: string, importer: string | undefined, options: { isEntry: boolean; kind?: string; custom?: Record<string, unknown> }], string | null | void | { id: string; external?: boolean | 'absolute' | 'relative'; moduleSideEffects?: boolean | 'no-treeshake' }>;
  load?: Hook<[id: string], string | null | void | { code: string; map?: unknown }>;
  transform?: Hook<[code: string, id: string], string | null | void | { code: string; map?: unknown }>;
  buildEnd?: Hook<[error?: Error], void>;
  renderChunk?: Hook<[code: string, chunk: Record<string, unknown>, options: Record<string, unknown>], string | null | void | { code: string; map?: unknown }>;
  generateBundle?: Hook<[options: Record<string, unknown>, bundle: Record<string, { type: string; fileName: string; code?: string; source?: string | Uint8Array }>, isWrite: boolean], void>;
  writeBundle?: Hook<[options: Record<string, unknown>, bundle: Record<string, { type: string; fileName: string; code?: string; source?: string | Uint8Array }>], void>;
  closeBundle?: Hook<[], void>;
  watchChange?: Hook<[id: string, change: { event: 'create' | 'update' | 'delete' }], void>;
  banner?: Hook<[chunk: Record<string, unknown>], string>;
  footer?: Hook<[chunk: Record<string, unknown>], string>;
  config?: Hook<[config: Record<string, unknown>, env: { command: string; mode: string }], Record<string, unknown> | null | void>;
  configResolved?: Hook<[config: Record<string, unknown>], void>;
  transformIndexHtml?: Hook<[html: string, context: Record<string, unknown>], string | { html: string; tags: unknown[] } | void>;
  handleHotUpdate?: Hook<[context: Record<string, unknown>], unknown[] | void>;
  [hook: string]: unknown;
}

/** A plugin made by a module, loaded where the build runs (so it has everything a config file's plugin has): the module's `export` (default `default`), called with `options` if it is a function. */
export interface PluginModule {
  module: string;
  export?: string;
  options?: unknown;
}

export interface Metafile {
  inputs: Record<string, { bytes: number; imports: Array<{ path: string; kind: string; external?: boolean }>; format?: 'esm' | 'cjs' }>;
  outputs: Record<string, { bytes: number; inputs: Record<string, { bytesInOutput: number }>; imports: Array<{ path: string; kind: string; external?: boolean }>; exports: string[]; entryPoint?: string }>;
}

export interface BuildResult {
  ok: true;
  status: 0;
  /** The files written: the `outfile`, the files of `outdir`, pages. */
  files: string[];
  /** The metafile, when `metafile` was asked for. */
  metafile?: Metafile;
  warnings: string[];
  errors: string[];
  stdout: string;
  stderr: string;
}

/** Thrown by {@link build} and {@link serve} when the program fails: the errors it reported are in `errors`. */
export class BuildError extends Error {
  status: number | null;
  errors: string[];
  warnings: string[];
  stdout: string;
  stderr: string;
}

/** Builds. Resolves when the program exits with 0; rejects with a {@link BuildError} otherwise. */
export function build(options: Options): Promise<BuildResult>;

export interface Server {
  /** Where it listens, `http://localhost:5173/`. */
  url: string;
  /** Stops the server and resolves when it has ended. */
  close(): Promise<void>;
}

/** Starts the development server: the build in memory with live reload. Resolves once it is listening. */
export function serve(options: Options): Promise<Server>;

export interface RunResult {
  status: number | null;
  signal: string | null;
  stdout: string;
  stderr: string;
}

/** Starts the program with the command line arguments `rkp` takes. A failed run is not an error: look at `status`. */
export function run(args: string[], options?: { cwd?: string; env?: Record<string, string>; signal?: AbortSignal }): Promise<RunResult>;

/** The path of the program for this platform (`RATHKIT_PACK_BIN` overrides it). Throws when there is none. */
export function binaryPath(): string;

/** The version of the program. */
export function version(): string;
