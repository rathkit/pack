//! Linking: connecting each import to the declaration it really refers to.
//!
//! `import { x } from "./a"` may name something `a` declares, something `a` re-exports from
//! `b` under another name, or something that arrives through `export *`. [`link`] follows
//! those chains (the specification's `ResolveExport`) and records, for every import binding
//! of every module, the [`Binding`] it ends up at. Everything later (tree-shaking, renaming,
//! chunking) works from that table, not from the import syntax.
//!
//! A name found through several `export *` that do not agree is ambiguous and an error when
//! imported. A name that is nowhere is an error too. Names that come from CommonJS, JSON or
//! external modules cannot be checked ahead of time; they link to a property lookup at run
//! time instead.

use std::collections::{BTreeSet, HashMap};

use crate::ast::NodeId;
use crate::graph::{Diagnostic, Edge, Graph, ModuleId, ModuleKind, Severity};
use crate::module::{ExportSource, ImportName};
use crate::semantic::SymbolId;

/// What an imported name ends up referring to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// A declaration in an ES module.
    Symbol { module: ModuleId, symbol: SymbolId },
    /// The value of `export default <expression>`, which has no declaration of its own.
    DefaultValue { module: ModuleId, node: NodeId },
    /// `import * as ns` or `export * as ns`: the module's whole export object.
    Namespace(ModuleId),
    /// A name taken from a CommonJS or JSON module's exports. Whether it exists is only known
    /// when the code runs.
    CommonJs { module: ModuleId, name: ImportName },
    /// A name from a module left outside the bundle.
    External { specifier: String, name: ImportName },
}

impl Binding {
    /// Bindings the linker cannot check, as opposed to ones backed by a declaration.
    fn is_weak(&self) -> bool {
        matches!(self, Binding::CommonJs { .. } | Binding::External { .. })
    }
}

/// The result of linking a graph.
#[derive(Debug, Default)]
pub struct Linked {
    /// Indexed by module: each import binding's symbol and where it leads.
    pub imports: Vec<HashMap<SymbolId, Binding>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Linked {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// Where the import binding `symbol` of `module` leads. `None` for symbols that are not
    /// imports, and for imports that failed to link.
    pub fn binding(&self, module: ModuleId, symbol: SymbolId) -> Option<&Binding> {
        self.imports[module.index()].get(&symbol)
    }
}

enum Lookup {
    Found(Binding),
    NotFound,
    Ambiguous,
}

/// Resolves every import and re-export in the graph.
pub fn link(graph: &Graph) -> Linked {
    let linker = Linker { graph };
    let mut linked = Linked { imports: Vec::with_capacity(graph.modules.len()), diagnostics: Vec::new() };
    for module in &graph.modules {
        let mut bindings = HashMap::new();
        let record = &module.record;
        for entry in &record.imports {
            let origin = ImportSite { module: module.id, request: entry.request, name: &entry.name };
            let mut set = Vec::new();
            match linker.follow(&origin, &mut set) {
                Lookup::Found(binding) => {
                    bindings.insert(entry.local, binding);
                }
                Lookup::NotFound => linked.diagnostics.push(linker.missing(module.id, entry.request, &entry.name, graph.module(module.id).ast_pos(entry.node))),
                Lookup::Ambiguous => linked.diagnostics.push(linker.ambiguous(module.id, entry.request, &entry.name, graph.module(module.id).ast_pos(entry.node))),
            }
        }
        // A re-export must name something that exists, though it binds nothing here.
        for export in &record.exports {
            let ExportSource::Indirect { request, name } = &export.source else { continue };
            let origin = ImportSite { module: module.id, request: *request, name };
            let mut set = Vec::new();
            match linker.follow(&origin, &mut set) {
                Lookup::Found(_) => {}
                // In a TypeScript file `export { Draft } from './x'` may name a type, which is gone from the output (as `tsc` and esbuild treat it).
                Lookup::NotFound if module.parsed.as_ref().is_some_and(|p| p.is_ts) => {}
                Lookup::NotFound => linked.diagnostics.push(linker.missing(module.id, *request, name, graph.module(module.id).ast_pos(export.node))),
                Lookup::Ambiguous => linked.diagnostics.push(linker.ambiguous(module.id, *request, name, graph.module(module.id).ast_pos(export.node))),
            }
        }
        linked.imports.push(bindings);
    }
    linked.diagnostics.sort_by(|a, b| (&a.file, a.pos, &a.message).cmp(&(&b.file, b.pos, &b.message)));
    linked
}

/// What `name` means as an export of `module`, or `None` if it has none (or is ambiguous).
pub fn resolve_export(graph: &Graph, module: ModuleId, name: &str) -> Option<Binding> {
    match (Linker { graph }).resolve_export(module, name, &mut Vec::new()) {
        Lookup::Found(binding) => Some(binding),
        Lookup::NotFound | Lookup::Ambiguous => None,
    }
}

/// The names `import * as ns` would expose from `module`, sorted. Ambiguous `export *`
/// names are left out, as the specification does, and `default` is never inherited.
pub fn export_names(graph: &Graph, module: ModuleId) -> Vec<String> {
    let linker = Linker { graph };
    let mut names = BTreeSet::new();
    let mut seen = Vec::new();
    linker.collect_names(module, &mut names, &mut seen, true);
    names.into_iter().filter(|n| matches!(linker.resolve_export(module, n, &mut Vec::new()), Lookup::Found(_))).collect()
}

struct Linker<'a> {
    graph: &'a Graph,
}

/// One import or re-export being followed: `name` from the module that `request` leads to.
struct ImportSite<'a> {
    module: ModuleId,
    request: usize,
    name: &'a ImportName,
}

impl Linker<'_> {
    /// Finds what `site` refers to.
    fn follow(&self, site: &ImportSite, set: &mut Vec<(ModuleId, String)>) -> Lookup {
        let module = self.graph.module(site.module);
        match &module.edges[site.request] {
            // Already reported where the specifier failed to resolve.
            Edge::Unresolved => Lookup::NotFound,
            Edge::External(specifier) => Lookup::Found(Binding::External { specifier: specifier.clone(), name: site.name.clone() }),
            Edge::Module(target) => match site.name {
                ImportName::Namespace => Lookup::Found(Binding::Namespace(*target)),
                ImportName::Default => self.resolve_export(*target, "default", set),
                ImportName::Named(name) => self.resolve_export(*target, name, set),
            },
        }
    }

    /// The specification's `ResolveExport`: what `name` means as an export of `module`.
    fn resolve_export(&self, module_id: ModuleId, name: &str, set: &mut Vec<(ModuleId, String)>) -> Lookup {
        let module = self.graph.module(module_id);
        if module.kind != ModuleKind::Esm {
            let name = if name == "default" { ImportName::Default } else { ImportName::Named(name.to_string()) };
            return Lookup::Found(Binding::CommonJs { module: module_id, name });
        }
        // Coming back to a name already being resolved is a cycle with no answer.
        if set.iter().any(|(m, n)| *m == module_id && n == name) {
            return Lookup::NotFound;
        }
        set.push((module_id, name.to_string()));
        let result = self.resolve_in(module_id, name, set);
        set.pop();
        result
    }

    fn resolve_in(&self, module_id: ModuleId, name: &str, set: &mut Vec<(ModuleId, String)>) -> Lookup {
        let module = self.graph.module(module_id);
        if let Some(entry) = module.record.export(name) {
            return match &entry.source {
                ExportSource::DefaultValue(node) => Lookup::Found(Binding::DefaultValue { module: module_id, node: *node }),
                ExportSource::Indirect { request, name } => self.follow(&ImportSite { module: module_id, request: *request, name }, set),
                ExportSource::Local(symbol) => {
                    // `import { x } from "m"; export { x }` re-exports `m`'s `x`.
                    match module.record.imports.iter().find(|i| i.local == *symbol) {
                        Some(import) => self.follow(&ImportSite { module: module_id, request: import.request, name: &import.name }, set),
                        None => Lookup::Found(Binding::Symbol { module: module_id, symbol: *symbol }),
                    }
                }
            };
        }
        // `export *` never passes on a default export.
        if name == "default" {
            return Lookup::NotFound;
        }
        let mut strong: Vec<Binding> = Vec::new();
        let mut weak: Option<Binding> = None;
        for &request in &module.record.star_exports {
            let found = match &module.edges[request] {
                Edge::Unresolved => continue,
                Edge::External(specifier) => Lookup::Found(Binding::External { specifier: specifier.clone(), name: ImportName::Named(name.to_string()) }),
                Edge::Module(target) => self.resolve_export(*target, name, set),
            };
            match found {
                Lookup::Found(binding) if binding.is_weak() => {
                    weak.get_or_insert(binding);
                }
                Lookup::Found(binding) => {
                    if !strong.contains(&binding) {
                        strong.push(binding);
                    }
                }
                Lookup::Ambiguous => return Lookup::Ambiguous,
                Lookup::NotFound => {}
            }
        }
        match (strong.len(), weak) {
            (1, _) => Lookup::Found(strong.pop().unwrap()),
            (0, Some(weak)) => Lookup::Found(weak),
            (0, None) => Lookup::NotFound,
            _ => Lookup::Ambiguous,
        }
    }

    fn collect_names(&self, module_id: ModuleId, names: &mut BTreeSet<String>, seen: &mut Vec<ModuleId>, top: bool) {
        if seen.contains(&module_id) {
            return;
        }
        seen.push(module_id);
        let module = self.graph.module(module_id);
        if module.kind != ModuleKind::Esm {
            return;
        }
        for export in &module.record.exports {
            if top || export.exported != "default" {
                names.insert(export.exported.clone());
            }
        }
        for &request in &module.record.star_exports {
            if let Edge::Module(target) = module.edges[request] {
                self.collect_names(target, names, seen, false);
            }
        }
    }

    fn describe(&self, module: ModuleId, request: usize, name: &ImportName) -> (String, String) {
        let from = &self.graph.module(module).record.requests[request].specifier;
        let name = match name {
            ImportName::Default => "default".to_string(),
            ImportName::Named(n) => n.clone(),
            ImportName::Namespace => "*".to_string(),
        };
        (from.clone(), name)
    }

    fn missing(&self, module: ModuleId, request: usize, name: &ImportName, pos: u32) -> Diagnostic {
        let (from, name) = self.describe(module, request, name);
        self.diagnostic(module, pos, format!("No matching export in '{from}' for import '{name}'"))
    }

    fn ambiguous(&self, module: ModuleId, request: usize, name: &ImportName, pos: u32) -> Diagnostic {
        let (from, name) = self.describe(module, request, name);
        self.diagnostic(module, pos, format!("Import '{name}' from '{from}' is ambiguous: several `export *` provide different bindings for it"))
    }

    fn diagnostic(&self, module: ModuleId, pos: u32, message: String) -> Diagnostic {
        Diagnostic { severity: Severity::Error, message, file: Some(self.graph.module(module).path.clone()), pos: Some(pos) }
    }
}

#[cfg(test)]
mod tests;
