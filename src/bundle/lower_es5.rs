//! `--target es5`, and engines that lack part of es2015: arrow functions, classes, `let` and `const`, template literals, destructuring, default
//! and rest parameters, spread, `for of` and the newer forms of object literals are written as es5.
//!
//! The rewrites are edits of the same kind `lower.rs` makes: a node is replaced by text made of the pieces that stay (`Piece::Range`, which
//! still get their own rewrites) and a few words. Names the rewrites need (`_$this`, `_$d0`) are declared where they are used, in the
//! function that uses them, so a recursive call cannot see another's. Where a rewrite needs a name outside any function (the strings of a
//! tagged template) it is a temporary of the chunk (`__rk0`), as in `lower.rs`. What cannot be written in es5 (generators and what is built on
//! them) stops the build with a message that names it and the target.
use super::*;
use crate::ast::PropKind;
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

/// A string literal that es5 can read (the line and paragraph separators are escaped).
fn js_string(s: &str) -> String {
    json_string(s)
}

/// `0b101` and `0o17` as decimal digits.
fn radix_number(literal: &str) -> Option<String> {
    let lower = literal.to_ascii_lowercase().replace('_', "");
    let (radix, digits) = if let Some(d) = lower.strip_prefix("0b") {
        (2, d)
    } else if let Some(d) = lower.strip_prefix("0o") {
        (8, d)
    } else {
        return None;
    };
    let mut value = 0f64;
    for c in digits.chars() {
        value = value * radix as f64 + c.to_digit(radix)? as f64;
    }
    Some(number_text(value))
}

/// The text of a string literal with `\u{1F600}` written as `😀`.
fn without_code_point_escapes(raw: &str) -> Option<String> {
    if !raw.contains("\\u{") {
        return None;
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            if chars[i + 1] == 'u' && chars.get(i + 2) == Some(&'{') {
                if let Some(close) = chars[i + 3..].iter().position(|&c| c == '}') {
                    let hex: String = chars[i + 3..i + 3 + close].iter().collect();
                    if let Some(cp) = u32::from_str_radix(&hex, 16).ok().filter(|&v| v <= 0x10FFFF) {
                        if cp > 0xFFFF {
                            let v = cp - 0x10000;
                            let _ = write!(out, "\\u{:04X}\\u{:04X}", 0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF));
                        } else {
                            let _ = write!(out, "\\u{cp:04X}");
                        }
                        i += close + 4;
                        continue;
                    }
                }
            }
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    Some(out)
}

/// What the arrow functions inside a function use from it.
#[derive(Default, Clone, Copy)]
pub(super) struct Uses {
    pub this: bool,
    pub arguments: bool,
    pub new_target: bool,
}

#[derive(Clone, Copy)]
struct Frame {
    function: Option<NodeId>,
    arrow: bool,
    /// The constructor of a class that extends another: `this` is what `super()` returned.
    derived_ctor: bool,
}

#[derive(Default)]
pub(super) struct State {
    frames: Vec<Frame>,
    /// Constructors of classes that extend another (their `this` is a variable).
    pub(super) derived: HashSet<NodeId>,
    /// Assignments whose value nobody reads.
    pub(super) unused: HashSet<NodeId>,
    /// Nodes a rewrite has already taken over.
    pub(super) handled: HashSet<NodeId>,
    /// The `return`, `break` and `continue` already rewritten for the function that is the body of a loop.
    pub(super) exit_edits: HashSet<NodeId>,
    /// The bodies of loops that are functions (start, end, how many functions deep).
    pub(super) active_wraps: Vec<(u32, u32, usize)>,
    /// For the functions of a class that is rewritten: what `super` is there.
    pub(super) homes: HashMap<NodeId, super::lower_es5_class::Home>,
    /// What `let` and `const` need in this module (found when first wanted).
    pub(super) blocks: Option<super::lower_es5_blocks::Blocks>,
}

impl Scanner<'_, '_> {
    /// The range of a node, in parentheses if it was written in them (the node's span does not hold them).
    pub(super) fn rng(&self, node: NodeId) -> Vec<Piece> {
        let (a, b) = self.span(node);
        if self.ast[node].parenthesized() { vec![text("("), Piece::Range(a, b), text(")")] } else { vec![Piece::Range(a, b)] }
    }

    /// A range in parentheses always, for a place where an expression of any kind must stand alone.
    pub(super) fn rng_wrapped(&self, node: NodeId) -> Vec<Piece> {
        let (a, b) = self.span(node);
        vec![text("("), Piece::Range(a, b), text(")")]
    }

    /// A name for a variable that the rewrite declares itself (a parameter, or a `var` in the same function).
    pub(super) fn local(&mut self, prefix: &str) -> String {
        let n = self.bundler.temp_count;
        self.bundler.temp_count += 1;
        format!("_${prefix}{n}")
    }

    pub(super) fn refuse(&mut self, feature: Feature, node: NodeId, why: &str) {
        let target = self.bundler.target.name();
        let message = format!("{} is not available in the target {target} (it came with es{}), {why}", feature.describe(), feature.since());
        self.lower_error_message(node, message);
    }

    pub(super) fn insert(&mut self, at: u32, pieces: Vec<Piece>) {
        self.edits.push(Edit { start: at, end: at, pieces, shorthand: None, transparent: false });
    }

    pub(super) fn es5_enter(&mut self, node: NodeId) {
        let arrow = self.ast[node].has(crate::ast::flags::ARROW);
        let derived_ctor = self.es5.derived.contains(&node);
        self.es5.frames.push(Frame { function: Some(node), arrow, derived_ctor });
    }

    /// A scope whose `this` is its own that is not a function node: a class field's value, a static block.
    pub(super) fn es5_enter_this_scope(&mut self) {
        self.es5.frames.push(Frame { function: None, arrow: false, derived_ctor: false });
    }

    pub(super) fn es5_exit(&mut self) {
        self.es5.frames.pop();
    }

    /// How many functions the walk is inside.
    pub(super) fn es5_depth(&self) -> usize {
        self.es5.frames.len()
    }

    /// What `super` means here: the class that has the nearest function that is not an arrow.
    pub(super) fn home(&self) -> Option<super::lower_es5_class::Home> {
        let frame = self.es5.frames.iter().rev().find(|f| !f.arrow)?;
        self.es5.homes.get(&frame.function?).cloned()
    }

    /// The nearest function that is not an arrow, and whether it is the constructor of a class that extends another.
    pub(super) fn es5_top_function(&self) -> Option<(NodeId, bool)> {
        let frame = self.es5.frames.iter().rev().find(|f| !f.arrow)?;
        Some((frame.function?, frame.derived_ctor))
    }

    /// Whether an arrow function is between here and the nearest function that is not one.
    pub(super) fn es5_in_arrow(&self) -> bool {
        self.es5.frames.last().is_some_and(|f| f.arrow)
    }

    /// What `this` is called here, when it is not `this`: inside an arrow that is rewritten as a function, or in a constructor of a class that
    /// extends another.
    fn this_name(&self) -> Option<&'static str> {
        let mut crossed_arrow = false;
        for frame in self.es5.frames.iter().rev() {
            if frame.arrow {
                crossed_arrow = crossed_arrow || self.lowers(Feature::Arrow);
                continue;
            }
            return if frame.derived_ctor || crossed_arrow { Some("_$this") } else { None };
        }
        None
    }

    /// Whether the nearest function that is not an arrow lies beyond an arrow that is rewritten (so `arguments` and `new.target` there
    /// are the outer function's and need a name).
    fn beyond_arrow(&self) -> bool {
        let mut crossed_arrow = false;
        for frame in self.es5.frames.iter().rev() {
            if frame.arrow {
                crossed_arrow = crossed_arrow || self.lowers(Feature::Arrow);
                continue;
            }
            return crossed_arrow;
        }
        false
    }

    /// What the arrow functions inside `node` (the body of a function) take from it.
    pub(super) fn arrow_uses(&self, node: NodeId, in_arrow: bool, out: &mut Uses) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::This | NodeKind::Super if in_arrow => out.this = true,
            NodeKind::Ident(a) if in_arrow && ast.atom(a) == "arguments" && self.semantic.symbol_of(node).is_none_or(|s| self.semantic.symbol(s).kind == SymbolKind::Arguments) => out.arguments = true,
            NodeKind::NewTarget if in_arrow => out.new_target = true,
            NodeKind::Function { .. } => {
                if !ast[node].has(crate::ast::flags::ARROW) {
                    return;
                }
                ast.for_each_child(node, |c| self.arrow_uses(c, true, out));
            }
            NodeKind::Class { superclass, members, .. } => {
                if let Some(s) = superclass {
                    self.arrow_uses(s, in_arrow, out);
                }
                for &m in ast.list(members) {
                    if let NodeKind::ClassMethod { key, .. } | NodeKind::ClassField { key, .. } = ast[m].kind {
                        if ast[m].has(crate::ast::flags::COMPUTED) {
                            self.arrow_uses(key, in_arrow, out);
                        }
                    }
                }
            }
            _ => ast.for_each_child(node, |c| self.arrow_uses(c, in_arrow, out)),
        }
    }

    pub(super) fn lower_es5(&mut self, node: NodeId) {
        let ast = self.ast;
        if self.es5.handled.contains(&node) {
            return;
        }
        if self.lowers(Feature::Class) && self.lower_super_use(node) {
            return;
        }
        if self.lowers(Feature::LetConst) {
            self.lower_block_scoped(node);
        }
        self.lower_es5_stmt(node);
        if self.lowers(Feature::LetConst) {
            self.lower_loop_body(node);
        }
        match ast[node].kind {
            NodeKind::Num(_) if self.lowers(Feature::NumberLiteral) => {
                let (a, b) = self.span(node);
                if let Some(decimal) = radix_number(self.source_between(a, b)) {
                    self.push(node, vec![Piece::Text(decimal)]);
                }
            }
            NodeKind::Str(_) if self.lowers(Feature::UnicodeEscape) => {
                let (a, b) = self.span(node);
                if let Some(fixed) = without_code_point_escapes(self.source_between(a, b)) {
                    self.push(node, vec![Piece::Text(fixed)]);
                }
            }
            NodeKind::Template { quasis, exprs } if self.lowers(Feature::Template) => self.lower_template(node, quasis, exprs),
            NodeKind::TaggedTemplate { tag, quasi } if self.lowers(Feature::Template) => self.lower_tagged_template(node, tag, quasi),
            NodeKind::This => {
                if let Some(name) = self.this_name() {
                    self.push(node, vec![text(name)]);
                } else if self.beyond_top_arrow() {
                    self.refuse(Feature::Arrow, node, "and an arrow function that reads `this` at the top of a CommonJS module cannot be rewritten");
                }
            }
            NodeKind::Ident(a) if ast.atom(a) == "arguments" && self.beyond_arrow() && self.semantic.symbol_of(node).is_none_or(|s| self.semantic.symbol(s).kind == SymbolKind::Arguments) => {
                self.push(node, vec![text("_$args")]);
            }
            NodeKind::NewTarget if self.lowers(Feature::NewTarget) => self.lower_new_target(node),
            NodeKind::Class { .. } if self.lowers(Feature::Class) => self.lower_class_es5(node),
            NodeKind::Object(props) if self.lowers(Feature::ObjectLiteral) && self.object_needs_lowering(props) => self.lower_object(node, props),
            NodeKind::Property { key, value, kind } if self.lowers(Feature::ObjectLiteral) && !self.es5.handled.contains(&node) => self.lower_property(node, key, value, kind),
            NodeKind::Function { params, body, .. } => self.lower_function_es5(node, params, body),
            _ => {}
        }
    }

    /// A loop whose body a function must be.
    fn lower_loop_body(&mut self, node: NodeId) {
        let ast = self.ast;
        let body = match ast[node].kind {
            NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::While { body, .. } | NodeKind::DoWhile { body, .. } => body,
            // A `for of` that is rewritten makes its own body.
            NodeKind::ForOf { body, .. } if !self.lowers(Feature::ForOf) => body,
            _ => return,
        };
        if !self.blocks().wrapped.contains_key(&node) {
            return;
        }
        if let Some(pieces) = self.loop_wrapper(node, body) {
            self.push(body, pieces);
        }
    }

    /// `new.target` in a constructor, a method or a named function.
    fn lower_new_target(&mut self, node: NodeId) {
        if self.beyond_arrow() {
            self.refuse(Feature::NewTarget, node, "and new.target in an arrow function cannot be rewritten for it yet");
            return;
        }
        let Some((function, _)) = self.es5_top_function() else {
            self.refuse(Feature::NewTarget, node, "and new.target outside a function cannot be rewritten");
            return;
        };
        let replacement = match self.es5.homes.get(&function) {
            Some(home) if home.is_ctor => "Object.getPrototypeOf(this).constructor".to_string(),
            Some(_) => "void 0".to_string(),
            None => match self.ast[function].kind {
                NodeKind::Function { name: Some(n), .. } => {
                    let (a, b) = self.span(n);
                    let name = self.source_between(a, b).to_string();
                    format!("(this instanceof {name} ? this.constructor : void 0)")
                }
                _ => {
                    self.refuse(Feature::NewTarget, node, "and new.target in a function with no name cannot be rewritten for it yet");
                    return;
                }
            },
        };
        self.push(node, vec![Piece::Text(replacement)]);
    }

    /// `this` at the top of a module, inside an arrow that is rewritten.
    fn beyond_top_arrow(&self) -> bool {
        self.in_wrapper && self.lowers(Feature::Arrow) && !self.es5.frames.is_empty() && self.es5.frames.iter().all(|f| f.arrow)
    }

    // ---- template literals ----

    fn cooked(&self, quasi: NodeId) -> String {
        match self.ast[quasi].kind {
            NodeKind::TemplateElement { cooked: Some(c), .. } => js_string(self.ast.atom(c)),
            _ => "\"\"".to_string(),
        }
    }

    fn expression_arg(&self, e: NodeId) -> Vec<Piece> {
        if matches!(self.ast[e].kind, NodeKind::Sequence(_)) { self.rng_wrapped(e) } else { self.rng(e) }
    }

    fn lower_template(&mut self, node: NodeId, quasis: List, exprs: List) {
        let ast = self.ast;
        let quasis = ast.list(quasis).to_vec();
        let exprs = ast.list(exprs).to_vec();
        if exprs.is_empty() {
            let literal = self.cooked(quasis[0]);
            self.push(node, vec![Piece::Text(literal)]);
            return;
        }
        let mut pieces = vec![Piece::Text(format!("{}.concat(", self.cooked(quasis[0])))];
        for (i, &e) in exprs.iter().enumerate() {
            if i > 0 {
                pieces.push(text(", "));
            }
            pieces.extend(self.expression_arg(e));
            if let Some(&q) = quasis.get(i + 1) {
                let literal = self.cooked(q);
                if literal != "\"\"" {
                    pieces.push(Piece::Text(format!(", {literal}")));
                }
            }
        }
        pieces.push(text(")"));
        self.push(node, pieces);
    }

    fn lower_tagged_template(&mut self, node: NodeId, tag: NodeId, quasi: NodeId) {
        let ast = self.ast;
        let NodeKind::Template { quasis, exprs } = ast[quasi].kind else { return };
        self.es5.handled.insert(quasi);
        self.bundler.helpers.es5_template = true;
        let strings = |raw: bool| -> String {
            let list: Vec<String> = ast
                .list(quasis)
                .iter()
                .map(|&q| match ast[q].kind {
                    NodeKind::TemplateElement { cooked, raw: r } => match (raw, cooked) {
                        (true, _) => js_string(ast.atom(r)),
                        (false, Some(c)) => js_string(ast.atom(c)),
                        (false, None) => "void 0".to_string(),
                    },
                    _ => "\"\"".to_string(),
                })
                .collect();
            format!("[{}]", list.join(", "))
        };
        let site = self.temp();
        let mut pieces = self.rng_wrapped_if_needed(tag);
        pieces.push(Piece::Text(format!("({site} || ({site} = __tpl({}, {}))", strings(false), strings(true))));
        for &e in ast.list(exprs) {
            pieces.push(text(", "));
            pieces.extend(self.expression_arg(e));
        }
        pieces.push(text(")"));
        self.push(node, pieces);
    }

    /// An expression that is called: in parentheses when it is not a name or a member access.
    fn rng_wrapped_if_needed(&self, node: NodeId) -> Vec<Piece> {
        match self.ast[node].kind {
            NodeKind::Ident(_) | NodeKind::Member { .. } | NodeKind::This | NodeKind::Call { .. } => self.rng(node),
            _ => self.rng_wrapped(node),
        }
    }

    // ---- object literals ----

    fn object_needs_lowering(&self, props: List) -> bool {
        let ast = self.ast;
        ast.list(props).iter().any(|&p| match ast[p].kind {
            NodeKind::Spread(_) => true,
            NodeKind::Property { .. } => ast[p].has(crate::ast::flags::COMPUTED),
            _ => false,
        })
    }

    /// The key of a property that is not computed, as text of a string literal.
    pub(super) fn key_string(&self, key: NodeId) -> String {
        let ast = self.ast;
        match ast[key].kind {
            NodeKind::PropName(a) | NodeKind::Str(a) => js_string(ast.atom(a)),
            NodeKind::Num(n) => js_string(&number_text(n.get())),
            _ => "\"\"".to_string(),
        }
    }

    /// `m: function (...) {...}`, `a: a`: a property of an object literal that has no es5 spelling, and no computed key.
    fn lower_property(&mut self, node: NodeId, key: NodeId, value: NodeId, kind: PropKind) {
        let ast = self.ast;
        if ast[node].has(crate::ast::flags::COMPUTED) {
            // Part of a whole object that is rewritten.
            return;
        }
        match kind {
            PropKind::Init if ast[node].has(crate::ast::flags::SHORTHAND) && matches!(ast[value].kind, NodeKind::Ident(_)) => {
                let (a, b) = self.span(key);
                let name = self.source_between(a, b).to_string();
                let mut pieces = vec![Piece::Text(format!("{name}: "))];
                pieces.extend(self.rng(value));
                self.push(node, pieces);
            }
            PropKind::Method => {
                let (ka, kb) = self.span(key);
                let (fa, fb) = self.span(value);
                self.push(node, vec![Piece::Range(ka, kb), text(": function "), Piece::Range(fa, fb)]);
            }
            _ => {}
        }
    }

    /// An object with computed keys or spread: written one property at a time, `__ck(__ck({ a: 1 }, k, v), "b", w)`, each call
    /// returning the object, so the properties are made in the order they are written and nothing needs a variable.
    fn lower_object(&mut self, node: NodeId, props: List) {
        let ast = self.ast;
        self.bundler.helpers.es5_object = true;
        let list = ast.list(props).to_vec();
        let first = list.iter().position(|&p| matches!(ast[p].kind, NodeKind::Spread(_)) || ast[p].has(crate::ast::flags::COMPUTED)).unwrap_or(list.len());
        let mut acc: Vec<Piece> = if first == 0 {
            vec![text("{}")]
        } else {
            let (a, _) = self.span(list[0]);
            let (_, b) = self.span(list[first - 1]);
            vec![text("{ "), Piece::Range(a, b), text(" }")]
        };
        for &p in &list[first..] {
            let mut next: Vec<Piece> = Vec::new();
            match ast[p].kind {
                NodeKind::Spread(arg) => {
                    self.bundler.helpers.spread = true;
                    next.push(text("__spreadValues("));
                    next.extend(acc);
                    next.push(text(", "));
                    next.extend(self.rng(arg));
                    next.push(text(")"));
                }
                NodeKind::Property { key, value, kind } => {
                    let computed = ast[p].has(crate::ast::flags::COMPUTED);
                    let key_pieces = if computed { self.rng(key) } else { vec![Piece::Text(self.key_string(key))] };
                    let (fa, fb) = self.span(value);
                    let function_text = vec![text("function "), Piece::Range(fa, fb)];
                    match kind {
                        PropKind::Init => {
                            next.push(text("__ck("));
                            next.extend(acc);
                            next.push(text(", "));
                            next.extend(key_pieces);
                            next.push(text(", "));
                            next.extend(self.rng(value));
                            next.push(text(")"));
                        }
                        PropKind::Method => {
                            next.push(text("__ck("));
                            next.extend(acc);
                            next.push(text(", "));
                            next.extend(key_pieces);
                            next.push(text(", "));
                            next.extend(function_text);
                            next.push(text(")"));
                        }
                        PropKind::Get | PropKind::Set => {
                            next.push(text("__cka("));
                            next.extend(acc);
                            next.push(text(", "));
                            next.extend(key_pieces);
                            next.push(Piece::Text(format!(", \"{}\", ", if kind == PropKind::Get { "get" } else { "set" })));
                            next.extend(function_text);
                            next.push(text(")"));
                        }
                    }
                    self.es5.handled.insert(p);
                }
                _ => next = acc,
            }
            acc = next;
        }
        self.push(node, acc);
    }

    // ---- functions ----

    /// Arrow functions, default and rest parameters and patterns in the parameter list.
    fn lower_function_es5(&mut self, node: NodeId, params: List, body: NodeId) {
        let ast = self.ast;
        let f = ast[node].flags;
        use crate::ast::flags;
        if f & flags::GENERATOR != 0 && self.lowers(Feature::Generator) {
            self.refuse(Feature::Generator, node, "and Rathkit Pack cannot rewrite it yet");
            return;
        }
        if f & flags::ASYNC != 0 && self.lowers(Feature::Generator) && self.lowers(Feature::AsyncFunction) {
            self.refuse(Feature::AsyncFunction, node, "and it is written with a generator, which Rathkit Pack cannot rewrite for it yet");
            return;
        }
        let arrow = f & flags::ARROW != 0;
        let param_nodes = ast.list(params).to_vec();
        let (list, prologue) = self.plan_params(&param_nodes);
        let block_body = !(arrow && f & flags::EXPR_BODY != 0);
        let (body_start, body_end) = self.span(body);
        if arrow && self.lowers(Feature::Arrow) {
            let mut pieces = vec![text("function (")];
            pieces.extend(list.unwrap_or_else(|| self.plain_params(&param_nodes)));
            pieces.push(text(") {"));
            pieces.extend(prologue);
            if block_body {
                pieces.push(Piece::Range(body_start + 1, body_end));
            } else {
                pieces.push(text(" return "));
                pieces.extend(self.rng(body));
                pieces.push(text("; }"));
            }
            self.push(node, pieces);
            return;
        }
        let mut insertion = Vec::new();
        if !arrow && self.lowers(Feature::Arrow) {
            let mut uses = Uses::default();
            ast.for_each_child(body, |c| self.arrow_uses(c, false, &mut uses));
            if uses.this && !self.es5.derived.contains(&node) {
                insertion.push(text(" var _$this = this;"));
            }
            if uses.arguments {
                insertion.push(text(" var _$args = arguments;"));
            }
        }
        if let Some(list) = list {
            let first = self.span(param_nodes[0]).0;
            let last = self.span(*param_nodes.last().unwrap()).1;
            let header_from = self.header_start(node);
            let open = self.source_between(header_from, first).rfind('(').map(|i| header_from + i as u32 + 1);
            let close = self.source_between(last, body_start).find(')').map(|i| last + i as u32);
            let (Some(open), Some(close)) = (open, close) else { return };
            self.edits.push(Edit { start: open, end: close, pieces: list, shorthand: None, transparent: false });
        }
        insertion.extend(prologue);
        if !insertion.is_empty() && block_body {
            self.insert(body_start + 1, insertion);
        }
    }

    /// Where the text between a function's start and its first parameter begins.
    fn header_start(&self, node: NodeId) -> u32 {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Function { name: Some(n), .. } => ast[n].span.end,
            _ => ast[node].span.start,
        }
    }

    /// The parameters as written, for an arrow that is rewritten and has nothing in its parameters to rewrite.
    fn plain_params(&self, params: &[NodeId]) -> Vec<Piece> {
        let mut out = Vec::new();
        for (i, &p) in params.iter().enumerate() {
            if i > 0 {
                out.push(text(", "));
            }
            out.extend(self.rng(p));
        }
        out
    }

    /// The new parameter list and the statements that go first in the body, when a parameter needs rewriting.
    fn plan_params(&mut self, params: &[NodeId]) -> (Option<Vec<Piece>>, Vec<Piece>) {
        let ast = self.ast;
        let needs = params.iter().any(|&p| match ast[p].kind {
            NodeKind::AssignPattern { .. } | NodeKind::Rest(_) => self.lowers(Feature::Params),
            NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) => self.lowers(Feature::Destructuring),
            _ => false,
        });
        if !needs {
            return (None, Vec::new());
        }
        let mut list: Vec<Piece> = Vec::new();
        let mut prologue: Vec<Piece> = Vec::new();
        for (i, &p) in params.iter().enumerate() {
            let comma = if list.is_empty() { None } else { Some(text(", ")) };
            match ast[p].kind {
                NodeKind::AssignPattern { target, default } => {
                    let plain = matches!(ast[target].kind, NodeKind::BindingIdent(_));
                    let name: Vec<Piece> = if plain { self.rng(target) } else { vec![Piece::Text(self.local("p"))] };
                    list.extend(comma);
                    list.extend(name.clone());
                    prologue.push(text(" if ("));
                    prologue.extend(name.clone());
                    prologue.push(text(" === void 0) "));
                    prologue.extend(name.clone());
                    prologue.push(text(" = "));
                    prologue.extend(self.rng(default));
                    prologue.push(text(";"));
                    if !plain {
                        let Piece::Text(n) = &name[0] else { unreachable!() };
                        let n = n.clone();
                        let items = self.destructure(target, &n, true);
                        prologue.push(text(" var "));
                        prologue.extend(join(items));
                        prologue.push(text(";"));
                    }
                }
                NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) if self.lowers(Feature::Destructuring) => {
                    let n = self.local("p");
                    list.extend(comma);
                    list.push(Piece::Text(n.clone()));
                    let items = self.destructure(p, &n, true);
                    prologue.push(text(" var "));
                    prologue.extend(join(items));
                    prologue.push(text(";"));
                }
                NodeKind::Rest(target) if self.lowers(Feature::Params) => {
                    let slice = format!("Array.prototype.slice.call(arguments, {i})");
                    if matches!(ast[target].kind, NodeKind::BindingIdent(_)) {
                        prologue.push(text(" var "));
                        prologue.extend(self.rng(target));
                        prologue.push(Piece::Text(format!(" = {slice};")));
                    } else {
                        let n = self.local("p");
                        prologue.push(Piece::Text(format!(" var {n} = {slice};")));
                        let items = self.destructure(target, &n, true);
                        prologue.push(text(" var "));
                        prologue.extend(join(items));
                        prologue.push(text(";"));
                    }
                }
                _ => {
                    list.extend(comma);
                    list.extend(self.rng(p));
                }
            }
        }
        (Some(list), prologue)
    }

    // ---- destructuring ----

    /// The assignments that take the parts of `source` (a name) apart into the targets of `pattern`, to be written one after the other, separated by
    /// commas: in a declaration (`decl`) the temporaries are declared in it, otherwise they are temporaries of the chunk.
    pub(super) fn destructure(&mut self, pattern: NodeId, source: &str, decl: bool) -> Vec<Vec<Piece>> {
        let mut items = Vec::new();
        self.destructure_into(pattern, source, decl, &mut items);
        items
    }

    fn scratch(&mut self, decl: bool) -> String {
        if decl { self.local("d") } else { self.temp() }
    }

    fn destructure_into(&mut self, pattern: NodeId, source: &str, decl: bool, items: &mut Vec<Vec<Piece>>) {
        let ast = self.ast;
        match ast[pattern].kind {
            NodeKind::ObjectPattern(list) => {
                let props = ast.list(list).to_vec();
                // The keys, for a rest element that leaves them out.
                let mut keys: Vec<String> = Vec::new();
                let has_rest = props.iter().any(|&p| matches!(ast[p].kind, NodeKind::Rest(_)));
                for &p in &props {
                    match ast[p].kind {
                        NodeKind::PatternProp { key, value } => {
                            let computed = ast[p].has(crate::ast::flags::COMPUTED);
                            let read: Vec<Piece> = if computed {
                                let k = self.rng(key);
                                if has_rest {
                                    let t = self.scratch(decl);
                                    let mut item = vec![Piece::Text(format!("{t} = "))];
                                    item.extend(k);
                                    items.push(item);
                                    keys.push(t.clone());
                                    vec![Piece::Text(format!("{source}[{t}]"))]
                                } else {
                                    let mut v = vec![Piece::Text(format!("{source}["))];
                                    v.extend(k);
                                    v.push(text("]"));
                                    v
                                }
                            } else {
                                keys.push(self.key_string(key));
                                vec![Piece::Text(self.member_access(source, key))]
                            };
                            self.bind(value, read, decl, items);
                        }
                        NodeKind::Rest(target) => {
                            self.bundler.helpers.spread = true;
                            let read = vec![Piece::Text(format!("__objRest({source}, [{}])", keys.join(", ")))];
                            self.bind(target, read, decl, items);
                        }
                        _ => {}
                    }
                }
            }
            NodeKind::ArrayPattern(list) => {
                self.bundler.helpers.es5_array = true;
                let elements = ast.list(list).to_vec();
                let rest = elements.iter().any(|&e| matches!(ast[e].kind, NodeKind::Rest(_)));
                let array = self.scratch(decl);
                let call = if rest { format!("__arr({source})") } else { format!("__arr({source}, {})", elements.len()) };
                items.push(vec![Piece::Text(format!("{array} = {call}"))]);
                for (i, &e) in elements.iter().enumerate() {
                    match ast[e].kind {
                        NodeKind::Hole => {}
                        NodeKind::Rest(target) => {
                            let read = vec![Piece::Text(format!("{array}.slice({i})"))];
                            self.bind(target, read, decl, items);
                        }
                        _ => {
                            let read = vec![Piece::Text(format!("{array}[{i}]"))];
                            self.bind(e, read, decl, items);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn member_access(&self, source: &str, key: NodeId) -> String {
        let ast = self.ast;
        match ast[key].kind {
            NodeKind::PropName(a) if is_identifier_name(ast.atom(a)) => format!("{source}.{}", ast.atom(a)),
            NodeKind::Num(n) => format!("{source}[{}]", number_text(n.get())),
            _ => format!("{source}[{}]", self.key_string(key)),
        }
    }

    /// `target = read`, where `target` may have a default (`a = 1`) or be a pattern itself.
    fn bind(&mut self, target: NodeId, read: Vec<Piece>, decl: bool, items: &mut Vec<Vec<Piece>>) {
        let ast = self.ast;
        let (target, read) = match ast[target].kind {
            NodeKind::AssignPattern { target: inner, default } => {
                let t = self.scratch(decl);
                let mut first = vec![Piece::Text(format!("{t} = "))];
                first.extend(read);
                items.push(first);
                let mut conditional = vec![Piece::Text(format!("{t} === void 0 ? "))];
                conditional.extend(self.rng(default));
                conditional.push(Piece::Text(format!(" : {t}")));
                (inner, conditional)
            }
            _ => (target, read),
        };
        match ast[target].kind {
            NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) => {
                let t = self.scratch(decl);
                let mut item = vec![Piece::Text(format!("{t} = "))];
                item.extend(read);
                items.push(item);
                self.destructure_into(target, &t, decl, items);
            }
            _ => {
                let mut item = self.rng(target);
                item.push(text(" = "));
                item.extend(read);
                items.push(item);
            }
        }
    }
}

/// The items joined by commas.
pub(super) fn join(items: Vec<Vec<Piece>>) -> Vec<Piece> {
    let mut out = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(text(", "));
        }
        out.extend(item);
    }
    out
}
