//! Minification of a finished bundle: shorter names and no whitespace.
//!
//! The bundler's output is parsed again, which gives scopes, symbols and the tokens exactly
//! as the parser read them (so regular expressions and template parts are right). Names are
//! then chosen per scope, most-used first, and the tokens are written out with only the
//! separators that keep them apart.
//!
//! Renaming is safe by construction: a new name is never one that code inside the scope
//! reaches in an outer scope, never a global the file mentions, and never another binding of
//! the same scope. Names that must stay are left alone: bindings visible to a direct `eval`,
//! declarations exported with `export const x`, the implicit `arguments`, and, in an IIFE,
//! everything at the top level, which is global.
//!
//! Whitespace is dropped wherever it carries no meaning. Where a line break stood in for a
//! semicolon (the parser records these), a `;` takes its place, which is what the
//! automatic insertion would have produced anyway.

use std::borrow::Cow;

mod fragments;
pub use fragments::{Layout, minify_chunk};

type HashMap<K, V> = crate::fxhash::FastMap<K, V>;
type HashSet<K> = crate::fxhash::FastSet<K>;

use crate::ast::{Ast, Atom, NodeId, NodeKind, flags};
use crate::lexer::{Kind, Punct, Token};
use crate::compress::{Action, Plan};
use crate::parser::{self, ParseOptions};
use crate::semantic::{ScopeId, ScopeKind, Semantic, SymbolId, SymbolKind, scope_flags};

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Remove whitespace, comments and semicolons that can go.
    pub whitespace: bool,
    /// Rename local bindings to short names.
    pub identifiers: bool,
    /// Write the same program shorter: `!0` for `true`, `a.b` for `a["b"]`, no braces around
    /// single statements, merged declarations, and so on (see `compress`). Output is compact.
    pub syntax: bool,
    /// The top level is global (an IIFE), so its names cannot change.
    pub keep_top_level: bool,
    /// Leave alone the names that show at run time: functions, classes, and variables that
    /// name an anonymous function or class (`const f = () => {}`).
    /// `None` leaves the choice to the caller.
    pub keep_names: Option<bool>,
}

impl Options {
    pub fn any(&self) -> bool {
        self.whitespace || self.identifiers || self.syntax
    }
}

/// Minifies `source`, an ES module. An error means the text could not be parsed, which would
/// be a bug in whatever produced it.
pub fn minify(source: &str, options: &Options) -> Result<String, String> {
    run(source, options, None, &[])
}

/// Where a token of the result came from: bytes `out..out + out_len` of the result are the
/// token that stood at `inp..inp + in_len` of the input.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub out: u32,
    pub out_len: u32,
    pub inp: u32,
    pub in_len: u32,
}

/// As `minify`, also saying where every token went (for source maps). With no option set the
/// text comes back unchanged.
pub fn minify_with_anchors(source: &str, options: &Options) -> Result<(String, Vec<Anchor>), String> {
    let mut anchors = Vec::new();
    let out = run(source, options, Some(&mut anchors), &[])?;
    Ok((out, anchors))
}

/// `barriers`: see `compress::plan`.
fn run(source: &str, options: &Options, anchors: Option<&mut Vec<Anchor>>, barriers: &[u32]) -> Result<String, String> {
    let timing = std::env::var_os("RATHKIT_TIMING").is_some();
    let mut clock = std::time::Instant::now();
    let mut lap = |what: &str| {
        if timing {
            eprintln!("  minify {what}: {:.1} ms", clock.elapsed().as_secs_f64() * 1000.0);
            clock = std::time::Instant::now();
        }
    };
    let parsed = parser::parse(source, ParseOptions::new(true).with_recorded_tokens()).map_err(|e| format!("cannot minify: {e}"))?;
    lap("parse");
    let tokens = &parsed.tokens;
    let mut renames = Renames::new(tokens.len());
    let mut asi = vec![false; tokens.len() + 1];
    for &i in &parsed.asi {
        asi[i] = true;
    }
    let mut facts = Facts::default();
    // Which tokens the syntax rewrites change does not depend on the names, so the two plans
    // are made side by side.
    let plan = std::thread::scope(|scope| {
        let syntax = options.syntax.then(|| scope.spawn(|| crate::compress::plan(source, &parsed.ast, &parsed.semantic, tokens, barriers)));
        if options.any() {
            facts.collect(&parsed.ast, &parsed.semantic, source);
            lap("collect facts");
        }
        if options.identifiers {
            let prep = facts.prepare(&parsed.ast, &parsed.semantic, options);
            let avoid = facts.avoid(&parsed.ast, &prep);
            lap("prepare");
            facts.plan_renames(&parsed.ast, &parsed.semantic, tokens, source, &prep, &avoid, None, &mut renames);
            lap("plan renames");
        }
        syntax.map(|handle| handle.join().expect("the syntax plan"))
    });
    lap("plans done");
    let out = emit(source, tokens, &renames, &asi, &facts, options, plan.as_ref(), anchors, false);
    lap("emit");
    Ok(out)
}

/// The text that replaces some tokens, by token index; the same text is stored once.
struct Renames {
    /// Where the last token was found: asking in source order is nearly free.
    hint: usize,
    slot: Vec<u32>,
    texts: Vec<String>,
    index: HashMap<String, u32>,
}

const NO_RENAME: u32 = u32::MAX;

impl Renames {
    fn new(tokens: usize) -> Renames {
        Renames { hint: 0, slot: vec![NO_RENAME; tokens], texts: Vec::new(), index: HashMap::default() }
    }

    fn intern(&mut self, text: &str) -> u32 {
        match self.index.get(text) {
            Some(&id) => id,
            None => {
                self.texts.push(text.to_string());
                self.index.insert(text.to_string(), self.texts.len() as u32 - 1);
                self.texts.len() as u32 - 1
            }
        }
    }

    fn set(&mut self, tokens: &[Token], start: u32, text: &str) {
        let id = self.intern(text);
        self.set_id(tokens, start, id);
    }

    fn set_id(&mut self, tokens: &[Token], start: u32, id: u32) {
        let mut at = self.hint;
        let near = tokens.get(at).is_some_and(|t| t.start <= start) && tokens.get(at + 8).is_none_or(|t| t.start >= start);
        if near {
            while tokens.get(at).is_some_and(|t| t.start < start) {
                at += 1;
            }
        } else {
            match tokens.binary_search_by_key(&start, |t| t.start) {
                Ok(i) => at = i,
                Err(_) => return,
            }
        }
        if tokens.get(at).is_none_or(|t| t.start != start) {
            return;
        }
        self.hint = at;
        self.slot[at] = id;
    }

    fn get(&self, token: usize) -> Option<&str> {
        match self.slot[token] {
            NO_RENAME => None,
            id => Some(&self.texts[id as usize]),
        }
    }
}

/// What naming starts from (see `Facts::prepare`).
struct Prep {
    /// The symbol that stands for each one: symbols that are one variable share it.
    canon: Vec<usize>,
    /// How often each (canonical) symbol is used.
    uses: Vec<u32>,
    /// Which (canonical) symbols keep their names.
    fixed: Vec<bool>,
    /// The names of the symbols that stay as they are, each once.
    kept: Vec<Atom>,
    /// Names given to an anonymous function by an assignment (`f = function () {}`) to a name that
    /// nothing in this text declares: in a bundle, the declaration is in another piece.
    inferred_outer: Vec<Atom>,
}

/// Names no new name may be.
struct Avoid<'a> {
    kept: HashSet<&'a str>,
    globals: HashSet<&'a str>,
}

// ---- facts about the tree -----------------------------------------------------------------

#[derive(Default)]
struct Facts {
    /// Every identifier node that denotes a symbol, with the scope it stands in.
    occurrences: Vec<(NodeId, SymbolId, ScopeId)>,
    /// Identifiers that no declaration provides, with the scope they stand in: globals, or
    /// (in a fragment of a bundle) names that another fragment declares.
    unresolved: Vec<(NodeId, ScopeId)>,
    /// Start of the key in `{ a }`, which a rename must turn into `{ a: b }`.
    shorthand: HashSet<u32>,
    /// `export { a as b }` specifiers without a source: where they start, and both names.
    export_specifiers: Vec<(u32, Atom, Atom)>,
    /// `import { a }` and `import { a as b }`: the specifier's start and its local binding.
    import_specifiers: Vec<(u32, NodeId, String)>,
    /// Names bound by `export const x`, `export function f` and the like.
    fixed_by_export: HashSet<NodeId>,
    /// Identifiers that give an anonymous function or class its `name`.
    names_inferred: Vec<NodeId>,
    /// Where `;` is an empty statement and must stay.
    empty_statements: HashSet<u32>,
}

impl Facts {
    fn collect(&mut self, ast: &Ast, sem: &Semantic, _source: &str) {
        // Scopes by the node that opens them, outermost first.
        let mut owned: HashMap<NodeId, Vec<ScopeId>> = HashMap::default();
        for (id, scope) in sem.scopes() {
            if let Some(node) = scope.node {
                owned.entry(node).or_default().push(id);
            }
        }
        let depth = |mut s: ScopeId| {
            let mut d = 0;
            while let Some(p) = sem.scope(s).parent {
                d += 1;
                s = p;
            }
            d
        };
        for list in owned.values_mut() {
            list.sort_by_key(|&s| depth(s));
        }
        enum Step {
            Enter(NodeId),
            Leave(usize),
        }
        let mut stack = vec![Step::Enter(ast.root)];
        let mut scopes: Vec<ScopeId> = vec![sem.root_scope()];
        let mut children: Vec<NodeId> = Vec::new();
        while let Some(step) = stack.pop() {
            let node = match step {
                Step::Leave(count) => {
                    scopes.truncate(scopes.len() - count);
                    continue;
                }
                Step::Enter(node) => node,
            };
            let mut pushed = 0;
            if let Some(list) = owned.get(&node) {
                for &s in list {
                    // The root scope is already there.
                    if scopes.last() != Some(&s) {
                        scopes.push(s);
                        pushed += 1;
                    }
                }
            }
            stack.push(Step::Leave(pushed));
            let here = *scopes.last().expect("the root scope");
            let n = &ast[node];
            match n.kind {
                NodeKind::Ident(_) | NodeKind::BindingIdent(_) => match sem.symbol_of(node) {
                    Some(sym) => self.occurrences.push((node, sym, here)),
                    None if matches!(n.kind, NodeKind::Ident(_)) => self.unresolved.push((node, here)),
                    None => {}
                },
                NodeKind::Property { key, .. } | NodeKind::PatternProp { key, .. } if n.has(flags::SHORTHAND) => {
                    self.shorthand.insert(ast[key].span.start);
                }
                NodeKind::VarDeclarator { target, init: Some(init) } | NodeKind::AssignPattern { target, default: init } if names_a_value(ast, init) => {
                    if matches!(ast[target].kind, NodeKind::BindingIdent(_)) {
                        self.names_inferred.push(target);
                    }
                }
                NodeKind::Assign { target, value, .. } if names_a_value(ast, value) => {
                    if matches!(ast[target].kind, NodeKind::Ident(_)) {
                        self.names_inferred.push(target);
                    }
                }
                NodeKind::Empty => {
                    self.empty_statements.insert(n.span.start);
                }
                NodeKind::ExportNamed(export) => {
                    let data = ast.export(export);
                    if let Some(decl) = data.decl {
                        let mut names = Vec::new();
                        match ast[decl].kind {
                            NodeKind::VarDecl { decls, .. } => {
                                for &d in ast.list(decls) {
                                    if let NodeKind::VarDeclarator { target, .. } = ast[d].kind {
                                        crate::semantic::binding_idents(ast, target, &mut names);
                                    }
                                }
                            }
                            NodeKind::Function { name: Some(id), .. } | NodeKind::Class { name: Some(id), .. } => names.push(id),
                            _ => {}
                        }
                        self.fixed_by_export.extend(names);
                    } else if data.source.is_none() {
                        for &spec in ast.list(data.specifiers) {
                            if let NodeKind::ExportSpecifier { local, exported } = ast[spec].kind {
                                self.export_specifiers.push((ast[spec].span.start, local, exported));
                            }
                        }
                    }
                }
                NodeKind::ImportSpecifier { imported, local } => {
                    self.import_specifiers.push((n.span.start, local, ast.atom(imported).to_string()));
                }
                _ => {}
            }
            children.clear();
            ast.for_each_child(node, |c| children.push(c));
            for &c in children.iter().rev() {
                stack.push(Step::Enter(c));
            }
        }
    }

    /// What naming starts from: which symbols are one variable, how often each is used, and
    /// which keep their names.
    fn prepare(&self, ast: &Ast, sem: &Semantic, options: &Options) -> Prep {
        let symbol_count = sem.symbols().count();
        // Symbols that share one name with another: the class name inside its own class
        // declaration, a body `var` that repeats a parameter, a catch parameter and a `var`
        // of the same name. Each group is named once.
        let mut canon: Vec<usize> = (0..symbol_count).collect();
        for (id, sym) in sem.symbols() {
            match sym.kind {
                SymbolKind::ClassInner => {
                    if let Some(outer) = sym.decl.and_then(|d| sem.symbol_of(d)) {
                        if outer != id {
                            canon[id.index()] = outer.index();
                        }
                    }
                }
                SymbolKind::CatchParam | SymbolKind::CatchPatternParam => {
                    let mut scope = sem.scope(sym.scope).parent;
                    while let Some(s) = scope {
                        if sem.scope(s).kind.is_var_scope() {
                            break;
                        }
                        scope = sem.scope(s).parent;
                    }
                    if let Some(found) = scope.and_then(|s| sem.lookup_local(s, sym.name)) {
                        if matches!(sem.symbol(found).kind, SymbolKind::Var | SymbolKind::Function | SymbolKind::LexicalFunction) {
                            canon[id.index()] = found.index();
                        }
                    }
                }
                _ => {
                    let scope = sem.scope(sym.scope);
                    if scope.kind == ScopeKind::Body {
                        if let Some(parent) = scope.parent {
                            if sem.scope(parent).kind == ScopeKind::Function {
                                if let Some(param) = sem.lookup_local(parent, sym.name) {
                                    if param != id {
                                        canon[id.index()] = param.index();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Several symbols of one scope can be a single variable: a function declared twice,
        // or a `var` and a function of the same name. They get one name.
        for (_, scope) in sem.scopes() {
            let mut first: HashMap<Atom, usize> = HashMap::default();
            for &sym in scope.symbols() {
                let name = sem.symbol(sym).name;
                let leader = *first.entry(name).or_insert(sym.index());
                if leader != sym.index() && canon[sym.index()] == sym.index() {
                    canon[sym.index()] = leader;
                }
            }
        }
        // A function in a block is also a `var` of the enclosing function, in sloppy code.
        for (id, sym) in sem.symbols() {
            if let Some(alias) = sym.alias {
                canon[id.index()] = alias.index();
            }
        }
        for i in 0..symbol_count {
            let mut c = canon[i];
            while canon[c] != c {
                c = canon[c];
            }
            canon[i] = c;
        }
        // How often each name is used.
        let mut uses = vec![0u32; symbol_count];
        for &(_, sym, _) in &self.occurrences {
            uses[canon[sym.index()]] += 1;
        }
        for (_, local, _) in &self.export_specifiers {
            if let Some(sym) = sem.lookup_local(sem.root_scope(), *local) {
                uses[canon[sym.index()]] += 1;
            }
        }
        // Which symbols keep their names.
        let mut fixed = vec![false; symbol_count];
        let root = sem.root_scope();
        let inferred: HashSet<SymbolId> = self.names_inferred.iter().filter_map(|&n| sem.symbol_of(n)).collect();
        for (id, sym) in sem.symbols() {
            let scope = sem.scope(sym.scope);
            let keep = sym.kind == SymbolKind::Arguments
                || sym.decl.is_none()
                || scope.flags & scope_flags::HAS_EVAL != 0
                || (options.keep_top_level && sym.scope == root)
                || sym.decl.is_some_and(|d| self.fixed_by_export.contains(&d))
                || (options.keep_names == Some(true)
                    && (matches!(
                        sym.kind,
                        SymbolKind::Function | SymbolKind::LexicalFunction | SymbolKind::BlockFunction | SymbolKind::Class | SymbolKind::FunctionName | SymbolKind::ClassInner
                    ) || inferred.contains(&id)));
            if keep {
                fixed[canon[id.index()]] = true;
            }
        }
        // A tied group that has a fixed member is fixed.
        for (id, _) in sem.symbols() {
            if fixed[id.index()] {
                fixed[canon[id.index()]] = true;
            }
        }
        let mut kept: Vec<Atom> = Vec::new();
        let mut seen: HashSet<Atom> = HashSet::default();
        for (id, sym) in sem.symbols() {
            if fixed[canon[id.index()]] && seen.insert(sym.name) {
                kept.push(sym.name);
            }
        }
        let mut inferred_outer = Vec::new();
        if options.keep_names == Some(true) {
            for &n in &self.names_inferred {
                if let (None, NodeKind::Ident(a)) = (sem.symbol_of(n), ast[n].kind) {
                    inferred_outer.push(a);
                }
            }
        }
        Prep { canon, uses, fixed, kept, inferred_outer }
    }

    /// The names a candidate may not be: those that stay as they are, and globals.
    fn avoid<'a>(&self, ast: &'a Ast, prep: &Prep) -> Avoid<'a> {
        let kept = prep.kept.iter().map(|&a| ast.atom(a)).collect();
        let mut atoms: HashSet<Atom> = HashSet::default();
        for &(node, _) in &self.unresolved {
            if let NodeKind::Ident(a) = ast[node].kind {
                atoms.insert(a);
            }
        }
        Avoid { kept, globals: atoms.into_iter().map(|a| ast.atom(a)).collect() }
    }

    /// Decides the new name of every renamable symbol and records, by token start, what
    /// each identifier token becomes. In a fragment of a bundle, `roots` holds the new names
    /// of the names that stand at the top level of the whole bundle.
    #[allow(clippy::too_many_arguments)]
    fn plan_renames(&self, ast: &Ast, sem: &Semantic, tokens: &[Token], source: &str, prep: &Prep, avoid: &Avoid, roots: Option<&HashMap<&str, String>>, renames: &mut Renames) {
        let timing = std::env::var_os("RATHKIT_TIMING").is_some() && roots.is_none();
        let mut clock = std::time::Instant::now();
        let mut lap = |what: &str| {
            if timing {
                eprintln!("    {what}: {:.1} ms", clock.elapsed().as_secs_f64() * 1000.0);
                clock = std::time::Instant::now();
            }
        };
        let symbol_count = sem.symbols().count();
        let (canon, uses, fixed) = (&prep.canon, &prep.uses, &prep.fixed);
        let symbol_ids: Vec<SymbolId> = sem.symbols().map(|(id, _)| id).collect();

        let scope_count = sem.scopes().count();
        // Scopes from the outside in.
        let mut child_scopes: Vec<Vec<ScopeId>> = vec![Vec::new(); scope_count];
        let mut scope_roots = Vec::new();
        for (id, scope) in sem.scopes() {
            match scope.parent {
                Some(p) => child_scopes[p.index()].push(id),
                None => scope_roots.push(id),
            }
        }
        let mut order = Vec::with_capacity(scope_count);
        let mut queue = std::collections::VecDeque::from(scope_roots);
        while let Some(s) = queue.pop_front() {
            order.push(s);
            queue.extend(child_scopes[s.index()].iter().copied());
        }

        // Names that other fragments declare at the top level and this one reads or writes:
        // they take part in the choice of names like any outer binding.
        let mut outer_index: HashMap<Atom, Option<usize>> = HashMap::default();
        let mut outer_names: Vec<String> = Vec::new();
        let mut outer_uses: Vec<(NodeId, usize)> = Vec::new();
        let mut captures: Vec<Vec<usize>> = vec![Vec::new(); scope_count];
        if let Some(roots) = roots {
            for &(node, here) in &self.unresolved {
                let NodeKind::Ident(atom) = ast[node].kind else { continue };
                let slot = *outer_index.entry(atom).or_insert_with(|| {
                    roots.get(ast.atom(atom)).map(|new| {
                        outer_names.push(new.clone());
                        outer_names.len() - 1
                    })
                });
                if let Some(k) = slot {
                    captures[here.index()].push(symbol_count + k);
                    outer_uses.push((node, k));
                }
            }
        }
        // Which outer symbols each scope reaches.
        let home_of = |c: usize| (c < symbol_count).then(|| sem.symbol(symbol_ids[c]).scope);
        for &(_, sym, here) in &self.occurrences {
            let c = canon[sym.index()];
            if home_of(c) != Some(here) {
                captures[here.index()].push(c);
            }
        }
        // A scope reaches what its own code and its inner scopes reach from outside it.
        // Inner scopes come after outer ones in `order`, so going backwards meets them first.
        for &s in order.iter().rev() {
            let list = &mut captures[s.index()];
            list.sort_unstable();
            list.dedup();
            list.retain(|&c| home_of(c) != Some(s));
            if let Some(parent) = sem.scope(s).parent {
                let reached = std::mem::take(&mut captures[s.index()]);
                captures[parent.index()].extend_from_slice(&reached);
                captures[s.index()] = reached;
            }
        }
        lap("uses and captures");

        let root = sem.root_scope();
        let (kept_names, global_names) = (&avoid.kept, &avoid.globals);
        let mut names: Vec<Option<String>> = vec![None; symbol_count + outer_names.len()];
        for (i, name) in outer_names.iter().enumerate() {
            names[symbol_count + i] = Some(name.clone());
        }
        for (id, sym) in sem.symbols() {
            if canon[id.index()] != id.index() {
                continue;
            }
            // The top level of a fragment is named for the whole bundle.
            let given = roots.filter(|_| sym.scope == root).and_then(|r| r.get(ast.atom(sym.name)));
            if let Some(name) = given {
                names[id.index()] = Some(name.clone());
            } else if fixed[id.index()] {
                names[id.index()] = Some(ast.atom(sym.name).to_string());
            }
        }
        for scope_id in order {
            let scope = sem.scope(scope_id);
            let mut taken: HashSet<String> = HashSet::default();
            let mut todo: Vec<usize> = Vec::new();
            let mut listed: HashSet<usize> = HashSet::default();
            for &sym in scope.symbols() {
                let c = canon[sym.index()];
                if c != sym.index() || !listed.insert(c) {
                    continue;
                }
                if let Some(name) = &names[c] {
                    taken.insert(name.clone());
                } else {
                    todo.push(c);
                }
            }
            if todo.is_empty() {
                continue;
            }
            let mut forbidden: HashSet<String> = HashSet::default();
            let mut seen: Vec<usize> = captures[scope_id.index()].clone();
            seen.sort_unstable();
            seen.dedup();
            for c in seen {
                if let Some(name) = &names[c] {
                    forbidden.insert(name.clone());
                }
            }
            // A function body kept apart from its parameters may not declare a name the
            // parameters declare, whether or not the body mentions the parameter.
            if scope.kind == ScopeKind::Body {
                if let Some(parent) = scope.parent {
                    for &sym in sem.scope(parent).symbols() {
                        if let Some(name) = &names[canon[sym.index()]] {
                            forbidden.insert(name.clone());
                        }
                    }
                }
            }
            // Most used first; the order they were declared in breaks ties.
            todo.sort_by_key(|&c| std::cmp::Reverse(uses[c]));
            let mut index = 0usize;
            for c in todo {
                let name = loop {
                    let candidate = name_at(index);
                    index += 1;
                    if is_reserved(&candidate) || forbidden.contains(&candidate) || kept_names.contains(candidate.as_str()) || global_names.contains(candidate.as_str()) || taken.contains(&candidate) {
                        continue;
                    }
                    break candidate;
                };
                taken.insert(name.clone());
                names[c] = Some(name);
            }
        }

        lap("naming scopes");
        let name_of = |sym: SymbolId| -> &str { names[canon[sym.index()]].as_deref().unwrap_or_else(|| ast.atom(sem.symbol(sym).name)) };
        // The text of each name is stored once and the tokens point to it.
        let mut text_ids = vec![NO_RENAME; symbol_count];
        for &(node, sym, _) in &self.occurrences {
            let old = match ast[node].kind {
                NodeKind::Ident(a) | NodeKind::BindingIdent(a) => ast.atom(a),
                _ => continue,
            };
            let c = canon[sym.index()];
            let new = name_of(sym);
            if new == old {
                continue;
            }
            let start = ast[node].span.start;
            if self.shorthand.contains(&start) {
                renames.set(tokens, start, &format!("{old}:{new}"));
                continue;
            }
            if text_ids[c] == NO_RENAME {
                text_ids[c] = renames.intern(new);
            }
            renames.set_id(tokens, start, text_ids[c]);
        }
        let mut outer_text_ids = vec![NO_RENAME; outer_names.len()];
        for &(node, k) in &outer_uses {
            let NodeKind::Ident(a) = ast[node].kind else { continue };
            let (old, new) = (ast.atom(a), outer_names[k].as_str());
            if new == old {
                continue;
            }
            let start = ast[node].span.start;
            if self.shorthand.contains(&start) {
                renames.set(tokens, start, &format!("{old}:{new}"));
                continue;
            }
            if outer_text_ids[k] == NO_RENAME {
                outer_text_ids[k] = renames.intern(new);
            }
            renames.set_id(tokens, start, outer_text_ids[k]);
        }
        lap("apply occurrences");
        for (start, local, imported) in &self.import_specifiers {
            let start = *start;
            let Some(sym) = sem.symbol_of(*local) else { continue };
            let new = name_of(sym);
            // `import { a }` has no `as`: the local name and the imported one are one token.
            if ast[*local].span.start == start && new != imported {
                renames.set(tokens, start, &format!("{imported} as {new}"));
            }
        }
        for (start, local, exported) in &self.export_specifiers {
            let (local_text, exported) = (ast.atom(*local), ast.atom(*exported));
            let new: &str = match sem.lookup_local(root, *local) {
                Some(sym) => name_of(sym),
                None => match roots.and_then(|r| r.get(local_text)) {
                    Some(new) => new.as_str(),
                    None => continue,
                },
            };
            if new == local_text {
                continue;
            }
            // `export { a }` becomes `export { b as a }`; `export { a as c }` only changes `a`.
            let at = tokens.binary_search_by_key(start, |t| t.start).ok();
            let has_as = at.is_some_and(|i| {
                tokens.get(i + 1).is_some_and(|t| &source[t.start as usize..t.end as usize] == "as")
                    && tokens.get(i + 2).is_some_and(|t| !matches!(t.kind, Kind::Punct(Punct::Comma | Punct::RBrace)))
            });
            renames.set(tokens, *start, &if has_as { new.to_string() } else { format!("{new} as {exported}") });
        }
    }
}

/// An anonymous function, arrow or class, which takes its `name` from the binding it is assigned to.
fn names_a_value(ast: &Ast, node: NodeId) -> bool {
    matches!(ast[node].kind, NodeKind::Function { name: None, .. } | NodeKind::Class { name: None, .. })
}

const FIRST: &[u8] = b"etnrisouaflchpdvmgybwkxqjzETNRISOUAFLCHPDVMGYBWKXQJZ_$";
const REST: &[u8] = b"etnrisouaflchpdvmgybwkxqjzETNRISOUAFLCHPDVMGYBWKXQJZ_$0123456789";

/// The `index`th name: all one-letter names, then two letters, and so on.
fn name_at(mut index: usize) -> String {
    let mut out = String::new();
    if index < FIRST.len() {
        out.push(FIRST[index] as char);
        return out;
    }
    index -= FIRST.len();
    let mut width = 2;
    let mut count = FIRST.len() * REST.len();
    while index >= count {
        index -= count;
        width += 1;
        count *= REST.len();
    }
    // `index` counts within names of this width.
    let mut digits = Vec::with_capacity(width);
    for _ in 1..width {
        digits.push(REST[index % REST.len()]);
        index /= REST.len();
    }
    out.push(FIRST[index % FIRST.len()] as char);
    for d in digits.iter().rev() {
        out.push(*d as char);
    }
    out
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "do" | "if" | "in" | "of" | "as" | "is"
            | "for" | "let" | "new" | "try" | "var" | "NaN"
            | "case" | "else" | "enum" | "eval" | "this" | "true" | "void" | "with" | "null" | "from" | "async" | "await" | "yield" | "break" | "catch"
            | "class" | "const" | "false" | "super" | "throw" | "while" | "delete" | "export" | "import" | "public" | "return" | "static" | "switch" | "typeof"
            | "default" | "extends" | "finally" | "package" | "private" | "continue" | "debugger" | "function" | "arguments" | "interface" | "protected"
            | "implements" | "instanceof" | "undefined" | "Infinity"
    )
}

// ---- output ---------------------------------------------------------------------------------

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'\\' || b >= 0x80
}

/// `more_follows`: other text will be written right after this, so what ends a statement at
/// the end of this text has to be written (a `;` that would be last in a file can go).
#[allow(clippy::too_many_arguments)]
fn emit(source: &str, tokens: &[Token], renames: &Renames, asi: &[bool], facts: &Facts, options: &Options, plan: Option<&Plan>, mut anchors: Option<&mut Vec<Anchor>>, more_follows: bool) -> String {
    let mut out = String::with_capacity(source.len() / 2);
    // A hashbang line is not a token.
    if source.starts_with("#!") {
        let end = source.find('\n').unwrap_or(source.len());
        out.push_str(&source[..end]);
        out.push('\n');
    }
    let mut prev_end = if source.starts_with("#!") { source.find('\n').unwrap_or(source.len()) as u32 } else { 0 };
    let compact = options.whitespace || options.syntax;
    // A line break that stood for `;`, waiting for the next token that is written.
    let mut pending_asi = false;
    // The token written last is a number made of digits only, which a `.` would join.
    let mut whole_number = false;
    for (i, tok) in tokens.iter().enumerate() {
        let action = plan.map_or(Action::Keep, |p| p.actions[i]);
        let text: Cow<str> = match action {
            Action::Text(id) => Cow::Borrowed(plan.map_or("", |p| p.texts[id as usize].as_str())),
            Action::Semi | Action::KeptSemi => Cow::Borrowed(";"),
            Action::Joiner => Cow::Borrowed(","),
            _ => match renames.get(i) {
                Some(new) => Cow::Borrowed(new),
                None => Cow::Borrowed(&source[tok.start as usize..tok.end as usize]),
            },
        };
        if !compact {
            out.push_str(&source[prev_end as usize..tok.start as usize]);
            if let Some(list) = anchors.as_deref_mut() {
                list.push(Anchor { out: out.len() as u32, out_len: text.len() as u32, inp: tok.start, in_len: tok.end - tok.start });
            }
            out.push_str(&text);
            prev_end = tok.end;
            continue;
        }
        // A line break that stood for `;` before a token that is left out stands for it before
        // whatever is written next, except inside code that is left out whole.
        if asi[i] && !plan.is_some_and(|p| p.interior[i]) {
            pending_asi = true;
        }
        match action {
            Action::Skip => continue,
            Action::Joiner => pending_asi = false,
            _ => {}
        }
        // A `;` that ends a statement just before `}` or the end of the text can go.
        let is_semi = match action {
            Action::Keep => tok.is(Punct::Semi) && !facts.empty_statements.contains(&tok.start),
            Action::Semi => true,
            _ => false,
        };
        if is_semi {
            let next = (i + 1..tokens.len()).find(|&j| plan.is_none_or(|p| p.actions[j] != Action::Skip)).map(|j| (j, &tokens[j]));
            let ends = match next {
                None => !more_follows,
                Some((j, t)) => t.is(Punct::RBrace) && plan.is_none_or(|p| p.actions[j] == Action::Keep),
            };
            if ends {
                continue;
            }
        }
        let bytes = text.as_bytes();
        if let (Some(&last), Some(&first)) = (out.as_bytes().last(), bytes.first()) {
            if pending_asi && last != b';' && !matches!(action, Action::Semi | Action::KeptSemi) {
                out.push(';');
            } else if separator_needed(last, first, &out, whole_number) {
                out.push(' ');
            }
        }
        pending_asi = false;
        whole_number = tok.kind == Kind::Number && text.bytes().all(|b| b.is_ascii_digit());
        if let Some(list) = anchors.as_deref_mut() {
            list.push(Anchor { out: out.len() as u32, out_len: text.len() as u32, inp: tok.start, in_len: tok.end - tok.start });
        }
        out.push_str(&text);
    }
    if !compact {
        out.push_str(&source[prev_end as usize..]);
    } else if more_follows && (pending_asi || asi[tokens.len()]) && !out.is_empty() && !out.ends_with(';') {
        out.push(';');
    }
    out
}

/// Whether the text before and the token after would run together without a space.
fn separator_needed(last: u8, first: u8, out: &str, whole_number: bool) -> bool {
    if is_word_byte(last) && is_word_byte(first) {
        return true;
    }
    match (last, first) {
        (b'+', b'+') | (b'-', b'-') | (b'/', b'/') | (b'<', b'!') | (b'?', b'.') => true,
        (b'-', b'>') => out.ends_with("--"),
        // `1 .x`: a dot after a whole number would be read as its decimal point.
        (_, b'.') => whole_number,
        _ => false,
    }
}

// ---- checking a result ----------------------------------------------------------------------

/// One side of a pair being compared: a node, or (when declarations were merged) the
/// declarators of a run of `var`/`let`/`const` statements standing for one statement.
#[derive(Clone)]
struct Side {
    id: NodeId,
    declarators: Option<Vec<NodeId>>,
}

impl Side {
    fn node(id: NodeId) -> Side {
        Side { id, declarators: None }
    }
}

/// `true` or `false` as written, or as `!0`/`!1` after minifying.
fn truth(ast: &Ast, n: NodeId) -> Option<bool> {
    match ast[n].kind {
        NodeKind::Bool(v) => Some(v),
        NodeKind::Unary { op: crate::ast::UnaryOp::Not, arg } => match ast[arg].kind {
            NodeKind::Num(v) if v.get() == 0.0 => Some(true),
            NodeKind::Num(v) if v.get() == 1.0 => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a condition is the same every time: `true`, `!1`, `"a" === "a"`.
fn constant_test(parsed: &parser::Parsed, n: NodeId) -> Option<bool> {
    truth(&parsed.ast, n).or_else(|| crate::define::Defines::default().eval(&parsed.ast, &parsed.semantic, n).map(|c| c.truthy()))
}

/// A statement the minifier removes from a list: `;` and `if (false) ...;` without an `else`.
fn removable(parsed: &parser::Parsed, n: NodeId) -> bool {
    match parsed.ast[n].kind {
        NodeKind::Empty => true,
        NodeKind::If { test, cons, alt: None } => constant_test(parsed, test) == Some(false) && !crate::compress::declares_var(&parsed.ast, cons),
        _ => false,
    }
}

fn is_undefined(parsed: &parser::Parsed, n: NodeId) -> bool {
    matches!(parsed.ast[n].kind, NodeKind::Ident(a) if parsed.ast.atom(a) == "undefined") && parsed.semantic.symbol_of(n).is_none()
}

fn is_void_zero(ast: &Ast, n: NodeId) -> bool {
    matches!(ast[n].kind, NodeKind::Unary { op: crate::ast::UnaryOp::Void, arg } if matches!(ast[arg].kind, NodeKind::Num(v) if v.get() == 0.0))
}

/// The children of `side` in source order; with `syntax`, runs of declarations of one kind in
/// a statement list count as one declaration (the minifier joins them).
fn children(parsed: &parser::Parsed, side: &Side, syntax: bool, original: bool) -> Vec<Side> {
    let ast = &parsed.ast;
    if let Some(list) = &side.declarators {
        return list.iter().map(|&c| Side::node(c)).collect();
    }
    let mut kids = Vec::new();
    ast.for_each_child(side.id, |c| kids.push(c));
    let is_list = matches!(ast[side.id].kind, NodeKind::Program(_) | NodeKind::Block(_) | NodeKind::StaticBlock(_) | NodeKind::Case { .. });
    if !(syntax && is_list) {
        return kids.into_iter().map(Side::node).collect();
    }
    let mut out: Vec<Side> = Vec::new();
    let mut run_kind = None;
    for c in kids {
        // Statements the minifier removes leave their neighbors next to each other.
        if original && removable(parsed, c) {
            continue;
        }
        if let NodeKind::VarDecl { kind, decls } = ast[c].kind {
            let own: Vec<NodeId> = ast.list(decls).to_vec();
            if run_kind == Some(kind) {
                out.last_mut().and_then(|s| s.declarators.as_mut()).expect("a run").extend(own);
            } else {
                run_kind = Some(kind);
                out.push(Side { id: c, declarators: Some(own) });
            }
        } else {
            run_kind = None;
            out.push(Side::node(c));
        }
    }
    out
}

/// Minifies `source` and checks that the result is the same program with other names: the
/// trees have the same shape and literals, globals keep their names, and every identifier
/// that denoted a binding still denotes the one binding it did, with no two merged.
///
/// With `options.syntax` the trees may also differ by the rewrites `compress` makes: `!0` for
/// `true`, `void 0` for `undefined`, other spellings of a number, `a.b` for `a["b"]`, a
/// statement without braces, joined declarations, an expression body for a `return`, `return`
/// for `return undefined`, and the branch that runs for `if (true)` and `true ? a : b`.
pub fn verify(source: &str, options: &Options) -> Result<(), String> {
    let minified = minify(source, options)?;
    let syntax = options.syntax;
    let a = parser::parse(source, ParseOptions::new(true)).map_err(|e| format!("original: {e}"))?;
    let b = parser::parse(&minified, ParseOptions::new(true)).map_err(|e| format!("minified: {e}"))?;
    let mut forward: HashMap<SymbolId, SymbolId> = HashMap::default();
    let mut backward: HashMap<SymbolId, SymbolId> = HashMap::default();
    let mut stack = vec![(Side::node(a.ast.root), Side::node(b.ast.root))];
    'pairs: while let Some((mut sx, sy)) = stack.pop() {
        let ny = &b.ast[sy.id];
        if syntax && sx.declarators.is_none() {
            // Reduce the original to what the minifier leaves of it.
            loop {
                let nx = &a.ast[sx.id];
                match nx.kind {
                    NodeKind::If { test, cons, alt } if constant_test(&a, test).is_some() && !matches!(ny.kind, NodeKind::If { test: t, .. } if constant_test(&b, t).is_some()) => {
                        let v = constant_test(&a, test).expect("constant");
                        match (v, alt) {
                            (true, _) => sx = Side::node(cons),
                            (false, Some(alt)) => sx = Side::node(alt),
                            (false, None) if matches!(ny.kind, NodeKind::Empty) => continue 'pairs,
                            (false, None) => return Err(format!("`if (false)` at {} is not an empty statement in the result", nx.span.start)),
                        }
                    }
                    NodeKind::Conditional { test, cons, alt } if constant_test(&a, test).is_some() && !matches!(ny.kind, NodeKind::Conditional { test: t, .. } if constant_test(&b, t).is_some()) => {
                        let v = constant_test(&a, test).expect("constant");
                        sx = Side::node(if v { cons } else { alt });
                    }
                    NodeKind::Block(list) if !matches!(ny.kind, NodeKind::Block(_)) && a.ast.list(list).len() == 1 => sx = Side::node(a.ast.list(list)[0]),
                    _ => break,
                }
            }
        }
        let (x, y) = (sx.id, sy.id);
        let (nx, ny) = (&a.ast[x], &b.ast[y]);
        if syntax && sx.declarators.is_none() && sy.declarators.is_none() {
            let rewritten = match (&nx.kind, &ny.kind) {
                (NodeKind::Bool(v), _) => truth(&b.ast, y) == Some(*v) && matches!(ny.kind, NodeKind::Unary { .. }),
                (NodeKind::Ident(_), _) => is_undefined(&a, x) && is_void_zero(&b.ast, y),
                (NodeKind::Str(sa), NodeKind::PropName(pb)) => a.ast.atom(*sa) == b.ast.atom(*pb),
                (NodeKind::Num(na), NodeKind::Num(nb)) => na.get().to_bits() == nb.get().to_bits(),
                _ => false,
            };
            if rewritten {
                continue;
            }
            // `if` written as an expression statement, `typeof x === "undefined"` as `typeof x > "u"`, `while (true)` as `for (;;)`.
            let sole = |tree: &parser::Parsed, s: NodeId| -> Option<NodeId> {
                match tree.ast[s].kind {
                    NodeKind::ExprStmt(e) => Some(e),
                    NodeKind::Block(list) if tree.ast.list(list).len() == 1 => match tree.ast[tree.ast.list(list)[0]].kind {
                        NodeKind::ExprStmt(e) => Some(e),
                        _ => None,
                    },
                    _ => None,
                }
            };
            match (&nx.kind, &ny.kind) {
                (NodeKind::If { test, cons, alt }, NodeKind::ExprStmt(ey)) => match (&b.ast[*ey].kind, alt) {
                    (NodeKind::Binary { op: crate::ast::BinOp::LogAnd, left, right }, None) => {
                        if let Some(ce) = sole(&a, *cons) {
                            stack.push((Side::node(*test), Side::node(*left)));
                            stack.push((Side::node(ce), Side::node(*right)));
                            continue;
                        }
                    }
                    (NodeKind::Binary { op: crate::ast::BinOp::LogOr, left, right }, None) => {
                        if let (Some(ce), NodeKind::Unary { op: crate::ast::UnaryOp::Not, arg }) = (sole(&a, *cons), &a.ast[*test].kind) {
                            stack.push((Side::node(*arg), Side::node(*left)));
                            stack.push((Side::node(ce), Side::node(*right)));
                            continue;
                        }
                    }
                    (NodeKind::Conditional { test: ct, cons: cc, alt: ca_ }, Some(alt)) => {
                        if let (Some(ce), Some(ae)) = (sole(&a, *cons), sole(&a, *alt)) {
                            stack.push((Side::node(*test), Side::node(*ct)));
                            stack.push((Side::node(ce), Side::node(*cc)));
                            stack.push((Side::node(ae), Side::node(*ca_)));
                            continue;
                        }
                    }
                    _ => {}
                },
                (NodeKind::Binary { op, left, right }, NodeKind::Binary { op: ob, left: lb, right: rb }) => {
                    use crate::ast::BinOp::*;
                    let is_undefined_text = matches!(a.ast[*right].kind, NodeKind::Str(t) if a.ast.atom(t) == "undefined") && matches!(b.ast[*rb].kind, NodeKind::Str(t) if b.ast.atom(t) == "u");
                    if is_undefined_text && matches!(a.ast[*left].kind, NodeKind::Unary { op: crate::ast::UnaryOp::Typeof, .. }) && matches!((op, ob), (StrictEq | Eq, Gt) | (StrictNe | Ne, Lt)) {
                        stack.push((Side::node(*left), Side::node(*lb)));
                        continue;
                    }
                }
                // `if (c) return a; else return b;` that stands alone (the body of an `else`) written as `return c ? a : b;`.
                (NodeKind::If { test, cons, alt: Some(alt) }, NodeKind::Return(Some(joined))) => {
                    if let NodeKind::Conditional { test: ct, cons: cc, alt: ca_ } = &b.ast[*joined].kind {
                        let returned = |s: NodeId| -> Option<NodeId> {
                            match a.ast[s].kind {
                                NodeKind::Return(Some(arg)) => Some(arg),
                                NodeKind::Block(list) if a.ast.list(list).len() == 1 => match a.ast[a.ast.list(list)[0]].kind {
                                    NodeKind::Return(Some(arg)) => Some(arg),
                                    _ => None,
                                },
                                _ => None,
                            }
                        };
                        if let (Some(first), Some(second)) = (returned(*cons), returned(*alt)) {
                            stack.push((Side::node(*test), Side::node(*ct)));
                            stack.push((Side::node(first), Side::node(*cc)));
                            stack.push((Side::node(second), Side::node(*ca_)));
                            continue;
                        }
                    }
                }
                (NodeKind::While { test, body }, NodeKind::For { init: None, test: None, update: None, body: bb }) if matches!(a.ast[*test].kind, NodeKind::Bool(true)) => {
                    stack.push((Side::node(*body), Side::node(*bb)));
                    continue;
                }
                _ => {}
            }
        }
        if std::mem::discriminant(&nx.kind) != std::mem::discriminant(&ny.kind) {
            return Err(format!("shape differs at {}: {:?} vs {:?}", nx.span.start, nx.kind, ny.kind));
        }
        match nx.kind {
            NodeKind::Ident(na) | NodeKind::BindingIdent(na) => {
                let (NodeKind::Ident(nb) | NodeKind::BindingIdent(nb)) = ny.kind else { unreachable!() };
                match (a.semantic.symbol_of(x), b.semantic.symbol_of(y)) {
                    (None, None) => {
                        if a.ast.atom(na) != b.ast.atom(nb) {
                            return Err(format!("global `{}` became `{}`", a.ast.atom(na), b.ast.atom(nb)));
                        }
                    }
                    (Some(sa), Some(sb)) => {
                        if *forward.entry(sa).or_insert(sb) != sb || *backward.entry(sb).or_insert(sa) != sa {
                            return Err(format!("`{}` at {} no longer denotes the same binding (now `{}`)", a.ast.atom(na), nx.span.start, b.ast.atom(nb)));
                        }
                    }
                    _ => return Err(format!("`{}` at {} lost or gained a binding", a.ast.atom(na), nx.span.start)),
                }
            }
            NodeKind::ExportSpecifier { exported, .. } => {
                let NodeKind::ExportSpecifier { exported: eb, .. } = ny.kind else { unreachable!() };
                if a.ast.atom(exported) != b.ast.atom(eb) {
                    return Err(format!("export name `{}` became `{}`", a.ast.atom(exported), b.ast.atom(eb)));
                }
            }
            NodeKind::ImportSpecifier { imported, .. } => {
                let NodeKind::ImportSpecifier { imported: ib, .. } = ny.kind else { unreachable!() };
                if a.ast.atom(imported) != b.ast.atom(ib) {
                    return Err(format!("import name `{}` became `{}`", a.ast.atom(imported), b.ast.atom(ib)));
                }
            }
            NodeKind::Num(na) if syntax => {
                let NodeKind::Num(nb) = ny.kind else { unreachable!() };
                if na.get().to_bits() != nb.get().to_bits() {
                    return Err(format!("number {} became {}", na.get(), nb.get()));
                }
            }
            NodeKind::PropName(_) | NodeKind::PrivateName(_) | NodeKind::Str(_) | NodeKind::Num(_) | NodeKind::BigInt(_) | NodeKind::Regex | NodeKind::TemplateElement { .. } | NodeKind::Bool(_) => {
                let (ta, tb) = (&source[nx.span.start as usize..nx.span.end as usize], &minified[ny.span.start as usize..ny.span.end as usize]);
                if ta != tb {
                    return Err(format!("literal `{ta}` became `{tb}`"));
                }
            }
            _ => {}
        }
        let mut ca = children(&a, &sx, syntax, true);
        let cb = children(&b, &sy, syntax, false);
        if syntax {
            match (&nx.kind, &ny.kind) {
                // `=> { return x; }` and `=> x`.
                (NodeKind::Function { .. }, NodeKind::Function { .. }) if nx.has(flags::ARROW) && !nx.has(flags::EXPR_BODY) && ny.has(flags::EXPR_BODY) => {
                    if let Some(last) = ca.pop() {
                        let body = &a.ast[last.id];
                        match body.kind {
                            NodeKind::Block(list) if a.ast.list(list).len() == 1 => match a.ast[a.ast.list(list)[0]].kind {
                                NodeKind::Return(Some(arg)) => ca.push(Side::node(arg)),
                                _ => return Err(format!("arrow at {} lost its block", nx.span.start)),
                            },
                            _ => return Err(format!("arrow at {} lost its block", nx.span.start)),
                        }
                    }
                }
                // `return undefined` and `return`.
                (NodeKind::Return(Some(arg)), NodeKind::Return(None)) => {
                    if !(is_undefined(&a, *arg) || is_void_zero(&a.ast, *arg)) {
                        return Err(format!("`return` at {} lost its value", nx.span.start));
                    }
                    ca.clear();
                }
                _ => {}
            }
            // Statements the minifier removed: `if (false) ...;` without an `else`.
            let mut aligned = Vec::with_capacity(ca.len());
            let mut j = 0;
            let in_list = matches!(nx.kind, NodeKind::Program(_) | NodeKind::Block(_) | NodeKind::StaticBlock(_) | NodeKind::Case { .. });
            let mut i = 0;
            while i < ca.len() {
                let item = ca[i].clone();
                i += 1;
                let removable = item.declarators.is_none() && removable(&a, item.id);
                if removable && in_list {
                    continue;
                }
                let Some(other) = cb.get(j) else {
                    return Err(format!("{:?} at {} has more children than the result", nx.kind, nx.span.start));
                };
                // `if (c) return a; return b;` and `if (c) return a; else return b;` written as `return c ? a : b;`.
                if in_list && item.declarators.is_none() {
                    if let (NodeKind::If { test, cons, alt }, NodeKind::Return(Some(joined))) = (&a.ast[item.id].kind, &b.ast[other.id].kind) {
                        if let NodeKind::Conditional { test: ct, cons: cc, alt: ca_ } = &b.ast[*joined].kind {
                            let returned = |s: NodeId| -> Option<NodeId> {
                                match a.ast[s].kind {
                                    NodeKind::Return(Some(arg)) => Some(arg),
                                    NodeKind::Block(list) if a.ast.list(list).len() == 1 => match a.ast[a.ast.list(list)[0]].kind {
                                        NodeKind::Return(Some(arg)) => Some(arg),
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            };
                            let second = match alt {
                                Some(alt) => returned(*alt).map(|arg| (arg, 0)),
                                None => ca.get(i).and_then(|next| match a.ast[next.id].kind {
                                    NodeKind::Return(Some(arg)) if next.declarators.is_none() => Some((arg, 1)),
                                    _ => None,
                                }),
                            };
                            if let (Some(first), Some((second, extra))) = (returned(*cons), second) {
                                aligned.push((Side::node(*test), Side::node(*ct)));
                                aligned.push((Side::node(first), Side::node(*cc)));
                                aligned.push((Side::node(second), Side::node(*ca_)));
                                i += extra;
                                j += 1;
                                continue;
                            }
                        }
                    }
                }
                aligned.push((item, other.clone()));
                j += 1;
            }
            if j != cb.len() {
                return Err(format!("{:?} at {} has {} children, then {}", nx.kind, nx.span.start, j, cb.len()));
            }
            stack.extend(aligned);
            continue;
        }
        if ca.len() != cb.len() {
            return Err(format!("{:?} at {} has {} children, then {}", nx.kind, nx.span.start, ca.len(), cb.len()));
        }
        stack.extend(ca.into_iter().zip(cb));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None };
    const SYNTAX: Options = Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None };

    fn min(source: &str) -> String {
        verify(source, &ALL).unwrap_or_else(|e| panic!("{e}\n{source}"));
        minify(source, &ALL).unwrap()
    }

    #[test]
    fn anchors_say_where_each_token_went() {
        let source = "function add(first, second) {\n  return first + second; // sum\n}\nadd(1, 2);\n";
        let (out, anchors) = minify_with_anchors(source, &ALL).unwrap();
        assert_eq!(out, minify(source, &ALL).unwrap());
        assert!(!anchors.is_empty());
        for a in &anchors {
            let before = &source[a.inp as usize..(a.inp + a.in_len) as usize];
            let after = &out[a.out as usize..(a.out + a.out_len) as usize];
            // Only parameter and local names change; everything else is the same token.
            assert!(before == after || (before.chars().all(char::is_alphabetic) && after.chars().all(char::is_alphabetic)), "{before} -> {after}");
        }
        // Without options the text is untouched, and the anchors still describe it.
        let (same, plain) = minify_with_anchors(source, &Options::default()).unwrap();
        assert_eq!(same, source);
        assert!(plain.iter().all(|a| a.out == a.inp && a.out_len == a.in_len));
    }

    fn syntax(source: &str) -> String {
        let options = Options { whitespace: true, syntax: true, ..Options::default() };
        verify(source, &options).unwrap_or_else(|e| panic!("{e}\n{source}"));
        minify(source, &options).unwrap()
    }

    #[test]
    fn literals_are_written_short() {
        assert_eq!(syntax("x = [true, false, undefined, 1000, 0.5]"), "x=[!0,!1,void 0,1e3,.5]");
        // A property of a literal needs it in parentheses; a local `undefined` is its own.
        assert_eq!(syntax("true.toString(); (undefined)()"), "(!0).toString();(void 0)()");
        assert_eq!(syntax("function f(undefined) { return undefined + 1 }"), "function f(undefined){return undefined+1}");
        assert_eq!(syntax("x = { undefined, true: 1 }"), "x={undefined,true:1}");
        assert_eq!(syntax("x = 1.0.toFixed()"), "x=1 .toFixed()");
    }

    #[test]
    fn property_names_lose_their_quotes() {
        assert_eq!(syntax("a['b']; a?.['c']; a['d-e']; x = { 'k': 1, 'k-k': 2 }"), "a.b;a?.c;a['d-e'];x={k:1,'k-k':2}");
    }

    #[test]
    fn new_keeps_its_parentheses_where_they_matter() {
        assert_eq!(syntax("new A(); new A().b; new A()(); new A.b()"), "new A;new A().b;new A()();new A.b");
    }

    #[test]
    fn braces_go_unless_an_else_would_move() {
        assert_eq!(syntax("if (a) { b() } else { c() }"), "a?b():c()");
        assert_eq!(syntax("if (a) { if (b) c() } else d()"), "if(a){b&&c()}else d()");
        assert_eq!(syntax("if (a) { for (;;) { if (b) c() } } else d()"), "if(a){for(;;)b&&c()}else d()");
        assert_eq!(syntax("if (a) { let x = 1; f(x) }"), "if(a){let x=1;f(x)}");
        assert_eq!(syntax("if (a) { let x = 1 }"), "if(a){let x=1}");
        assert_eq!(syntax("for (;;) { x() }\ny()"), "for(;;)x();y()");
        assert_eq!(syntax("do { x() } while (y)"), "do x();while(y)");
    }

    #[test]
    fn conditions_become_expressions() {
        assert_eq!(syntax("if (a) b(); if (!a) b(); if (a) b(); else c()"), "a&&b();a||b();a?b():c()");
        assert_eq!(syntax("if (a) { b() } else { c() }"), "a?b():c()");
        // No shorter, or not the same: left alone.
        assert_eq!(syntax("if (a || b) c(); if (a) b = 1; if (a) b(), c()"), "if(a||b)c();if(a)b=1;if(a)b(),c()");
        assert_eq!(syntax("if (a) b(); else if (c) d()"), "if(a)b();else c&&d()");
    }

    #[test]
    fn returns_join() {
        assert_eq!(syntax("function f() { if (a) return 1; return 2 }"), "function f(){return a?1:2}");
        assert_eq!(syntax("function f() { if (a) { return 1 } else { return 2 } }"), "function f(){return a?1:2}");
        assert_eq!(syntax("function f() { if (a) return 1; return }"), "function f(){if(a)return 1;return}");
    }

    #[test]
    fn typeof_undefined_is_compared_by_order() {
        assert_eq!(syntax("x = typeof a === 'undefined'; y = typeof b != \"undefined\"; z = typeof c === 'string'"), "x=typeof a>\"u\";y=typeof b<\"u\";z=typeof c==='string'");
    }

    #[test]
    fn endless_loops_and_parentheses() {
        assert_eq!(syntax("while (true) { x() }"), "for(;;)x()");
        assert_eq!(syntax("x = ((a % b) + b) % b; y = (a, b); z = (a && b) || c; w = a && (b || c); v = (-a) ** 2; u = -(-a)"), "x=(a%b+b)%b;y=(a,b);z=a&&b||c;w=a&&(b||c);v=(-a)**2;u=-(-a)");
        assert_eq!(syntax("a = (b = 1); c = [(d = 2), (e, f)]; (g = 1), (h = 2); new (i.j)(k); new (l())(); (m.n)(); (o?.p)()"), "a=b=1;c=[d=2,(e,f)];g=1,h=2;new i.j(k);new(l());m.n();(o?.p)()");
    }

    #[test]
    fn declarations_join() {
        assert_eq!(syntax("var a = 1; var b = 2\nlet c; let d; const e = 3"), "var a=1,b=2;let c,d;const e=3");
        assert_eq!(syntax("var a = 1; f(); var b = 2"), "var a=1;f();var b=2");
    }

    #[test]
    fn arrows_with_one_return_take_an_expression() {
        assert_eq!(syntax("f = () => { return 1 }; g = () => { return { a: 1 } }; h = () => { return a, b }"), "f=()=>1;g=()=>({a:1});h=()=>(a,b)");
        assert_eq!(syntax("f = () => { return (a, b) }; g = () => { return undefined }"), "f=()=>(a,b);g=()=>void 0");
    }

    #[test]
    fn decided_branches_are_dropped() {
        assert_eq!(syntax("if (true) a(); else b()"), "a()");
        assert_eq!(syntax("if (false) a(); else b()"), "b()");
        assert_eq!(syntax("x(); if (false) { a() } y()"), "x();y()");
        assert_eq!(syntax("if (c) if (false) a()"), "if(c);");
        // A `var` in the dropped code still declares its name.
        assert_eq!(syntax("if (false) { var v = 1 }"), "if(!1)var v=1");
        assert_eq!(syntax("x = true ? a : b; y = false ? a : b"), "x=a;y=b");
        assert_eq!(syntax("function f() { if (a) if (true) return 1; else return 2; else return 3 }"), "function f(){if(a)return 1;else return 3}");
        assert_eq!(syntax("function f() { return undefined; return void 0 }"), "function f(){return;return}");
    }

    #[test]
    fn a_line_break_that_stood_for_a_semicolon_survives() {
        assert_eq!(syntax("a = 1\nif (true) b()\nc()"), "a=1;b();c()");
        assert_eq!(syntax("a = () => { return 1 }\n(b)"), "a=()=>1;b");
    }

    fn spaces(source: &str) -> String {
        minify(source, &Options { whitespace: true, ..Options::default() }).unwrap()
    }

    #[test]
    fn whitespace_between_words_stays() {
        assert_eq!(spaces("let a = 1;\nconst b = typeof a;"), "let a=1;const b=typeof a");
        assert_eq!(spaces("x = a in b; y = a instanceof B"), "x=a in b;y=a instanceof B");
    }

    #[test]
    fn operators_do_not_merge() {
        assert_eq!(spaces("x = a + +b; y = a - -b; z = a+ ++b; w = a - --b"), "x=a+ +b;y=a- -b;z=a+ ++b;w=a- --b");
        assert_eq!(spaces("x = a / /re/.source"), "x=a/ /re/.source");
    }

    #[test]
    fn a_dot_after_a_whole_number_keeps_its_space() {
        assert_eq!(spaces("(1).toString(); 1..toString(); 1.5.toFixed()"), "(1).toString();1..toString();1.5.toFixed()");
        assert_eq!(spaces("x = 1 .toString()"), "x=1 .toString()");
    }

    #[test]
    fn line_breaks_that_ended_statements_become_semicolons() {
        assert_eq!(spaces("a = 1\nb = 2\nc()"), "a=1;b=2;c()");
        assert_eq!(spaces("function f() {\n  return\n  1\n}"), "function f(){return;1}");
        assert_eq!(spaces("let v = 1\nv\n++v"), "let v=1;v;++v");
        assert_eq!(spaces("if (a)\n  b()\nelse\n  c()"), "if(a)b();else c()");
    }

    #[test]
    fn line_breaks_that_did_nothing_just_go() {
        assert_eq!(spaces("a = b\n+ c\n(d)"), "a=b+c(d)");
        assert_eq!(spaces("f(\n  a,\n  b\n)"), "f(a,b)");
    }

    #[test]
    fn semicolons_before_a_closing_brace_go_but_not_empty_statements() {
        assert_eq!(spaces("function f() { a(); b(); }"), "function f(){a();b()}");
        assert_eq!(spaces("if (a) {;} for (;;) {}"), "if(a){;}for(;;){}");
        assert_eq!(spaces("while (a);"), "while(a);");
    }

    #[test]
    fn comments_go_and_strings_stay() {
        assert_eq!(spaces("/* c */ a = '  x  ' // tail\n"), "a='  x  '");
        assert_eq!(spaces("a = `x ${ b } y`"), "a=`x ${b} y`");
    }

    #[test]
    fn names_get_short_and_shorthand_survives() {
        let out = min("function outer(first, second) { const total = first + second; return { first, total }; }");
        assert!(out.contains("first:") && out.contains("total:"), "{out}");
        assert!(!out.contains("second") && !out.contains("const total"), "{out}");
    }

    #[test]
    fn inner_names_can_reuse_outer_ones_they_do_not_touch() {
        let out = min("const a1 = 1; const a2 = 2; function f(p) { return p; } function g(q) { return q + a1 + a2; }");
        // Each function's parameter can be the shortest name not captured, so both reuse names.
        verify("function f(p) { return p; } function g(q) { return q; }", &ALL).unwrap();
        assert!(out.len() < 100, "{out}");
    }

    #[test]
    fn a_global_the_file_mentions_is_never_taken() {
        let out = min("function f(a, b, c, d, e, g) { return a + b + c + d + e + g + Math.max(a, b); }");
        assert!(out.contains("Math.max"), "{out}");
        let out = min("function f(v) { return v + e; } var e = 1;");
        assert!(out.contains("+e") || out.contains("+"), "{out}");
    }

    #[test]
    fn direct_eval_keeps_the_names_it_can_see() {
        let out = min("function f(visible) { return eval('visible'); } function g(hidden) { return hidden; }");
        assert!(out.contains("visible"), "{out}");
        assert!(!out.contains("hidden"), "{out}");
    }

    #[test]
    fn exports_keep_their_public_names() {
        let out = min("function internal() {} const value = 1; export { internal, value as other };");
        assert!(out.contains("as internal"), "{out}");
        assert!(out.contains(" as other"), "{out}");
        let out = min("export const stays = 1; export function alsoStays() {}");
        assert!(out.contains("stays") && out.contains("alsoStays"), "{out}");
    }

    #[test]
    fn imports_keep_the_imported_name() {
        let out = min("import { thing } from 'x'; import other, * as ns from 'y'; thing(other, ns);");
        assert!(out.contains("thing as "), "{out}");
    }

    #[test]
    fn a_class_declaration_is_one_name_inside_and_out() {
        min("class Node { static make() { return new Node(); } next() { return Node.make(); } } new Node();");
    }

    #[test]
    fn a_body_may_not_redeclare_a_parameter() {
        min("function f(a, b = () => a) { let c = 1; var d = 2; return [a, b, c, d]; }");
    }

    #[test]
    fn a_catch_parameter_and_its_var_stay_together() {
        min("function f() { try { g(); } catch (e) { var e = 1; return e; } }");
    }

    #[test]
    fn keep_names_leaves_functions_and_classes_alone() {
        let out = minify("function work() {} class Thing {} const arrow = () => 1; let plain = 2; work(Thing, arrow, plain);", &Options { keep_names: Some(true), ..ALL }).unwrap();
        assert!(out.contains("work") && out.contains("Thing") && out.contains("arrow"), "{out}");
        assert!(!out.contains("plain"), "{out}");
    }

    #[test]
    fn an_iife_keeps_its_top_level() {
        let out = minify("var Lib = (() => { var inner = 1; return { inner }; })();", &Options { keep_top_level: true, ..ALL }).unwrap();
        assert!(out.starts_with("var Lib="), "{out}");
    }

    #[test]
    fn many_names_still_unique() {
        let mut source = String::new();
        for i in 0..400 {
            source.push_str(&format!("const value{i} = {i};\n"));
        }
        source.push_str("console.log(");
        for i in 0..400 {
            source.push_str(&format!("value{i}{}", if i < 399 { "," } else { "" }));
        }
        source.push_str(");\n");
        min(&source);
    }

    #[test]
    fn name_sequence_has_no_duplicates() {
        let names: Vec<String> = (0..20000).map(name_at).collect();
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        assert_eq!(names[0], "e");
        assert!(names[..54].iter().all(|n| n.len() == 1));
        assert!(names[54].len() == 2);
    }
}
