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
    pub(super) renames: HashMap<NodeId, String>,
    /// Declarations in a loop, whose names without a value start as `undefined` in every turn.
    loop_decls: HashSet<NodeId>,
    /// Function declarations in a block, which es5 has no place for in strict code.
    block_functions: HashSet<NodeId>,
    /// Declarations that are the head of a `for in` or `for of`.
    pub(super) heads: HashSet<NodeId>,
    /// Declarations that are the first part of a `for (;;)`.
    pub(super) for_inits: HashSet<NodeId>,
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
        // A function declared in a block stays where it is, as a `var` with a function: it is not there before its line.
        for &f in &blocks.block_functions {
            let NodeKind::Function { name: Some(n), .. } = ast[f].kind else { continue };
            let Some(sym) = semantic.symbol_of(n) else { continue };
            let Some(&at) = index.get(&n) else { continue };
            let (fa, _) = self.span(f);
            // The name is inside the function node, one deeper than where the declaration is.
            let depth = seen[at].fn_depth.saturating_sub(1);
            let symbol_name = semantic.symbol(sym).name;
            let (ba, _) = self.scope_span(semantic.symbol(sym).scope);
            for &i in by_name.get(&symbol_name).map(|v| v.as_slice()).unwrap_or(&[]) {
                let r = &seen[i];
                let start = ast[r.node].span.start;
                if (r.symbol == Some(sym) || (r.symbol.is_some() && r.symbol == semantic.symbol(sym).alias)) && r.fn_depth == depth && start < fa && start >= ba && r.node != n {
                    errors.push((r.node, format!("the function '{}' is used before its declaration in the block, which es5 does not hoist; Rathkit Pack cannot rewrite that yet", ast.atom(symbol_name))));
                    break;
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
                    if matches!(ast[i].kind, NodeKind::VarDecl { .. }) {
                        blocks.for_inits.insert(i);
                    }
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
                if matches!(ast[left].kind, NodeKind::VarDecl { .. }) {
                    blocks.heads.insert(left);
                }
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

    pub(super) fn children(&self, node: NodeId) -> Vec<NodeId> {
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
        self.scan_exits(body, label, false, false, true, &mut exits);
        if exits.refused {
            return None;
        }
        let o = self.local("o");
        let (ba, bb) = self.span(body);
        let this = if self.arrows_rewritten() && self.es5_in_arrow() || self.es5_top_function().is_some_and(|(_, derived)| derived) { "_$this" } else { "this" };
        let enclosing = self.es5.active_wraps.iter().rev().find(|w| w.start <= ba && bb <= w.end && w.depth == self.es5_depth()).cloned();
        let nested = enclosing.is_some();
        self.es5.active_wraps.push(super::lower_es5::Wrap { start: ba, end: bb, depth: self.es5_depth(), label, inside: exits.inner_labels.clone(), codes: exits.outer.clone() });
        // Jumps to the labels of loops outside this one.
        let mut after = String::new();
        for &(is_continue, l, code) in &exits.outer {
            let name = self.ast.atom(l).to_string();
            let inside_enclosing = enclosing.as_ref().map_or(true, |w| w.inside.contains(&l));
            let action = if inside_enclosing {
                format!("{} {name};", if is_continue { "continue" } else { "break" })
            } else if enclosing.as_ref().is_some_and(|w| w.label == Some(l)) {
                if is_continue { "return;".to_string() } else { "return 1;".to_string() }
            } else if let Some(&(_, _, outer_code)) = enclosing.as_ref().and_then(|w| w.codes.iter().find(|&&(k, lab, _)| k == is_continue && lab == l)) {
                format!("return {outer_code};")
            } else {
                self.refuse(Feature::LetConst, body, "and a break or continue to a label out of a loop body that is a function cannot be rewritten for es5 yet");
                return None;
            };
            after.push_str(&format!(" if ({o} === {code}) {action}"));
        }
        let mut pieces = vec![Piece::Text(format!("{{ {}(function ({}) {{", if exits.returns || exits.breaks || !exits.outer.is_empty() { format!("var {o} = ") } else { String::new() }, params.join(", ")))];
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            pieces.push(Piece::Range(ba + 1, bb));
        } else {
            pieces.push(Piece::Range(ba, bb));
            pieces.push(Piece::Text("}".to_string()));
        }
        let arguments = if params.is_empty() { String::new() } else { format!(", {}", params.join(", ")) };
        pieces.push(Piece::Text(format!(").call({this}{arguments});")));
        let propagate = if nested { format!("return {{ v: {o}.v }};") } else { format!("return {o}.v;") };
        if exits.breaks {
            pieces.push(Piece::Text(format!(" if ({o} === 1) break;")));
        }
        if exits.returns {
            pieces.push(Piece::Text(format!(" if (typeof {o} == \"object\") {propagate}")));
        }
        pieces.push(Piece::Text(after));
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
    fn scan_exits(&mut self, node: NodeId, label: Option<crate::ast::Atom>, in_loop: bool, in_switch: bool, edit: bool, exits: &mut Exits) {
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
                if edit && self.es5.exit_edits.insert(node) {
                    self.push(node, pieces);
                }
            }
            NodeKind::Break(target) => {
                let ours = match target {
                    None => !in_loop && !in_switch,
                    Some(l) => Some(l) == label,
                };
                if ours {
                    exits.breaks = true;
                    if edit && self.es5.exit_edits.insert(node) {
                        self.push(node, vec![text("return 1;")]);
                    }
                } else if let Some(l) = target {
                    if !exits.inner_labels.contains(&l) {
                        let code = exits.code(false, l);
                        if edit && self.es5.exit_edits.insert(node) {
                            self.push(node, vec![Piece::Text(format!("return {code};"))]);
                        }
                    }
                }
            }
            NodeKind::Continue(target) => {
                let ours = match target {
                    None => !in_loop,
                    Some(l) => Some(l) == label,
                };
                if ours {
                    if edit && self.es5.exit_edits.insert(node) {
                        self.push(node, vec![text("return;")]);
                    }
                } else if let Some(l) = target {
                    if !exits.inner_labels.contains(&l) {
                        let code = exits.code(true, l);
                        if edit && self.es5.exit_edits.insert(node) {
                            self.push(node, vec![Piece::Text(format!("return {code};"))]);
                        }
                    }
                }
            }
            NodeKind::Labeled { label: l, body } => {
                exits.inner_labels.push(l);
                self.scan_exits(body, label, in_loop, in_switch, edit, exits);
            }
            NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } => {
                // A loop that is a function of its own takes care of its exits (and tells this one what it must do).
                let own_wrap = self.blocks().wrapped.contains_key(&node);
                for c in self.children(node) {
                    self.scan_exits(c, label, true, in_switch, edit && !own_wrap, exits);
                }
            }
            NodeKind::Switch { .. } => {
                for c in self.children(node) {
                    self.scan_exits(c, label, in_loop, true, edit, exits);
                }
            }
            _ => {
                for c in self.children(node) {
                    self.scan_exits(c, label, in_loop, in_switch, edit, exits);
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
    /// Jumps to a label outside: (is a continue, the label, the code the function returns for it).
    outer: Vec<(bool, crate::ast::Atom, u32)>,
}

impl Exits {
    fn code(&mut self, is_continue: bool, label: crate::ast::Atom) -> u32 {
        if let Some(&(_, _, c)) = self.outer.iter().find(|&&(k, l, _)| k == is_continue && l == label) {
            return c;
        }
        let code = 3 + self.outer.len() as u32;
        self.outer.push((is_continue, label, code));
        code
    }
}
