//! `let`, `const` and block-level declarations as es5 `var`s: each name is hoisted to its function, so a name that another declaration or
//! reference in the function would meet is given another, and one that a function inside a loop captures (one binding for each turn of the
//! loop) is refused, as there is no es5 form of it that this program writes yet.
use super::*;
use crate::ast::VarKind;
use crate::semantic::ScopeId;
use crate::target::Feature;

#[derive(Default)]
pub(super) struct Blocks {
    /// The names given to block-scoped bindings and the references to them.
    renames: HashMap<NodeId, String>,
    /// Declarations in a loop, whose names without a value start as `undefined` in every turn.
    loop_decls: HashSet<NodeId>,
    /// Function declarations in a block, which es5 has no place for in strict code.
    block_functions: HashSet<NodeId>,
    /// Declarations that are the head of a `for in` or `for of`.
    heads: HashSet<NodeId>,
    /// The blocks that are the body of a function.
    function_bodies: HashSet<NodeId>,
    /// The loops whose body is a function in es5, because a function inside it captures a binding that is new in each turn, with the
    /// names declared in the head of the loop (which the function takes as parameters).
    pub(super) wrapped: HashMap<NodeId, Vec<NodeId>>,
    /// The label of the loops that have one.
    labels: HashMap<NodeId, crate::ast::Atom>,
}

struct Seen {
    node: NodeId,
    symbol: Option<SymbolId>,
    fn_depth: u32,
    /// The innermost loop (of the same function) the identifier is in.
    loop_node: Option<NodeId>,
    in_head: bool,
}

impl Scanner<'_, '_> {
    pub(super) fn blocks(&mut self) -> &Blocks {
        if self.es5.blocks.is_none() {
            let blocks = self.analyze_blocks();
            self.es5.blocks = Some(blocks);
        }
        self.es5.blocks.as_ref().unwrap()
    }

    fn analyze_blocks(&mut self) -> Blocks {
        let ast = self.ast;
        let mut blocks = Blocks::default();
        let mut seen: Vec<Seen> = Vec::new();
        self.visit_blocks(ast.root, 0, &mut Vec::new(), false, &mut blocks, &mut seen);
        let semantic = self.semantic;
        let mut by_name: HashMap<crate::ast::Atom, Vec<usize>> = HashMap::new();
        let index: HashMap<NodeId, usize> = seen.iter().enumerate().map(|(i, s)| (s.node, i)).collect();
        for (i, s) in seen.iter().enumerate() {
            let atom = match ast[s.node].kind {
                NodeKind::Ident(a) | NodeKind::BindingIdent(a) => a,
                _ => continue,
            };
            by_name.entry(atom).or_default().push(i);
        }
        let mut errors: Vec<(NodeId, String)> = Vec::new();
        let mut taken = 0u32;
        for (id, symbol) in semantic.symbols() {
            if !matches!(symbol.kind, SymbolKind::Let | SymbolKind::Const | SymbolKind::Class) {
                continue;
            }
            let scope = semantic.scope(symbol.scope);
            if scope.kind.is_var_scope() || matches!(scope.kind, ScopeKind::Class) {
                continue;
            }
            let Some(decl) = symbol.decl else { continue };
            let Some(&first) = index.get(&decl) else { continue };
            let first = &seen[first];
            let (decl_depth, decl_loop, in_head) = (first.fn_depth, first.loop_node, first.in_head);
            let name = ast.atom(symbol.name).to_string();
            let var_scope = self.var_scope_of(symbol.scope);
            let (va, vb) = self.scope_span(var_scope);
            let (ba, bb) = self.scope_span(symbol.scope);
            let all = by_name.get(&symbol.name).map(|v| v.as_slice()).unwrap_or(&[]);
            // A function inside a loop that reads or writes a binding of the loop: one binding for each turn.
            if let Some(loop_node) = decl_loop.filter(|_| all.iter().any(|&i| seen[i].symbol == Some(id) && seen[i].fn_depth > decl_depth)) {
                let entry = blocks.wrapped.entry(loop_node).or_default();
                if in_head {
                    entry.push(decl);
                    // The loop's head keeps its own copy, so a write in the body would be lost.
                    let (la, lb) = self.span(loop_node);
                    let body_from = match ast[loop_node].kind {
                        NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } => ast[body].span.start,
                        _ => la,
                    };
                    if all.iter().any(|&i| seen[i].symbol == Some(id) && ast[seen[i].node].has(crate::ast::flags::ASSIGNED) && ast[seen[i].node].span.start >= body_from && ast[seen[i].node].span.end <= lb) {
                        errors.push((decl, format!("the variable '{name}' of a loop is changed in the loop's body and read by a function there, which needs a binding for each turn of the loop; Rathkit Pack cannot rewrite that for es5 yet")));
                    }
                }
            }
            let at_top = semantic.scope(var_scope).kind == ScopeKind::Module && !self.in_wrapper;
            let conflict = all.iter().any(|&i| {
                let s = &seen[i];
                let at = ast[s.node].span.start;
                s.symbol != Some(id) && at >= va && at < vb && !(at >= ba && at < bb)
            });
            if !(at_top || conflict) {
                continue;
            }
            taken += 1;
            let new = format!("{name}${}", self.bundler.temp_count + taken);
            for &i in all {
                if seen[i].symbol == Some(id) {
                    blocks.renames.insert(seen[i].node, new.clone());
                }
            }
        }
        self.bundler.temp_count += taken + 1;
        for (node, message) in errors {
            self.lower_error_message(node, message);
        }
        blocks
    }

    fn var_scope_of(&self, mut scope: ScopeId) -> ScopeId {
        let semantic = self.semantic;
        loop {
            let s = semantic.scope(scope);
            if s.kind.is_var_scope() {
                return scope;
            }
            match s.parent {
                Some(p) => scope = p,
                None => return scope,
            }
        }
    }

    fn scope_span(&self, scope: ScopeId) -> (u32, u32) {
        match self.semantic.scope(scope).node {
            Some(n) => self.span(n),
            None => (0, self.module.source.len() as u32),
        }
    }

    fn visit_blocks(&self, node: NodeId, fn_depth: u32, loops: &mut Vec<NodeId>, head: bool, blocks: &mut Blocks, seen: &mut Vec<Seen>) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Ident(_) | NodeKind::BindingIdent(_) => {
                seen.push(Seen { node, symbol: self.semantic.symbol_of(node), fn_depth, loop_node: loops.last().copied(), in_head: head });
                return;
            }
            NodeKind::Function { body, .. } => {
                blocks.function_bodies.insert(body);
                let children = self.children(node);
                let mut inner = Vec::new();
                for c in children {
                    self.visit_blocks(c, fn_depth + 1, &mut inner, false, blocks, seen);
                }
                return;
            }
            NodeKind::Labeled { label, body } => {
                if matches!(ast[body].kind, NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. }) {
                    blocks.labels.insert(body, label);
                }
            }
            NodeKind::VarDecl { .. } => {
                if !loops.is_empty() && !head {
                    blocks.loop_decls.insert(node);
                }
                if head {
                    blocks.heads.insert(node);
                }
            }
            NodeKind::Block(list) | NodeKind::StaticBlock(list) => {
                if matches!(ast[node].kind, NodeKind::Block(_)) {
                    for &s in ast.list(list) {
                        if matches!(ast[s].kind, NodeKind::Function { .. }) && ast[s].has(crate::ast::flags::DECL) && !blocks.function_bodies.contains(&node) {
                            blocks.block_functions.insert(s);
                        }
                    }
                }
            }
            NodeKind::Case { body, .. } => {
                for &s in ast.list(body) {
                    if matches!(ast[s].kind, NodeKind::Function { .. }) && ast[s].has(crate::ast::flags::DECL) {
                        blocks.block_functions.insert(s);
                    }
                }
            }
            NodeKind::For { init, test, update, body } => {
                loops.push(node);
                if let Some(i) = init {
                    self.visit_blocks(i, fn_depth, loops, true, blocks, seen);
                }
                for c in [test, update].into_iter().flatten() {
                    self.visit_blocks(c, fn_depth, loops, false, blocks, seen);
                }
                self.visit_blocks(body, fn_depth, loops, false, blocks, seen);
                loops.pop();
                return;
            }
            NodeKind::ForIn { left, right, body } | NodeKind::ForOf { left, right, body } => {
                loops.push(node);
                self.visit_blocks(left, fn_depth, loops, true, blocks, seen);
                self.visit_blocks(right, fn_depth, loops, false, blocks, seen);
                self.visit_blocks(body, fn_depth, loops, false, blocks, seen);
                loops.pop();
                return;
            }
            NodeKind::While { .. } | NodeKind::DoWhile { .. } => {
                loops.push(node);
                for c in self.children(node) {
                    self.visit_blocks(c, fn_depth, loops, false, blocks, seen);
                }
                loops.pop();
                return;
            }
            _ => {}
        }
        for c in self.children(node) {
            self.visit_blocks(c, fn_depth, loops, head, blocks, seen);
        }
    }

    fn children(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.ast.for_each_child(node, |c| out.push(c));
        out
    }

    pub(super) fn lower_block_scoped(&mut self, node: NodeId) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Ident(_) | NodeKind::BindingIdent(_) => {
                if let Some(name) = self.blocks().renames.get(&node).cloned() {
                    self.push_ident_edit(node, vec![Piece::Text(name)]);
                }
            }
            NodeKind::VarDecl { kind, decls } if kind != VarKind::Var && self.lowers(Feature::LetConst) => {
                let (start, _) = self.span(node);
                let length = if kind == VarKind::Let { 3 } else { 5 };
                self.edits.push(Edit { start, end: start + length, pieces: vec![Piece::Text("var".to_string())], shorthand: None, transparent: false });
                // A name with no value starts as `undefined` in every turn of a loop.
                if self.blocks().loop_decls.contains(&node) && kind == VarKind::Let {
                    for &d in ast.list(decls) {
                        if let NodeKind::VarDeclarator { target, init: None } = ast[d].kind {
                            if matches!(ast[target].kind, NodeKind::BindingIdent(_)) {
                                let end = ast[target].span.end;
                                self.insert(end, vec![Piece::Text(" = void 0".to_string())]);
                            }
                        }
                    }
                }
            }
            NodeKind::Function { .. } if ast[node].has(crate::ast::flags::DECL) && self.lowers(Feature::LetConst) && self.blocks().block_functions.contains(&node) => {
                let NodeKind::Function { name: Some(name), .. } = ast[node].kind else { return };
                let (start, end) = self.span(node);
                let mut head = vec![Piece::Text("var ".to_string())];
                head.extend(self.rng(name));
                head.push(Piece::Text(" = ".to_string()));
                self.insert(start, head);
                self.insert(end, vec![Piece::Text(";".to_string())]);
            }
            _ => {}
        }
    }

    /// The body of a loop that captures a binding that is new in each turn, as a function called in each turn: the names of the loop's head are
    /// its parameters, `break` and `return` become what the function returns and the loop acts on it afterwards.
    /// `None` when the loop needs none (or the body cannot be rewritten, which is an error).
    pub(super) fn loop_wrapper(&mut self, lp: NodeId, body: NodeId) -> Option<Vec<Piece>> {
        let ast = self.ast;
        let decls = self.blocks().wrapped.get(&lp).cloned()?;
        let mut params: Vec<String> = Vec::new();
        for d in decls {
            let name = match self.blocks().renames.get(&d).cloned() {
                Some(renamed) => renamed,
                None => match ast[d].kind {
                    NodeKind::BindingIdent(a) | NodeKind::Ident(a) => ast.atom(a).to_string(),
                    _ => continue,
                },
            };
            if !params.contains(&name) {
                params.push(name);
            }
        }
        let label = self.blocks().labels.get(&lp).copied();
        if self.mentions_arguments(body) {
            self.refuse(Feature::LetConst, body, "and a function for the body of a loop cannot be made where the body reads `arguments`");
            return None;
        }
        let mut exits = Exits::default();
        self.scan_exits(body, label, false, false, &mut exits);
        if exits.refused {
            return None;
        }
        let o = self.local("o");
        let (ba, bb) = self.span(body);
        let this = if self.lowers(Feature::Arrow) && self.es5_in_arrow() || self.es5_top_function().is_some_and(|(_, derived)| derived) { "_$this" } else { "this" };
        let nested = self.es5.active_wraps.iter().any(|&(a, b, depth)| a <= ba && bb <= b && depth == self.es5_depth());
        self.es5.active_wraps.push((ba, bb, self.es5_depth()));
        let mut pieces = vec![Piece::Text(format!("{{ {}(function ({}) {{", if exits.returns || exits.breaks { format!("var {o} = ") } else { String::new() }, params.join(", ")))];
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            pieces.push(Piece::Range(ba + 1, bb));
        } else {
            pieces.push(Piece::Range(ba, bb));
            pieces.push(Piece::Text("}".to_string()));
        }
        let arguments = if params.is_empty() { String::new() } else { format!(", {}", params.join(", ")) };
        pieces.push(Piece::Text(format!(").call({this}{arguments});")));
        let propagate = if nested { format!("return {{ v: {o}.v }};") } else { format!("return {o}.v;") };
        match (exits.returns, exits.breaks) {
            (true, true) => pieces.push(Piece::Text(format!(" if ({o}) {{ if ({o} === 1) break; {propagate} }}"))),
            (true, false) => pieces.push(Piece::Text(format!(" if ({o}) {propagate}"))),
            (false, true) => pieces.push(Piece::Text(format!(" if ({o} === 1) break;"))),
            (false, false) => {}
        }
        pieces.push(Piece::Text(" }".to_string()));
        Some(pieces)
    }

    fn mentions_arguments(&self, node: NodeId) -> bool {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Ident(a) if ast.atom(a) == "arguments" => true,
            NodeKind::Function { .. } if !ast[node].has(crate::ast::flags::ARROW) => false,
            _ => {
                let mut found = false;
                ast.for_each_child(node, |c| found = found || self.mentions_arguments(c));
                found
            }
        }
    }

    /// Rewrites the `return`, `break` and `continue` of a loop's body that leave it, into what the function for the body returns.
    fn scan_exits(&mut self, node: NodeId, label: Option<crate::ast::Atom>, in_loop: bool, in_switch: bool, exits: &mut Exits) {
        let ast = self.ast;
        let text = |s: &str| Piece::Text(s.to_string());
        match ast[node].kind {
            NodeKind::Function { .. } => {}
            NodeKind::Return(arg) => {
                exits.returns = true;
                let mut pieces = vec![text("return { v: ")];
                match arg {
                    Some(a) => pieces.extend(self.rng(a)),
                    None => pieces.push(text("void 0")),
                }
                pieces.push(text(" };"));
                if !self.es5.exit_edits.insert(node) {
                    return;
                }
                self.push(node, pieces);
            }
            NodeKind::Break(target) => {
                let ours = match target {
                    None => !in_loop && !in_switch,
                    Some(l) => Some(l) == label,
                };
                if ours {
                    exits.breaks = true;
                    if self.es5.exit_edits.insert(node) {
                        self.push(node, vec![text("return 1;")]);
                    }
                } else if let Some(l) = target {
                    if !exits.inner_labels.contains(&l) {
                        exits.refused = true;
                        self.refuse(Feature::LetConst, node, "and a break out to a label outside a loop whose body needs a function cannot be rewritten for it yet");
                    }
                }
            }
            NodeKind::Continue(target) => {
                let ours = match target {
                    None => !in_loop,
                    Some(l) => Some(l) == label,
                };
                if ours {
                    if self.es5.exit_edits.insert(node) {
                        self.push(node, vec![text("return;")]);
                    }
                } else if let Some(l) = target {
                    if !exits.inner_labels.contains(&l) {
                        exits.refused = true;
                        self.refuse(Feature::LetConst, node, "and a continue to a label outside a loop whose body needs a function cannot be rewritten for it yet");
                    }
                }
            }
            NodeKind::Labeled { label: l, body } => {
                exits.inner_labels.push(l);
                self.scan_exits(body, label, in_loop, in_switch, exits);
            }
            NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } => {
                for c in self.children(node) {
                    self.scan_exits(c, label, true, in_switch, exits);
                }
            }
            NodeKind::Switch { .. } => {
                for c in self.children(node) {
                    self.scan_exits(c, label, in_loop, true, exits);
                }
            }
            _ => {
                for c in self.children(node) {
                    self.scan_exits(c, label, in_loop, in_switch, exits);
                }
            }
        }
    }
}

#[derive(Default)]
struct Exits {
    returns: bool,
    breaks: bool,
    refused: bool,
    inner_labels: Vec<crate::ast::Atom>,
}
