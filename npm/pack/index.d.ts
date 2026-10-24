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
  /** The names of copied files; `[name]`, `[hash]` and `[ext]` (no dot) are filled in. */
  assetNames?: string;
  /** What copied files' addresses start with. */
  publicPath?: string;
  jsx?: 'automatic' | 'classic';
  jsxImportSource?: string;
  jsxFactory?: string;
  jsxFragment?: string;
  decoratorMetadata?: boolean;
  experimentalDecorators?: boolean;
  strictNullChecks?: boolean;
  /** A `tsconfig.json` to use, or `false` for none. */
  tsconfig?: string | false;
  /** A config file that lists plugins (`rathkit.config.mjs`), or `false` to ignore the one next to the project. */
  config?: string | false;
  /** Passed to plugins as the mode (`production`, `development`). */
  mode?: string;
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

export interface BuildResult {
  ok: true;
  status: 0;
  /** The files written: the `outfile`, the files of `outdir`, pages. */
  files: string[];
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
