//! Parameters that have the same value at every call: an `options` that no caller passes is `undefined` everywhere in the function.
//!
//! A library written as `function addDays(date, amount, options) { return constructFrom(options?.in || date, ...) }` takes the options for
//! callers outside; inside a bundle where nobody passes them, `options?.in || date` is `date`. Rollup knows this, and it is where most of its
//! lead over other bundlers comes from on libraries of that style.
//!
//! The analysis is a constant propagation over the calls of the bundle: a function declared at the top of an ES module is a candidate when
//! every reference to it (in its own module, and through the imports that lead to it) is the callee of a call with no spread, it is not
//! exported out of the bundle, it does not read `arguments` and nothing in it calls `eval`. For each parameter that is a plain name, with no
//! default, never assigned, the value is what the arguments at all the calls are: nothing at all is `undefined`, and a call inside another
//! candidate function passes the value of its parameter when that is known. The values form a lattice (nothing seen yet, one constant, not
//! constant) and are joined over the calls until they stop changing.
//!
//! The result is a table of `(module, parameter symbol)` to the value. The bundler writes the value in place of each read of the parameter
//! when it minifies; the parameter and the arguments stay, so a wrong guess could not change what is passed, only what is read.

use crate::ast::{Ast, BinOp, NodeId, NodeKind, flags};
use crate::define::{Const, Defines};
use crate::graph::{Edge, Graph, ModuleId, ModuleKind};
use crate::link::{Binding, Linked, export_names, resolve_export};
use crate::module::{ExportSource, ImportName, RequestKind};
use crate::semantic::{Semantic, SymbolId, scope_flags};
use std::collections::HashMap;

/// The value of a parameter, by module and the symbol of the parameter.
pub type ParamConsts = HashMap<(ModuleId, SymbolId), Const>;

#[derive(Clone, Debug, PartialEq)]
enum Lat {
    /// No call has been seen that passes anything yet.
    Bottom,
    One(Const),
    Many,
}

fn join(a: &Lat, b: &Lat) -> Lat {
    match (a, b) {
        (Lat::Bottom, x) | (x, Lat::Bottom) => x.clone(),
        (Lat::One(x), Lat::One(y)) if x == y => Lat::One(x.clone()),
        _ => Lat::Many,
    }
}

struct Candidate {
    module: ModuleId,
    /// The symbol of each parameter that is a plain name, in order; `None` for any other kind (a default, a rest, a pattern).
    params: Vec<Option<SymbolId>>,
    state: Vec<Lat>,
    bad: bool,
}

struct Site {
    module: ModuleId,
    candidate: usize,
    args: Vec<NodeId>,
}

/// The parameters of the bundle that have one value at every call. Empty when there is nothing to say.
pub fn analyze(graph: &Graph, linked: &Linked, shaken: &crate::shake::Shaken) -> ParamConsts {
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
            let NodeKind::Function { name: Some(name), params, body } = ast[declaration].kind else { continue };
            if !ast[declaration].has(flags::DECL) {
                continue;
            }
            let Some(symbol) = sem.symbol_of(name) else { continue };
            let list = ast.list(params);
            let plain: Vec<Option<SymbolId>> = list.iter().map(|&p| if matches!(ast[p].kind, NodeKind::BindingIdent(_)) { sem.symbol_of(p) } else { None }).collect();
            if plain.iter().all(Option::is_none) {
                continue;
            }
            // Calls with one more argument than there are parameters are fine; `arguments` and `eval` can read any of them.
            let bad = reads_arguments(ast, sem, body) || has_eval(sem, plain.iter().flatten().next().copied());
            by_symbol.insert((module.id, symbol), candidates.len());
            candidates.push(Candidate { module: module.id, state: vec![Lat::Bottom; plain.len()], params: plain, bad });
        }
    }
    if candidates.is_empty() {
        return ParamConsts::new();
    }
    // `export default addDays;` gives the function a second name, the one `import addDays from './addDays'` binds: the expression that is exported.
    let mut default_alias: HashMap<(ModuleId, NodeId), usize> = HashMap::new();
    for module in &graph.modules {
        let Some(parsed) = module.parsed.as_ref() else { continue };
        for export in &module.record.exports {
            if let ExportSource::DefaultValue(node) = export.source {
                if let Some(&c) = parsed.semantic.symbol_of(node).and_then(|s| by_symbol.get(&(module.id, s))) {
                    default_alias.insert((module.id, node), c);
                }
            }
        }
    }
    // The names that lead to a candidate: its own, and the imports in other modules that resolve to it.
    let mut local: Vec<HashMap<SymbolId, usize>> = vec![HashMap::new(); graph.modules.len()];
    for (&(module, symbol), &c) in &by_symbol {
        local[module.index()].insert(symbol, c);
    }
    for (m, imports) in linked.imports.iter().enumerate() {
        for (&symbol, binding) in imports {
            match binding {
                Binding::Symbol { module, symbol: target } => {
                    if let Some(&c) = by_symbol.get(&(*module, *target)) {
                        local[m].insert(symbol, c);
                    }
                }
                Binding::DefaultValue { module, node } => {
                    if let Some(&c) = default_alias.get(&(*module, *node)) {
                        local[m].insert(symbol, c);
                    }
                }
                _ => {}
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
            match resolve_export(graph, module.id, &name) {
                Some(Binding::Symbol { module: at, symbol }) => {
                    if let Some(&c) = by_symbol.get(&(at, symbol)) {
                        candidates[c].bad = true;
                    }
                }
                Some(Binding::DefaultValue { module: at, node }) => {
                    if let Some(&c) = default_alias.get(&(at, node)) {
                        candidates[c].bad = true;
                    }
                }
                _ => {}
            }
        }
        for (&(at, _), &c) in default_alias.iter().filter(|((at, _), _)| *at == module.id) {
            let _ = at;
            candidates[c].bad = true;
        }
        // A module that is not ES (CommonJS, JSON) is not a candidate, and one that exports a name by a path the lookup above cannot follow
        // keeps the functions it names out.
        for export in &module.record.exports {
            if let ExportSource::Local(symbol) = export.source {
                if let Some(&c) = by_symbol.get(&(module.id, symbol)) {
                    candidates[c].bad = true;
                }
            }
        }
    }
    // Every reference to a candidate must be a call.
    let mut sites: Vec<Site> = Vec::new();
    for module in &graph.modules {
        let names = &local[module.id.index()];
        if names.is_empty() {
            continue;
        }
        let Some(parsed) = module.parsed.as_ref() else { continue };
        let (ast, sem) = (&parsed.ast, &parsed.semantic);
        // Only code that stays in the output counts: a call in a statement that tree shaking left out does not happen.
        let shake = &shaken.modules[module.id.index()];
        if !shake.included {
            continue;
        }
        let dead: std::collections::HashSet<NodeId> = if shake.demand_known {
            let mut live: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
            let mut all: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
            for (part, &l) in shake.parts.iter().zip(&shake.live) {
                all.insert(part.stmt);
                if l {
                    live.insert(part.stmt);
                }
            }
            all.difference(&live).copied().collect()
        } else {
            Default::default()
        };
        let mut stack: Vec<(NodeId, Option<NodeId>)> = vec![(ast.root, None)];
        while let Some((n, parent)) = stack.pop() {
            if matches!(ast[n].kind, NodeKind::Ident(_)) {
                if let Some(&c) = sem.symbol_of(n).and_then(|s| names.get(&s)) {
                    match parent.map(|p| (p, ast[p].kind)) {
                        Some((p, NodeKind::Call { callee, args })) if callee == n && !ast[p].has(flags::OPTIONAL) && !ast.list(args).iter().any(|&a| matches!(ast[a].kind, NodeKind::Spread(_))) => {
                            sites.push(Site { module: module.id, candidate: c, args: ast.list(args).to_vec() });
                        }
                        // `export default f;`: the other names it gives the function are followed above, and a module that is exposed marks it.
                        Some((_, NodeKind::ExportDefault(value))) if value == n => {}
                        _ => candidates[c].bad = true,
                    }
                }
            }
            ast.for_each_child(n, |child| {
                if !(parent.is_none() && dead.contains(&child)) {
                    stack.push((child, Some(n)));
                }
            });
        }
    }
    // The symbol of each parameter, to find the value a call passes when it passes a parameter on.
    let mut param_at: HashMap<(ModuleId, SymbolId), (usize, usize)> = HashMap::new();
    for (c, candidate) in candidates.iter().enumerate() {
        for (i, p) in candidate.params.iter().enumerate() {
            if let Some(symbol) = p {
                param_at.insert((candidate.module, *symbol), (c, i));
            }
        }
    }
    // Joined over the calls until nothing changes (each value moves up a lattice of three levels, so this ends).
    for _ in 0..64 {
        let mut changed = false;
        for site in &sites {
            let Some(parsed) = graph.module(site.module).parsed.as_ref() else { continue };
            let (ast, sem) = (&parsed.ast, &parsed.semantic);
            let count = candidates[site.candidate].params.len();
            for i in 0..count {
                if candidates[site.candidate].params[i].is_none() {
                    continue;
                }
                let value = match site.args.get(i) {
                    Some(&arg) => value_of(ast, sem, site.module, arg, &candidates, &param_at, 0),
                    None => Lat::One(Const::Undefined),
                };
                let joined = join(&candidates[site.candidate].state[i], &value);
                if joined != candidates[site.candidate].state[i] {
                    candidates[site.candidate].state[i] = joined;
                    changed = true;
                }
            }
        }
        if !changed {
            let mut out = ParamConsts::new();
            for candidate in candidates.iter().filter(|c| !c.bad) {
                let Some(parsed) = graph.module(candidate.module).parsed.as_ref() else { continue };
                for (i, p) in candidate.params.iter().enumerate() {
                    let (Some(symbol), Lat::One(value)) = (p, &candidate.state[i]) else { continue };
                    if parsed.semantic.symbol(*symbol).writes == 0 && replaceable(value) {
                        out.insert((candidate.module, *symbol), value.clone());
                    }
                }
            }
            return out;
        }
    }
    ParamConsts::new()
}

/// Values that can be written where a name was read without a change of meaning in front of `.`, `(` or `[`.
fn replaceable(value: &Const) -> bool {
    matches!(value, Const::Undefined | Const::Null | Const::Bool(_))
}

/// Whether `eval` is called in the function (the scope of its parameter, or one inside it).
fn has_eval(sem: &Semantic, any_param: Option<SymbolId>) -> bool {
    any_param.is_some_and(|p| sem.scope(sem.symbol(p).scope).flags & scope_flags::HAS_EVAL != 0)
}

/// Whether the function body mentions `arguments` (other than as a name it declares).
fn reads_arguments(ast: &Ast, sem: &Semantic, body: NodeId) -> bool {
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        if let NodeKind::Ident(name) = ast[n].kind {
            if ast.atom(name) == "arguments" {
                return true;
            }
        }
        // A nested function that is not an arrow has an `arguments` of its own.
        if let NodeKind::Function { .. } = ast[n].kind {
            if !ast[n].has(flags::ARROW) && n != body {
                continue;
            }
        }
        ast.for_each_child(n, |c| stack.push(c));
    }
    let _ = sem;
    false
}

/// What the argument expression `node` is worth, given what is known of the parameters it may pass on.
fn value_of(ast: &Ast, sem: &Semantic, module: ModuleId, node: NodeId, candidates: &[Candidate], param_at: &HashMap<(ModuleId, SymbolId), (usize, usize)>, depth: u16) -> Lat {
    // A chain of 50,000 `||` is a tree that deep: past a few levels nothing is known.
    if depth > 40 {
        return Lat::Many;
    }
    if let Some(c) = Defines::default().fold(ast, sem, node) {
        return Lat::One(c);
    }
    let again = |n: NodeId| value_of(ast, sem, module, n, candidates, param_at, depth + 1);
    match ast[node].kind {
        NodeKind::Ident(_) => match sem.symbol_of(node).and_then(|s| param_at.get(&(module, s))) {
            Some(&(c, i)) if !candidates[c].bad && sem.symbol(sem.symbol_of(node).expect("a symbol")).writes == 0 => candidates[c].state[i].clone(),
            _ => Lat::Many,
        },
        // `a?.b.c`: nothing when the first `?.` finds `null` or `undefined`.
        NodeKind::Chain(inner) => {
            let mut at = inner;
            loop {
                let next = match ast[at].kind {
                    NodeKind::Member { object, .. } => object,
                    NodeKind::Call { callee, .. } => callee,
                    _ => return Lat::Many,
                };
                if ast[at].has(flags::OPTIONAL) {
                    match again(next) {
                        Lat::One(Const::Undefined | Const::Null) => return Lat::One(Const::Undefined),
                        Lat::Bottom => return Lat::Bottom,
                        _ => {}
                    }
                }
                at = next;
            }
        }
        NodeKind::Binary { op: op @ (BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish), left, right } => match again(left) {
            Lat::One(c) => {
                let keeps_left = match op {
                    BinOp::LogAnd => !c.truthy(),
                    BinOp::LogOr => c.truthy(),
                    _ => !matches!(c, Const::Undefined | Const::Null),
                };
                if keeps_left { Lat::One(c) } else { again(right) }
            }
            other => other,
        },
        NodeKind::Conditional { test, cons, alt } => match again(test) {
            Lat::One(c) => again(if c.truthy() { cons } else { alt }),
            other => other,
        },
        _ => Lat::Many,
    }
}
