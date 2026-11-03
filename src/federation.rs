//! Module federation: an application offers some of its modules to others (`exposes`), takes modules
//! from other applications (`remotes`), and agrees with them on one copy of each package they have
//! in common (`shared`). The options are those of webpack's and rspack's `ModuleFederationPlugin`.
//!
//! The work is done by a small runtime ([`runtime.js`](./federation/runtime.js)) that speaks the
//! container protocol the other bundlers' output speaks too. This module decides what the runtime is
//! given. It writes a few modules that exist only in memory (under [`ROOT`]) and adds them to the build:
//!
//! - `core.js` starts the runtime with the application's remotes and shared packages, and holds the
//!   container (`get` and `init`) that other applications load;
//! - `start.js` waits until every shared package and remote the code reads has been loaded, and every
//!   entry point imports it first;
//! - an import of a remote (`app/Button`) or of a shared package (`react`) becomes a small CommonJS
//!   module that returns what `start.js` loaded, so named imports, default imports and `require` all
//!   work without knowing the exports in advance;
//! - the files the other side reads: `remoteEntry.js`, which names the container, and the manifest.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use crate::bundle::{ChunkInfo, Output, OutputFile};
use crate::graph::{Diagnostic, Severity};
use crate::json::Json;
use crate::module::RequestKind;
use crate::resolve::{Alias, FileSystem, ResolveKind, ResolveOptions, Resolver};

/// Where the modules that exist only in memory live.
pub const ROOT: &str = "/@rathkit/federation";

const RUNTIME: &str = include_str!("federation/runtime.js");
pub(crate) const DTS: &str = include_str!("federation/dts.mjs");
const SEMVER: &str = include_str!("federation/semver.js");

#[derive(Clone, Debug)]
pub struct Expose {
    /// `./Button`: what a remote asks for.
    pub key: String,
    /// The module, as written (`./src/Button.js`).
    pub import: String,
}

#[derive(Clone, Debug)]
pub struct Remote {
    pub name: String,
    pub entry: String,
    /// How the entry is loaded: `global` (a script that defines a variable), `module`, `commonjs`, `system`; or `code`, an
    /// expression that gives the container, and `internal`, a module of this application (then `entry` is that).
    pub kind: String,
    pub global_name: String,
    pub share_scope: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Shared {
    /// The package (`react`), or a prefix ending in `/` for every path below it (`react-dom/`).
    pub key: String,
    /// What is bundled as the fallback; `None` when the application only consumes (`import: false`).
    pub import: Option<String>,
    pub version: Option<String>,
    /// `None` takes it from the application's `package.json`.
    pub required_version: Option<RequiredVersion>,
    pub singleton: bool,
    pub strict_version: bool,
    pub eager: bool,
    pub share_scope: Vec<String>,
    /// `include` and `exclude`: which copies may be shared, as JSON the runtime reads.
    pub include: Option<String>,
    pub exclude: Option<String>,
    /// `treeShaking`: the copy bundled here is also written with only the exports this application reads, for others
    /// whose needs it covers. The mode is carried to the runtime.
    pub tree_shaking: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RequiredVersion {
    Range(String),
    /// `false`: any version will do.
    Any,
}

/// TypeScript declarations: written for what this application exposes, and read for the remotes it uses.
#[derive(Clone, Debug)]
pub struct Dts {
    pub generate: bool,
    pub consume: bool,
    pub ts_config: Option<String>,
    /// `@mf-types`: the folder, and the name of the zip beside the output.
    pub types_folder: String,
    pub compiled_types_folder: String,
    /// Where the types of remotes are put, relative to the project.
    pub consume_folder: String,
    /// Also map the remotes' modules in `tsconfig.json` (`paths`); off, the editor finds them through `node_modules`.
    pub update_ts_config: bool,
    /// `generateTypes.abortOnError` / `consumeTypes.abortOnError`: a failure stops the build instead of being a warning.
    pub abort_generate: bool,
    pub abort_consume: bool,
    /// `generateTypes.additionalFilesToCompile`: files described besides the exposed modules.
    pub additional_files: Vec<String>,
    /// `generateTypes.generateAPITypes`: write `apis.d.ts` and `@mf-types.d.ts` too.
    pub generate_api: bool,
    /// `generateTypes.outputDir`: the folder of the output that the types go in.
    pub output_dir: Option<String>,
    /// `consumeTypes.timeout` (milliseconds) and `maxRetries`.
    pub timeout_ms: u64,
    pub max_retries: u32,
    /// `consumeTypes.remoteTypeUrls`: where a remote's types are, by remote name, `(zip, api)`.
    pub remote_type_urls: Vec<(String, String, String)>,
    /// `dev.disableHotTypesReload`: types are not read again while watching.
    pub hot_types: bool,
    /// `generateTypes.compilerInstance`: the command that writes the declarations (`tsc`, or `vue-tsc`, `tspc`, ...).
    pub compiler: String,
    /// `generateTypes.extractThirdParty`: copy the declarations of the packages the types import into the zip, minus these.
    pub extract_third_party: Option<Vec<String>>,
    /// `generateTypes.extractRemoteTypes`: copy the types read of this application's own remotes into the zip.
    pub extract_remote_types: bool,
    /// `generateTypes.afterGenerate`: the source of a function called with the files written.
    pub after_generate: Option<String>,
    /// `consumeTypes.family`: 4 or 6 to use only that kind of address (0: either).
    pub family: u8,
    /// `consumeTypes.remoteTypesFolder`: the archive of a remote is `<this>.zip` beside its entry, when its manifest does not say.
    pub remote_types_folder: Option<String>,
    /// `consumeTypes.consumeAPITypes` and `runtimePkgs`: write `index.d.ts` in the types folder, typing `loadRemote`.
    pub consume_api_types: bool,
    pub runtime_pkgs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PluginRef {
    pub path: String,
    /// JSON of the argument the plugin function is called with.
    pub params: Option<String>,
}

pub struct Federation {
    pub name: String,
    pub filename: String,
    pub library_type: String,
    pub library_name: String,
    pub exposes: Vec<Expose>,
    pub remotes: Vec<Remote>,
    pub shared: Vec<Shared>,
    pub share_scope: Vec<String>,
    pub share_strategy: String,
    pub runtime_plugins: Vec<PluginRef>,
    pub dts: Dts,
    /// `dev.disableLiveReload` is not set: pages of hosts reload when this application builds again while serving.
    pub live_reload: bool,
    /// `bridge`: `Some(true)` the router bridge is on, `Some(false)` it is off, `None` it is on if `@module-federation/bridge-react` is a dependency.
    pub bridge_router: Option<bool>,
    /// The manifest's file name; `None` writes none.
    pub manifest: Option<String>,
    /// Where the files are served from, when it is known at build time (`auto` is the default).
    pub public_path: Option<String>,
    /// `getPublicPath`: the body of a function that returns the address the container's files are served from.
    pub public_path_code: Option<String>,
    pub version: Option<String>,
    cwd: PathBuf,
    files: RwLock<Arc<HashMap<PathBuf, String>>>,
    used: Mutex<BTreeSet<String>>,
    /// The types of the remotes were read once; the remotes may be down the first time, and a build can ask again.
    types_read: std::sync::atomic::AtomicBool,
    /// The remotes whose types have been read.
    types_remotes: Mutex<Vec<String>>,
    /// Which shared packages the bundled copy of each shared package reads (`shared:react-dom` reads `shared:react`):
    /// found from the module graph, so that the packages are loaded in an order that has each in place when it is read.
    needs: Mutex<std::collections::BTreeMap<String, BTreeSet<String>>>,
    /// The file each bundled copy of a shared package is built from.
    fallbacks: Mutex<HashMap<PathBuf, String>>,
    /// For each shared package, the exports the application reads (`None`: all of them, or not known), and whether its
    /// bundled copy is an ES module, which is what a tree-shaken copy needs. Found from the module graph.
    reads: Mutex<std::collections::BTreeMap<String, (Option<BTreeSet<String>>, bool)>>,
    /// The copies written with only some exports are built apart from the rest, from these settings.
    graph_options: Mutex<Option<crate::graph::GraphOptions>>,
    bundle_options: Mutex<Option<crate::bundle::Options>>,
    shaken: Mutex<Vec<(String, String)>>,
    /// Why a shared package got no copy with only the exports in use, said once the build is done.
    shaken_skipped: Mutex<Vec<String>>,
    /// The copies with only some exports built so far: what they were made from, and the text.
    shaken_cache: Mutex<HashMap<String, (Vec<(PathBuf, std::time::SystemTime)>, String)>>,
    /// The modules of the application that are containers (remotes written as `internal`).
    internal: Mutex<Vec<PathBuf>>,
    /// The declarations written for the exposed modules by the last run, and whether another run is going.
    types_cache: Mutex<Option<Vec<OutputFile>>>,
    types_running: std::sync::atomic::AtomicBool,
    /// Told when a later run has new declarations (the development server serves them without a rebuild).
    types_hook: Mutex<Option<Arc<dyn Fn(&[OutputFile]) + Send + Sync>>>,
    /// A run that reads the declarations of the remotes is going, and when the last one started.
    reading: std::sync::atomic::AtomicBool,
    last_read: Mutex<Option<std::time::Instant>>,
}

impl std::fmt::Debug for Federation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Federation({})", self.name)
    }
}

/// Options of `ModuleFederationPlugin` that change what a build does and are not carried out here, said on stderr when the options are
/// read: a build meant to behave one way must not quietly behave another. (Settings of how the real plugin is arranged inside webpack or
/// rspack, such as `runtime`, `implementation`, `async`, `virtualRuntimeEntry` and the `treeShaking*` directories, change nothing a
/// program can observe here and are accepted without a word.)
fn warn_unused(options: &Json) {
    if options.get("dts").and_then(|d| d.get("implementation")).is_some_and(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
        eprintln!("warning: federation option `dts.implementation` has no effect: another implementation of the types plugin is not run");
    }
}

pub(crate) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn string_list(value: Option<&Json>, fallback: &str) -> Vec<String> {
    match value {
        Some(Json::String(s)) => vec![s.clone()],
        Some(Json::Array(items)) => {
            let list: Vec<String> = items.iter().filter_map(|i| i.as_str().map(String::from)).collect();
            if list.is_empty() { vec![fallback.to_string()] } else { list }
        }
        _ => vec![fallback.to_string()],
    }
}

/// `app@https://host/remoteEntry.js` is the global `app` and the address; a bare address names no global.
fn split_external(external: &str) -> (Option<String>, String) {
    if let Some(at) = external.find('@') {
        let (name, rest) = external.split_at(at);
        let rest = &rest[1..];
        let named = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '$' | '.'));
        let address = rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("//") || rest.starts_with('/') || rest.starts_with("./") || rest.starts_with("file:");
        if named && address {
            return (Some(name.to_string()), rest.to_string());
        }
    }
    (None, external.to_string())
}

impl Federation {
    /// Reads the options of a `ModuleFederationPlugin`, as JSON.
    pub fn from_json(options: &Json, cwd: &Path) -> Result<Federation, String> {
        let name = options.get("name").and_then(Json::as_str).ok_or("module federation needs a `name`")?.to_string();
        warn_unused(options);
        let library = options.get("library");
        let library_type = library.and_then(|l| l.get("type")).and_then(Json::as_str).unwrap_or("var").to_string();
        let library_name = library.and_then(|l| l.get("name")).and_then(Json::as_str).unwrap_or(&name).to_string();
        let filename = options.get("filename").and_then(Json::as_str).unwrap_or("remoteEntry.js").to_string();
        let remote_type = options.get("remoteType").and_then(Json::as_str).unwrap_or(if library_type == "module" { "module" } else { "script" });
        let share_scope = string_list(options.get("shareScope"), "default");

        // exposes: { "./a": "./src/a.js" } | { "./a": { import } } | ["./a"] | [{ "./a": "./src/a.js" }]
        let mut exposes = Vec::new();
        let mut add_expose = |key: &str, value: &Json| -> Result<(), String> {
            let import = match value {
                Json::String(s) => s.clone(),
                Json::Array(list) => list.first().and_then(Json::as_str).ok_or("an expose has no module")?.to_string(),
                other => match other.get("import") {
                    Some(Json::String(s)) => s.clone(),
                    Some(Json::Array(list)) => list.first().and_then(Json::as_str).ok_or("an expose has no module")?.to_string(),
                    _ => return Err(format!("the expose {key} has no module")),
                },
            };
            exposes.push(Expose { key: if key.starts_with('.') { key.to_string() } else { format!("./{key}") }, import });
            Ok(())
        };
        match options.get("exposes") {
            Some(Json::Object(entries)) => {
                for (key, value) in entries {
                    add_expose(key, value)?;
                }
            }
            Some(Json::Array(items)) => {
                for item in items {
                    match item {
                        Json::String(s) => add_expose(s, item)?,
                        Json::Object(entries) => {
                            for (key, value) in entries {
                                add_expose(key, value)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        // remotes: { app: "app@url" } | { app: { external, shareScope } } | ["app@url"] | [{ app: "app@url" }]
        let mut remotes = Vec::new();
        let mut add_remote = |key: &str, value: &Json| -> Result<(), String> {
            let (external, scope) = match value {
                Json::String(s) => (s.clone(), share_scope.clone()),
                Json::Array(list) => (list.first().and_then(Json::as_str).ok_or("a remote has no address")?.to_string(), share_scope.clone()),
                other => {
                    let external = match other.get("external").or_else(|| other.get("entry")) {
                        Some(Json::String(s)) => s.clone(),
                        Some(Json::Array(list)) => list.first().and_then(Json::as_str).ok_or("a remote has no address")?.to_string(),
                        _ => return Err(format!("the remote {key} has no address")),
                    };
                    (external, string_list(other.get("shareScope"), &share_scope[0]))
                }
            };
            // `promise <expression>` and `var <expression>` give the container (or a promise of it) as code; `internal <module>`
            // is a module of this application; `script <address>` is a script that defines the global named like the remote.
            let word = external.split_once(' ').map(|(w, rest)| (w, rest.trim()));
            if let Some((w @ ("promise" | "var" | "internal"), rest)) = word {
                remotes.push(Remote { name: key.to_string(), entry: rest.to_string(), kind: if w == "internal" { "internal" } else { "code" }.to_string(), global_name: key.to_string(), share_scope: scope });
                return Ok(());
            }
            let external = match word {
                Some(("script", rest)) => format!("{key}@{rest}"),
                _ => external,
            };
            let (global, entry) = split_external(&external);
            // A remote can say how its own entry is loaded; otherwise `remoteType` does for all of them.
            let own = match value {
                Json::Object(_) => value.get("type").and_then(Json::as_str),
                _ => None,
            };
            let remote_type = own.unwrap_or(remote_type);
            let kind = if remote_type == "jsonp" {
                "jsonp"
            } else if remote_type == "system" || remote_type == "systemjs" {
                "system"
            } else if entry.ends_with(".mjs") || remote_type == "module" {
                "module"
            } else if remote_type == "commonjs" || remote_type == "commonjs2" {
                "commonjs"
            } else {
                "global"
            };
            remotes.push(Remote { name: key.to_string(), entry, kind: kind.to_string(), global_name: global.unwrap_or_else(|| key.to_string()), share_scope: scope });
            Ok(())
        };
        match options.get("remotes") {
            Some(Json::Object(entries)) => {
                for (key, value) in entries {
                    add_remote(key, value)?;
                }
            }
            Some(Json::Array(items)) => {
                for item in items {
                    match item {
                        Json::String(s) => {
                            let (global, _) = split_external(s);
                            let key = global.ok_or_else(|| format!("the remote {s} has no name; write `name@address`"))?;
                            add_remote(&key, item)?;
                        }
                        Json::Object(entries) => {
                            for (key, value) in entries {
                                add_remote(key, value)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        // shared: ["react", { "react-dom": { singleton: true } }] | { react: "^19" } | { react: { singleton } }
        let mut shared = Vec::new();
        let mut add_shared = |key: &str, value: &Json| {
            let object = matches!(value, Json::Object(_));
            let version_text = match value {
                Json::String(s) => Some(s.clone()),
                _ => None,
            };
            let import = match value.get("import") {
                Some(Json::Bool(false)) => None,
                Some(Json::String(s)) => Some(s.clone()),
                _ => Some(key.to_string()),
            };
            let required = match value.get("requiredVersion") {
                Some(Json::Bool(false)) => Some(RequiredVersion::Any),
                Some(Json::String(s)) => Some(RequiredVersion::Range(s.clone())),
                _ => version_text.map(RequiredVersion::Range),
            };
            let flag = |name: &str| object && value.get(name).and_then(Json::as_bool).unwrap_or(false);
            shared.push(Shared {
                key: key.to_string(),
                import,
                version: value.get("version").and_then(Json::as_str).map(String::from),
                required_version: required,
                singleton: flag("singleton"),
                strict_version: flag("strictVersion"),
                eager: flag("eager"),
                share_scope: string_list(value.get("shareScope"), &share_scope[0]),
                tree_shaking: match value.get("treeShaking") {
                    Some(Json::Bool(true)) => Some("runtime-infer".to_string()),
                    Some(t @ Json::Object(_)) => Some(t.get("mode").and_then(Json::as_str).unwrap_or("runtime-infer").to_string()),
                    _ => None,
                },
                include: value.get("include").filter(|v| matches!(v, Json::Object(_))).map(Json::to_string),
                exclude: value.get("exclude").filter(|v| matches!(v, Json::Object(_))).map(Json::to_string),
            });
        };
        match options.get("shared") {
            Some(Json::Object(entries)) => {
                for (key, value) in entries {
                    add_shared(key, value);
                }
            }
            Some(Json::Array(items)) => {
                for item in items {
                    match item {
                        Json::String(s) => add_shared(s, &Json::Null),
                        Json::Object(entries) => {
                            for (key, value) in entries {
                                add_shared(key, value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        let runtime_plugins = match options.get("runtimePlugins") {
            Some(Json::Array(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Json::String(s) => Some(PluginRef { path: s.clone(), params: None }),
                    Json::Array(pair) => pair.first().and_then(Json::as_str).map(|p| PluginRef { path: p.to_string(), params: pair.get(1).map(Json::to_string) }),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let manifest = match options.get("manifest") {
            Some(Json::Bool(false)) => None,
            Some(object @ Json::Object(_)) => {
                let file = object.get("fileName").and_then(Json::as_str).unwrap_or("mf-manifest.json");
                let dir = object.get("filePath").and_then(Json::as_str).unwrap_or("");
                Some(if dir.is_empty() { file.to_string() } else { format!("{}/{file}", dir.trim_end_matches('/')) })
            }
            _ => Some("mf-manifest.json".to_string()),
        };
        let dts_option = options.get("dts");
        let flag = |value: Option<&Json>| !matches!(value, Some(Json::Bool(false)));
        let section = |key: &str| dts_option.and_then(|d| d.get(key)).filter(|v| matches!(v, Json::Object(_)));
        let text = |key: &str, from: Option<&Json>| from.and_then(|o| o.get(key)).and_then(Json::as_str).map(String::from);
        let dts = Dts {
            generate: flag(dts_option) && flag(dts_option.and_then(|d| d.get("generateTypes"))),
            consume: flag(dts_option) && flag(dts_option.and_then(|d| d.get("consumeTypes"))),
            ts_config: text("tsConfigPath", section("generateTypes")).or_else(|| text("tsConfigPath", dts_option)),
            types_folder: text("typesFolder", section("generateTypes")).unwrap_or_else(|| "@mf-types".to_string()),
            compiled_types_folder: text("compiledTypesFolder", section("generateTypes")).unwrap_or_else(|| "compiled-types".to_string()),
            consume_folder: text("typesFolder", section("consumeTypes")).unwrap_or_else(|| "@mf-types".to_string()),
            update_ts_config: section("consumeTypes").and_then(|c| c.get("updateTsConfig")).and_then(Json::as_bool).unwrap_or(false),
            abort_generate: section("generateTypes").and_then(|c| c.get("abortOnError")).and_then(Json::as_bool).unwrap_or(false),
            abort_consume: section("consumeTypes").and_then(|c| c.get("abortOnError")).and_then(Json::as_bool).unwrap_or(false),
            additional_files: match section("generateTypes").and_then(|c| c.get("additionalFilesToCompile")) {
                Some(Json::Array(list)) => list.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                _ => Vec::new(),
            },
            generate_api: section("generateTypes").and_then(|c| c.get("generateAPITypes")).and_then(Json::as_bool).unwrap_or(true),
            output_dir: text("outputDir", section("generateTypes")).filter(|d| !d.is_empty()),
            timeout_ms: section("consumeTypes").and_then(|c| c.get("timeout")).and_then(Json::as_f64).map_or(5000, |t| t as u64),
            max_retries: section("consumeTypes").and_then(|c| c.get("maxRetries")).and_then(Json::as_f64).map_or(0, |t| t as u32),
            remote_type_urls: match section("consumeTypes").and_then(|c| c.get("remoteTypeUrls")) {
                Some(Json::Object(entries)) => entries
                    .iter()
                    .filter_map(|(name, v)| Some((name.clone(), v.get("zip")?.as_str()?.to_string(), v.get("api").and_then(Json::as_str).unwrap_or("").to_string())))
                    .collect(),
                _ => Vec::new(),
            },
            compiler: text("compilerInstance", section("generateTypes")).unwrap_or_else(|| "tsc".to_string()),
            extract_third_party: match section("generateTypes").and_then(|c| c.get("extractThirdParty")) {
                Some(Json::Bool(true)) => Some(Vec::new()),
                Some(object @ Json::Object(_)) => Some(match object.get("exclude") {
                    Some(Json::Array(list)) => list.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                    _ => Vec::new(),
                }),
                _ => None,
            },
            extract_remote_types: section("generateTypes").and_then(|c| c.get("extractRemoteTypes")).and_then(Json::as_bool).unwrap_or(false),
            after_generate: section("generateTypes").and_then(|c| c.get("afterGenerate")).and_then(Json::as_str).map(String::from),
            family: section("consumeTypes").and_then(|c| c.get("family")).and_then(Json::as_f64).map_or(0, |f| f as u8),
            remote_types_folder: text("remoteTypesFolder", section("consumeTypes")),
            consume_api_types: section("consumeTypes").and_then(|c| c.get("consumeAPITypes")).and_then(Json::as_bool).unwrap_or(false),
            runtime_pkgs: match section("consumeTypes").and_then(|c| c.get("runtimePkgs")) {
                Some(Json::Array(list)) => list.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                _ => Vec::new(),
            },
            hot_types: !matches!(options.get("dev"), Some(d) if d.get("disableHotTypesReload").and_then(Json::as_bool) == Some(true)),
        };
        let live_reload = !matches!(options.get("dev"), Some(d) if d.get("disableLiveReload").and_then(Json::as_bool) == Some(true));
        Ok(Federation {
            name,
            filename,
            library_type,
            library_name,
            exposes,
            remotes,
            shared,
            share_scope,
            share_strategy: options.get("shareStrategy").and_then(Json::as_str).unwrap_or("version-first").to_string(),
            runtime_plugins,
            dts,
            live_reload,
            bridge_router: match options.get("bridge") {
                Some(b) if b.get("enableBridgeRouter").and_then(Json::as_bool) == Some(true) => Some(true),
                Some(b) if b.get("enableBridgeRouter").and_then(Json::as_bool) == Some(false) || b.get("disableAlias").and_then(Json::as_bool) == Some(true) => Some(false),
                _ => None,
            },
            manifest,
            public_path: options.get("publicPath").and_then(Json::as_str).filter(|p| *p != "auto").map(String::from),
            public_path_code: options.get("getPublicPath").and_then(Json::as_str).map(String::from),
            version: options.get("version").and_then(Json::as_str).map(String::from),
            cwd: cwd.to_path_buf(),
            files: RwLock::new(Arc::new(HashMap::new())),
            used: Mutex::new(BTreeSet::new()),
            types_read: std::sync::atomic::AtomicBool::new(false),
            types_remotes: Mutex::new(Vec::new()),
            needs: Mutex::new(std::collections::BTreeMap::new()),
            fallbacks: Mutex::new(HashMap::new()),
            reads: Mutex::new(std::collections::BTreeMap::new()),
            graph_options: Mutex::new(None),
            bundle_options: Mutex::new(None),
            shaken: Mutex::new(Vec::new()),
            shaken_skipped: Mutex::new(Vec::new()),
            shaken_cache: Mutex::new(HashMap::new()),
            internal: Mutex::new(Vec::new()),
            types_cache: Mutex::new(None),
            types_running: std::sync::atomic::AtomicBool::new(false),
            types_hook: Mutex::new(None),
            reading: std::sync::atomic::AtomicBool::new(false),
            last_read: Mutex::new(None),
        })
    }

    pub fn core_path(&self) -> PathBuf {
        PathBuf::from(format!("{ROOT}/core.js"))
    }

    fn remote_for(&self, specifier: &str) -> Option<&Remote> {
        self.remotes.iter().filter(|r| specifier == r.name || specifier.strip_prefix(r.name.as_str()).is_some_and(|rest| rest.starts_with('/'))).max_by_key(|r| r.name.len())
    }

    fn shared_for(&self, specifier: &str) -> Option<&Shared> {
        self.shared.iter().find(|s| s.key == specifier).or_else(|| self.shared.iter().filter(|s| s.key.ends_with('/') && specifier.starts_with(s.key.as_str())).max_by_key(|s| s.key.len()))
    }

    pub fn internal_paths(&self) -> Vec<PathBuf> {
        self.internal.lock().expect("internal").clone()
    }

    /// The files the bundled copies of shared packages are built from.
    pub fn fallback_paths(&self) -> Vec<PathBuf> {
        self.fallbacks.lock().expect("fallbacks").keys().cloned().collect()
    }

    /// Finds which shared packages each bundled copy reads, from the graph. True when that is new.
    pub fn learn(&self, graph: &crate::graph::Graph) -> bool {
        use crate::graph::Edge;
        let fallbacks = self.fallbacks.lock().expect("fallbacks").clone();
        let by_path: HashMap<&Path, crate::graph::ModuleId> = graph.modules.iter().map(|m| (m.path.as_path(), m.id)).collect();
        let prefix = format!("{ROOT}/shared/");
        let mut found: std::collections::BTreeMap<String, BTreeSet<String>> = std::collections::BTreeMap::new();
        for (path, key) in &fallbacks {
            let Some(&start) = by_path.get(path.as_path()) else { continue };
            let mut seen = BTreeSet::new();
            let mut stack = vec![start];
            let mut reads = BTreeSet::new();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let module = graph.module(id);
                for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                    if request.kind == RequestKind::Dynamic {
                        continue;
                    }
                    if let Edge::Module(target) = edge {
                        let target_path = graph.module(*target).path.to_string_lossy().into_owned();
                        match target_path.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(".js")) {
                            Some(read) => {
                                reads.insert(format!("shared:{read}"));
                            }
                            None => stack.push(*target),
                        }
                    }
                }
            }
            found.insert(key.clone(), reads);
        }
        // The names the application imports from each shared package, wherever it is imported.
        let mut names: std::collections::BTreeMap<String, Option<BTreeSet<String>>> = std::collections::BTreeMap::new();
        let mut note = |key: &str, name: Option<&str>| {
            let entry = names.entry(key.to_string()).or_insert_with(|| Some(BTreeSet::new()));
            match (entry.as_mut(), name) {
                (Some(set), Some(name)) => {
                    set.insert(name.to_string());
                }
                _ => *entry = None,
            }
        };
        for module in &graph.modules {
            let stub = |request: usize| match module.edges.get(request) {
                Some(Edge::Module(t)) => graph.module(*t).path.to_string_lossy().strip_prefix(&prefix).and_then(|r| r.strip_suffix(".js")).map(String::from),
                _ => None,
            };
            let mut seen_requests = BTreeSet::new();
            for import in &module.record.imports {
                if let Some(key) = stub(import.request) {
                    seen_requests.insert(import.request);
                    match &import.name {
                        crate::module::ImportName::Named(n) => note(&key, Some(n)),
                        crate::module::ImportName::Default => note(&key, Some("default")),
                        crate::module::ImportName::Namespace => note(&key, None),
                    }
                }
            }
            for export in &module.record.exports {
                if let crate::module::ExportSource::Indirect { request, name } = &export.source {
                    if let Some(key) = stub(*request) {
                        seen_requests.insert(*request);
                        match name {
                            crate::module::ImportName::Named(n) => note(&key, Some(n)),
                            crate::module::ImportName::Default => note(&key, Some("default")),
                            crate::module::ImportName::Namespace => note(&key, None),
                        }
                    }
                }
            }
            // Anything else that reaches a stub (`export *`, `require`, `import()`, a bare `import "x"`) may read any name.
            for request in 0..module.edges.len() {
                if !seen_requests.contains(&request) {
                    if let Some(key) = stub(request) {
                        if module.record.requests[request].kind != RequestKind::Static || module.record.star_exports.contains(&request) {
                            note(&key, None);
                        }
                    }
                }
            }
        }
        let esm_of: HashMap<&str, bool> = fallbacks.iter().filter_map(|(path, key)| by_path.get(path.as_path()).map(|id| (key.as_str(), graph.module(*id).kind == crate::graph::ModuleKind::Esm))).collect();
        let reads: std::collections::BTreeMap<String, (Option<BTreeSet<String>>, bool)> =
            names.into_iter().map(|(key, used)| { let esm = esm_of.get(format!("shared:{key}").as_str()).copied().unwrap_or(false); (key, (used, esm)) }).collect();
        let mut needs = self.needs.lock().expect("needs");
        let mut known = self.reads.lock().expect("reads");
        let changed = *needs != found || *known != reads;
        *needs = found;
        *known = reads;
        changed
    }

    /// What an import of a remote or a shared package is replaced by.
    pub fn intercept(&self, specifier: &str, kind: RequestKind) -> Option<Intercept> {
        if specifier.starts_with('.') || specifier.starts_with('/') || specifier.contains(':') {
            return None;
        }
        // Code written against the official runtime package uses this one, so there is a single instance.
        if matches!(specifier, "@module-federation/runtime" | "@module-federation/enhanced/runtime") {
            return Some(Intercept::Module(PathBuf::from(format!("{ROOT}/runtime.js"))));
        }
        let (what, key) = if self.remote_for(specifier).is_some() {
            ("remote", specifier)
        } else if self.shared_for(specifier).is_some() {
            ("shared", specifier)
        } else {
            return None;
        };
        if kind == RequestKind::Dynamic {
            return Some(Intercept::Dynamic);
        }
        self.used.lock().expect("used").insert(format!("{what}:{key}"));
        Some(Intercept::Module(PathBuf::from(format!("{ROOT}/{what}/{key}.js"))))
    }

    /// The remote or shared package a dynamic `import()` asks for: the call is rewritten to ask the runtime.
    pub fn dynamic_target(&self, specifier: &str) -> Option<DynamicKind> {
        if self.remote_for(specifier).is_some() {
            Some(DynamicKind::Remote)
        } else if self.shared_for(specifier).is_some() {
            Some(DynamicKind::Shared)
        } else {
            None
        }
    }

    /// What the build has asked for so far: `remote:app/Button`, `shared:react`.
    pub fn used(&self) -> BTreeSet<String> {
        self.used.lock().expect("used").clone()
    }

    pub fn files(&self) -> Arc<HashMap<PathBuf, String>> {
        self.files.read().expect("files").clone()
    }

    /// The module for an import of a remote or a shared package: what was loaded before anything ran.
    fn stub(&self, path: &Path) -> Option<String> {
        let rest = path.to_str()?.strip_prefix(ROOT)?.strip_prefix('/')?;
        let (what, key) = rest.split_once('/')?;
        let key = key.strip_suffix(".js")?;
        if what != "shared" && what != "remote" {
            return None;
        }
        let id = format!("{what}:{key}");
        Some(format!("var value = require({}).values[{}];\nif (value === undefined) throw new Error({});\nmodule.exports = value;\n", quote(&format!("{ROOT}/values.js")), quote(&id), quote(&format!("module federation: \"{key}\" was read before it was loaded"))))
    }

    /// The files an application needs besides its own, for the packages and remotes it uses.
    pub fn generate(&self, resolve: &ResolveOptions, entries: &[PathBuf], used: &BTreeSet<String>) -> Result<(), Vec<Diagnostic>> {
        let resolver = Resolver::new(resolve.clone());
        let mut files: HashMap<PathBuf, String> = HashMap::new();
        let mut problems: Vec<Diagnostic> = Vec::new();
        let problem = |message: String| Diagnostic { severity: Severity::Error, message, file: None, pos: None };
        let at = |name: &str| PathBuf::from(format!("{ROOT}/{name}"));
        files.insert(at("runtime.js"), RUNTIME.to_string());
        files.insert(at("semver.js"), SEMVER.to_string());
        // One table for the whole application, even for the copies built apart, which have a values.js of their own.
        files.insert(at("values.js"), format!("export const values = (((globalThis.__FEDERATION__ ||= {{}}).__RATHKIT_VALUES__ ||= {{}})[{}] ||= Object.create(null));\n", quote(&self.name)));
        files.insert(at("start.js"), format!("import {{ ready }} from {};\nawait ready();\n", quote(&format!("{ROOT}/core.js"))));

        let resolve_path = |specifier: &str| -> Result<(PathBuf, Option<String>), String> {
            let found = resolver.resolve(&self.cwd, specifier, ResolveKind::Import).map_err(|e| e.to_string())?;
            let version = found.package.as_ref().and_then(|p| p.version.clone());
            Ok((found.path, version))
        };

        // The shared packages: everything configured (a prefix only for what is imported below it).
        let mut shared_lines: Vec<String> = Vec::new();
        let mut shared_used: Vec<String> = Vec::new();
        let reads = self.reads.lock().expect("reads").clone();
        let needs = self.needs.lock().expect("needs").clone();
        let mut skipped: Vec<String> = Vec::new();
        let mut internal: Vec<PathBuf> = Vec::new();
        let mut shaken: Vec<(String, String)> = Vec::new();
        let project = std::fs::read_to_string(self.cwd.join("package.json")).ok().and_then(|t| Json::parse(&t).ok());
        let mut register = |item: &Shared, key: &str| {
            let (fallback, installed) = match &item.import {
                Some(import) => {
                    let target = if item.key.ends_with('/') { format!("{}{}", import, &key[item.key.len()..]) } else { import.clone() };
                    match resolve_path(&target) {
                        Ok((path, version)) => (Some(path), version),
                        Err(message) => {
                            problems.push(problem(format!("shared package {key}: {message}")));
                            return;
                        }
                    }
                }
                None => (None, None),
            };
            if let Some(path) = &fallback {
                self.fallbacks.lock().expect("fallbacks").insert(path.clone(), format!("shared:{key}"));
            }
            let version = item.version.clone().or(installed);
            let required = match &item.required_version {
                Some(RequiredVersion::Any) => "false".to_string(),
                Some(RequiredVersion::Range(r)) => quote(r),
                None => {
                    // The range the application's own `package.json` asks for, else what is installed.
                    let declared = project.as_ref().and_then(|p| ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"].iter().find_map(|section| p.get(section).and_then(|d| d.get(key)).and_then(Json::as_str)));
                    match (declared, &version) {
                        (Some(range), _) if !range.contains(':') && !range.contains('/') => quote(range),
                        (_, Some(v)) => quote(&format!("^{v}")),
                        _ => "false".to_string(),
                    }
                }
            };
            let scope = item.share_scope.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ");
            let filters = format!("{}{}", item.include.as_ref().map_or(String::new(), |j| format!(", include: {j}")), item.exclude.as_ref().map_or(String::new(), |j| format!(", exclude: {j}")));
            let config = format!("{{ singleton: {}, requiredVersion: {required}, strictVersion: {}, eager: {}{filters} }}", item.singleton, item.strict_version, item.eager);
            let version_text = version.as_deref().map_or("\"0\"".to_string(), quote);
            // With treeShaking: a second copy that has only the exports the application reads, for others whose needs it covers.
            let shaken = match (&item.tree_shaking, &fallback, reads.get(key)) {
                (Some(mode), Some(path), Some((Some(used), true))) if !used.is_empty() => {
                    let list: Vec<&String> = used.iter().collect();
                    let names = list.iter().map(|n| quote(n)).collect::<Vec<_>>().join(", ");
                    let mut source = String::new();
                    for name in &list {
                        if name.as_str() == "default" {
                            source.push_str(&format!("export {{ default }} from {};\n", quote(&path.to_string_lossy())));
                        } else if name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                            source.push_str(&format!("export {{ {name} }} from {};\n", quote(&path.to_string_lossy())));
                        } else {
                            source.push_str(&format!("export {{ {} as {} }} from {};\n", quote(name), quote(name), quote(&path.to_string_lossy())));
                        }
                    }
                    files.insert(PathBuf::from(format!("{ROOT}/shaken/{key}.js")), source);
                    let file = format!("shaken-{}.js", key.replace(['/', '@'], "_"));
                    shaken.push((key.to_string(), file.clone()));
                    format!(", treeShaking: {{ mode: {}, usedExports: [{names}], get: () => import(new URL({}, import.meta.url).href).then((ns) => () => rt.fallbackValue(ns)) }}", quote(mode), quote(&file))
                }
                (Some(_), Some(_), Some((_, false))) => {
                    skipped.push(format!("{key}: it is CommonJS, whose exports are only known as it runs"));
                    String::new()
                }
                _ => String::new(),
            };
            let fetch = match &fallback {
                // Bundled in this chunk, with nothing to fetch, but run when first read: a package that reads another shared
                // package has to wait for it, and an import would run it as soon as this chunk loads.
                Some(path) if item.eager => format!("lib: () => rt.fallbackValue(require({}))", quote(&path.to_string_lossy())),
                Some(path) => format!("get: () => import({}).then((ns) => () => rt.fallbackValue(ns))", quote(&path.to_string_lossy())),
                None => "consumeOnly: true, get: () => Promise.reject(new Error(\"the shared package is not bundled here and no other application provides it\"))".to_string(),
            };
            shared_lines.push(format!("    {}: {{ version: {version_text}, scope: [{scope}], shareConfig: {config}, {fetch}{shaken} }},\n", quote(key)));
            shared_used.push(key.to_string());
        };
        for item in &self.shared {
            if item.key.ends_with('/') {
                continue;
            }
            register(item, &item.key);
        }
        let used_shared: Vec<&str> = used.iter().filter_map(|u| u.strip_prefix("shared:")).collect();
        for key in &used_shared {
            if self.shared.iter().any(|s| s.key == *key) {
                continue;
            }
            if let Some(item) = self.shared_for(key) {
                register(item, key);
            }
        }

        // Plugins that run in the runtime, called with their arguments.
        let mut plugin_imports = String::new();
        let mut plugin_calls: Vec<String> = Vec::new();
        for (i, plugin) in self.runtime_plugins.iter().enumerate() {
            match resolve_path(&plugin.path) {
                Ok((path, _)) => {
                    plugin_imports.push_str(&format!("import __plugin{i} from {};\n", quote(&path.to_string_lossy())));
                    plugin_calls.push(format!("__plugin{i}({})", plugin.params.as_deref().unwrap_or("")));
                }
                Err(message) => problems.push(problem(format!("runtime plugin {}: {message}", plugin.path))),
            }
        }

        let mut remotes = String::new();
        for r in &self.remotes {
            let scope = if r.share_scope.len() == 1 { quote(&r.share_scope[0]) } else { format!("[{}]", r.share_scope.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ")) };
            let how = match r.kind.as_str() {
                "code" => format!("type: \"code\", entry: \"\", load: () => ({})", r.entry),
                "internal" => match resolve_path(&r.entry) {
                    Ok((path, _)) => {
                        // It is read like a bundled copy of a shared package: by the runtime, to give a container.
                        self.fallbacks.lock().expect("fallbacks").insert(path.clone(), format!("internal:{}", r.name));
                        internal.push(path.clone());
                        format!("type: \"code\", entry: \"\", internal: true, load: () => import({})", quote(&path.to_string_lossy()))
                    }
                    Err(message) => {
                        problems.push(problem(format!("remote {}: {message}", r.name)));
                        continue;
                    }
                },
                kind => format!("entry: {}, type: {}, entryGlobalName: {}", quote(&r.entry), quote(kind), quote(&r.global_name)),
            };
            remotes.push_str(&format!("    {{ name: {}, {how}, shareScope: {scope} }},\n", quote(&r.name)));
        }

        let mut expose_lines = String::new();
        for expose in &self.exposes {
            match resolve_path(&expose.import) {
                Ok((path, _)) => expose_lines.push_str(&format!("  {}: () => import({}),\n", quote(&expose.key), quote(&path.to_string_lossy()))),
                Err(message) => problems.push(problem(format!("exposed module {}: {message}", expose.key))),
            }
        }

        // What `start.js` waits for: the packages and remotes the code imports statically.
        let mut preload = String::new();
        let loaded: BTreeSet<String> = shared_used.iter().map(|k| format!("shared:{k}")).filter(|k| used.contains(k)).collect();
        // A package is loaded after the ones it reads; two that read each other are loaded together.
        let reaches = |from: &str, to: &str| -> bool {
            let mut seen = BTreeSet::new();
            let mut stack = vec![from.to_string()];
            while let Some(k) = stack.pop() {
                if k == to {
                    return true;
                }
                if seen.insert(k.clone()) {
                    stack.extend(needs.get(&k).into_iter().flatten().cloned());
                }
            }
            false
        };
        for key in &shared_used {
            let id = format!("shared:{key}");
            if used.contains(&id) {
                let before: Vec<String> = needs.get(&id).into_iter().flatten().filter(|d| loaded.contains(*d) && **d != id && !reaches(d, &id)).map(|d| quote(d)).collect();
                preload.push_str(&format!("  [{}, () => mf.loadShare({}).then((f) => rt.fallbackValue(f())), [{}]],\n", quote(&id), quote(key), before.join(", ")));
            }
        }
        for entry in used.iter().filter(|u| u.starts_with("remote:")) {
            let id = &entry["remote:".len()..];
            // A container that is a module of the application reads shared packages when it runs.
            let before: Vec<String> = self
                .remote_for(id)
                .and_then(|r| needs.get(&format!("internal:{}", r.name)))
                .into_iter()
                .flatten()
                .filter(|d| loaded.contains(*d))
                .map(|d| quote(d))
                .collect();
            preload.push_str(&format!("  [{}, () => mf.loadRemote({}).then(rt.moduleValue), [{}]],\n", quote(entry), quote(id), before.join(", ")));
        }

        let scope_list = self.share_scope.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ");
        let version_line = self.version.as_deref().map_or(String::new(), |v| format!("  version: {},\n", quote(v)));
        let core = format!(
            "import * as rt from {runtime};\nimport {{ values }} from {values};\n{plugin_imports}\nexport const mf = rt.init({{\n  name: {name},\n{version_line}  shareStrategy: {strategy},\n  remotes: [\n{remotes}  ],\n  shared: {{\n{shared}  }},\n  plugins: [{plugins}],\n}});\n\nconst preload = [\n{preload}];\nconst byKey = new Map(preload.map((entry) => [entry[0], entry]));\nconst started = new Map();\nconst start = (key) => {{\n  if (!started.has(key)) {{\n    const [, load, before] = byKey.get(key);\n    started.set(key, Promise.all(before.map(start)).then(load).then((value) => {{ values[key] = value; }}));\n  }}\n  return started.get(key);\n}};\nlet loading;\nexport function ready() {{\n  return (loading ||= Promise.all(preload.map(([key]) => start(key))).then(() => {{}}));\n}}\n\nconst exposes = {{\n{exposes}}};\nexport const container = rt.containerFor(mf, {{ exposes, ready, shareScope: [{scope_list}] }});\nexport const loadRemoteNamespace = (id) => mf.loadRemote(id).then(rt.moduleNamespace);\nexport const loadSharedNamespace = (key) => mf.loadShare(key).then((f) => rt.moduleNamespace(rt.fallbackValue(f())));\n",
            runtime = quote(&format!("{ROOT}/runtime.js")),
            values = quote(&format!("{ROOT}/values.js")),
            name = quote(&self.name),
            strategy = quote(&self.share_strategy),
            remotes = remotes,
            shared = shared_lines.concat(),
            plugins = plugin_calls.join(", "),
            preload = preload,
            exposes = expose_lines,
        );
        files.insert(at("core.js"), core);

        // Each entry point starts the runtime and waits for what it needs before its own code runs.
        for (i, entry) in entries.iter().enumerate() {
            let stem = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "entry.js".to_string());
            files.insert(at(&format!("entry/{i}/{stem}")), format!("import {};\nexport * from {};\n", quote(&format!("{ROOT}/start.js")), quote(&entry.to_string_lossy())));
        }

        if !problems.is_empty() {
            return Err(problems);
        }
        *self.files.write().expect("files") = Arc::new(files);
        *self.internal.lock().expect("internal") = internal;
        *self.shaken.lock().expect("shaken") = shaken;
        *self.shaken_skipped.lock().expect("shaken_skipped") = skipped;
        Ok(())
    }

    /// Builds the module graph with the federation modules in it. What the application imports from
    /// remotes and shared packages is only known once the graph exists, and the modules that load
    /// them are written from that, so the graph is built again until nothing new turns up.
    /// The aliases of the router bridge (`@module-federation/bridge-react`): `react-router-dom` is read through the bridge's router for
    /// the version of React Router the project has, and the bridge's own imports of the real package go to it. As the real plugin does it,
    /// only when the bridge's router file is installed, and an alias the project set itself wins.
    fn bridge_aliases(&self, existing: &[Alias]) -> Result<Vec<Alias>, String> {
        let modules = self.cwd.join("node_modules");
        let bridge = modules.join("@module-federation").join("bridge-react").join("dist");
        let dependencies = std::fs::read_to_string(self.cwd.join("package.json")).ok().and_then(|t| Json::parse(&t).ok());
        let has_dependency = |name: &str| ["dependencies", "devDependencies"].iter().any(|k| dependencies.as_ref().and_then(|d| d.get(k)).and_then(|d| d.get(name)).is_some());
        let enabled = self.bridge_router.unwrap_or_else(|| has_dependency("@module-federation/bridge-react"));
        let alias = |pattern: &str, to: PathBuf| Alias { pattern: pattern.to_string(), replacements: vec![to.to_string_lossy().into_owned()], exact_only: true };
        if !enabled {
            // Without the router, the package is read from its base entry.
            let base = bridge.join("base.es.js");
            return Ok(if has_dependency("@module-federation/bridge-react") && base.is_file() { vec![alias("@module-federation/bridge-react", base)] } else { Vec::new() });
        }
        if !bridge.join("router.es.js").is_file() {
            return Ok(Vec::new());
        }
        if self.shared.iter().any(|s| s.key == "react-router-dom") {
            return Err("react-router-dom cannot be shared when the router bridge is used".to_string());
        }
        let own = existing.iter().find(|a| a.pattern == "react-router-dom" || a.pattern == "react-router-dom$").and_then(|a| a.replacements.first()).map(PathBuf::from);
        let package = own.or_else(|| if has_dependency("react-router") { Some(modules.join("react-router")) } else if has_dependency("react-router-dom") { Some(modules.join("react-router-dom")) } else { None });
        let major = package.as_ref().and_then(|p| {
            let mut dir = Some(p.as_path());
            while let Some(d) = dir {
                if let Ok(text) = std::fs::read_to_string(d.join("package.json")) {
                    return Json::parse(&text).ok()?.get("version")?.as_str()?.split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())?.parse::<u32>().ok();
                }
                dir = d.parent();
            }
            None
        });
        let router = |v: u32| bridge.join(format!("router-v{v}.es.js"));
        let real = package.clone().unwrap_or_default();
        Ok(match (major, package) {
            (Some(5), Some(_)) => vec![alias("react-router-dom", router(5)), alias("react-router-dom/index.js", real)],
            (Some(6), Some(_)) => vec![alias("react-router-dom", router(6)), alias("react-router-dom/dist/index.js", real)],
            (Some(_), Some(_)) | (None, Some(_)) => vec![
                alias("react-router", router(7)),
                alias("react-router-dom", router(7)),
                alias("react-router/dist/development/index.js", real.clone()),
                alias("react-router/dist/production/index.js", real.clone()),
                alias("react-router-dom/dist/index.js", real),
            ],
            _ => vec![alias("react-router-dom", router(6))],
        })
    }

    pub fn build_graph(self: &Arc<Self>, cwd: &Path, entries: &[String], options: &crate::graph::GraphOptions) -> Result<crate::graph::Graph, Vec<Diagnostic>> {
        let mut with_bridge;
        let options = match self.bridge_aliases(&options.resolve.alias) {
            Ok(extra) if extra.is_empty() => options,
            Ok(extra) => {
                with_bridge = options.clone();
                with_bridge.resolve.alias.extend(extra);
                &with_bridge
            }
            Err(message) => return Err(vec![Diagnostic { severity: Severity::Error, message, file: None, pos: None }]),
        };
        let resolver = Resolver::new(options.resolve.clone());
        let mut absolute: Vec<PathBuf> = Vec::new();
        let mut problems = Vec::new();
        for entry in entries {
            let specifier = crate::graph::entry_specifier(entry);
            match resolver.resolve(cwd, &specifier, ResolveKind::Import) {
                Ok(found) => absolute.push(found.path),
                Err(e) => problems.push(Diagnostic { severity: Severity::Error, message: format!("Entry point: {e}"), file: None, pos: None }),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let mut roots: Vec<String> = absolute.iter().enumerate().map(|(i, p)| self.wrap_entry(i, p)).collect();
        roots.push(self.core_path().to_string_lossy().into_owned());
        let mut alone = options.clone();
        alone.federation = None;
        alone.cache = None;
        *self.graph_options.lock().expect("graph_options") = Some(alone);
        let cache = options.cache.clone().unwrap_or_default();
        let mut used = BTreeSet::new();
        let mut round_options = options.clone();
        round_options.cache = Some(cache.clone());
        for round in 0..6 {
            self.generate(&options.resolve, &absolute, &used)?;
            let graph = crate::graph::build(cwd, &roots, &round_options);
            let now = self.used();
            let needs_changed = self.learn(&graph);
            if (now == used && !needs_changed) || round == 5 || graph.has_errors() {
                return Ok(graph);
            }
            used = now;
            cache.store(graph);
        }
        unreachable!("the loop returns")
    }

    /// The entry point `entry` is built through: it starts the runtime first.
    pub fn wrap_entry(&self, index: usize, entry: &Path) -> String {
        let stem = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "entry.js".to_string());
        format!("{ROOT}/entry/{index}/{stem}")
    }

    /// Declarations for what the application exposes, as files to add to the output. Empty when there is nothing to
    /// describe, or no TypeScript to do it with (a JavaScript project is not told).
    fn generate_types(&self) -> (Vec<OutputFile>, Vec<String>, Option<String>) {
        let mut messages = Vec::new();
        if !self.dts.generate || self.exposes.is_empty() {
            return (Vec::new(), messages, None);
        }
        let resolver = Resolver::new(ResolveOptions::default());
        let mut exposes = Vec::new();
        for expose in &self.exposes {
            if let Ok(found) = resolver.resolve(&self.cwd, &expose.import, ResolveKind::Import) {
                exposes.push((expose.key.clone(), found.path));
            }
        }
        let typed = exposes.iter().any(|(_, p)| p.extension().is_some_and(|e| matches!(e.to_str(), Some("ts" | "tsx" | "mts" | "cts")))) || self.cwd.join("tsconfig.json").is_file();
        if !typed || exposes.is_empty() {
            return (Vec::new(), messages, None);
        }
        // The declarations are written to a folder of ours, new and private, and read from there.
        let out_dir = match private_dir("rathkit-types") {
            Ok(dir) => dir,
            Err(message) => {
                messages.push(format!("types were not written: {message}"));
                return (Vec::new(), messages, None);
            }
        };
        let list = exposes.iter().map(|(k, p)| format!("{{\"key\":{},\"file\":{}}}", quote(k), quote(&p.to_string_lossy()))).collect::<Vec<_>>().join(",");
        let input = format!(
            "{{\"command\":\"generate\",\"cwd\":{},\"name\":{},\"exposes\":[{list}],\"outDir\":{},\"tsConfigPath\":{},\"typesFolder\":{},\"compiledTypesFolder\":{},\"additionalFiles\":[{}],\"generateApi\":{},\"compilerInstance\":{},\"extractThirdParty\":{},\"extractRemoteTypes\":{},\"hostTypesFolder\":{},\"afterGenerate\":{}}}",
            quote(&self.cwd.to_string_lossy()),
            quote(&self.name),
            quote(&out_dir.to_string_lossy()),
            self.dts.ts_config.as_deref().map_or("null".to_string(), quote),
            quote(&self.dts.types_folder),
            quote(&self.dts.compiled_types_folder),
            self.dts.additional_files.iter().map(|f| quote(&self.cwd.join(f).to_string_lossy())).collect::<Vec<_>>().join(","),
            self.dts.generate_api,
            quote(&self.dts.compiler),
            self.dts.extract_third_party.as_ref().map_or("false".to_string(), |exclude| format!("{{\"exclude\":[{}]}}", exclude.iter().map(|e| quote(e)).collect::<Vec<_>>().join(","))),
            self.dts.extract_remote_types,
            quote(&self.dts.consume_folder),
            quote(self.dts.after_generate.as_deref().unwrap_or("")),
        );
        let answer = match run_node(DTS, &input, &self.cwd) {
            Ok(answer) => answer,
            Err(message) => {
                let _ = std::fs::remove_dir_all(&out_dir);
                messages.push(format!("types were not written: {message}"));
                return (Vec::new(), messages, None);
            }
        };
        if let Some(Json::Array(notes)) = answer.get("messages") {
            messages.extend(notes.iter().filter_map(|n| n.as_str().map(String::from)));
        }
        if let Some(error) = answer.get("error").and_then(Json::as_str) {
            let _ = std::fs::remove_dir_all(&out_dir);
            messages.push(format!("types were not written: {}", error.lines().next().unwrap_or("")));
            return (Vec::new(), messages, None);
        }
        let type_errors = answer.get("typeErrors").and_then(Json::as_f64).unwrap_or(0.0) as u64;
        let abort = (self.dts.abort_generate && type_errors > 0).then(|| format!("{type_errors} type error(s) in the exposed modules (abortOnError)"));
        let mut files = Vec::new();
        if answer.get("written").and_then(Json::as_bool) == Some(true) {
            fn walk(dir: &Path, base: &str, out: &mut Vec<(String, Vec<u8>)>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let name = format!("{base}{}", entry.file_name().to_string_lossy());
                        if entry.path().is_dir() {
                            walk(&entry.path(), &format!("{name}/"), out);
                        } else if let Ok(bytes) = std::fs::read(entry.path()) {
                            out.push((name, bytes));
                        }
                    }
                }
            }
            let mut found = Vec::new();
            walk(&out_dir, "", &mut found);
            found.sort();
            for (name, bytes) in found {
                let name = match &self.dts.output_dir {
                    Some(dir) => format!("{}/{name}", dir.trim_matches('/')),
                    None => name,
                };
                files.push(OutputFile { name, code: String::new(), map: None, data: Some(bytes), original: None });
            }
        }
        let _ = std::fs::remove_dir_all(&out_dir);
        (files, messages, abort)
    }

    /// Downloads the declarations of the remotes this application imports from, into the project. Remotes that are not
    /// up, or publish none, are skipped; this is for the editor, and a build does not depend on it. While watching, it
    /// happens beside the build and is tried again, at most every ten seconds, until every remote has answered.
    pub fn read_types(self: &Arc<Self>, background: bool) -> Result<(), String> {
        use std::sync::atomic::Ordering::SeqCst;
        if !self.dts.consume || self.remotes.is_empty() || !self.cwd.join("tsconfig.json").is_file() {
            return Ok(());
        }
        // `dev.disableHotTypesReload`: after the first read, watching leaves the types alone.
        if background && !self.dts.hot_types && self.types_read.load(SeqCst) {
            return Ok(());
        }
        // Installing packages removes the folders that stand for the remotes in node_modules; they are put back from what was read.
        if self.types_read.load(SeqCst) && background {
            let gone = self.types_remotes.lock().expect("types_remotes").iter().any(|name| !self.cwd.join("node_modules").join(name).join("package.json").is_file());
            if gone {
                let input = format!("{{\"command\":\"link\",\"cwd\":{},\"typesFolder\":{}}}", quote(&self.cwd.to_string_lossy()), quote(&self.dts.consume_folder));
                let _ = run_node(DTS, &input, &self.cwd);
            }
        }
        // A one-off build reads once; while watching, the remotes are asked again (at most every ten seconds), since a remote that
        // is up may have new types, and a zip that has not changed leaves the files alone.
        if self.types_read.load(SeqCst) && !background {
            return Ok(());
        }
        if background {
            if self.last_read.lock().expect("last_read").is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(std::env::var("RATHKIT_TYPES_RETRY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000))) || self.reading.swap(true, SeqCst) {
                return Ok(());
            }
            *self.last_read.lock().expect("last_read") = Some(std::time::Instant::now());
            let this = self.clone();
            std::thread::spawn(move || {
                this.read_types_now();
                this.reading.store(false, SeqCst);
            });
            Ok(())
        } else {
            let failed = self.read_types_now();
            match (self.dts.abort_consume, failed.is_empty()) {
                (true, false) => Err(format!("the types of {} could not be read (abortOnError)", failed.join(", "))),
                _ => Ok(()),
            }
        }
    }

    fn read_types_now(&self) -> Vec<String> {
        let remotes = self.remotes.iter().filter(|r| !matches!(r.kind.as_str(), "code" | "internal")).map(|r| format!("{{\"name\":{},\"entry\":{}}}", quote(&r.name), quote(&r.entry))).collect::<Vec<_>>().join(",");
        let input = format!("{{\"command\":\"consume\",\"cwd\":{},\"remotes\":[{remotes}],\"typesFolder\":{},\"updateTsConfig\":{},\"timeout\":{},\"maxRetries\":{},\"typeUrls\":{{{}}},\"family\":{},\"remoteTypesFolder\":{},\"consumeApiTypes\":{},\"runtimePkgs\":[{}]}}", quote(&self.cwd.to_string_lossy()), quote(&self.dts.consume_folder), self.dts.update_ts_config, self.dts.timeout_ms, self.dts.max_retries, self.dts.remote_type_urls.iter().map(|(n, z, a)| format!("{}:{{\"zip\":{},\"api\":{}}}", quote(n), quote(z), quote(a))).collect::<Vec<_>>().join(","), self.dts.family, quote(self.dts.remote_types_folder.as_deref().unwrap_or("")), self.dts.consume_api_types, self.dts.runtime_pkgs.iter().map(|p| quote(p)).collect::<Vec<_>>().join(","));
        match run_node(DTS, &input, &self.cwd) {
            Ok(answer) => {
                if let Some(Json::Array(notes)) = answer.get("messages") {
                    for note in notes.iter().filter_map(Json::as_str) {
                        eprintln!("warning: {note}");
                    }
                }
                let got = answer.get("fetched").and_then(Json::as_array).map_or(0, |a| a.len());
                *self.types_remotes.lock().expect("types_remotes") = answer.get("fetched").and_then(Json::as_array).map_or(Vec::new(), |a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect());
                let failed: Vec<String> = answer.get("failed").and_then(Json::as_array).map_or(Vec::new(), |a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect());
                if got == self.remotes.iter().filter(|r| !matches!(r.kind.as_str(), "code" | "internal")).count() {
                    self.types_read.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                failed
            }
            Err(message) => {
                eprintln!("warning: types of remotes were not read: {message}");
                self.remotes.iter().filter(|r| !matches!(r.kind.as_str(), "code" | "internal")).map(|r| r.name.clone()).collect()
            }
        }
    }

    /// Calls `hook` with declarations written after the build that asked for them.
    pub fn on_types(&self, hook: Arc<dyn Fn(&[OutputFile]) + Send + Sync>) {
        *self.types_hook.lock().expect("types_hook") = Some(hook);
    }

    /// The declarations of the exposed modules. The first run is part of the build (the manifest says whether there
    /// are any); when building again and again, the earlier ones are used and new ones are written beside the build.
    fn types_files(self: &Arc<Self>, repeated: bool) -> Result<Vec<OutputFile>, String> {
        use std::sync::atomic::Ordering::SeqCst;
        let cached = self.types_cache.lock().expect("types_cache").clone();
        match cached {
            Some(files) if repeated => {
                if !self.types_running.swap(true, SeqCst) {
                    let this = self.clone();
                    std::thread::spawn(move || {
                        let (files, messages, _) = this.generate_types();
                        for message in messages {
                            eprintln!("warning: {message}");
                        }
                        if !files.is_empty() {
                            *this.types_cache.lock().expect("types_cache") = Some(files.clone());
                            if let Some(hook) = this.types_hook.lock().expect("types_hook").clone() {
                                hook(&files);
                            }
                        }
                        this.types_running.store(false, SeqCst);
                    });
                }
                Ok(files)
            }
            _ => {
                let (files, messages, abort) = self.generate_types();
                for message in messages {
                    eprintln!("warning: {message}");
                }
                if let Some(reason) = abort {
                    return Err(reason);
                }
                *self.types_cache.lock().expect("types_cache") = Some(files.clone());
                Ok(files)
            }
        }
    }

    /// The bundle settings of the build, for the copies that are built apart.
    pub fn remember(&self, options: &crate::bundle::Options) {
        *self.bundle_options.lock().expect("bundle_options") = Some(options.clone());
    }

    /// The copies of shared packages with only the exports the application reads: each a bundle of its own, so it carries
    /// nothing the others in the build use (code shared between chunks would pull in the whole package).
    fn shaken_files(self: &Arc<Self>) -> Vec<OutputFile> {
        let list = self.shaken.lock().expect("shaken").clone();
        let (Some(mut graph_options), Some(mut bundle_options)) = (self.graph_options.lock().expect("graph_options").clone(), self.bundle_options.lock().expect("bundle_options").clone()) else { return Vec::new() };
        bundle_options.splitting = false;
        bundle_options.federation = None;
        bundle_options.sourcemap = crate::bundle::SourceMapMode::None;
        bundle_options.entry_names = Vec::new();
        bundle_options.file_name = None;
        // Other shared packages it reads are read from the application's table of what was loaded, as in the rest of the build.
        graph_options.federation = Some(self.clone());
        let used = self.reads.lock().expect("reads").clone();
        let mut files = Vec::new();
        for (key, file) in list {
            // The same copy as last time unless a file it is made of, or what is read from it, has changed.
            let signature = format!("{key}|{:?}|{:?}", used.get(&key).map(|(u, _)| u), bundle_options.minify);
            if let Some((stamps, code)) = self.shaken_cache.lock().expect("shaken_cache").get(&signature) {
                if stamps.iter().all(|(path, time)| std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|t| t == *time)) {
                    files.push(OutputFile { name: file, code: code.clone(), map: None, data: None, original: None });
                    continue;
                }
            }
            let entry = format!("{ROOT}/shaken/{key}.js");
            let fs = Box::new(Overlay { federation: self.clone(), inner: Box::new(crate::resolve::RealFs) });
            let graph = crate::graph::build_with_fs(fs, &self.cwd, &[entry], &graph_options);
            if graph.has_errors() {
                eprintln!("warning: the copy of {key} with only the exports in use was not built: {}", graph.diagnostics.iter().find(|d| d.severity == Severity::Error).map_or(String::new(), |d| d.message.clone()));
                continue;
            }
            let linked = crate::link::link(&graph);
            let shaken = crate::shake::shake(&graph, &linked);
            match crate::bundle::bundle(&graph, &linked, &shaken, &bundle_options) {
                Ok(output) => {
                    let stamps: Vec<(PathBuf, std::time::SystemTime)> = graph.modules.iter().filter_map(|m| std::fs::metadata(&m.path).and_then(|md| md.modified()).ok().map(|t| (m.path.clone(), t))).collect();
                    self.shaken_cache.lock().expect("shaken_cache").insert(signature, (stamps, output.code.clone()));
                    files.push(OutputFile { name: file, code: output.code, map: None, data: None, original: None });
                }
                Err(problems) => eprintln!("warning: the copy of {key} with only the exports in use was not built: {}", problems.first().map_or("", |d| d.message.as_str())),
            }
        }
        files
    }

    /// The files others read: the entry that names the container, and the manifest.
    pub fn finish(self: &Arc<Self>, output: &mut Output, hot: bool, repeated: bool) -> Result<(), String> {
        let core = self.core_path().to_string_lossy().into_owned();
        let core_file = output.chunks.iter().find(|c| c.facade_module.as_deref() == Some(core.as_str())).map(|c| c.file.clone()).ok_or("the federation runtime was not built; the build needs the esm format with splitting")?;
        if !self.exposes.is_empty() {
            let styles: Vec<(String, Vec<String>)> = self.expose_assets(output).into_iter().filter(|(_, _, _, css)| !css.is_empty()).map(|(e, _, _, css)| (e.key.clone(), css)).collect();
            output.files.push(OutputFile { name: self.filename.clone(), code: self.remote_entry(&core_file, hot && self.live_reload, &styles), map: None, data: None, original: None });
        }
        for reason in self.shaken_skipped.lock().expect("shaken_skipped").iter() {
            eprintln!("warning: no copy of {reason} was written with only the exports in use; it is shared whole");
        }
        let started = std::time::Instant::now();
        output.files.extend(self.shaken_files());
        if std::env::var_os("RATHKIT_TIMING").is_some() {
            eprintln!("  federation: copies with only some exports: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        let types = self.types_files(repeated)?;
        let zip_name = match &self.dts.output_dir {
            Some(dir) => format!("{}/{}.zip", dir.trim_matches('/'), self.dts.types_folder),
            None => format!("{}.zip", self.dts.types_folder),
        };
        let has_types = types.iter().any(|f| f.name == zip_name);
        output.files.extend(types);
        if let Some(name) = &self.manifest {
            let manifest = self.manifest_json(output, has_types);
            output.files.push(OutputFile { name: name.clone(), code: manifest, map: None, data: None, original: None });
        }
        Ok(())
    }

    /// The script that defines the container: it loads the runtime chunk when a host asks for something.
    fn remote_entry(&self, core_file: &str, hot: bool, styles: &[(String, Vec<String>)]) -> String {
        let file = quote(core_file);
        let base = match &self.public_path {
            // `getPublicPath` is the body of a function; its answer gets the trailing slash the files are joined with.
            _ if self.public_path_code.is_some() => format!("((p) => p.endsWith('/') ? p : p + '/')((function () {{ {} }})())", self.public_path_code.as_deref().unwrap_or("")),
            Some(p) => quote(&if p.ends_with('/') { p.clone() } else { format!("{p}/") }),
            // SystemJS runs the module later, when there is no current script; it says where the module is.
            None if self.library_type == "system" => "context.meta.url.slice(0, context.meta.url.lastIndexOf('/') + 1)".to_string(),
            None => "(document.currentScript && document.currentScript.src ? document.currentScript.src.slice(0, document.currentScript.src.lastIndexOf('/') + 1) : '')".to_string(),
        };
        // The style sheets of an exposed module load with it, so a host that knows nothing of them still gets them.
        let table = styles.iter().map(|(key, files)| format!("{}: [{}]", quote(key), files.iter().map(|f| quote(f)).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join(", ");
        let sheets = if styles.is_empty() {
            String::new()
        } else {
            format!(
                "  const styles = {{ {table} }};\n  const sheets = (module) => Promise.all((styles[module] || []).map((file) => new Promise((resolve) => {{\n    if (typeof document === 'undefined') return resolve();\n    const href = new URL(file, base || document.baseURI).href;\n    if ([...document.querySelectorAll('link[rel=\"stylesheet\"]')].some((l) => l.href === href)) return resolve();\n    const link = document.createElement('link');\n    link.rel = 'stylesheet';\n    link.href = href;\n    link.onload = link.onerror = () => resolve();\n    document.head.appendChild(link);\n  }})));\n"
            )
        };
        let get = if styles.is_empty() { "core().then((c) => c.container.get(module))".to_string() } else { "Promise.all([sheets(module), core().then((c) => c.container.get(module))]).then((both) => both[1])".to_string() };
        if self.library_type == "module" {
            let start = if self.public_path.is_some() || self.public_path_code.is_some() { base.clone() } else { "(import.meta.url.slice(0, import.meta.url.lastIndexOf('/') + 1))".to_string() };
            return format!(
                "const base = {start};\n{sheets}const core = () => import(/* @vite-ignore */ new URL({file}, base).href);\nexport const get = (module) => {get};\nexport const init = (scope, initScope, options) => core().then((c) => c.container.init(scope, initScope, options));\n",
            );
        }
        // A development server tells the pages that use this application to reload when it builds again.
        let reload = if hot {
            "  if (typeof EventSource !== 'undefined' && base) {\n    let opened = false;\n    const events = new EventSource(base + '__rathkit/events');\n    events.onmessage = (event) => {\n      const message = JSON.parse(event.data);\n      if (message.type === 'connected') {\n        if (opened) location.reload();\n        opened = true;\n      } else if (message.type === 'full-reload' || message.type === 'css-update') location.reload();\n    };\n  }\n"
        } else {
            ""
        };
        let container = format!("(() => {{\n  const base = {base};\n{reload}{sheets}  const core = () => import(/* @vite-ignore */ base + {file});\n  return {{\n    get: (module) => {get},\n    init: (scope, initScope, options) => core().then((c) => c.container.init(scope, initScope, options)),\n  }};\n}})()");
        let name = &self.library_name;
        match self.library_type.as_str() {
            "window" | "self" | "global" | "globalThis" => format!("{}[{}] = {container};\n", if self.library_type == "global" { "globalThis" } else { &self.library_type }, quote(name)),
            "this" => format!("this[{}] = {container};\n", quote(name)),
            "assign" | "assign-properties" => format!("{name} = {container};\n"),
            "commonjs" | "commonjs2" => format!("module.exports = {container};\n"),
            "jsonp" => format!("{name}({container});\n"),
            "system" => format!("System.register([], (exports, context) => ({{ execute: () => {{ exports({container}); }} }}));\n"),
            "umd" => format!("(function (root, factory) {{ if (typeof exports === 'object' && typeof module === 'object') module.exports = factory(); else root[{}] = factory(); }})(typeof self !== 'undefined' ? self : this, () => {container});\n", quote(name)),
            _ => format!("var {name};\n{name} = {container};\n"),
        }
    }

    /// The files each exposed module needs: its chunk and the chunks it imports when it loads, the ones it may load later,
    /// and the style sheets of all of them.
    fn expose_assets<'a>(&'a self, output: &Output) -> Vec<(&'a Expose, Vec<String>, Vec<String>, Vec<String>)> {
        let by_facade: HashMap<&str, &ChunkInfo> = output.chunks.iter().filter_map(|c| c.facade_module.as_deref().map(|m| (m, c))).collect();
        let by_file: HashMap<&str, &ChunkInfo> = output.chunks.iter().map(|c| (c.file.as_str(), c)).collect();
        let css_of = |chunk: &ChunkInfo| -> Vec<String> {
            let stem = chunk.file.strip_suffix(".js").unwrap_or(&chunk.file);
            output.files.iter().filter(|f| f.name == format!("{stem}.css")).map(|f| f.name.clone()).collect()
        };
        let closure = |chunk: &ChunkInfo| -> (Vec<String>, Vec<String>) {
            let mut sync = vec![chunk.file.clone()];
            let mut seen: BTreeSet<String> = sync.iter().cloned().collect();
            let mut stack = vec![chunk.file.clone()];
            let mut later: Vec<String> = Vec::new();
            while let Some(file) = stack.pop() {
                if let Some(c) = by_file.get(file.as_str()) {
                    for import in &c.imports {
                        if by_file.contains_key(import.as_str()) && seen.insert(import.clone()) {
                            sync.push(import.clone());
                            stack.push(import.clone());
                        }
                    }
                    for import in &c.dynamic_imports {
                        if by_file.contains_key(import.as_str()) && !later.contains(import) {
                            later.push(import.clone());
                        }
                    }
                }
            }
            later.retain(|f| !seen.contains(f));
            (sync, later)
        };
        let mut found = Vec::new();
        for expose in &self.exposes {
            let target = self.resolve_for_manifest(&expose.import);
            let chunk = target.as_deref().and_then(|t| by_facade.get(t));
            let (js_sync, js_async, css) = match chunk {
                Some(chunk) => {
                    let (sync, later) = closure(chunk);
                    let css: Vec<String> = sync.iter().filter_map(|f| by_file.get(f.as_str())).flat_map(|c| css_of(c)).collect();
                    (sync, later, css)
                }
                None => (Vec::new(), Vec::new(), Vec::new()),
            };
            found.push((expose, js_sync, js_async, css));
        }
        found
    }

    fn manifest_json(&self, output: &Output, has_types: bool) -> String {
        let by_facade: HashMap<&str, &ChunkInfo> = output.chunks.iter().filter_map(|c| c.facade_module.as_deref().map(|m| (m, c))).collect();
        let list = |items: &[String]| format!("[{}]", items.iter().map(|i| quote(i)).collect::<Vec<_>>().join(", "));
        let assets = |js_sync: &[String], js_async: &[String], css_sync: &[String]| format!("{{ \"js\": {{ \"async\": {}, \"sync\": {} }}, \"css\": {{ \"async\": [], \"sync\": {} }} }}", list(js_async), list(js_sync), list(css_sync));
        let mut exposes = Vec::new();
        for (expose, js_sync, js_async, css) in self.expose_assets(output) {
            let name = expose.key.trim_start_matches("./");
            exposes.push(format!("    {{ \"id\": {}, \"name\": {}, \"path\": {}, \"assets\": {} }}", quote(&format!("{}:{name}", self.name)), quote(name), quote(&expose.key), assets(&js_sync, &js_async, &css)));
        }
        let mut shared = Vec::new();
        let core_file = by_facade.get(self.core_path().to_string_lossy().as_ref()).map(|c| c.file.clone()).unwrap_or_default();
        for item in &self.shared {
            if item.key.ends_with('/') {
                continue;
            }
            let version = item.version.clone().or_else(|| self.installed_version(&item.key));
            let required = match &item.required_version {
                Some(RequiredVersion::Range(r)) => quote(r),
                Some(RequiredVersion::Any) => "\"*\"".to_string(),
                None => version.as_deref().map_or("\"*\"".to_string(), |v| quote(&format!("^{v}"))),
            };
            shared.push(format!(
                "    {{ \"id\": {}, \"name\": {}, \"version\": {}, \"singleton\": {}, \"requiredVersion\": {required}, \"assets\": {}, \"fallback\": \"\" }}",
                quote(&format!("{}:{}", self.name, item.key)),
                quote(&item.key),
                quote(version.as_deref().unwrap_or("0")),
                item.singleton,
                assets(&if core_file.is_empty() { Vec::new() } else { vec![core_file.clone()] }, &[], &[])
            ));
        }
        let remotes = self
            .remotes
            .iter()
            .map(|r| format!("    {{ \"federationContainerName\": {}, \"moduleName\": \"\", \"alias\": {}, \"entry\": {} }}", quote(&r.global_name), quote(&r.name), quote(&r.entry)))
            .collect::<Vec<_>>();
        let public_path = self.public_path.as_deref().unwrap_or("auto");
        format!(
            "{{\n  \"id\": {name},\n  \"name\": {name},\n  \"metaData\": {{\n    \"name\": {name},\n    \"type\": \"app\",\n    \"buildInfo\": {{ \"buildVersion\": {version}, \"buildName\": {name} }},\n    \"remoteEntry\": {{ \"name\": {file}, \"path\": \"\", \"type\": {kind} }},\n    \"types\": {{ \"path\": {types_path}, \"name\": \"\", \"zip\": {zip}, \"api\": {api} }},\n    \"globalName\": {global},\n    \"pluginVersion\": \"rathkit-pack\",\n    \"publicPath\": {public}\n  }},\n  \"shared\": [\n{shared}\n  ],\n  \"remotes\": [\n{remotes}\n  ],\n  \"exposes\": [\n{exposes}\n  ]\n}}\n",
            name = quote(&self.name),
            version = quote(self.version.as_deref().unwrap_or("local")),
            file = quote(&self.filename),
            kind = quote(match self.library_type.as_str() {
                "module" => "module",
                "commonjs" | "commonjs2" => "commonjs",
                "system" => "system",
                "jsonp" => "jsonp",
                _ => "global",
            }),
            global = quote(&self.library_name),
            zip = quote(&if has_types { format!("{}.zip", self.dts.types_folder) } else { String::new() }),
            api = quote(&if has_types && self.dts.generate_api { format!("{}.d.ts", self.dts.types_folder) } else { String::new() }),
            types_path = quote(self.dts.output_dir.as_deref().map_or("", |d| d.trim_matches('/'))),
            public = quote(public_path),
            shared = shared.join(",\n"),
            remotes = remotes.join(",\n"),
            exposes = exposes.join(",\n"),
        )
    }

    fn resolve_for_manifest(&self, import: &str) -> Option<String> {
        let resolver = Resolver::new(ResolveOptions::default());
        resolver.resolve(&self.cwd, import, ResolveKind::Import).ok().map(|f| f.path.to_string_lossy().into_owned())
    }

    fn installed_version(&self, key: &str) -> Option<String> {
        let resolver = Resolver::new(ResolveOptions::default());
        resolver.resolve(&self.cwd, key, ResolveKind::Import).ok().and_then(|f| f.package).and_then(|p| p.version.clone())
    }
}

/// How a request for a remote or a shared package is answered.
pub enum Intercept {
    /// A module that returns what was loaded at the start.
    Module(PathBuf),
    /// `import()`: asks the runtime when it runs.
    Dynamic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DynamicKind {
    Remote,
    Shared,
}

/// The real files, and the modules that exist only in memory on top of them.
pub struct Overlay {
    pub federation: Arc<Federation>,
    pub inner: Box<dyn FileSystem>,
}

impl FileSystem for Overlay {
    fn is_file(&self, path: &Path) -> bool {
        self.federation.files().contains_key(path) || self.federation.stub(path).is_some() || self.inner.is_file(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.starts_with(ROOT) && (path == Path::new(ROOT) || self.federation.files().keys().any(|k| k.starts_with(path)) || path.ends_with("shared") || path.ends_with("remote")) || self.inner.is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        if let Some(text) = self.federation.files().get(path) {
            return Ok(text.clone());
        }
        match self.federation.stub(path) {
            Some(text) => Ok(text),
            None => self.inner.read_to_string(path),
        }
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        if self.federation.files().contains_key(path) || self.federation.stub(path).is_some() {
            Ok(path.to_path_buf())
        } else {
            self.inner.canonicalize(path)
        }
    }
}

/// Runs a script with Node, which reads one JSON object on stdin and writes one on stdout. The script goes in a private
/// folder, new, so another user of the machine cannot swap it.
pub(crate) fn run_node(script: &str, input: &str, cwd: &Path) -> Result<Json, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("rathkit-dts-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos())));
    let file = dir.join("dts.mjs");
    {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&file).and_then(|mut f| f.write_all(script.as_bytes())).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    let spawned = Command::new(std::env::var_os("RATHKIT_NODE").unwrap_or_else(|| "node".into()))
        .arg("--no-warnings")
        .arg(&file)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Node.js could not be started ({e})"));
    let result = spawned.and_then(|mut child| {
        child.stdin.take().ok_or("no stdin")?.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err("the script failed".to_string());
        }
        Json::parse(&String::from_utf8_lossy(&output.stdout)).map_err(|e| e.to_string())
    });
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir(&dir);
    result
}

/// A new folder in the temporary one that only this user can use, so that what is written there is not another user's to
/// swap or to point somewhere with a link.
fn private_dir(prefix: &str) -> Result<PathBuf, String> {
    for attempt in 0..8u32 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{attempt}", std::process::id()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot make {}: {e}", dir.display())),
        }
    }
    Err("cannot make a private folder".to_string())
}
