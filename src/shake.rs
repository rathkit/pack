//! Tree-shaking: working out which modules and which top-level statements the bundle needs.
//!
//! Each ES module's top level is split into *parts*, one per statement. A part records what
//! it declares, which module-level names it uses, and whether running it can have effects
//! ([`crate::purity`]). Liveness spreads from the roots:
//!
//! - everything the entry points export;
//! - every part with effects, in every module that ends up in the bundle;
//! - the targets of `import()` and `require()`, which are kept whole.
//!
//! A live part makes the declarations it uses live, following imports through the linker's
//! bindings to the module that really declares them. A module joins the bundle when one of
//! its parts is live, or when a module in the bundle imports it and its package does not
//! promise `"sideEffects": false`. CommonJS and JSON modules are kept whole.
//!
//! Granularity is the statement: `const a = 1, b = f();` is one part. Splitting declarators
//! is a later refinement.

use std::collections::HashMap;

use crate::ast::NodeId;
use crate::graph::{Edge, Graph, ModuleId, ModuleKind};
use crate::link::{Binding, Linked, export_names, resolve_export};
use crate::module::RequestKind;
use crate::purity::Purity;
use crate::semantic::SymbolId;

/// A top-level statement of an ES module.
#[derive(Debug)]
pub struct Part {
    pub stmt: NodeId,
    /// Module-level symbols the statement declares.
    pub declares: Vec<SymbolId>,
    pub uses: Vec<Use>,
    /// Running the statement can do something besides define values.
    pub effects: bool,
    /// For `export default <expression>`: the expression, which is what a
    /// [`Binding::DefaultValue`] names.
    pub default_value: Option<NodeId>,
}

/// A module-level name a statement refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Use {
    Symbol(SymbolId),
    /// `ns.name`, where `ns` may be a namespace import: only that one export is needed.
    Member(SymbolId, String),
}

#[derive(Debug, Default)]
pub struct ModuleShake {
    /// The module is part of the bundle.
    pub included: bool,
    pub parts: Vec<Part>,
    /// Parallel to `parts`: the statement stays.
    pub live: Vec<bool>,
    /// Module-level symbols that something that stays asks for. A declaration kept for what its
    /// initializer does, whose name is not here, does not need the name.
    pub demanded: std::collections::HashSet<SymbolId>,
    /// `demanded` is complete (tree shaking ran); without it every name must be assumed to be used.
    pub demand_known: bool,
}

#[derive(Debug)]
pub struct Shaken {
    pub modules: Vec<ModuleShake>,
}

impl Shaken {
    pub fn included_modules(&self) -> usize {
        self.modules.iter().filter(|m| m.included).count()
    }

    pub fn part_counts(&self) -> (usize, usize) {
        let live = self.modules.iter().map(|m| m.live.iter().filter(|&&l| l).count()).sum();
        let total = self.modules.iter().map(|m| m.parts.len()).sum();
        (live, total)
    }
}

/// Keeps everything: every module and every statement. For comparing a bundle with and
/// without shaking.
pub fn keep_everything(graph: &Graph) -> Shaken {
    let mut modules: Vec<ModuleShake> = graph.modules.iter().map(|m| build_parts(graph, m.id)).collect();
    for module in &mut modules {
        module.included = true;
        module.live = vec![true; module.parts.len()];
    }
    Shaken { modules }
}

/// Decides what to keep, starting from the graph's entry points.
pub fn shake(graph: &Graph, linked: &Linked) -> Shaken {
    let mut modules: Vec<ModuleShake> = graph.modules.iter().map(|m| build_parts(graph, m.id)).collect();
    let mut by_symbol: Vec<HashMap<SymbolId, Vec<usize>>> = Vec::with_capacity(modules.len());
    for module in &modules {
        let mut map: HashMap<SymbolId, Vec<usize>> = HashMap::new();
        for (index, part) in module.parts.iter().enumerate() {
            for &symbol in &part.declares {
                map.entry(symbol).or_default().push(index);
            }
        }
        by_symbol.push(map);
    }
    let mut shaker = Shaker { graph, linked, modules: &mut modules, by_symbol, work: Vec::new(), exports_done: vec![false; graph.modules.len()], full: vec![false; graph.modules.len()] };
    for &entry in &graph.entries {
        shaker.work.push(Work::Module(entry));
        shaker.work.push(Work::AllExports(entry));
    }
    shaker.run();
    for module in &mut modules {
        module.demand_known = true;
    }
    Shaken { modules }
}

enum Work {
    /// The module is in the bundle; its effectful parts and side-effectful imports come with it.
    Module(ModuleId),
    /// Everything the module has: all parts and all exports (`import()`, `require()`).
    Full(ModuleId),
    Part(ModuleId, usize),
    Symbol(ModuleId, SymbolId),
    Binding(Binding),
    /// Every export of the module is needed (an entry, or a namespace used as a value).
    AllExports(ModuleId),
}

struct Shaker<'a> {
    graph: &'a Graph,
    linked: &'a Linked,
    modules: &'a mut Vec<ModuleShake>,
    by_symbol: Vec<HashMap<SymbolId, Vec<usize>>>,
    work: Vec<Work>,
    exports_done: Vec<bool>,
    full: Vec<bool>,
}

impl Shaker<'_> {
    fn run(&mut self) {
        while let Some(item) = self.work.pop() {
            match item {
                Work::Module(m) => self.include(m),
                Work::Full(m) => self.make_full(m),
                Work::Part(m, i) => self.make_live(m, i),
                Work::Symbol(m, symbol) => self.use_symbol(m, symbol),
                Work::Binding(binding) => self.use_binding(binding),
                Work::AllExports(m) => self.all_exports(m),
            }
        }
    }

    fn include(&mut self, id: ModuleId) {
        if std::mem::replace(&mut self.modules[id.index()].included, true) {
            return;
        }
        let module = self.graph.module(id);
        if module.kind != ModuleKind::Esm {
            // CommonJS and JSON have no parts to choose from, and whatever they require is
            // needed whole because nothing says which of it they use.
            for edge in &module.edges {
                if let Edge::Module(target) = edge {
                    self.work.push(Work::Full(*target));
                }
            }
            return;
        }
        for index in 0..self.modules[id.index()].parts.len() {
            if self.modules[id.index()].parts[index].effects {
                self.work.push(Work::Part(id, index));
            }
        }
        // The classic JSX transform calls `React.createElement`, which the source never names.
        if let Some(parsed) = module.parsed.as_ref() {
            if parsed.ast.uses_jsx && !self.graph.jsx.automatic {
                for path in [&self.graph.jsx.factory, &self.graph.jsx.fragment] {
                    let root = path.split('.').next().unwrap_or("");
                    if let Some(atom) = parsed.ast.find_atom(root) {
                        if let Some(symbol) = parsed.semantic.lookup_local(parsed.semantic.root_scope(), atom) {
                            self.work.push(Work::Symbol(id, symbol));
                        }
                    }
                }
            }
        }
        // JSX turns into calls of the runtime's functions, which the source never names.
        if let Some(request) = module.record.jsx_runtime {
            if let Edge::Module(target) = module.edges[request] {
                for name in ["jsx", "jsxs", "Fragment"] {
                    if let Some(binding) = resolve_export(self.graph, target, name) {
                        self.work.push(Work::Binding(binding));
                    }
                }
            }
        }
        for (request, edge) in module.record.requests.iter().zip(&module.edges) {
            let Edge::Module(target) = edge else { continue };
            match request.kind {
                // Imported only for what it does: kept unless the package says it does nothing.
                RequestKind::Static => {
                    if self.graph.module(*target).side_effects {
                        self.work.push(Work::Module(*target));
                    }
                }
                RequestKind::Dynamic | RequestKind::Require => self.work.push(Work::Full(*target)),
                // Only a path was wanted, and resolved edges of this kind are never modules.
                RequestKind::Resolve => {}
                // A copied file: its module is kept so the file is written.
                RequestKind::AssetUrl => self.work.push(Work::Module(*target)),
            }
        }
    }

    fn make_full(&mut self, id: ModuleId) {
        if std::mem::replace(&mut self.full[id.index()], true) {
            return;
        }
        self.work.push(Work::Module(id));
        for index in 0..self.modules[id.index()].parts.len() {
            self.work.push(Work::Part(id, index));
        }
        self.work.push(Work::AllExports(id));
    }

    fn make_live(&mut self, id: ModuleId, index: usize) {
        if std::mem::replace(&mut self.modules[id.index()].live[index], true) {
            return;
        }
        self.include(id);
        // `take` avoids holding a borrow of the part while queueing work.
        let uses = std::mem::take(&mut self.modules[id.index()].parts[index].uses);
        for used in &uses {
            match used {
                Use::Symbol(symbol) => self.work.push(Work::Symbol(id, *symbol)),
                Use::Member(symbol, name) => match self.linked.binding(id, *symbol) {
                    Some(Binding::Namespace(target)) => {
                        self.work.push(Work::Module(*target));
                        if let Some(binding) = resolve_export(self.graph, *target, name) {
                            self.work.push(Work::Binding(binding));
                        }
                    }
                    _ => self.work.push(Work::Symbol(id, *symbol)),
                },
            }
        }
        self.modules[id.index()].parts[index].uses = uses;
    }

    fn use_symbol(&mut self, id: ModuleId, symbol: SymbolId) {
        self.include(id);
        self.modules[id.index()].demanded.insert(symbol);
        if let Some(binding) = self.linked.binding(id, symbol) {
            self.work.push(Work::Binding(binding.clone()));
            return;
        }
        if let Some(parts) = self.by_symbol[id.index()].get(&symbol) {
            for &index in parts {
                self.work.push(Work::Part(id, index));
            }
        }
    }

    fn use_binding(&mut self, binding: Binding) {
        match binding {
            Binding::Symbol { module, symbol } => self.work.push(Work::Symbol(module, symbol)),
            Binding::DefaultValue { module, node } => {
                self.include(module);
                if let Some(index) = self.modules[module.index()].parts.iter().position(|p| p.default_value == Some(node)) {
                    self.work.push(Work::Part(module, index));
                }
            }
            Binding::Namespace(module) => {
                self.work.push(Work::Module(module));
                self.work.push(Work::AllExports(module));
            }
            Binding::CommonJs { module, .. } => self.work.push(Work::Full(module)),
            Binding::External { .. } => {}
        }
    }

    fn all_exports(&mut self, id: ModuleId) {
        if std::mem::replace(&mut self.exports_done[id.index()], true) {
            return;
        }
        self.include(id);
        if self.graph.module(id).kind != ModuleKind::Esm {
            return;
        }
        for name in export_names(self.graph, id) {
            if let Some(binding) = resolve_export(self.graph, id, &name) {
                self.work.push(Work::Binding(binding));
            }
        }
    }
}

/// Splits an ES module's top level into parts. Other kinds of module have none.
fn build_parts(graph: &Graph, id: ModuleId) -> ModuleShake {
    let module = graph.module(id);
    let (ModuleKind::Esm, Some(parsed)) = (module.kind, module.parsed.as_ref()) else { return ModuleShake::default() };
    use crate::ast::NodeKind;
    let (ast, semantic) = (&parsed.ast, &parsed.semantic);
    let defines = Some(&graph.defines).filter(|d| !d.is_empty());
    let purity = Purity { source: &module.source, ast, semantic, defines };
    let root = semantic.root_scope();
    let NodeKind::Program(body) = ast[ast.root].kind else { return ModuleShake::default() };
    let mut parts = Vec::new();
    let mut stack = Vec::new();
    for &stmt in ast.list(body) {
        match ast[stmt].kind {
            NodeKind::Import(_) | NodeKind::ExportAll { .. } => continue,
            NodeKind::ExportNamed(export) if ast.export(export).decl.is_none() => continue,
            _ => {}
        }
        let mut declares = Vec::new();
        let mut default_value = None;
        let declaration = match ast[stmt].kind {
            NodeKind::ExportNamed(export) => ast.export(export).decl.unwrap_or(stmt),
            NodeKind::ExportDefault(value) => {
                default_value = Some(value);
                value
            }
            _ => stmt,
        };
        match ast[declaration].kind {
            NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } if ast[declaration].has(crate::ast::flags::DECL) => {
                declares.extend(semantic.symbol_of(n));
            }
            NodeKind::TsEnum { name, .. } | NodeKind::TsNamespace { name, .. } => declares.extend(semantic.symbol_of(name)),
            NodeKind::VarDecl { decls, .. } => {
                let mut idents = Vec::new();
                for &d in ast.list(decls) {
                    if let NodeKind::VarDeclarator { target, .. } = ast[d].kind {
                        crate::semantic::binding_idents(ast, target, &mut idents);
                    }
                }
                declares.extend(idents.into_iter().filter_map(|i| semantic.symbol_of(i)));
            }
            _ => {}
        }
        // Collect every module-level name the statement refers to, however deeply nested.
        let mut uses = Vec::new();
        stack.push(stmt);
        while let Some(node) = stack.pop() {
            match ast[node].kind {
                NodeKind::Ident(_) => {
                    if let Some(symbol) = semantic.symbol_of(node) {
                        if semantic.symbol(symbol).scope == root {
                            uses.push(Use::Symbol(symbol));
                        }
                    }
                    continue;
                }
                NodeKind::Member { object, prop } if !ast[node].has(crate::ast::flags::COMPUTED) => {
                    if let (NodeKind::Ident(_), NodeKind::PropName(name)) = (ast[object].kind, ast[prop].kind) {
                        if let Some(symbol) = semantic.symbol_of(object) {
                            if semantic.symbol(symbol).scope == root {
                                uses.push(Use::Member(symbol, ast.atom(name).to_string()));
                                continue;
                            }
                        }
                    }
                }
                _ => {}
            }
            // Names only a branch that never runs mentions are not used.
            let dead = defines.and_then(|d| d.dead_branch(ast, semantic, node)).map(|d| d.node);
            ast.for_each_child(node, |child| {
                if Some(child) != dead {
                    stack.push(child)
                }
            });
        }
        uses.sort_by_key(|u| match u {
            Use::Symbol(s) => (s.index(), String::new()),
            Use::Member(s, n) => (s.index(), n.clone()),
        });
        uses.dedup();
        parts.push(Part { stmt, declares, uses, effects: !purity.statement_is_pure(stmt), default_value });
    }
    let live = vec![false; parts.len()];
    ModuleShake { included: false, parts, live, demanded: Default::default(), demand_known: false }
}

#[cfg(test)]
mod tests;
