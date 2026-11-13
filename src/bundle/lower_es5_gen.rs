//! Generators, async functions, async generators and `for await` as es5: the body becomes a state machine, a `switch` on a label that
//! `__generator` (see `es5_helpers.rs`) runs. A `yield` or `await` ends a case (`return [4, value]`) and the next case takes up what is
//! after it, with the value as `_$s.sent()`; loops, conditions and `try` that have a `yield` inside are written with jumps between
//! cases (`return [3, label]`) and, for `try`, an entry in `_$s.trys`. What has no `yield` and no jump out of it is kept as it is, with the
//! `return`s in it rewritten and its `var`s turned into assignments (they are declared outside the state machine, in the function).
//! An `async` function is such a generator run by `__async`; an async generator is run by `__asyncGenerator`, an `await` in it being
//! `yield new __await(value)`.
use super::*;
use crate::ast::{Atom, BinOp, PropKind, UnaryOp, VarKind};
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum GenKind {
    Generator,
    Async,
    AsyncGenerator,
}

struct Jump {
    labels: Vec<Atom>,
    brk: Option<usize>,
    cont: Option<usize>,
}

struct Gen {
    kind: GenKind,
    cases: Vec<(usize, Vec<Piece>)>,
    syms: usize,
    vars: Vec<String>,
    jumps: Vec<Jump>,
    pending: Vec<Atom>,
    yields: HashMap<NodeId, bool>,
    hoisted: Vec<Piece>,
    failed: bool,
}

impl Gen {
    fn new(kind: GenKind) -> Gen {
        Gen { kind, cases: vec![(0, Vec::new())], syms: 1, vars: Vec::new(), jumps: Vec::new(), pending: Vec::new(), yields: HashMap::new(), hoisted: Vec::new(), failed: false }
    }

    fn emit(&mut self, pieces: Vec<Piece>) {
        self.cases.last_mut().unwrap().1.extend(pieces);
    }

    fn say(&mut self, s: &str) {
        self.cases.last_mut().unwrap().1.push(text(s));
    }

    fn sym(&mut self) -> usize {
        self.syms += 1;
        self.syms - 1
    }

    /// A reference to a label that is given its number once all cases are known.
    fn at(sym: usize) -> Piece {
        Piece::Text(format!("\u{1}{sym}\u{2}"))
    }

    /// Starts the case `sym`, which what comes before falls into.
    fn mark(&mut self, sym: usize) {
        self.emit(vec![text("_$s.label = "), Gen::at(sym), text(";")]);
        self.cases.push((sym, Vec::new()));
    }

    /// A case that follows a yield: the machine counts the label up by itself.
    fn next_case(&mut self) -> usize {
        let sym = self.sym();
        self.cases.push((sym, Vec::new()));
        sym
    }

    fn declare(&mut self, name: String) {
        if !self.vars.contains(&name) {
            self.vars.push(name);
        }
    }

    fn temp(&mut self, name: String) -> String {
        self.declare(name.clone());
        name
    }
}

impl Scanner<'_, '_> {
    /// Whether this function is written as a state machine: a generator, an async function or an async generator that the target lacks.
    pub(super) fn gen_kind(&self, f: u8) -> Option<GenKind> {
        use crate::ast::flags;
        let generator = f & flags::GENERATOR != 0;
        let asynchronous = f & flags::ASYNC != 0;
        match (asynchronous, generator) {
            (false, true) if self.lowers(Feature::Generator) => Some(GenKind::Generator),
            (true, false) if self.lowers(Feature::Generator) => Some(GenKind::Async),
            (true, true) if self.lowers(Feature::Generator) => Some(GenKind::AsyncGenerator),
            _ => None,
        }
    }

    /// The text that goes inside the braces of the function: the declarations and the call that runs the state machine. `None` after an
    /// error. The body is a block, or (an arrow) an expression.
    pub(super) fn gen_body(&mut self, function: NodeId, body: NodeId, kind: GenKind) -> Option<Vec<Piece>> {
        let ast = self.ast;
        let mut g = Gen::new(kind);
        let expression_body = !matches!(ast[body].kind, NodeKind::Block(_));
        self.bundler.helpers.es5_generator = true;
        match kind {
            GenKind::Async => self.bundler.helpers.async_fn = true,
            GenKind::AsyncGenerator => {
                self.bundler.helpers.async_gen = true;
                self.bundler.helpers.es5_iter = true;
            }
            GenKind::Generator => {}
        }
        if expression_body {
            let value = self.gx(&mut g, body);
            let mut p = vec![text("return [2, ")];
            p.extend(value);
            p.push(text("];"));
            g.emit(p);
        } else if let NodeKind::Block(list) = ast[body].kind {
            for &s in ast.list(list) {
                self.gst(&mut g, s);
            }
        }
        if g.failed {
            return None;
        }
        g.say("return [2];");
        // Case numbers.
        let numbers: HashMap<usize, usize> = g.cases.iter().enumerate().map(|(i, (s, _))| (*s, i)).collect();
        let resolve = |t: &str| -> String {
            let mut out = String::new();
            let mut rest = t;
            while let Some(i) = rest.find('\u{1}') {
                out.push_str(&rest[..i]);
                let after = &rest[i + 1..];
                let j = after.find('\u{2}').unwrap_or(0);
                let sym: usize = after[..j].parse().unwrap_or(0);
                out.push_str(&numbers.get(&sym).copied().unwrap_or(0).to_string());
                rest = &after[j + 1..];
            }
            out.push_str(rest);
            out
        };
        let mut machine: Vec<Piece> = vec![text("return __generator(this, function (_$s) { switch (_$s.label) {")];
        for (i, (_, code)) in g.cases.iter().enumerate() {
            machine.push(Piece::Text(format!(" case {i}: ")));
            for p in code {
                match p {
                    Piece::Text(t) if t.contains('\u{1}') => machine.push(Piece::Text(resolve(t))),
                    other => machine.push(other.clone()),
                }
            }
        }
        machine.push(text(" } });"));
        let mut out: Vec<Piece> = Vec::new();
        if self.mentions_arguments_deep(body) {
            out.push(text(" var _$args = arguments;"));
        }
        let mut wrapper_vars = String::new();
        if !g.vars.is_empty() {
            wrapper_vars = format!(" var {};", g.vars.join(", "));
        }
        let this = if ast[function].has(crate::ast::flags::ARROW) { "void 0" } else { "this" };
        match kind {
            GenKind::Generator => {
                out.push(Piece::Text(wrapper_vars));
                out.extend(g.hoisted);
                out.extend(machine);
            }
            GenKind::Async | GenKind::AsyncGenerator => {
                let runner = if kind == GenKind::Async { "__async" } else { "__asyncGenerator" };
                out.push(Piece::Text(format!(" return {runner}({this}, {}, function () {{{wrapper_vars}", if this == "this" { "arguments" } else { "[]" })));
                out.extend(g.hoisted);
                out.extend(machine);
                out.push(text(" });"));
            }
        }
        Some(out)
    }

    /// `arguments` anywhere in the body outside the functions that have their own (arrows included, which share it).
    fn mentions_arguments_deep(&self, node: NodeId) -> bool {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Ident(a) if ast.atom(a) == "arguments" => true,
            NodeKind::Function { .. } if !ast[node].has(crate::ast::flags::ARROW) => false,
            _ => {
                let mut found = false;
                ast.for_each_child(node, |c| found = found || self.mentions_arguments_deep(c));
                found
            }
        }
    }

    fn fail(&mut self, g: &mut Gen, node: NodeId, why: &str) {
        if !g.failed {
            g.failed = true;
            let feature = match g.kind {
                GenKind::Generator => Feature::Generator,
                GenKind::Async => Feature::AsyncFunction,
                GenKind::AsyncGenerator => Feature::AsyncIteration,
            };
            self.refuse(feature, node, why);
        }
    }

    // ---- what has a yield in it ----

    fn is_yield(&self, g: &Gen, node: NodeId) -> bool {
        match self.ast[node].kind {
            NodeKind::Yield(_) => true,
            NodeKind::Await(_) => true,
            NodeKind::ForOf { .. } => self.ast[node].has(crate::ast::flags::AWAIT) && g.kind != GenKind::Generator,
            _ => false,
        }
    }

    fn has_yield(&self, g: &mut Gen, node: NodeId) -> bool {
        if let Some(&known) = g.yields.get(&node) {
            return known;
        }
        let ast = self.ast;
        let found = if matches!(ast[node].kind, NodeKind::Function { .. }) {
            false
        } else if self.is_yield(g, node) {
            true
        } else {
            let mut kids = Vec::new();
            ast.for_each_child(node, |c| kids.push(c));
            let mut any = false;
            for c in kids {
                if self.has_yield(g, c) {
                    any = true;
                    break;
                }
            }
            any
        };
        g.yields.insert(node, found);
        found
    }

    /// Whether a `break` or `continue` inside `node` leaves it (and so cannot be kept as it is), looking for the labels declared inside.
    fn jumps_out(&self, node: NodeId, in_loop: bool, in_switch: bool, labels: &mut Vec<Atom>) -> bool {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Function { .. } => false,
            NodeKind::Break(None) => !in_loop && !in_switch,
            NodeKind::Continue(None) => !in_loop,
            NodeKind::Break(Some(l)) | NodeKind::Continue(Some(l)) => !labels.contains(&l),
            NodeKind::Labeled { label, body } => {
                labels.push(label);
                let r = self.jumps_out(body, in_loop, in_switch, labels);
                labels.pop();
                r
            }
            NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } => {
                let mut kids = Vec::new();
                ast.for_each_child(node, |c| kids.push(c));
                kids.into_iter().any(|c| self.jumps_out(c, true, in_switch, labels))
            }
            NodeKind::Switch { .. } => {
                let mut kids = Vec::new();
                ast.for_each_child(node, |c| kids.push(c));
                kids.into_iter().any(|c| self.jumps_out(c, in_loop, true, labels))
            }
            _ => {
                let mut kids = Vec::new();
                ast.for_each_child(node, |c| kids.push(c));
                kids.into_iter().any(|c| self.jumps_out(c, in_loop, in_switch, labels))
            }
        }
    }

    // ---- statements kept as they are ----

    /// Edits for what a statement that is kept has in it: its `return`s, and its `var`s.
    fn gprepare(&mut self, g: &mut Gen, node: NodeId) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Function { .. } => return,
            NodeKind::Return(arg) => {
                let mut p = vec![text("return [2")];
                if let Some(a) = arg {
                    p.push(text(", "));
                    p.extend(self.rng(a));
                }
                p.push(text("];"));
                self.push(node, p);
            }
            NodeKind::VarDecl { .. } => {
                let pieces = self.gdecl(g, node, true);
                self.push(node, pieces);
                // The initial values may have more of them (functions are left alone).
                return;
            }
            _ => {}
        }
        let mut kids = Vec::new();
        ast.for_each_child(node, |c| kids.push(c));
        for c in kids {
            self.gprepare(g, c);
        }
    }

    /// A declaration as assignments (its names are declared outside the machine). `statement`: write the `;` of a statement.
    fn gdecl(&mut self, g: &mut Gen, node: NodeId, statement: bool) -> Vec<Piece> {
        let ast = self.ast;
        let NodeKind::VarDecl { kind, decls } = ast[node].kind else { return Vec::new() };
        let head = self.es5_in_head(node);
        let for_init = self.blocks().for_inits.contains(&node);
        let statement = statement && !for_init;
        let mut items: Vec<Vec<Piece>> = Vec::new();
        for &d in ast.list(decls) {
            let NodeKind::VarDeclarator { target, init } = ast[d].kind else { continue };
            self.gdeclare_names(g, target);
            if head {
                // `for (var k in o)`: the variable stays the loop's.
                items.push(self.rng(target));
                continue;
            }
            match init {
                Some(init) => {
                    if matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) {
                        let before = self.bundler.temp_count;
                        let source = self.local("d");
                        let mut first = vec![Piece::Text(format!("{source} = "))];
                        first.extend(self.rng(init));
                        items.push(first);
                        items.extend(self.destructure(target, &source, true));
                        for n in before..self.bundler.temp_count {
                            g.declare(format!("_$d{n}"));
                        }
                    } else {
                        let mut item = self.rng(target);
                        item.push(text(" = "));
                        item.extend(self.rng(init));
                        items.push(item);
                    }
                }
                None if kind == VarKind::Let && matches!(ast[target].kind, NodeKind::BindingIdent(_)) => {
                    let mut item = self.rng(target);
                    item.push(text(" = void 0"));
                    items.push(item);
                }
                None => {}
            }
        }
        let mut out = super::lower_es5::join(items);
        if statement {
            out.push(text(";"));
        }
        out
    }

    /// Declares the names a binding pattern has, in the function that holds the machine.
    fn gdeclare_names(&mut self, g: &mut Gen, pattern: NodeId) {
        let ast = self.ast;
        match ast[pattern].kind {
            NodeKind::BindingIdent(a) => {
                let name = match self.blocks().renames.get(&pattern).cloned() {
                    Some(n) => n,
                    None => ast.atom(a).to_string(),
                };
                g.declare(name);
            }
            _ => {
                let mut kids = Vec::new();
                ast.for_each_child(pattern, |c| kids.push(c));
                for c in kids {
                    if !matches!(ast[c].kind, NodeKind::Function { .. }) {
                        self.gdeclare_names(g, c);
                    }
                }
            }
        }
    }

    /// Whether a declaration is the head of a `for in` or `for of` (it has no values, and is not a statement).
    fn es5_in_head(&mut self, decl: NodeId) -> bool {
        self.blocks().heads.contains(&decl)
    }

    /// A statement kept as it is.
    fn graw(&mut self, g: &mut Gen, node: NodeId) {
        self.gprepare(g, node);
        let (a, b) = self.span(node);
        g.emit(vec![Piece::Range(a, b)]);
    }

    // ---- statements ----

    fn needs_machine(&mut self, g: &mut Gen, node: NodeId) -> bool {
        self.has_yield(g, node) || self.jumps_out(node, false, false, &mut Vec::new())
    }

    fn gst(&mut self, g: &mut Gen, node: NodeId) {
        if g.failed {
            return;
        }
        let ast = self.ast;
        let kind = ast[node].kind;
        // Declarations of functions are made before the machine runs.
        if let NodeKind::Function { name: Some(_), .. } = kind {
            if ast[node].has(crate::ast::flags::DECL) {
                let (a, b) = self.span(node);
                g.hoisted.push(text(" "));
                g.hoisted.push(Piece::Range(a, b));
                self.edits.push(Edit { start: a, end: b, pieces: Vec::new(), shorthand: None, transparent: true });
                return;
            }
        }
        if matches!(kind, NodeKind::Class { .. }) {
            self.fail(g, node, "and a class declared inside it cannot be rewritten for es5 yet");
            return;
        }
        let pending = std::mem::take(&mut g.pending);
        let is_loop = matches!(kind, NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } | NodeKind::Switch { .. });
        if let NodeKind::Labeled { label, body } = kind {
            if self.needs_machine(g, body) {
                let mut labels = pending;
                labels.push(label);
                g.pending = labels;
                if matches!(ast[body].kind, NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } | NodeKind::Switch { .. }) {
                    self.gst(g, body);
                } else {
                    let end = g.sym();
                    g.jumps.push(Jump { labels: std::mem::take(&mut g.pending), brk: Some(end), cont: None });
                    self.gst(g, body);
                    g.jumps.pop();
                    g.mark(end);
                }
                return;
            }
        }
        let own = if is_loop { pending } else { Vec::new() };
        if !self.needs_machine(g, node) {
            self.graw(g, node);
            return;
        }
        match kind {
            NodeKind::Empty => {}
            NodeKind::ExprStmt(e) => {
                let value = self.gx(g, e);
                let mut p = value;
                p.push(text(";"));
                g.emit(p);
            }
            NodeKind::VarDecl { decls, .. } => {
                for &d in ast.list(decls) {
                    let NodeKind::VarDeclarator { target, init } = ast[d].kind else { continue };
                    self.gdeclare_names(g, target);
                    if self.has_yield(g, target) {
                        self.fail(g, target, "and a yield in a default value of a pattern cannot be rewritten for es5 yet");
                        return;
                    }
                    let Some(init) = init else { continue };
                    let value = self.gx(g, init);
                    if matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) {
                        let before = self.bundler.temp_count;
                        let source = self.local("d");
                        let mut first = vec![Piece::Text(format!("{source} = "))];
                        first.extend(value);
                        let mut items = vec![first];
                        items.extend(self.destructure(target, &source, true));
                        for n in before..self.bundler.temp_count {
                            g.declare(format!("_$d{n}"));
                        }
                        let mut p = super::lower_es5::join(items);
                        p.push(text(";"));
                        g.emit(p);
                    } else {
                        let mut p = self.rng(target);
                        p.push(text(" = "));
                        p.extend(value);
                        p.push(text(";"));
                        g.emit(p);
                    }
                }
            }
            NodeKind::Block(list) => {
                for &s in ast.list(list) {
                    self.gst(g, s);
                }
            }
            NodeKind::If { test, cons, alt } => {
                let c = self.gx(g, test);
                let (other, end) = (g.sym(), g.sym());
                let mut p = vec![text("if (!(")];
                p.extend(c);
                p.extend([text(")) return [3, "), Gen::at(other), text("];")]);
                g.emit(p);
                self.gst(g, cons);
                if let Some(alt) = alt {
                    g.emit(vec![text("return [3, "), Gen::at(end), text("];")]);
                    g.mark(other);
                    self.gst(g, alt);
                    g.mark(end);
                } else {
                    g.mark(other);
                }
            }
            NodeKind::While { test, body } => {
                let (top, end) = (g.sym(), g.sym());
                g.mark(top);
                let c = self.gx(g, test);
                let mut p = vec![text("if (!(")];
                p.extend(c);
                p.extend([text(")) return [3, "), Gen::at(end), text("];")]);
                g.emit(p);
                g.jumps.push(Jump { labels: own, brk: Some(end), cont: Some(top) });
                self.gst(g, body);
                g.jumps.pop();
                g.emit(vec![text("return [3, "), Gen::at(top), text("];")]);
                g.mark(end);
            }
            NodeKind::DoWhile { body, test } => {
                let (top, cont, end) = (g.sym(), g.sym(), g.sym());
                g.mark(top);
                g.jumps.push(Jump { labels: own, brk: Some(end), cont: Some(cont) });
                self.gst(g, body);
                g.jumps.pop();
                g.mark(cont);
                let c = self.gx(g, test);
                let mut p = vec![text("if (")];
                p.extend(c);
                p.extend([text(") return [3, "), Gen::at(top), text("];")]);
                g.emit(p);
                g.mark(end);
            }
            NodeKind::For { init, test, update, body } => {
                if self.blocks().wrapped.contains_key(&node) {
                    self.fail(g, node, "and a loop with a yield whose body has a function that captures the loop's variable cannot be rewritten for es5 yet");
                    return;
                }
                if let Some(i) = init {
                    if matches!(ast[i].kind, NodeKind::VarDecl { .. }) {
                        let mut p = self.gdecl(g, i, false);
                        p.push(text(";"));
                        g.emit(p);
                    } else {
                        let v = self.gx(g, i);
                        let mut p = v;
                        p.push(text(";"));
                        g.emit(p);
                    }
                }
                let (top, step, end) = (g.sym(), g.sym(), g.sym());
                g.mark(top);
                if let Some(t) = test {
                    let c = self.gx(g, t);
                    let mut p = vec![text("if (!(")];
                    p.extend(c);
                    p.extend([text(")) return [3, "), Gen::at(end), text("];")]);
                    g.emit(p);
                }
                g.jumps.push(Jump { labels: own, brk: Some(end), cont: Some(step) });
                self.gst(g, body);
                g.jumps.pop();
                g.mark(step);
                if let Some(u) = update {
                    let v = self.gx(g, u);
                    let mut p = v;
                    p.push(text(";"));
                    g.emit(p);
                }
                g.emit(vec![text("return [3, "), Gen::at(top), text("];")]);
                g.mark(end);
            }
            NodeKind::ForIn { left, right, body } => {
                // The keys are taken first, then walked.
                let keys = g.temp(self.local("k"));
                let at = g.temp(self.local("n"));
                let key = g.temp(self.local("c"));
                let source = self.gx(g, right);
                let mut p = vec![Piece::Text(format!("{keys} = []; for ({key} in "))];
                p.extend(source);
                p.push(Piece::Text(format!(") {keys}.push({key}); {at} = 0;")));
                g.emit(p);
                let (top, end) = (g.sym(), g.sym());
                g.mark(top);
                g.emit(vec![Piece::Text(format!("if ({at} >= {keys}.length) return [3, ")), Gen::at(end), text("];")]);
                let take = self.gtake(g, left, &format!("{keys}[{at}++]"));
                g.emit(take);
                g.jumps.push(Jump { labels: own, brk: Some(end), cont: Some(top) });
                self.gst(g, body);
                g.jumps.pop();
                g.emit(vec![text("return [3, "), Gen::at(top), text("];")]);
                g.mark(end);
            }
            NodeKind::ForOf { left, right, body } => self.gfor_of(g, node, left, right, body, own),
            NodeKind::Switch { disc, cases } => {
                let end = g.sym();
                let value = self.gx(g, disc);
                let t = g.temp(self.local("w"));
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(value);
                p.push(text(";"));
                g.emit(p);
                let list = ast.list(cases).to_vec();
                let labels: Vec<usize> = list.iter().map(|_| g.sym()).collect();
                let mut dispatch = vec![Piece::Text(format!("switch ({t}) {{"))];
                let mut default_to = end;
                for (i, &c) in list.iter().enumerate() {
                    let NodeKind::Case { test, .. } = ast[c].kind else { continue };
                    match test {
                        Some(tv) => {
                            if self.has_yield(g, tv) {
                                self.fail(g, tv, "and a yield in the test of a case cannot be rewritten for es5 yet");
                                return;
                            }
                            dispatch.push(text(" case "));
                            dispatch.extend(self.rng(tv));
                            dispatch.extend([text(": return [3, "), Gen::at(labels[i]), text("];")]);
                        }
                        None => default_to = labels[i],
                    }
                }
                dispatch.extend([text(" default: return [3, "), Gen::at(default_to), text("]; }")]);
                g.emit(dispatch);
                g.jumps.push(Jump { labels: own, brk: Some(end), cont: None });
                for (i, &c) in list.iter().enumerate() {
                    let NodeKind::Case { body, .. } = ast[c].kind else { continue };
                    g.mark(labels[i]);
                    for &s in ast.list(body) {
                        self.gst(g, s);
                    }
                }
                g.jumps.pop();
                g.mark(end);
            }
            NodeKind::Try { block, handler, finalizer } => {
                let (try_at, catch_at, finally_at, end) = (g.sym(), g.sym(), g.sym(), g.sym());
                g.mark(try_at);
                g.emit(vec![
                    text("_$s.trys.push(["),
                    Gen::at(try_at),
                    text(", "),
                    if handler.is_some() { Gen::at(catch_at) } else { text("") },
                    text(", "),
                    if finalizer.is_some() { Gen::at(finally_at) } else { text("") },
                    text(", "),
                    Gen::at(end),
                    text("]);"),
                ]);
                self.gst(g, block);
                g.emit(vec![text("return [3, "), Gen::at(end), text("];")]);
                if let Some(h) = handler {
                    g.mark(catch_at);
                    if let NodeKind::Catch { param, body } = ast[h].kind {
                        match param {
                            Some(p) => {
                                self.gdeclare_names(g, p);
                                if matches!(ast[p].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) {
                                    let before = self.bundler.temp_count;
                                    let source = self.local("e");
                                    let mut items = vec![vec![Piece::Text(format!("{source} = _$s.sent()"))]];
                                    items.extend(self.destructure(p, &source, true));
                                    g.declare(source);
                                    for n in before..self.bundler.temp_count {
                                        g.declare(format!("_$d{n}"));
                                    }
                                    let mut code = super::lower_es5::join(items);
                                    code.push(text(";"));
                                    g.emit(code);
                                } else {
                                    let mut code = self.rng(p);
                                    code.push(text(" = _$s.sent();"));
                                    g.emit(code);
                                }
                            }
                            None => g.say("_$s.sent();"),
                        }
                        self.gst(g, body);
                    }
                    g.emit(vec![text("return [3, "), Gen::at(end), text("];")]);
                }
                if let Some(f) = finalizer {
                    g.mark(finally_at);
                    self.gst(g, f);
                    g.say("return [7];");
                }
                g.mark(end);
            }
            NodeKind::Labeled { body, .. } => self.gst(g, body),
            NodeKind::Break(label) => match self.gtarget(g, label, false) {
                Some(l) => g.emit(vec![text("return [3, "), Gen::at(l), text("];")]),
                None => self.fail(g, node, "and this break has nowhere to go in the state machine"),
            },
            NodeKind::Continue(label) => match self.gtarget(g, label, true) {
                Some(l) => g.emit(vec![text("return [3, "), Gen::at(l), text("];")]),
                None => self.fail(g, node, "and this continue has nowhere to go in the state machine"),
            },
            NodeKind::Return(arg) => {
                let mut p = vec![text("return [2")];
                if let Some(a) = arg {
                    let v = self.gx(g, a);
                    p.push(text(", "));
                    p.extend(v);
                }
                p.push(text("];"));
                g.emit(p);
            }
            NodeKind::Throw(arg) => {
                let v = self.gx(g, arg);
                let mut p = vec![text("throw ")];
                p.extend(v);
                p.push(text(";"));
                g.emit(p);
            }
            _ => self.fail(g, node, "and Rathkit Pack cannot rewrite this statement with a yield or a jump in it"),
        }
    }

    fn gtarget(&self, g: &Gen, label: Option<Atom>, cont: bool) -> Option<usize> {
        for j in g.jumps.iter().rev() {
            let hit = match label {
                None => if cont { j.cont.is_some() } else { j.brk.is_some() },
                Some(l) => j.labels.contains(&l),
            };
            if hit {
                return if cont { j.cont } else { j.brk };
            }
        }
        None
    }

    /// Takes the value `read` (text) into the variable or pattern of a loop's head.
    fn gtake(&mut self, g: &mut Gen, left: NodeId, read: &str) -> Vec<Piece> {
        let ast = self.ast;
        let (target, declared) = match ast[left].kind {
            NodeKind::VarDecl { decls, .. } => match ast.list(decls).first().map(|&d| ast[d].kind) {
                Some(NodeKind::VarDeclarator { target, .. }) => {
                    self.gdeclare_names(g, target);
                    (target, true)
                }
                _ => return Vec::new(),
            },
            _ => (left, false),
        };
        let _ = declared;
        if matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) {
            let before = self.bundler.temp_count;
            let source = self.local("d");
            let mut items = vec![vec![Piece::Text(format!("{source} = {read}"))]];
            items.extend(self.destructure(target, &source, true));
            for n in before..self.bundler.temp_count {
                g.declare(format!("_$d{n}"));
            }
            let mut p = super::lower_es5::join(items);
            p.push(text(";"));
            p
        } else {
            let mut p = self.rng(target);
            p.push(Piece::Text(format!(" = {read};")));
            p
        }
    }

    fn gfor_of(&mut self, g: &mut Gen, node: NodeId, left: NodeId, right: NodeId, body: NodeId, own: Vec<Atom>) {
        let ast = self.ast;
        let asynchronous = ast[node].has(crate::ast::flags::AWAIT);
        if self.blocks().wrapped.contains_key(&node) {
            self.fail(g, node, "and a loop with a yield whose body has a function that captures the loop's variable cannot be rewritten for es5 yet");
            return;
        }
        self.bundler.helpers.es5_iter = true;
        if asynchronous {
            self.bundler.helpers.for_await = true;
        }
        let it = g.temp(self.local("i"));
        let step = g.temp(self.local("r"));
        let source = self.gx(g, right);
        let mut p = vec![Piece::Text(format!("{it} = {}(", if asynchronous { "__forAwait" } else { "__iter" }))];
        p.extend(source);
        p.push(Piece::Text(format!("); {step} = void 0;")));
        g.emit(p);
        // The iterator is closed when the loop is left early: a `finally` around the loop.
        let (try_at, finally_at, end, top, done) = (g.sym(), g.sym(), g.sym(), g.sym(), g.sym());
        g.mark(try_at);
        g.emit(vec![text("_$s.trys.push(["), Gen::at(try_at), text(", , "), Gen::at(finally_at), text(", "), Gen::at(end), text("]);")]);
        g.mark(top);
        if asynchronous {
            let call = format!("{it}.next()");
            let wrapped = if g.kind == GenKind::AsyncGenerator { format!("new __await({call})") } else { call };
            g.emit(vec![Piece::Text(format!("return [4, {wrapped}];"))]);
            g.next_case();
            g.emit(vec![Piece::Text(format!("{step} = _$s.sent();"))]);
        } else {
            g.emit(vec![Piece::Text(format!("{step} = {it}.next();"))]);
        }
        g.emit(vec![Piece::Text(format!("if ({step}.done) return [3, ")), Gen::at(done), text("];")]);
        let take = self.gtake(g, left, &format!("{step}.value"));
        g.emit(take);
        g.jumps.push(Jump { labels: own, brk: Some(done), cont: Some(top) });
        self.gst(g, body);
        g.jumps.pop();
        g.emit(vec![text("return [3, "), Gen::at(top), text("];")]);
        g.mark(done);
        g.emit(vec![Piece::Text(format!("{step} = void 0;")), text("return [3, "), Gen::at(end), text("];")]);
        g.mark(finally_at);
        if asynchronous {
            let skip = g.sym();
            g.emit(vec![Piece::Text(format!("if (!({step} && !{step}.done && {it}[\"return\"])) return [3, ")), Gen::at(skip), text("];")]);
            let call = format!("{it}[\"return\"]()");
            let wrapped = if g.kind == GenKind::AsyncGenerator { format!("new __await({call})") } else { call };
            g.emit(vec![Piece::Text(format!("return [4, {wrapped}];"))]);
            g.next_case();
            g.say("_$s.sent();");
            g.mark(skip);
        } else {
            g.emit(vec![Piece::Text(format!("if ({step} && !{step}.done && {it}[\"return\"]) {it}[\"return\"]();"))]);
        }
        g.say("return [7];");
        g.mark(end);
    }

    // ---- expressions ----

    /// The pieces of an expression that stands for its value, after any `yield` in it has been written as the end of a case.
    fn gx(&mut self, g: &mut Gen, e: NodeId) -> Vec<Piece> {
        let ast = self.ast;
        if g.failed || !self.has_yield(g, e) {
            return self.rng(e);
        }
        match ast[e].kind {
            NodeKind::Yield(arg) => {
                let delegate = ast[e].has(crate::ast::flags::DELEGATE);
                let value = match arg {
                    Some(a) => self.gx(g, a),
                    None => vec![text("void 0")],
                };
                if delegate {
                    if g.kind == GenKind::AsyncGenerator {
                        // Each value of the (async) iterable is awaited and yielded; the value sent in goes to the next `next` (a `throw` or `return`
                        // sent in is not passed on).
                        self.bundler.helpers.for_await = true;
                        self.bundler.helpers.es5_iter = true;
                        let it = g.temp(self.local("i"));
                        let sent = g.temp(self.local("v"));
                        let r = g.temp(self.local("r"));
                        let mut p = vec![Piece::Text(format!("{it} = __forAwait("))];
                        p.extend(value);
                        p.push(Piece::Text(format!("); {sent} = void 0;")));
                        g.emit(p);
                        let (top, end) = (g.sym(), g.sym());
                        g.mark(top);
                        g.emit(vec![Piece::Text(format!("return [4, new __await({it}.next({sent}))];"))]);
                        g.next_case();
                        g.emit(vec![Piece::Text(format!("{r} = _$s.sent(); if ({r}.done) return [3, ")), Gen::at(end), text("];")]);
                        g.emit(vec![Piece::Text(format!("return [4, {r}.value];"))]);
                        g.next_case();
                        g.emit(vec![Piece::Text(format!("{sent} = _$s.sent();")), text("return [3, "), Gen::at(top), text("];")]);
                        g.mark(end);
                        return vec![Piece::Text(format!("{r}.value"))];
                    }
                    self.bundler.helpers.es5_iter = true;
                    let mut p = vec![text("return [5, __iter(")];
                    p.extend(value);
                    p.push(text(")];"));
                    g.emit(p);
                } else {
                    let mut p = vec![text("return [4, ")];
                    p.extend(value);
                    p.push(text("];"));
                    g.emit(p);
                }
                g.next_case();
                vec![text("_$s.sent()")]
            }
            NodeKind::Await(arg) => {
                let value = self.gx(g, arg);
                let mut p = vec![text(if g.kind == GenKind::AsyncGenerator { "return [4, new __await(" } else { "return [4, " })];
                p.extend(value);
                p.push(text(if g.kind == GenKind::AsyncGenerator { ")];" } else { "];" }));
                g.emit(p);
                g.next_case();
                vec![text("_$s.sent()")]
            }
            NodeKind::Binary { op, left, right } if matches!(op, BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish) => {
                let t = g.temp(self.local("t"));
                let l = self.gx(g, left);
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(l);
                p.push(text(";"));
                g.emit(p);
                let end = g.sym();
                let test = match op {
                    BinOp::LogAnd => format!("if (!{t}) return [3, "),
                    BinOp::LogOr => format!("if ({t}) return [3, "),
                    _ => format!("if ({t} != null) return [3, "),
                };
                g.emit(vec![Piece::Text(test), Gen::at(end), text("];")]);
                let r = self.gx(g, right);
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(r);
                p.push(text(";"));
                g.emit(p);
                g.mark(end);
                vec![Piece::Text(t)]
            }
            NodeKind::Binary { op, left, right } => {
                let parts = self.gordered(g, &[left, right]);
                let mut p = self.atom(parts[0].clone(), left);
                p.push(Piece::Text(format!(" {} ", op.as_str())));
                p.extend(self.atom(parts[1].clone(), right));
                vec![text("("), Piece::Text(String::new())].into_iter().chain(p).chain([text(")")]).collect()
            }
            NodeKind::Unary { op, arg } if op != UnaryOp::Delete => {
                let v = self.gx(g, arg);
                let mut p = vec![Piece::Text(format!("({}{}", op.as_str(), if matches!(op, UnaryOp::Typeof | UnaryOp::Void) { " " } else { "" }))];
                p.extend(self.atom(v, arg));
                p.push(text(")"));
                p
            }
            NodeKind::Conditional { test, cons, alt } => {
                let t = g.temp(self.local("t"));
                let c = self.gx(g, test);
                let (other, end) = (g.sym(), g.sym());
                let mut p = vec![text("if (!(")];
                p.extend(c);
                p.extend([text(")) return [3, "), Gen::at(other), text("];")]);
                g.emit(p);
                let a = self.gx(g, cons);
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(a);
                p.push(text(";"));
                g.emit(p);
                g.emit(vec![text("return [3, "), Gen::at(end), text("];")]);
                g.mark(other);
                let b = self.gx(g, alt);
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(b);
                p.push(text(";"));
                g.emit(p);
                g.mark(end);
                vec![Piece::Text(t)]
            }
            NodeKind::Member { object, prop } => {
                let computed = ast[e].has(crate::ast::flags::COMPUTED);
                let kids: Vec<NodeId> = if computed { vec![object, prop] } else { vec![object] };
                let parts = self.gordered(g, &kids);
                let mut p = self.atom(parts[0].clone(), object);
                if computed {
                    p.push(text("["));
                    p.extend(parts[1].clone());
                    p.push(text("]"));
                } else {
                    let (a, b) = self.span(prop);
                    p.push(text("."));
                    p.push(Piece::Range(a, b));
                }
                p
            }
            NodeKind::Call { callee, args } | NodeKind::New { callee, args } => {
                let items = ast.list(args).to_vec();
                if items.iter().any(|&a| matches!(ast[a].kind, NodeKind::Spread(_))) || ast[e].has(crate::ast::flags::OPTIONAL) {
                    self.fail(g, e, "and a yield next to a spread or an optional call cannot be rewritten for es5 yet");
                    return Vec::new();
                }
                let is_new = matches!(ast[e].kind, NodeKind::New { .. });
                let mut kids = vec![];
                // A method is called on what it was read from.
                let member = match ast[callee].kind {
                    NodeKind::Member { object, prop } if !is_new && !ast[callee].has(crate::ast::flags::OPTIONAL) && !matches!(ast[object].kind, NodeKind::Super) => Some((object, prop, ast[callee].has(crate::ast::flags::COMPUTED))),
                    _ => None,
                };
                match member {
                    Some((object, prop, computed)) => {
                        kids.push(object);
                        if computed {
                            kids.push(prop);
                        }
                    }
                    None => kids.push(callee),
                }
                let lead = kids.len();
                kids.extend(items.iter().copied());
                let parts = self.gordered(g, &kids);
                let mut p: Vec<Piece> = if is_new { vec![text("new ")] } else { Vec::new() };
                match member {
                    Some((object, prop, computed)) => {
                        p.extend(self.atom(parts[0].clone(), object));
                        if computed {
                            p.push(text("["));
                            p.extend(parts[1].clone());
                            p.push(text("]"));
                        } else {
                            let (a, b) = self.span(prop);
                            p.push(text("."));
                            p.push(Piece::Range(a, b));
                        }
                    }
                    None => p.extend(self.atom(parts[0].clone(), callee)),
                }
                p.push(text("("));
                for (i, part) in parts[lead..].iter().enumerate() {
                    if i > 0 {
                        p.push(text(", "));
                    }
                    p.extend(part.clone());
                }
                p.push(text(")"));
                p
            }
            NodeKind::Array(list) => {
                let items = ast.list(list).to_vec();
                if items.iter().any(|&a| matches!(ast[a].kind, NodeKind::Spread(_) | NodeKind::Hole)) {
                    self.fail(g, e, "and a yield next to a spread or a hole in an array cannot be rewritten for es5 yet");
                    return Vec::new();
                }
                let parts = self.gordered(g, &items);
                let mut p = vec![text("[")];
                for (i, part) in parts.into_iter().enumerate() {
                    if i > 0 {
                        p.push(text(", "));
                    }
                    p.extend(part);
                }
                p.push(text("]"));
                p
            }
            NodeKind::Object(list) => {
                let props = ast.list(list).to_vec();
                let mut values = Vec::new();
                for &pr in &props {
                    match ast[pr].kind {
                        NodeKind::Property { value, kind: PropKind::Init, .. } if !ast[pr].has(crate::ast::flags::COMPUTED) && !ast[pr].has(crate::ast::flags::SHORTHAND) => values.push(value),
                        NodeKind::Property { kind: PropKind::Init, .. } | NodeKind::Spread(_) => {
                            self.fail(g, e, "and a yield in an object with computed keys, shorthand or spread cannot be rewritten for es5 yet");
                            return Vec::new();
                        }
                        _ => {}
                    }
                }
                let parts = self.gordered(g, &values);
                let mut at = 0;
                let mut p = vec![text("{ ")];
                for (i, &pr) in props.iter().enumerate() {
                    if i > 0 {
                        p.push(text(", "));
                    }
                    match ast[pr].kind {
                        NodeKind::Property { key, kind: PropKind::Init, .. } => {
                            let (a, b) = self.span(key);
                            p.push(Piece::Range(a, b));
                            p.push(text(": "));
                            p.extend(parts[at].clone());
                            at += 1;
                        }
                        _ => {
                            let (a, b) = self.span(pr);
                            p.push(Piece::Range(a, b));
                        }
                    }
                }
                p.push(text(" }"));
                p
            }
            NodeKind::Template { quasis, exprs } => {
                let quasis = ast.list(quasis).to_vec();
                let exprs = ast.list(exprs).to_vec();
                let parts = self.gordered(g, &exprs);
                let cooked = |this: &Self, q: NodeId| match this.ast[q].kind {
                    NodeKind::TemplateElement { cooked: Some(c), .. } => json_string(this.ast.atom(c)),
                    _ => "\"\"".to_string(),
                };
                let mut p = vec![Piece::Text(format!("{}.concat(", cooked(self, quasis[0])))];
                for (i, part) in parts.into_iter().enumerate() {
                    if i > 0 {
                        p.push(text(", "));
                    }
                    p.extend(part);
                    if let Some(&q) = quasis.get(i + 1) {
                        p.push(Piece::Text(format!(", {}", cooked(self, q))));
                    }
                }
                p.push(text(")"));
                p
            }
            NodeKind::Sequence(list) => {
                let items = ast.list(list).to_vec();
                for &i in &items[..items.len() - 1] {
                    let mut v = self.gx(g, i);
                    v.push(text(";"));
                    g.emit(v);
                }
                self.gx(g, *items.last().unwrap())
            }
            NodeKind::Assign { op, target, value } if matches!(ast[target].kind, NodeKind::Ident(_)) => {
                let v = self.gx(g, value);
                let mut p = vec![text("(")];
                p.extend(self.rng(target));
                p.push(Piece::Text(format!(" {} ", op.as_str())));
                p.extend(v);
                p.push(text(")"));
                p
            }
            NodeKind::Assign { op, target, value } if matches!(ast[target].kind, NodeKind::Member { .. }) => {
                let NodeKind::Member { object, prop } = ast[target].kind else { return Vec::new() };
                let computed = ast[target].has(crate::ast::flags::COMPUTED);
                let mut kids = vec![object];
                if computed {
                    kids.push(prop);
                }
                kids.push(value);
                let parts = self.gordered(g, &kids);
                let mut p = vec![text("(")];
                p.extend(self.atom(parts[0].clone(), object));
                if computed {
                    p.push(text("["));
                    p.extend(parts[1].clone());
                    p.push(text("]"));
                } else {
                    let (a, b) = self.span(prop);
                    p.push(text("."));
                    p.push(Piece::Range(a, b));
                }
                p.push(Piece::Text(format!(" {} ", op.as_str())));
                p.extend(parts.last().unwrap().clone());
                p.push(text(")"));
                p
            }
            NodeKind::Assign { op: crate::ast::AssignOp::Assign, target, value } if matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) => {
                if self.has_yield(g, target) {
                    self.fail(g, target, "and a yield in a default value of a pattern cannot be rewritten for es5 yet");
                    return Vec::new();
                }
                let v = self.gx(g, value);
                let t = g.temp(self.local("t"));
                let before = self.bundler.temp_count;
                let source = self.local("d");
                let mut first = vec![Piece::Text(format!("{t} = {source} = "))];
                first.extend(v);
                let mut items = vec![first];
                items.extend(self.destructure(target, &source, true));
                for n in before..self.bundler.temp_count {
                    g.declare(format!("_$d{n}"));
                }
                let mut code = super::lower_es5::join(items);
                code.push(text(";"));
                g.emit(code);
                vec![Piece::Text(t)]
            }
            _ => {
                self.fail(g, e, "and a yield in this kind of expression cannot be rewritten for es5 yet");
                Vec::new()
            }
        }
    }

    /// The parts of an expression (operands, arguments), each written so that evaluating them one after the other keeps their order: those
    /// before the last one that has a `yield` go in variables.
    fn gordered(&mut self, g: &mut Gen, kids: &[NodeId]) -> Vec<Vec<Piece>> {
        let ast = self.ast;
        let mut last = None;
        for (i, &k) in kids.iter().enumerate() {
            if self.has_yield(g, k) {
                last = Some(i);
            }
        }
        let mut out = Vec::new();
        for (i, &k) in kids.iter().enumerate() {
            let value = self.gx(g, k);
            let before_last = last.is_some_and(|l| i < l);
            let constant = matches!(ast[k].kind, NodeKind::Num(_) | NodeKind::Str(_) | NodeKind::Bool(_) | NodeKind::Null);
            if before_last && !constant {
                let t = g.temp(self.local("t"));
                let mut p = vec![Piece::Text(format!("{t} = "))];
                p.extend(value);
                p.push(text(";"));
                g.emit(p);
                out.push(vec![Piece::Text(t)]);
            } else {
                out.push(value);
            }
        }
        out
    }

    /// Pieces of an operand, in parentheses unless they are a name or a literal.
    fn atom(&self, pieces: Vec<Piece>, node: NodeId) -> Vec<Piece> {
        let plain = match pieces.as_slice() {
            [Piece::Text(t)] => t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') || t == "_$s.sent()",
            _ => matches!(self.ast[node].kind, NodeKind::Ident(_) | NodeKind::Num(_) | NodeKind::Str(_) | NodeKind::This | NodeKind::Member { .. } | NodeKind::Call { .. }) && !self.ast[node].parenthesized() || matches!(self.ast[node].kind, NodeKind::Ident(_)),
        };
        if plain {
            pieces
        } else {
            [vec![text("(")], pieces, vec![text(")")]].concat()
        }
    }
}
