//! The module graph: every file reachable from the entry points, parsed and linked to the
//! files it imports.
//!
//! Discovery runs on a pool of threads, each taking a file from a shared queue, reading and
//! parsing it, resolving what it imports, and queueing the files it has not seen. Threads
//! finish in an unpredictable order, so once discovery is done the modules are renumbered by
//! a depth-first walk from the entries: the same project always produces the same graph.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use crate::json::Json;
use crate::loader::{Asset, Loader};
use crate::module::{JsxOptions, ModuleRecord, Request, RequestKind};
use crate::parser::{ParseError, ParseOptions, Parsed, parse};
use crate::resolve::{FileSystem, Package, RealFs, ResolveKind, ResolveOptions, Resolver, normalize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(u32);

impl ModuleId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// How a file's code is organized, which decides how the linker treats it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    /// Uses `import`/`export`.
    Esm,
    /// A script that may use `require` and `module.exports`.
    CommonJs,
    /// A `.json` file: one value, exported as default.
    Json,
}

/// Where a request leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edge {
    Module(ModuleId),
    /// Left for the runtime to load: a Node built-in or a configured external.
    External(String),
    /// Could not be resolved; a diagnostic says why.
    Unresolved,
}

pub struct Module {
    pub id: ModuleId,
    pub path: PathBuf,
    pub source: String,
    pub kind: ModuleKind,
    /// `None` for JSON.
    pub parsed: Option<Parsed>,
    pub record: ModuleRecord,
    /// One per entry of `record.requests`.
    pub edges: Vec<Edge>,
    pub package: Option<Arc<Package>>,
    /// Whether the file may have effects when imported but unused (`sideEffects` in its
    /// `package.json`).
    pub side_effects: bool,
    /// A file copied to the output (the `file` loader).
    pub asset: Option<Asset>,
    /// A style sheet, whose text goes to a `.css` file; the module's own source is its JavaScript side.
    pub css: Option<CssSheet>,
    /// Modification time and size of the file when it was read, for builds that reuse modules (watching).
    pub stamp: Option<Stamp>,
}

/// A file's modification time and size.
pub type Stamp = (std::time::SystemTime, u64);

pub fn file_stamp(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()))
}

/// Modules from the last build, kept for the next: one whose file has not changed is not read, parsed or
/// scanned again; only its imports are resolved again.
#[derive(Default)]
pub struct ModuleCache {
    modules: Mutex<HashMap<PathBuf, Module>>,
}

impl ModuleCache {
    fn take(&self, path: &Path, stamp: Stamp) -> Option<Module> {
        let mut modules = self.modules.lock().unwrap();
        match modules.get(path) {
            Some(m) if m.stamp == Some(stamp) => modules.remove(path),
            _ => None,
        }
    }

    /// Keeps the modules of a finished build (those read from disk without plugins).
    pub fn store(&self, graph: Graph) {
        let mut modules = self.modules.lock().unwrap();
        modules.clear();
        for module in graph.modules {
            if module.stamp.is_some() {
                modules.insert(module.path.clone(), module);
            }
        }
    }

    pub fn clear(&self) {
        self.modules.lock().unwrap().clear();
    }
}

/// The style sheet side of a module.
pub struct CssSheet {
    pub text: String,
    pub parsed: crate::css::Parsed,
    /// Class names are local to the sheet.
    pub local: bool,
    /// Mixed into local names so sheets with the same class name do not collide.
    pub seed: String,
}

impl Module {
    /// Where a syntax node of this module starts, as a byte offset into `source`.
    pub fn ast_pos(&self, node: crate::ast::NodeId) -> u32 {
        self.parsed.as_ref().map_or(0, |p| p.ast[node].span.start)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    /// Does not stop the build: an unresolvable `require` inside a `try` is a common
    /// optional-dependency pattern.
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub file: Option<PathBuf>,
    /// Byte offset in `file`.
    pub pos: Option<u32>,
}

pub struct Graph {
    pub jsx: JsxOptions,
    pub decorator_metadata: bool,
    /// Decorators are TypeScript's legacy kind (`experimentalDecorators`), not TC39's.
    pub experimental_decorators: bool,
    /// Expressions replaced by constants, and the branches that follow from them.
    pub defines: crate::define::Defines,
    /// Indexed by `ModuleId`. In depth-first order from the entries.
    pub modules: Vec<Module>,
    pub entries: Vec<ModuleId>,
    pub diagnostics: Vec<Diagnostic>,
    /// `--pure`: names whose calls are free of effects.
    pub pure_calls: Vec<String>,
    /// Directories read for `import.meta.glob`: a file added to one changes the build.
    pub watch_dirs: Vec<PathBuf>,
}

impl Graph {
    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.index()]
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

#[derive(Clone)]
pub struct GraphOptions {
    /// Plugins (Rollup, Vite, esbuild, webpack loaders) run by a Node process.
    pub plugins: Option<Arc<crate::plugins::PluginHost>>,
    pub resolve: ResolveOptions,
    pub jsx: JsxOptions,
    /// Emit the `design:*` metadata that dependency injection frameworks read.
    pub decorator_metadata: bool,
    /// Use TypeScript's legacy decorators instead of the standard ones.
    pub experimental_decorators: bool,
    pub strict_null_checks: bool,
    /// Loaders chosen by extension (with the dot, lowercase) before the defaults.
    pub loaders: Vec<(String, Loader)>,
    /// Names of copied files: `[name]`, `[hash]` and `[ext]` are filled in.
    pub asset_names: String,
    /// What copied files' addresses start with.
    pub public_path: String,
    /// `--define`: expressions replaced by constants.
    pub defines: crate::define::Defines,
    /// `--pure`: names whose calls are free of effects (`styled.div`), as if each carried `/* @__PURE__ */`.
    pub pure_calls: Vec<String>,
    /// `--inject`: files whose exports stand in for the globals of the same names, in every module that uses one.
    pub inject: Vec<Inject>,
    /// Specifiers left out of the bundle: exact names, or a prefix ending in `/*`.
    pub external: Vec<String>,
    /// Treat Node's built-in modules (`fs`, `node:path`, ...) as external.
    pub node_builtins: bool,
    /// Leave every package import (a name that is not a path, nor made into one by an alias) out of the bundle.
    pub packages_external: bool,
    /// Worker threads; 0 means one per core.
    pub threads: usize,
    /// Modules of the previous build to reuse when their files have not changed.
    pub cache: Option<Arc<ModuleCache>>,
    /// Module federation: remotes and shared packages are not bundled in the usual way.
    pub federation: Option<Arc<crate::federation::Federation>>,
    /// Builds a script on its own as an ES module (for `new Worker(new URL(...))` and `?worker`).
    pub worker_builder: Option<Arc<WorkerBuilder>>,
}

/// A script built alone: its output, or what went wrong.
pub type WorkerBuilder = dyn Fn(&Path) -> Result<Vec<u8>, String> + Send + Sync;

impl Default for GraphOptions {
    fn default() -> Self {
        GraphOptions { plugins: None, resolve: ResolveOptions::default(), jsx: JsxOptions::default(), decorator_metadata: false, experimental_decorators: false, strict_null_checks: false, loaders: Vec::new(), asset_names: "[name]-[hash]".to_string(), public_path: "./".to_string(), defines: Default::default(), pure_calls: Vec::new(), inject: Vec::new(), external: Vec::new(), node_builtins: true, packages_external: false, threads: 0, cache: None, federation: None, worker_builder: None }
    }
}

/// Builds the graph for `entries`, each a path relative to `cwd` or absolute.
pub fn build(cwd: &Path, entries: &[String], options: &GraphOptions) -> Graph {
    match &options.federation {
        // The modules the runtime needs exist only in memory, on top of the real files.
        Some(federation) => build_with_fs(Box::new(crate::federation::Overlay { federation: federation.clone(), inner: Box::new(RealFs) }), cwd, entries, options),
        None => build_with_fs(Box::new(RealFs), cwd, entries, options),
    }
}

pub fn build_with_fs(fs: Box<dyn FileSystem>, cwd: &Path, entries: &[String], options: &GraphOptions) -> Graph {
    let resolver = Arc::new(Resolver::with_fs(fs, options.resolve.clone()));
    // The plugin host asks for the bundler's own resolution (`this.resolve`, and when no plugin claims a request).
    if let Some(host) = &options.plugins {
        let shared_resolver = resolver.clone();
        let (external, builtins) = (options.external.clone(), options.node_builtins);
        let root = cwd.to_path_buf();
        host.set_resolver(Some(Arc::new(move |source: &str, importer: Option<&str>, kind: &str| {
            if (builtins && is_node_builtin(source)) || external.iter().any(|e| e == source) {
                return Some((source.to_string(), true));
            }
            let dir = importer.map(Path::new).and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_else(|| root.clone());
            let kind = if matches!(kind, "require-call" | "require-resolve") { ResolveKind::Require } else { ResolveKind::Import };
            shared_resolver.resolve(&dir, source, kind).ok().map(|found| (found.path.to_string_lossy().into_owned(), false))
        })));
    }
    let mut diagnostics = Vec::new();
    let mut shared = Shared::default();
    let mut entry_slots = Vec::new();
    for entry in entries {
        // Plugins first: an entry may be a virtual module.
        if let Some(host) = &options.plugins {
            match host.resolve_id(entry, None, true, "import-statement") {
                Ok(Some(found)) if !found.external => {
                    entry_slots.push(shared.register(PathBuf::from(found.id)));
                    continue;
                }
                Ok(_) => {}
                Err(message) => {
                    diagnostics.push(Diagnostic { severity: Severity::Error, message: format!("Entry point: {message}"), file: None, pos: None });
                    continue;
                }
            }
        }
        // `.hidden/x.js` is a path, not a relative specifier: only `./` and `../` are.
        let relative = entry == "." || entry == ".." || entry.starts_with("./") || entry.starts_with("../");
        let specifier = if relative || Path::new(entry).is_absolute() { entry.clone() } else { format!("./{entry}") };
        match resolver.resolve(cwd, &specifier, ResolveKind::Import) {
            Ok(found) => entry_slots.push(shared.register(found.path)),
            Err(e) => diagnostics.push(Diagnostic { severity: Severity::Error, message: format!("Entry point: {e}"), file: None, pos: None }),
        }
    }
    shared.diagnostics = diagnostics;
    for &slot in &entry_slots {
        let path = shared.paths[slot].clone();
        shared.entry_paths.insert(path);
    }
    let threads = if options.threads == 0 { std::thread::available_parallelism().map_or(4, |n| n.get()) } else { options.threads };
    let state = (Mutex::new(shared), Condvar::new());
    let context = Context { cwd, resolver: &resolver, options, state: &state };
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| context.work());
        }
    });
    let shared = state.0.into_inner().unwrap();
    let mut graph = finish(shared, &entry_slots);
    graph.jsx = options.jsx.clone();
    graph.decorator_metadata = options.decorator_metadata;
    graph.experimental_decorators = options.experimental_decorators;
    graph.defines = options.defines.clone();
    graph.pure_calls = options.pure_calls.clone();
    graph
}

#[derive(Default)]
struct Shared {
    /// Slots in discovery order; a slot is filled once its file is processed.
    modules: Vec<Option<Module>>,
    paths: Vec<PathBuf>,
    by_path: HashMap<PathBuf, usize>,
    queue: Vec<usize>,
    /// Files queued or being processed. Discovery is done when this reaches zero.
    pending: usize,
    /// Files met as `new URL("./x", import.meta.url)`: copied files whatever their extension.
    as_assets: std::collections::HashSet<PathBuf>,
    /// The entry points, for plugins that are told of a module as it is read.
    entry_paths: std::collections::HashSet<PathBuf>,
    diagnostics: Vec<Diagnostic>,
    watch_dirs: Vec<PathBuf>,
}

impl Shared {
    /// Returns the slot for `path`, queueing the file the first time it is seen.
    fn register(&mut self, path: PathBuf) -> usize {
        if let Some(&slot) = self.by_path.get(&path) {
            return slot;
        }
        let slot = self.modules.len();
        self.modules.push(None);
        self.paths.push(path.clone());
        self.by_path.insert(path, slot);
        self.queue.push(slot);
        self.pending += 1;
        slot
    }
}

struct Context<'a> {
    cwd: &'a Path,
    resolver: &'a Resolver,
    options: &'a GraphOptions,
    state: &'a (Mutex<Shared>, Condvar),
}

/// A processed file whose edges still name paths rather than slots.
struct Processed {
    module: Module,
    targets: Vec<Target>,
    diagnostics: Vec<Diagnostic>,
}

enum Target {
    Path(PathBuf),
    /// A file to copy, found by `new URL(...)`.
    Asset(PathBuf),
    External(String),
    Unresolved,
}

impl Context<'_> {
    fn work(&self) {
        let (lock, wake) = self.state;
        loop {
            let (slot, path) = {
                let mut shared = lock.lock().unwrap();
                loop {
                    if let Some(slot) = shared.queue.pop() {
                        break (slot, shared.paths[slot].clone());
                    }
                    if shared.pending == 0 {
                        return;
                    }
                    shared = wake.wait(shared).unwrap();
                }
            };
            let processed = self.process(slot, &path);
            // Plugins hear of a module as soon as it is read, with what it imports.
            if let (Ok(done), Some(host)) = (&processed, &self.options.plugins) {
                if host.has("moduleParsed") {
                    let name = |t: &Target| match t {
                        Target::Path(p) | Target::Asset(p) => Some(p.to_string_lossy().into_owned()),
                        Target::External(spec) => Some(spec.clone()),
                        Target::Unresolved => None,
                    };
                    let (mut imported, mut dynamic) = (Vec::new(), Vec::new());
                    for (request, target) in done.module.record.requests.iter().zip(&done.targets) {
                        if let Some(id) = name(target) {
                            let list = if request.kind == RequestKind::Dynamic { &mut dynamic } else { &mut imported };
                            if !list.contains(&id) {
                                list.push(id);
                            }
                        }
                    }
                    let exports: Vec<String> = done.module.record.exports.iter().map(|e| e.exported.clone()).collect();
                    let entry = self.state.0.lock().unwrap().entry_paths.contains(&done.module.path);
                    host.module_parsed(&done.module.path.to_string_lossy(), &imported, &dynamic, done.module.kind == ModuleKind::Esm, done.module.side_effects, &exports, entry);
                }
            }
            let mut shared = lock.lock().unwrap();
            match processed {
                Ok(Processed { mut module, targets, diagnostics }) => {
                    module.edges = targets
                        .into_iter()
                        .map(|t| match t {
                            Target::Path(p) => Edge::Module(ModuleId(shared.register(p) as u32)),
                            Target::Asset(p) => {
                                shared.as_assets.insert(p.clone());
                                Edge::Module(ModuleId(shared.register(p) as u32))
                            }
                            Target::External(name) => Edge::External(name),
                            Target::Unresolved => Edge::Unresolved,
                        })
                        .collect();
                    shared.diagnostics.extend(diagnostics);
                    shared.modules[slot] = Some(module);
                }
                Err(diagnostic) => shared.diagnostics.push(diagnostic),
            }
            shared.pending -= 1;
            // New work, or the last job done: either way sleepers need to look again.
            wake.notify_all();
        }
    }

    fn process(&self, slot: usize, module_path: &Path) -> Result<Processed, Diagnostic> {
        // `./logo.svg?raw`: the file is the part before the `?`; the whole name is what plugins and the output know the module by.
        let (file_path, query) = split_query(module_path);
        let path: &Path = &file_path;
        let error = |message: String, pos: Option<u32>| Diagnostic { severity: Severity::Error, message, file: Some(module_path.to_path_buf()), pos };
        let mut diagnostics = Vec::new();
        let package = self.resolver.package_for(path).unwrap_or_else(|e| {
            // The same broken file is met once per module under it; the sort in `finish`
            // puts the repeats side by side so they can be dropped.
            diagnostics.push(Diagnostic { severity: Severity::Warning, message: format!("{e}"), file: None, pos: None });
            None
        });
        // Without plugins a module is a function of its file: one that has not changed is taken from the last build.
        let stamp = if self.options.plugins.is_none() && self.options.cache.is_some() && query.is_empty() { file_stamp(path) } else { None };
        if let (Some(cache), Some(stamp)) = (&self.options.cache, stamp) {
            if let Some(mut module) = cache.take(path, stamp) {
                let dir = path.parent().unwrap_or(Path::new("/"));
                let mut targets = Vec::with_capacity(module.record.requests.len());
                for request in &module.record.requests {
                    targets.push(self.resolve_request(dir, path, request, module.css.is_some(), &mut diagnostics));
                }
                module.id = ModuleId(slot as u32);
                module.edges = Vec::new();
                module.package = package;
                return Ok(Processed { module, targets, diagnostics });
            }
        }
        let id = module_path.to_string_lossy().into_owned();
        let plugin_error = |message: String| error(message, None);
        // What plugins make of the module: `load` replaces reading the file, `transform` changes the text.
        let mut plugin_text: Option<String> = None;
        let mut plugin_bytes: Option<Vec<u8>> = None;
        let mut module_type: Option<String> = None;
        let mut map: Option<String> = None;
        let mut resolve_dir: Option<PathBuf> = None;
        let mut plugin_side_effects: Option<bool> = None;
        let mut js_face: Option<String> = None;
        // `./w.js?worker` is a constructor for a worker; the script itself is built on its own and copied like an asset.
        let worker = worker_query(&query);
        let file_name_override = matches!(worker, Some(WorkerQuery::Bundled)).then(|| format!("{}.js", path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        if let Some(kind) = &worker {
            let build = |what: &str| -> Result<Vec<u8>, Diagnostic> {
                let builder = self.options.worker_builder.as_ref().ok_or_else(|| error(format!("{what}: workers cannot be built here"), None))?;
                builder(path).map_err(|m| error(m, None))
            };
            match kind {
                WorkerQuery::Bundled => plugin_bytes = Some(build("worker")?),
                WorkerQuery::Wrapper { shared, inline, url } => {
                    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let code = if *url {
                        format!("export default new URL({}, import.meta.url).href;\n", quote(&format!("./{file}?rk-worker")))
                    } else {
                        let class = if *shared { "SharedWorker" } else { "Worker" };
                        if *inline {
                            let script = String::from_utf8_lossy(&build("worker")?).into_owned();
                            format!("var code = {};\nexport default function WorkerWrapper(options) {{\n  var url = URL.createObjectURL(new Blob([code], {{ type: \"text/javascript\" }}));\n  try {{ return new {class}(url, {{ type: \"module\", name: options && options.name }}); }} finally {{ URL.revokeObjectURL(url); }}\n}}\n", quote(&script))
                        } else {
                            format!("export default function WorkerWrapper(options) {{\n  return new {class}(new URL({}, import.meta.url), {{ type: \"module\", name: options && options.name }});\n}}\n", quote(&format!("./{file}")))
                        }
                    };
                    plugin_text = Some(code);
                    module_type = Some("js".to_string());
                }
            }
        }
        // `./a.wasm?init` (Vite's): a function that instantiates the module. The file is copied like any `new URL("./a.wasm", import.meta.url)`.
        let wasm_init = worker.is_none() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wasm")) && query.split('&').any(|q| q == "init");
        if wasm_init {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            plugin_text = Some(wasm_init_module(&quote(&format!("./{file}"))));
            module_type = Some("js".to_string());
        }
        let host = self.options.plugins.as_ref().filter(|_| worker.is_none());
        if let Some(host) = host {
            if let Some(loaded) = host.load(&id).map_err(plugin_error)? {
                plugin_text = loaded.code;
                plugin_bytes = loaded.bytes;
                module_type = loaded.module_type;
                map = loaded.map;
                resolve_dir = loaded.resolve_dir.map(PathBuf::from);
                plugin_side_effects = loaded.side_effects;
                js_face = loaded.js_face;
            }
        }
        let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
        let forced_asset = self.state.0.lock().unwrap().as_assets.contains(path);
        let ext_loader = match &worker {
            Some(WorkerQuery::Bundled) => Some(Loader::File),
            Some(WorkerQuery::Wrapper { .. }) => None,
            None if wasm_init => None,
            None => query_loader(&query).or_else(|| self.loader_for(path)).or(forced_asset.then_some(Loader::File)),
        };
        let mut by_plugin = plugin_text.is_some() || plugin_bytes.is_some();
        if let Some(host) = host.filter(|h| h.has("transform")) {
            // Plugins see text: the file's, unless a loader reads it as bytes.
            let base = match (&plugin_text, &plugin_bytes) {
                (Some(text), _) => Some(text.clone()),
                (None, Some(_)) => None,
                // A file read as bytes (an image, an SVG) is shown to plugins when it is text, as Rollup does.
                (None, None) if ext_loader.is_some_and(|l| l.needs_bytes()) => self.resolver.read(path).ok().and_then(|bytes| String::from_utf8(bytes).ok()),
                (None, None) => Some(self.resolver.read_to_string(path).map_err(|e| error(format!("Cannot read file: {e}"), None))?),
            };
            if let Some(base) = base {
                if let Some(done) = host.transform(&base, &id, module_type.as_deref()).map_err(plugin_error)? {
                    plugin_text = Some(done.code);
                    plugin_bytes = None;
                    if done.map.is_some() {
                        map = done.map;
                    }
                    if done.module_type.is_some() {
                        module_type = done.module_type;
                    }
                    by_plugin = true;
                } else if plugin_text.is_none() {
                    plugin_text = Some(base);
                }
            }
        }
        // What kind of module the text is: a plugin's word first, then the extension. A file a plugin
        // rewrote may no longer be what its extension says (`.json` made into JavaScript).
        #[derive(Clone, Copy)]
        enum Kind {
            Js(Option<&'static str>),
            Json,
            Loader(Loader),
        }
        let from_type = module_type.as_deref().and_then(|t| match t {
            "js" | "mjs" | "cjs" => Some(Kind::Js(None)),
            "jsx" => Some(Kind::Js(Some("jsx"))),
            "ts" => Some(Kind::Js(Some("ts"))),
            "tsx" => Some(Kind::Js(Some("tsx"))),
            "json" => Some(Kind::Json),
            "copy" => Some(Kind::Loader(Loader::File)),
            other => Loader::parse(other).map(Kind::Loader),
        });
        let looks_like_script = |t: &str| {
            let head = t.trim_start();
            ["import ", "import{", "export ", "export{", "var ", "const ", "let ", "function", "/*#__PURE__*/", "\"use ", "'use ", "(function", "module.exports"].iter().any(|k| head.starts_with(k)) && crate::parser::parse(t, ParseOptions::new(true)).is_ok()
        };
        let looks_like_json = |text: &str| Json::parse(text).is_ok() && text.trim_start().starts_with(['{', '[', '"']);
        let kind_of = |text: Option<&str>| -> Kind {
            if let Some(kind) = from_type {
                return kind;
            }
            match (ext_loader, extension.as_deref()) {
                (Some(_), _) if by_plugin && text.is_some_and(looks_like_script) => Kind::Js(None),
                (Some(l), _) => Kind::Loader(l),
                (None, Some("json")) if by_plugin && text.is_some_and(|t| !looks_like_json(t)) => Kind::Js(None),
                (None, Some("json")) => Kind::Json,
                (None, Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" | "cjs" | "cts")) => Kind::Js(None),
                // A plugin made JavaScript of a file type the bundler does not know (.vue, .svelte).
                (None, _) if by_plugin => Kind::Js(None),
                (None, _) => Kind::Js(None),
            }
        };
        let kind_now = kind_of(plugin_text.as_deref());
        if from_type.is_none() && ext_loader.is_none() && !by_plugin && !matches!(extension.as_deref(), Some("json" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" | "cjs" | "cts")) {
            let ext = extension.as_deref().map_or(String::new(), |e| format!(".{e}"));
            return Err(error(format!("No loader for '{ext}' files"), None));
        }
        let (loader, force_js, syntax_hint, as_json) = match kind_now {
            Kind::Loader(l) => (Some(l), false, None, false),
            Kind::Json => (None, false, None, true),
            Kind::Js(hint) => (None, true, hint, false),
        };
        let mut asset = None;
        // A module that lists files is not kept between builds: a file added later changes it.
        let mut used_glob = false;
        let mut css = None;
        let mut css_requests: Vec<Request> = Vec::new();
        let mut source = match loader {
            Some(loader) => {
                let bytes = match (plugin_bytes.take(), plugin_text.take()) {
                    (Some(bytes), _) => bytes,
                    (None, Some(text)) => text.into_bytes(),
                    (None, None) => self.resolver.read(path).map_err(|e| error(format!("Cannot read file: {e}"), None))?,
                };
                let file_name = file_name_override.clone().unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                if loader.is_css() {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let local = loader == Loader::LocalCss;
                    let parsed = crate::css::parse(&text, local).map_err(|e| error(format!("{e}"), None))?;
                    let seed = self.css_seed(path);
                    let js = if let Some(face) = js_face.take() {
                        face
                    } else if local {
                        // Class names composed from another sheet carry that sheet's seed.
                        let mut replacements = vec![crate::css::Replacement::Leave; parsed.refs.len()];
                        for (r, slot) in parsed.refs.iter().zip(replacements.iter_mut()) {
                            if r.kind != crate::css::RefKind::Compose {
                                continue;
                            }
                            let request = Request { specifier: r.specifier.clone(), kind: RequestKind::Static, attrs: Vec::new(), span: crate::ast::Span { start: 0, end: 0 } };
                            if let Target::Path(other) = self.resolve_request(path.parent().unwrap_or(Path::new("/")), path, &request, true, &mut diagnostics) {
                                *slot = crate::css::Replacement::Seed(self.css_seed(&other));
                            }
                        }
                        crate::loader::local_names_module(&crate::css::render(&text, &parsed, &replacements, Some(&seed), false).exports)
                    } else {
                        crate::loader::synthesize(loader, &[], &file_name, None)
                    };
                    for r in &parsed.refs {
                        if !css_requests.iter().any(|q| q.specifier == r.specifier) {
                            css_requests.push(Request { specifier: r.specifier.clone(), kind: RequestKind::Static, attrs: Vec::new(), span: crate::ast::Span { start: 0, end: 0 } });
                        }
                    }
                    css = Some(CssSheet { text, parsed, local, seed });
                    js
                } else if loader == Loader::File {
                    let name = crate::loader::asset_name(&self.options.asset_names, &file_name, &bytes);
                    let url = format!("{}{}", self.options.public_path, name);
                    let js = crate::loader::synthesize(loader, &bytes, &file_name, Some(&url));
                    asset = Some(Asset { name, url, data: Arc::new(bytes), original: path.to_string_lossy().into_owned() });
                    js
                } else {
                    crate::loader::synthesize(loader, &bytes, &file_name, None)
                }
            }
            None => {
                let mut text = match (plugin_text.take(), plugin_bytes.take()) {
                    (Some(text), _) => text,
                    (None, Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                    (None, None) => self.resolver.read_to_string(path).map_err(|e| error(format!("Cannot read file: {e}"), None))?,
                };
                // `import(\`./dir/${name}.js\`)` becomes a choice among imports of the files that fit.
                if text.contains("import(") || text.contains("import (") {
                    let mut dirs = Vec::new();
                    let mut warnings = Vec::new();
                    let rewritten = crate::glob::rewrite_dynamic_imports(&text, path, self.cwd, self.resolver.file_system(), &mut dirs, &mut warnings);
                    self.state.0.lock().unwrap().watch_dirs.extend(dirs);
                    diagnostics.extend(warnings.into_iter().map(|message| Diagnostic { severity: Severity::Warning, message, file: Some(path.to_path_buf()), pos: None }));
                    if let Some(rewritten) = rewritten {
                        text = rewritten;
                        used_glob = true;
                    }
                }
                // `import.meta.glob(...)` becomes the imports of the files it names.
                if text.contains("import.meta.glob") {
                    let mut dirs = Vec::new();
                    let rewritten = crate::glob::rewrite(&text, path, self.cwd, self.resolver.file_system(), &mut dirs).map_err(|m| error(m, None))?;
                    self.state.0.lock().unwrap().watch_dirs.extend(dirs);
                    if let Some(rewritten) = rewritten {
                        text = rewritten;
                        used_glob = true;
                    }
                }
                // A map a plugin made points back to what it read; the maps this bundler builds
                // follow it when it sits at the end of the text, as a file's own would.
                if let Some(map) = map.take().filter(|_| !as_json) {
                    text.push_str("\n//# sourceMappingURL=data:application/json;base64,");
                    text.push_str(&crate::loader::base64(map.as_bytes()));
                    text.push('\n');
                }
                text
            }
        };
        let mode = if loader.is_some() {
            Some(Mode::Module)
        } else if as_json {
            Json::parse(&source).map_err(|e| error(format!("Invalid JSON: {e}"), Some(e.pos as u32)))?;
            None
        } else if force_js && by_plugin {
            // The text came from a plugin: module syntax unless the extension says it is a script.
            match extension.as_deref() {
                Some("mjs" | "mts") => Some(Mode::Module),
                // A plugin may have turned a CommonJS file into an ES module (or the other way), so the text decides.
                _ => Some(Mode::Auto),
            }
        } else {
            match extension.as_deref() {
                Some("mjs" | "mts") => Some(Mode::Module),
                Some("cjs" | "cts") => Some(Mode::Script),
                _ => Some(match package.as_deref().and_then(|p| p.field("type")).and_then(Json::as_str) {
                    Some("module") => Mode::Module,
                    // `"type": "commonjs"` is what `npm init` writes; the code may still use `import`, as bundlers allow.
                    Some("commonjs") => Mode::Auto,
                    _ => Mode::Auto,
                }),
            }
        };
        let (kind, parsed, record) = match mode {
            None => (ModuleKind::Json, None, ModuleRecord::default()),
            Some(mode) => {
                let mut options = syntax_for(path);
                match syntax_hint {
                    Some("ts") => options = options.typescript(),
                    Some("jsx") => options = options.with_jsx(),
                    Some("tsx") => options = options.typescript().with_jsx(),
                    _ => {}
                }
                options.decorator_metadata = self.options.decorator_metadata;
                options.strict_null_checks = self.options.strict_null_checks;
                let (mut parsed, mut as_module) = parse_file(&source, mode, options).map_err(|e| error(e.message.clone(), Some(e.pos)))?;
                // `--inject`: a global the module uses that a file exports is imported from it, written on the module's first line so no line moves.
                if !self.options.inject.is_empty() {
                    let mut wanted: Vec<(&Path, Vec<&str>)> = Vec::new();
                    for inject in &self.options.inject {
                        if inject.path == path {
                            continue;
                        }
                        let used: Vec<&str> = inject.names.iter().map(String::as_str).filter(|n| parsed.semantic.unresolved().iter().any(|&id| matches!(parsed.ast.node(id).kind, crate::ast::NodeKind::Ident(a) if parsed.ast.atom(a) == *n))).collect();
                        if !used.is_empty() {
                            wanted.push((&inject.path, used));
                        }
                    }
                    if !wanted.is_empty() {
                        let lines: Vec<String> = wanted
                            .iter()
                            .map(|(file, names)| {
                                let spec = Json::String(file.to_string_lossy().replace('\\', "/")).to_string();
                                if as_module { format!("import {{ {} }} from {spec};", names.join(", ")) } else { format!("var {{ {} }} = require({spec});", names.join(", ")) }
                            })
                            .collect();
                        let prefix = lines.join(" ");
                        // A `#!` line stays the first line.
                        let at = if source.starts_with("#!") { source.find('\n').map_or(source.len(), |n| n + 1) } else { 0 };
                        source.insert_str(at, &format!("{prefix} "));
                        (parsed, as_module) = parse_file(&source, mode, options).map_err(|e| error(e.message.clone(), Some(e.pos)))?;
                    }
                }
                let mut record = ModuleRecord::build_with(&parsed, &self.options.jsx, &self.options.defines);
                record.requests.extend(css_requests);
                // Whatever needed module rules to parse is an ES module, import or export or not.
                // `export =` and `import x = require()` are CommonJS written with TypeScript syntax.
                let kind = if as_module && !parsed.ast.ts_commonjs { ModuleKind::Esm } else { ModuleKind::CommonJs };
                (kind, Some(parsed), record)
            }
        };
        // Imports in a module a plugin made up are relative to where the plugin says, else to the project.
        let virtual_dir = if resolve_dir.is_none() && path.is_absolute() && !id.starts_with('\0') { None } else { Some(resolve_dir.clone().unwrap_or_else(|| self.cwd.to_path_buf())) };
        let dir = virtual_dir.as_deref().unwrap_or_else(|| path.parent().unwrap_or(Path::new("/")));
        let mut targets = Vec::with_capacity(record.requests.len());
        for request in &record.requests {
            targets.push(self.resolve_request(dir, path, request, css.is_some() || false, &mut diagnostics));
        }
        let side_effects = plugin_side_effects.unwrap_or_else(|| match loader {
            // A value file has no effect of its own: it goes when nothing imports its value. Style sheets always apply.
            Some(l) => l.is_css(),
            None => package.as_ref().is_none_or(|p| p.file_has_side_effects(path)),
        });
        let stamp = if used_glob { None } else { stamp };
        let module = Module { id: ModuleId(slot as u32), path: module_path.to_path_buf(), source, kind, parsed, record, edges: Vec::new(), package, side_effects, asset, css, stamp };
        Ok(Processed { module, targets, diagnostics })
    }

    /// The loader for a file that is not JavaScript or JSON: an option for its extension, else the default.
    fn loader_for(&self, path: &Path) -> Option<Loader> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        let ext = name.rsplit_once('.').map(|(_, e)| format!(".{e}"));
        if let Some(ext) = ext {
            if let Some((_, loader)) = self.options.loaders.iter().rev().find(|(e, _)| *e == ext) {
                return Some(*loader);
            }
        }
        Loader::by_default(&name)
    }

    /// `relative_first`: a bare name in a style sheet is a path next to it before it is a package.
    fn resolve_request(&self, dir: &Path, file: &Path, request: &Request, relative_first: bool, diagnostics: &mut Vec<Diagnostic>) -> Target {
        if request.kind == RequestKind::WorkerUrl {
            // The script is built on its own: the module is the file with a mark that says so.
            let mut plain = request.clone();
            plain.specifier = request.specifier.trim_end_matches("?rk-worker").to_string();
            return match self.resolve_request_inner(dir, file, &plain, relative_first, diagnostics) {
                Target::Path(p) => Target::Asset(PathBuf::from(format!("{}?rk-worker", p.display()))),
                Target::External(_) => Target::Unresolved,
                other => other,
            };
        }
        let target = self.resolve_request_inner(dir, file, request, relative_first, diagnostics);
        if request.kind != RequestKind::AssetUrl {
            return target;
        }
        // A script is not a copied file; `new URL("./worker.js", import.meta.url)` stays as written.
        match target {
            Target::Path(p) if p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "mts" | "cts" | "json")) => Target::Unresolved,
            Target::Path(p) => Target::Asset(p),
            Target::External(_) => Target::Unresolved,
            other => other,
        }
    }

    fn resolve_request_inner(&self, dir: &Path, file: &Path, request: &Request, relative_first: bool, diagnostics: &mut Vec<Diagnostic>) -> Target {
        let specifier = request.specifier.as_str();
        if let Some(federation) = &self.options.federation {
            match federation.intercept(specifier, request.kind) {
                Some(crate::federation::Intercept::Module(path)) => return Target::Path(path),
                Some(crate::federation::Intercept::Dynamic) => return Target::External(specifier.to_string()),
                None => {}
            }
        }
        if self.is_external(specifier) {
            return Target::External(specifier.to_string());
        }
        let kind = if matches!(request.kind, RequestKind::Require | RequestKind::Resolve) { ResolveKind::Require } else { ResolveKind::Import };
        if let Some(host) = &self.options.plugins {
            let plugin_kind = match request.kind {
                RequestKind::Static => "import-statement",
                RequestKind::Dynamic => "dynamic-import",
                RequestKind::Require => "require-call",
                RequestKind::Resolve => "require-resolve",
                RequestKind::AssetUrl | RequestKind::WorkerUrl => "import-statement",
            };
            match host.resolve_id(specifier, Some(&file.to_string_lossy()), false, plugin_kind) {
                Ok(Some(found)) if found.external => return Target::External(found.id),
                Ok(Some(found)) => return Target::Path(PathBuf::from(found.id)),
                Ok(None) => {}
                Err(message) => {
                    diagnostics.push(Diagnostic { severity: Severity::Error, message, file: Some(file.to_path_buf()), pos: Some(request.span.start) });
                    return Target::Unresolved;
                }
            }
        }
        // In a style sheet `/fonts/a.woff2` is a file of the project (its root, then `public/`) before it is a place on the disk;
        // one that is nowhere stays as written, to be found when the page loads.
        if relative_first && specifier.starts_with('/') && !specifier.starts_with("//") {
            let inside = specifier.trim_start_matches('/');
            for base in [self.cwd.to_path_buf(), self.cwd.join("public")] {
                let candidate = base.join(inside);
                if self.resolver.file_system().is_file(&candidate) {
                    return Target::Path(candidate);
                }
            }
            if !self.resolver.file_system().is_file(Path::new(specifier)) {
                return Target::Unresolved;
            }
        }
        if relative_first && !specifier.starts_with('.') && !specifier.starts_with('/') {
            if let Ok(found) = self.resolver.resolve(dir, &format!("./{specifier}"), kind) {
                return Target::Path(found.path);
            }
        }
        // `./logo.svg?raw` and `./icon.svg?react`: the file is looked for without the query, which stays on the module's name.
        if let Some((bare, query)) = specifier.split_once('?') {
            if !bare.is_empty() && !query.contains(['/', '\\']) && request.kind != RequestKind::Resolve {
                if let Ok(found) = self.resolver.resolve(dir, bare, kind) {
                    return Target::Path(PathBuf::from(format!("{}?{query}", found.path.to_string_lossy())));
                }
            }
        }
        match self.resolver.resolve(dir, specifier, kind) {
            // `require.resolve` only wants the path; the file is not part of the bundle.
            Ok(found) if request.kind == RequestKind::Resolve => Target::External(found.path.to_string_lossy().into_owned()),
            // A native addon cannot be bundled; it stays where it is and is loaded from there.
            Ok(found) if found.path.extension().is_some_and(|e| e == "node") => Target::External(found.path.to_string_lossy().into_owned()),
            Ok(found) => Target::Path(found.path),
            Err(e) => {
                let severity = if request.kind == RequestKind::Static { Severity::Error } else { Severity::Warning };
                diagnostics.push(Diagnostic { severity, message: e.to_string(), file: Some(file.to_path_buf()), pos: Some(request.span.start) });
                Target::Unresolved
            }
        }
    }

    /// What goes in front of a style sheet's local class names so two sheets never share one:
    /// a hash of the file's place in the project.
    fn css_seed(&self, path: &Path) -> String {
        let relative = path.strip_prefix(self.cwd).unwrap_or(path);
        crate::loader::hash(relative.to_string_lossy().as_bytes(), 5)
    }

    fn is_external(&self, specifier: &str) -> bool {
        if self.options.node_builtins && is_node_builtin(specifier) {
            return true;
        }
        if self.options.packages_external && is_package_name(specifier) && !self.options.resolve.alias.iter().any(|a| a.matches(specifier)) {
            return true;
        }
        self.options.external.iter().any(|e| match e.strip_suffix("/*") {
            Some(prefix) => specifier.starts_with(prefix) && specifier[prefix.len()..].starts_with('/'),
            None => e == specifier,
        })
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Module,
    Script,
    /// A `.js` file nobody has labeled: a script unless it turns out to use module syntax.
    Auto,
}

/// The language a file is written in, from its name.
fn syntax_for(path: &Path) -> ParseOptions {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
        return ParseOptions::new(true).declaration_file();
    }
    let mut options = ParseOptions::new(true);
    if name.ends_with(".ts") || name.ends_with(".mts") || name.ends_with(".cts") || name.ends_with(".tsx") {
        options = options.typescript();
    }
    if name.ends_with(".tsx") || name.ends_with(".jsx") {
        options = options.with_jsx();
    }
    options
}

/// Parses a file, returning the tree and whether it was read as an ES module.
fn parse_file(source: &str, mode: Mode, syntax: ParseOptions) -> Result<(Parsed, bool), ParseError> {
    let as_script = ParseOptions { module: false, ..syntax };
    let as_module = ParseOptions { module: true, ..syntax };
    match mode {
        Mode::Module => parse(source, as_module).map(|p| (p, true)),
        // `.cts` is CommonJS with TypeScript's module syntax (`export =`), which only parses
        // as a module; it is still treated as CommonJS.
        Mode::Script if syntax.ts => parse(source, as_module).map(|p| (p, false)),
        Mode::Script => parse(source, as_script).map(|p| (p, false)),
        Mode::Auto => match parse(source, as_script) {
            Ok(parsed) => Ok((parsed, false)),
            // Module syntax is a syntax error in a script, and usually near the top of the
            // file, so the failed attempt is cheap. If neither reading works, report the
            // error that got further: it is the better guess at what the file is.
            Err(script_error) => parse(source, as_module).map(|p| (p, true)).map_err(|module_error| {
                if module_error.pos >= script_error.pos { module_error } else { script_error }
            }),
        },
    }
}

/// Turns the discovery result into the final graph: modules renumbered in depth-first order
/// from the entries, so the numbering does not depend on thread timing.
fn finish(shared: Shared, entry_slots: &[usize]) -> Graph {
    let Shared { mut modules, mut diagnostics, mut watch_dirs, .. } = shared;
    let mut visited = vec![false; modules.len()];
    let mut new_id: Vec<Option<u32>> = vec![None; modules.len()];
    let mut order: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = entry_slots.iter().rev().copied().collect();
    while let Some(slot) = stack.pop() {
        if std::mem::replace(&mut visited[slot], true) {
            continue;
        }
        // A file that failed to read or parse leaves its slot empty; a diagnostic holds the
        // reason, and edges to it become unresolved.
        let Some(module) = &modules[slot] else { continue };
        new_id[slot] = Some(order.len() as u32);
        order.push(slot);
        // Pushed in reverse so the first import is visited first.
        for edge in module.edges.iter().rev() {
            if let Edge::Module(id) = edge {
                stack.push(id.index());
            }
        }
    }
    let mut result: Vec<Module> = order.iter().map(|&slot| modules[slot].take().unwrap()).collect();
    for (index, module) in result.iter_mut().enumerate() {
        module.id = ModuleId(index as u32);
        for edge in &mut module.edges {
            if let Edge::Module(old) = *edge {
                *edge = match new_id[old.index()] {
                    Some(id) => Edge::Module(ModuleId(id)),
                    None => Edge::Unresolved,
                };
            }
        }
    }
    let entries = entry_slots.iter().filter_map(|&s| new_id[s].map(ModuleId)).collect();
    // Thread timing also shuffles diagnostics; sort them so the output is stable.
    diagnostics.sort_by(|a, b| (&a.file, a.pos, &a.message).cmp(&(&b.file, b.pos, &b.message)));
    diagnostics.dedup_by(|a, b| a.file == b.file && a.pos == b.pos && a.message == b.message);
    Graph { jsx: JsxOptions::default(), decorator_metadata: false, experimental_decorators: false, defines: Default::default(), modules: result, entries, diagnostics, pure_calls: Vec::new(), watch_dirs: { watch_dirs.sort(); watch_dirs.dedup(); watch_dirs } }
}

const NODE_BUILTINS: &[&str] = &[
    "assert", "async_hooks", "buffer", "child_process", "cluster", "console", "constants", "crypto", "dgram", "diagnostics_channel", "dns", "domain", "events",
    "fs", "http", "http2", "https", "inspector", "module", "net", "os", "path", "perf_hooks", "process", "punycode", "querystring", "readline", "repl", "stream",
    "string_decoder", "sys", "timers", "tls", "trace_events", "tty", "url", "util", "v8", "vm", "wasi", "worker_threads", "zlib",
];

/// `fs`, `fs/promises`, `node:test`, ... The `node:` prefix always means a built-in.
pub fn is_node_builtin(specifier: &str) -> bool {
    if specifier.starts_with("node:") {
        return true;
    }
    let root = specifier.split('/').next().unwrap_or(specifier);
    NODE_BUILTINS.contains(&root)
}

/// For use by callers that hold a path relative to the working directory.
pub fn absolute(cwd: &Path, path: &str) -> PathBuf {
    normalize(&cwd.join(path))
}

#[cfg(test)]
mod tests;

/// A file given to `--inject` and the names it exports.
#[derive(Clone, Debug)]
pub struct Inject {
    pub path: PathBuf,
    pub names: Vec<String>,
}

impl Inject {
    /// The names `path` exports (not `default`), read from the file.
    pub fn read(path: &Path) -> Result<Inject, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parsed = parse(&text, syntax_for(path)).map_err(|e| format!("{}: {}", path.display(), e.message))?;
        let record = ModuleRecord::build(&parsed);
        let names: Vec<String> = record.exports.iter().map(|e| e.exported.clone()).filter(|n| n != "default").collect();
        if names.is_empty() {
            return Err(format!("{}: has no named exports to inject", path.display()));
        }
        Ok(Inject { path: path.to_path_buf(), names })
    }
}

/// A name of a package (`react`, `@scope/pkg/sub`), not a path (`./a`, `/a`, `C:\a`), an address or a `#` import of the package itself.
fn is_package_name(specifier: &str) -> bool {
    let first = specifier.chars().next();
    let path_like = matches!(first, Some('.' | '/' | '#' | '\\')) || specifier.as_bytes().get(1) == Some(&b':') || specifier.contains("://") || specifier.starts_with("data:") || specifier.starts_with('\0');
    !specifier.is_empty() && !path_like
}

/// A module name with a query (`/a/logo.svg?raw`): the file, and the query without the `?`.
fn split_query(path: &Path) -> (PathBuf, String) {
    let text = path.to_string_lossy();
    // `\\?\C:\dir` (a Windows verbatim path) has a `?` that starts nothing: a query never holds a path separator.
    match text.rsplit_once('?') {
        Some((file, query)) if !file.is_empty() && !query.contains(['/', '\\']) && !text.starts_with('\0') => (PathBuf::from(file), query.to_string()),
        _ => (path.to_path_buf(), String::new()),
    }
}

/// The module `./a.wasm?init` is: `init(imports)` resolves to the instance, reading the file with Node's `fs` there and with `fetch` in a browser.
fn wasm_init_module(url: &str) -> String {
    format!(
        "export default async function init(imports) {{\n  const url = new URL({url}, import.meta.url);\n  if (typeof process !== \"undefined\" && process.versions && process.versions.node && url.protocol === \"file:\") {{\n    const {{ readFile }} = await import(\"node:fs/promises\");\n    const {{ fileURLToPath }} = await import(\"node:url\");\n    return (await WebAssembly.instantiate(await readFile(fileURLToPath(url)), imports)).instance;\n  }}\n  const response = await fetch(url);\n  if (WebAssembly.instantiateStreaming && (response.headers.get(\"content-type\") || \"\").startsWith(\"application/wasm\")) return (await WebAssembly.instantiateStreaming(response, imports)).instance;\n  return (await WebAssembly.instantiate(await response.arrayBuffer(), imports)).instance;\n}}\n"
    )
}

fn quote(text: &str) -> String {
    Json::String(text.to_string()).to_string()
}

/// What a worker query asks for: the script built alone (what `new Worker(new URL(...))` copies), or a module that makes the worker.
enum WorkerQuery {
    Bundled,
    Wrapper { shared: bool, inline: bool, url: bool },
}

fn worker_query(query: &str) -> Option<WorkerQuery> {
    let parts: Vec<&str> = query.split('&').collect();
    let has = |name: &str| parts.contains(&name);
    if has("rk-worker") {
        Some(WorkerQuery::Bundled)
    } else if has("worker") || has("sharedworker") {
        Some(WorkerQuery::Wrapper { shared: has("sharedworker"), inline: has("inline"), url: has("url") })
    } else {
        None
    }
}

/// What a query asks for by itself (as in Vite): `?raw` is the text, `?url` the address of a copied file, `?inline` a data URL.
fn query_loader(query: &str) -> Option<Loader> {
    query.split('&').find_map(|part| match part.split_once('=').map_or(part, |(k, _)| k) {
        "raw" => Some(Loader::Text),
        "url" => Some(Loader::File),
        "inline" => Some(Loader::DataUrl),
        _ => None,
    })
}
