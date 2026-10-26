//! `--target`: syntax the target's language does not have is written another way, as the scanner meets it.
//!
//! Each rewrite replaces a node with text made of its pieces (`Piece::Range` of the parts that stay, which still get their own rewrites and
//! renames) and a few words. A value that is used twice is a plain name, or is first put in a temporary, a `var` declared once at the top
//! of the file (`__rk0`, `__rk1`, ...): nothing runs between the assignment and the last read of one, so a recursive call cannot see
//! another's. What cannot be rewritten (yet) stops the build with a message that names it and the target, and is never passed through.

use super::*;
use crate::ast::{AssignOp, BinOp};
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

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
    fn lowers(&self, feature: Feature) -> bool {
        !self.bundler.target.supports(feature)
    }

    fn lower_error_message(&mut self, node: NodeId, message: String) {
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
    fn temp(&mut self) -> String {
        let name = format!("__rk{}", self.bundler.temp_count);
        self.bundler.temp_count += 1;
        self.bundler.temps.push((self.module.id, name.clone()));
        name
    }

    fn source_between(&self, from: u32, to: u32) -> &str {
        self.module.source.get(from as usize..to as usize).unwrap_or("")
    }

    /// An expression that is safe to write twice: a name or `this`.
    fn is_plain(&self, node: NodeId) -> bool {
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
        match ast[node].kind {
            NodeKind::Binary { op: BinOp::Exp, left, right } if self.lowers(Feature::Exponent) => self.lower_exponent(node, left, right),
            NodeKind::Assign { op: AssignOp::Op(BinOp::Exp), target, value } if self.lowers(Feature::Exponent) => self.lower_exponent_assignment(node, target, value),
            NodeKind::Binary { op: BinOp::Nullish, left, right } if self.lowers(Feature::NullishCoalescing) => self.lower_nullish(node, left, right),
            NodeKind::Assign { op: AssignOp::Op(op @ (BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish)), target, value } if self.lowers(Feature::LogicalAssignment) => self.lower_logical_assignment(node, op, target, value),
            NodeKind::Chain(inner) if self.lowers(Feature::OptionalChain) && !self.skip.contains(&node) => {
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
                if flags & crate::ast::flags::ASYNC != 0 {
                    let feature = if flags & crate::ast::flags::GENERATOR != 0 { Feature::AsyncIteration } else { Feature::AsyncFunction };
                    if self.lowers(feature) {
                        self.lower_error(feature, node);
                    }
                }
                if self.lowers(Feature::ObjectRest) {
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
            // Not rewritten yet: stop instead of writing what the target cannot run.
            NodeKind::Object(props) if self.lowers(Feature::ObjectSpread) && ast.list(props).iter().any(|&p| matches!(ast[p].kind, NodeKind::Spread(_))) => self.lower_object_spread(node, props),
            // A pattern with `...rest` is rewritten where its parent does it (a declaration, a parameter, a `for of`); anywhere else it is refused.
            NodeKind::VarDeclarator { target, init: Some(init) } if self.has_object_rest(target) && self.lowers(Feature::ObjectRest) => self.lower_rest_declarator(node, target, init),
            NodeKind::ForOf { left, body, .. } if self.lowers(Feature::ObjectRest) && self.for_of_rest(left).is_some() => self.lower_rest_for_of(node, left, body),
            NodeKind::ObjectPattern(_) if self.has_object_rest(node) && self.lowers(Feature::ObjectRest) && !self.skip.contains(&node) => self.lower_error(Feature::ObjectRest, node),
            NodeKind::ClassField { key, .. } if self.lowers(Feature::ClassField) || (self.lowers(Feature::ClassPrivate) && matches!(ast[key].kind, NodeKind::PrivateName(_))) => {
                let feature = if matches!(ast[key].kind, NodeKind::PrivateName(_)) { Feature::ClassPrivate } else { Feature::ClassField };
                self.lower_error(feature, node);
            }
            NodeKind::ClassMethod { key, .. } if self.lowers(Feature::ClassPrivate) && matches!(ast[key].kind, NodeKind::PrivateName(_)) => self.lower_error(Feature::ClassPrivate, node),
            NodeKind::StaticBlock(_) if self.lowers(Feature::ClassStaticBlock) => self.lower_error(Feature::ClassStaticBlock, node),
            NodeKind::Binary { op: BinOp::In, left, .. } if self.lowers(Feature::ClassPrivate) && matches!(ast[left].kind, NodeKind::PrivateName(_)) => self.lower_error(Feature::ClassPrivate, node),
            NodeKind::ForOf { .. } if ast[node].flags & crate::ast::flags::AWAIT != 0 && self.lowers(Feature::AsyncIteration) => self.lower_error(Feature::AsyncIteration, node),
            NodeKind::Await(_) if self.fn_depth == 0 && self.lowers(Feature::TopLevelAwait) => self.lower_error(Feature::TopLevelAwait, node),
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
                    if matches!(ast[prop].kind, NodeKind::PrivateName(_)) {
                        return None;
                    }
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
            if link.call {
                let mut next = Vec::new();
                match this_value.take() {
                    // A call of a method keeps `this` through `.call` when the callee has been put in a name.
                    Some(this) if link.optional => {
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
            } else {
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

    /// The parts of `{ a, b: x, ...rest }`: the span of the plain properties, the keys that are left out of the rest, and the rest's name.
    /// `None` after an error (a computed key, or something this does not handle).
    fn rest_parts(&mut self, pattern: NodeId) -> Option<(Option<(u32, u32)>, Vec<String>, NodeId)> {
        let ast = self.ast;
        let NodeKind::ObjectPattern(props) = ast[pattern].kind else { return None };
        let mut plain: Option<(u32, u32)> = None;
        let mut keys = Vec::new();
        let mut rest = None;
        for &p in ast.list(props) {
            match ast[p].kind {
                NodeKind::Rest(target) if matches!(ast[target].kind, NodeKind::BindingIdent(_)) => rest = Some(target),
                NodeKind::PatternProp { key, .. } if ast[p].flags & crate::ast::flags::COMPUTED == 0 => {
                    let name = match ast[key].kind {
                        NodeKind::PropName(a) | NodeKind::Str(a) => json_string(ast.atom(a)),
                        _ => {
                            self.lower_error(Feature::ObjectRest, pattern);
                            return None;
                        }
                    };
                    keys.push(name);
                    let (start, end) = self.span(p);
                    plain = Some(match plain {
                        Some((a, _)) => (a, end),
                        None => (start, end),
                    });
                }
                _ => {
                    self.lower_error(Feature::ObjectRest, pattern);
                    return None;
                }
            }
        }
        let rest = rest?;
        self.skip.insert(pattern);
        Some((plain, keys, rest))
    }

    /// `const { a, ...rest } = init` as `const { a } = (T = init), rest = __objRest(T, ["a"])`.
    fn lower_rest_declarator(&mut self, node: NodeId, pattern: NodeId, init: NodeId) {
        let Some((plain, keys, rest)) = self.rest_parts(pattern) else { return };
        let (is, ie) = self.span(init);
        let (rs, re) = self.span(rest);
        self.bundler.helpers.spread = true;
        let mut pieces: Vec<Piece> = Vec::new();
        match plain {
            Some((a, b)) => {
                let t = self.temp();
                pieces.extend([text("{ "), Piece::Range(a, b), text(&format!(" }} = ({t} = (")), Piece::Range(is, ie), text("))")]);
                pieces.extend([text(", "), Piece::Range(rs, re), text(&format!(" = __objRest({t}, [{}])", keys.join(", ")))]);
            }
            None => {
                pieces.extend([Piece::Range(rs, re), text(" = __objRest("), Piece::Range(is, ie), text(", [])")]);
            }
        }
        self.push(node, pieces);
    }

    /// The declarator of `for (const { a, ...rest } of list)`, when that is what the left side is.
    fn for_of_rest(&self, left: NodeId) -> Option<NodeId> {
        let NodeKind::VarDecl { decls, .. } = self.ast[left].kind else { return None };
        let [only] = self.ast.list(decls) else { return None };
        match self.ast[*only].kind {
            NodeKind::VarDeclarator { target, init: None } if self.has_object_rest(target) => Some(target),
            _ => None,
        }
    }

    /// `for (const { a, ...rest } of list) body` as `for (const __rkarg0 of list) { const { a } = __rkarg0, rest = __objRest(...); body }`.
    fn lower_rest_for_of(&mut self, node: NodeId, left: NodeId, body: NodeId) {
        let ast = self.ast;
        let Some(pattern) = self.for_of_rest(left) else { return };
        let NodeKind::VarDecl { kind, .. } = ast[left].kind else { return };
        let Some((plain, keys, rest)) = self.rest_parts(pattern) else { return };
        let _ = node;
        let name = self.argument_name();
        let (rs, re) = self.span(rest);
        self.bundler.helpers.spread = true;
        let mut declaration = vec![text(kind.as_str()), text(" ")];
        if let Some((a, b)) = plain {
            declaration.extend([text("{ "), Piece::Range(a, b), text(&format!(" }} = {name}, "))]);
        }
        declaration.extend([Piece::Range(rs, re), text(&format!(" = __objRest({name}, [{}]);", keys.join(", ")))]);
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
                NodeKind::ObjectPattern(_) if self.has_object_rest(param) => param,
                NodeKind::AssignPattern { target, .. } if self.has_object_rest(target) => target,
                _ => continue,
            };
            let Some((plain, keys, rest)) = self.rest_parts(pattern) else { continue };
            let name = self.argument_name();
            let (rs, re) = self.span(rest);
            self.bundler.helpers.spread = true;
            statements.push(text("var "));
            if let Some((a, b)) = plain {
                statements.extend([text("{ "), Piece::Range(a, b), text(&format!(" }} = {name}, "))]);
            }
            statements.extend([Piece::Range(rs, re), text(&format!(" = __objRest({name}, [{}]); ", keys.join(", ")))]);
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
}
