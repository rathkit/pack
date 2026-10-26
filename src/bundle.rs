//! Putting the modules together: one output file with every module's code in one scope.
//!
//! This is scope hoisting. The modules of the bundle are laid out in execution order and
//! their top-level names share one scope, so an import becomes a direct reference to the
//! declaration it names and costs nothing at run time. That needs three things:
//!
//! 1. **Rewriting references.** The output is made from the original source text with span
//!    edits (rename an identifier, drop `export`, turn `ns.name` into the name itself, turn
//!    `require("x")` into a call of the bundled module), not printed from the tree. Formatting
//!    and comments inside statements survive, and every edit is small and local.
//! 2. **Naming.** Top-level names from different modules may collide, so each needed
//!    declaration gets a unique final name. A name is also kept clear of any variable in an
//!    inner scope that could capture a reference to it.
//! 3. **Wrapping what cannot be hoisted.** CommonJS modules (and JSON) run lazily inside a
//!    function, since `require` may be called at any time or never.
//!
//! Known limits for now: without `--splitting` there is one output chunk (dynamic `import()`
//! of a bundled ES module runs that module on first use but loads nothing), and CommonJS code
//! becomes strict when the output is an ES module.

mod lower;
mod lower_class;
mod std_decorators;

use std::collections::{HashMap, HashSet};
use crate::json::Json;
use std::fmt::Write as _;

use crate::ast::{Ast, List, NodeId, NodeKind, flags};
use crate::graph::{Diagnostic, Edge, Graph, Module, ModuleId, ModuleKind};
use crate::link::{Binding, Linked, export_names, resolve_export};
use crate::module::{ImportName, RequestKind};
use crate::semantic::{ScopeKind, Semantic, SymbolId, SymbolKind};
use crate::enums::{EnumValue, number_text};
use crate::shake::Shaken;

/// A thing that needs a name in the output.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// A module-level declaration.
    Symbol(ModuleId, SymbolId),
    /// The value of `export default <expression>`.
    Default(ModuleId),
    /// The namespace object of an ES module.
    Namespace(ModuleId),
    /// The function that runs a CommonJS (or JSON) module and returns its `exports`.
    Require(ModuleId),
    /// A binding imported from outside the bundle.
    External(String, ExtName),
    /// A local alias for a root's export that is not a plain name.
    Export(ModuleId, String),
    /// The function that runs a lazily initialized ES module.
    Init(ModuleId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ExtName {
    Default,
    Named(String),
    Namespace,
}

/// Text to put in place of a span: literal text, or a name decided later.
#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    Name(Key),
    /// `init_x(), ` when the module is initialized lazily, nothing otherwise.
    Init(ModuleId),
    /// Source text between two offsets, with the module's other edits applied inside it.
    Range(u32, u32),
    /// `import("./chunk.js")` for the chunk that holds a module loaded dynamically.
    DynamicChunk(ModuleId),
}

#[derive(Debug)]
struct Edit {
    start: u32,
    end: u32,
    pieces: Vec<Piece>,
    /// For a shorthand property `{ a }`: the written name. If the replacement is that same
    /// name the property stays shorthand, otherwise it becomes `a: replacement`.
    shorthand: Option<String>,
    /// Applied to the statement's own text but not inside a `Piece::Range`: a decorator is
    /// cut out of the class and read back into the `__decorate` call after it.
    transparent: bool,
}

/// How an output statement is written.
#[derive(Debug)]
enum Form {
    /// The statement text with edits applied.
    Plain,
    /// `export default <expression>` and anonymous declarations: `var name = <text>;`.
    DefaultValue(Key),
    /// An anonymous `export default function` keeps hoisting by receiving a name.
    DefaultFunction(Key),
    /// A class declaration whose name changed: `let New = class Old { ... };`, so that
    /// references to `Old` inside the class still work.
    Class { key: Key, original: String, decorated: bool },
}

/// What kind of statement it is, which matters when a module is initialized lazily and its
/// declarations turn into assignments.
#[derive(Debug)]
enum Shape {
    /// A function declaration, which stays hoisted at the top of the bundle.
    Function,
    /// `var`/`let`/`const`.
    Vars(Vec<Declarator>),
    /// An enum or namespace: `var N;` and an immediately called function that fills it in.
    Declared { symbol: SymbolId, needs_var: bool, alone: bool },
    Other,
}

#[derive(Debug)]
struct Declarator {
    target: (u32, u32),
    init: Option<(u32, u32)>,
    /// `{ a, b } = x` needs parentheses to be a statement.
    object_pattern: bool,
}

struct Stmt {
    start: u32,
    end: u32,
    edits: Vec<Edit>,
    form: Form,
    shape: Shape,
    /// Module-level symbols the statement declares.
    declares: Vec<SymbolId>,
    /// Statements that follow it: the `__decorate` calls of a decorated class.
    trailer: Vec<Piece>,
    /// Needs a `;` after it when the text does not end with one.
    terminate: bool,
    /// `const x = f();` kept for the call, with `x` unused: only `f();` is written (the initializer's span, and
    /// whether it needs parentheses to start a statement).
    drop_binding: Option<((u32, u32), bool)>,
}

struct EsmCode {
    stmts: Vec<Stmt>,
    /// The module uses top-level `await`, which a lazy init function cannot do.
    has_tla: bool,
    /// Every top-level declaration comes from a statement that can become an assignment.
    hoistable: bool,
}

/// What one module contributes.
enum Code {
    /// Statements of an ES module, hoisted into the bundle scope.
    Esm(EsmCode),
    /// A CommonJS or JSON module: the whole file as a function body.
    Wrapped {
        edits: Vec<Edit>,
        json: bool,
        /// The code uses `require` as a value (`factory(require)`, `require.cache`), so it gets a
        /// `require` function: the literal specifiers it uses, mapped to what they load.
        require_cases: Option<Vec<(String, Vec<Piece>)>>,
    },
}

enum Item {
    /// Emit the module's statements here.
    Run(ModuleId),
    /// Initialize a lazily wrapped ES module here.
    RunLazy(ModuleId),
    /// Execute a CommonJS module here, as an `import` of it would.
    RunCommonJs(ModuleId),
}

/// The shape of the output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    /// An ES module: `import` for externals, `export` for the entry's exports.
    #[default]
    Esm,
    /// A CommonJS module: `require` for externals, `module.exports` for the exports.
    Cjs,
    /// An immediately called function, for a `<script>` tag. Externals are globals.
    Iife,
}

#[derive(Clone)]
pub struct Options {
    /// Split into several files: one per entry, one per module loaded with `import()`, and
    /// shared chunks for what several of those use. Needs the esm format.
    pub splitting: bool,
    pub format: Format,
    /// For `Iife`: the global variable the entry's exports are assigned to.
    pub global_name: Option<String>,
    /// For `Iife`: which global holds each external module, as `(specifier, global)`.
    pub globals: Vec<(String, String)>,
    /// Replace `__dirname` and `__filename` in CommonJS modules by the original file's
    /// directory and path. Otherwise they name the bundle itself, which suits code that finds
    /// files next to the bundle, and breaks code that looks for files next to its own source.
    pub original_paths: bool,
    /// Shorten names and remove whitespace in the finished files.
    pub minify: crate::minify::Options,
    /// Write a source map for each output file.
    pub sourcemap: SourceMapMode,
    /// Put the original files' text into the map, so it works without them.
    pub sources_content: bool,
    /// The directory the output goes to, which the map's `sources` are relative to.
    pub map_base: Option<std::path::PathBuf>,
    /// The name of the one output file without splitting, which a linked map is named after.
    pub file_name: Option<String>,
    /// Serve mode: `import.meta.hot` is the page's hot API (`globalThis.__rathkit_hot`) instead of nothing.
    pub hot: bool,
    /// File names for entries, by position, where the name is not the entry file's own (`foo` or `foo.js`).
    pub entry_names: Vec<Option<String>>,
    /// Module federation: `import()` of a remote or a shared package asks the runtime.
    pub federation: Option<std::sync::Arc<crate::federation::Federation>>,
    /// Text put before and after the code of every script file and every style sheet file.
    pub banner_js: Option<String>,
    pub footer_js: Option<String>,
    pub banner_css: Option<String>,
    pub footer_css: Option<String>,
    /// Remove every `console.*(...)` call, and every `debugger` statement.
    pub drop_console: bool,
    pub drop_debugger: bool,
    /// How files are named: `[name]` and `[hash]` (a hash of what is in the file and in the files it loads) filled in. Entries and the
    /// other chunks have a template each; none means the names the build chooses (`name`, and `name-hash` or `chunk-hash` for the rest).
    pub entry_template: Option<String>,
    pub chunk_template: Option<String>,
    /// The extension of each kind of output file, by the one it would have: `(".js", ".mjs")`.
    pub out_extensions: Vec<(String, String)>,
    /// What becomes of the legal comments (`/*! ... */`, `@license`, `@preserve`) of the modules when the output is minified.
    pub legal_comments: crate::legal::LegalComments,
    /// The version of JavaScript the output may use; newer syntax is rewritten (or refused).
    pub target: crate::target::Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SourceMapMode {
    #[default]
    None,
    /// A `.map` file next to each output file, which the output points to.
    Linked,
    /// The map inside the output file, as a data URL.
    Inline,
    /// A `.map` file the output does not point to.
    External,
}

impl Default for Options {
    fn default() -> Self {
        Options { splitting: false, format: Format::Esm, global_name: None, globals: Vec::new(), original_paths: false, minify: crate::minify::Options::default(), sourcemap: SourceMapMode::None, sources_content: true, map_base: None, file_name: None, hot: false, entry_names: Vec::new(), federation: None, banner_js: None, footer_js: None, banner_css: None, footer_css: None, drop_console: false, drop_debugger: false, entry_template: None, chunk_template: None, out_extensions: Vec::new(), legal_comments: crate::legal::LegalComments::Eof, target: crate::target::Target::ESNEXT }
    }
}

/// A file of output.
#[derive(Clone)]
pub struct OutputFile {
    pub name: String,
    pub code: String,
    /// The source map (JSON), when asked for and not inline.
    pub map: Option<String>,
    /// For a copied file: its bytes (and `code` is empty).
    pub data: Option<Vec<u8>>,
    /// For a copied file: where it was copied from.
    pub original: Option<String>,
}

/// The finished bundle.
pub struct Output {
    /// The main file: the only one without splitting, otherwise the first entry's.
    pub code: String,
    /// Every file, entries first.
    pub files: Vec<OutputFile>,
    /// What each script chunk is made of, in the order of `files`.
    pub chunks: Vec<ChunkInfo>,
}

/// What a script chunk is made of, as plugins ask to know it.
#[derive(Clone, Debug, Default)]
pub struct ChunkInfo {
    pub file: String,
    pub name: String,
    pub is_entry: bool,
    pub is_dynamic_entry: bool,
    /// The entry module the chunk stands for.
    pub facade_module: Option<String>,
    /// The modules in the chunk: path and size of the original text.
    pub modules: Vec<(String, usize)>,
    /// Chunks and external modules the chunk imports when it loads.
    pub imports: Vec<String>,
    /// Chunks and external modules it may import later.
    pub dynamic_imports: Vec<String>,
    pub exports: Vec<String>,
}

/// Builds the bundle for `graph`, keeping what `shaken` says to keep.
pub fn bundle(graph: &Graph, linked: &Linked, shaken: &Shaken, options: &Options) -> Result<Output, Vec<Diagnostic>> {
    let mut b = Bundler::new(graph, linked, shaken);
    b.run(options)
}

/// One output file: the modules that live in it, and the roots whose exports it carries.
struct Chunk {
    file: String,
    modules: Vec<ModuleId>,
    roots: Vec<ModuleId>,
    /// What `[name]` stands for in a name template: the entry's own or given name, a lazy chunk's module, or `chunk`.
    base: String,
    /// Named by the build (an entry the command line or a plugin gave a file name): templates leave it be.
    given: bool,
    is_entry: bool,
}

/// How chunks refer to one another.
struct ChunkLinks {
    /// For each chunk, the names it imports from each other chunk.
    imports: Vec<std::collections::BTreeMap<usize, Vec<Key>>>,
    /// For each chunk, the names other chunks import from it.
    exports: Vec<Vec<Key>>,
    /// For each chunk, the externals it refers to.
    externals: Vec<Vec<Key>>,
    /// For each chunk, the chunks that must run before it, in order.
    deps: Vec<Vec<usize>>,
}

#[derive(Default)]
struct ChunkPlan {
    chunks: Vec<Chunk>,
    /// The chunk each module is in.
    of_module: Vec<Option<usize>>,
    /// The chunk that carries each root's exports.
    root_chunk: HashMap<ModuleId, usize>,
}

#[derive(Default)]
struct Helpers {
    esm: bool,
    decorate: bool,
    std_decorators: bool,
    metadata: bool,
    to_default: bool,
    to_ns: bool,
    require_shim: bool,
    path_shim: bool,
    /// `__spreadValues`, `__spreadProps` and `__objRest`: object spread and rest written for an older target.
    spread: bool,
    /// `__publicField`, `__privateGet` and the others: class fields and private members written for an older target.
    class_private: bool,
    async_fn: bool,
    for_await: bool,
    async_gen: bool,
}

struct Bundler<'a> {
    graph: &'a Graph,
    linked: &'a Linked,
    shaken: &'a Shaken,
    /// Keys in the order first seen, with the modules whose code refers to each.
    keys: Vec<Key>,
    key_refs: HashMap<Key, Vec<Option<ModuleId>>>,
    code: HashMap<ModuleId, Code>,
    ns_entries: HashMap<ModuleId, Vec<(String, Vec<Piece>)>>,
    pending_ns: Vec<ModuleId>,
    helpers: Helpers,
    errors: Vec<Diagnostic>,
    /// Entry exports whose value is an expression: `var alias = expr;` after everything ran.
    alias_values: Vec<(ModuleId, Key, Vec<Piece>)>,
    original_paths: bool,
    hot: bool,
    drop_console: bool,
    drop_debugger: bool,
    out_extensions: Vec<(String, String)>,
    /// `--target`: syntax newer than this is rewritten.
    target: crate::target::Target,
    /// The temporaries rewritten code uses, each declared once at the top of the chunk of the module that uses it.
    temps: Vec<(ModuleId, String)>,
    temp_count: u32,
    entry_names: Vec<Option<String>>,
    federation: Option<std::sync::Arc<crate::federation::Federation>>,
    /// Module federation: chunks whose code reads shared packages and remotes wait for the runtime to have loaded them
    /// before they run, and what they call to do it.
    ready_calls: HashMap<usize, Vec<Piece>>,
    format: Format,
    global_name: Option<String>,
    globals: Vec<(String, String)>,
    /// ES modules that run when first loaded, not at startup.
    wrapped: HashSet<ModuleId>,
    splitting: bool,
    plan: ChunkPlan,
    /// Whether output text is tagged with where it came from (see `mark_copy`).
    single_name: String,
    mapping: bool,
    /// The sources the tags in the text refer to.
    marks: std::sync::Mutex<Vec<Mark>>,
    /// Modules whose text can be tagged: it holds none of the tag characters.
    markable: Vec<bool>,
}

/// A stretch of a chunk's text, `start..end`, that `mark` accounts for.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: u32,
    end: u32,
    mark: usize,
    replacement: bool,
}

/// One segment of a map before sources and names are numbered: `source` and `name` count in
/// the `Traced` it is in.
struct Entry {
    line: u32,
    column: u32,
    /// The span a replacement belongs to: only its first segment is kept.
    replacement: u32,
    /// `u32::MAX`: no origin.
    source: u32,
    origin_line: u32,
    origin_column: u32,
    /// `u32::MAX`: no name.
    name: u32,
}

const NONE: u32 = u32::MAX;

impl Entry {
    fn origin(&self) -> Option<crate::sourcemap::Origin> {
        (self.source != NONE).then(|| crate::sourcemap::Origin { source: self.source, line: self.origin_line, column: self.origin_column, name: (self.name != NONE).then_some(self.name) })
    }
}

/// What the segments of a map need to say about a source.
enum SourceText {
    None,
    Module(ModuleId),
    Given(Option<String>),
}

/// The segments of one range of anchors with the sources and names they used.
#[derive(Default)]
struct Traced {
    entries: Vec<Entry>,
    /// (module, slot in that module's own input map or `u32::MAX` for the module itself)
    keys: crate::fxhash::FastMap<(u32, u32), u32>,
    sources: Vec<((u32, u32), String, SourceText)>,
    name_keys: crate::fxhash::FastMap<String, u32>,
    names: Vec<String>,
}

/// A piece of output text that came from a module: a copy of source text from `start`, or a
/// replacement for the text between `start` and `end`.
#[derive(Clone, Copy, Debug)]
struct Mark {
    module: ModuleId,
    start: u32,
    end: u32,
}

// Private-use characters that tag text while it is assembled. `copy` is followed by a number
// and `TAG_NUMBER_END`, then the text, then `TAG_CLOSE`.
const TAG_COPY: char = '\u{E000}';
const TAG_REPLACEMENT: char = '\u{E001}';
const TAG_CLOSE: char = '\u{E002}';
const TAG_NUMBER_END: char = '\u{E003}';

impl<'a> Bundler<'a> {
    fn new(graph: &'a Graph, linked: &'a Linked, shaken: &'a Shaken) -> Bundler<'a> {
        Bundler {
            graph,
            linked,
            shaken,
            keys: Vec::new(),
            key_refs: HashMap::new(),
            code: HashMap::new(),
            ns_entries: HashMap::new(),
            pending_ns: Vec::new(),
            helpers: Helpers::default(),
            errors: Vec::new(),
            alias_values: Vec::new(),
            original_paths: false,
            hot: false,
            drop_console: false,
            drop_debugger: false,
            out_extensions: Vec::new(),
            target: crate::target::Target::ESNEXT,
            temps: Vec::new(),
            temp_count: 0,
            entry_names: Vec::new(),
            federation: None,
            ready_calls: HashMap::new(),
            format: Format::Esm,
            global_name: None,
            globals: Vec::new(),
            wrapped: HashSet::new(),
            splitting: false,
            plan: ChunkPlan::default(),
            single_name: "bundle.js".to_string(),
            mapping: false,
            marks: std::sync::Mutex::new(Vec::new()),
            markable: Vec::new(),
                }
    }

    fn included(&self, id: ModuleId) -> bool {
        self.shaken.modules[id.index()].included
    }

    fn run(&mut self, options: &Options) -> Result<Output, Vec<Diagnostic>> {
        self.original_paths = options.original_paths;
        self.entry_names = options.entry_names.clone();
        self.hot = options.hot;
        self.drop_console = options.drop_console;
        self.drop_debugger = options.drop_debugger;
        self.out_extensions = options.out_extensions.clone();
        self.target = options.target;
        self.federation = options.federation.clone();
        self.format = options.format;
        self.splitting = options.splitting && options.format == Format::Esm;
        self.global_name = options.global_name.clone();
        self.globals = options.globals.clone();
        let graph = self.graph;
        self.mapping = options.sourcemap != SourceMapMode::None;
        if let Some(name) = &options.file_name {
            self.single_name = name.clone();
        }
        if self.mapping {
            self.markable = graph.modules.iter().map(|m| !m.source.contains(|c| ('\u{E000}'..='\u{E003}').contains(&c))).collect();
        }
        // Scan every module that will be emitted, collecting edits and the names they need.
        let included: Vec<ModuleId> = graph.modules.iter().filter(|m| self.included(m.id)).map(|m| m.id).collect();
        for id in included {
            self.scan(id);
        }
        // What each root (an entry, or with splitting a module loaded by `import()`) exports.
        let roots = self.roots();
        let root_exports = self.root_exports(&roots);
        // Namespace objects bring in the names of everything they expose.
        while let Some(id) = self.pending_ns.pop() {
            let mut entries = Vec::new();
            for name in export_names(graph, id) {
                if let Some(binding) = resolve_export(graph, id, &name) {
                    entries.push((name, self.binding_pieces(Some(id), &binding)));
                }
            }
            self.ns_entries.insert(id, entries);
        }
        if !self.errors.is_empty() {
            return Err(std::mem::take(&mut self.errors));
        }
        if self.format == Format::Iife {
            let missing: Vec<String> = self
                .keys
                .iter()
                .filter_map(|k| match k {
                    Key::External(spec, _) if !self.globals.iter().any(|(s, _)| s == spec) => Some(spec.clone()),
                    _ => None,
                })
                .collect();
            let mut seen = Vec::new();
            for spec in missing {
                if !seen.contains(&spec) {
                    self.errors.push(Diagnostic {
                        severity: crate::graph::Severity::Error,
                        message: format!("The iife format needs a global for the external module '{spec}' (--global {spec}=Name)"),
                        file: None,
                        pos: None,
                    });
                    seen.push(spec);
                }
            }
        }
        let forced = self.compute_wrapped();
        if self.format != Format::Esm {
            for module in &graph.modules {
                if let Some(Code::Esm(code)) = self.code.get(&module.id) {
                    if code.has_tla && !self.wrapped.contains(&module.id) && self.included(module.id) {
                        self.errors.push(Diagnostic {
                            severity: crate::graph::Severity::Error,
                            message: "Top-level await is only available in the esm output format".to_string(),
                            file: Some(module.path.clone()),
                            pos: None,
                        });
                    }
                }
            }
            if !self.errors.is_empty() {
                return Err(std::mem::take(&mut self.errors));
            }
        }
        let order = self.execution_order(&roots, &forced);
        self.plan = if self.splitting { self.split_plan(&roots) } else { self.single_plan() };
        self.federation_waits();
        let names = self.allocate_names();
        let links = self.chunk_links(&root_exports, &order);
        if let Err(message) = self.rename_chunks(&links, options) {
            return Err(vec![Diagnostic { severity: crate::graph::Severity::Error, message, file: None, pos: None }]);
        }
        let chunk_infos = self.chunk_infos(&roots, &root_exports, &links);
        let timing = std::env::var_os("RATHKIT_TIMING").is_some();
        let mut raws = Vec::new();
        for c in 0..self.plan.chunks.len() {
            let mut exports: Vec<(String, Vec<Piece>)> = Vec::new();
            for (root, list) in &root_exports {
                if self.plan.root_chunk.get(root) == Some(&c) {
                    exports.extend(list.iter().cloned());
                }
            }
            let started = std::time::Instant::now();
            raws.push(self.emit_chunk(c, &order, &exports, &names, &links));
            if timing {
                eprintln!("  emit chunk {c}: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // Frameworks that use decorators read class names at run time (Nest builds its module
        // tokens from them), so shortening them breaks those programs unless asked for.
        let keep_names = options.minify.keep_names.unwrap_or(graph.decorator_metadata || graph.experimental_decorators);
        let settings = crate::minify::Options { keep_top_level: self.format == Format::Iife, keep_names: Some(keep_names), ..options.minify };
        let marks = std::mem::take(&mut *self.marks.lock().expect("marks"));
        // The chunks are independent from here on, so with several they are finished side by side.
        let finish = |c: usize, (raw, layout): (String, crate::minify::Layout)| self.finish_chunk(c, raw, layout, options, &settings, &marks);
        let results: Vec<Result<OutputFile, Vec<Diagnostic>>> = if raws.len() < 2 {
            raws.into_iter().enumerate().map(|(c, raw)| finish(c, raw)).collect()
        } else {
            let total = raws.len();
            let queue = std::sync::Mutex::new(raws.into_iter().enumerate().collect::<Vec<_>>());
            let done: std::sync::Mutex<Vec<Option<Result<OutputFile, Vec<Diagnostic>>>>> = std::sync::Mutex::new((0..total).map(|_| None).collect());
            let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(total);
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(|| {
                        loop {
                            let Some((c, raw)) = queue.lock().expect("queue").pop() else { break };
                            let result = finish(c, raw);
                            done.lock().expect("results")[c] = Some(result);
                        }
                    });
                }
            });
            done.into_inner().expect("results").into_iter().map(|r| r.expect("every chunk finished")).collect()
        };
        let mut files = Vec::new();
        for result in results {
            files.push(result?);
        }
        let legal = self.legal_files(&files, options, &settings);
        files.extend(legal);
        files.extend(self.style_and_asset_files(&settings, options)?);
        let code = files.first().map(|f| f.code.clone()).unwrap_or_default();
        Ok(Output { code, files, chunks: chunk_infos })
    }

    /// With module federation, what a chunk of the application reads from a shared package or a remote is there only once the
    /// runtime has loaded it. Entry points wait for that before their own code, but a chunk of code they share, or that a
    /// module of the application the entry does not import shares with an exposed one, may run first. So every chunk with
    /// application code in it waits too (unless it is the runtime itself, or loaded by the runtime to get a package).
    fn federation_waits(&mut self) {
        let Some(federation) = self.federation.clone() else { return };
        let graph = self.graph;
        let Some(core) = graph.modules.iter().find(|m| m.path == federation.core_path()).map(|m| m.id) else { return };
        let Some(binding) = resolve_export(graph, core, "ready") else { return };
        let fallbacks: Vec<std::path::PathBuf> = federation.fallback_paths();
        let reach = |starts: Vec<ModuleId>, dynamic: bool| -> HashSet<ModuleId> {
            let mut seen = HashSet::new();
            let mut stack = starts;
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let module = graph.module(id);
                for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                    if let Edge::Module(t) = edge {
                        if dynamic || request.kind != RequestKind::Dynamic {
                            stack.push(*t);
                        }
                    }
                }
            }
            seen
        };
        let mut fallbacks = fallbacks;
        // A container that is a module of the application is loaded by the runtime too.
        fallbacks.extend(federation.internal_paths());
        let fallback_ids: Vec<ModuleId> = graph.modules.iter().filter(|m| fallbacks.contains(&m.path)).map(|m| m.id).collect();
        // The runtime and the copies it loads to give a package: they cannot wait for themselves.
        let mut inner = reach(vec![core], false);
        inner.extend(reach(fallback_ids.clone(), false));
        // The application: the entry points and what is exposed, with everything they reach.
        let mut roots: Vec<ModuleId> = graph.entries.clone();
        for (request, edge) in graph.module(core).record.requests.iter().zip(&graph.module(core).edges) {
            if let (Edge::Module(t), RequestKind::Dynamic) = (edge, request.kind) {
                if !fallback_ids.contains(t) {
                    roots.push(*t);
                }
            }
        }
        let app = reach(roots, true);
        for c in 0..self.plan.chunks.len() {
            let modules = &self.plan.chunks[c].modules;
            if modules.contains(&core) || !modules.iter().any(|m| app.contains(m) && !inner.contains(m)) {
                continue;
            }
            let Some(&first) = modules.first() else { continue };
            let pieces = self.binding_pieces(Some(first), &binding);
            self.ready_calls.insert(c, pieces);
        }
    }

    /// The extension a kind of output file gets (`.js` by default; `--out-extension:.js=.mjs`).
    fn out_extension(&self, default: &str) -> String {
        self.out_extensions.iter().find(|(from, _)| from == default).map_or_else(|| default.to_string(), |(_, to)| to.clone())
    }

    /// Names the chunks by the templates, once it is known which chunks load which: `[hash]` is a hash of the text of the modules in a chunk
    /// and of the hashes of the chunks it loads (statically or with `import()`), so a name changes when anything it runs does, and a
    /// chunk is named after what it loads before it is. (A chunk that loads itself through others is hashed without them at that point.)
    fn rename_chunks(&mut self, links: &ChunkLinks, options: &Options) -> Result<(), String> {
        let (entry_template, chunk_template) = (options.entry_template.as_deref(), options.chunk_template.as_deref());
        if entry_template.is_none() && chunk_template.is_none() && self.out_extensions.is_empty() {
            return Ok(());
        }
        let n = self.plan.chunks.len();
        let graph = self.graph;
        let mut loads: Vec<Vec<usize>> = links.deps.clone();
        for (c, chunk) in self.plan.chunks.iter().enumerate() {
            for &m in &chunk.modules {
                for (request, edge) in graph.module(m).record.requests.iter().zip(&graph.module(m).edges) {
                    if let (RequestKind::Dynamic, Edge::Module(t)) = (request.kind, edge) {
                        if let Some(&d) = self.plan.root_chunk.get(t) {
                            if d != c && !loads[c].contains(&d) {
                                loads[c].push(d);
                            }
                        }
                    }
                }
            }
        }
        // What the files are made with besides their modules' text.
        let mut salt: u64 = 0xcbf29ce484222325;
        let mix = |hash: &mut u64, bytes: &[u8]| {
            for &b in bytes {
                *hash = (*hash ^ b as u64).wrapping_mul(0x100000001b3);
            }
        };
        mix(&mut salt, format!("{:?}{:?}{:?}", self.format, options.minify.whitespace, options.minify.identifiers).as_bytes());
        mix(&mut salt, format!("{:?}{:?}{}", options.banner_js, options.footer_js, options.minify.syntax).as_bytes());
        let own: Vec<u64> = self
            .plan
            .chunks
            .iter()
            .map(|chunk| {
                let mut h = salt;
                for &m in &chunk.modules {
                    let module = graph.module(m);
                    mix(&mut h, module.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_bytes());
                    mix(&mut h, module.source.as_bytes());
                    mix(&mut h, &[0]);
                }
                h
            })
            .collect();
        // The final hash of each chunk: its own, then those of the chunks it loads (a chunk met again on the way is left out).
        fn settle(c: usize, loads: &[Vec<usize>], own: &[u64], done: &mut Vec<Option<u64>>, active: &mut Vec<bool>) -> u64 {
            if let Some(h) = done[c] {
                return h;
            }
            active[c] = true;
            let mut h = own[c];
            let mut deps: Vec<u64> = Vec::new();
            for &d in &loads[c] {
                if !active[d] {
                    deps.push(settle(d, loads, own, done, active));
                }
            }
            deps.sort_unstable();
            for d in deps {
                h = (h ^ d).wrapping_mul(0x100000001b3).rotate_left(7);
            }
            active[c] = false;
            done[c] = Some(h);
            h
        }
        let mut done = vec![None; n];
        let mut active = vec![false; n];
        let hashes: Vec<u64> = (0..n).map(|c| settle(c, &loads, &own, &mut done, &mut active)).collect();
        let digits = |mut h: u64| {
            let mut text = String::new();
            for _ in 0..8 {
                text.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(h % 36) as usize] as char);
                h /= 36;
            }
            text
        };
        let mut used: HashSet<String> = HashSet::new();
        let ext = self.out_extension(".js");
        for c in 0..n {
            let chunk = &self.plan.chunks[c];
            let old_ext = if chunk.file.ends_with(".mjs") { ".mjs" } else { ".js" };
            let stem = chunk.file.strip_suffix(old_ext).unwrap_or(&chunk.file).to_string();
            let template = if chunk.given { None } else if chunk.is_entry { entry_template } else { chunk_template };
            let base = match template {
                Some(t) => t.replace("[name]", &chunk.base).replace("[hash]", &digits(hashes[c])),
                None => stem,
            };
            if base.contains('/') || base.contains('\\') || base.is_empty() {
                return Err(format!("the name '{base}' made by a template is not a file name in the output folder (a directory in a name is not supported)"));
            }
            let ext = if chunk.given && old_ext == ".mjs" { ".mjs".to_string() } else { ext.clone() };
            let mut name = format!("{base}{ext}");
            let mut counter = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}-{counter}{ext}");
                counter += 1;
            }
            self.plan.chunks[c].file = name;
        }
        Ok(())
    }

    /// What each chunk holds and how it connects to the others.
    fn chunk_infos(&self, roots: &[ModuleId], root_exports: &[(ModuleId, Vec<(String, Vec<Piece>)>)], links: &ChunkLinks) -> Vec<ChunkInfo> {
        let graph = self.graph;
        let entry_count = graph.entries.len();
        let order_of_root: HashMap<ModuleId, usize> = roots.iter().enumerate().map(|(i, &r)| (r, i)).collect();
        let mut infos = Vec::with_capacity(self.plan.chunks.len());
        for (c, chunk) in self.plan.chunks.iter().enumerate() {
            let first_root = chunk.roots.first().copied();
            let is_entry = first_root.is_some_and(|r| order_of_root.get(&r).is_some_and(|&i| i < entry_count));
            let mut info = ChunkInfo { file: chunk.file.clone(), name: chunk.file.rsplit_once('.').map_or(chunk.file.clone(), |(s, _)| s.to_string()), is_entry, is_dynamic_entry: !is_entry && first_root.is_some(), ..ChunkInfo::default() };
            info.facade_module = first_root.map(|r| graph.module(r).path.to_string_lossy().into_owned());
            info.modules = chunk.modules.iter().map(|&m| (graph.module(m).path.to_string_lossy().into_owned(), graph.module(m).source.len())).collect();
            for &dep in &links.deps[c] {
                info.imports.push(self.plan.chunks[dep].file.clone());
            }
            for key in &links.externals[c] {
                if let Key::External(spec, _) = key {
                    if !info.imports.contains(spec) {
                        info.imports.push(spec.clone());
                    }
                }
            }
            for &m in &chunk.modules {
                for (request, edge) in graph.module(m).record.requests.iter().zip(&graph.module(m).edges) {
                    if request.kind != RequestKind::Dynamic {
                        continue;
                    }
                    let target = match edge {
                        Edge::Module(t) => self.plan.root_chunk.get(t).map(|&d| self.plan.chunks[d].file.clone()),
                        Edge::External(spec) => Some(spec.clone()),
                        Edge::Unresolved => None,
                    };
                    if let Some(t) = target {
                        if !info.dynamic_imports.contains(&t) {
                            info.dynamic_imports.push(t);
                        }
                    }
                }
            }
            for (root, list) in root_exports {
                if self.plan.root_chunk.get(root) == Some(&c) {
                    info.exports.extend(list.iter().map(|(name, _)| name.clone()));
                }
            }
            infos.push(info);
        }
        infos
    }

    /// The `.css` file of each chunk that holds style sheets, and the files the `file` loader copies.
    fn style_and_asset_files(&self, settings: &crate::minify::Options, options: &Options) -> Result<Vec<OutputFile>, Vec<Diagnostic>> {
        use crate::css::{self, RefKind, Replacement};
        let graph = self.graph;
        let mut files = Vec::new();
        if graph.modules.iter().any(|m| m.css.is_some() && self.included(m.id)) {
            // Sheets go out in the order a program would have run them: what a sheet imports comes first.
            let mut position = vec![usize::MAX; graph.modules.len()];
            let mut counter = 0;
            let mut starts = self.roots();
            starts.extend(graph.modules.iter().map(|m| m.id));
            for start in starts {
                if position[start.index()] != usize::MAX {
                    continue;
                }
                position[start.index()] = usize::MAX - 1;
                let mut stack = vec![(start, 0usize)];
                while let Some((id, next)) = stack.pop() {
                    let edges = &graph.module(id).edges;
                    let mut step = next;
                    let mut descended = false;
                    while step < edges.len() {
                        let edge = &edges[step];
                        step += 1;
                        if let Edge::Module(target) = edge {
                            if position[target.index()] == usize::MAX {
                                position[target.index()] = usize::MAX - 1;
                                stack.push((id, step));
                                stack.push((*target, 0));
                                descended = true;
                                break;
                            }
                        }
                    }
                    if !descended {
                        position[id.index()] = counter;
                        counter += 1;
                    }
                }
            }
            for chunk in &self.plan.chunks {
                let mut sheets: Vec<ModuleId> = chunk.modules.iter().copied().filter(|&m| self.included(m) && graph.module(m).css.is_some()).collect();
                if sheets.is_empty() {
                    continue;
                }
                sheets.sort_by_key(|m| position[m.index()]);
                // `@import "x" screen` wraps what x holds.
                let mut wrappers: HashMap<ModuleId, String> = HashMap::new();
                for &id in &sheets {
                    let module = graph.module(id);
                    let sheet = module.css.as_ref().expect("a sheet");
                    for r in sheet.parsed.refs.iter().filter(|r| r.kind == RefKind::Import && !r.conditions.is_empty()) {
                        if let Some(Edge::Module(target)) = module.record.request_index(&r.specifier, RequestKind::Static).map(|i| &module.edges[i]) {
                            wrappers.entry(*target).or_insert_with(|| r.conditions.clone());
                        }
                    }
                }
                let mut hoisted: Vec<String> = Vec::new();
                let mut body = String::new();
                // Places in the body that stand for places in a sheet: (offset, sheet, byte in the sheet's text).
                let mut anchors: Vec<(usize, ModuleId, u32)> = Vec::new();
                let mapping = options.sourcemap != SourceMapMode::None;
                for &id in &sheets {
                    let module = graph.module(id);
                    let sheet = module.css.as_ref().expect("a sheet");
                    let replacements: Vec<Replacement> = sheet
                        .parsed
                        .refs
                        .iter()
                        .map(|r| {
                            let edge = module.record.request_index(&r.specifier, RequestKind::Static).map(|i| &module.edges[i]);
                            match (&r.kind, edge) {
                                (RefKind::Import, Some(Edge::Module(_))) => Replacement::Leave,
                                (RefKind::Import, _) => Replacement::Keep,
                                (RefKind::Url, Some(Edge::Module(target))) => match &graph.module(*target).asset {
                                    Some(asset) => Replacement::Text(asset.url.clone()),
                                    None => Replacement::Leave,
                                },
                                (RefKind::Url, _) => Replacement::Leave,
                                (RefKind::Compose, Some(Edge::Module(target))) => match &graph.module(*target).css {
                                    Some(other) => Replacement::Seed(other.seed.clone()),
                                    None => Replacement::Leave,
                                },
                                (RefKind::Compose, _) => Replacement::Leave,
                            }
                        })
                        .collect();
                    let rendered = css::render_mapped(&sheet.text, &sheet.parsed, &replacements, sheet.local.then_some(sheet.seed.as_str()), settings.whitespace, mapping);
                    for h in rendered.hoisted {
                        if !hoisted.contains(&h) {
                            hoisted.push(h);
                        }
                    }
                    let text = rendered.css.trim();
                    if text.is_empty() {
                        continue;
                    }
                    let block = match wrappers.get(&id) {
                        Some(c) if c.starts_with("layer(") => format!("@layer {}{{{}}}", c[6..].trim_end_matches(')'), text),
                        Some(c) if c.starts_with("supports(") => format!("@supports {}{{{}}}", &c[8..c.rfind(')').map_or(c.len(), |p| p + 1)], text),
                        Some(c) => format!("@media {c}{{{text}}}"),
                        None => text.to_string(),
                    };
                    if mapping {
                        let lead = rendered.css.len() - rendered.css.trim_start().len();
                        let prefix = if wrappers.contains_key(&id) { block.len() - text.len() - 1 } else { 0 };
                        for &(offset, source) in &rendered.anchors {
                            if offset >= lead && offset - lead <= text.len() {
                                anchors.push((body.len() + prefix + offset - lead, id, source));
                            }
                        }
                    }
                    body.push_str(&block);
                    body.push('\n');
                }
                let mut out = String::new();
                if let Some(banner) = &options.banner_css {
                    out.push_str(banner);
                    out.push('\n');
                }
                for h in hoisted {
                    out.push_str(&h);
                    out.push('\n');
                }
                let body_at = out.len();
                out.push_str(&body);
                if let Some(footer) = &options.footer_css {
                    out.push_str(footer);
                    out.push('\n');
                }
                let stem = chunk.file.rsplit_once('.').map_or(chunk.file.as_str(), |(s, _)| s);
                let name = format!("{stem}{}", self.out_extension(".css"));
                let mut map = None;
                if mapping {
                    let mut builder = crate::sourcemap::Builder::default();
                    let mut slots: HashMap<ModuleId, u32> = HashMap::new();
                    let mut inputs: HashMap<ModuleId, crate::sourcemap::Positions> = HashMap::new();
                    let mut generated = crate::sourcemap::Positions::new(&out);
                    for &(offset, id, source) in &anchors {
                        let module = graph.module(id);
                        let sheet = module.css.as_ref().expect("a sheet");
                        let slot = *slots.entry(id).or_insert_with(|| {
                            let shown = match &options.map_base {
                                Some(base) => crate::sourcemap::relative(base, &module.path),
                                None => module.path.display().to_string(),
                            };
                            builder.add_source(shown, options.sources_content.then_some(std::borrow::Cow::Borrowed(sheet.text.as_str())))
                        });
                        let (line, column) = generated.at((body_at + offset) as u32);
                        let (source_line, source_column) = inputs.entry(id).or_insert_with(|| crate::sourcemap::Positions::new(&sheet.text)).at(source);
                        builder.add(line, column, Some(crate::sourcemap::Origin { source: slot, line: source_line, column: source_column, name: None }));
                    }
                    let json = builder.finish(&name);
                    match options.sourcemap {
                        SourceMapMode::Linked => {
                            let _ = write!(out, "/*# sourceMappingURL={name}.map */\n");
                            map = Some(json);
                        }
                        SourceMapMode::Inline => {
                            let _ = write!(out, "/*# sourceMappingURL=data:application/json;base64,{} */\n", base64(json.as_bytes()));
                        }
                        _ => map = Some(json),
                    }
                }
                files.push(OutputFile { name, code: out, map, data: None, original: None });
            }
        }
        // A copied file is written when something that stays in the bundle uses its value: a script that
        // kept the import, or a style sheet that points to it with `url()`.
        let mut used_by_sheets = std::collections::HashSet::new();
        for module in graph.modules.iter().filter(|m| m.css.is_some() && self.included(m.id)) {
            for edge in &module.edges {
                if let Edge::Module(target) = edge {
                    used_by_sheets.insert(*target);
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        for module in &graph.modules {
            if let (true, Some(asset)) = (self.included(module.id) || used_by_sheets.contains(&module.id), &module.asset) {
                if seen.insert(asset.name.clone()) {
                    files.push(OutputFile { name: asset.name.clone(), code: String::new(), map: None, data: Some(asset.data.as_ref().clone()), original: Some(asset.original.clone()) });
                }
            }
        }
        Ok(files)
    }

    /// The legal comments of the modules in chunk `c`, one after another, that are not already in `code` (text that is not minified keeps
    /// the comments inside what it copies, and loses only those between statements at the top of a module, which is where licenses are).
    fn legal_text(&self, c: usize, code: &str) -> String {
        let mut all: Vec<String> = Vec::new();
        for &m in &self.plan.chunks[c].modules {
            let module = self.graph.module(m);
            // The parser noted where they are; a module it did not parse (a sheet, JSON) has none worth keeping.
            let Some(parsed) = &module.parsed else { continue };
            for &(start, end) in &parsed.legal {
                let comment = module.source[start as usize..end as usize].trim_end().to_string();
                if !all.contains(&comment) && !code.contains(&comment) {
                    all.push(comment);
                }
            }
        }
        all.join("\n")
    }

    /// A file with the legal comments of its modules after the code (or named there), as `--legal-comments` says.
    fn with_legal_comments(&self, mut code: String, c: usize, file: &str, options: &Options, _settings: &crate::minify::Options) -> String {
        use crate::legal::LegalComments;
        if options.legal_comments == LegalComments::None || options.legal_comments == LegalComments::External {
            return code;
        }
        let text = self.legal_text(c, &code);
        if text.is_empty() {
            return code;
        }
        if !code.ends_with('\n') {
            code.push('\n');
        }
        match options.legal_comments {
            LegalComments::Linked => {
                let _ = writeln!(code, "/*! For license information please see {file}.LEGAL.txt */");
            }
            _ => {
                code.push_str(&text);
                code.push('\n');
            }
        }
        code
    }

    /// The files of legal comments (`--legal-comments=linked` and `external`), one for each file that has some.
    fn legal_files(&self, files: &[OutputFile], options: &Options, _settings: &crate::minify::Options) -> Vec<OutputFile> {
        use crate::legal::LegalComments;
        if !matches!(options.legal_comments, LegalComments::Linked | LegalComments::External) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (c, chunk) in self.plan.chunks.iter().enumerate() {
            let text = match files.iter().find(|f| f.name == chunk.file) {
                Some(file) => self.legal_text(c, &file.code),
                None => continue,
            };
            if !text.is_empty() {
                out.push(OutputFile { name: format!("{}.LEGAL.txt", chunk.file), code: format!("{text}\n"), map: None, data: None, original: None });
            }
        }
        out
    }

    /// Minifies one chunk's text and builds its map, as asked for.
    fn finish_chunk(&self, c: usize, raw: String, mut layout: crate::minify::Layout, options: &Options, settings: &crate::minify::Options, marks: &[Mark]) -> Result<OutputFile, Vec<Diagnostic>> {
        let timing = std::env::var_os("RATHKIT_TIMING").is_some();
        let fail = |message: String| vec![Diagnostic { severity: crate::graph::Severity::Error, message: format!("{}: {message}", self.plan.chunks[c].file), file: None, pos: None }];
        let file = self.plan.chunks[c].file.clone();
        if !self.mapping {
            let code = if settings.any() { crate::minify::minify_chunk(&raw, &layout, settings, None).map_err(fail)? } else { raw };
            let code = self.with_legal_comments(code, c, &file, options, settings);
            let code = wrap_text(code, options.banner_js.as_deref(), options.footer_js.as_deref());
            return Ok(OutputFile { name: file, code, map: None, data: None, original: None });
        }
        let tagging = std::time::Instant::now();
        let (text, spans) = self.split_tags(&raw, &mut layout);
        if timing {
            eprintln!("  split tags: {:.1} ms", tagging.elapsed().as_secs_f64() * 1000.0);
        }
        // Without minifying the text stays as it is, and copies of source are exact, so word
        // and punctuation starts inside them stand in for tokens: no second parse.
        let (mut code, anchors) = if settings.any() {
            {
                let mut anchors = Vec::new();
                let code = crate::minify::minify_chunk(&text, &layout, settings, Some(&mut anchors)).map_err(fail)?;
                (code, anchors)
            }
        } else {
            let anchors = synthetic_anchors(&text, &spans);
            (text, anchors)
        };
        let mapping = std::time::Instant::now();
        let mut map = self.build_map(&file, &code, &anchors, &spans, marks, options);
        if timing {
            eprintln!("  build map: {:.1} ms", mapping.elapsed().as_secs_f64() * 1000.0);
        }
        // A banner comes before everything the map knows of: its lines move every mapping down.
        if let Some(banner) = options.banner_js.as_deref() {
            map = shift_map_lines(&map, banner.matches('\n').count() + 1);
        }
        code = self.with_legal_comments(code, c, &file, options, settings);
        code = wrap_text(code, options.banner_js.as_deref(), options.footer_js.as_deref());
        let map_name = format!("{file}.map");
        let mut map_file = None;
        match options.sourcemap {
            SourceMapMode::Linked => {
                let _ = write!(code, "\n//# sourceMappingURL={map_name}\n");
                map_file = Some(map);
            }
            SourceMapMode::Inline => {
                let _ = write!(code, "\n//# sourceMappingURL=data:application/json;base64,{}\n", base64(map.as_bytes()));
            }
            _ => map_file = Some(map),
        }
        Ok(OutputFile { name: file, code, map: map_file, data: None, original: None })
    }

    /// Removes the tags from a chunk's text. What comes back are the pieces of the clean text
    /// that came from source files, in order and not overlapping.
    fn split_tags(&self, raw: &str, layout: &mut crate::minify::Layout) -> (String, Vec<Span>) {
        let bytes = raw.as_bytes();
        // The offsets the layout holds move with the text they stand in.
        let mut points: Vec<usize> = Vec::with_capacity(layout.cuts.len() + 2);
        points.push(layout.head);
        points.extend_from_slice(&layout.cuts);
        points.push(layout.tail);
        let mut moved = vec![0usize; points.len()];
        let mut next_point = 0;
        let mut out = String::with_capacity(raw.len());
        let mut spans: Vec<Span> = Vec::new();
        // Open tags: the mark, whether it replaces text, and where its current piece began.
        let mut open: Vec<(usize, bool, usize)> = Vec::new();
        let mut run = 0;
        let mut i = 0;
        while i + 2 < bytes.len() {
            // The tag characters are U+E000..U+E003, which UTF-8 writes as EE 80 80..83. Most
            // text has none of the lead byte for a long way, so look at eight bytes at a time.
            while i + 8 <= bytes.len() {
                let word = u64::from_le_bytes(bytes[i..i + 8].try_into().expect("eight bytes")) ^ 0xEEEE_EEEE_EEEE_EEEE;
                let zero = word.wrapping_sub(0x0101_0101_0101_0101) & !word & 0x8080_8080_8080_8080;
                if zero != 0 {
                    i += (zero.trailing_zeros() / 8) as usize;
                    break;
                }
                i += 8;
            }
            if i + 2 >= bytes.len() {
                break;
            }
            if bytes[i] != 0xEE || bytes[i + 1] != 0x80 || bytes[i + 2] > 0x83 {
                i += 1;
                continue;
            }
            while next_point < points.len() && points[next_point] <= i {
                moved[next_point] = out.len() + points[next_point].saturating_sub(run);
                next_point += 1;
            }
            out.push_str(&raw[run..i]);
            let tag = bytes[i + 2];
            i += 3;
            run = i;
            if tag == 0x82 {
                // close
                if let Some((mark, replacement, from)) = open.pop() {
                    if out.len() > from || replacement {
                        spans.push(Span { start: from as u32, end: out.len() as u32, mark, replacement });
                    }
                    if let Some(parent) = open.last_mut() {
                        parent.2 = out.len();
                    }
                }
            } else if tag != 0x83 {
                let mut j = i;
                while bytes[j] != 0xEE {
                    j += 1;
                }
                let mark: usize = raw[i..j].parse().unwrap_or(0);
                i = j + 3;
                run = i;
                if let Some(parent) = open.last() {
                    // The enclosing piece ends where the nested one starts.
                    if out.len() > parent.2 || parent.1 {
                        spans.push(Span { start: parent.2 as u32, end: out.len() as u32, mark: parent.0, replacement: parent.1 });
                    }
                }
                open.push((mark, tag == 0x81, out.len()));
            }
        }
        while next_point < points.len() {
            moved[next_point] = out.len() + points[next_point].saturating_sub(run);
            next_point += 1;
        }
        out.push_str(&raw[run..]);
        layout.head = moved[0];
        layout.tail = moved[moved.len() - 1];
        layout.cuts = moved[1..moved.len() - 1].to_vec();
        (out, spans)
    }

    /// The map for one output file, from where its tokens went and which pieces of its text
    /// came from which source. Ranges of anchors are traced side by side; numbering sources and
    /// names and writing the segments (which depend on what came before) is one pass after.
    fn build_map(&self, file: &str, code: &str, anchors: &[crate::minify::Anchor], spans: &[Span], marks: &[Mark], options: &Options) -> String {
        let timing = std::env::var_os("RATHKIT_TIMING").is_some();
        let tracing = std::time::Instant::now();
        let generated = crate::sourcemap::Positions::new(code);
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        // Small maps are traced in one go; RATHKIT_MAP_ANCHORS sets the least a range holds (tests use few).
        let least: usize = std::env::var("RATHKIT_MAP_ANCHORS").ok().and_then(|v| v.parse().ok()).unwrap_or(5_000).max(1);
        let parts = if anchors.len() < 4 * least { 1 } else { workers.min(anchors.len() / least).max(1) };
        let size = anchors.len().div_ceil(parts).max(1);
        let ranges: Vec<&[crate::minify::Anchor]> = anchors.chunks(size).collect();
        let mut traced: Vec<Traced> = if ranges.len() <= 1 {
            ranges.iter().map(|r| self.trace(r, code, spans, marks, options, generated.clone())).collect()
        } else {
            std::thread::scope(|scope| {
                let handles: Vec<_> = ranges.iter().map(|r| scope.spawn(|| self.trace(r, code, spans, marks, options, generated.clone()))).collect();
                handles.into_iter().map(|h| h.join().expect("trace")).collect()
            })
        };
        if timing {
            eprintln!("  trace anchors: {:.1} ms", tracing.elapsed().as_secs_f64() * 1000.0);
        }
        let writing = std::time::Instant::now();
        let mut builder = crate::sourcemap::Builder::default();
        let mut source_index: crate::fxhash::FastMap<(u32, u32), u32> = Default::default();
        let mut last_replacement = NONE;
        // Pass one, in order: number sources and names, and leave out the repeats of a replacement.
        // The encoder state at the start of each range is what lets the ranges be written side by side.
        let mut walker = crate::sourcemap::Encoder::default();
        let mut starts = Vec::with_capacity(traced.len());
        let mut scratch = String::new();
        for part in &mut traced {
            starts.push(walker);
            // Where this range's sources and names sit in the map; filled in as they are first used.
            let mut source_at = vec![NONE; part.sources.len()];
            let mut name_at = vec![NONE; part.names.len()];
            for entry in &mut part.entries {
                if entry.replacement != NONE {
                    // Everything a replacement wrote answers to where the replaced text began; once is enough.
                    if last_replacement == entry.replacement {
                        entry.line = NONE;
                        continue;
                    }
                    last_replacement = entry.replacement;
                }
                if entry.source != NONE {
                    let slot = entry.source as usize;
                    if source_at[slot] == NONE {
                        let (key, shown, content) = &part.sources[slot];
                        // A source another range met first keeps its place.
                        source_at[slot] = *source_index.entry(*key).or_insert_with(|| {
                            let content = match content {
                                SourceText::None => None,
                                SourceText::Module(id) => Some(std::borrow::Cow::Borrowed(self.graph.module(*id).source.as_str())),
                                SourceText::Given(text) => text.clone().map(std::borrow::Cow::Owned),
                            };
                            builder.add_source(shown.clone(), content)
                        });
                    }
                    entry.source = source_at[slot];
                    if entry.name != NONE {
                        let n = entry.name as usize;
                        if name_at[n] == NONE {
                            name_at[n] = builder.add_name(&part.names[n]);
                        }
                        entry.name = name_at[n];
                    }
                }
                walker.add::<false>(&mut scratch, entry.line, entry.column, entry.origin());
            }
        }
        // Pass two: each range writes its segments from the state the one before it left.
        let traced = &traced;
        let write = |i: usize| {
            let mut encoder = starts[i];
            let mut text = String::with_capacity(traced[i].entries.len() * 6);
            for entry in &traced[i].entries {
                if entry.line != NONE {
                    encoder.add::<true>(&mut text, entry.line, entry.column, entry.origin());
                }
            }
            (text, encoder)
        };
        let written: Vec<(String, crate::sourcemap::Encoder)> = if traced.len() <= 1 {
            (0..traced.len()).map(write).collect()
        } else {
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..traced.len()).map(|i| scope.spawn(move || write(i))).collect();
                handles.into_iter().map(|h| h.join().expect("write")).collect()
            })
        };
        for (text, end) in written {
            builder.append(&text, end);
        }
        if timing {
            eprintln!("  number and write segments: {:.1} ms", writing.elapsed().as_secs_f64() * 1000.0);
        }
        let loop_done = std::time::Instant::now();
        let json = builder.finish(file);
        if timing {
            eprintln!("  map json: {:.1} ms", loop_done.elapsed().as_secs_f64() * 1000.0);
        }
        json
    }

    /// Where each anchor of one range came from, without numbering anything yet.
    fn trace(&self, anchors: &[crate::minify::Anchor], code: &str, spans: &[Span], marks: &[Mark], options: &Options, mut generated: crate::sourcemap::Positions) -> Traced {
        // What is known about each module once it has been met.
        struct Known<'m> {
            positions: crate::sourcemap::Positions<'m>,
            input: Option<crate::sourcemap::Input>,
        }
        let mut known: Vec<Option<Known>> = (0..self.graph.modules.len()).map(|_| None).collect();
        let mut out = Traced { entries: Vec::with_capacity(anchors.len()), ..Traced::default() };
        let Some(first) = anchors.first() else {
            return out;
        };
        let mut at = spans.partition_point(|s| s.end <= first.inp);
        let mut last_replacement: Option<usize> = None;
        for anchor in anchors {
            while at < spans.len() && spans[at].end <= anchor.inp {
                at += 1;
            }
            let span = spans.get(at).filter(|s| s.start <= anchor.inp);
            let (line, column) = generated.at(anchor.out);
            let Some(span) = span else {
                out.entries.push(Entry { line, column, replacement: NONE, source: NONE, origin_line: 0, origin_column: 0, name: NONE });
                continue;
            };
            let mark = marks[span.mark];
            let module = self.graph.module(mark.module);
            let offset = if span.replacement {
                if last_replacement == Some(at) {
                    continue;
                }
                last_replacement = Some(at);
                mark.start
            } else {
                mark.start + (anchor.inp - span.start)
            };
            let replacement = if span.replacement { at as u32 } else { NONE };
            let state = known[mark.module.index()].get_or_insert_with(|| Known {
                positions: crate::sourcemap::Positions::new(&module.source),
                // A file that was itself built from other files says so in its own map.
                input: crate::sourcemap::Input::find(&module.path, &module.source),
            });
            let (origin_line, origin_column) = state.positions.at(offset);
            // A name is worth recording when an identifier is written differently than in the source.
            let was = if span.replacement { module.source.get(mark.start as usize..mark.end as usize) } else { module.source.get(offset as usize..(offset + anchor.in_len) as usize) };
            let now = &code[anchor.out as usize..(anchor.out + anchor.out_len) as usize];
            let mut name = match was {
                Some(was) if was != now && is_identifier_name(was) && is_identifier_name(now) => Some(was),
                _ => None,
            };
            let (source, line_in, column_in) = match &state.input {
                Some(input) => {
                    let Some(hit) = input.lookup(origin_line, origin_column).filter(|h| h.source != u32::MAX) else {
                        out.entries.push(Entry { line, column, replacement, source: NONE, origin_line: 0, origin_column: 0, name: NONE });
                        continue;
                    };
                    if let Some(n) = hit.name {
                        let written = input.names.get(n as usize).map_or("", |n| n.as_str());
                        name = (written != now).then_some(written);
                    }
                    let key = (mark.module.index() as u32, hit.source);
                    let slot = *out.keys.entry(key).or_insert_with(|| {
                        let (path, content) = input.sources.get(hit.source as usize).cloned().unwrap_or_default();
                        let shown = match &options.map_base {
                            Some(base) => crate::sourcemap::relative(base, std::path::Path::new(&path)),
                            None => path,
                        };
                        out.sources.push((key, shown, SourceText::Given(content.filter(|_| options.sources_content))));
                        out.sources.len() as u32 - 1
                    });
                    (slot, hit.line, hit.origin_column)
                }
                None => {
                    let key = (mark.module.index() as u32, u32::MAX);
                    let slot = *out.keys.entry(key).or_insert_with(|| {
                        let shown = match &options.map_base {
                            Some(base) => crate::sourcemap::relative(base, &module.path),
                            None => module.path.display().to_string(),
                        };
                        out.sources.push((key, shown, if options.sources_content { SourceText::Module(mark.module) } else { SourceText::None }));
                        out.sources.len() as u32 - 1
                    });
                    (slot, origin_line, origin_column)
                }
            };
            let name = match name {
                Some(n) => match out.name_keys.get(n) {
                    Some(&i) => i,
                    None => {
                        out.names.push(n.to_string());
                        out.name_keys.insert(n.to_string(), out.names.len() as u32 - 1);
                        out.names.len() as u32 - 1
                    }
                },
                None => NONE,
            };
            out.entries.push(Entry { line, column, replacement, source, origin_line: line_in, origin_column: column_in, name });
        }
        out
    }

    /// Entries, and with splitting every ES module that `import()` loads.
    fn roots(&self) -> Vec<ModuleId> {
        let graph = self.graph;
        let mut roots = graph.entries.clone();
        if self.splitting {
            let mut dynamic: Vec<ModuleId> = Vec::new();
            for module in &graph.modules {
                if !self.included(module.id) {
                    continue;
                }
                for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                    if let (Edge::Module(target), RequestKind::Dynamic) = (edge, request.kind) {
                        if graph.module(*target).kind == ModuleKind::Esm && self.included(*target) && !roots.contains(target) && !dynamic.contains(target) {
                            dynamic.push(*target);
                        }
                    }
                }
            }
            dynamic.sort();
            roots.extend(dynamic);
        }
        roots
    }

    fn single_plan(&self) -> ChunkPlan {
        let graph = self.graph;
        let modules: Vec<ModuleId> = graph.modules.iter().filter(|m| self.included(m.id)).map(|m| m.id).collect();
        let mut of_module = vec![None; graph.modules.len()];
        for m in &modules {
            of_module[m.index()] = Some(0);
        }
        let root_chunk = graph.entries.iter().map(|&e| (e, 0)).collect();
        ChunkPlan { chunks: vec![Chunk { file: self.single_name.clone(), modules, roots: graph.entries.clone(), base: String::new(), given: true, is_entry: true }], of_module, root_chunk }
    }

    /// Groups modules by which roots can reach them: modules reachable from exactly the same
    /// roots share a chunk, so a module used by two entries is in a chunk of its own.
    fn split_plan(&self, roots: &[ModuleId]) -> ChunkPlan {
        let graph = self.graph;
        let words = roots.len().div_ceil(64).max(1);
        let mut reach: Vec<Vec<u64>> = vec![vec![0; words]; graph.modules.len()];
        for (index, &root) in roots.iter().enumerate() {
            let mut stack = vec![root];
            // What the root exports may be declared in a module it reaches only through files of re-exports.
            if graph.module(root).kind == ModuleKind::Esm {
                for name in export_names(graph, root) {
                    if let Some(binding) = resolve_export(graph, root, &name) {
                        stack.extend(self.binding_module(&binding));
                    }
                }
            }
            while let Some(id) = stack.pop() {
                let bits = &mut reach[id.index()];
                if bits[index / 64] >> (index % 64) & 1 == 1 {
                    continue;
                }
                bits[index / 64] |= 1 << (index % 64);
                let module = graph.module(id);
                for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                    // `import()` of an ES module starts a root of its own.
                    if let Edge::Module(target) = edge {
                        if !self.starts_chunk(request.kind, *target) && self.included(*target) {
                            stack.push(*target);
                        }
                    }
                }
                stack.extend(self.bound_modules(id));
            }
        }
        let mut by_signature: std::collections::BTreeMap<Vec<u64>, usize> = std::collections::BTreeMap::new();
        let mut chunks: Vec<Chunk> = Vec::new();
        let mut of_module = vec![None; graph.modules.len()];
        let mut signatures: Vec<Vec<u64>> = Vec::new();
        for module in &graph.modules {
            let bits = &reach[module.id.index()];
            if !self.included(module.id) || bits.iter().all(|w| *w == 0) {
                continue;
            }
            let index = *by_signature.entry(bits.clone()).or_insert_with(|| {
                chunks.push(Chunk { file: String::new(), modules: Vec::new(), roots: Vec::new(), base: String::new(), given: false, is_entry: false });
                signatures.push(bits.clone());
                chunks.len() - 1
            });
            chunks[index].modules.push(module.id);
            of_module[module.id.index()] = Some(index);
        }
        // A root whose chunk is reached by no other root carries its exports itself; otherwise
        // it gets a thin file of its own that re-exports from the shared chunks.
        let mut root_chunk = HashMap::new();
        let popcount = |bits: &[u64]| bits.iter().map(|w| w.count_ones()).sum::<u32>();
        for (index, &root) in roots.iter().enumerate() {
            let own = of_module[root.index()];
            let exclusive = own.filter(|&c| popcount(&signatures[c]) == 1 && signatures[c][index / 64] >> (index % 64) & 1 == 1);
            let chunk = match exclusive {
                Some(c) => c,
                None => {
                    chunks.push(Chunk { file: String::new(), modules: Vec::new(), roots: Vec::new(), base: String::new(), given: false, is_entry: false });
                    chunks.len() - 1
                }
            };
            chunks[chunk].roots.push(root);
            root_chunk.insert(root, chunk);
        }
        // File names: entries by their own name, the rest by a hash of their contents' paths.
        let mut used: HashSet<String> = HashSet::new();
        let entry_count = graph.entries.len();
        let order_of_root: HashMap<ModuleId, usize> = roots.iter().enumerate().map(|(i, &r)| (r, i)).collect();
        for chunk in chunks.iter_mut() {
            // A virtual module's id (`\0virtual:x`) is not a file name; characters a file name cannot hold are replaced.
            let stem = |id: ModuleId| {
                let name = graph.module(id).path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "chunk".to_string());
                name.chars().map(|c| if c.is_control() || "\"#$%&*+,:;<=>?[]^`{|}\\/".contains(c) { '_' } else { c }).collect::<String>()
            };
            let mut hash: u64 = 0xcbf29ce484222325;
            let hashed: Vec<ModuleId> = if chunk.modules.is_empty() { chunk.roots.clone() } else { chunk.modules.clone() };
            for id in &hashed {
                for b in graph.module(*id).path.to_string_lossy().bytes() {
                    hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                }
            }
            let digest = {
                let mut n = hash;
                let mut text = String::new();
                for _ in 0..8 {
                    text.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize] as char);
                    n /= 36;
                }
                text
            };
            let mut ext = ".js";
            let (base, name_base, given, is_entry) = match chunk.roots.first() {
                Some(&root) if order_of_root[&root] < entry_count => match self.entry_names.get(order_of_root[&root]).and_then(|n| n.as_deref()) {
                    Some(given) if given.ends_with(".mjs") => {
                        ext = ".mjs";
                        let b = given[..given.len() - 4].to_string();
                        (b.clone(), b, true, true)
                    }
                    Some(given) => {
                        let b = given.strip_suffix(".js").unwrap_or(given).to_string();
                        (b.clone(), b, true, true)
                    }
                    None => {
                        let b = stem(root);
                        (b.clone(), b, false, true)
                    }
                },
                Some(&root) => (format!("{}-{digest}", stem(root)), stem(root), false, false),
                None => (format!("chunk-{digest}"), "chunk".to_string(), false, false),
            };
            chunk.base = name_base;
            chunk.given = given;
            chunk.is_entry = is_entry;
            let mut name = format!("{base}{ext}");
            let mut counter = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}-{counter}{ext}");
                counter += 1;
            }
            chunk.file = name;
        }
        ChunkPlan { chunks, of_module, root_chunk }
    }

    /// The module a binding is declared in, if it is one of the bundle's (not an outside module).
    fn binding_module(&self, binding: &Binding) -> Option<ModuleId> {
        let module = match binding {
            Binding::Symbol { module, .. } | Binding::DefaultValue { module, .. } | Binding::CommonJs { module, .. } | Binding::Namespace(module) => *module,
            Binding::External { .. } => return None,
        };
        self.included(module).then_some(module)
    }

    /// The modules the imports of `id` are declared in. A file of re-exports that is left out of the output (it has nothing
    /// of its own to run) is passed over, and what it passes on is reached through the bindings.
    fn bound_modules(&self, id: ModuleId) -> Vec<ModuleId> {
        // Only imports that are used: one the shaker found no use for does not make the module it names part of the entry.
        let shaken = &self.shaken.modules[id.index()];
        self.linked.imports[id.index()].iter().filter(|(symbol, _)| !shaken.demand_known || shaken.demanded.contains(symbol)).filter_map(|(_, b)| self.binding_module(b)).collect()
    }

    /// Whether loading `target` this way starts a chunk of its own: `import()` of an ES module. A
    /// CommonJS module `import()` loads is part of the chunk that asks for it.
    fn starts_chunk(&self, kind: RequestKind, target: ModuleId) -> bool {
        kind == RequestKind::Dynamic && self.graph.module(target).kind == ModuleKind::Esm
    }

    fn in_chunk(&self, module: ModuleId, chunk: usize) -> bool {
        self.plan.of_module[module.index()] == Some(chunk)
    }

    /// The chunk a name is defined in.
    fn key_home(&self, key: &Key) -> Option<usize> {
        match key {
            Key::Symbol(m, _) | Key::Default(m) | Key::Namespace(m) | Key::Require(m) | Key::Init(m) => self.plan.of_module[m.index()],
            Key::Export(root, _) => self.plan.root_chunk.get(root).copied(),
            Key::External(..) => None,
        }
    }

    /// Which names cross from one chunk to another, and which externals each chunk imports.
    fn chunk_links(&self, root_exports: &[(ModuleId, Vec<(String, Vec<Piece>)>)], order: &[Item]) -> ChunkLinks {
        let graph = self.graph;
        let n = self.plan.chunks.len();
        let mut used: HashMap<Key, std::collections::BTreeSet<usize>> = HashMap::new();
        for (key, referencers) in &self.key_refs {
            for r in referencers.iter().flatten() {
                if let Some(c) = self.plan.of_module[r.index()] {
                    used.entry(key.clone()).or_default().insert(c);
                }
            }
        }
        // What a root's exports and aliases refer to is used in the chunk that carries them.
        let keys_of = |pieces: &[Piece]| -> Vec<Key> { pieces.iter().filter_map(|p| if let Piece::Name(k) = p { Some(k.clone()) } else { None }).collect() };
        for (root, list) in root_exports {
            let Some(&c) = self.plan.root_chunk.get(root) else { continue };
            for (_, pieces) in list {
                for k in keys_of(pieces) {
                    used.entry(k).or_default().insert(c);
                }
            }
        }
        for (root, _, pieces) in &self.alias_values {
            let Some(&c) = self.plan.root_chunk.get(root) else { continue };
            for k in keys_of(pieces) {
                used.entry(k).or_default().insert(c);
            }
        }
        let mut links = ChunkLinks { imports: vec![std::collections::BTreeMap::new(); n], exports: vec![Vec::new(); n], externals: vec![Vec::new(); n], deps: vec![Vec::new(); n] };
        for key in &self.keys {
            let Some(chunks) = used.get(key) else { continue };
            if matches!(key, Key::External(..)) {
                for &c in chunks {
                    links.externals[c].push(key.clone());
                }
                continue;
            }
            let Some(home) = self.key_home(key) else { continue };
            for &c in chunks {
                if c != home {
                    links.imports[c].entry(home).or_default().push(key.clone());
                    if !links.exports[home].contains(key) {
                        links.exports[home].push(key.clone());
                    }
                }
            }
        }
        // Which chunks must run first: the ones whose modules this chunk imports, and the
        // ones whose names it uses, in the order the original modules would have run.
        let mut position: HashMap<ModuleId, usize> = HashMap::new();
        for (i, item) in order.iter().enumerate() {
            let (Item::Run(m) | Item::RunLazy(m) | Item::RunCommonJs(m)) = item;
            position.entry(*m).or_insert(i);
        }
        let first: Vec<usize> = (0..n).map(|c| self.plan.chunks[c].modules.iter().filter_map(|m| position.get(m).copied()).min().unwrap_or(usize::MAX)).collect();
        for c in 0..n {
            let mut deps: std::collections::BTreeSet<usize> = links.imports[c].keys().copied().collect();
            let mut sources: Vec<ModuleId> = self.plan.chunks[c].modules.clone();
            // A file that only re-exports a root must also run everything the root needs.
            if self.plan.chunks[c].modules.is_empty() {
                for &root in &self.plan.chunks[c].roots {
                    let mut stack = vec![root];
                    let mut seen = HashSet::new();
                    while let Some(id) = stack.pop() {
                        if !seen.insert(id) {
                            continue;
                        }
                        sources.push(id);
                        stack.extend(self.bound_modules(id));
                        for (request, edge) in graph.module(id).record.requests.iter().zip(&graph.module(id).edges) {
                            if let Edge::Module(t) = edge {
                                if !self.starts_chunk(request.kind, *t) && self.included(*t) {
                                    stack.push(*t);
                                }
                            }
                        }
                    }
                }
            }
            for id in sources {
                for (request, edge) in graph.module(id).record.requests.iter().zip(&graph.module(id).edges) {
                    if let Edge::Module(t) = edge {
                        if let (false, Some(d)) = (self.starts_chunk(request.kind, *t), self.plan.of_module[t.index()]) {
                            if d != c {
                                deps.insert(d);
                            }
                        }
                    }
                }
            }
            let mut deps: Vec<usize> = deps.into_iter().collect();
            deps.sort_by_key(|&d| (first[d], d));
            links.deps[c] = deps;
        }
        links
    }

    /// Decides which ES modules run lazily: the ones `import()` or `require()` load, and
    /// everything they import. Returns the modules in that group that cannot be wrapped (they
    /// run at startup instead).
    fn compute_wrapped(&mut self) -> Vec<ModuleId> {
        let graph = self.graph;
        let mut group: Vec<ModuleId> = Vec::new();
        let mut stack: Vec<ModuleId> = Vec::new();
        for module in &graph.modules {
            if !self.included(module.id) {
                continue;
            }
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                let lazy_edge = request.kind == RequestKind::Require || (request.kind == RequestKind::Dynamic && !self.splitting);
                if let (Edge::Module(target), true) = (edge, lazy_edge) {
                    if graph.module(*target).kind == ModuleKind::Esm && self.included(*target) {
                        stack.push(*target);
                    }
                }
            }
        }
        while let Some(id) = stack.pop() {
            if group.contains(&id) {
                continue;
            }
            group.push(id);
            let module = graph.module(id);
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let (Edge::Module(target), RequestKind::Static) = (edge, request.kind) {
                    if graph.module(*target).kind == ModuleKind::Esm && self.included(*target) {
                        stack.push(*target);
                    }
                }
            }
        }
        group.sort();
        let mut forced = Vec::new();
        for id in group {
            let ok = matches!(self.code.get(&id), Some(Code::Esm(code)) if !code.has_tla && code.hoistable);
            if ok {
                self.wrapped.insert(id);
                self.helpers.esm = true;
                self.register(Key::Init(id), None);
            } else {
                forced.push(id);
            }
        }
        forced
    }

    // ---- ordering ------------------------------------------------------------------------

    /// Modules in the order they execute: each module after the ones it imports, as the
    /// specification orders them. Modules that were shaken out are walked through (their own
    /// imports may still matter) but not emitted.
    fn execution_order(&self, root_modules: &[ModuleId], forced: &[ModuleId]) -> Vec<Item> {
        let graph = self.graph;
        let mut order = Vec::new();
        let mut seen = vec![false; graph.modules.len()];
        // `import()` and `require()` edges are not followed: what they load runs when they do,
        // except modules that could not be wrapped, which run at startup.
        let mut roots = root_modules.to_vec();
        roots.extend_from_slice(forced);
        for root in roots {
            if seen[root.index()] {
                continue;
            }
            seen[root.index()] = true;
            // Iterative post-order walk over static imports.
            let mut stack: Vec<(ModuleId, usize)> = vec![(root, 0)];
            while let Some((id, next)) = stack.pop() {
                let module = graph.module(id);
                let mut child = None;
                let mut position = next;
                while position < module.edges.len() {
                    if let (Edge::Module(target), RequestKind::Static) = (&module.edges[position], module.record.requests[position].kind) {
                        if !seen[target.index()] {
                            child = Some(*target);
                            position += 1;
                            break;
                        }
                    }
                    position += 1;
                }
                match child {
                    Some(target) => {
                        stack.push((id, position));
                        seen[target.index()] = true;
                        stack.push((target, 0));
                    }
                    None => {
                        if self.included(id) {
                            match module.kind {
                                ModuleKind::Esm if self.wrapped.contains(&id) => order.push(Item::RunLazy(id)),
                                ModuleKind::Esm => order.push(Item::Run(id)),
                                // What a federated import returns is read where it is used, not when the module is first imported:
                                // it is not there before the runtime has loaded it.
                                ModuleKind::CommonJs if module.path.starts_with(crate::federation::ROOT) => {}
                                ModuleKind::CommonJs => order.push(Item::RunCommonJs(id)),
                                ModuleKind::Json => {}
                            }
                        }
                    }
                }
            }
        }
        order
    }

    // ---- scanning ------------------------------------------------------------------------

    fn register(&mut self, key: Key, from: Option<ModuleId>) {
        let refs = self.key_refs.entry(key.clone()).or_default();
        if refs.is_empty() {
            self.keys.push(key.clone());
            if let Key::Namespace(id) = key {
                if self.graph.module(id).kind == ModuleKind::Esm {
                    self.pending_ns.push(id);
                }
            }
        }
        if !refs.contains(&from) {
            refs.push(from);
        }
    }

    /// Whether `module` follows Node's rules for importing CommonJS: `.mjs` files and files in
    /// a package with `"type": "module"`. Elsewhere the `__esModule` convention applies, as
    /// in Babel and webpack.
    fn node_mode(&self, module: Option<ModuleId>) -> bool {
        let Some(id) = module else { return true };
        let m = self.graph.module(id);
        m.path.extension().is_some_and(|e| e == "mjs") || m.package.as_ref().is_some_and(|p| p.is_module)
    }

    /// The text that stands for `binding`, as seen from code in `from`.
    fn binding_pieces(&mut self, from: Option<ModuleId>, binding: &Binding) -> Vec<Piece> {
        // What a federated import reads follows the `__esModule` convention, as the other bundlers read it.
        let node = self.node_mode(from);
        let flag_of = |graph: &Graph, module: ModuleId| if node && !graph.module(module).path.starts_with(crate::federation::ROOT) { ", 1)" } else { ")" };
        match binding {
            Binding::Symbol { module, symbol } => {
                let key = Key::Symbol(*module, *symbol);
                self.register(key.clone(), from);
                vec![Piece::Name(key)]
            }
            Binding::DefaultValue { module, .. } => {
                let key = Key::Default(*module);
                self.register(key.clone(), from);
                vec![Piece::Name(key)]
            }
            Binding::Namespace(module) => {
                if self.graph.module(*module).kind == ModuleKind::Esm {
                    let key = Key::Namespace(*module);
                    self.register(key.clone(), from);
                    vec![Piece::Name(key)]
                } else {
                    self.helpers.to_ns = true;
                    let key = Key::Require(*module);
                    self.register(key.clone(), from);
                    let flag = flag_of(self.graph, *module);
                    vec![Piece::Text("(__toNs(".into()), Piece::Name(key), Piece::Text(format!("(){flag})"))]
                }
            }
            Binding::CommonJs { module, name } => {
                let key = Key::Require(*module);
                self.register(key.clone(), from);
                let flag = flag_of(self.graph, *module);
                match name {
                    ImportName::Named(n) => {
                        let access = if is_identifier_name(n) { format!("().{n})") } else { format!("()[{}])", json_string(n)) };
                        vec![Piece::Text("(0, ".into()), Piece::Name(key), Piece::Text(access)]
                    }
                    ImportName::Default => {
                        self.helpers.to_default = true;
                        vec![Piece::Text("(__toDefault(".into()), Piece::Name(key), Piece::Text(format!("(){flag})"))]
                    }
                    ImportName::Namespace => {
                        self.helpers.to_ns = true;
                        vec![Piece::Text("(__toNs(".into()), Piece::Name(key), Piece::Text(format!("(){flag})"))]
                    }
                }
            }
            Binding::External { specifier, name } => {
                let ext = match name {
                    ImportName::Default => ExtName::Default,
                    ImportName::Named(n) => ExtName::Named(n.clone()),
                    ImportName::Namespace => ExtName::Namespace,
                };
                let key = Key::External(specifier.clone(), ext);
                self.register(key.clone(), from);
                vec![Piece::Name(key)]
            }
        }
    }

    fn scan(&mut self, id: ModuleId) {
        let module = self.graph.module(id);
        let Some(parsed) = module.parsed.as_ref() else {
            // JSON: the value becomes `module.exports`.
            self.register(Key::Require(id), None);
            self.code.insert(id, Code::Wrapped { edits: Vec::new(), json: true, require_cases: None });
            return;
        };
        let mut scanner = Scanner { bundler: self, module, places: None, private_scopes: Vec::new(), private_refs: HashMap::new(), chain_members: HashSet::new(), ast: &parsed.ast, semantic: &parsed.semantic, edits: Vec::new(), shorthand: HashSet::new(), skip: HashSet::new(), written: HashSet::new(), fn_depth: 0, async_ctx: Vec::new(), site_next: HashMap::new(), has_tla: false, needs_require: false, top_decl: None, in_wrapper: false, name_hint: None, shared_names: None };
        let code = if module.kind == ModuleKind::Esm { scanner.scan_esm(id) } else { scanner.scan_wrapped(id) };
        self.code.insert(id, code);
    }

    /// What each root makes available to whoever loads it.
    fn root_exports(&mut self, roots: &[ModuleId]) -> Vec<(ModuleId, Vec<(String, Vec<Piece>)>)> {
        let graph = self.graph;
        let mut all = Vec::new();
        for &root in roots {
            let module = graph.module(root);
            let mut out = Vec::new();
            match module.kind {
                // An iife with no global name has nobody to hand its exports to (and the shaker kept none of them).
                ModuleKind::Esm if self.format == Format::Iife && self.global_name.is_none() => {}
                ModuleKind::Esm => {
                    for name in export_names(graph, root) {
                        let Some(binding) = resolve_export(graph, root, &name) else { continue };
                        let pieces = self.binding_pieces(Some(root), &binding);
                        out.push((name, pieces));
                    }
                }
                _ => {
                    self.helpers.to_default = true;
                    let key = Key::Require(root);
                    self.register(key.clone(), Some(root));
                    out.push(("default".to_string(), vec![Piece::Text("__toDefault(".into()), Piece::Name(key), Piece::Text("(), 1)".into())]));
                }
            }
            // An export whose value is not a plain name needs one.
            for (name, pieces) in &mut out {
                if !matches!(pieces.as_slice(), [Piece::Name(_)]) {
                    let key = Key::Export(root, name.clone());
                    self.keys.push(key.clone());
                    self.key_refs.entry(key.clone()).or_default().push(Some(root));
                    let value = std::mem::replace(pieces, vec![Piece::Name(key.clone())]);
                    self.alias_values.push((root, key, value));
                }
            }
            all.push((root, out));
        }
        all
    }
}

// ---- the scanner: finds the edits a module needs -------------------------------------------

struct Scanner<'b, 'a> {
    bundler: &'b mut Bundler<'a>,
    module: &'a Module,
    /// The module's lines, for the place of a JSX element (`--jsx-dev`), made when first needed.
    places: Option<crate::sourcemap::Positions<'a>>,
    /// The private names of the classes being rewritten for `--target`, the innermost last.
    private_scopes: Vec<lower_class::PrivateScope>,
    /// What each private member node of an optional chain names, for the chain's rewrite.
    private_refs: HashMap<NodeId, lower_class::Private>,
    /// Private member nodes already rewritten (or left to a chain), so no other rewrite touches them.
    chain_members: HashSet<NodeId>,
    ast: &'a Ast,
    semantic: &'a Semantic,
    edits: Vec<Edit>,
    /// Identifiers written as `{ a }` in an object, which become `{ a: renamed }`.
    shorthand: HashSet<NodeId>,
    /// Identifiers already handled by the statement form.
    skip: HashSet<NodeId>,
    /// Expressions that are assigned to, which a definition does not replace.
    written: HashSet<NodeId>,
    /// How many functions the walk is inside.
    fn_depth: u32,
    /// For each function the walk is inside, how its `await`s are written once the target lacks async functions.
    async_ctx: Vec<lower::FnCtx>,
    /// How many of the temporaries for `super` writes each rewritten function has handed out.
    site_next: HashMap<u32, u32>,
    has_tla: bool,
    /// A CommonJS module mentions `require` other than as a direct call of a literal.
    needs_require: bool,
    /// The top-level statement being scanned, whose enum or namespace declares its variable
    /// separately (an inner one declares it inline).
    top_decl: Option<NodeId>,
    /// Scanning a CommonJS module's text, where top-level names are not renamed.
    in_wrapper: bool,
    /// Enums and namespaces of this module's top level that share their name with another one, found when first needed.
    shared_names: Option<HashSet<SymbolId>>,
    /// An anonymous class expression and the name its surroundings give it (`const X = class {}`).
    name_hint: Option<(NodeId, String)>,
}

impl<'a> Scanner<'_, 'a> {
    fn span(&self, node: NodeId) -> (u32, u32) {
        let s = self.ast[node].span;
        (s.start, s.end)
    }

    /// Everything in an ES module that stays, one statement at a time.
    fn scan_esm(&mut self, id: ModuleId) -> Code {
        let ast = self.ast;
        let shake = &self.bundler.shaken.modules[id.index()];
        let live: Vec<(NodeId, Vec<SymbolId>)> = shake.parts.iter().zip(&shake.live).filter(|(_, l)| **l).map(|(p, _)| (p.stmt, p.declares.clone())).collect();
        let declared: HashSet<SymbolId> = shake.parts.iter().flat_map(|p| p.declares.iter().copied()).collect();
        let root = self.semantic.root_scope();
        let hoistable = self.semantic.symbols().all(|(sid, sym)| sym.scope != root || sym.kind == SymbolKind::Import || declared.contains(&sid));
        let mut stmts = Vec::new();
        for (stmt, declares) in live {
            let (decl, form_default) = match ast[stmt].kind {
                NodeKind::ExportNamed(export) => (ast.export(export).decl.unwrap_or(stmt), false),
                NodeKind::ExportDefault(value) => (value, true),
                _ => (stmt, false),
            };
            let named_class = match ast[decl].kind {
                NodeKind::Class { name: Some(n), .. } if !form_default || ast[decl].has(flags::DECL) => Some(n),
                _ => None,
            };
            let mut terminate = !matches!(ast[decl].kind, NodeKind::Function { .. } | NodeKind::Class { .. } | NodeKind::Block(_) | NodeKind::Try { .. } | NodeKind::Switch { .. } | NodeKind::TsEnum { .. } | NodeKind::TsNamespace { .. });
            let form = if let Some(name_node) = named_class {
                self.skip.insert(name_node);
                match self.semantic.symbol_of(name_node) {
                    Some(sym) => {
                        let key = Key::Symbol(id, sym);
                        self.bundler.register(key.clone(), None);
                        let NodeKind::BindingIdent(atom) = ast[name_node].kind else { unreachable!() };
                        Form::Class { key, original: ast.atom(atom).to_string(), decorated: false }
                    }
                    None => Form::Plain,
                }
            } else if form_default {
                match ast[decl].kind {
                    // `export default name;` of a binding that stands for itself: nothing to write.
                    NodeKind::Ident(_) if matches!(self.bundler.graph.module(id).record.export("default").map(|e| &e.source), Some(crate::module::ExportSource::Local(_))) => continue,
                    NodeKind::Function { name: Some(_), .. } => Form::Plain,
                    NodeKind::Function { name: None, .. } if ast[decl].has(flags::DECL) => {
                        let key = Key::Default(id);
                        self.bundler.register(key.clone(), None);
                        Form::DefaultFunction(key)
                    }
                    _ => {
                        let key = Key::Default(id);
                        self.bundler.register(key.clone(), None);
                        terminate = true;
                        Form::DefaultValue(key)
                    }
                }
            } else {
                if let NodeKind::ExprStmt(_) = ast[decl].kind {
                    if ast[decl].has(flags::DIRECTIVE) {
                        continue;
                    }
                }
                if matches!(ast[decl].kind, NodeKind::Empty) {
                    continue;
                }
                Form::Plain
            };
            self.top_decl = Some(decl);
            let mut form = form;
            let mut trailer = Vec::new();
            let standard = !self.bundler.graph.experimental_decorators && matches!(ast[decl].kind, NodeKind::Class { .. }) && self.needs_standard_lowering(decl);
            if standard {
                let key = match &mut form {
                    Form::Class { key, decorated, .. } => {
                        *decorated = true;
                        Some(key.clone())
                    }
                    Form::DefaultValue(key) => Some(key.clone()),
                    _ => None,
                };
                if let Some(key) = key {
                    self.standard_lowering(decl, &std_decorators::Binding::Key(key), None);
                }
            } else if let Form::Class { key, decorated, .. } = &mut form {
                if self.class_is_decorated(decl) {
                    *decorated = true;
                    let key = key.clone();
                    trailer = self.decorator_trailer(decl, &key);
                }
            }
            let shape = match ast[decl].kind {
                NodeKind::TsEnum { name, .. } | NodeKind::TsNamespace { name, .. } => match self.semantic.symbol_of(name) {
                    Some(symbol) => {
                        let instantiated = !matches!(ast[decl].kind, NodeKind::TsNamespace { .. }) || self.namespace_has_values(decl);
                        // An enum with the name to itself is written `var E = ((E) => {...})({})`, which names it once.
                        let alone = matches!(ast[decl].kind, NodeKind::TsEnum { .. }) && self.enum_stands_alone(decl);
                        let needs_var = instantiated && !alone && self.semantic.symbol(symbol).kind == SymbolKind::Var;
                        if needs_var {
                            self.bundler.register(Key::Symbol(id, symbol), Some(id));
                        }
                        Shape::Declared { symbol, needs_var, alone }
                    }
                    None => Shape::Other,
                },
                NodeKind::Function { .. } => Shape::Function,
                NodeKind::VarDecl { decls, .. } => Shape::Vars(
                    ast.list(decls)
                        .iter()
                        .filter_map(|&d| {
                            let NodeKind::VarDeclarator { target, init } = ast[d].kind else { return None };
                            Some(Declarator { target: self.span(target), init: init.map(|i| self.span(i)), object_pattern: matches!(ast[target].kind, NodeKind::ObjectPattern(_)) })
                        })
                        .collect(),
                ),
                _ => Shape::Other,
            };
            self.walk(decl, false);
            let (start, end) = self.span(decl);
            self.add_erasures(start, end);
            let edits = std::mem::take(&mut self.edits);
            let drop_binding = self.unused_binding(id, decl, &form, &shape);
            stmts.push(Stmt { start, end, edits, form, shape, declares, trailer, terminate, drop_binding });
        }
        Code::Esm(EsmCode { stmts, has_tla: self.has_tla, hoistable })
    }

    /// A top-level `const x = init;` whose statement stays for what `init` does and whose name nothing uses:
    /// the span of `init` and whether it must be parenthesized to begin a statement.
    fn unused_binding(&self, id: ModuleId, decl: NodeId, form: &Form, shape: &Shape) -> Option<((u32, u32), bool)> {
        let ast = self.ast;
        let (Form::Plain, Shape::Vars(declarators)) = (form, shape) else { return None };
        let NodeKind::VarDecl { decls, .. } = ast[decl].kind else { return None };
        let [only] = ast.list(decls) else { return None };
        let NodeKind::VarDeclarator { target, init: Some(init) } = ast[*only].kind else { return None };
        let [_] = declarators.as_slice() else { return None };
        let NodeKind::BindingIdent(_) = ast[target].kind else { return None };
        let sym = self.semantic.symbol_of(target)?;
        let symbol = self.semantic.symbol(sym);
        if symbol.scope != self.semantic.root_scope() || self.semantic.scope(symbol.scope).flags & crate::semantic::scope_flags::HAS_EVAL != 0 || symbol.flags & crate::semantic::symbol_flags::DYNAMIC != 0 {
            return None;
        }
        let shake = &self.bundler.shaken.modules[id.index()];
        if !shake.demand_known || shake.demanded.contains(&sym) {
            return None;
        }
        let (start, end) = self.span(init);
        // As a statement, text that starts like a function, class or block would read as one (`function () {}()`).
        let head = self.module.source[start as usize..].trim_start();
        let wrap = ["function", "class", "{", "async"].iter().any(|word| head.starts_with(word));
        Some(((start, end), wrap))
    }

    /// A CommonJS module: its whole text goes inside a function.
    fn scan_wrapped(&mut self, id: ModuleId) -> Code {
        self.bundler.register(Key::Require(id), None);
        self.in_wrapper = true;
        self.walk(self.ast.root, true);
        self.add_erasures(0, self.module.source.len() as u32);
        let require_cases = if self.needs_require {
            let module = self.module;
            let mut cases = Vec::new();
            for request in &module.record.requests {
                if request.kind == RequestKind::Require {
                    if let Some(pieces) = self.load_pieces(&request.specifier, RequestKind::Require) {
                        cases.push((request.specifier.clone(), pieces));
                    }
                }
            }
            Some(cases)
        } else {
            None
        };
        Code::Wrapped { edits: std::mem::take(&mut self.edits), json: false, require_cases }
    }

    /// TypeScript-only text in `start..end` is deleted.
    fn add_erasures(&mut self, start: u32, end: u32) {
        for span in self.ast.erased_within(start, end) {
            self.edits.push(Edit { start: span.start, end: span.end, pieces: Vec::new(), shorthand: None, transparent: false });
        }
        for (span, text) in self.ast.rewrites_within(start, end) {
            self.edits.push(Edit { start: span.start, end: span.end, pieces: vec![Piece::Text((*text).to_string())], shorthand: None, transparent: false });
        }
    }

    fn push(&mut self, node: NodeId, pieces: Vec<Piece>) {
        let (start, end) = self.span(node);
        self.edits.push(Edit { start, end, pieces, shorthand: None, transparent: false });
    }

    fn ident(&mut self, node: NodeId) {
        let Some(sym_id) = self.semantic.symbol_of(node) else { return };
        let sym = self.semantic.symbol(sym_id);
        if self.skip.contains(&node) {
            return;
        }
        if let Some(pieces) = self.enum_member_reference(node, sym_id) {
            self.push_ident_edit(node, pieces);
            return;
        }
        if sym.scope != self.semantic.root_scope() || self.in_wrapper {
            return;
        }
        let Some(pieces) = self.symbol_pieces(sym_id) else { return };
        self.push_ident_edit(node, pieces);
    }

    /// The output's spelling of a module-level symbol referenced from this module.
    fn symbol_pieces(&mut self, sym_id: SymbolId) -> Option<Vec<Piece>> {
        let id = self.module.id;
        Some(match self.bundler.linked.binding(id, sym_id) {
            Some(binding) => {
                let binding = binding.clone();
                self.bundler.binding_pieces(Some(id), &binding)
            }
            None if self.semantic.symbol(sym_id).kind == SymbolKind::Import => return None,
            None => {
                let key = Key::Symbol(id, sym_id);
                self.bundler.register(key.clone(), Some(id));
                vec![Piece::Name(key)]
            }
        })
    }

    fn push_ident_edit(&mut self, node: NodeId, pieces: Vec<Piece>) {
        let (start, end) = self.span(node);
        let shorthand = self.shorthand.contains(&node).then(|| self.module.source[start as usize..end as usize].to_string());
        self.edits.push(Edit { start, end, pieces, shorthand, transparent: false });
    }

    /// The text of an enum member's value, for `Enum.Member` and `Enum["Member"]`.
    fn inlined_enum_member(&self, node: NodeId, object: NodeId, prop: NodeId) -> Option<String> {
        let ast = self.ast;
        let name = match ast[prop].kind {
            NodeKind::PropName(a) if !ast[node].has(flags::COMPUTED) => a,
            NodeKind::Str(a) if ast[node].has(flags::COMPUTED) => a,
            _ => return None,
        };
        let symbol = self.semantic.symbol_of(object)?;
        if self.semantic.symbol(symbol).scope != self.semantic.root_scope() {
            return None;
        }
        let value = self.bundler.shaken.enums.member(self.bundler.linked, self.module.id, symbol, ast.atom(name))?;
        Some(match value {
            EnumValue::Str(text) => json_string(&text),
            EnumValue::Num(n) => {
                let text = number_text(n);
                // `-1.x` and `1.x` read differently from `(-1).x` and `(1).x`.
                let member_access = self.module.source[ast[node].span.end as usize..].trim_start().starts_with('.');
                if text.starts_with('-') || n.is_nan() || n.is_infinite() || member_access { format!("({text})") } else { text }
            }
        })
    }

    /// A bare member name inside an enum body means `Enum.Member`.
    fn enum_member_reference(&mut self, node: NodeId, sym_id: SymbolId) -> Option<Vec<Piece>> {
        let sym = self.semantic.symbol(sym_id);
        if sym.kind != SymbolKind::Const || sym.decl == Some(node) {
            return None;
        }
        let scope = self.semantic.scope(sym.scope);
        if scope.kind != ScopeKind::Block {
            return None;
        }
        let NodeKind::TsEnum { name, .. } = self.ast[scope.node?].kind else { return None };
        let member = self.ast.atom(sym.name).to_string();
        let mut pieces = self.decl_name(name);
        pieces.push(Piece::Text(format!(".{member}")));
        Some(pieces)
    }

    /// The name of an enum or namespace as the output spells it: the bundle-level name at module
    /// scope (unless inside a CommonJS wrapper, which keeps its own), otherwise as written.
    fn decl_name(&mut self, name_node: NodeId) -> Vec<Piece> {
        let NodeKind::BindingIdent(atom) = self.ast[name_node].kind else { return Vec::new() };
        let text = self.ast.atom(atom).to_string();
        match self.semantic.symbol_of(name_node) {
            Some(sym) if !self.in_wrapper && self.semantic.symbol(sym).scope == self.semantic.root_scope() => {
                let key = Key::Symbol(self.module.id, sym);
                self.bundler.register(key.clone(), Some(self.module.id));
                vec![Piece::Name(key)]
            }
            _ => vec![Piece::Text(text)],
        }
    }

    /// `import("app/Button")` of a remote, or `import("react")` of a shared package: the runtime is asked when it runs.
    fn federated_import(&mut self, specifier: &str) -> Option<Vec<Piece>> {
        let federation = self.bundler.federation.clone()?;
        let export = match federation.dynamic_target(specifier)? {
            crate::federation::DynamicKind::Remote => "loadRemoteNamespace",
            crate::federation::DynamicKind::Shared => "loadSharedNamespace",
        };
        let graph = self.bundler.graph;
        let core = graph.modules.iter().find(|m| m.path == federation.core_path())?;
        let binding = resolve_export(graph, core.id, export)?;
        let mut pieces = self.bundler.binding_pieces(Some(self.module.id), &binding);
        pieces.push(Piece::Text(format!("({})", json_string(specifier))));
        Some(pieces)
    }

    /// The replacement for a `require("x")` or `import("x")` of a bundled module.
    fn load_pieces(&mut self, specifier: &str, kind: RequestKind) -> Option<Vec<Piece>> {
        let module = self.module;
        let index = module.record.request_index(specifier, kind)?;
        let Edge::Module(target) = module.edges[index] else { return None };
        let from = Some(module.id);
        let target_kind = self.bundler.graph.module(target).kind;
        if self.bundler.splitting && kind == RequestKind::Dynamic && target_kind == ModuleKind::Esm {
            return Some(vec![Piece::DynamicChunk(target)]);
        }
        let value = if target_kind == ModuleKind::Esm {
            // A lazily initialized module has to run before anything is read from it.
            let key = Key::Namespace(target);
            self.bundler.register(key.clone(), from);
            self.bundler.register(Key::Init(target), from);
            vec![Piece::Text("(".into()), Piece::Init(target), Piece::Name(key), Piece::Text(")".into())]
        } else if kind == RequestKind::Require {
            let key = Key::Require(target);
            self.bundler.register(key.clone(), from);
            vec![Piece::Name(key), Piece::Text("()".into())]
        } else {
            self.bundler.helpers.to_ns = true;
            let key = Key::Require(target);
            self.bundler.register(key.clone(), from);
            // `import()` always follows Node's rules for CommonJS.
            vec![Piece::Text("__toNs(".into()), Piece::Name(key), Piece::Text("(), 1)".into())]
        };
        if kind == RequestKind::Dynamic {
            let mut pieces = vec![Piece::Text("Promise.resolve().then(() => ".into())];
            pieces.extend(value);
            pieces.push(Piece::Text(")".into()));
            Some(pieces)
        } else {
            Some(value)
        }
    }

    /// `wrapped`: the node is in a CommonJS module, where only loads are rewritten.
    fn walk(&mut self, node: NodeId, wrapped: bool) {
        self.walk_in(node, wrapped, false)
    }

    /// `this_bound`: an enclosing function or class member gives `this` a value, so the
    /// module's own `this` (undefined) is not what it means here.
    fn walk_in(&mut self, node: NodeId, wrapped: bool, this_bound: bool) {
        let ast = self.ast;
        self.note_name_hint(node);
        if matches!(ast[node].kind, NodeKind::Class { .. }) && !self.bundler.graph.experimental_decorators && !self.skip.contains(&node) && self.needs_standard_lowering(node) {
            self.lower_nested_class(node);
        }
        match ast[node].kind {
            NodeKind::Assign { target, .. } => {
                self.written.insert(target);
            }
            NodeKind::Update { arg, .. } | NodeKind::Unary { op: crate::ast::UnaryOp::Delete, arg } => {
                self.written.insert(arg);
            }
            _ => {}
        }
        if !self.bundler.graph.defines.is_empty() {
            if matches!(ast[node].kind, NodeKind::Ident(_) | NodeKind::Member { .. } | NodeKind::ImportMeta) && !self.written.contains(&node) {
                if let Some(define) = self.bundler.graph.defines.lookup(ast, self.semantic, node) {
                    let text = define.replacement();
                    self.push_ident_edit(node, vec![Piece::Text(text)]);
                    return;
                }
            }
            if let Some(dead) = self.bundler.graph.defines.dead_branch(ast, self.semantic, node) {
                self.push(dead.node, vec![Piece::Text(dead.replacement.to_string())]);
                let mut children = Vec::new();
                ast.for_each_child(node, |c| children.push(c));
                for child in children.into_iter().filter(|&c| c != dead.node) {
                    self.walk_in(child, wrapped, this_bound);
                }
                return;
            }
        }
        if !self.bundler.target.is_esnext() {
            self.lower(node);
        }
        match ast[node].kind {
            NodeKind::Ident(name) => {
                if wrapped {
                    if let Some(sym) = self.semantic.symbol_of(node) {
                        if let Some(pieces) = self.enum_member_reference(node, sym) {
                            self.push_ident_edit(node, pieces);
                        }
                    } else {
                        match ast.atom(name) {
                            "require" => {
                                self.needs_require = true;
                                self.bundler.helpers.require_shim = true;
                            }
                            "__dirname" | "__filename" if self.bundler.original_paths => {
                                let path = &self.module.path;
                                let text = if ast.atom(name) == "__filename" { path.parent().map(|_| path.as_path()) } else { path.parent() };
                                let text = json_string(&text.unwrap_or(path).to_string_lossy());
                                self.push(node, vec![Piece::Text(text)]);
                            }
                            "__dirname" | "__filename" => self.bundler.helpers.path_shim = true,
                            _ => {}
                        }
                    }
                } else {
                    self.ident(node);
                }
                return;
            }
            NodeKind::BindingIdent(_) => {
                if !wrapped {
                    self.ident(node);
                }
                return;
            }
            // `new URL("./logo.png", import.meta.url)`: the string becomes the copied file's name.
            NodeKind::New { callee, args } if matches!(ast[callee].kind, NodeKind::Ident(n) if ast.atom(n) == "URL") => {
                if let [first, _] = ast.list(args) {
                    if let NodeKind::Str(spec) = ast[*first].kind {
                        let spec = ast.atom(spec).to_string();
                        // A script is only a copied file as a worker (a plain `new URL` of one stays as written), so the worker's edge is the one that has a file.
                        let edge = [RequestKind::WorkerUrl, RequestKind::AssetUrl].into_iter().filter_map(|k| self.module.record.request_index(&spec, k)).map(|i| &self.module.edges[i]).find(|e| matches!(e, Edge::Module(_)));
                        if let Some(Edge::Module(target)) = edge {
                            if let Some(asset) = &self.bundler.graph.module(*target).asset {
                                let text = json_string(&asset.url);
                                self.push(*first, vec![Piece::Text(text)]);
                            }
                        }
                    }
                }
            }
            NodeKind::This => {
                if !wrapped && !this_bound {
                    self.push(node, vec![Piece::Text("(void 0)".into())]);
                }
                return;
            }
            // webpack gives ES modules a `module` with an `id` and `hot`; generated code (css-loader) reads them.
            NodeKind::Member { object, prop }
                if !wrapped
                    && !ast[node].has(flags::COMPUTED)
                    && self.module.kind == ModuleKind::Esm
                    && matches!(ast[object].kind, NodeKind::Ident(n) if ast.atom(n) == "module")
                    && self.semantic.symbol_of(object).is_none()
                    && matches!(ast[prop].kind, NodeKind::PropName(p) if matches!(ast.atom(p), "id" | "hot")) =>
            {
                let NodeKind::PropName(p) = ast[prop].kind else { unreachable!() };
                let text = if ast.atom(p) == "id" { json_string(&self.module.path.to_string_lossy()) } else { "(void 0)".to_string() };
                self.push(node, vec![Piece::Text(text)]);
                return;
            }
            // `import.meta.hot` in the dev server: the page's hot object for this module, or nothing before the client runs.
            NodeKind::Member { object, prop } if !wrapped && self.bundler.hot && !ast[node].has(flags::COMPUTED) && matches!(ast[object].kind, NodeKind::ImportMeta) && matches!(ast[prop].kind, NodeKind::PropName(p) if ast.atom(p) == "hot") => {
                let id = json_string(&self.module.path.to_string_lossy());
                self.push(node, vec![Piece::Text(format!("(globalThis.__rathkit_hot && globalThis.__rathkit_hot({id}))"))]);
                return;
            }
            NodeKind::Member { object, prop } if !wrapped && !ast[node].has(flags::COMPUTED) && self.bundler.original_paths && matches!(ast[object].kind, NodeKind::ImportMeta) => {
                if let NodeKind::PropName(name) = ast[prop].kind {
                    let path = &self.module.path;
                    let text = match ast.atom(name) {
                        "url" => Some(file_url(path)),
                        "filename" => Some(json_string(&path.to_string_lossy())),
                        "dirname" => path.parent().map(|d| json_string(&d.to_string_lossy())),
                        _ => None,
                    };
                    if let Some(text) = text {
                        self.push(node, vec![Piece::Text(text)]);
                        return;
                    }
                }
            }
            // `Enum.Member` with a value known at build time.
            NodeKind::Member { object, prop } if matches!(ast[object].kind, NodeKind::Ident(_)) && !self.written.contains(&node) && self.inlined_enum_member(node, object, prop).is_some() => {
                let text = self.inlined_enum_member(node, object, prop).unwrap();
                self.push(node, vec![Piece::Text(text)]);
                return;
            }
            NodeKind::Member { object, prop } if !wrapped && !ast[node].has(flags::COMPUTED) => {
                if let (NodeKind::Ident(_), NodeKind::PropName(name)) = (ast[object].kind, ast[prop].kind) {
                    if let Some(sym) = self.semantic.symbol_of(object) {
                        if let Some(Binding::Namespace(target)) = self.bundler.linked.binding(self.module.id, sym).cloned() {
                            let name = ast.atom(name).to_string();
                            let pieces = match resolve_export(self.bundler.graph, target, &name) {
                                Some(binding) => self.bundler.binding_pieces(Some(self.module.id), &binding),
                                None => vec![Piece::Text("(void 0)".into())],
                            };
                            self.push(node, pieces);
                            return;
                        }
                    }
                }
            }
            // `--drop:console`: a call of a method of the global `console` is not made.
            NodeKind::Call { callee, .. }
                if self.bundler.drop_console
                    && matches!(ast[callee].kind, NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Ident(n) if ast.atom(n) == "console") && self.semantic.symbol_of(object).is_none()) =>
            {
                self.push(node, vec![Piece::Text("(void 0)".into())]);
                return;
            }
            // `--drop:debugger`
            NodeKind::Debugger if self.bundler.drop_debugger => {
                self.push(node, vec![Piece::Text(";".into())]);
                return;
            }
            NodeKind::Call { callee, args } => {
                // `require.resolve("x")` becomes the path it resolved to.
                if let NodeKind::Member { object, prop } = ast[callee].kind {
                    let list = ast.list(args);
                    if wrapped && matches!(ast[object].kind, NodeKind::Ident(_)) && self.semantic.symbol_of(object).is_none() && list.len() == 1 {
                        if let (NodeKind::Ident(o), NodeKind::PropName(p), NodeKind::Str(spec)) = (ast[object].kind, ast[prop].kind, ast[list[0]].kind) {
                            if ast.atom(o) == "require" && ast.atom(p) == "resolve" {
                                let spec = ast.atom(spec).to_string();
                                let edge = self.module.record.request_index(&spec, RequestKind::Resolve).map(|i| &self.module.edges[i]);
                                if let Some(Edge::External(path)) = edge {
                                    let text = json_string(path);
                                    self.push(node, vec![Piece::Text(text)]);
                                    return;
                                }
                            }
                        }
                    }
                }
                if let NodeKind::Ident(name) = ast[callee].kind {
                    let list = ast.list(args);
                    if ast.atom(name) == "require" && self.semantic.symbol_of(callee).is_none() && list.len() == 1 {
                        if let NodeKind::Str(spec) = ast[list[0]].kind {
                            let spec = ast.atom(spec).to_string();
                            if let Some(pieces) = self.load_pieces(&spec, RequestKind::Require) {
                                self.push(node, pieces);
                                return;
                            }
                            // A file left outside the bundle (a native addon) is loaded from where it is.
                            let external = self.module.record.request_index(&spec, RequestKind::Require).map(|i| &self.module.edges[i]);
                            if let Some(Edge::External(path)) = external {
                                if *path != spec && std::path::Path::new(path).is_absolute() {
                                    self.bundler.helpers.require_shim = true;
                                    let text = json_string(path);
                                    self.push(list[0], vec![Piece::Text(text)]);
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            NodeKind::ImportCall { source, options: None } => {
                if let NodeKind::Str(spec) = ast[source].kind {
                    if let Some(pieces) = self.federated_import(&ast.atom(spec).to_string()) {
                        self.push(node, pieces);
                        return;
                    }
                    if let Some(pieces) = self.load_pieces(&ast.atom(spec).to_string(), RequestKind::Dynamic) {
                        self.push(node, pieces);
                        return;
                    }
                }
            }
            NodeKind::Property { value, .. } | NodeKind::PatternProp { value, .. } if ast[node].has(flags::SHORTHAND) => {
                let target = match ast[value].kind {
                    NodeKind::AssignPattern { target, .. } | NodeKind::Assign { target, .. } => target,
                    _ => value,
                };
                self.shorthand.insert(target);
            }
            NodeKind::JsxElement(_) | NodeKind::JsxFragment { .. } => self.jsx_edit(node),
            NodeKind::ImportMeta if !wrapped && self.bundler.format != Format::Esm => {
                let text = if self.bundler.format == Format::Cjs { "({ url: require(\"url\").pathToFileURL(__filename).href })" } else { "({})" };
                self.push(node, vec![Piece::Text(text.to_string())]);
                return;
            }
            NodeKind::TsEnum { .. } => self.ts_enum_edit(node),
            NodeKind::TsNamespace { .. } => self.ts_namespace_edit(node),
            NodeKind::ClassField { key, value } => {
                self.walk_in(key, wrapped, this_bound);
                if let Some(value) = value {
                    self.walk_in(value, wrapped, true);
                }
                return;
            }
            NodeKind::StaticBlock(_) => {
                let mut children = Vec::new();
                ast.for_each_child(node, |c| children.push(c));
                for child in children {
                    self.walk_in(child, wrapped, true);
                }
                return;
            }
            _ => {}
        }
        let is_function = matches!(ast[node].kind, NodeKind::Function { .. });
        if self.fn_depth == 0 && (matches!(ast[node].kind, NodeKind::Await(_)) || matches!(ast[node].kind, NodeKind::ForOf { .. }) && ast[node].has(flags::AWAIT)) {
            self.has_tla = true;
        }
        let bound = this_bound || is_function && !ast[node].has(flags::ARROW);
        let mut children = Vec::new();
        ast.for_each_child(node, |c| children.push(c));
        self.fn_depth += is_function as u32;
        if is_function {
            let ctx = self.fn_ctx(node);
            self.async_ctx.push(ctx);
        }
        for child in children {
            self.walk_in(child, wrapped, bound);
        }
        if is_function {
            self.async_ctx.pop();
        }
        self.fn_depth -= is_function as u32;
        if is_function {
            self.parameter_properties(node);
        }
    }
}

// ---- naming ----------------------------------------------------------------------------------

const RESERVED: &[&str] = &[
    "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends", "false",
    "finally", "for", "function", "if", "implements", "import", "in", "instanceof", "interface", "let", "new", "null", "package", "private", "protected",
    "public", "return", "static", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with", "yield", "async", "of",
    "arguments", "eval", "undefined", "NaN", "Infinity", "require", "module", "exports", "__cjs", "__toDefault", "__toNs", "__ns", "__createRequire",
    "__filename", "__dirname", "__fileURLToPath", "__pathDirname", "__esm", "__req", "__decorate", "__param", "__metadata",
    "__esDecorate", "__runInitializers", "__setFunctionName", "__propKey",
];

impl Bundler<'_> {
    fn module_stem(&self, id: ModuleId) -> String {
        let path = &self.graph.module(id).path;
        let mut stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if stem == "index" {
            if let Some(parent) = path.parent().and_then(|p| p.file_name()) {
                stem = parent.to_string_lossy().into_owned();
            }
        }
        identifier_from(&stem)
    }

    fn base_name(&self, key: &Key) -> String {
        match key {
            Key::Symbol(m, s) => {
                let parsed = self.graph.module(*m).parsed.as_ref().unwrap();
                parsed.ast.atom(parsed.semantic.symbol(*s).name).to_string()
            }
            Key::Default(m) => format!("{}_default", self.module_stem(*m)),
            Key::Namespace(m) => format!("{}_exports", self.module_stem(*m)),
            Key::Require(m) => format!("require_{}", self.module_stem(*m)),
            Key::External(spec, ExtName::Named(n)) if is_identifier_name(n) => {
                let _ = spec;
                n.clone()
            }
            Key::External(spec, _) => identifier_from(spec),
            Key::Export(_, name) => format!("export_{}", identifier_from(name)),
            Key::Init(m) => format!("init_{}", self.module_stem(*m)),
        }
    }

    /// Final names for everything that needs one.
    fn allocate_names(&self) -> HashMap<Key, String> {
        let graph = self.graph;
        let mut globals: HashSet<String> = RESERVED.iter().map(|s| s.to_string()).collect();
        let mut nested: HashMap<ModuleId, HashSet<String>> = HashMap::new();
        for module in &graph.modules {
            if !self.included(module.id) {
                continue;
            }
            let Some(parsed) = module.parsed.as_ref() else { continue };
            for &node in parsed.semantic.unresolved() {
                if let NodeKind::Ident(name) = parsed.ast[node].kind {
                    globals.insert(parsed.ast.atom(name).to_string());
                }
            }
            // What an inner scope declares can capture a reference to a bundle-level name.
            let root = parsed.semantic.root_scope();
            let wrapped = module.kind != ModuleKind::Esm;
            let names: HashSet<String> = parsed
                .semantic
                .symbols()
                .filter(|(_, s)| wrapped || s.scope != root)
                .map(|(_, s)| parsed.ast.atom(s.name).to_string())
                .collect();
            nested.insert(module.id, names);
        }
        let mut names: HashMap<Key, String> = HashMap::new();
        let mut taken: HashSet<String> = HashSet::new();
        let entry_first = |k: &Key| matches!(k, Key::Symbol(m, _) if graph.entries.contains(m));
        let order: Vec<&Key> = self.keys.iter().filter(|k| entry_first(k)).chain(self.keys.iter().filter(|k| !entry_first(k))).collect();
        for key in order {
            let base = self.base_name(key);
            let refs = &self.key_refs[key];
            let mut n = 1;
            let name = loop {
                let candidate = if n == 1 { base.clone() } else { format!("{base}{n}") };
                n += 1;
                if taken.contains(&candidate) || globals.contains(&candidate) {
                    continue;
                }
                let captured = refs.iter().flatten().any(|r| {
                    let own = matches!(key, Key::Symbol(m, _) if m == r) && candidate == base;
                    !own && nested.get(r).is_some_and(|set| set.contains(&candidate))
                });
                if !captured {
                    break candidate;
                }
            };
            taken.insert(name.clone());
            names.insert(key.clone(), name);
        }
        names
    }
}

/// `"file:///a/b%20c.js"` for an absolute path, as `import.meta.url` reads in Node.
fn file_url(path: &std::path::Path) -> String {
    let mut url = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@' => url.push(b as char),
            _ => {
                let _ = write!(url, "%{b:02X}");
            }
        }
    }
    json_string(&url)
}

fn identifier_from(text: &str) -> String {
    let mut out: String = text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '$' { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Whether `text` ends in `;`, not counting whitespace or the tags of a source map.
fn ends_with_semicolon(text: &str) -> bool {
    strip_tags(text).trim_end().ends_with(';')
}

/// `text` without the tags of a source map.
fn strip_tags(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(|c: char| ('\u{E000}'..='\u{E003}').contains(&c)) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut in_number = false;
    for c in text.chars() {
        match c {
            TAG_COPY | TAG_REPLACEMENT => in_number = true,
            TAG_NUMBER_END => in_number = false,
            TAG_CLOSE => {}
            _ if in_number => {}
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Anchors for text that is not rewritten: the start of every word and punctuation mark inside
/// copies of source, the start of every replacement, and the start of generated text between.
fn synthetic_anchors(text: &str, spans: &[Span]) -> Vec<crate::minify::Anchor> {
    use crate::minify::Anchor;
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    let mut anchors = Vec::with_capacity(text.len() / 5);
    let mut covered = 0usize;
    let gap = |anchors: &mut Vec<Anchor>, at: usize| anchors.push(Anchor { out: at as u32, out_len: 0, inp: at as u32, in_len: 0 });
    for span in spans {
        let (start, end) = (span.start as usize, span.end as usize);
        if start > covered {
            gap(&mut anchors, covered);
        }
        if span.replacement {
            let mut j = start;
            while j < end && word(bytes[j]) {
                j += 1;
            }
            let len = if j > start { j - start } else { 1.min(end - start) };
            // A replacement that starts mid-character cannot happen: it starts where an edit began.
            anchors.push(Anchor { out: start as u32, out_len: len as u32, inp: start as u32, in_len: len as u32 });
        } else {
            let mut i = start;
            while i < end {
                let b = bytes[i];
                if b.is_ascii_whitespace() {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                if word(b) {
                    while j < end && word(bytes[j]) {
                        j += 1;
                    }
                }
                anchors.push(Anchor { out: i as u32, out_len: (j - i) as u32, inp: i as u32, in_len: (j - i) as u32 });
                i = j;
            }
        }
        covered = end;
    }
    if covered < bytes.len() {
        gap(&mut anchors, covered);
    }
    anchors
}

fn base64(data: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(DIGITS[(n >> 18) as usize & 63] as char);
        out.push(DIGITS[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { DIGITS[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { DIGITS[n as usize & 63] as char } else { '=' });
    }
    out
}

fn is_identifier_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// `code` with a banner before it and a footer after it, each on lines of their own.
fn wrap_text(code: String, banner: Option<&str>, footer: Option<&str>) -> String {
    if banner.is_none() && footer.is_none() {
        return code;
    }
    let mut out = String::with_capacity(code.len() + banner.map_or(0, str::len) + footer.map_or(0, str::len) + 2);
    if let Some(banner) = banner {
        out.push_str(banner);
        out.push('\n');
    }
    out.push_str(&code);
    if let Some(footer) = footer {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(footer);
        out.push('\n');
    }
    out
}

/// A source map whose generated lines start `lines` lower: an empty line of mappings for each.
fn shift_map_lines(map: &str, lines: usize) -> String {
    let Ok(Json::Object(mut fields)) = Json::parse(map) else { return map.to_string() };
    for (key, value) in &mut fields {
        if key == "mappings" {
            if let Json::String(text) = value {
                *text = format!("{}{}", ";".repeat(lines), text);
            }
        }
    }
    Json::Object(fields).to_string()
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- output ----------------------------------------------------------------------------------

const HELPER_CJS: &str = "var __cjs = (fn, req) => { var m; return () => m ? m.exports : (m = { exports: {} }, fn.call(m.exports, m.exports, m, req), m.exports); };\n";
const HELPER_ASYNC: &str = "var __async = (__this, __arguments, generator) => new Promise((resolve, reject) => { var fulfilled = (value) => { try { step(generator.next(value)); } catch (e) { reject(e); } }; var rejected = (value) => { try { step(generator.throw(value)); } catch (e) { reject(e); } }; var step = (x) => x.done ? resolve(x.value) : Promise.resolve(x.value).then(fulfilled, rejected); step((generator = generator.apply(__this, __arguments)).next()); });\n";
const HELPER_FOR_AWAIT: &str = "var __forAwait = (obj) => { var method = obj[Symbol.asyncIterator]; if (method != null) return method.call(obj); var it = obj[Symbol.iterator](); var wrap = (k) => (v) => { var r = it[k](v); return Promise.resolve(r.value).then((value) => ({ value, done: r.done })); }; return { next: wrap('next'), return: it.return && wrap('return') }; };\n";
const HELPER_ASYNC_GEN: &str = "var __await = function (v) { this.v = v; };\nvar __asyncGenerator = (__this, __arguments, fn) => { var gen = fn.apply(__this, __arguments), queue = [], busy = false, it = {}; var next = () => { if (busy || !queue.length) return; busy = true; var job = queue.shift(); step(job.k, job.v, job); }; var settle = (job, ok, v) => { busy = false; ok ? job.yes(v) : job.no(v); next(); }; var step = (k, v, job) => { var r; try { r = gen[k](v); } catch (e) { return settle(job, false, e); } var value = r.value; if (!r.done && value instanceof __await) Promise.resolve(value.v).then((x) => step('next', x, job), (e) => step('throw', e, job)); else Promise.resolve(value).then((x) => settle(job, true, { value: x, done: r.done }), (e) => r.done ? settle(job, false, e) : step('throw', e, job)); }; var method = (k) => it[k] = (v) => new Promise((yes, no) => { queue.push({ k, v, yes, no }); next(); }); method('next'); method('throw'); method('return'); it[Symbol.asyncIterator] = () => it; it[Symbol.toStringTag] = 'AsyncGenerator'; return it; };\nvar __yieldStar = function* (value) { var it = __forAwait(value), k = 'next', v, r, finished = false; try { for (;;) { try { r = yield new __await(it[k](v)); } catch (e) { finished = true; throw e; } if (r.done) { finished = true; return r.value; } try { v = yield r.value; k = 'next'; } catch (e) { if (!it.throw) throw e; k = 'throw'; v = e; } } } finally { if (!finished && it.return) yield new __await(it.return()); } };\n";
const HELPER_SPREAD: &str = "var __hasOwn = Object.prototype.hasOwnProperty;\nvar __propIsEnum = Object.prototype.propertyIsEnumerable;\nvar __defNormal = (obj, key, value) => key in obj ? Object.defineProperty(obj, key, { enumerable: true, configurable: true, writable: true, value }) : obj[key] = value;\nvar __spreadValues = (a, b) => { for (var prop in b || (b = {})) if (__hasOwn.call(b, prop)) __defNormal(a, prop, b[prop]); if (Object.getOwnPropertySymbols) for (var prop of Object.getOwnPropertySymbols(b)) { if (__propIsEnum.call(b, prop)) __defNormal(a, prop, b[prop]); } return a; };\nvar __spreadProps = (a, b) => { for (var key of Reflect.ownKeys(b)) Object.defineProperty(a, key, Object.getOwnPropertyDescriptor(b, key)); return a; };\nvar __objRest = (source, exclude) => { var target = {}; for (var prop in source) if (__hasOwn.call(source, prop) && exclude.indexOf(prop) < 0) target[prop] = source[prop]; if (source != null && Object.getOwnPropertySymbols) for (var prop of Object.getOwnPropertySymbols(source)) { if (exclude.indexOf(prop) < 0 && __propIsEnum.call(source, prop)) target[prop] = source[prop]; } return target; };\n";
const HELPER_CLASS_PRIVATE: &str = "var __typeError = (msg) => { throw TypeError(msg); };\nvar __defNormalProp = (obj, key, value) => key in obj ? Object.defineProperty(obj, key, { enumerable: true, configurable: true, writable: true, value }) : obj[key] = value;\nvar __publicField = (obj, key, value) => __defNormalProp(obj, typeof key !== \"symbol\" ? key + \"\" : key, value);\nvar __accessCheck = (obj, member, msg) => member.has(obj) || __typeError(\"Cannot \" + msg);\nvar __privateGet = (obj, member, getter) => (__accessCheck(obj, member, \"read from private field\"), getter ? getter.call(obj) : member.get(obj));\nvar __privateAdd = (obj, member, value) => member.has(obj) ? __typeError(\"Cannot add the same private member more than once\") : member instanceof WeakSet ? member.add(obj) : member.set(obj, value);\nvar __privateSet = (obj, member, value, setter) => (__accessCheck(obj, member, \"write to private field\"), setter ? setter.call(obj, value) : member.set(obj, value), value);\nvar __privateMethod = (obj, member, method) => (__accessCheck(obj, member, \"access private method\"), method);\nvar __privateWrapper = (obj, member, setter, getter) => ({ set _(value) { __privateSet(obj, member, value, setter); }, get _() { return __privateGet(obj, member, getter); } });\nvar __privateIn = (member, obj) => Object(obj) !== obj ? __typeError('Cannot use the \"in\" operator on this value') : member.has(obj);\n";
const HELPER_TO_DEFAULT: &str = "var __toDefault = (e, node) => e && !node && e.__esModule ? e.default : e;\n";
const HELPER_TO_NS: &str = "var __toNs = (e, node) => { if (!node && e && e.__esModule) return e; var t = Object.create(null, { [Symbol.toStringTag]: { value: \"Module\" } }); if (e && (typeof e == \"object\" || typeof e == \"function\")) for (let k of new Set(e.__esModule ? [...Object.keys(e), \"__esModule\"] : Object.keys(e))) if (k != \"default\") Object.defineProperty(t, k, { get: () => e[k], enumerable: true }); return Object.defineProperty(t, \"default\", { value: e, enumerable: true }); };\n";
const HELPER_REQ: &str = "var __req = (f, url) => { var r; var load = () => r || (r = __createRequire(url)); return Object.assign((id) => f(id, load()), { cache: Object.create(null), extensions: Object.create(null), resolve: (...a) => load().resolve(...a), main: void 0 }); };\n";
const HELPER_STD_DECORATORS: &str = "var __esDecorate = function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) { function accept(f) { if (f !== void 0 && typeof f !== \"function\") throw new TypeError(\"Function expected\"); return f; } var kind = contextIn.kind, key = kind === \"getter\" ? \"get\" : kind === \"setter\" ? \"set\" : \"value\"; var target = !descriptorIn && ctor ? contextIn[\"static\"] ? ctor : ctor.prototype : null; var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {}); var _, done = false; for (var i = decorators.length - 1; i >= 0; i--) { var context = {}; for (var p in contextIn) context[p] = p === \"access\" ? {} : contextIn[p]; for (var p in contextIn.access) context.access[p] = contextIn.access[p]; context.addInitializer = function (f) { if (done) throw new TypeError(\"Cannot add initializers after decoration has completed\"); extraInitializers.push(accept(f || null)); }; var result = (0, decorators[i])(kind === \"accessor\" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context); if (kind === \"accessor\") { if (result === void 0) continue; if (result === null || typeof result !== \"object\") throw new TypeError(\"Object expected\"); if (_ = accept(result.get)) descriptor.get = _; if (_ = accept(result.set)) descriptor.set = _; if (_ = accept(result.init)) initializers.unshift(_); } else if (_ = accept(result)) { if (kind === \"field\") initializers.unshift(_); else descriptor[key] = _; } } if (target) Object.defineProperty(target, contextIn.name, descriptor); done = true; };\nvar __runInitializers = function (thisArg, initializers, value) { var useValue = arguments.length > 2; for (var i = 0; i < initializers.length; i++) { value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg); } return useValue ? value : void 0; };\nvar __setFunctionName = function (f, name, prefix) { if (typeof name === \"symbol\") name = name.description ? \"[\".concat(name.description, \"]\") : \"\"; return Object.defineProperty(f, \"name\", { configurable: true, value: prefix ? \"\".concat(prefix, \" \", name) : name }); };\nvar __propKey = function (x) { return typeof x === \"symbol\" ? x : \"\".concat(x); };\n";
const HELPER_DECORATE: &str = "var __decorate = function (decorators, target, key, desc) { var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d; if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") r = Reflect.decorate(decorators, target, key, desc); else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r; return c > 3 && r && Object.defineProperty(target, key, r), r; };\nvar __param = function (index, decorator) { return function (target, key) { decorator(target, key, index); }; };\n";
const HELPER_METADATA: &str = "var __metadata = function (k, v) { if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") return Reflect.metadata(k, v); };\n";
const HELPER_REQ_CJS: &str = "var __req = (f) => Object.assign((id) => f(id, require), { cache: Object.create(null), extensions: Object.create(null), resolve: (...a) => require.resolve(...a), main: void 0 });\n";
const HELPER_REQ_IIFE: &str = "var __require = typeof require == \"function\" ? require : (id) => { throw new Error('Dynamic require of \"' + id + '\" is not supported'); };\nvar __req = (f) => Object.assign((id) => f(id, __require), { cache: Object.create(null), extensions: Object.create(null), resolve: (...a) => __require.resolve(...a), main: void 0 });\n";
const HELPER_EXPORT: &str = "var __export = (t, all) => { for (var k in all) Object.defineProperty(t, k, { get: all[k], enumerable: true }); return t; };\n";
const HELPER_ESM: &str = "var __esm = (fn) => { var done; return () => { done || (done = 1, fn()); }; };\n";
const HELPER_NS: &str = "var __ns = (all) => { var t = Object.create(null, { [Symbol.toStringTag]: { value: \"Module\" } }); for (let k in all) Object.defineProperty(t, k, { get: all[k], enumerable: true }); return t; };\n";

impl Bundler<'_> {
    fn text_of(&self, pieces: &[Piece], names: &HashMap<Key, String>) -> String {
        let mut out = String::new();
        for piece in pieces {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Name(k) => out.push_str(&names[k]),
                Piece::Init(id) => {
                    if self.wrapped.contains(id) {
                        out.push_str(&names[&Key::Init(*id)]);
                        out.push_str("(), ");
                    }
                }
                // Only `render` knows the edits to apply inside the range.
                Piece::Range(..) => unreachable!("a source range is rendered with its module's edits"),
                Piece::DynamicChunk(id) => {
                    let file = self.plan.root_chunk.get(id).map(|&c| self.plan.chunks[c].file.as_str()).unwrap_or("");
                    out.push_str(&format!("import({})", json_string(&format!("./{file}"))));
                }
            }
        }
        out
    }

    /// The source between `start` and `end` with `edits` applied.
    fn render(&self, id: ModuleId, start: u32, end: u32, edits: &[Edit], names: &HashMap<Key, String>) -> String {
        self.render_skipping(id, start, end, edits, names, &[])
    }

    /// As `render`, leaving out the edits in `skip`: the ones whose replacement is being
    /// written, whose own text refers back into the range they replace.
    fn render_skipping(&self, id: ModuleId, start: u32, end: u32, edits: &[Edit], names: &HashMap<Key, String>, skip: &[*const Edit]) -> String {
        // Inside a range read back from a replacement, the edits that cut text out of the
        // statement do not apply.
        let inside_range = !skip.is_empty();
        let mut sorted: Vec<&Edit> = edits.iter().filter(|e| !skip.contains(&(*e as *const Edit)) && !(inside_range && e.transparent)).collect();
        // Insertions first at a position, then the widest edit, so a replacement wins over
        // the smaller edits inside it.
        sorted.sort_by_key(|e| (e.start, e.start != e.end, std::cmp::Reverse(e.end)));
        let mut out = String::with_capacity((end - start) as usize);
        let mut at = start;
        for edit in sorted {
            if edit.start < at || edit.end > end {
                continue; // inside an edit already applied
            }
            self.copy(&mut out, id, at, edit.start);
            let mut inner_skip: Vec<*const Edit> = skip.to_vec();
            inner_skip.push(edit as *const Edit);
            let mut text = String::new();
            for piece in &edit.pieces {
                match piece {
                    Piece::Range(from, to) => text.push_str(&self.render_skipping(id, *from, *to, edits, names, &inner_skip)),
                    other => text.push_str(&self.text_of(std::slice::from_ref(other), names)),
                }
            }
            let opened = self.open_replacement(&mut out, id, edit.start, edit.end);
            match &edit.shorthand {
                Some(written) if *written != strip_tags(&text) => {
                    out.push_str(written);
                    out.push_str(": ");
                    out.push_str(&text);
                }
                _ => out.push_str(&text),
            }
            if opened {
                out.push(TAG_CLOSE);
            }
            at = edit.end;
        }
        self.copy(&mut out, id, at, end);
        out
    }

    /// Appends the source text between `from` and `to`, tagged with where it came from when
    /// a source map is being built.
    fn copy(&self, out: &mut String, id: ModuleId, from: u32, to: u32) {
        if from >= to {
            return;
        }
        let text = &self.graph.module(id).source[from as usize..to as usize];
        if !self.mapping || !self.markable[id.index()] {
            out.push_str(text);
            return;
        }
        let number = self.add_mark(id, from, to);
        out.push(TAG_COPY);
        let _ = write!(out, "{number}");
        out.push(TAG_NUMBER_END);
        out.push_str(text);
        out.push(TAG_CLOSE);
    }

    /// Inserts generated text into rendered text. Inside a tagged copy of source, the copy is
    /// split in two so that what follows the insertion still maps to the right place.
    fn insert_text(&self, text: &mut String, at: usize, insertion: &str) {
        if self.mapping {
            // Find the copy the position is inside of, if any.
            let bytes = text.as_bytes();
            let mut open: Option<(usize, usize)> = None; // (mark, where its text begins)
            let mut i = 0;
            while i + 2 < bytes.len() && i < at {
                if bytes[i] == 0xEE && bytes[i + 1] == 0x80 && bytes[i + 2] <= 0x83 {
                    match bytes[i + 2] {
                        0x80 | 0x81 => {
                            let mut j = i + 3;
                            while bytes[j] != 0xEE {
                                j += 1;
                            }
                            let mark: usize = text[i + 3..j].parse().unwrap_or(0);
                            open = (bytes[i + 2] == 0x80).then_some((mark, j + 3));
                            i = j + 3;
                            continue;
                        }
                        0x82 => open = None,
                        _ => {}
                    }
                    i += 3;
                } else {
                    i += 1;
                }
            }
            if let Some((mark, begins)) = open {
                let Mark { module, start, end } = self.marks.lock().expect("marks")[mark];
                let rest = self.add_mark(module, start + (at - begins) as u32, end);
                let joined = format!("{TAG_CLOSE}{insertion}{TAG_COPY}{rest}{TAG_NUMBER_END}");
                text.insert_str(at, &joined);
                return;
            }
        }
        text.insert_str(at, insertion);
    }

    /// Starts tagging a replacement for the source between `start` and `end`; the caller
    /// writes `TAG_CLOSE` after the text when this returns true.
    fn open_replacement(&self, out: &mut String, id: ModuleId, start: u32, end: u32) -> bool {
        if !self.mapping || !self.markable[id.index()] {
            return false;
        }
        let number = self.add_mark(id, start, end);
        out.push(TAG_REPLACEMENT);
        let _ = write!(out, "{number}");
        out.push(TAG_NUMBER_END);
        true
    }

    fn add_mark(&self, module: ModuleId, start: u32, end: u32) -> usize {
        let mut marks = self.marks.lock().expect("marks");
        marks.push(Mark { module, start, end });
        marks.len() - 1
    }

    /// One statement as it appears in the output, edits applied and the form written out.
    fn render_stmt(&self, id: ModuleId, stmt: &Stmt, names: &HashMap<Key, String>) -> String {
        if let Some(((start, end), wrap)) = stmt.drop_binding {
            let text = self.render(id, start, end, &stmt.edits, names);
            return if wrap { format!("({text});") } else { format!("{text};") };
        }
        let text = self.render_stmt_body(id, stmt, names);
        match &stmt.shape {
            Shape::Declared { symbol, needs_var: true, .. } => format!("var {};\n{text}", names[&Key::Symbol(id, *symbol)]),
            Shape::Declared { alone: true, .. } => format!("var {text}"),
            _ => text,
        }
    }

    /// As `render_stmt`, without the `var` an enum or namespace declares its name with.
    fn render_stmt_body(&self, id: ModuleId, stmt: &Stmt, names: &HashMap<Key, String>) -> String {
        let mut text = self.render(id, stmt.start, stmt.end, &stmt.edits, names);
        match &stmt.form {
            Form::Plain => {}
            Form::DefaultValue(key) => text = format!("var {} = {text}", names[key]),
            Form::DefaultFunction(key) => {
                if let Some(at) = text.find("function") {
                    let mut i = at + "function".len();
                    let bytes = text.as_bytes();
                    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\n' || bytes[i] == b'*') {
                        i += 1;
                    }
                    // Keep the `*` of a generator before the name.
                    self.insert_text(&mut text, i, &format!("{} ", names[key]));
                }
            }
            Form::Class { key, original, decorated } => {
                if &names[key] != original || *decorated {
                    text = format!("let {} = {text}", names[key]);
                }
            }
        }
        let renamed_class = matches!(stmt.form, Form::Class { .. }) && text.starts_with("let ");
        if (stmt.terminate || renamed_class) && !ends_with_semicolon(&text) {
            text.push(';');
        }
        text.push_str(&self.render_trailer(id, stmt, names));
        text
    }

    /// What follows a decorated class: its `__decorate` calls.
    fn render_trailer(&self, id: ModuleId, stmt: &Stmt, names: &HashMap<Key, String>) -> String {
        let mut out = String::new();
        // A non-empty skip list is what marks a range as read from inside a replacement.
        let sentinel = [std::ptr::null::<Edit>()];
        for piece in &stmt.trailer {
            match piece {
                Piece::Range(from, to) => out.push_str(&self.render_skipping(id, *from, *to, &stmt.edits, names, &sentinel)),
                other => out.push_str(&self.text_of(std::slice::from_ref(other), names)),
            }
        }
        out
    }

    fn emit_chunk(&mut self, chunk: usize, order: &[Item], exports: &[(String, Vec<Piece>)], names: &HashMap<Key, String>, links: &ChunkLinks) -> (String, crate::minify::Layout) {
        let graph = self.graph;
        let format = self.format;
        let mut out = String::new();
        // A CommonJS entry written as CommonJS (or as an IIFE with a global) is its own `module.exports`, as
        // other bundlers write it, not an object with a `default` in it.
        let cjs_entry: Option<ModuleId> = match (format, self.plan.chunks[chunk].roots.as_slice()) {
            (Format::Cjs, [root]) | (Format::Iife, [root]) if graph.entries.contains(root) && (format == Format::Cjs || self.global_name.is_some()) && matches!(self.code.get(root), Some(Code::Wrapped { .. })) => Some(*root),
            _ => None,
        };
        // Where a top-level statement has ended, so the minifier can work on the parts side by side.
        let mut cuts: Vec<usize> = Vec::new();
        let mut head = 0;
        // Externals reach CommonJS and IIFE output through `require` or a global, with the
        // same interop the `import` forms have.
        if format != Format::Esm {
            for key in &links.externals[chunk] {
                if let Key::External(_, ext) = key {
                    match ext {
                        ExtName::Namespace => self.helpers.to_ns = true,
                        ExtName::Default => self.helpers.to_default = true,
                        ExtName::Named(_) => {}
                    }
                }
            }
        }
        match format {
            Format::Cjs => out.push_str("\"use strict\";\n"),
            Format::Iife => {
                match &self.global_name {
                    Some(name) => {
                        let _ = writeln!(out, "var {name} = (() => {{");
                    }
                    None => out.push_str("(() => {\n"),
                }
                head = out.len();
                out.push_str("\"use strict\";\n");
            }
            Format::Esm => {}
        }
        // Imports from outside the bundle come first, as imports are hoisted anyway.
        let mut bare: Vec<&str> = Vec::new();
        for module in &graph.modules {
            if !self.included(module.id) || module.kind != ModuleKind::Esm || !self.in_chunk(module.id, chunk) {
                continue;
            }
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let (Edge::External(spec), RequestKind::Static) = (edge, request.kind) {
                    if !bare.contains(&spec.as_str()) {
                        bare.push(spec);
                    }
                }
            }
        }
        if self.helpers.require_shim && format == Format::Esm {
            out.push_str("import { createRequire as __createRequire } from \"node:module\";\nvar require = __createRequire(import.meta.url);\n");
        }
        if self.helpers.path_shim && format == Format::Esm {
            out.push_str("import { fileURLToPath as __fileURLToPath } from \"node:url\";\nimport { dirname as __pathDirname } from \"node:path\";\nvar __filename = __fileURLToPath(import.meta.url);\nvar __dirname = __pathDirname(__filename);\n");
        }
        if format == Format::Esm {
            for spec in &bare {
                let _ = writeln!(out, "import {};", json_string(spec));
            }
        }
        // Other chunks of this build, in the order they have to run, then outside modules.
        if format == Format::Esm {
            for &dep in &links.deps[chunk] {
                let file = json_string(&format!("./{}", self.plan.chunks[dep].file));
                match links.imports[chunk].get(&dep) {
                    Some(keys) => {
                        let list: Vec<&str> = keys.iter().map(|k| names[k].as_str()).collect();
                        let _ = writeln!(out, "import {{ {} }} from {file};", list.join(", "));
                    }
                    None => {
                        let _ = writeln!(out, "import {file};");
                    }
                }
            }
        }
        for key in &links.externals[chunk] {
            if format != Format::Esm {
                break;
            }
            if let Key::External(spec, ext) = key {
                let name = &names[key];
                let _ = match ext {
                    ExtName::Namespace => writeln!(out, "import * as {name} from {};", json_string(spec)),
                    ExtName::Default => writeln!(out, "import {name} from {};", json_string(spec)),
                    ExtName::Named(n) if n == name => writeln!(out, "import {{ {n} }} from {};", json_string(spec)),
                    ExtName::Named(n) => writeln!(out, "import {{ {} as {name} }} from {};", if is_identifier_name(n) { n.clone() } else { json_string(n) }, json_string(spec)),
                };
            }
        }
        let needs_cjs = self.code.values().any(|c| matches!(c, Code::Wrapped { .. }));
        if needs_cjs {
            out.push_str(HELPER_CJS);
        }
        if self.helpers.esm {
            out.push_str(HELPER_ESM);
        }
        if self.helpers.decorate {
            out.push_str(HELPER_DECORATE);
        }
        if self.helpers.metadata {
            out.push_str(HELPER_METADATA);
        }
        if self.helpers.std_decorators {
            out.push_str(HELPER_STD_DECORATORS);
        }
        if self.helpers.spread {
            out.push_str(HELPER_SPREAD);
        }
        if self.helpers.class_private {
            out.push_str(HELPER_CLASS_PRIVATE);
        }
        if self.helpers.async_fn {
            out.push_str(HELPER_ASYNC);
        }
        if self.helpers.for_await || self.helpers.async_gen {
            out.push_str(HELPER_FOR_AWAIT);
        }
        if self.helpers.async_gen {
            out.push_str(HELPER_ASYNC_GEN);
        }
        if self.code.values().any(|c| matches!(c, Code::Wrapped { require_cases: Some(_), .. })) {
            out.push_str(match format {
                Format::Esm => HELPER_REQ,
                Format::Cjs => HELPER_REQ_CJS,
                Format::Iife => HELPER_REQ_IIFE,
            });
        }
        if self.helpers.to_default {
            out.push_str(HELPER_TO_DEFAULT);
        }
        if self.helpers.to_ns {
            out.push_str(HELPER_TO_NS);
        }
        let has_ns = self.keys.iter().any(|k| matches!(k, Key::Namespace(_)));
        if has_ns {
            out.push_str(HELPER_NS);
        }
        // The temporaries of rewritten syntax (`--target`) that the modules of this chunk use.
        let temps: Vec<&str> = self.temps.iter().filter(|(m, _)| self.in_chunk(*m, chunk)).map(|(_, n)| n.as_str()).collect();
        if !temps.is_empty() {
            let _ = writeln!(out, "var {};", temps.join(", "));
        }
        if format != Format::Esm {
            if format == Format::Cjs {
                for spec in &bare {
                    let _ = writeln!(out, "require({});", json_string(spec));
                }
            }
            for key in &links.externals[chunk] {
                let Key::External(spec, ext) = key else { continue };
                let name = &names[key];
                let source = match format {
                    Format::Cjs => format!("require({})", json_string(spec)),
                    _ => self.globals.iter().find(|(s, _)| s == spec).map(|(_, g)| g.clone()).unwrap_or_else(|| "undefined".to_string()),
                };
                let _ = match ext {
                    ExtName::Namespace => writeln!(out, "var {name} = __toNs({source}, 1);"),
                    ExtName::Default => writeln!(out, "var {name} = __toDefault({source}, 1);"),
                    ExtName::Named(n) if is_identifier_name(n) => writeln!(out, "var {name} = {source}.{n};"),
                    ExtName::Named(n) => writeln!(out, "var {name} = {source}[{}];", json_string(n)),
                };
            }
            // The entry's exports, as getters so they can be defined before the modules run.
            let target = if format == Format::Cjs { "exports" } else { "__exports" };
            // An iife with no global name has nobody to hand its exports to.
            if !exports.is_empty() && cjs_entry.is_none() && (format != Format::Iife || self.global_name.is_some()) {
                out.push_str(HELPER_EXPORT);
                if format == Format::Iife {
                    out.push_str("var __exports = {};\n");
                }
                let fields: Vec<String> = exports
                    .iter()
                    .map(|(name, pieces)| {
                        let label = if is_identifier_name(name) { name.clone() } else { json_string(name) };
                        format!("{label}: () => {}", self.text_of(pieces, names))
                    })
                    .collect();
                let _ = writeln!(out, "__export({target}, {{ {} }});", fields.join(", "));
                if format == Format::Cjs {
                    out.push_str("Object.defineProperty(exports, \"__esModule\", { value: true });\n");
                }
            }
        }
        cuts.push(out.len());
        // CommonJS and JSON modules, each a lazy function.
        for module in &graph.modules {
            if !self.in_chunk(module.id, chunk) {
                continue;
            }
            let Some(Code::Wrapped { edits, json, require_cases }) = self.code.get(&module.id) else { continue };
            let name = &names[&Key::Require(module.id)];
            let body = if *json {
                let trimmed = module.source.trim();
                let from = (trimmed.as_ptr() as usize - module.source.as_ptr() as usize) as u32;
                let mut body = String::from("module.exports = ");
                self.copy(&mut body, module.id, from, from + trimmed.len() as u32);
                body.push(';');
                body
            } else {
                // A hashbang line is only valid at the start of a file.
                let start = if module.source.starts_with("#!") { module.source.find('\n').unwrap_or(module.source.len()) as u32 } else { 0 };
                self.render(module.id, start, module.source.len() as u32, edits, names)
            };
            match require_cases {
                None => {
                    let _ = write!(out, "var {name} = __cjs(function (exports, module) {{\n{body}\n}});\n");
                }
                Some(cases) => {
                    let mut table = String::from("__req((id, r) => { switch (id) { ");
                    for (spec, pieces) in cases {
                        let _ = write!(table, "case {}: return {}; ", json_string(spec), self.text_of(pieces, names));
                    }
                    // Anything else is looked up like Node would from the module's own place.
                    let url = if format != Format::Esm { "0".to_string() } else if self.original_paths { file_url(&module.path) } else { "import.meta.url".to_string() };
                    let _ = write!(table, "}} return r(id); }}, {url})");
                    let _ = write!(out, "var {name} = __cjs(function (exports, module, require) {{\n{body}\n}}, {table});\n");
                }
            }
            cuts.push(out.len());
        }
        // Namespace objects of ES modules: getters, so they can be built before the modules run.
        for key in &self.keys {
            let Key::Namespace(id) = key else { continue };
            if graph.module(*id).kind != ModuleKind::Esm || !self.in_chunk(*id, chunk) {
                continue;
            }
            let mut fields = Vec::new();
            for (name, pieces) in self.ns_entries.get(id).map(|v| v.as_slice()).unwrap_or(&[]) {
                let label = if is_identifier_name(name) { name.clone() } else { json_string(name) };
                fields.push(format!("{label}: () => {}", self.text_of(pieces, names)));
            }
            let _ = writeln!(out, "var {} = __ns({{ {} }});", names[key], fields.join(", "));
            cuts.push(out.len());
        }
        // ES modules that start lazily: hoisted declarations, hoisted functions, an init function.
        for module in &graph.modules {
            if !self.wrapped.contains(&module.id) || !self.in_chunk(module.id, chunk) {
                continue;
            }
            let Some(Code::Esm(code)) = self.code.get(&module.id) else { continue };
            let mut hoisted: Vec<String> = Vec::new();
            let mut functions = String::new();
            let mut body = String::new();
            for dep in module.record.requests.iter().zip(&module.edges).filter_map(|(r, e)| match (e, r.kind) {
                (Edge::Module(t), RequestKind::Static) if self.wrapped.contains(t) => Some(*t),
                _ => None,
            }) {
                let line = format!("{}();\n", names[&Key::Init(dep)]);
                if !body.contains(&line) {
                    body.push_str(&line);
                }
            }
            for stmt in &code.stmts {
                let text = self.render_stmt_body(module.id, stmt, names);
                match (&stmt.shape, &stmt.form) {
                    (Shape::Declared { symbol, needs_var, alone }, _) => {
                        if *needs_var || *alone {
                            let name = names[&Key::Symbol(module.id, *symbol)].clone();
                            if !hoisted.contains(&name) {
                                hoisted.push(name);
                            }
                        }
                        body.push_str(&text);
                        body.push('\n');
                        continue;
                    }
                    (Shape::Function, _) => {
                        functions.push_str(&text);
                        functions.push('\n');
                        continue;
                    }
                    (Shape::Vars(decls), _) => {
                        for &sym in &stmt.declares {
                            let name = names[&Key::Symbol(module.id, sym)].clone();
                            if !hoisted.contains(&name) {
                                hoisted.push(name);
                            }
                        }
                        let parts: Vec<String> = decls
                            .iter()
                            .filter_map(|d| {
                                let init = d.init?;
                                let target = self.render(module.id, d.target.0, d.target.1, &stmt.edits, names);
                                let value = self.render(module.id, init.0, init.1, &stmt.edits, names);
                                Some(if d.object_pattern { format!("({target} = {value})") } else { format!("{target} = {value}") })
                            })
                            .collect();
                        if !parts.is_empty() {
                            body.push_str(&parts.join(", "));
                            body.push_str(";\n");
                        }
                        continue;
                    }
                    _ => {}
                }
                let converted = match &stmt.form {
                    Form::DefaultValue(key) => {
                        let name = names[key].clone();
                        if !hoisted.contains(&name) {
                            hoisted.push(name);
                        }
                        // `render_stmt` wrote `var name = value;`: keep only the assignment.
                        text.strip_prefix("var ").unwrap_or(&text).to_string()
                    }
                    Form::Class { key, .. } => {
                        let name = names[key].clone();
                        if !hoisted.contains(&name) {
                            hoisted.push(name.clone());
                        }
                        // Always an assignment, whether or not the name changed.
                        let raw = self.render(module.id, stmt.start, stmt.end, &stmt.edits, names);
                        let _ = &text;
                        format!("{name} = {raw};{}", self.render_trailer(module.id, stmt, names))
                    }
                    _ => text,
                };
                body.push_str(&converted);
                body.push('\n');
            }
            if !hoisted.is_empty() {
                let _ = writeln!(out, "var {};", hoisted.join(", "));
            }
            out.push_str(&functions);
            let _ = write!(out, "var {} = __esm(() => {{\n{body}}});\n", names[&Key::Init(module.id)]);
            cuts.push(out.len());
        }
        if let Some(pieces) = self.ready_calls.get(&chunk) {
            let _ = writeln!(out, "await {}();", self.text_of(pieces, names));
            cuts.push(out.len());
        }
        for item in order {
            let (Item::Run(m) | Item::RunLazy(m) | Item::RunCommonJs(m)) = item;
            if !self.in_chunk(*m, chunk) {
                continue;
            }
            match item {
                Item::RunCommonJs(id) => {
                    let prefix = match (cjs_entry == Some(*id), format) {
                        (true, Format::Cjs) => "module.exports = ",
                        (true, _) => "return ",
                        _ => "",
                    };
                    let _ = writeln!(out, "{prefix}{}();", names[&Key::Require(*id)]);
                    cuts.push(out.len());
                }
                Item::RunLazy(id) => {
                    let _ = writeln!(out, "{}();", names[&Key::Init(*id)]);
                    cuts.push(out.len());
                }
                Item::Run(id) => {
                    let Some(Code::Esm(code)) = self.code.get(id) else { continue };
                    for stmt in &code.stmts {
                        out.push_str(&self.render_stmt(*id, stmt, names));
                        out.push('\n');
                        cuts.push(out.len());
                    }
                }
            }
        }
        for (root, key, pieces) in &self.alias_values {
            if self.plan.root_chunk.get(root) != Some(&chunk) || cjs_entry == Some(*root) {
                continue;
            }
            let _ = writeln!(out, "var {} = {};", names[key], self.text_of(pieces, names));
            cuts.push(out.len());
        }
        let mut tail = None;
        if format == Format::Iife {
            if self.global_name.is_some() && !exports.is_empty() && cjs_entry.is_none() {
                out.push_str("return __exports;\n");
            }
            tail = Some(out.len());
            out.push_str("})();\n");
        }
        if format == Format::Esm && (!exports.is_empty() || !links.exports[chunk].is_empty()) {
            // The root's own exports, then the names other chunks take from this one.
            let mut list: Vec<String> = exports
                .iter()
                .map(|(name, pieces)| {
                    let local = self.text_of(pieces, names);
                    let exported = if is_identifier_name(name) || name == "default" { name.clone() } else { json_string(name) };
                    if &local == name { local } else { format!("{local} as {exported}") }
                })
                .collect();
            for key in &links.exports[chunk] {
                let entry = names[key].clone();
                if !list.contains(&entry) {
                    list.push(entry);
                }
            }
            let _ = writeln!(out, "export {{ {} }};", list.join(", "));
        }
        let layout = crate::minify::Layout { head, tail: tail.unwrap_or(out.len()), cuts, reserved: if format == Format::Iife { self.global_name.iter().cloned().collect() } else { Vec::new() } };
        (out, layout)
    }
}

// ---- TypeScript that becomes JavaScript ---------------------------------------------------------

impl Scanner<'_, '_> {
    fn eval_enum(&self, id: NodeId, enum_node: NodeId, values: &HashMap<crate::ast::Atom, EnumValue>) -> Option<EnumValue> {
        crate::enums::eval(self.ast, self.semantic, id, enum_node, values)
    }

    /// Whether the top-level enum `node` is the only enum or namespace of its name, so nothing adds to it.
    fn enum_stands_alone(&mut self, node: NodeId) -> bool {
        let NodeKind::TsEnum { name, .. } = self.ast[node].kind else { return false };
        if self.in_wrapper || self.top_decl != Some(node) {
            return false;
        }
        let Some(symbol) = self.semantic.symbol_of(name) else { return false };
        if self.semantic.symbol(symbol).kind != SymbolKind::Var {
            return false;
        }
        if self.shared_names.is_none() {
            let ast = self.ast;
            let mut seen = HashSet::new();
            let mut shared = HashSet::new();
            if let NodeKind::Program(body) = ast[ast.root].kind {
                for &stmt in ast.list(body) {
                    let decl = match ast[stmt].kind {
                        NodeKind::ExportNamed(export) => ast.export(export).decl.unwrap_or(stmt),
                        _ => stmt,
                    };
                    if let NodeKind::TsEnum { name, .. } | NodeKind::TsNamespace { name, .. } = ast[decl].kind {
                        if let Some(symbol) = self.semantic.symbol_of(name) {
                            if !seen.insert(symbol) {
                                shared.insert(symbol);
                            }
                        }
                    }
                }
            }
            self.shared_names = Some(shared);
        }
        !self.shared_names.as_ref().unwrap().contains(&symbol)
    }

    /// `enum E { A = 1, B }` as `((E) => { E[E["A"] = 1] = "A"; ... })(E || (E = {}));`
    fn ts_enum_edit(&mut self, node: NodeId) {
        let NodeKind::TsEnum { name, members } = self.ast[node].kind else { return };
        let alone = self.enum_stands_alone(node);
        let inline_var = self.top_decl != Some(node);
        let target = self.decl_name(name);
        let mut pieces: Vec<Piece> = Vec::new();
        let text = |s: &str| Piece::Text(s.to_string());
        if alone {
            // The statement's `var` is written around this (see `render_stmt`).
            pieces.extend(target.iter().cloned());
            pieces.push(text(" = "));
        } else if inline_var {
            pieces.push(text("var "));
            pieces.extend(target.iter().cloned());
            pieces.push(text(";\n"));
        }
        pieces.push(text("(("));
        pieces.extend(target.iter().cloned());
        pieces.push(text(") => {\n"));
        let mut values: HashMap<crate::ast::Atom, EnumValue> = HashMap::new();
        let mut auto: Option<f64> = Some(0.0);
        let mut previous: Option<String> = None;
        for &member in self.ast.list(members) {
            let NodeKind::TsEnumMember { name: member_name, init } = self.ast[member].kind else { continue };
            let (member_text, atom) = match self.ast[member_name].kind {
                NodeKind::BindingIdent(a) => (self.ast.atom(a).to_string(), Some(a)),
                NodeKind::Str(a) => (self.ast.atom(a).to_string(), None),
                _ => continue,
            };
            let known = match init {
                Some(e) => self.eval_enum(e, node, &values),
                None => auto.map(EnumValue::Num),
            };
            let (value, numeric) = match (&known, init) {
                (Some(EnumValue::Num(n)), _) => (vec![Piece::Text(number_text(*n))], true),
                (Some(EnumValue::Str(t)), _) => (vec![Piece::Text(json_string(t))], false),
                (None, Some(e)) => {
                    let s = self.ast[e].span;
                    (vec![Piece::Range(s.start, s.end)], true)
                }
                (None, None) => {
                    // A member after one whose value is only known at run time.
                    let mut after = target.clone();
                    after.push(Piece::Text(format!("[{}] + 1", json_string(previous.as_deref().unwrap_or("")))));
                    (after, true)
                }
            };
            auto = match &known {
                Some(EnumValue::Num(n)) => Some(n + 1.0),
                _ => None,
            };
            if let (Some(a), Some(v)) = (atom, known) {
                values.insert(a, v);
            }
            let key = json_string(&member_text);
            pieces.push(text("  "));
            pieces.extend(target.iter().cloned());
            if numeric {
                pieces.push(text("["));
                pieces.extend(target.iter().cloned());
                pieces.push(text(&format!("[{key}] = ")));
                pieces.extend(value);
                pieces.push(text(&format!("] = {key};\n")));
            } else {
                pieces.push(text(&format!("[{key}] = ")));
                pieces.extend(value);
                pieces.push(text(";\n"));
            }
            previous = Some(member_text);
        }
        if alone {
            pieces.push(text("  return "));
            pieces.extend(target.iter().cloned());
            pieces.push(text(";\n})({});"));
        } else {
            pieces.push(text("})("));
            pieces.extend(target.iter().cloned());
            pieces.push(text(" || ("));
            pieces.extend(target.iter().cloned());
            pieces.push(text(" = {}));"));
        }
        self.push(node, pieces);
    }

    fn namespace_has_values(&self, node: NodeId) -> bool {
        self.ast.ts_namespace_has_values(node)
    }

    /// `namespace N { export const a = 1; }` as an immediately called function that fills in
    /// `N`, with `N.a = a` after each exported declaration.
    fn ts_namespace_edit(&mut self, node: NodeId) {
        let NodeKind::TsNamespace { name, body } = self.ast[node].kind else { return };
        if !self.namespace_has_values(node) {
            self.push(node, Vec::new());
            return;
        }
        let inline_var = self.top_decl != Some(node);
        let target = self.decl_name(name);
        let text = |s: &str| Piece::Text(s.to_string());
        let mut pieces: Vec<Piece> = Vec::new();
        let declares_var = self.semantic.symbol_of(name).is_none_or(|s| self.semantic.symbol(s).kind == SymbolKind::Var);
        if inline_var && declares_var {
            pieces.push(text("var "));
            pieces.extend(target.iter().cloned());
            pieces.push(text(";\n"));
        }
        pieces.push(text("(function ("));
        pieces.extend(target.iter().cloned());
        pieces.push(text(") {\n"));
        for &item in self.ast.list(body) {
            match self.ast[item].kind {
                NodeKind::TsExport(decl) => {
                    let span = self.ast[decl].span;
                    if matches!(self.ast[decl].kind, NodeKind::TsNamespace { .. }) && !self.namespace_has_values(decl) {
                        continue;
                    }
                    pieces.push(Piece::Range(span.start, span.end));
                    for exported in self.exported_names(decl) {
                        pieces.push(text(";\n"));
                        pieces.extend(target.iter().cloned());
                        pieces.push(text(&format!(".{exported} = {exported};")));
                    }
                    pieces.push(text("\n"));
                }
                NodeKind::Empty => {}
                _ => {
                    let span = self.ast[item].span;
                    pieces.push(Piece::Range(span.start, span.end));
                    pieces.push(text("\n"));
                }
            }
        }
        pieces.push(text("})("));
        pieces.extend(target.iter().cloned());
        pieces.push(text(" || ("));
        pieces.extend(target.iter().cloned());
        pieces.push(text(" = {}));"));
        self.push(node, pieces);
    }

    /// The names a declaration inside a namespace body introduces.
    fn exported_names(&self, decl: NodeId) -> Vec<String> {
        let ast = self.ast;
        let mut idents = Vec::new();
        match ast[decl].kind {
            NodeKind::VarDecl { decls, .. } => {
                for &d in ast.list(decls) {
                    if let NodeKind::VarDeclarator { target, .. } = ast[d].kind {
                        crate::semantic::binding_idents(ast, target, &mut idents);
                    }
                }
            }
            NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } | NodeKind::TsEnum { name: n, .. } | NodeKind::TsNamespace { name: n, .. } => idents.push(n),
            _ => {}
        }
        idents
            .into_iter()
            .filter_map(|i| match ast[i].kind {
                NodeKind::BindingIdent(a) => Some(ast.atom(a).to_string()),
                _ => None,
            })
            .collect()
    }

    /// `constructor(private x)` assigns `this.x = x` before the rest of the body, after
    /// `super(...)` if there is one.
    fn parameter_properties(&mut self, function: NodeId) {
        let props = self.ast.param_props_of(function);
        if props.is_empty() {
            return;
        }
        let NodeKind::Function { body, .. } = self.ast[function].kind else { return };
        let NodeKind::Block(statements) = self.ast[body].kind else { return };
        let super_call = self.ast.list(statements).iter().find(|&&s| match self.ast[s].kind {
            NodeKind::ExprStmt(e) => matches!(self.ast[e].kind, NodeKind::Call { callee, .. } if matches!(self.ast[callee].kind, NodeKind::Super)),
            _ => false,
        });
        let at = match super_call {
            Some(&s) => self.ast[s].span.end,
            None => self.ast[body].span.start + 1,
        };
        let mut text = String::new();
        for &prop in props {
            if let NodeKind::BindingIdent(a) = self.ast[prop].kind {
                let name = self.ast.atom(a);
                text.push_str(&format!("\nthis.{name} = {name};"));
            }
        }
        self.edits.push(Edit { start: at, end: at, pieces: vec![Piece::Text(text)], shorthand: None, transparent: false });
    }
}

// ---- JSX --------------------------------------------------------------------------------------

impl Scanner<'_, '_> {
    /// A dotted name such as `React.createElement`, its first part resolved like any
    /// reference to a module-level name in this file.
    fn dotted_pieces(&mut self, path: &str) -> Vec<Piece> {
        let mut parts = path.split('.');
        let root = parts.next().unwrap_or("");
        let symbol = self.ast.find_atom(root).and_then(|atom| self.semantic.lookup_local(self.semantic.root_scope(), atom));
        let mut pieces = match symbol.and_then(|sym| if self.in_wrapper { None } else { self.symbol_pieces(sym) }) {
            Some(pieces) => pieces,
            None => vec![Piece::Text(root.to_string())],
        };
        for part in parts {
            pieces.push(Piece::Text(format!(".{part}")));
        }
        pieces
    }

    /// The line and column (from 0) at which an element starts in the module's text.
    fn element_place(&mut self, offset: u32) -> (u32, u32) {
        let source = &self.module.source;
        let positions = self.places.get_or_insert_with(|| crate::sourcemap::Positions::new(source));
        positions.at(offset)
    }

    /// One of the automatic runtime's exports (`jsx`, `jsxs`, `jsxDEV`, `Fragment`), as a callable.
    fn jsx_runtime_pieces(&mut self, name: &str) -> Vec<Piece> {
        let module = self.module;
        let graph = self.bundler.graph;
        if let Some(index) = module.record.jsx_runtime {
            match &module.edges[index] {
                Edge::Module(target) => {
                    if let Some(binding) = resolve_export(graph, *target, name) {
                        return self.bundler.binding_pieces(Some(module.id), &binding);
                    }
                }
                Edge::External(specifier) => {
                    let binding = Binding::External { specifier: specifier.clone(), name: ImportName::Named(name.to_string()) };
                    return self.bundler.binding_pieces(Some(module.id), &binding);
                }
                Edge::Unresolved => {}
            }
        }
        vec![Piece::Text(name.to_string())]
    }

    fn attr_name(&self, name: crate::ast::Atom) -> String {
        let text = self.ast.atom(name);
        if is_identifier_name(text) { text.to_string() } else { json_string(text) }
    }

    /// An attribute's value as the pieces of an expression.
    fn attr_value(&self, value: Option<NodeId>) -> Vec<Piece> {
        match value {
            None => vec![Piece::Text("true".into())],
            Some(v) => match self.ast[v].kind {
                NodeKind::Str(a) => vec![Piece::Text(json_string(self.ast.atom(a)))],
                _ => {
                    let span = self.ast[v].span;
                    vec![Piece::Range(span.start, span.end)]
                }
            },
        }
    }

    fn child_pieces(&self, child: NodeId) -> Vec<Piece> {
        match self.ast[child].kind {
            NodeKind::JsxText(a) => vec![Piece::Text(json_string(self.ast.atom(a)))],
            NodeKind::Spread(inner) => {
                let span = self.ast[inner].span;
                vec![Piece::Text("...".into()), Piece::Range(span.start, span.end)]
            }
            _ => {
                let span = self.ast[child].span;
                vec![Piece::Range(span.start, span.end)]
            }
        }
    }

    /// An element or fragment as a call: `_jsx(tag, { props, children })` with the automatic
    /// runtime, `React.createElement(tag, props, ...children)` with the classic one.
    fn jsx_edit(&mut self, node: NodeId) {
        let ast = self.ast;
        let jsx = self.bundler.graph.jsx.clone();
        let (tag, attrs, children) = match ast[node].kind {
            NodeKind::JsxElement(data) => {
                let data = *ast.jsx(data);
                (Some(data.tag), data.attrs, data.children)
            }
            NodeKind::JsxFragment { children } => (None, List::default(), children),
            _ => return,
        };
        let text = |s: &str| Piece::Text(s.to_string());
        let mut tag_pieces: Vec<Piece> = match tag {
            Some(t) => match ast[t].kind {
                NodeKind::Str(a) => vec![Piece::Text(json_string(ast.atom(a)))],
                _ => {
                    let span = ast[t].span;
                    vec![Piece::Range(span.start, span.end)]
                }
            },
            None if jsx.automatic => self.jsx_runtime_pieces("Fragment"),
            None => self.dotted_pieces(&jsx.fragment),
        };
        let child_ids: Vec<NodeId> = ast.list(children).to_vec();
        let attr_ids: Vec<NodeId> = ast.list(attrs).to_vec();
        let mut pieces: Vec<Piece> = Vec::new();
        if jsx.automatic {
            // `key` is passed on its own; everything else, and the children, is `props`.
            let mut key: Option<Vec<Piece>> = None;
            let mut entries: Vec<Vec<Piece>> = Vec::new();
            let mut after_spread = false;
            for &a in &attr_ids {
                match ast[a].kind {
                    NodeKind::JsxAttr { name, value } => {
                        if ast.atom(name) == "key" && !after_spread && value.is_some() {
                            key = Some(self.attr_value(value));
                            continue;
                        }
                        let mut entry = vec![Piece::Text(format!("{}: ", self.attr_name(name)))];
                        entry.extend(self.attr_value(value));
                        entries.push(entry);
                    }
                    NodeKind::JsxSpreadAttr(expr) => {
                        after_spread = true;
                        let span = ast[expr].span;
                        entries.push(vec![Piece::Text("...".into()), Piece::Range(span.start, span.end)]);
                    }
                    _ => {}
                }
            }
            let function = match child_ids.len() {
                _ if jsx.dev => "jsxDEV",
                0 | 1 => "jsx",
                _ => "jsxs",
            };
            match child_ids.len() {
                0 => {}
                1 => {
                    let mut entry = vec![text("children: ")];
                    entry.extend(self.child_pieces(child_ids[0]));
                    entries.push(entry);
                }
                _ => {
                    let mut entry = vec![text("children: [")];
                    for (i, &c) in child_ids.iter().enumerate() {
                        if i > 0 {
                            entry.push(text(", "));
                        }
                        entry.extend(self.child_pieces(c));
                    }
                    entry.push(text("]"));
                    entries.push(entry);
                }
            }
            pieces.extend(self.jsx_runtime_pieces(function));
            pieces.push(text("("));
            pieces.append(&mut tag_pieces);
            pieces.push(text(", "));
            if entries.is_empty() {
                pieces.push(text("{}"));
            } else {
                pieces.push(text("{ "));
                for (i, entry) in entries.into_iter().enumerate() {
                    if i > 0 {
                        pieces.push(text(", "));
                    }
                    pieces.extend(entry);
                }
                pieces.push(text(" }"));
            }
            if jsx.dev {
                // jsxDEV(type, props, key, isStaticChildren, { fileName, lineNumber, columnNumber }, this): where the element is written.
                let (line, column) = self.element_place(ast[node].span.start);
                pieces.push(text(", "));
                match key {
                    Some(key) => pieces.extend(key),
                    None => pieces.push(text("void 0")),
                }
                let file = json_string(&self.module.path.to_string_lossy());
                pieces.push(text(&format!(", {}, {{ fileName: {file}, lineNumber: {}, columnNumber: {} }}, this", child_ids.len() > 1, line + 1, column + 1)));
            } else if let Some(key) = key {
                pieces.push(text(", "));
                pieces.extend(key);
            }
            pieces.push(text(")"));
        } else {
            pieces.extend(self.dotted_pieces(&jsx.factory));
            pieces.push(text("("));
            pieces.append(&mut tag_pieces);
            pieces.push(text(", "));
            if attr_ids.is_empty() {
                pieces.push(text("null"));
            } else {
                pieces.push(text("{ "));
                for (i, &a) in attr_ids.iter().enumerate() {
                    if i > 0 {
                        pieces.push(text(", "));
                    }
                    match ast[a].kind {
                        NodeKind::JsxAttr { name, value } => {
                            pieces.push(Piece::Text(format!("{}: ", self.attr_name(name))));
                            pieces.extend(self.attr_value(value));
                        }
                        NodeKind::JsxSpreadAttr(expr) => {
                            let span = ast[expr].span;
                            pieces.push(text("..."));
                            pieces.push(Piece::Range(span.start, span.end));
                        }
                        _ => {}
                    }
                }
                pieces.push(text(" }"));
            }
            for &c in &child_ids {
                pieces.push(text(", "));
                pieces.extend(self.child_pieces(c));
            }
            pieces.push(text(")"));
        }
        self.push(node, pieces);
    }
}

// ---- decorators ---------------------------------------------------------------------------------

impl Scanner<'_, '_> {
    fn class_is_decorated(&self, class: NodeId) -> bool {
        let ast = self.ast;
        if !ast.decorators_of(class).is_empty() {
            return true;
        }
        let NodeKind::Class { members, .. } = ast[class].kind else { return false };
        ast.list(members).iter().any(|&m| {
            if !ast.decorators_of(m).is_empty() {
                return true;
            }
            match ast[m].kind {
                NodeKind::ClassMethod { value, .. } => match ast[value].kind {
                    NodeKind::Function { params, .. } => ast.list(params).iter().any(|&p| !ast.decorators_of(p).is_empty()),
                    _ => false,
                },
                _ => false,
            }
        })
    }

    /// Decorators are cut out of the class and applied after it, as TypeScript's
    /// `experimentalDecorators` does: member decorators first, then the class's own.
    fn decorator_trailer(&mut self, class: NodeId, class_key: &Key) -> Vec<Piece> {
        let ast = self.ast;
        let text = |s: &str| Piece::Text(s.to_string());
        self.bundler.helpers.decorate = true;
        let NodeKind::Class { members, .. } = ast[class].kind else { return Vec::new() };
        // Instance members are decorated before static ones, each in source order.
        let mut pieces: Vec<Piece> = Vec::new();
        let mut static_pieces: Vec<Piece> = Vec::new();
        let mut class_entries: Vec<Vec<Piece>> = Vec::new();
        for d in ast.decorators_of(class) {
            let span = ast[d.expr].span;
            class_entries.push(vec![Piece::Range(span.start, span.end)]);
        }
        // `__param(index, decorator)` for each decorated parameter, in parameter order.
        let params_of = |this: &Self, function: NodeId| -> Vec<Vec<Piece>> {
            let mut entries = Vec::new();
            if let NodeKind::Function { params, .. } = this.ast[function].kind {
                for (index, &p) in this.ast.list(params).iter().enumerate() {
                    for d in this.ast.decorators_of(p) {
                        let span = this.ast[d.expr].span;
                        entries.push(vec![Piece::Text(format!("__param({index}, ")), Piece::Range(span.start, span.end), Piece::Text(")".to_string())]);
                    }
                }
            }
            entries
        };
        // The decorators are cut from the class text wherever they stand inside it.
        let mut removals: Vec<crate::ast::Span> = ast.decorators_of(class).iter().map(|d| d.span).collect();
        for &m in ast.list(members) {
            let mut entries: Vec<Vec<Piece>> = Vec::new();
            for d in ast.decorators_of(m) {
                let span = ast[d.expr].span;
                entries.push(vec![Piece::Range(span.start, span.end)]);
                removals.push(d.span);
            }
            let (key_node, value, is_ctor) = match ast[m].kind {
                NodeKind::ClassMethod { key, value, kind } => (key, Some(value), kind == crate::ast::MethodKind::Constructor),
                NodeKind::ClassField { key, .. } => (key, None, false),
                _ => continue,
            };
            if let Some(function) = value {
                if let NodeKind::Function { params, .. } = ast[function].kind {
                    for &p in ast.list(params) {
                        for d in ast.decorators_of(p) {
                            removals.push(d.span);
                        }
                    }
                }
                let params = params_of(self, function);
                if is_ctor {
                    class_entries.extend(params);
                    continue;
                }
                entries.extend(params);
            }
            if entries.is_empty() {
                continue;
            }
            if self.bundler.graph.decorator_metadata {
                entries.extend(self.member_metadata(members, m));
            }
            let computed = ast[m].has(flags::COMPUTED);
            let key_text = match ast[key_node].kind {
                NodeKind::PropName(a) | NodeKind::Str(a) if !computed => json_string(ast.atom(a)),
                NodeKind::Num(n) if !computed => json_string(&number_text(n.get())),
                _ => {
                    self.bundler.errors.push(Diagnostic {
                        severity: crate::graph::Severity::Error,
                        message: "Decorators on computed or private members are not supported yet".to_string(),
                        file: Some(self.module.path.clone()),
                        pos: Some(ast[m].span.start),
                    });
                    continue;
                }
            };
            let is_static = ast[m].has(flags::STATIC);
            let descriptor = if matches!(ast[m].kind, NodeKind::ClassField { .. }) { "void 0" } else { "null" };
            let target = if is_static { &mut static_pieces } else { &mut pieces };
            target.push(text("\n__decorate(["));
            for (i, entry) in entries.into_iter().enumerate() {
                if i > 0 {
                    target.push(text(", "));
                }
                target.extend(entry);
            }
            target.push(text("], "));
            target.push(Piece::Name(class_key.clone()));
            target.push(text(&format!("{}, {key_text}, {descriptor});", if is_static { "" } else { ".prototype" })));
        }
        pieces.extend(static_pieces);
        if !class_entries.is_empty() && self.bundler.graph.decorator_metadata {
            // The constructor's parameter types, for a class that is decorated at all.
            let ctor = ast.list(members).iter().find_map(|&m| match ast[m].kind {
                NodeKind::ClassMethod { value, kind: crate::ast::MethodKind::Constructor, .. } => Some(value),
                _ => None,
            });
            if let Some(function) = ctor {
                let list = self.param_types_list(function);
                class_entries.push(self.metadata_entry("design:paramtypes", list));
            }
        }
        if !class_entries.is_empty() {
            pieces.push(text("\n"));
            pieces.push(Piece::Name(class_key.clone()));
            pieces.push(text(" = __decorate(["));
            for (i, entry) in class_entries.into_iter().enumerate() {
                if i > 0 {
                    pieces.push(text(", "));
                }
                pieces.extend(entry);
            }
            pieces.push(text("], "));
            pieces.push(Piece::Name(class_key.clone()));
            pieces.push(text(");"));
        }
        for span in removals {
            self.edits.push(Edit { start: span.start, end: span.end, pieces: Vec::new(), shorthand: None, transparent: true });
        }
        // A decorator's expression may stand outside the class's own text, and still holds
        // TypeScript syntax of its own (`(config: Config) => ...`).
        let ranges: Vec<(u32, u32)> = pieces.iter().filter_map(|p| if let Piece::Range(a, b) = p { Some((*a, *b)) } else { None }).collect();
        for (from, to) in ranges {
            self.add_erasures(from, to);
        }
        pieces
    }
}

// ---- decorator metadata -----------------------------------------------------------------------

impl Scanner<'_, '_> {
    /// A name from a type annotation, as a reference in the output.
    fn reference_pieces(&mut self, node: NodeId) -> Vec<Piece> {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Ident(a) => {
                let name = ast.atom(a).to_string();
                match self.semantic.symbol_of(node) {
                    Some(sym) if self.semantic.symbol(sym).scope == self.semantic.root_scope() && !self.in_wrapper => {
                        self.symbol_pieces(sym).unwrap_or_else(|| vec![Piece::Text(name)])
                    }
                    _ => vec![Piece::Text(name)],
                }
            }
            NodeKind::Member { object, prop } => {
                let mut pieces = self.reference_pieces(object);
                if let NodeKind::PropName(a) = ast[prop].kind {
                    pieces.push(Piece::Text(format!(".{}", ast.atom(a))));
                }
                pieces
            }
            _ => vec![Piece::Text("Object".to_string())],
        }
    }

    /// How a type is serialized: `String`, `Array`, or the class itself when it exists.
    fn type_meta_pieces(&mut self, meta: crate::ast::TypeMeta) -> Vec<Piece> {
        use crate::ast::TypeMeta;
        let text = |s: &str| vec![Piece::Text(s.to_string())];
        match meta {
            TypeMeta::Object => text("Object"),
            TypeMeta::Function => text("Function"),
            TypeMeta::Array => text("Array"),
            TypeMeta::String => text("String"),
            TypeMeta::Number => text("Number"),
            TypeMeta::Boolean => text("Boolean"),
            TypeMeta::Symbol => text("Symbol"),
            TypeMeta::BigInt => text("BigInt"),
            TypeMeta::Void => text("void 0"),
            TypeMeta::Ref(node) => {
                // The name may not exist at run time (an interface), then it is `Object`.
                let reference = self.reference_pieces(node);
                let mut pieces = text("typeof ");
                pieces.extend(reference.iter().cloned());
                pieces.push(Piece::Text(" === \"function\" ? ".to_string()));
                pieces.extend(reference);
                pieces.push(Piece::Text(" : Object".to_string()));
                pieces
            }
        }
    }

    fn metadata_entry(&mut self, key: &str, value: Vec<Piece>) -> Vec<Piece> {
        self.bundler.helpers.metadata = true;
        let mut pieces = vec![Piece::Text(format!("__metadata(\"{key}\", "))];
        pieces.extend(value);
        pieces.push(Piece::Text(")".to_string()));
        pieces
    }

    /// `[String, Foo, Object]` for the parameters of `function`.
    fn param_types_list(&mut self, function: NodeId) -> Vec<Piece> {
        let ast = self.ast;
        let NodeKind::Function { params, .. } = ast[function].kind else { return vec![Piece::Text("[]".to_string())] };
        let mut pieces = vec![Piece::Text("[".to_string())];
        for (i, &p) in ast.list(params).iter().enumerate() {
            if i > 0 {
                pieces.push(Piece::Text(", ".to_string()));
            }
            let target = match ast[p].kind {
                NodeKind::AssignPattern { target, .. } => target,
                NodeKind::Rest(target) => target,
                _ => p,
            };
            let meta = ast.type_meta_of(target).unwrap_or(crate::ast::TypeMeta::Object);
            pieces.extend(self.type_meta_pieces(meta));
        }
        pieces.push(Piece::Text("]".to_string()));
        pieces
    }

    /// The `design:*` entries TypeScript adds to a decorated member.
    fn member_metadata(&mut self, members: crate::ast::List, member: NodeId) -> Vec<Vec<Piece>> {
        use crate::ast::{MethodKind, TypeMeta};
        let ast = self.ast;
        let mut entries = Vec::new();
        match ast[member].kind {
            NodeKind::ClassField { .. } => {
                let meta = ast.type_meta_of(member).unwrap_or(TypeMeta::Object);
                let value = self.type_meta_pieces(meta);
                entries.push(self.metadata_entry("design:type", value));
            }
            NodeKind::ClassMethod { key, value: function, kind } => match kind {
                MethodKind::Method => {
                    entries.push(self.metadata_entry("design:type", vec![Piece::Text("Function".to_string())]));
                    let list = self.param_types_list(function);
                    entries.push(self.metadata_entry("design:paramtypes", list));
                    let returns = match ast.type_meta_of(function) {
                        Some(meta) => self.type_meta_pieces(meta),
                        // An async method without a return type returns a promise.
                        None if ast[function].has(flags::ASYNC) => {
                            let promise = Piece::Text("Promise".to_string());
                            vec![Piece::Text("typeof ".to_string()), promise.clone(), Piece::Text(" === \"function\" ? ".to_string()), promise, Piece::Text(" : Object".to_string())]
                        }
                        None => vec![Piece::Text("void 0".to_string())],
                    };
                    entries.push(self.metadata_entry("design:returntype", returns));
                }
                MethodKind::Get | MethodKind::Set => {
                    // A getter and a setter are one property: the setter's parameter gives its type.
                    let setter = if kind == MethodKind::Set {
                        Some(function)
                    } else {
                        let name = match ast[key].kind {
                            NodeKind::PropName(a) | NodeKind::Str(a) => Some(a),
                            _ => None,
                        };
                        ast.list(members).iter().find_map(|&other| match ast[other].kind {
                            NodeKind::ClassMethod { key: k2, value, kind: MethodKind::Set } if matches!(ast[k2].kind, NodeKind::PropName(a) | NodeKind::Str(a) if Some(a) == name) => Some(value),
                            _ => None,
                        })
                    };
                    let type_meta = match (kind, setter) {
                        (MethodKind::Get, _) => ast.type_meta_of(function),
                        (_, Some(f)) => match ast[f].kind {
                            NodeKind::Function { params, .. } => ast.list(params).first().and_then(|&p| {
                                let target = match ast[p].kind {
                                    NodeKind::AssignPattern { target, .. } => target,
                                    _ => p,
                                };
                                ast.type_meta_of(target)
                            }),
                            _ => None,
                        },
                        _ => None,
                    };
                    let value = self.type_meta_pieces(type_meta.unwrap_or(TypeMeta::Object));
                    entries.push(self.metadata_entry("design:type", value));
                    if let Some(f) = setter {
                        let list = self.param_types_list(f);
                        entries.push(self.metadata_entry("design:paramtypes", list));
                    }
                }
                MethodKind::Constructor => {}
            },
            _ => {}
        }
        entries
    }
}
