//! Plugins: Rollup/Vite plugins, esbuild plugins and webpack loaders, run by a Node process.
//!
//! The plugins are JavaScript, so they run where JavaScript does: [`PluginHost`] starts Node
//! with `plugins/host.mjs` (embedded in this binary) and talks to it over a pipe, one JSON
//! message per line. The host turns every kind of plugin into Rollup's hooks; this side calls
//! the hooks at the right points of the build ([`crate::graph`] for `resolveId`, `load` and
//! `transform`, the driver for `renderChunk` and `generateBundle`).
//!
//! Without a config that lists plugins nothing here runs and Node is not needed.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use crate::bundle::ChunkInfo;
use crate::graph::{Edge, Graph};
use crate::json::Json;
use crate::loader::quote;

const HOST: &str = concat!(include_str!("plugins/host.mjs"), "\n", include_str!("plugins/webpack.mjs"), "\n", include_str!("plugins/parcel.mjs"), "\n", include_str!("plugins/server.mjs"));

/// What the host found out when it loaded the configuration.
#[derive(Debug, Default, Clone)]
pub struct HostInfo {
    pub plugins: Vec<String>,
    pub hooks: HashSet<String>,
    /// `define` entries from the config (and `DefinePlugin`), as `(key, expression)`.
    pub define: Vec<(String, String)>,
    /// Entry points the config names (Rollup's `input`).
    pub input: Vec<String>,
    /// The options of a module federation plugin in the config, as JSON.
    pub federation: Option<Json>,
}

/// The command line settings plugins may look at.
#[derive(Debug, Default, Clone)]
pub struct CliInfo {
    pub entries: Vec<String>,
    pub outdir: Option<String>,
    pub outfile: Option<String>,
    pub format: String,
    pub minify: bool,
    pub sourcemap: bool,
    pub splitting: bool,
    pub external: Vec<String>,
    pub loaders: Vec<(String, String)>,
    pub platform_browser: bool,
    pub mode: String,
}

type ResolverFn = dyn Fn(&str, Option<&str>, &str) -> Option<(String, bool)> + Send + Sync;
/// Builds an entry on its own and returns the code: `(entry, global name)`. Child compilers of webpack plugins use it.
type ChildBuilder = dyn Fn(&str, &str) -> Result<(String, Vec<(String, Vec<u8>)>), String> + Send + Sync;
type Pending = Mutex<HashMap<String, Sender<Result<Json, String>>>>;

struct Shared {
    pending: Pending,
    resolver: Mutex<Option<Arc<ResolverFn>>>,
    child_builder: Mutex<Option<Arc<ChildBuilder>>>,
    /// Where messages plugins send to the page (`server.ws.send`) go.
    page_sink: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
    stdin: Mutex<ChildStdin>,
}

pub struct PluginHost {
    shared: Arc<Shared>,
    child: Mutex<Child>,
    next: AtomicU64,
    pub info: HostInfo,
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost").field("plugins", &self.info.plugins).finish()
    }
}

/// A plugin's answer to `resolveId`.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub id: String,
    pub external: bool,
    pub side_effects: Option<bool>,
}

/// A plugin's answer to `load`.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub code: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub map: Option<String>,
    pub module_type: Option<String>,
    pub resolve_dir: Option<String>,
    pub side_effects: Option<bool>,
    /// For a style sheet: the JavaScript the module stands for (its exports), instead of the usual.
    pub js_face: Option<String>,
}

/// What `transform` made of a module's text.
#[derive(Debug, Clone)]
pub struct Transformed {
    pub code: String,
    pub map: Option<String>,
    pub module_type: Option<String>,
}

/// A file of output as plugins see it.
#[derive(Debug, Clone)]
pub struct BundleFile {
    pub file_name: String,
    pub is_chunk: bool,
    pub name: Option<String>,
    pub is_entry: bool,
    /// The chunk's code or the asset's text; empty for binary assets.
    pub code: String,
    pub bytes: Option<Vec<u8>>,
    pub map: Option<String>,
    /// For a chunk: what it is made of.
    pub info: Option<ChunkInfo>,
}

/// A chunk a plugin asked for with `this.emitFile({ type: 'chunk' })`.
#[derive(Debug, Clone)]
pub struct EmittedChunk {
    pub reference: String,
    /// The resolved id of the module the chunk starts from.
    pub id: String,
    /// The name of the chunk's file, decided when the plugin asked.
    pub file_name: String,
}

/// A chunk as Rollup's `OutputChunk` describes it (without the code).
fn chunk_json(file_name: &str, is_entry: bool, info: Option<&ChunkInfo>) -> String {
    let name = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let Some(info) = info else {
        return format!("{{\"fileName\":{},\"name\":{},\"isEntry\":{is_entry},\"type\":\"chunk\",\"exports\":[],\"imports\":[],\"dynamicImports\":[],\"modules\":{{}}}}", quote(file_name), quote(name));
    };
    let modules: Vec<String> = info.modules.iter().map(|(id, len)| format!("{}:{{\"renderedLength\":{len},\"originalLength\":{len},\"removedExports\":[],\"renderedExports\":[],\"code\":null}}", quote(id))).collect();
    let ids: Vec<String> = info.modules.iter().map(|(id, _)| id.clone()).collect();
    format!(
        "{{\"type\":\"chunk\",\"fileName\":{},\"name\":{},\"isEntry\":{},\"isDynamicEntry\":{},\"isImplicitEntry\":false,\"facadeModuleId\":{},\"exports\":{},\"imports\":{},\"dynamicImports\":{},\"importedBindings\":{{}},\"referencedFiles\":[],\"implicitlyLoadedBefore\":[],\"modules\":{{{}}},\"moduleIds\":{},\"preliminaryFileName\":{}}}",
        quote(file_name),
        quote(&info.name),
        info.is_entry,
        info.is_dynamic_entry,
        opt_str(info.facade_module.as_deref()),
        string_list(&info.exports),
        string_list(&info.imports),
        string_list(&info.dynamic_imports),
        modules.join(","),
        string_list(&ids),
        quote(file_name)
    )
}

fn opt_str(value: Option<&str>) -> String {
    value.map_or("null".to_string(), quote)
}

fn string_list(values: &[String]) -> String {
    format!("[{}]", values.iter().map(|s| quote(s)).collect::<Vec<_>>().join(","))
}

impl PluginHost {
    /// Starts the host and loads `config` (relative to `root`). Fails with a message that says what
    /// to fix: Node missing, a config that does not load.
    pub fn start(root: &Path, config: Option<&str>, cli: &CliInfo) -> Result<Arc<PluginHost>, String> {
        // The host is too long for a command line argument, so it goes in a file: in a private folder, created
        // new, so another user of the machine cannot swap it for theirs or point it somewhere with a link.
        let host_dir = std::env::temp_dir().join(format!("rathkit-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos())));
        let host_file = host_dir.join("host.mjs");
        {
            let mut dir = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
            dir.create(&host_dir).map_err(|e| format!("cannot make {}: {e}", host_dir.display()))?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(&host_file).map_err(|e| format!("cannot write the plugin host to {}: {e}", host_file.display()))?;
            file.write_all(HOST.as_bytes()).map_err(|e| format!("cannot write the plugin host to {}: {e}", host_file.display()))?;
        }
        let child = Command::new(std::env::var_os("RATHKIT_NODE").unwrap_or_else(|| "node".into()))
            .arg("--no-warnings")
            .arg(&host_file)
            .current_dir(root)
            // `this.parse` in plugins runs this same program (`--estree`).
            .env("RATHKIT_EXE", std::env::current_exe().unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("plugins need Node.js, which could not be started ({e}); install Node or set RATHKIT_NODE"));
        // Node has read the file by the time it answers; it is not needed after that.
        let mut child = child?;
        let stdin = child.stdin.take().ok_or("no stdin for the plugin host")?;
        let stdout = child.stdout.take().ok_or("no stdout for the plugin host")?;
        let shared = Arc::new(Shared { pending: Mutex::new(HashMap::new()), resolver: Mutex::new(None), child_builder: Mutex::new(None), page_sink: Mutex::new(None), stdin: Mutex::new(stdin) });
        let (ready_tx, ready_rx) = channel::<()>();
        let reader_shared = shared.clone();
        std::thread::spawn(move || {
            let mut ready = Some(ready_tx);
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = Json::parse(&line) else { continue };
                if message.get("ready").is_some() {
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(());
                    }
                } else if message.get("event").and_then(Json::as_str) == Some("ws") {
                    let sink = reader_shared.page_sink.lock().expect("sink").clone();
                    if let (Some(sink), Some(payload)) = (sink, message.get("payload").and_then(Json::as_str)) {
                        sink(payload);
                    }
                } else if let Some(level) = message.get("log").and_then(Json::as_str) {
                    let text = message.get("message").and_then(Json::as_str).unwrap_or("");
                    match level {
                        "debug" => {}
                        "error" => eprintln!("error: {text}"),
                        _ => eprintln!("warning: {text}"),
                    }
                } else if let Some(method) = message.get("method").and_then(Json::as_str) {
                    let id = message.get("id").and_then(Json::as_str).unwrap_or("").to_string();
                    if method == "bundleEntry" {
                        // A whole build, which asks the host for load and transform: it must not hold up this thread.
                        let shared = reader_shared.clone();
                        let params = message.get("params").cloned();
                        std::thread::spawn(move || {
                            let reply = answer(&shared, "bundleEntry", params.as_ref());
                            reply_to_host(&shared, &id, reply);
                        });
                    } else {
                        let reply = answer(&reader_shared, method, message.get("params"));
                        reply_to_host(&reader_shared, &id, reply);
                    }
                } else if let Some(id) = message.get("id").and_then(Json::as_str) {
                    let slot = reader_shared.pending.lock().expect("pending").remove(id);
                    if let Some(tx) = slot {
                        let outcome = match message.get("error") {
                            Some(error) => Err(describe_error(error)),
                            None => Ok(message.get("result").cloned().unwrap_or(Json::Null)),
                        };
                        let _ = tx.send(outcome);
                    }
                }
            }
            // The host is gone: nobody is going to answer.
            for (_, tx) in reader_shared.pending.lock().expect("pending").drain() {
                let _ = tx.send(Err("the plugin host stopped".to_string()));
            }
        });
        let ready = ready_rx.recv_timeout(std::time::Duration::from_secs(30)).map_err(|_| "the plugin host did not start".to_string());
        let _ = std::fs::remove_file(&host_file);
        let _ = std::fs::remove_dir(&host_dir);
        ready?;
        let mut host = PluginHost { shared, child: Mutex::new(child), next: AtomicU64::new(1), info: HostInfo::default() };
        let params = format!(
            "{{\"root\":{},\"config\":{},\"cli\":{{\"entries\":{},\"outdir\":{},\"outfile\":{},\"format\":{},\"minify\":{},\"sourcemap\":{},\"splitting\":{},\"external\":{},\"loaders\":[{}],\"platformBrowser\":{},\"mode\":{}}}}}",
            quote(&root.to_string_lossy()),
            opt_str(config),
            string_list(&cli.entries),
            opt_str(cli.outdir.as_deref()),
            opt_str(cli.outfile.as_deref()),
            quote(&cli.format),
            cli.minify,
            cli.sourcemap,
            cli.splitting,
            string_list(&cli.external),
            cli.loaders.iter().map(|(a, b)| format!("[{},{}]", quote(a), quote(b))).collect::<Vec<_>>().join(","),
            cli.platform_browser,
            quote(if cli.mode.is_empty() { "production" } else { &cli.mode }),
        );
        let result = host.call("init", &params)?;
        let strings = |key: &str| -> Vec<String> { result.get(key).and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        host.info.plugins = strings("plugins");
        host.info.input = strings("input");
        host.info.federation = result.get("federation").filter(|f| !matches!(f, Json::Null)).cloned();
        if let Some(Json::Object(hooks)) = result.get("hooks") {
            host.info.hooks = hooks.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect();
        }
        if let Some(defines) = result.get("define").and_then(Json::as_array) {
            for pair in defines {
                if let Some([k, v]) = pair.as_array() {
                    if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                        host.info.define.push((k.to_string(), v.to_string()));
                    }
                }
            }
        }
        Ok(Arc::new(host))
    }

    /// Where `server.ws.send` messages from plugins go (the dev server's pages).
    pub fn set_page_sink(&self, sink: Option<Arc<dyn Fn(&str) + Send + Sync>>) {
        *self.shared.page_sink.lock().expect("sink") = sink;
    }

    /// `configureServer` for the plugins; whether any added a middleware.
    pub fn configure_server(&self, port: u16, host: &str) -> Result<bool, String> {
        let result = self.call("configureServer", &format!("{{\"port\":{port},\"host\":{}}}", quote(host)))?;
        Ok(result.get("middlewares").and_then(Json::as_bool).unwrap_or(false))
    }

    /// A request through the plugins' middlewares: `(status, headers, body)` if one answered.
    pub fn serve_request(&self, method: &str, url: &str, headers: &[(String, String)], body: &[u8]) -> Result<Option<(u16, Vec<(String, String)>, Vec<u8>)>, String> {
        let list: Vec<String> = headers.iter().map(|(k, v)| format!("{}:{}", quote(k), quote(v))).collect();
        let result = self.call("serveRequest", &format!("{{\"method\":{},\"url\":{},\"headers\":{{{}}},\"body\":{}}}", quote(method), quote(url), list.join(","), quote(&crate::loader::base64(body))))?;
        if result.get("handled").and_then(Json::as_bool) != Some(true) {
            return Ok(None);
        }
        let status = result.get("status").and_then(Json::as_f64).unwrap_or(200.0) as u16;
        let mut out = Vec::new();
        if let Some(Json::Object(map)) = result.get("headers") {
            for (k, v) in map {
                if let Some(v) = v.as_str() {
                    out.push((k.clone(), v.to_string()));
                }
            }
        }
        let body = crate::loader::base64_decode(result.get("body").and_then(Json::as_str).unwrap_or(""));
        Ok(Some((status, out, body)))
    }

    /// Plugins' `transformIndexHtml`.
    pub fn transform_index_html(&self, html: &str, url: &str) -> Result<String, String> {
        let result = self.call("transformIndexHtml", &format!("{{\"html\":{},\"path\":{}}}", quote(html), quote(url)))?;
        Ok(result.as_str().map(str::to_string).unwrap_or_else(|| html.to_string()))
    }

    /// Plugins' `handleHotUpdate` for a changed file; true if one of them stopped the page from reloading.
    pub fn hot_update(&self, file: &str, event: &str) -> Result<bool, String> {
        let result = self.call("hotUpdate", &format!("{{\"file\":{},\"event\":{}}}", quote(file), quote(event)))?;
        Ok(result.get("suppressed").and_then(Json::as_bool).unwrap_or(false))
    }

    /// A message the page sent with `import.meta.hot.send`, as the JSON text `{"event":..,"data":..}`.
    pub fn page_message(&self, raw: &str) -> Result<(), String> {
        self.call("wsMessage", &format!("{{\"raw\":{}}}", quote(raw))).map(|_| ())
    }

    /// Lets plugins compile an entry on its own (webpack child compilers).
    pub fn set_child_builder(&self, builder: Option<Arc<ChildBuilder>>) {
        *self.shared.child_builder.lock().expect("builder") = builder;
    }

    pub fn has(&self, hook: &str) -> bool {
        self.info.hooks.contains(hook)
    }

    /// Lets plugins' `this.resolve` and the host's fall-through use the bundler's resolver:
    /// `(specifier, importer, kind)` -> `(path, external)`.
    pub fn set_resolver(&self, resolver: Option<Arc<ResolverFn>>) {
        *self.shared.resolver.lock().expect("resolver") = resolver;
    }

    fn call(&self, method: &str, params: &str) -> Result<Json, String> {
        let id = format!("r{}", self.next.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = channel();
        self.shared.pending.lock().expect("pending").insert(id.clone(), tx);
        {
            let mut stdin = self.shared.stdin.lock().expect("stdin");
            writeln!(stdin, "{{\"id\":{},\"method\":{},\"params\":{params}}}", quote(&id), quote(method)).map_err(|e| format!("the plugin host is gone: {e}"))?;
            stdin.flush().map_err(|e| format!("the plugin host is gone: {e}"))?;
        }
        rx.recv().map_err(|_| "the plugin host stopped".to_string())?
    }

    pub fn build_start(&self) -> Result<(), String> {
        if self.has("buildStart") { self.call("buildStart", "{}").map(|_| ()) } else { Ok(()) }
    }

    pub fn build_end(&self, error: Option<&str>) -> Result<(), String> {
        if self.has("buildEnd") { self.call("buildEnd", &format!("{{\"error\":{}}}", opt_str(error))).map(|_| ()) } else { Ok(()) }
    }

    /// Files plugins asked to be watched (`this.addWatchFile`).
    pub fn watch_files(&self) -> Vec<std::path::PathBuf> {
        match self.call("watchFiles", "{}") {
            Ok(list) => list.as_array().unwrap_or(&[]).iter().filter_map(|v| v.as_str()).map(std::path::PathBuf::from).filter(|p| p.is_file()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `this.meta.watchMode` for the plugins.
    pub fn set_watch_mode(&self, on: bool) {
        let _ = self.call("watchMode", &format!("{{\"on\":{on}}}"));
    }

    /// Files changed while watching: what plugins cached about them is dropped and `watchChange` runs.
    pub fn watch_change(&self, changes: &[(String, &str)]) -> Result<(), String> {
        let items: Vec<String> = changes.iter().map(|(id, event)| format!("{{\"id\":{},\"event\":{}}}", quote(id), quote(event))).collect();
        self.call("watchChange", &format!("{{\"changes\":[{}]}}", items.join(","))).map(|_| ())
    }

    pub fn close_watcher(&self) -> Result<(), String> {
        self.call("closeWatcher", "{}").map(|_| ())
    }

    pub fn close_bundle(&self) -> Result<(), String> {
        if self.has("closeBundle") { self.call("closeBundle", "{}").map(|_| ()) } else { Ok(()) }
    }

    /// The first plugin's answer to `resolveId`, if any claims the request.
    pub fn resolve_id(&self, source: &str, importer: Option<&str>, is_entry: bool, kind: &str) -> Result<Option<Resolved>, String> {
        if !self.has("resolveId") {
            return Ok(None);
        }
        let result = self.call("resolveId", &format!("{{\"source\":{},\"importer\":{},\"isEntry\":{is_entry},\"kind\":{},\"onlyPlugins\":true}}", quote(source), opt_str(importer), quote(kind)))?;
        if matches!(result, Json::Null) {
            return Ok(None);
        }
        Ok(Some(Resolved {
            id: result.get("id").and_then(Json::as_str).unwrap_or(source).to_string(),
            external: result.get("external").and_then(Json::as_bool).unwrap_or(false),
            side_effects: result.get("sideEffects").and_then(Json::as_bool),
        }))
    }

    /// `moduleParsed`, as soon as one module is read and its imports are known (not when the whole graph is). Plugins
    /// that wait for every module to be parsed (the federation plugin does, before it writes its own) need it this way.
    pub fn module_parsed(&self, id: &str, imported: &[String], dynamic: &[String], esm: bool, side_effects: bool, exports: &[String], entry: bool) {
        if !self.has("moduleParsed") {
            return;
        }
        let _ = self.call(
            "moduleParsed",
            &format!(
                "{{\"id\":{},\"importedIds\":{},\"dynamicallyImportedIds\":{},\"esm\":{esm},\"sideEffects\":{side_effects},\"isEntry\":{entry},\"exports\":{},\"hasDefaultExport\":{}}}",
                quote(id),
                string_list(imported),
                string_list(dynamic),
                string_list(exports),
                exports.iter().any(|e| e == "default")
            ),
        );
    }

    pub fn load(&self, id: &str) -> Result<Option<Loaded>, String> {
        if !self.has("load") {
            return Ok(None);
        }
        let result = self.call("load", &format!("{{\"id\":{}}}", quote(id)))?;
        if matches!(result, Json::Null) {
            return Ok(None);
        }
        let text = |key: &str| result.get(key).and_then(Json::as_str).map(str::to_string);
        Ok(Some(Loaded {
            code: text("code"),
            bytes: text("bytes").map(|b| crate::loader::base64_decode(&b)),
            map: text("map"),
            module_type: text("moduleType"),
            resolve_dir: text("resolveDir"),
            side_effects: result.get("sideEffects").and_then(Json::as_bool),
            js_face: text("jsFace"),
        }))
    }

    pub fn transform(&self, code: &str, id: &str, module_type: Option<&str>) -> Result<Option<Transformed>, String> {
        if !self.has("transform") {
            return Ok(None);
        }
        let result = self.call("transform", &format!("{{\"code\":{},\"id\":{},\"moduleType\":{}}}", quote(code), quote(id), opt_str(module_type)))?;
        if matches!(result, Json::Null) {
            return Ok(None);
        }
        let text = |key: &str| result.get(key).and_then(Json::as_str).map(str::to_string);
        Ok(Some(Transformed { code: text("code").unwrap_or_default(), map: text("map"), module_type: text("moduleType") }))
    }

    /// `renderChunk` over all plugins. Returns the new code and map when something changed.
    pub fn render_chunk(&self, code: &str, file_name: &str, is_entry: bool, format: &str, map: Option<&str>, info: Option<&ChunkInfo>) -> Result<Option<(String, Option<String>, bool)>, String> {
        if !self.has("renderChunk") {
            return Ok(None);
        }
        let params = format!(
            "{{\"code\":{},\"map\":{},\"chunk\":{},\"options\":{{\"format\":{},\"sourcemap\":{}}}}}",
            quote(code),
            opt_str(map),
            chunk_json(file_name, is_entry, info),
            quote(format),
            map.is_some()
        );
        let result = self.call("renderChunk", &params)?;
        if matches!(result, Json::Null) {
            return Ok(None);
        }
        let new_code = result.get("code").and_then(Json::as_str).unwrap_or(code).to_string();
        let new_map = result.get("map").and_then(Json::as_str).map(str::to_string);
        let dropped = result.get("mapDropped").and_then(Json::as_bool).unwrap_or(false);
        Ok(Some((new_code, new_map, dropped)))
    }

    /// The chunks plugins have asked for so far, with the ids they start from resolved.
    pub fn emitted_chunks(&self) -> Result<Vec<EmittedChunk>, String> {
        let result = self.call("emittedChunks", "{}")?;
        let mut chunks = Vec::new();
        for item in result.as_array().unwrap_or(&[]) {
            let text = |key: &str| item.get(key).and_then(Json::as_str).map(str::to_string);
            if let (Some(reference), Some(id), Some(file_name)) = (text("reference"), text("id"), text("fileName")) {
                chunks.push(EmittedChunk { reference, id, file_name });
            }
        }
        Ok(chunks)
    }

    /// Tells the host the shape of the finished graph: `moduleParsed` runs for each module, and
    /// `getModuleInfo` knows who imports whom.
    pub fn module_graph(&self, graph: &Graph, entries: &[String]) -> Result<(), String> {
        if !self.has("moduleParsed") && !self.has("buildEnd") && !self.has("generateBundle") && !self.has("renderChunk") && !self.has("transform") {
            return Ok(());
        }
        let id_of = |edge: &Edge| -> Option<String> {
            match edge {
                Edge::Module(m) => Some(graph.module(*m).path.to_string_lossy().into_owned()),
                Edge::External(spec) => Some(spec.clone()),
                Edge::Unresolved => None,
            }
        };
        let mut importers: HashMap<String, Vec<String>> = HashMap::new();
        let mut dynamic_importers: HashMap<String, Vec<String>> = HashMap::new();
        for module in &graph.modules {
            let me = module.path.to_string_lossy().into_owned();
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let Some(target) = id_of(edge) {
                    let map = if request.kind == crate::module::RequestKind::Dynamic { &mut dynamic_importers } else { &mut importers };
                    let list = map.entry(target).or_default();
                    if !list.contains(&me) {
                        list.push(me.clone());
                    }
                }
            }
        }
        let mut items = Vec::new();
        for module in &graph.modules {
            let me = module.path.to_string_lossy().into_owned();
            let (mut imported, mut dynamic) = (Vec::new(), Vec::new());
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let Some(target) = id_of(edge) {
                    let list = if request.kind == crate::module::RequestKind::Dynamic { &mut dynamic } else { &mut imported };
                    if !list.contains(&target) {
                        list.push(target);
                    }
                }
            }
            let exports = crate::link::export_names(graph, module.id);
            items.push(format!(
                "{{\"id\":{},\"isEntry\":{},\"importedIds\":{},\"dynamicallyImportedIds\":{},\"importers\":{},\"dynamicImporters\":{},\"exports\":{},\"hasDefaultExport\":{},\"esm\":{},\"sideEffects\":{}}}",
                quote(&me),
                graph.entries.contains(&module.id),
                string_list(&imported),
                string_list(&dynamic),
                string_list(importers.get(&me).map_or(&[][..], |v| v.as_slice())),
                string_list(dynamic_importers.get(&me).map_or(&[][..], |v| v.as_slice())),
                string_list(&exports),
                exports.iter().any(|e| e == "default"),
                module.kind == crate::graph::ModuleKind::Esm,
                module.side_effects
            ));
        }
        self.call("moduleGraph", &format!("{{\"modules\":[{}],\"entries\":{}}}", items.join(","), string_list(entries))).map(|_| ())
    }

    /// `generateBundle`: the plugins may change files, add some, and remove some. Returns the files
    /// that are different (or new) and the names that are gone.
    pub fn generate_bundle(&self, files: &[BundleFile], format: &str, dir: Option<&str>) -> Result<(Vec<BundleFile>, Vec<String>), String> {
        let list: Vec<String> = files
            .iter()
            .map(|f| {
                if f.is_chunk {
                    format!("{{\"type\":\"chunk\",\"fileName\":{},\"name\":{},\"code\":{},\"map\":{},\"isEntry\":{},\"chunk\":{}}}", quote(&f.file_name), opt_str(f.name.as_deref()), quote(&f.code), opt_str(f.map.as_deref()), f.is_entry, chunk_json(&f.file_name, f.is_entry, f.info.as_ref()))
                } else if let Some(bytes) = &f.bytes {
                    format!("{{\"type\":\"asset\",\"fileName\":{},\"name\":{},\"source\":{},\"encoding\":\"base64\"}}", quote(&f.file_name), opt_str(f.name.as_deref()), quote(&crate::loader::base64(bytes)))
                } else {
                    format!("{{\"type\":\"asset\",\"fileName\":{},\"name\":{},\"source\":{},\"encoding\":\"utf8\"}}", quote(&f.file_name), opt_str(f.name.as_deref()), quote(&f.code))
                }
            })
            .collect();
        let params = format!("{{\"files\":[{}],\"options\":{{\"format\":{},\"dir\":{}}}}}", list.join(","), quote(format), opt_str(dir));
        let result = self.call("generateBundle", &params)?;
        let mut changed = Vec::new();
        for item in result.get("files").and_then(Json::as_array).unwrap_or(&[]) {
            let text = |key: &str| item.get(key).and_then(Json::as_str).map(str::to_string);
            let is_chunk = text("type").as_deref() == Some("chunk");
            let file_name = text("fileName").unwrap_or_default();
            let (code, bytes) = if is_chunk {
                (text("code").unwrap_or_default(), None)
            } else if text("encoding").as_deref() == Some("base64") {
                (String::new(), Some(crate::loader::base64_decode(&text("source").unwrap_or_default())))
            } else {
                (text("source").unwrap_or_default(), None)
            };
            changed.push(BundleFile { file_name, is_chunk, name: text("name"), is_entry: false, code, bytes, map: text("map"), info: None });
        }
        let deleted = result.get("deleted").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Ok((changed, deleted))
    }

    pub fn write_bundle(&self, names: &[String], format: &str, dir: Option<&str>) -> Result<(), String> {
        if !self.has("writeBundle") {
            return Ok(());
        }
        self.call("writeBundle", &format!("{{\"names\":{},\"options\":{{\"format\":{},\"dir\":{}}}}}", string_list(names), quote(format), opt_str(dir))).map(|_| ())
    }

    /// Stops Node.
    pub fn shutdown(&self) {
        let _ = self.call("shutdown", "{}");
        let _ = self.child.lock().expect("child").wait();
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

fn reply_to_host(shared: &Shared, id: &str, reply: Result<String, String>) {
    let text = match reply {
        Ok(result) => format!("{{\"id\":{},\"result\":{result}}}", quote(id)),
        Err(error) => format!("{{\"id\":{},\"error\":{{\"message\":{}}}}}", quote(id), quote(&error)),
    };
    let mut stdin = shared.stdin.lock().expect("stdin");
    let _ = writeln!(stdin, "{text}");
    let _ = stdin.flush();
}

fn describe_error(error: &Json) -> String {
    let text = |key: &str| error.get(key).and_then(Json::as_str);
    let mut message = text("message").unwrap_or("plugin error").to_string();
    if let Some(plugin) = text("plugin") {
        message = format!("[{plugin}] {message}");
    }
    if let Some(id) = text("id") {
        message = format!("{message} (in {id})");
    }
    if std::env::var_os("RATHKIT_PLUGIN_DEBUG").is_some_and(|v| !v.is_empty()) {
        if let Some(stack) = text("stack") {
            message = format!("{message}\n{stack}");
        }
    }
    message
}

/// Requests the host makes of us.
fn answer(shared: &Shared, method: &str, params: Option<&Json>) -> Result<String, String> {
    match method {
        "defaultResolve" => {
            let get = |key: &str| params.and_then(|p| p.get(key)).and_then(Json::as_str);
            let source = get("source").unwrap_or("");
            let resolver = shared.resolver.lock().expect("resolver").clone();
            let Some(resolver) = resolver else { return Ok("null".to_string()) };
            match resolver(source, get("importer"), get("kind").unwrap_or("import-statement")) {
                Some((id, external)) => Ok(format!("{{\"id\":{},\"external\":{external}}}", quote(&id))),
                None => Ok("null".to_string()),
            }
        }
        "bundleEntry" => {
            let get = |key: &str| params.and_then(|p| p.get(key)).and_then(Json::as_str);
            let builder = shared.child_builder.lock().expect("builder").clone();
            let Some(builder) = builder else { return Err("this build cannot compile a child entry".to_string()) };
            let (code, assets) = builder(get("entry").unwrap_or(""), get("globalName").unwrap_or("RESULT"))?;
            let files: Vec<String> = assets.iter().map(|(name, data)| format!("{{\"name\":{},\"data\":{}}}", quote(name), quote(&crate::loader::base64(data)))).collect();
            Ok(format!("{{\"code\":{},\"assets\":[{}]}}", quote(&code), files.join(",")))
        }
        other => Err(format!("unknown request {other}")),
    }
}

// ---- the output side ------------------------------------------------------------------------------

use crate::bundle::OutputFile;

fn is_chunk(name: &str) -> bool {
    name.ends_with(".js") || name.ends_with(".mjs") || name.ends_with(".cjs")
}

/// Splits the `//# sourceMappingURL=` line the bundler adds to the end of a chunk off its code.
fn split_map_comment(code: &str) -> (&str, &str) {
    let trimmed = code.trim_end_matches('\n');
    if let Some(at) = trimmed.rfind("\n//# sourceMappingURL=") {
        if !trimmed[at + 1..].contains('\n') {
            return (&code[..at], &code[at..]);
        }
    }
    (code, "")
}

/// A map for text that now has `lines` more lines in front.
fn shift_map(map: &str, lines: usize) -> String {
    match map.find("\"mappings\":\"") {
        Some(at) => {
            let start = at + "\"mappings\":\"".len();
            format!("{}{}{}", &map[..start], ";".repeat(lines), &map[start..])
        }
        None => map.to_string(),
    }
}

/// Runs `renderChunk` and `generateBundle` over the finished files, as Rollup does before writing.
/// Plugins may change chunks, change or add assets, and remove files.
pub fn post_process(host: &PluginHost, files: &mut Vec<OutputFile>, chunks: &[ChunkInfo], format: &str, dir: Option<&str>) -> Result<(), String> {
    let info_of = |name: &str| chunks.iter().find(|c| c.file == name);
    for file in files.iter_mut().filter(|f| is_chunk(&f.name) && f.data.is_none()) {
        let (body, comment) = split_map_comment(&file.code);
        let (body, comment) = (body.to_string(), comment.to_string());
        let info = info_of(&file.name);
        let Some((code, map, dropped)) = host.render_chunk(&body, &file.name, info.is_some_and(|i| i.is_entry), format, file.map.as_deref(), info)? else { continue };
        if code == body {
            continue;
        }
        let new_map = match (map, &file.map) {
            (Some(m), _) => Some(m),
            // No map came back: if the plugin only wrapped the code in lines of its own, the old map still
            // describes it once the first line moves down.
            (None, Some(old)) => match code.find(&body) {
                Some(at) if code[..at].ends_with('\n') || at == 0 => Some(shift_map(old, code[..at].matches('\n').count())),
                _ => {
                    if dropped {
                        eprintln!("warning: a renderChunk plugin changed {} without a source map; its map is dropped", file.name);
                    }
                    None
                }
            },
            (None, None) => None,
        };
        file.code = format!("{code}{comment}");
        if new_map.is_some() || file.map.is_some() {
            file.map = new_map;
        }
    }
    let view: Vec<BundleFile> = files
        .iter()
        .map(|f| BundleFile {
            file_name: f.name.clone(),
            is_chunk: is_chunk(&f.name) && f.data.is_none(),
            name: Some(f.name.rsplit_once('.').map_or(f.name.clone(), |(s, _)| s.to_string())),
            is_entry: info_of(&f.name).is_some_and(|i| i.is_entry),
            code: f.code.clone(),
            bytes: f.data.clone(),
            map: f.map.clone(),
            info: info_of(&f.name).cloned(),
        })
        .collect();
    let (changed, deleted) = host.generate_bundle(&view, format, dir)?;
    files.retain(|f| !deleted.contains(&f.name));
    for item in changed {
        match files.iter_mut().find(|f| f.name == item.file_name) {
            Some(existing) => {
                existing.code = item.code;
                existing.data = item.bytes;
                if item.map.is_some() {
                    existing.map = item.map;
                }
            }
            None => files.push(OutputFile { name: item.file_name, code: item.code, map: item.map, data: item.bytes }),
        }
    }
    Ok(())
}
