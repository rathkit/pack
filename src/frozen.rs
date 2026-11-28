//! Objects that nothing writes: `const config = { debug: false, onError: null }` that every module only reads as `config.debug`.
//!
//! A library keeps its settings in one object that other modules import and read. When no code in the bundle writes to the object, passes it
//! on, spreads it or exports it out of the bundle, a read of a property whose value is `false`, `null` or `undefined` is that value. The
//! reads become the values, the branches they decide are known (`if (config.debug) { ... }`), and the shaker does not count what only those
//! branches use. Rollup does this, and it is where much of its lead on RxJS comes from (`config.useDeprecatedSynchronousErrorHandling`).
//!
//! A declaration is a candidate when it is `var`, `let` or `const` at the top of an ES module, names one plain binding that is written only by
//! its initializer, and the initializer is an object literal of plain properties (no spread, accessor, computed key or `__proto__`, no key
//! twice). It stays one when every reference to the name, here and through the imports that lead to it, is `name.prop` or `name["prop"]` read
//! as a value: not assigned, updated or deleted, not called as `name.prop()` where the value would be called, and the name is not exported
//! out of the bundle, read by `eval`, or reached by `with`.
//!
//! The result is a table by module and symbol (the declaration's, and each import that leads to it) to the properties with a value that can be
//! written in place of the read.

use crate::ast::{Ast, NodeId, NodeKind, PropKind, flags};
use crate::define::{Const, Defines};
use crate::graph::{Edge, Graph, ModuleId, ModuleKind};
use crate::link::{Binding, Linked, export_names, resolve_export};
use crate::module::{ExportSource, ImportName, RequestKind};
use crate::semantic::{Semantic, SymbolId, SymbolKind, scope_flags, symbol_flags};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The properties of one object with a value that is known, by property name.
pub type Props = HashMap<String, Const>;
/// By symbol of a module.
pub type ModuleTable = HashMap<SymbolId, Props>;

/// The frozen objects of a bundle, by module (the tables are shared with the per-module definitions that read them).
#[derive(Debug, Default)]
pub struct Frozen {
    pub modules: Vec<Arc<ModuleTable>>,
}

impl Frozen {
    /// Whether any module has a frozen object.
    pub fn is_empty(&self) -> bool {
        self.modules.iter().all(|m| m.is_empty())
    }

    /// The table of a module, when it has one.
    pub fn of(&self, module: ModuleId) -> Option<&Arc<ModuleTable>> {
        self.modules.get(module.index()).filter(|m| !m.is_empty())
    }
}

/// Values that can be written where `name.prop` was read without a change of meaning in front of `.`, `(` or `[`.
fn replaceable(value: &Const) -> bool {
    matches!(value, Const::Undefined | Const::Null | Const::Bool(_))
}

/// The property a member expression reads: `a.b` or `a["b"]`.
pub fn member_name<'a>(ast: &'a Ast, member: NodeId) -> Option<&'a str> {
    let NodeKind::Member { prop, .. } = ast[member].kind else { return None };
    if ast[member].has(flags::OPTIONAL) {
        return None;
    }
    match ast[prop].kind {
        NodeKind::PropName(a) if !ast[member].has(flags::COMPUTED) => Some(ast.atom(a)),
        NodeKind::Str(a) if ast[member].has(flags::COMPUTED) => Some(ast.atom(a)),
        _ => None,
    }
}

/// The property a member expression reads, `a.b`, `a["b"]` or `a?.b`.
pub fn member_name_any<'a>(ast: &'a Ast, member: NodeId) -> Option<&'a str> {
    let NodeKind::Member { prop, .. } = ast[member].kind else { return None };
    match ast[prop].kind {
        NodeKind::PropName(a) if !ast[member].has(flags::COMPUTED) => Some(ast.atom(a)),
        NodeKind::Str(a) if ast[member].has(flags::COMPUTED) => Some(ast.atom(a)),
        _ => None,
    }
}

struct Candidate {
    props: Props,
    bad: bool,
}

/// The properties of the object literal `init` when it is plain, with the ones whose value is known.
fn plain_object(ast: &Ast, sem: &Semantic, init: NodeId) -> Option<Props> {
    let NodeKind::Object(list) = ast[init].kind else { return None };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut props = Props::new();
    for &p in ast.list(list) {
        let NodeKind::Property { key, value, kind: PropKind::Init | PropKind::Method } = ast[p].kind else { return None };
        if ast[p].has(flags::COMPUTED) {
            return None;
        }
        let name = match ast[key].kind {
            NodeKind::PropName(a) | NodeKind::Str(a) => ast.atom(a),
            _ => return None,
        };
        if name == "__proto__" || !seen.insert(name) {
            return None;
        }
        if let Some(c) = Defines::default().eval(ast, sem, value).filter(replaceable) {
            if !ast[p].has(flags::SHORTHAND) {
                props.insert(name.to_string(), c);
            }
        }
    }
    Some(props)
}

/// The frozen objects of the bundle. Empty when there are none.
pub fn analyze(graph: &Graph, linked: &Linked) -> Frozen {
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut by_symbol: HashMap<(ModuleId, SymbolId), usize> = HashMap::new();
    for module in &graph.modules {
        let (ModuleKind::Esm, Some(parsed)) = (module.kind, module.parsed.as_ref()) else { continue };
        let (ast, sem) = (&parsed.ast, &parsed.semantic);
        let NodeKind::Program(body) = ast[ast.root].kind else { continue };
        for &stmt in ast.list(body) {
            let declaration = match ast[stmt].kind {
                NodeKind::ExportNamed(export) => ast.export(export).decl.unwrap_or(stmt),
                _ => stmt,
            };
            let NodeKind::VarDecl { decls, .. } = ast[declaration].kind else { continue };
            for &d in ast.list(decls) {
                let NodeKind::VarDeclarator { target, init: Some(init) } = ast[d].kind else { continue };
                if !matches!(ast[target].kind, NodeKind::BindingIdent(_)) {
                    continue;
                }
                let Some(symbol) = sem.symbol_of(target) else { continue };
                let s = sem.symbol(symbol);
                if !matches!(s.kind, SymbolKind::Var | SymbolKind::Let | SymbolKind::Const) || s.writes != 1 || s.flags & symbol_flags::DYNAMIC != 0 || sem.scope(s.scope).flags & scope_flags::HAS_EVAL != 0 {
                    continue;
                }
                let Some(props) = plain_object(ast, sem, init) else { continue };
                if props.is_empty() {
                    continue;
                }
                by_symbol.insert((module.id, symbol), candidates.len());
                candidates.push(Candidate { props, bad: false });
            }
        }
    }
    if candidates.is_empty() {
        return Frozen::default();
    }
    // The names that lead to a candidate: its own, and the imports in other modules that resolve to it.
    let mut local: Vec<HashMap<SymbolId, usize>> = vec![HashMap::new(); graph.modules.len()];
    for (&(module, symbol), &c) in &by_symbol {
        local[module.index()].insert(symbol, c);
    }
    for (m, imports) in linked.imports.iter().enumerate() {
        for (&symbol, binding) in imports {
            if let Binding::Symbol { module, symbol: target } = binding {
                if let Some(&c) = by_symbol.get(&(*module, *target)) {
                    local[m].insert(symbol, c);
                }
            }
        }
    }
    // What leaves the bundle: everything an entry, a module loaded another way, or a module taken whole as a namespace exports.
    let mut exposed = vec![false; graph.modules.len()];
    for &entry in &graph.entries {
        exposed[entry.index()] = true;
    }
    for module in &graph.modules {
        for (request, edge) in module.record.requests.iter().zip(&module.edges) {
            if let Edge::Module(target) = edge {
                if request.kind != RequestKind::Static {
                    exposed[target.index()] = true;
                }
            }
        }
        for import in &module.record.imports {
            if import.name == ImportName::Namespace {
                if let Some(Edge::Module(target)) = module.edges.get(import.request) {
                    exposed[target.index()] = true;
                }
            }
        }
        for export in &module.record.exports {
            if let ExportSource::Indirect { request, name: ImportName::Namespace } = &export.source {
                if let Some(Edge::Module(target)) = module.edges.get(*request) {
                    exposed[target.index()] = true;
                }
            }
        }
    }
    for module in &graph.modules {
        if !exposed[module.id.index()] {
            continue;
        }
        for name in export_names(graph, module.id) {
            if let Some(Binding::Symbol { module: at, symbol }) = resolve_export(graph, module.id, &name) {
                if let Some(&c) = by_symbol.get(&(at, symbol)) {
                    candidates[c].bad = true;
                }
            }
        }
        for export in &module.record.exports {
            if let ExportSource::Local(symbol) = export.source {
                if let Some(&c) = by_symbol.get(&(module.id, symbol)) {
                    candidates[c].bad = true;
                }
            }
        }
    }
    // Every reference must be a read of a property.
    for module in &graph.modules {
        let names = &local[module.id.index()];
        if names.is_empty() {
            continue;
        }
        let Some(parsed) = module.parsed.as_ref() else { continue };
        let (ast, sem) = (&parsed.ast, &parsed.semantic);
        let mut stack: Vec<(NodeId, Option<NodeId>, Option<NodeId>)> = vec![(ast.root, None, None)];
        while let Some((n, parent, grand)) = stack.pop() {
            if matches!(ast[n].kind, NodeKind::Ident(_)) {
                if let Some(&c) = sem.symbol_of(n).and_then(|s| names.get(&s)) {
                    if !is_plain_read(ast, n, parent, grand) {
                        candidates[c].bad = true;
                    }
                }
            }
            ast.for_each_child(n, |child| stack.push((child, Some(n), parent)));
        }
    }
    let mut tables: Vec<ModuleTable> = vec![ModuleTable::new(); graph.modules.len()];
    for (m, names) in local.iter().enumerate() {
        for (&symbol, &c) in names {
            if !candidates[c].bad {
                tables[m].insert(symbol, candidates[c].props.clone());
            }
        }
    }
    Frozen { modules: tables.into_iter().map(Arc::new).collect() }
}

/// Whether the identifier `n` is read as `n.prop` where the property is only read: not assigned, updated, deleted or called.
fn is_plain_read(ast: &Ast, n: NodeId, parent: Option<NodeId>, grand: Option<NodeId>) -> bool {
    let Some(p) = parent else { return false };
    let NodeKind::Member { object, .. } = ast[p].kind else { return false };
    if object != n || member_name(ast, p).is_none() {
        return false;
    }
    let Some(g) = grand else { return true };
    match ast[g].kind {
        NodeKind::Assign { target, .. } => target != p,
        NodeKind::Update { arg, .. } => arg != p,
        NodeKind::Unary { op: crate::ast::UnaryOp::Delete, arg } => arg != p,
        NodeKind::Call { callee, .. } | NodeKind::New { callee, .. } => callee != p,
        NodeKind::TaggedTemplate { tag, .. } => tag != p,
        // `[config.a] = x`, `for (config.a of y)` and the like write the property.
        NodeKind::ArrayPattern(_) | NodeKind::ObjectPattern(_) | NodeKind::PatternProp { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } => false,
        _ => true,
    }
}
