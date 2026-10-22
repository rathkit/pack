//! Syntax-level shrinking for the minifier: shorter ways to write the same program.
//!
//! The minifier writes out the tokens of the parsed text, so every rewrite here is a decision
//! about tokens, made from the syntax tree: leave one out, write other text in its place, or
//! turn a `;` into a `,`. Nothing is rebuilt from the tree. The decisions are collected into a
//! [`Plan`] with one [`Action`] per token, which the writer follows.
//!
//! The rewrites:
//! - `true` and `false` become `!0` and `!1`, `undefined` becomes `void 0`
//! - numbers take their shortest spelling (`1000` is `1e3`, `0.5` is `.5`, `0x10` is `16`)
//! - `a["b"]` is `a.b` and `{ "b": 1 }` is `{ b: 1 }`
//! - `new X()` is `new X` where nothing follows that would change its meaning
//! - braces around a single statement go, unless an `else` could then attach to the wrong `if`
//! - `var a = 1; var b = 2;` is `var a = 1, b = 2;` (also `let` and `const`)
//! - `() => { return x; }` is `() => x`
//! - `return undefined` is `return`
//! - `if (true)`/`if (false)` and `true ? a : b` keep the branch that runs, if dropping the
//!   other cannot change which names are declared
//!
//! Each is exact: the result parses to the program the rewrite started from, and the checks in
//! `minify::verify` compare the trees knowing these rules.

use crate::ast::{Ast, NodeId, NodeKind, UnaryOp, VarKind, flags};
use crate::lexer::{Kind, Kw, Punct, Token};
use crate::semantic::Semantic;

/// What the writer does with one token.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Action {
    /// Write it as it is.
    Keep,
    /// Leave it out.
    Skip,
    /// Write this instead: an index into `Plan::texts`.
    Text(u32),
    /// A `;` that can go when a `}` or the end follows (it replaces a `}` that was dropped).
    Semi,
    /// A `;` that stands for an empty statement and must stay.
    KeptSemi,
    /// A `,` that joins two declarations: the `;` before it is gone, and so is any line-break
    /// semicolon.
    Joiner,
}

pub(crate) struct Plan {
    pub actions: Vec<Action>,
    /// Tokens inside code that is left out, other than its first token. A line break there
    /// belonged to code that is gone.
    pub interior: Vec<bool>,
    /// The texts `Action::Text` points to.
    pub texts: Vec<String>,
}

/// `barriers` are source offsets (in order) where declarations on either side are not merged.
pub(crate) fn plan(source: &str, ast: &Ast, sem: &Semantic, tokens: &[Token], barriers: &[u32]) -> Plan {
    let mut ctx = Ctx { source, ast, sem, tokens, barriers, parent: vec![None; ast.node_count() + 1], actions: vec![Action::Keep; tokens.len()], dead: Vec::new(), kept_semi: Vec::new(), texts: Vec::new(), text_index: Default::default() };
    let mut order = Vec::new();
    let mut stack = vec![ast.root];
    while let Some(n) = stack.pop() {
        order.push(n);
        ast.for_each_child(n, |c| {
            ctx.parent[c.index()] = Some(n);
            stack.push(c);
        });
    }
    for &n in &order {
        ctx.visit(n);
    }
    // What a rewrite leaves out wins over anything decided inside it.
    let mut interior = vec![false; tokens.len()];
    for &(from, to) in &ctx.dead {
        let to = to.min(tokens.len());
        for a in &mut ctx.actions[from..to] {
            *a = Action::Skip;
        }
        for flag in interior.iter_mut().take(to).skip(from + 1) {
            *flag = true;
        }
        // A line break that stood for the `;` of a statement that is gone, in front of the `else` after it: that `;`
        // would end up behind a block, where it is an error.
        if tokens.get(to).is_some_and(|t| t.kind == Kind::Keyword(Kw::Else)) {
            interior[to] = true;
        }
    }
    // A `;` kept where a whole statement went only counts if that statement is not inside a larger one that went.
    let mut outer: Vec<(usize, usize)> = ctx.dead.clone();
    outer.sort_unstable_by_key(|&(from, to)| (from, std::cmp::Reverse(to)));
    let mut tops: Vec<(usize, usize)> = Vec::new();
    for (from, to) in outer {
        if tops.last().is_none_or(|&(_, last_to)| from >= last_to) {
            tops.push((from, to));
        }
    }
    for &i in &ctx.kept_semi {
        let at = tops.partition_point(|&(from, _)| from <= i);
        let nested = at > 0 && tops[at - 1].0 != i && i < tops[at - 1].1;
        if !nested {
            ctx.actions[i] = Action::KeptSemi;
        }
    }
    Plan { actions: ctx.actions, interior, texts: ctx.texts }
}

struct Ctx<'a> {
    source: &'a str,
    ast: &'a Ast,
    sem: &'a Semantic,
    tokens: &'a [Token],
    barriers: &'a [u32],
    parent: Vec<Option<NodeId>>,
    actions: Vec<Action>,
    /// Token ranges `[from, to)` that are left out whatever else was decided.
    dead: Vec<(usize, usize)>,
    kept_semi: Vec<usize>,
    texts: Vec<String>,
    text_index: crate::fxhash::FastMap<String, u32>,
}

impl Ctx<'_> {
    fn first_tok(&self, pos: u32) -> usize {
        self.tokens.partition_point(|t| t.start < pos)
    }

    fn last_tok(&self, end: u32) -> usize {
        self.tokens.partition_point(|t| t.end <= end).saturating_sub(1)
    }

    fn text(&self, i: usize) -> &str {
        let t = &self.tokens[i];
        &self.source[t.start as usize..t.end as usize]
    }

    fn is(&self, i: usize, p: Punct) -> bool {
        self.tokens.get(i).is_some_and(|t| t.is(p))
    }

    fn kw(&self, i: usize, k: Kw) -> bool {
        self.tokens.get(i).is_some_and(|t| t.kind == Kind::Keyword(k))
    }

    /// The action that writes `text` in place of a token.
    fn text_action(&mut self, text: &str) -> Action {
        if let Some(&id) = self.text_index.get(text) {
            return Action::Text(id);
        }
        self.texts.push(text.to_string());
        let id = self.texts.len() as u32 - 1;
        self.text_index.insert(text.to_string(), id);
        Action::Text(id)
    }

    fn set_text(&mut self, i: usize, text: &str) {
        let action = self.text_action(text);
        self.set(i, action);
    }

    fn set(&mut self, i: usize, action: Action) {
        if let Some(slot) = self.actions.get_mut(i) {
            *slot = action;
        }
    }

    fn visit(&mut self, n: NodeId) {
        let ast = self.ast;
        let node = &ast[n];
        match node.kind {
            NodeKind::Bool(value) => {
                let i = self.first_tok(node.span.start);
                if matches!(self.tokens.get(i).map(|t| t.kind), Some(Kind::Keyword(Kw::True | Kw::False))) {
                    let text = if value { "!0" } else { "!1" };
                    self.write_unary(i, text);
                }
            }
            NodeKind::Ident(name) if ast.atom(name) == "undefined" => {
                if self.is_global_undefined(n) && !self.is_shorthand_value(n) {
                    let i = self.first_tok(node.span.start);
                    self.write_unary(i, "void 0");
                }
            }
            NodeKind::Empty if self.in_statement_list(n) => {
                let i = self.first_tok(node.span.start);
                if self.is(i, Punct::Semi) {
                    self.dead.push((i, i + 1));
                }
            }
            NodeKind::Num(_) => {
                let i = self.first_tok(node.span.start);
                if self.tokens.get(i).is_some_and(|t| t.kind == Kind::Number) {
                    if let Some(short) = shorten_number(self.text(i)) {
                        self.set_text(i, &short);
                    }
                }
            }
            NodeKind::Member { prop, .. } if node.has(flags::COMPUTED) => self.dot_access(prop),
            NodeKind::Property { key, .. } if !node.has(flags::COMPUTED) && !node.has(flags::SHORTHAND) => {
                if let NodeKind::Str(_) = ast[key].kind {
                    let i = self.first_tok(ast[key].span.start);
                    if let Some(name) = self.plain_name(i, key) {
                        let name = name.to_string();
                        self.set_text(i, &name);
                    }
                }
            }
            NodeKind::New { callee, args } if ast.list(args).is_empty() => self.new_without_parens(n, callee),
            NodeKind::If { test, cons, alt } => {
                self.unbrace(cons);
                if let Some(alt) = alt {
                    self.unbrace(alt);
                }
                if let Some(value) = self.constant(test) {
                    self.dead_if(n, value, cons, alt);
                }
            }
            NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } | NodeKind::While { body, .. } | NodeKind::DoWhile { body, .. } => self.unbrace(body),
            NodeKind::Program(list) | NodeKind::Block(list) | NodeKind::StaticBlock(list) => self.merge_declarations(ast.list(list)),
            NodeKind::Case { body, .. } => self.merge_declarations(ast.list(body)),
            NodeKind::Function { body, .. } if node.has(flags::ARROW) && !node.has(flags::EXPR_BODY) => self.arrow_expression(n, body),
            NodeKind::Return(Some(arg)) => self.return_nothing(n, arg),
            NodeKind::Conditional { test, cons, alt } => {
                if let Some(value) = self.constant(test) {
                    self.dead_conditional(n, value, test, cons, alt);
                }
            }
            _ => {}
        }
    }

    /// Writes `text` (a unary expression) for the token at `i`, in parentheses where the
    /// syntax around it needs a primary expression.
    fn write_unary(&mut self, i: usize, text: &str) {
        let after = self.tokens.get(i + 1).is_some_and(|t| {
            matches!(t.kind, Kind::Punct(Punct::Dot | Punct::QuestionDot | Punct::LBracket | Punct::LParen | Punct::StarStar) | Kind::NoSubTemplate | Kind::TemplateHead)
        });
        let before = i > 0 && (self.kw(i - 1, Kw::Extends) || self.kw(i - 1, Kw::New));
        if after || before {
            self.set_text(i, &format!("({text})"));
        } else {
            self.set_text(i, text);
        }
    }

    /// `undefined` that no declaration provides and the code only reads.
    fn is_global_undefined(&self, n: NodeId) -> bool {
        let node = &self.ast[n];
        self.sem.symbol_of(n).is_none() && !node.has(flags::ASSIGNED)
    }

    fn is_shorthand_value(&self, n: NodeId) -> bool {
        self.parent[n.index()].is_some_and(|p| matches!(self.ast[p].kind, NodeKind::Property { .. } | NodeKind::PatternProp { .. }) && self.ast[p].has(flags::SHORTHAND))
    }

    /// The text of the string token at `i` without quotes, if it is a plain identifier name
    /// that the string node `node` really holds.
    fn plain_name(&self, i: usize, node: NodeId) -> Option<&str> {
        let t = self.tokens.get(i)?;
        if t.kind != Kind::String {
            return None;
        }
        let text = self.text(i);
        let inner = text.get(1..text.len().checked_sub(1)?)?;
        let NodeKind::Str(atom) = self.ast[node].kind else { return None };
        (is_plain_identifier(inner) && self.ast.atom(atom) == inner).then_some(inner)
    }

    /// `a["b"]` is `a.b`.
    fn dot_access(&mut self, prop: NodeId) {
        let NodeKind::Str(_) = self.ast[prop].kind else { return };
        let i = self.first_tok(self.ast[prop].span.start);
        if i == 0 || !self.is(i - 1, Punct::LBracket) || !self.is(i + 1, Punct::RBracket) {
            return;
        }
        let Some(name) = self.plain_name(i, prop) else { return };
        let name = name.to_string();
        if i >= 2 && self.is(i - 2, Punct::QuestionDot) {
            self.set(i - 1, Action::Skip);
        } else {
            self.set_text(i - 1, ".");
        }
        self.set_text(i, &name);
        self.set(i + 1, Action::Skip);
    }

    /// `new X()` is `new X` unless a `.`, call or the like follows, which would then belong
    /// to `X` and not to the new object.
    fn new_without_parens(&mut self, n: NodeId, callee: NodeId) {
        let end = self.last_tok(self.ast[n].span.end);
        let callee_end = self.last_tok(self.ast[callee].span.end);
        if end < 1 || !self.is(end, Punct::RParen) || !self.is(end - 1, Punct::LParen) || end - 1 <= callee_end {
            return;
        }
        let follows = self.tokens.get(end + 1).is_some_and(|t| {
            matches!(t.kind, Kind::Punct(Punct::Dot | Punct::QuestionDot | Punct::LBracket | Punct::LParen) | Kind::NoSubTemplate | Kind::TemplateHead)
        });
        if !follows {
            self.set(end - 1, Action::Skip);
            self.set(end, Action::Skip);
        }
    }

    // ---- statements ----------------------------------------------------------------------

    /// Whether `s` is a statement that can stand alone as the body of an `if` or loop without
    /// braces, with the same meaning.
    fn can_stand_alone(&self, s: NodeId) -> bool {
        let node = &self.ast[s];
        match node.kind {
            NodeKind::ExprStmt(_) => !node.has(flags::DIRECTIVE),
            NodeKind::VarDecl { kind, .. } => kind == VarKind::Var,
            NodeKind::If { .. } | NodeKind::For { .. } | NodeKind::ForIn { .. } | NodeKind::ForOf { .. } | NodeKind::While { .. } | NodeKind::DoWhile { .. } => true,
            NodeKind::Return(_) | NodeKind::Break(_) | NodeKind::Continue(_) | NodeKind::Throw(_) | NodeKind::Try { .. } | NodeKind::Switch { .. } | NodeKind::Debugger => true,
            _ => false,
        }
    }

    /// Whether the statement ends with a `;` (or a line break standing for one) rather than a
    /// `}`: it is a simple statement, or its last part is (`if (a) b; else c` ends like `c`).
    fn needs_semicolon(&self, mut s: NodeId) -> bool {
        loop {
            match self.ast[s].kind {
                NodeKind::If { cons, alt, .. } => s = alt.unwrap_or(cons),
                NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } | NodeKind::While { body, .. } | NodeKind::With { body, .. } | NodeKind::Labeled { body, .. } => s = body,
                NodeKind::ExprStmt(_) | NodeKind::VarDecl { .. } | NodeKind::Return(_) | NodeKind::Break(_) | NodeKind::Continue(_) | NodeKind::Throw(_) | NodeKind::DoWhile { .. } | NodeKind::Debugger => return true,
                _ => return false,
            }
        }
    }

    /// Whether the statement ends in an `if` without `else`, which would take an `else` that
    /// comes right after it.
    fn ends_open(&self, mut s: NodeId) -> bool {
        loop {
            match self.ast[s].kind {
                NodeKind::If { alt: None, .. } => return true,
                NodeKind::If { alt: Some(alt), .. } => s = alt,
                NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } | NodeKind::While { body, .. } | NodeKind::With { body, .. } | NodeKind::Labeled { body, .. } => s = body,
                // A block of one statement may lose its braces too, so look inside.
                NodeKind::Block(list) => match self.ast.list(list) {
                    [only] => s = *only,
                    _ => return false,
                },
                _ => return false,
            }
        }
    }

    /// Whether an `else` follows `n` once the statements around it are written without
    /// braces: `n` ends the `then` part of an `if` that has an `else`.
    fn else_follows(&self, n: NodeId) -> bool {
        let mut child = n;
        loop {
            let Some(p) = self.parent[child.index()] else { return false };
            match self.ast[p].kind {
                NodeKind::If { cons, alt, .. } => {
                    if child == cons && alt.is_some() {
                        return true;
                    }
                }
                NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } | NodeKind::While { body, .. } | NodeKind::With { body, .. } | NodeKind::Labeled { body, .. } if child == body => {}
                _ => return false,
            }
            child = p;
        }
    }

    /// `if (a) { b; }` is `if (a) b;`.
    fn unbrace(&mut self, block: NodeId) {
        let NodeKind::Block(list) = self.ast[block].kind else { return };
        let items = self.ast.list(list);
        let [s] = items else { return };
        let s = *s;
        if !self.can_stand_alone(s) || (self.ends_open(s) && self.else_follows(block)) {
            return;
        }
        // A decided `if` may vanish, and a body must not.
        if matches!(self.ast[s].kind, NodeKind::If { test, .. } if self.constant(test).is_some()) {
            return;
        }
        let open = self.first_tok(self.ast[block].span.start);
        let close = self.last_tok(self.ast[block].span.end);
        if !self.is(open, Punct::LBrace) || !self.is(close, Punct::RBrace) || close <= open + 1 {
            return;
        }
        self.set(open, Action::Skip);
        // The statement's own `;` stays; one that a line break or the `}` stood for is written.
        if self.is(close - 1, Punct::Semi) || !self.needs_semicolon(s) {
            self.set(close, Action::Skip);
        } else {
            self.set(close, Action::Semi);
        }
    }

    /// `var a = 1; var b = 2;` is `var a = 1, b = 2;`.
    fn merge_declarations(&mut self, items: &[NodeId]) {
        for pair in items.windows(2) {
            let (NodeKind::VarDecl { kind: k1, .. }, NodeKind::VarDecl { kind: k2, .. }) = (self.ast[pair[0]].kind, self.ast[pair[1]].kind) else { continue };
            if k1 != k2 {
                continue;
            }
            let last = self.last_tok(self.ast[pair[0]].span.end);
            let next = self.first_tok(self.ast[pair[1]].span.start);
            if next <= last || next - last > 2 {
                continue;
            }
            let (end, start) = (self.ast[pair[0]].span.end, self.ast[pair[1]].span.start);
            let at = self.barriers.partition_point(|&b| b < end);
            if self.barriers.get(at).is_some_and(|&b| b <= start) {
                continue;
            }
            // Between them there is a `;` or nothing (a line break stood for it).
            let semi = if self.is(last, Punct::Semi) {
                Some(last)
            } else if self.is(last + 1, Punct::Semi) && last + 1 < next {
                Some(last + 1)
            } else {
                None
            };
            if semi.is_none() && next != last + 1 {
                continue;
            }
            if let Some(semi) = semi {
                self.set(semi, Action::Skip);
            }
            self.set(next, Action::Joiner);
        }
    }

    /// `x => { return y; }` is `x => y`.
    fn arrow_expression(&mut self, f: NodeId, body: NodeId) {
        let NodeKind::Block(list) = self.ast[body].kind else { return };
        let [s] = self.ast.list(list) else { return };
        let NodeKind::Return(Some(arg)) = self.ast[*s].kind else { return };
        if self.in_declaration_head(f) {
            return;
        }
        let open = self.first_tok(self.ast[body].span.start);
        let close = self.last_tok(self.ast[body].span.end);
        let ret = self.first_tok(self.ast[*s].span.start);
        if !self.is(open, Punct::LBrace) || !self.is(close, Punct::RBrace) || !self.kw(ret, Kw::Return) || ret != open + 1 {
            return;
        }
        let arg_node = &self.ast[arg];
        let first = self.first_tok(arg_node.span.start);
        // An object literal would read as a block, a sequence as several arguments.
        let wrap = !arg_node.has(flags::PARENTHESIZED) && (matches!(arg_node.kind, NodeKind::Sequence(_)) || self.is(first, Punct::LBrace));
        if wrap {
            self.set_text(open, "(");
            self.set_text(close, ")");
        } else {
            self.set(open, Action::Skip);
            self.set(close, Action::Skip);
        }
        self.set(ret, Action::Skip);
        if close >= 1 && self.is(close - 1, Punct::Semi) && close - 1 > ret {
            self.set(close - 1, Action::Skip);
        }
    }

    /// Whether `n` is inside the head of a `for` loop, where an `in` operator would be taken
    /// for the `for ... in` form (and so is not written where an expression body would put it).
    fn in_declaration_head(&self, n: NodeId) -> bool {
        let mut child = n;
        while let Some(p) = self.parent[child.index()] {
            match self.ast[p].kind {
                NodeKind::For { init: Some(init), .. } if init == child => return true,
                NodeKind::ForIn { left, .. } | NodeKind::ForOf { left, .. } if left == child => return true,
                _ => {}
            }
            child = p;
        }
        false
    }

    /// `return undefined` and `return void 0` are `return`.
    fn return_nothing(&mut self, n: NodeId, arg: NodeId) {
        let node = &self.ast[arg];
        // The body of `() => undefined` is a return statement too, with no `return` written.
        if node.has(flags::PARENTHESIZED) || !self.kw(self.first_tok(self.ast[n].span.start), Kw::Return) {
            return;
        }
        // The only statement of an arrow becomes its body, `() => undefined`, not an empty one.
        if let Some(block) = self.parent[n.index()] {
            if matches!(self.ast[block].kind, NodeKind::Block(l) if self.ast.list(l).len() == 1) && self.parent[block.index()].is_some_and(|f| self.ast[f].has(flags::ARROW) && matches!(self.ast[f].kind, NodeKind::Function { .. })) {
                return;
            }
        }
        let nothing = match node.kind {
            NodeKind::Ident(name) => self.ast.atom(name) == "undefined" && self.is_global_undefined(arg),
            NodeKind::Unary { op: UnaryOp::Void, arg: inner } => matches!(self.ast[inner].kind, NodeKind::Num(v) if v.get() == 0.0),
            _ => false,
        };
        if nothing {
            let from = self.first_tok(node.span.start);
            let to = self.last_tok(node.span.end) + 1;
            self.dead.push((from, to));
        }
    }

    // ---- branches that are decided ahead of time -----------------------------------------

    fn declares_var(&self, root: NodeId) -> bool {
        declares_var(self.ast, root)
    }

    fn in_statement_list(&self, n: NodeId) -> bool {
        self.parent[n.index()].is_some_and(|p| matches!(self.ast[p].kind, NodeKind::Program(_) | NodeKind::Block(_) | NodeKind::StaticBlock(_) | NodeKind::Case { .. }))
    }

    /// What a condition is when it is the same every time (`true`, `"a" === "a"`, `!0`).
    fn constant(&self, test: NodeId) -> Option<bool> {
        if let NodeKind::Bool(value) = self.ast[test].kind {
            return Some(value);
        }
        crate::define::Defines::default().eval(self.ast, self.sem, test).map(|c| c.truthy())
    }

    /// `if (true) a; else b;` is `a;`, and `if (false) a;` is nothing.
    fn dead_if(&mut self, n: NodeId, value: bool, cons: NodeId, alt: Option<NodeId>) {
        let (keep, drop) = if value { (Some(cons), alt) } else { (alt, Some(cons)) };
        if drop.is_some_and(|d| self.declares_var(d)) {
            return;
        }
        let start = self.first_tok(self.ast[n].span.start);
        let end = self.last_tok(self.ast[n].span.end);
        let Some(keep) = keep else {
            self.dead.push((start, end + 1));
            if !self.in_statement_list(n) {
                self.kept_semi.push(start);
            }
            return;
        };
        if self.ends_open(keep) && self.else_follows(n) {
            return;
        }
        let keep_first = self.first_tok(self.ast[keep].span.start);
        if value && alt.is_some() {
            let mut j = self.last_tok(self.ast[keep].span.end) + 1;
            while self.is(j, Punct::Semi) {
                j += 1;
            }
            if !self.kw(j, Kw::Else) {
                return;
            }
            self.dead.push((j, end + 1));
        }
        self.dead.push((start, keep_first));
    }

    /// `true ? a : b` is `a`.
    fn dead_conditional(&mut self, n: NodeId, value: bool, test: NodeId, cons: NodeId, alt: NodeId) {
        if self.ast[test].has(flags::PARENTHESIZED) {
            return;
        }
        let start = self.first_tok(self.ast[n].span.start);
        let end = self.last_tok(self.ast[n].span.end);
        let mut q = self.last_tok(self.ast[test].span.end) + 1;
        while self.is(q, Punct::RParen) {
            q += 1;
        }
        let mut c = self.last_tok(self.ast[cons].span.end) + 1;
        while self.is(c, Punct::RParen) {
            c += 1;
        }
        if !self.is(q, Punct::Question) || !self.is(c, Punct::Colon) {
            return;
        }
        let kept_first = if value { q + 1 } else { c + 1 };
        let _ = alt;
        // What starts a statement or arrow body would read differently on its own.
        let starts_badly = match self.tokens.get(kept_first) {
            Some(t) => matches!(t.kind, Kind::Punct(Punct::LBrace) | Kind::Keyword(Kw::Function | Kw::Class)) || (t.kind == Kind::Ident && matches!(self.text(kept_first), "async" | "let")),
            None => true,
        };
        if starts_badly {
            return;
        }
        if value {
            self.dead.push((start, q + 1));
            self.dead.push((c, end + 1));
        } else {
            self.dead.push((start, c + 1));
        }
    }
}

/// Whether running `root` could declare a `var` visible outside it, which removing the code
/// would change.
pub(crate) fn declares_var(ast: &Ast, root: NodeId) -> bool {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match ast[n].kind {
            NodeKind::VarDecl { kind: VarKind::Var, .. } => return true,
            NodeKind::Function { .. } | NodeKind::StaticBlock(_) => continue,
            _ => {}
        }
        ast.for_each_child(n, |c| stack.push(c));
    }
    false
}

fn is_plain_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'$') && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}

/// The shortest way to write the number `text`, if shorter than `text`.
pub(crate) fn shorten_number(text: &str) -> Option<String> {
    let clean: String = text.chars().filter(|&c| c != '_').collect();
    let b = clean.as_bytes();
    // `017` and `08` mean other things in sloppy code; leave them.
    if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
        return None;
    }
    let value = if b.len() > 2 && b[0] == b'0' && matches!(b[1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            _ => 2,
        };
        let v = u64::from_str_radix(&clean[2..], radix).ok()?;
        if v >= 1 << 53 {
            return None;
        }
        v as f64
    } else {
        clean.parse::<f64>().ok()?
    };
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let best = shortest(value);
    (best.len() < text.len()).then_some(best)
}

fn shortest(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    // Rust prints the fewest digits that read back as the same number: `1.5e-7`.
    let scientific = format!("{v:e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("an exponent");
    let exp: i32 = exponent.parse().expect("an exponent number");
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let n = digits.len() as i32;
    let plain = if exp >= 0 {
        if n <= exp + 1 {
            format!("{digits}{}", "0".repeat((exp + 1 - n) as usize))
        } else {
            format!("{}.{}", &digits[..(exp + 1) as usize], &digits[(exp + 1) as usize..])
        }
    } else {
        format!(".{}{digits}", "0".repeat((-exp - 1) as usize))
    };
    let shift = exp - (n - 1);
    let exponential = if shift == 0 { digits.clone() } else { format!("{digits}e{shift}") };
    if exponential.len() < plain.len() { exponential } else { plain }
}

#[cfg(test)]
mod tests {
    use super::shorten_number;

    #[test]
    fn numbers_take_their_shortest_spelling() {
        let cases = [
            ("1000", "1e3"),
            ("0.5", ".5"),
            ("1.0", "1"),
            ("0x10", "16"),
            ("1_000_000", "1e6"),
            ("0.00015", "15e-5"),
            ("1500000", "15e5"),
            ("100", "100"),
            ("1e3", "1e3"),
            ("0.0", "0"),
            ("5.", "5"),
            ("0b11", "3"),
            ("123456789", "123456789"),
            ("1.5e10", "15e9"),
        ];
        for (input, want) in cases {
            let got = shorten_number(input).unwrap_or_else(|| input.to_string());
            assert_eq!(got, want, "{input}");
        }
        // Leave legacy octal and anything too big to be exact alone.
        assert_eq!(shorten_number("017"), None);
        assert_eq!(shorten_number("0xFFFFFFFFFFFFFFFFF"), None);
    }
}
