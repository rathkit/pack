//! `--target`: syntax the target's language does not have is written another way, as the scanner meets it.
//!
//! Each rewrite replaces a node with text made of its pieces (`Piece::Range` of the parts that stay, which still get their own rewrites and
//! renames) and a few words. A value that is used twice is a plain name, or is first put in a temporary, a `var` declared once at the top
//! of the file (`__rk0`, `__rk1`, ...): nothing runs between the assignment and the last read of one, so a recursive call cannot see
//! another's. What cannot be rewritten (yet) stops the build with a message that names it and the target, and is never passed through.

use super::*;
use crate::ast::{AssignOp, BinOp};
use crate::target::Feature;

fn ast_index(node: NodeId) -> u32 {
    node.index() as u32
}

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

/// How the `await`s of a function are written. Some target lacks async functions (`Yield`: the function becomes a generator run by
/// `__async`), or async generators (`Gen`: `await` is `yield new __await(...)`), or neither (`Native`: an `await` stays one).
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum AsyncKind {
    #[default]
    Native,
    Yield,
    Gen,
}

#[derive(Clone, Copy, Default)]
pub(super) struct FnCtx {
    kind: AsyncKind,
    /// Inside an async function that was rewritten and is a method, so `super.x` is read through its `__superGet`.
    super_ok: bool,
    /// The rewritten function whose generator declares the temporaries of the `super` writes in this one (a plain arrow shares its parent's).
    owner: u32,
}

const SUPER_ACCESSORS: &str = " const __superGet = (k) => super[k]; const __superSet = (k, v) => super[k] = v; ";

/// One link of a chain such as `a?.b[c](d)`, after the first expression.
struct Link {
    node: NodeId,
    /// Written with `?.`.
    optional: bool,
    call: bool,
    /// The text of the link as it follows an expression, without its `?.` (`.b`, `[c]`, `(d)`), and for a call the arguments only.
    tail: Vec<Piece>,
    arguments: Vec<Piece>,
    has_arguments: bool,
}

impl Scanner<'_, '_> {
    /// Whether an async function or async generator with these flags is rewritten (it is not an `async` any more).
    pub(super) fn async_lowered(&self, flags: u8) -> bool {
        use crate::ast::flags;
        flags & flags::ASYNC != 0 && self.lowers(if flags & flags::GENERATOR != 0 { Feature::AsyncIteration } else { Feature::AsyncFunction })
    }

    pub(super) fn fn_ctx(&self, node: NodeId) -> FnCtx {
        use crate::ast::flags;
        let ast = self.ast;
        let NodeKind::Function { body, .. } = ast[node].kind else { return FnCtx::default() };
        let f = ast[node].flags;
        let kind = match (self.async_lowered(f), f & flags::GENERATOR != 0) {
            (true, true) => AsyncKind::Gen,
            (true, false) => AsyncKind::Yield,
            _ => AsyncKind::Native,
        };
        let parent = self.async_ctx.last().copied().unwrap_or_default();
        let arrow = f & flags::ARROW != 0;
        // An arrow whose `super` the method around it does not provide gets accessors of its own (see `lower_async`).
        let super_ok = if arrow { parent.super_ok || kind != AsyncKind::Native && self.contains_super(body) } else { kind != AsyncKind::Native && self.contains_super(body) };
        let owner = if arrow && kind == AsyncKind::Native { parent.owner } else { node.index() as u32 };
        FnCtx { kind, super_ok, owner }
    }

    /// How an `await` here is written: by the function it is in, or, at the top of a module whose program becomes a function, by that one.
    fn kind_here(&self) -> AsyncKind {
        match self.async_ctx.last() {
            Some(ctx) => ctx.kind,
            None if self.fn_depth == 0 && self.bundler.tla_possible && self.lowers(Feature::AsyncFunction) => AsyncKind::Yield,
            None => AsyncKind::Native,
        }
    }

    fn await_pieces(&mut self, inner: Vec<Piece>) -> Vec<Piece> {
        let kind = self.kind_here();
        let (open, close) = match kind {
            AsyncKind::Native => ("(await ", ")"),
            AsyncKind::Yield => ("(yield ", ")"),
            AsyncKind::Gen => ("(yield new __await(", "))"),
        };
        if kind == AsyncKind::Gen {
            self.bundler.helpers.async_gen = true;
        }
        [vec![text(open)], inner, vec![text(close)]].concat()
    }

    pub(super) fn lowers(&self, feature: Feature) -> bool {
        !self.bundler.target.supports(feature)
    }

    pub(super) fn lower_error_message(&mut self, node: NodeId, message: String) {
        self.bundler.errors.push(Diagnostic { severity: crate::graph::Severity::Error, message, file: Some(self.module.path.clone()), pos: Some(self.ast[node].span.start) });
    }

    fn lower_error(&mut self, feature: Feature, node: NodeId) {
        let target = self.bundler.target.name();
        // BigInt and top-level await have no older way of being written; the others will be rewritten.
        let how = if matches!(feature, Feature::BigInt | Feature::TopLevelAwait) { "and it cannot be rewritten" } else { "and Rathkit Pack cannot rewrite it yet" };
        let message = format!("{} is not available in the target {target} (it came with es{}), {how}", feature.describe(), feature.since());
        self.bundler.errors.push(Diagnostic { severity: crate::graph::Severity::Error, message, file: Some(self.module.path.clone()), pos: Some(self.ast[node].span.start) });
    }

    /// A new temporary: a name for a `var` that the file's top declares.
    pub(super) fn temp(&mut self) -> String {
        let name = format!("__rk{}", self.bundler.temp_count);
        self.bundler.temp_count += 1;
        self.bundler.temps.push((self.module.id, name.clone()));
        name
    }

    pub(super) fn source_between(&self, from: u32, to: u32) -> &str {
        self.module.source.get(from as usize..to as usize).unwrap_or("")
    }

    /// An expression that is safe to write twice: a name or `this`.
    pub(super) fn is_plain(&self, node: NodeId) -> bool {
        matches!(self.ast[node].kind, NodeKind::Ident(_) | NodeKind::This | NodeKind::Super)
    }

    /// Where `op` is between two operands (the first one in the text after the left one, past any closing parentheses).
    fn operator_at(&self, left: NodeId, right: NodeId, op: &str) -> Option<u32> {
        let from = self.ast[left].span.end;
        let to = self.ast[right].span.start.max(from);
        self.source_between(from, to).find(op).map(|at| from + at as u32)
    }

    /// Where the link after `previous` begins: past the closing parentheses of a parenthesized `previous` and any white space.
    fn link_start(&self, previous_end: u32, node_end: u32) -> u32 {
        let rest = self.source_between(previous_end, node_end);
        let skipped = rest.len() - rest.trim_start_matches(|c: char| c == ')' || c.is_whitespace()).len();
        previous_end + skipped as u32
    }

    pub(super) fn lower(&mut self, node: NodeId) {
        let ast = self.ast;
        if self.async_ctx.last().is_some_and(|c| c.super_ok) {
            self.lower_super(node);
        }
        if self.bundler.legacy {
            self.lower_es5(node);
        }
        match ast[node].kind {
            NodeKind::Binary { op: BinOp::Exp, left, right } if self.lowers(Feature::Exponent) => self.lower_exponent(node, left, right),
            NodeKind::Assign { op: AssignOp::Op(BinOp::Exp), target, value } if self.lowers(Feature::Exponent) => self.lower_exponent_assignment(node, target, value),
            NodeKind::Binary { op: BinOp::Nullish, left, right } if self.lowers(Feature::NullishCoalescing) => self.lower_nullish(node, left, right),
            NodeKind::Assign { op: AssignOp::Op(op @ (BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish)), target, value } if self.lowers(Feature::LogicalAssignment) => self.lower_logical_assignment(node, op, target, value),
            NodeKind::Chain(inner) if (self.lowers(Feature::OptionalChain) || self.chain_has_private(inner)) && !self.skip.contains(&node) => {
                let pieces = self.lower_chain(inner, false);
                self.push(node, pieces);
            }
            NodeKind::Unary { op: crate::ast::UnaryOp::Delete, arg } if self.lowers(Feature::OptionalChain) && matches!(ast[arg].kind, NodeKind::Chain(_)) => {
                let NodeKind::Chain(inner) = ast[arg].kind else { return };
                self.skip.insert(arg);
                let pieces = self.lower_chain(inner, true);
                self.push(node, pieces);
            }
            NodeKind::Num(_) | NodeKind::BigInt(_) => {
                let (start, end) = self.span(node);
                let literal = self.source_between(start, end);
                if matches!(ast[node].kind, NodeKind::BigInt(_)) && self.lowers(Feature::BigInt) {
                    self.lower_error(Feature::BigInt, node);
                } else if literal.contains('_') && self.lowers(Feature::NumericSeparator) {
                    let plain = literal.replace('_', "");
                    self.push(node, vec![Piece::Text(plain)]);
                }
            }
            NodeKind::Catch { param: None, .. } if self.lowers(Feature::OptionalCatch) => {
                let at = ast[node].span.start;
                if self.source_between(at, at + 5) == "catch" {
                    self.edits.push(Edit { start: at + 5, end: at + 5, pieces: vec![text(" (__rkError)")], shorthand: None, transparent: false });
                }
            }
            NodeKind::Call { args, .. } | NodeKind::New { args, .. } if self.lowers(Feature::TrailingComma) => {
                if let Some(&last) = ast.list(args).last() {
                    let end = ast[node].span.end;
                    if end > 0 && self.module.source.as_bytes().get(end as usize - 1) == Some(&b')') {
                        self.drop_trailing_comma(ast[last].span.end, end - 1);
                    }
                }
            }
            NodeKind::Function { params, body, .. } => {
                let flags = ast[node].flags;
                if self.async_lowered(flags) && !self.skip.contains(&node) {
                    // A method's `async` comes before its key; the method has already handled it (see below).
                    let at = ast[node].span.start;
                    if self.source_between(at, at + 5) == "async" {
                        self.lower_async(node, at, None);
                    } else {
                        self.lower_error(Feature::AsyncFunction, node);
                    }
                }
                if self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) {
                    self.lower_rest_params(node, params, body);
                }
                if self.lowers(Feature::TrailingComma) {
                    if let Some(&last) = ast.list(params).last() {
                        let from = ast[last].span.end;
                        let to = ast[body].span.start.max(from);
                        let region = self.source_between(from, to);
                        if let Some(close) = region.rfind(')') {
                            self.drop_trailing_comma(from, from + close as u32);
                        }
                    }
                }
            }
            NodeKind::ClassMethod { key, value, .. } if self.is_async_method(value) => self.lower_async_method(node, key, value),
            NodeKind::Property { key, value, kind: crate::ast::PropKind::Method } if self.is_async_method(value) => self.lower_async_method(node, key, value),
            NodeKind::Await(arg) if self.kind_here() != AsyncKind::Native => {
                let (start, end) = self.span(node);
                let (a, b) = self.span(arg);
                let pieces = self.await_pieces(vec![Piece::Range(a, b)]);
                self.edits.push(Edit { start, end, pieces, shorthand: None, transparent: false });
            }
            NodeKind::Yield(Some(arg)) if ast[node].flags & crate::ast::flags::DELEGATE != 0 && self.async_ctx.last().is_some_and(|c| c.kind == AsyncKind::Gen) => {
                self.bundler.helpers.async_gen = true;
                let (a, b) = self.span(arg);
                self.edits.push(Edit { start: a, end: a, pieces: vec![text("__yieldStar(")], shorthand: None, transparent: false });
                self.edits.push(Edit { start: b, end: b, pieces: vec![text(")")], shorthand: None, transparent: false });
            }
            // Not rewritten yet: stop instead of writing what the target cannot run.
            NodeKind::Object(props) if self.lowers(Feature::ObjectSpread) && !self.lowers(Feature::ObjectLiteral) && ast.list(props).iter().any(|&p| matches!(ast[p].kind, NodeKind::Spread(_))) => self.lower_object_spread(node, props),
            // A pattern with `...rest` is rewritten where its parent does it (a declaration, a parameter, a `for of`); anywhere else it is refused.
            NodeKind::VarDeclarator { target, init: Some(init) } if self.contains_object_rest(target) && self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) => self.lower_rest_declarator(node, target, init),
            NodeKind::Labeled { body, .. } if self.fn_depth > 0 && self.lowers(Feature::AsyncIteration) && self.labeled_for_await(body).is_some_and(|i| !self.skip.contains(&i)) => {
                let inner = self.labeled_for_await(body).unwrap();
                let NodeKind::ForOf { left, right, body } = ast[inner].kind else { return };
                self.skip.insert(inner);
                let label_span = self.span(node);
                self.lower_for_await(inner, left, right, body, Some(label_span));
            }
            NodeKind::ForOf { left, right, body } if ast[node].flags & crate::ast::flags::AWAIT != 0 && self.lowers(Feature::AsyncIteration) && !self.skip.contains(&node) => {
                if self.fn_depth == 0 && !self.bundler.tla_possible {
                    self.lower_error(Feature::TopLevelAwait, node);
                } else {
                    self.lower_for_await(node, left, right, body, None);
                }
            }
            NodeKind::ForOf { left, body, .. } if self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) && self.for_of_rest(left).is_some() => self.lower_rest_for_of(node, left, body),
            NodeKind::Assign { op: AssignOp::Assign, target, value } if self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) && matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.contains_object_rest(target) && !self.skip.contains(&target) => self.lower_rest_assignment(node, target, value),
            NodeKind::Catch { param: Some(param), body } if self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) && self.contains_object_rest(param) => self.lower_rest_catch(param, body),
            NodeKind::ObjectPattern(_) if self.has_object_rest(node) && self.lowers(Feature::ObjectRest) && !self.lowers(Feature::Destructuring) && !self.skip.contains(&node) => self.lower_error(Feature::ObjectRest, node),
            // Fields, private members and static blocks are rewritten with the class that has them.
            // Fields, private members and static blocks are rewritten together: an engine can have some and lack the others.
            NodeKind::Class { .. } if (self.lowers(Feature::ClassField) || self.lowers(Feature::ClassPrivate) || self.lowers(Feature::ClassStaticBlock)) && !self.lowers(Feature::Class) && self.class_needs_lowering(node) => self.lower_class(node),
            NodeKind::Await(_) if self.fn_depth == 0 && self.lowers(Feature::TopLevelAwait) && !self.bundler.tla_possible => self.lower_error(Feature::TopLevelAwait, node),
            _ => {}
        }
    }

    /// Removes the comma that ends the text between `from` and `to` (a list's last item and the `)` after it), if there is one.
    fn drop_trailing_comma(&mut self, from: u32, to: u32) {
        let region = self.source_between(from, to);
        let trimmed = region.trim_end();
        if trimmed.ends_with(',') {
            let at = from + trimmed.len() as u32 - 1;
            self.edits.push(Edit { start: at, end: at + 1, pieces: Vec::new(), shorthand: None, transparent: false });
        }
    }

    // ---- ** and **= ----

    fn lower_exponent(&mut self, node: NodeId, left: NodeId, right: NodeId) {
        let (start, end) = self.span(node);
        let Some(at) = self.operator_at(left, right, "**") else { return };
        self.push(node, vec![text("Math.pow("), Piece::Range(start, at), text(", "), Piece::Range(at + 2, end), text(")")]);
    }

    fn lower_exponent_assignment(&mut self, node: NodeId, target: NodeId, value: NodeId) {
        let (start, end) = self.span(node);
        let Some(at) = self.operator_at(target, value, "**=") else { return };
        let Some((first, again)) = self.assignment_target(target) else {
            self.lower_error(Feature::ClassPrivate, node);
            return;
        };
        let mut pieces = first;
        pieces.push(text(" = Math.pow("));
        pieces.extend(again);
        pieces.push(text(", "));
        pieces.push(Piece::Range(at + 3, end));
        pieces.push(text(")"));
        let _ = start;
        self.push(node, pieces);
    }

    /// An assignment target written for its first use (the parts that are not plain assigned to temporaries) and again for the next uses.
    fn assignment_target(&mut self, target: NodeId) -> Option<(Vec<Piece>, Vec<Piece>)> {
        let ast = self.ast;
        match ast[target].kind {
            NodeKind::Member { object, prop } if ast[target].flags & crate::ast::flags::OPTIONAL == 0 && matches!(ast[prop].kind, NodeKind::PrivateName(_)) => {
                let info = self.private_of_member(target)?;
                let t = self.temp();
                let (os, oe) = self.span(object);
                let mut first = vec![text(&format!("({t} = "))];
                first.extend(self.private_wrapper_call(&info, vec![Piece::Range(os, oe)]));
                first.push(text(")._"));
                Some((first, vec![text(&format!("{t}._"))]))
            }
            NodeKind::Member { object, prop } if ast[target].flags & crate::ast::flags::OPTIONAL == 0 => {
                let (os, oe) = self.span(object);
                let (first_object, again_object) = if self.is_plain(object) {
                    (vec![Piece::Range(os, oe)], vec![Piece::Range(os, oe)])
                } else {
                    let t = self.temp();
                    (vec![text(&format!("({t} = (")), Piece::Range(os, oe), text("))")], vec![text(&t)])
                };
                if ast[target].flags & crate::ast::flags::COMPUTED != 0 {
                    let (ks, ke) = self.span(prop);
                    let literal = matches!(ast[prop].kind, NodeKind::Str(_) | NodeKind::Num(_) | NodeKind::Ident(_) | NodeKind::Bool(_) | NodeKind::Null);
                    let (mut first, mut again) = (first_object, again_object);
                    if literal {
                        first.extend([text("["), Piece::Range(ks, ke), text("]")]);
                        again.extend([text("["), Piece::Range(ks, ke), text("]")]);
                    } else {
                        let k = self.temp();
                        first.extend([text(&format!("[{k} = ")), Piece::Range(ks, ke), text("]")]);
                        again.push(text(&format!("[{k}]")));
                    }
                    Some((first, again))
                } else {
                    let (ps, pe) = self.span(prop);
                    let (mut first, mut again) = (first_object, again_object);
                    first.extend([text("."), Piece::Range(ps, pe)]);
                    again.extend([text("."), Piece::Range(ps, pe)]);
                    Some((first, again))
                }
            }
            NodeKind::Ident(_) => {
                let (s, e) = self.span(target);
                Some((vec![Piece::Range(s, e)], vec![Piece::Range(s, e)]))
            }
            _ => None,
        }
    }

    // ---- ??, ||=, &&=, ??= ----

    fn lower_nullish(&mut self, node: NodeId, left: NodeId, right: NodeId) {
        let (start, end) = self.span(node);
        let Some(at) = self.operator_at(left, right, "??") else { return };
        let mut pieces = vec![text("(")];
        if self.is_plain(left) {
            let (ls, le) = self.span(left);
            pieces.extend([Piece::Range(ls, le), text(" != null ? "), Piece::Range(ls, le)]);
        } else {
            let t = self.temp();
            pieces.extend([text(&format!("({t} = ")), Piece::Range(start, at), text(&format!(") != null ? {t}"))]);
        }
        pieces.extend([text(" : "), Piece::Range(at + 2, end), text(")")]);
        self.push(node, pieces);
    }

    fn lower_logical_assignment(&mut self, node: NodeId, op: BinOp, target: NodeId, value: NodeId) {
        let (_, end) = self.span(node);
        let symbol = match op {
            BinOp::LogAnd => "&&=",
            BinOp::LogOr => "||=",
            _ => "??=",
        };
        let Some(at) = self.operator_at(target, value, symbol) else { return };
        let Some((first, again)) = self.assignment_target(target) else {
            self.lower_error(Feature::ClassPrivate, node);
            return;
        };
        let value_pieces = |again: Vec<Piece>| {
            let mut v = again;
            v.push(text(" = "));
            v.push(Piece::Range(at + 3, end));
            v
        };
        let mut pieces = vec![text("(")];
        match op {
            BinOp::LogAnd | BinOp::LogOr => {
                pieces.extend(first);
                pieces.push(text(if op == BinOp::LogAnd { " && (" } else { " || (" }));
                pieces.extend(value_pieces(again));
                pieces.push(text(")"));
            }
            _ => {
                if matches!(self.ast[target].kind, NodeKind::Ident(_)) {
                    pieces.extend(first.clone());
                    pieces.push(text(" != null ? "));
                    pieces.extend(first);
                    pieces.push(text(" : "));
                } else {
                    let t = self.temp();
                    pieces.push(text(&format!("({t} = ")));
                    pieces.extend(first);
                    pieces.push(text(&format!(") != null ? {t} : ")));
                }
                pieces.extend(value_pieces(again));
            }
        }
        pieces.push(text(")"));
        self.push(node, pieces);
    }

    // ---- optional chains ----

    /// The links of a chain from its first expression outwards, and that expression.
    fn chain_links(&self, inner: NodeId) -> (NodeId, Vec<Link>) {
        let ast = self.ast;
        let mut spine = Vec::new();
        let mut at = inner;
        loop {
            match ast[at].kind {
                NodeKind::Member { object, .. } => {
                    spine.push(at);
                    at = object;
                }
                NodeKind::Call { callee, .. } => {
                    spine.push(at);
                    at = callee;
                }
                _ => break,
            }
            if matches!(ast[at].kind, NodeKind::Chain(_)) {
                break;
            }
        }
        spine.reverse();
        let base = at;
        let mut links = Vec::new();
        let mut previous_end = ast[base].span.end;
        for node in spine {
            let (_, end) = self.span(node);
            let optional = ast[node].flags & crate::ast::flags::OPTIONAL != 0;
            let mut begin = self.link_start(previous_end, end);
            let rest = self.source_between(begin, end);
            // `?.` is dropped: what follows it is written as a plain link (`?.b` is `.b`, `?.[b]` is `[b]`, `?.(b)` is `(b)`).
            let mut dot = String::new();
            if optional {
                if let Some(q) = rest.find("?.") {
                    begin += q as u32 + 2;
                    let after = self.source_between(begin, end).trim_start();
                    if !after.starts_with('[') && !after.starts_with('(') && !after.starts_with('<') {
                        dot.push('.');
                    }
                }
            }
            let call = matches!(ast[node].kind, NodeKind::Call { .. });
            let mut tail = Vec::new();
            if !dot.is_empty() {
                tail.push(Piece::Text(dot));
            }
            tail.push(Piece::Range(begin, end));
            let (mut arguments, mut has_arguments) = (Vec::new(), false);
            if let NodeKind::Call { args, .. } = ast[node].kind {
                has_arguments = !ast.list(args).is_empty();
                // The text inside the parentheses: from after the `(` to before the last `)`.
                let after_open = self.source_between(begin, end).find('(').map_or(begin, |p| begin + p as u32 + 1);
                arguments.push(Piece::Range(after_open, end.saturating_sub(1)));
            }
            links.push(Link { node, optional, call, tail, arguments, has_arguments });
            previous_end = end;
        }
        (base, links)
    }

    /// An optional chain as a conditional expression: `a?.b.c(d)` is `(a == null ? void 0 : a.b.c(d))`. `delete` is the chain as the
    /// operand of `delete`, which gives `true` for a short circuit.
    fn lower_chain(&mut self, inner: NodeId, delete: bool) -> Vec<Piece> {
        let ast = self.ast;
        let (base, links) = self.chain_links(inner);
        let none = if delete { "true" } else { "void 0" };
        let (bs, be) = self.span(base);
        let mut prefix: Vec<Piece> = Vec::new();
        let mut current: Vec<Piece> = vec![Piece::Range(bs, be)];
        let mut current_plain = self.is_plain(base);
        let mut this_value: Option<Vec<Piece>> = if matches!(ast[base].kind, NodeKind::Super) { Some(vec![text("this")]) } else { None };
        let mut last_private = false;
        for (i, link) in links.iter().enumerate() {
            let next_is_optional_call = links.get(i + 1).is_some_and(|l| l.call && l.optional);
            if link.optional {
                if current_plain {
                    prefix.extend(current.clone());
                    prefix.push(text(&format!(" == null ? {none} : ")));
                } else {
                    let t = self.temp();
                    prefix.push(text(&format!("({t} = (")));
                    prefix.extend(current);
                    prefix.push(text(&format!(")) == null ? {none} : ")));
                    current = vec![text(&t)];
                    current_plain = true;
                }
            }
            if !link.call {
                if let Some(info) = self.private_ref(link.node).cloned() {
                    // A private member: read through the helper, and a call after it keeps `this` with `.call`.
                    let next_is_call = links.get(i + 1).is_some_and(|l| l.call);
                    let mut object = current;
                    let mut this_text = None;
                    if next_is_call {
                        if current_plain {
                            this_text = Some(object.clone());
                        } else {
                            let t = self.temp();
                            let mut captured = vec![text(&format!("({t} = ("))];
                            captured.extend(object);
                            captured.push(text("))"));
                            object = captured;
                            this_text = Some(vec![text(&t)]);
                        }
                    }
                    current = self.private_read(&info, object);
                    current_plain = false;
                    this_value = this_text;
                    last_private = true;
                    continue;
                }
            }
            if link.call {
                let mut next = Vec::new();
                match this_value.take() {
                    // A call of a method keeps `this` through `.call` when the callee has been put in a name.
                    Some(this) if link.optional || last_private => {
                        next.extend(current);
                        next.push(text(".call("));
                        next.extend(this);
                        if link.has_arguments {
                            next.push(text(", "));
                            next.extend(link.arguments.clone());
                        }
                        next.push(text(")"));
                    }
                    _ => {
                        next.extend(current);
                        next.extend(link.tail.clone());
                    }
                }
                current = next;
                current_plain = false;
                this_value = None;
                last_private = false;
            } else {
                last_private = false;
                // A member: its object is the `this` of a call that follows with `?.`, so it must be evaluated once.
                let mut object = current;
                let mut this_text = None;
                if next_is_optional_call && !current_plain {
                    let t = self.temp();
                    let mut captured = vec![text(&format!("({t} = ("))];
                    captured.extend(object);
                    captured.push(text("))"));
                    object = captured;
                    this_text = Some(vec![text(&t)]);
                } else if next_is_optional_call || matches!(ast[base].kind, NodeKind::Super) && i == 0 {
                    // The object is a name already: `this` for the call is that name.
                    this_text = Some(if matches!(ast[base].kind, NodeKind::Super) && i == 0 { vec![text("this")] } else { object.clone() });
                }
                let mut next = object;
                next.extend(link.tail.clone());
                current = next;
                current_plain = false;
                this_value = this_text;
            }
            let _ = link.node;
        }
        let mut out = vec![text("(")];
        out.extend(prefix);
        if delete {
            out.push(text("delete "));
        }
        out.extend(current);
        out.push(text(")"));
        out
    }

    // ---- object spread and rest ----

    /// `{ a, ...b, c }` as `__spreadProps(__spreadValues({ a }, b), { c })`: each spread copies what is enumerable, and the plain
    /// properties after one are defined (so getters and methods stay what they are).
    fn lower_object_spread(&mut self, node: NodeId, props: List) {
        let ast = self.ast;
        enum Segment {
            Run(u32, u32),
            Spread(NodeId),
        }
        let mut segments: Vec<Segment> = Vec::new();
        let mut run: Option<(u32, u32)> = None;
        let mut spread_seen = false;
        for &p in ast.list(props) {
            // `__proto__: x` sets the prototype of the object it is written in, which a copy after a spread cannot do.
            if spread_seen && matches!(ast[p].kind, NodeKind::Property { key, kind: crate::ast::PropKind::Init, .. } if matches!(ast[key].kind, NodeKind::PropName(a) if ast.atom(a) == "__proto__") && ast[p].flags & (crate::ast::flags::COMPUTED | crate::ast::flags::SHORTHAND) == 0) {
                let target = self.bundler.target.name();
                self.lower_error_message(p, format!("a __proto__ property after a spread cannot be rewritten for the target {target} (put it before the spread, or use Object.setPrototypeOf)"));
                return;
            }
            if let NodeKind::Spread(argument) = ast[p].kind {
                spread_seen = true;
                if let Some((a, b)) = run.take() {
                    segments.push(Segment::Run(a, b));
                }
                segments.push(Segment::Spread(argument));
            } else {
                let (start, end) = self.span(p);
                run = Some(match run {
                    Some((a, _)) => (a, end),
                    None => (start, end),
                });
            }
        }
        if let Some((a, b)) = run {
            segments.push(Segment::Run(a, b));
        }
        self.bundler.helpers.spread = true;
        let mut acc: Vec<Piece> = vec![text("{}")];
        let mut fresh = true;
        for segment in segments {
            match segment {
                Segment::Run(a, b) if fresh => acc = vec![text("{ "), Piece::Range(a, b), text(" }")],
                Segment::Run(a, b) => {
                    let mut next = vec![text("__spreadProps(")];
                    next.extend(acc);
                    next.extend([text(", { "), Piece::Range(a, b), text(" })")]);
                    acc = next;
                }
                Segment::Spread(argument) => {
                    let (a, b) = self.span(argument);
                    let mut next = vec![text("__spreadValues(")];
                    next.extend(acc);
                    next.extend([text(", ("), Piece::Range(a, b), text("))")]);
                    acc = next;
                }
            }
            fresh = false;
        }
        self.push(node, acc);
    }

    /// Whether a pattern is an object pattern that ends with `...rest`.
    fn has_object_rest(&self, pattern: NodeId) -> bool {
        match self.ast[pattern].kind {
            NodeKind::ObjectPattern(props) => self.ast.list(props).iter().any(|&p| matches!(self.ast[p].kind, NodeKind::Rest(_))),
            _ => false,
        }
    }

    /// Whether a pattern has an object rest anywhere in it: `{ ...r }`, `{ a: { ...r } }`, `[{ ...r }]`, `{ a: [x, { ...r }] = d }`.
    fn contains_object_rest(&self, pattern: NodeId) -> bool {
        let ast = self.ast;
        match ast[pattern].kind {
            NodeKind::ObjectPattern(props) => ast.list(props).iter().any(|&p| match ast[p].kind {
                NodeKind::Rest(_) => true,
                NodeKind::PatternProp { value, .. } => self.contains_object_rest(value),
                _ => false,
            }),
            NodeKind::ArrayPattern(elements) => ast.list(elements).iter().any(|&e| self.contains_object_rest(e)),
            NodeKind::AssignPattern { target, .. } | NodeKind::Rest(target) => self.contains_object_rest(target),
            _ => false,
        }
    }

    /// A name for something the rewrite binds: it is declared by the declaration it is part of, so a call has its own.
    fn binding_name(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.bundler.temp_count);
        self.bundler.temp_count += 1;
        name
    }

    /// A name for a part of a pattern rewrite: a binding of the declaration it is in, or, in an assignment, a `var` of the chunk.
    fn part_name(&mut self, prefix: &str, assign: bool) -> String {
        if assign { self.temp() } else { self.binding_name(prefix) }
    }

    /// A pattern with an object rest in it, as declarators that read `source` (a name): the keys that are computed first, then the pattern
    /// with each part that has a rest in it replaced by a name, then the rest itself (`__objRest(source, [the keys before it])`), then each
    /// of those parts, rewritten the same way. They go in one declaration (`var`, `let` or `const`), after the declarator that holds the
    /// value. A nested pattern is read after the outer one is done, not in turn with it. `None` after an error.
    fn rest_declarators(&mut self, pattern: NodeId, source: &str, assign: bool) -> Option<Vec<Vec<Piece>>> {
        let ast = self.ast;
        self.skip.insert(pattern);
        let mut before: Vec<Vec<Piece>> = Vec::new();
        let mut after: Vec<Vec<Piece>> = Vec::new();
        // A part of a pattern that has a rest in it becomes a name (with its default), and its own declarators follow.
        let nested = |this: &mut Self, part: NodeId, after: &mut Vec<Vec<Piece>>| -> Option<Vec<Piece>> {
            let (inner, default) = match ast[part].kind {
                NodeKind::AssignPattern { target, default } => (target, Some(default)),
                _ => (part, None),
            };
            let name = this.part_name("__rkp", assign);
            after.extend(this.rest_declarators(inner, &name, assign)?);
            let mut pieces = vec![text(&name)];
            if let Some(default) = default {
                let (ds, de) = this.span(default);
                pieces.extend([text(" = "), Piece::Range(ds, de)]);
            }
            Some(pieces)
        };
        match ast[pattern].kind {
            NodeKind::ObjectPattern(props) => {
                let has_rest = ast.list(props).iter().any(|&p| matches!(ast[p].kind, NodeKind::Rest(_)));
                let mut kept: Vec<Vec<Piece>> = Vec::new();
                let mut keys: Vec<String> = Vec::new();
                let mut rest = None;
                for &p in ast.list(props) {
                    match ast[p].kind {
                        NodeKind::Rest(target) if assign || matches!(ast[target].kind, NodeKind::BindingIdent(_)) => rest = Some(target),
                        NodeKind::PatternProp { key, value } => {
                            let computed = ast[p].flags & crate::ast::flags::COMPUTED != 0;
                            let mut property: Option<Vec<Piece>> = None;
                            if computed && has_rest {
                                // Evaluated once, first, so the rest can leave it out.
                                let name = self.part_name("__rkk", assign);
                                let (ks, ke) = self.span(key);
                                before.push(vec![text(&format!("{name} = (")), Piece::Range(ks, ke), text(")")]);
                                keys.push(name.clone());
                                property = Some(vec![text(&format!("[{name}]: "))]);
                            } else if !computed && has_rest {
                                let exclusion = match ast[key].kind {
                                    NodeKind::PropName(a) | NodeKind::Str(a) => json_string(ast.atom(a)),
                                    NodeKind::Num(_) => {
                                        let (ks, ke) = self.span(key);
                                        let literal = self.source_between(ks, ke).replace('_', "");
                                        match literal.parse::<f64>() {
                                            Ok(n) if n.fract() == 0.0 && n.abs() < 1e15 => json_string(&format!("{}", n as i64)),
                                            _ => {
                                                self.lower_error(Feature::ObjectRest, pattern);
                                                return None;
                                            }
                                        }
                                    }
                                    _ => {
                                        self.lower_error(Feature::ObjectRest, pattern);
                                        return None;
                                    }
                                };
                                keys.push(exclusion);
                            }
                            if self.contains_object_rest(value) {
                                let (ks, ke) = self.span(key);
                                let mut pieces = property.unwrap_or_else(|| if computed { vec![text("["), Piece::Range(ks, ke), text("]: ")] } else { vec![Piece::Range(ks, ke), text(": ")] });
                                pieces.extend(nested(self, value, &mut after)?);
                                kept.push(pieces);
                            } else if let Some(mut pieces) = property {
                                let (vs, ve) = self.span(value);
                                pieces.push(Piece::Range(vs, ve));
                                kept.push(pieces);
                            } else {
                                let (ps, pe) = self.span(p);
                                kept.push(vec![Piece::Range(ps, pe)]);
                            }
                        }
                        _ => {
                            self.lower_error(Feature::ObjectRest, pattern);
                            return None;
                        }
                    }
                }
                let mut declarators = before;
                if !kept.is_empty() {
                    let mut pieces = vec![text("{ ")];
                    for (i, part) in kept.into_iter().enumerate() {
                        if i > 0 {
                            pieces.push(text(", "));
                        }
                        pieces.extend(part);
                    }
                    pieces.push(text(&format!(" }} = {source}")));
                    if assign {
                        pieces.insert(0, text("("));
                        pieces.push(text(")"));
                    }
                    declarators.push(pieces);
                }
                if let Some(rest) = rest {
                    self.bundler.helpers.spread = true;
                    let (rs, re) = self.span(rest);
                    declarators.push(vec![Piece::Range(rs, re), text(&format!(" = __objRest({source}, [{}])", keys.join(", ")))]);
                }
                declarators.extend(after);
                Some(declarators)
            }
            NodeKind::ArrayPattern(elements) => {
                let mut pieces = vec![text("[")];
                for (i, &element) in ast.list(elements).iter().enumerate() {
                    if i > 0 {
                        pieces.push(text(", "));
                    }
                    match ast[element].kind {
                        NodeKind::Hole => {}
                        NodeKind::Rest(target) if self.contains_object_rest(target) => {
                            pieces.push(text("..."));
                            pieces.extend(nested(self, target, &mut after)?);
                        }
                        _ if self.contains_object_rest(element) => pieces.extend(nested(self, element, &mut after)?),
                        _ => {
                            let (es, ee) = self.span(element);
                            pieces.push(Piece::Range(es, ee));
                        }
                    }
                }
                pieces.push(text(&format!("] = {source}")));
                if assign {
                    pieces.insert(0, text("("));
                    pieces.push(text(")"));
                }
                let mut declarators = vec![pieces];
                declarators.extend(after);
                Some(declarators)
            }
            _ => {
                self.lower_error(Feature::ObjectRest, pattern);
                None
            }
        }
    }

    /// The declarators joined by commas.
    fn joined(declarators: Vec<Vec<Piece>>) -> Vec<Piece> {
        let mut out = Vec::new();
        for (i, declarator) in declarators.into_iter().enumerate() {
            if i > 0 {
                out.push(text(", "));
            }
            out.extend(declarator);
        }
        out
    }

    /// `const { a, ...rest } = init` as `const __rks = (init), { a } = __rks, rest = __objRest(__rks, ["a"])`.
    fn lower_rest_declarator(&mut self, node: NodeId, pattern: NodeId, init: NodeId) {
        let source = self.binding_name("__rks");
        let Some(declarators) = self.rest_declarators(pattern, &source, false) else { return };
        let (is, ie) = self.span(init);
        let mut pieces = vec![text(&format!("{source} = (")), Piece::Range(is, ie), text("), ")];
        pieces.extend(Self::joined(declarators));
        self.push(node, pieces);
    }

    /// The declarator of `for (const { a, ...rest } of list)`, when that is what the left side is.
    fn for_of_rest(&self, left: NodeId) -> Option<NodeId> {
        let NodeKind::VarDecl { decls, .. } = self.ast[left].kind else { return None };
        let [only] = self.ast.list(decls) else { return None };
        match self.ast[*only].kind {
            NodeKind::VarDeclarator { target, init: None } if self.contains_object_rest(target) => Some(target),
            _ => None,
        }
    }

    /// `for (const { a, ...rest } of list) body` as `for (const __rkarg0 of list) { const { a } = __rkarg0, rest = __objRest(...); body }`.
    fn lower_rest_for_of(&mut self, node: NodeId, left: NodeId, body: NodeId) {
        let ast = self.ast;
        let Some(pattern) = self.for_of_rest(left) else { return };
        let NodeKind::VarDecl { kind, .. } = ast[left].kind else { return };
        let _ = node;
        let name = self.argument_name();
        let Some(declarators) = self.rest_declarators(pattern, &name, false) else { return };
        let mut declaration = vec![text(kind.as_str()), text(" ")];
        declaration.extend(Self::joined(declarators));
        declaration.push(text(";"));
        let (ps, pe) = self.span(pattern);
        self.edits.push(Edit { start: ps, end: pe, pieces: vec![text(&name)], shorthand: None, transparent: false });
        let (bs, be) = self.span(body);
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            self.edits.push(Edit { start: bs + 1, end: bs + 1, pieces: [vec![text(" ")], declaration].concat(), shorthand: None, transparent: false });
        } else {
            self.edits.push(Edit { start: bs, end: bs, pieces: [vec![text("{ ")], declaration, vec![text(" ")]].concat(), shorthand: None, transparent: false });
            self.edits.push(Edit { start: be, end: be, pieces: vec![text(" }")], shorthand: None, transparent: false });
        }
    }

    /// `({ a, ...rest } = value)` as `(T = (value), ({ a } = T), rest = __objRest(T, ["a"]), T)`: the value of the assignment is the value.
    fn lower_rest_assignment(&mut self, node: NodeId, target: NodeId, value: NodeId) {
        let source = self.temp();
        let Some(parts) = self.rest_declarators(target, &source, true) else { return };
        let (vs, ve) = self.span(value);
        let mut pieces = vec![text(&format!("({source} = (")), Piece::Range(vs, ve), text("), ")];
        pieces.extend(Self::joined(parts));
        pieces.push(text(&format!(", {source})")));
        self.push(node, pieces);
    }

    /// `catch ({ a, ...rest }) { body }` as `catch (__rkarg0) { let { a } = __rkarg0, rest = __objRest(...); body }`.
    fn lower_rest_catch(&mut self, param: NodeId, body: NodeId) {
        let ast = self.ast;
        let name = self.argument_name();
        let Some(declarators) = self.rest_declarators(param, &name, false) else { return };
        let (ps, pe) = self.span(param);
        self.edits.push(Edit { start: ps, end: pe, pieces: vec![text(&name)], shorthand: None, transparent: false });
        let (bs, _) = self.span(body);
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            let mut declaration = vec![text(" let ")];
            declaration.extend(Self::joined(declarators));
            declaration.push(text(";"));
            self.edits.push(Edit { start: bs + 1, end: bs + 1, pieces: declaration, shorthand: None, transparent: false });
        }
    }

    /// A name for a parameter that stands in for a pattern.
    fn argument_name(&mut self) -> String {
        let name = format!("__rkarg{}", self.bundler.temp_count);
        self.bundler.temp_count += 1;
        name
    }

    /// Parameters that are `{ a, ...rest }` (or that with a default) become a name, and the body starts by destructuring it.
    fn lower_rest_params(&mut self, function: NodeId, params: List, body: NodeId) {
        let ast = self.ast;
        let mut statements: Vec<Piece> = Vec::new();
        let mut last_end = 0;
        for &param in ast.list(params) {
            let pattern = match ast[param].kind {
                NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) if self.contains_object_rest(param) => param,
                NodeKind::AssignPattern { target, .. } if self.contains_object_rest(target) => target,
                _ => continue,
            };
            let name = self.argument_name();
            let Some(declarators) = self.rest_declarators(pattern, &name, false) else { continue };
            statements.push(text("var "));
            statements.extend(Self::joined(declarators));
            statements.push(text("; "));
            let (ps, pe) = self.span(pattern);
            self.edits.push(Edit { start: ps, end: pe, pieces: vec![text(&name)], shorthand: None, transparent: false });
            last_end = self.span(param).1;
        }
        if statements.is_empty() {
            return;
        }
        let (bs, _) = self.span(body);
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            // After the directives (`"use strict"`) that begin the body.
            let NodeKind::Block(list) = ast[body].kind else { return };
            let mut at = bs + 1;
            for &statement in ast.list(list) {
                match ast[statement].kind {
                    NodeKind::ExprStmt(_) if ast[statement].flags & crate::ast::flags::DIRECTIVE != 0 => at = ast[statement].span.end,
                    _ => break,
                }
            }
            self.edits.push(Edit { start: at, end: at, pieces: [vec![text(" ")], statements].concat(), shorthand: None, transparent: false });
        } else {
            // An arrow with an expression for a body: `=> expression` becomes `=> { var ...; return expression; }`.
            let region = self.source_between(last_end, bs);
            let Some(arrow) = region.rfind("=>") else {
                self.lower_error(Feature::ObjectRest, function);
                return;
            };
            let at = last_end + arrow as u32 + 2;
            let end = ast[function].span.end;
            self.edits.push(Edit { start: at, end: at, pieces: [vec![text(" { ")], statements, vec![text("return ")]].concat(), shorthand: None, transparent: false });
            self.edits.push(Edit { start: end, end, pieces: vec![text("; }")], shorthand: None, transparent: false });
        }
    }

    /// The `for await` a (possibly nested) label is on.
    fn labeled_for_await(&self, mut node: NodeId) -> Option<NodeId> {
        while let NodeKind::Labeled { body, .. } = self.ast[node].kind {
            node = body;
        }
        (matches!(self.ast[node].kind, NodeKind::ForOf { .. }) && self.ast[node].flags & crate::ast::flags::AWAIT != 0).then_some(node)
    }

    fn is_async_method(&self, value: NodeId) -> bool {
        matches!(self.ast[value].kind, NodeKind::Function { .. }) && self.async_lowered(self.ast[value].flags)
    }

    /// `async name() {}` in a class or an object: the keyword is somewhere before the key.
    fn lower_async_method(&mut self, method: NodeId, key: NodeId, value: NodeId) {
        let from = self.ast[method].span.start;
        let to = self.ast[key].span.start;
        let region = self.source_between(from, to);
        let found = region.match_indices("async").find(|&(i, _)| {
            let after = region.as_bytes().get(i + 5).copied().unwrap_or(b' ');
            !(after.is_ascii_alphanumeric() || after == b'_' || after == b'$')
        });
        let Some((i, _)) = found else {
            self.lower_error(Feature::AsyncFunction, value);
            return;
        };
        let star = region[i + 5..].find('*').map(|j| from + (i + 5 + j) as u32);
        self.skip.insert(value);
        self.lower_async(value, from + i as u32, star);
    }

    /// An async function becomes a plain one that runs a generator through `__async` (or `__asyncGenerator`), each `await` a `yield`.
    /// `star` is where the `*` of a method is (a function's is looked for after its keyword).
    fn lower_async(&mut self, function: NodeId, async_at: u32, star: Option<u32>) {
        let ast = self.ast;
        let NodeKind::Function { body, .. } = ast[function].kind else { return };
        let generator = ast[function].flags & crate::ast::flags::GENERATOR != 0;
        let arrow = ast[function].has(crate::ast::flags::ARROW);
        let uses_super = self.contains_super(body);
        // An arrow reads `super` through the accessors of the method around it; one that has none (a plain method, a field) gets
        // accessors made where the arrow is, which is where `super` means what it should.
        let wrap_super = uses_super && arrow && !self.async_ctx.last().is_some_and(|c| c.super_ok);
        if generator {
            self.bundler.helpers.async_gen = true;
        } else {
            self.bundler.helpers.async_fn = true;
        }
        // `async` and the space after it.
        let rest = self.source_between(async_at + 5, ast[function].span.end);
        let after = rest.len() - rest.trim_start().len();
        let star = star.or_else(|| {
            if !generator {
                return None;
            }
            let header = self.source_between(async_at, ast[function].span.end);
            header[..header.find('(')?].find('*').map(|j| async_at + j as u32)
        });
        let prefix = if wrap_super { vec![text("((__superGet, __superSet) => ")] } else { Vec::new() };
        let suffix = if wrap_super { ")((k) => super[k], (k, v) => super[k] = v)" } else { "" };
        let expression_body = !matches!(ast[body].kind, NodeKind::Block(_));
        // An expression body is wrapped by one edit that holds the body as a range: edits at the body's own first and last offsets would
        // apply twice where a rewrite of the body (`??`, `?.`) reads it back through a range of its own.
        if !expression_body {
            self.edits.push(Edit { start: async_at, end: async_at + 5 + after as u32, pieces: prefix.clone(), shorthand: None, transparent: false });
        }
        if let (true, Some(star)) = (generator, star) {
            self.edits.push(Edit { start: star, end: star + 1, pieces: Vec::new(), shorthand: None, transparent: false });
        }
        let (bs, be) = self.span(body);
        let arguments = if !arrow || self.source_between(bs, be).contains("arguments") { "arguments" } else { "null" };
        let driver = if generator { "__asyncGenerator" } else { "__async" };
        let open = format!("{driver}(this, {arguments}, function* () ");
        let accessor = if uses_super && !arrow { SUPER_ACCESSORS.trim_start() } else { "" };
        // The temporaries of the writes to `super` properties, declared in the generator so each call has its own.
        let sites = self.count_super_sites(body);
        let owner = ast_index(function);
        let declare = if sites == 0 { String::new() } else { format!("var {}; ", (0..sites * 2).map(|i| format!("__sk{owner}_{i}")).collect::<Vec<_>>().join(", ")) };
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            self.edits.push(Edit { start: bs, end: bs, pieces: vec![text(&format!("{{ {accessor}return {open}"))], shorthand: None, transparent: false });
            if !declare.is_empty() {
                let NodeKind::Block(list) = ast[body].kind else { return };
                let mut at = bs + 1;
                for &statement in ast.list(list) {
                    match ast[statement].kind {
                        NodeKind::ExprStmt(_) if ast[statement].flags & crate::ast::flags::DIRECTIVE != 0 => at = ast[statement].span.end,
                        _ => break,
                    }
                }
                self.edits.push(Edit { start: at, end: at, pieces: vec![text(&format!(" {declare}"))], shorthand: None, transparent: false });
            }
            // Replacing the closing brace, rather than adding after it, keeps the edit inside a method that is moved elsewhere.
            self.edits.push(Edit { start: be - 1, end: be, pieces: vec![text(&format!("}} ); }}{suffix}"))], shorthand: None, transparent: false });
        } else {
            let function_end = ast[function].span.end;
            let mut pieces = prefix;
            pieces.extend([Piece::Range(async_at + 5 + after as u32, bs), text(&format!("{open}{{ {declare}return ")), Piece::Range(bs, be), text("; })")]);
            if function_end > be {
                pieces.push(Piece::Range(be, function_end));
            }
            if !suffix.is_empty() {
                pieces.push(text(suffix));
            }
            self.edits.push(Edit { start: async_at, end: function_end.max(be), pieces, shorthand: None, transparent: false });
        }
    }

    /// How many writes to a `super` property in this function need temporaries (not counting a function of its own or an arrow that is rewritten).
    fn count_super_sites(&self, node: NodeId) -> usize {
        let ast = self.ast;
        let mut count = 0;
        let mut children = Vec::new();
        ast.for_each_child(node, |c| children.push(c));
        for c in children {
            if let NodeKind::Function { .. } = ast[c].kind {
                if !ast[c].has(crate::ast::flags::ARROW) || self.async_lowered(ast[c].flags) {
                    continue;
                }
            }
            count += self.super_site_needs_temps(c) as usize + self.count_super_sites(c);
        }
        count
    }

    fn super_site_needs_temps(&self, node: NodeId) -> bool {
        let ast = self.ast;
        let super_member = |n: NodeId| matches!(ast[n].kind, NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Super));
        match ast[node].kind {
            NodeKind::Assign { op, target, .. } if super_member(target) => match op {
                AssignOp::Assign => false,
                AssignOp::Op(BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish) => true,
                AssignOp::Op(_) => ast[target].has(crate::ast::flags::COMPUTED),
            },
            NodeKind::Update { arg, .. } if super_member(arg) => ast[arg].has(crate::ast::flags::COMPUTED) || !ast[node].has(crate::ast::flags::PREFIX),
            _ => false,
        }
    }

    /// Whether `super` is used in the function (not in a function of its own, but in an arrow it is).
    fn contains_super(&self, node: NodeId) -> bool {
        let ast = self.ast;
        if matches!(ast[node].kind, NodeKind::Super) {
            return true;
        }
        if matches!(ast[node].kind, NodeKind::Function { .. }) && !ast[node].has(crate::ast::flags::ARROW) {
            return false;
        }
        let mut found = false;
        ast.for_each_child(node, |c| found = found || self.contains_super(c));
        found
    }

    /// `super.x` and `super.x(...)` inside a rewritten async method: the generator that runs it is no method, so they go through
    /// `__superGet`, which the method defines. Writing to a `super` property is refused.
    fn lower_super(&mut self, node: NodeId) {
        let ast = self.ast;
        let super_member = |n: NodeId| match ast[n].kind {
            NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Super) => true,
            _ => false,
        };
        match ast[node].kind {
            NodeKind::Assign { op, target, value } if super_member(target) => {
                self.skip.insert(target);
                let needs = self.super_site_needs_temps(node);
                let computed = ast[target].has(crate::ast::flags::COMPUTED);
                let (t1, t2) = if needs { self.site_temps() } else { (String::new(), String::new()) };
                // The key: written once, or kept in a temporary for the second use.
                let (decl, key): (Vec<Piece>, Vec<Piece>) = if computed && op != AssignOp::Assign {
                    let (ps, pe) = self.span(match ast[target].kind {
                        NodeKind::Member { prop, .. } => prop,
                        _ => target,
                    });
                    (vec![text(&format!("{t1} = ")), Piece::Range(ps, pe), text(", ")], vec![text(&t1)])
                } else {
                    (Vec::new(), self.super_key(target))
                };
                let (vs, ve) = self.span(value);
                let v = vec![text("("), Piece::Range(vs, ve), text(")")];
                let get = [vec![text("__superGet(")], key.clone(), vec![text(")")]].concat();
                let set = |value: Vec<Piece>| [vec![text("__superSet(")], key.clone(), vec![text(", ")], value, vec![text(")")]].concat();
                let body = match op {
                    AssignOp::Assign => set(v),
                    AssignOp::Op(BinOp::Exp) if self.lowers(Feature::Exponent) => set([vec![text("Math.pow(")], get, vec![text(", ")], v, vec![text(")")]].concat()),
                    AssignOp::Op(BinOp::LogOr) => [vec![text(&format!("({t2} = "))], get.clone(), vec![text(&format!(") ? {t2} : "))], set(v)].concat(),
                    AssignOp::Op(BinOp::LogAnd) => [vec![text(&format!("({t2} = "))], get.clone(), vec![text(") ? ")], set(v), vec![text(&format!(" : {t2}"))]].concat(),
                    AssignOp::Op(BinOp::Nullish) => [vec![text(&format!("({t2} = "))], get.clone(), vec![text(&format!(") != null ? {t2} : "))], set(v)].concat(),
                    AssignOp::Op(op) => set([get, vec![text(&format!(" {} ", op.as_str()))], v].concat()),
                };
                let (s, e) = self.span(node);
                self.edits.push(Edit { start: s, end: e, pieces: [vec![text("(")], decl, body, vec![text(")")]].concat(), shorthand: None, transparent: false });
            }
            NodeKind::Update { op, arg } if super_member(arg) => {
                self.skip.insert(arg);
                let (t1, t2) = if self.super_site_needs_temps(node) { self.site_temps() } else { (String::new(), String::new()) };
                let computed = ast[arg].has(crate::ast::flags::COMPUTED);
                let (decl, key): (Vec<Piece>, Vec<Piece>) = if computed {
                    let NodeKind::Member { prop, .. } = ast[arg].kind else { return };
                    let (ps, pe) = self.span(prop);
                    (vec![text(&format!("{t1} = ")), Piece::Range(ps, pe), text(", ")], vec![text(&t1)])
                } else {
                    (Vec::new(), self.super_key(arg))
                };
                let sign = if op == crate::ast::UpdateOp::Inc { "+" } else { "-" };
                let k: Vec<Piece> = key;
                let pieces = if ast[node].has(crate::ast::flags::PREFIX) {
                    [vec![text("(")], decl, vec![text("__superSet(")], k.clone(), vec![text(", +__superGet(")], k, vec![text(&format!(") {sign} 1))"))]].concat()
                } else {
                    [vec![text("(")], decl, vec![text(&format!("{t2} = +__superGet("))], k.clone(), vec![text("), __superSet(")], k, vec![text(&format!(", {t2} {sign} 1), {t2})"))]].concat()
                };
                let (s, e) = self.span(node);
                self.edits.push(Edit { start: s, end: e, pieces, shorthand: None, transparent: false });
            }
            NodeKind::Call { callee, args } if super_member(callee) => {
                self.skip.insert(callee);
                let key = self.super_key(callee);
                let (cs, ce) = self.span(callee);
                self.edits.push(Edit { start: cs, end: ce, pieces: [vec![text("__superGet(")], key, vec![text(").call")]].concat(), shorthand: None, transparent: false });
                let end = ast[node].span.end;
                if let Some(open) = self.source_between(ce, end).find('(') {
                    let at = ce + open as u32 + 1;
                    let sep = if ast.list(args).is_empty() { "this" } else { "this, " };
                    self.edits.push(Edit { start: at, end: at, pieces: vec![text(sep)], shorthand: None, transparent: false });
                }
            }
            NodeKind::Member { .. } if super_member(node) && !self.skip.contains(&node) => {
                let key = self.super_key(node);
                let (s, e) = self.span(node);
                self.edits.push(Edit { start: s, end: e, pieces: [vec![text("__superGet(")], key, vec![text(")")]].concat(), shorthand: None, transparent: false });
            }
            _ => {}
        }
    }

    /// The next two temporaries of the rewritten function that owns the code being scanned.
    fn site_temps(&mut self) -> (String, String) {
        let owner = self.async_ctx.last().map_or(0, |c| c.owner);
        let next = self.site_next.entry(owner).or_insert(0);
        let names = (format!("__sk{owner}_{}", *next), format!("__sk{owner}_{}", *next + 1));
        *next += 2;
        names
    }

    /// The property of a `super.name` or `super[expression]`, as an expression.
    fn super_key(&self, member: NodeId) -> Vec<Piece> {
        let ast = self.ast;
        let NodeKind::Member { prop, .. } = ast[member].kind else { return Vec::new() };
        match ast[prop].kind {
            NodeKind::PropName(a) | NodeKind::Ident(a) if !ast[member].has(crate::ast::flags::COMPUTED) => vec![Piece::Text(json_string(ast.atom(a)))],
            _ => {
                let (s, e) = self.span(prop);
                vec![Piece::Range(s, e)]
            }
        }
    }

    /// `for await (left of right) body` as a loop over the iterator's `next()`, each result waited for, that closes the iterator when the
    /// loop is left early or throws. The names are `var`s of the function, so concurrent calls do not share them.
    fn lower_for_await(&mut self, node: NodeId, left: NodeId, right: NodeId, body: NodeId, label_from: Option<(u32, u32)>) {
        let ast = self.ast;
        let (for_start, for_end) = self.span(node);
        // Labels go on the loop, inside the `try`, so that `continue label` still names it.
        let (start, end) = (label_from.map_or(for_start, |l| l.0), label_from.map_or(for_end, |l| l.1));
        let labels = self.source_between(start, for_start).to_string();
        let n = self.bundler.temp_count;
        self.bundler.temp_count += 1;
        let (it, more, value, err, e) = (format!("__rkIt{n}"), format!("__rkMore{n}"), format!("__rkVal{n}"), format!("__rkErr{n}"), format!("__rkE{n}"));
        self.bundler.helpers.for_await = true;
        let (rs, re) = self.span(right);
        let (ls, le) = self.span(left);
        let next = self.await_pieces(vec![text(&format!("{it}.next()"))]);
        let close = self.await_pieces(vec![text(&format!("{value}.call({it})"))]);
        let mut pieces = vec![text(&format!("try {{ {labels}for (var {it} = __forAwait(")), Piece::Range(rs, re), text(&format!("), {more}, {value}, {err}; {more} = !({value} = "))];
        pieces.extend(next);
        pieces.push(text(&format!(").done; {more} = false) {{ ")));
        if matches!(ast[left].kind, NodeKind::VarDecl { .. }) {
            pieces.extend([Piece::Range(ls, le), text(&format!(" = {value}.value; "))]);
        } else {
            pieces.extend([text("("), Piece::Range(ls, le), text(&format!(" = {value}.value); "))]);
        }
        let (bs, be) = self.span(body);
        if matches!(ast[body].kind, NodeKind::Block(_)) {
            pieces.push(Piece::Range(bs + 1, be - 1));
        } else {
            pieces.push(Piece::Range(bs, be));
        }
        pieces.push(text(&format!(" }} }} catch ({e}) {{ {err} = [{e}]; }} finally {{ try {{ {more} && ({value} = {it}.return) && ")));
        pieces.extend(close);
        pieces.push(text(&format!("; }} finally {{ if ({err}) throw {err}[0]; }} }}")));
        self.edits.push(Edit { start, end, pieces, shorthand: None, transparent: false });
    }
}
