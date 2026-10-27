//! `--hmr`: the modules of a development build as functions in a table, so a changed one can be replaced in a running page.
//!
//! The usual output is one scope-hoisted program, in which a module is not a thing a page can swap. Here each module is a function
//! (`__r.def(id, kind, function (exports, module, __r, require) {...}, deps, specifiers)`), run on first use. What a module imports
//! it reads from the other module's exports object, which keeps its identity when that module runs again, so the importer sees the new
//! bindings. Nothing is renamed, shaken or minified. The runtime (`registry.js`) and what it does on an update are described there.

use super::*;
use crate::module::{ExportSource, ImportEntry};

const RUNTIME: &str = include_str!("registry.js");

/// A module as a hot update sends it.
pub struct HmrModule {
    pub id: String,
    pub kind: &'static str,
    /// The function expression.
    pub code: String,
    /// The modules it imports (statically), for the runtime's graph of who imports whom.
    pub deps: Vec<String>,
    /// Each specifier it uses and the module it names, for `import.meta.hot.accept("./dep")` and `require`.
    pub specs: Vec<(String, String)>,
    /// It registers components with React Fast Refresh.
    pub refresh: bool,
}

fn member(name: &ImportName) -> String {
    match name {
        ImportName::Default => ".default".to_string(),
        ImportName::Named(n) if is_identifier_name(n) => format!(".{n}"),
        ImportName::Named(n) => format!("[{}]", json_string(n)),
        ImportName::Namespace => String::new(),
    }
}

impl Scanner<'_, '_> {
    /// The module as an ES module in the table: its statements as the usual output has them (declarations, classes with decorators, enums
    /// and namespaces), with the `export` keywords gone and the names as written. The imports and exports are a header, made at the end.
    pub(super) fn scan_registry(&mut self, id: ModuleId) -> Code {
        match self.scan_esm(id) {
            Code::Esm(mut code) => {
                let refresh = if self.bundler.react_refresh { self.react_refresh(&mut code) } else { None };
                Code::Registry(code, refresh)
            }
            other => other,
        }
    }

    /// How an import is spelled where it is used: a read of the exports object of the module it comes from.
    pub(super) fn registry_binding(&self, import: &ImportEntry) -> String {
        format!("__rki{}{}", import.request, member(&import.name))
    }

    /// A reference to an imported name in a module of the table. Called, it must not call with the exports object as `this`.
    pub(super) fn registry_reference(&mut self, node: NodeId, sym: SymbolId) {
        let Some(import) = self.module.record.imports.iter().find(|i| i.local == sym) else { return };
        let text = self.registry_binding(import);
        let text = if self.call_callees.contains(&node) && !matches!(import.name, ImportName::Namespace) { format!("(0, {text})") } else { text };
        self.push_ident_edit(node, vec![Piece::Text(text)]);
    }
}

impl Bundler<'_> {
    /// The lines that stand for a module's imports and exports: getters for what it exports (defined before anything runs, so a cycle sees
    /// them), then each import. An import of a module that waits (top-level await) is waited for.
    fn registry_header(&mut self, module: &Module, asyncs: &HashSet<ModuleId>) -> String {
        let graph = self.graph;
        let mut header = String::new();
        let mut getters = Vec::new();
        for export in &module.record.exports {
            let value = match &export.source {
                ExportSource::Local(sym) => match module.record.imports.iter().find(|i| i.local == *sym) {
                    Some(import) => format!("__rki{}{}", import.request, member(&import.name)),
                    None => match module.parsed.as_ref() {
                        Some(parsed) => parsed.ast.atom(parsed.semantic.symbol(*sym).name).to_string(),
                        None => continue,
                    },
                },
                ExportSource::DefaultValue(_) => "__rkd".to_string(),
                ExportSource::Indirect { request, name } => format!("__rki{request}{}", member(name)),
            };
            let label = if is_identifier_name(&export.exported) { export.exported.clone() } else { json_string(&export.exported) };
            getters.push(format!("{label}: () => {value}"));
        }
        if !getters.is_empty() {
            let _ = write!(header, "__r.e(exports, {{ {} }});\n", getters.join(", "));
        }
        for (index, request) in module.record.requests.iter().enumerate() {
            if request.kind != RequestKind::Static {
                continue;
            }
            match &module.edges[index] {
                Edge::Module(target) => {
                    let target_module = graph.module(*target);
                    let path = json_string(&target_module.path.to_string_lossy());
                    if asyncs.contains(target) {
                        let _ = write!(header, "var __rki{index} = await __r.w({path});\n");
                    } else {
                        let call = if target_module.kind == ModuleKind::Esm { "i" } else { "c" };
                        let _ = write!(header, "var __rki{index} = __r.{call}({path});\n");
                    }
                }
                Edge::External(spec) => {
                    let n = self.registry_external(spec);
                    let _ = write!(header, "var __rki{index} = __rkx{n};\n");
                }
                Edge::Unresolved => {}
            }
        }
        for &index in &module.record.star_exports {
            if !matches!(module.edges[index], Edge::Unresolved) {
                let _ = write!(header, "__r.s(exports, __rki{index});\n");
            }
        }
        header
    }

    /// The modules that wait for something: those with top-level await, and the ones that import them.
    fn registry_async_modules(&self) -> HashSet<ModuleId> {
        let graph = self.graph;
        let mut waits: HashSet<ModuleId> = graph.modules.iter().filter(|m| self.included(m.id) && matches!(self.code.get(&m.id), Some(Code::Registry(code, _)) if code.has_tla)).map(|m| m.id).collect();
        loop {
            let mut grew = false;
            for module in &graph.modules {
                if waits.contains(&module.id) || !self.included(module.id) || !matches!(self.code.get(&module.id), Some(Code::Registry(..))) {
                    continue;
                }
                let imports_waiter = module.record.requests.iter().zip(&module.edges).any(|(r, e)| r.kind == RequestKind::Static && matches!(e, Edge::Module(t) if waits.contains(t)));
                if imports_waiter {
                    waits.insert(module.id);
                    grew = true;
                }
            }
            if !grew {
                return waits;
            }
        }
    }

    /// The number a module of the table refers to an external module by: `__rkx0`, `__rkx1`, ...
    pub(super) fn registry_external(&mut self, spec: &str) -> usize {
        match self.registry_externals.iter().position(|s| s == spec) {
            Some(n) => n,
            None => {
                self.registry_externals.push(spec.to_string());
                self.registry_externals.len() - 1
            }
        }
    }

    /// The build as a table of modules. Returns the text of the one script, and the modules as a hot update sends them.
    pub(super) fn emit_registry(&mut self, names: &HashMap<Key, String>) -> (String, Vec<HmrModule>) {
        let graph = self.graph;
        let asyncs = self.registry_async_modules();
        let mut modules = Vec::new();
        let mut definitions = String::new();
        for module in &graph.modules {
            if !self.included(module.id) {
                continue;
            }
            let path = module.path.to_string_lossy().into_owned();
            let mut is_refreshable = false;
            let header = if matches!(self.code.get(&module.id), Some(Code::Registry(..))) { self.registry_header(module, &asyncs) } else { String::new() };
            let Some(code) = self.code.get(&module.id) else { continue };
            // The temporaries of the module's rewrites for the target, declared where they are used.
            let temps: Vec<&str> = self.temps.iter().filter(|(m, _)| *m == module.id).map(|(_, n)| n.as_str()).collect();
            let declare = if temps.is_empty() { String::new() } else { format!("var {};\n", temps.join(", ")) };
            let (kind, text) = match code {
                Code::Registry(code, refresh) => {
                    let body: String = code.stmts.iter().map(|stmt| self.render_stmt(module.id, stmt, names) + "\n").collect();
                    let waits = asyncs.contains(&module.id);
                    let (before, after) = refresh.as_ref().map_or((String::new(), String::new()), |r| (r.prefix.clone(), format!("{}\n", r.suffix)));
                    is_refreshable = refresh.is_some();
                    ("esm", format!("{}function (exports, module, __r) {{\n\"use strict\";\n{declare}{before}{header}{body}{after}\n}}", if waits { "async " } else { "" }))
                }
                Code::Wrapped { edits, json, .. } => {
                    let body = if *json {
                        let trimmed = module.source.trim();
                        let from = (trimmed.as_ptr() as usize - module.source.as_ptr() as usize) as u32;
                        let mut body = String::from("module.exports = ");
                        self.copy(&mut body, module.id, from, from + trimmed.len() as u32);
                        body.push(';');
                        body
                    } else {
                        let start = if module.source.starts_with("#!") { module.source.find('\n').unwrap_or(module.source.len()) as u32 } else { 0 };
                        self.render(module.id, start, module.source.len() as u32, edits, names)
                    };
                    ("cjs", format!("function (exports, module, __r, require) {{\n{declare}{body}\n}}"))
                }
                Code::Esm(_) => continue,
            };
            let mut deps = Vec::new();
            let mut specs = Vec::new();
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let Edge::Module(target) = edge {
                    let target_path = graph.module(*target).path.to_string_lossy().into_owned();
                    if matches!(request.kind, RequestKind::Static | RequestKind::Require) && !deps.contains(&target_path) {
                        deps.push(target_path.clone());
                    }
                    if !specs.iter().any(|(s, _): &(String, String)| *s == request.specifier) {
                        specs.push((request.specifier.clone(), target_path));
                    }
                }
            }
            let list = deps.iter().map(|d| json_string(d)).collect::<Vec<_>>().join(", ");
            let table = specs.iter().map(|(s, t)| format!("{}: {}", json_string(s), json_string(t))).collect::<Vec<_>>().join(", ");
            let _ = write!(definitions, "__r.def({}, {}, {text}, [{list}], {{ {table} }}{});\n", json_string(&path), json_string(kind), if is_refreshable { ", true" } else { "" });
            modules.push(HmrModule { id: path, kind, code: strip_tags(&text).into_owned(), deps, specs, refresh: is_refreshable });
        }
        let mut out = String::new();
        // External modules are ES imports of the script itself; the table reads them as namespaces.
        for (n, spec) in self.registry_externals.iter().enumerate() {
            let _ = write!(out, "import * as __rkx{n} from {};\n", json_string(spec));
        }
        out.push_str(RUNTIME);
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
        out.push_str(&definitions);
        let entries = graph.entries.iter().map(|&e| json_string(&graph.module(e).path.to_string_lossy())).collect::<Vec<_>>().join(", ");
        let _ = write!(out, "__r.run([{entries}]);\n");
        (out, modules)
    }

    /// Names for the keys the scanner registered: a module's own names stay as they are.
    pub(super) fn registry_names(&self) -> HashMap<Key, String> {
        let mut names = HashMap::new();
        for (n, key) in self.keys.iter().enumerate() {
            let name = match key {
                Key::Symbol(id, sym) => match self.graph.module(*id).parsed.as_ref() {
                    Some(parsed) => parsed.ast.atom(parsed.semantic.symbol(*sym).name).to_string(),
                    None => format!("__rk{n}"),
                },
                Key::Default(_) => "__rkd".to_string(),
                _ => format!("__rkn{n}"),
            };
            names.insert(key.clone(), name);
        }
        names
    }

    pub(super) fn run_registry(&mut self, options: &Options) -> Result<Output, Vec<Diagnostic>> {
        if !self.errors.is_empty() {
            return Err(std::mem::take(&mut self.errors));
        }
        let fail = |message: &str| Err(vec![Diagnostic { severity: crate::graph::Severity::Error, message: message.to_string(), file: None, pos: None }]);
        if self.format != Format::Esm {
            return fail("--hmr needs the esm output format");
        }
        if self.federation.is_some() {
            return fail("--hmr does not work with module federation");
        }
        self.plan = self.single_plan();
        let names = self.registry_names();
        let (raw, modules) = self.emit_registry(&names);
        let marks = std::mem::take(&mut *self.marks.lock().expect("marks"));
        let settings = crate::minify::Options { keep_top_level: false, keep_names: Some(true), ..options.minify };
        let file = self.finish_chunk(0, raw, crate::minify::Layout::default(), options, &settings, &marks)?;
        let mut files = vec![file];
        files.extend(self.style_and_asset_files(&settings, options)?);
        let code = files.first().map(|f| f.code.clone()).unwrap_or_default();
        // Every entry is in the one script.
        let chunks = self
            .graph
            .entries
            .iter()
            .map(|&e| ChunkInfo { file: files[0].name.clone(), name: String::new(), is_entry: true, facade_module: Some(self.graph.module(e).path.to_string_lossy().into_owned()), ..ChunkInfo::default() })
            .collect();
        Ok(Output { code, files, chunks, hmr: Some(modules) })
    }
}
