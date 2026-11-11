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
/// Whether a `const` declaration can be written `let`: nothing assigns to its names (an assignment to a
/// constant is an error that `let` would swallow), and no `eval` can reach them. `let` is two letters shorter.
pub(crate) fn const_is_let(ast: &Ast, sem: &Semantic, decl: NodeId) -> bool {
    let NodeKind::VarDecl { kind: VarKind::Const, decls } = ast[decl].kind else { return false };
    let mut stack: Vec<NodeId> = ast.list(decls).iter().filter_map(|&d| if let NodeKind::VarDeclarator { target, .. } = ast[d].kind { Some(target) } else { None }).collect();
    while let Some(n) = stack.pop() {
        if matches!(ast[n].kind, NodeKind::BindingIdent(_)) {
            let Some(sym) = sem.symbol_of(n) else { return false };
            let symbol = sem.symbol(sym);
            if symbol.writes > 1 || symbol.flags & crate::semantic::symbol_flags::DYNAMIC != 0 || sem.scope(symbol.scope).flags & crate::semantic::scope_flags::HAS_EVAL != 0 {
                return false;
            }
        } else {
            ast.for_each_child(n, |c| stack.push(c));
        }
    }
    true
}

pub(crate) fn plan(source: &str, ast: &Ast, sem: &Semantic, tokens: &[Token], barriers: &[u32]) -> Plan {
    let mut ctx = Ctx { source, ast, sem, tokens, barriers, parent: vec![None; ast.node_count() + 1], actions: vec![Action::Keep; tokens.len()], locked: vec![false; tokens.len()], dead: Vec::new(), kept_semi: Vec::new(), texts: Vec::new(), text_index: Default::default() };
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
    /// Tokens a rewrite has decided on, which later rules (visiting the children) must not change.
    locked: Vec<bool>,
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

    /// Whether the string `n` stands where a template literal means the same: the value of a variable or property, an assignment, an array
    /// element, a returned value or an argument. Not a directive, a key, a module name or the operand of an operator, and not what
    /// `require` is given (other tools read that as a string).
    fn string_may_be_template(&self, n: NodeId) -> bool {
        let Some(parent) = self.parent[n.index()] else { return false };
        match self.ast[parent].kind {
            NodeKind::VarDeclarator { init, .. } => init == Some(n),
            NodeKind::Assign { value, .. } | NodeKind::Property { value, .. } => value == n && !self.ast[parent].has(flags::SHORTHAND),
            NodeKind::Array(_) | NodeKind::Return(_) => true,
            NodeKind::Call { callee, .. } => callee != n && !matches!(self.ast[callee].kind, NodeKind::Ident(a) if self.ast.atom(a) == "require"),
            _ => false,
        }
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
        if self.locked.get(i) == Some(&true) {
            return;
        }
        if let Some(slot) = self.actions.get_mut(i) {
            *slot = action;
        }
    }

    /// As `set`, and nothing set later changes it.
    fn force(&mut self, i: usize, action: Action) {
        if let Some(slot) = self.actions.get_mut(i) {
            *slot = action;
            self.locked[i] = true;
        }
    }

    fn force_text(&mut self, i: usize, text: &str) {
        let action = self.text_action(text);
        self.force(i, action);
    }

    fn visit(&mut self, n: NodeId) {
        let ast = self.ast;
        let node = &ast[n];
        if node.has(flags::PARENTHESIZED) {
            self.drop_parens(n);
        }
        if let NodeKind::Function { params, .. } = node.kind {
            if node.has(flags::ARROW) {
                self.arrow_parameter(n, params);
            }
        }
        match node.kind {
            NodeKind::Bool(_) if self.parent[n.index()].is_some_and(|p| matches!(ast[p].kind, NodeKind::While { test, .. } if test == n)) => {}
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
            // The global `Infinity`, read only: `1/0`, in parentheses where an operator that binds tighter than `/` stands beside it.
            NodeKind::Ident(name) if ast.atom(name) == "Infinity" && self.is_global_undefined(n) && !self.is_shorthand_value(n) => {
                let i = self.first_tok(node.span.start);
                if self.tokens.get(i).is_some_and(|t| t.kind == Kind::Ident) {
                    let text = if self.division_may_stand_here(n) { "1/0" } else { "(1/0)" };
                    self.set_text(i, text);
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
            NodeKind::Str(_) if self.string_may_be_template(n) => {
                let i = self.first_tok(node.span.start);
                if self.tokens.get(i).is_some_and(|t| t.kind == Kind::String) {
                    if let Some(template) = as_template(self.text(i)) {
                        self.set_text(i, &template);
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
            NodeKind::VarDecl { kind: VarKind::Const, .. } if const_is_let(ast, self.sem, n) => {
                let i = self.first_tok(node.span.start);
                // Not when the declaration was joined to the one before it: its keyword is gone.
                if self.kw(i, Kw::Const) && self.actions[i] == Action::Keep {
                    self.set_text(i, "let");
                }
            }
            NodeKind::If { test, cons, alt } => {
                self.unbrace(cons);
                if let Some(alt) = alt {
                    self.unbrace(alt);
                }
                if let Some(value) = self.constant(test) {
                    self.dead_if(n, value, cons, alt);
                } else {
                    self.if_to_expression(n, test, cons, alt);
                }
            }
            NodeKind::While { test, body } if matches!(ast[test].kind, NodeKind::Bool(true)) && !ast[test].has(flags::PARENTHESIZED) => {
                self.unbrace(body);
                // `while (true)` is `for (;;)`.
                let w = self.first_tok(node.span.start);
                let t = self.first_tok(ast[test].span.start);
                if self.kw(w, Kw::While) && self.is(w + 1, Punct::LParen) && t == w + 2 && self.is(t + 1, Punct::RParen) {
                    self.set_text(w, "for");
                    self.set_text(t, ";;");
                }
            }
            NodeKind::Binary { op, left, right } => self.typeof_undefined(op, left, right),
            NodeKind::For { body, .. } | NodeKind::ForIn { body, .. } | NodeKind::ForOf { body, .. } | NodeKind::While { body, .. } | NodeKind::DoWhile { body, .. } => self.unbrace(body),
            NodeKind::Program(list) | NodeKind::Block(list) | NodeKind::StaticBlock(list) => {
                self.merge_declarations(ast.list(list));
                self.merge_returns(ast.list(list));
            }
            NodeKind::Case { body, .. } => {
                self.merge_declarations(ast.list(body));
                self.merge_returns(ast.list(body));
            }
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

    /// Whether `a / b` can stand where `n` stands without parentheses: its parent is not an operator or a form that binds tighter than `/`.
    /// A sign in front (`-Infinity`) goes before the whole division, which reads the same, so that sign's own parent decides.
    fn division_may_stand_here(&self, n: NodeId) -> bool {
        let Some(mut parent) = self.parent[n.index()] else { return false };
        let mut child = n;
        if let NodeKind::Unary { op: UnaryOp::Neg | UnaryOp::Pos, .. } = self.ast[parent].kind {
            // `-1/0` is `(-1)/0`, which is the same value as `-(1/0)`.
            child = parent;
            let Some(grand) = self.parent[parent.index()] else { return false };
            parent = grand;
        }
        match self.ast[parent].kind {
            NodeKind::VarDeclarator { init, .. } => init == Some(child),
            NodeKind::Assign { value, .. } | NodeKind::Property { value, .. } => value == child,
            NodeKind::Array(_) | NodeKind::Return(_) | NodeKind::Sequence(_) => true,
            NodeKind::Call { callee, .. } | NodeKind::New { callee, .. } => callee != child,
            NodeKind::Conditional { .. } => true,
            NodeKind::Binary { op, .. } => {
                use crate::ast::BinOp::*;
                !matches!(op, Mul | Div | Rem | Exp)
            }
            _ => false,
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

    // ---- `if` as an expression --------------------------------------------------------------

    /// The expression of a statement that is one, looking through a block whose braces went.
    fn sole_expression(&self, s: NodeId) -> Option<NodeId> {
        match self.ast[s].kind {
            NodeKind::ExprStmt(e) if !self.ast[s].has(flags::DIRECTIVE) => Some(e),
            NodeKind::Block(list) => {
                let [only] = self.ast.list(list) else { return None };
                let open = self.first_tok(self.ast[s].span.start);
                if self.actions.get(open) == Some(&Action::Skip) { self.sole_expression(*only) } else { None }
            }
            _ => None,
        }
    }

    /// The `return` argument of a statement that is one, looking through a block whose braces went.
    fn sole_return(&self, s: NodeId) -> Option<(NodeId, NodeId)> {
        match self.ast[s].kind {
            NodeKind::Return(Some(arg)) => Some((s, arg)),
            NodeKind::Block(list) => {
                let [only] = self.ast.list(list) else { return None };
                let open = self.first_tok(self.ast[s].span.start);
                if self.actions.get(open) == Some(&Action::Skip) { self.sole_return(*only) } else { None }
            }
            _ => None,
        }
    }

    /// The `;` that ends statement `s`, if it is written.
    fn semi_of(&self, s: NodeId) -> Option<usize> {
        let last = self.last_tok(self.ast[s].span.end);
        if self.is(last, Punct::Semi) { Some(last) } else { None }
    }

    fn starts_statement_badly(&self, n: NodeId) -> bool {
        let first = self.first_tok(self.ast[n].span.start);
        match self.tokens.get(first) {
            Some(t) => matches!(t.kind, Kind::Punct(Punct::LBrace) | Kind::Keyword(Kw::Function | Kw::Class)) || (t.kind == Kind::Ident && matches!(self.text(first), "async" | "let")),
            None => true,
        }
    }

    /// `if (a) b();` is `a&&b();`, `if (!a) b();` is `a||b();` and `if (a) b(); else c();` is `a?b():c();`.
    fn if_to_expression(&mut self, n: NodeId, test: NodeId, cons: NodeId, alt: Option<NodeId>) {
        let ast = self.ast;
        let Some(ce) = self.sole_expression(cons) else {
            if let Some(alt) = alt {
                self.if_else_sequences(n, test, cons, alt);
            }
            return;
        };
        let if_tok = self.first_tok(ast[n].span.start);
        let open = if_tok + 1;
        let close = self.first_tok(ast[cons].span.start).wrapping_sub(1);
        if !self.kw(if_tok, Kw::If) || !self.is(open, Punct::LParen) || !self.is(close, Punct::RParen) || self.starts_statement_badly(test) {
            return;
        }
        let keeps_parens = ast[test].has(flags::PARENTHESIZED);
        let test_precedence = self.precedence(test);
        match alt {
            None => {
                let ce_precedence = self.precedence(ce);
                if let (NodeKind::Unary { op: crate::ast::UnaryOp::Not, arg }, false) = (ast[test].kind, keeps_parens) {
                    let bang = self.first_tok(ast[test].span.start);
                    let arg_precedence = self.precedence(arg);
                    let arg_nullish = matches!(ast[arg].kind, NodeKind::Binary { op: crate::ast::BinOp::Nullish, .. }) && !ast[arg].has(flags::PARENTHESIZED);
                    if arg_precedence >= 6 && !arg_nullish && ce_precedence >= 6 && !self.starts_statement_badly(arg) && self.is(bang, Punct::Bang) {
                        self.force(if_tok, Action::Skip);
                        self.force(open, Action::Skip);
                        self.force(bang, Action::Skip);
                        self.force_text(close, "||");
                    }
                } else if test_precedence >= 6 && !keeps_parens && ce_precedence >= 7 {
                    self.force(if_tok, Action::Skip);
                    self.force(open, Action::Skip);
                    self.force_text(close, "&&");
                }
            }
            Some(alt) => {
                let Some(ae) = self.sole_expression(alt) else { return self.if_else_sequences(n, test, cons, alt) };
                let Some(cons_semi) = self.sole_statement_semi(cons) else { return };
                let else_tok = self.last_tok(ast[cons].span.end) + 1;
                if !self.kw(else_tok, Kw::Else) || self.precedence(ce) < 2 || self.precedence(ae) < 2 {
                    return;
                }
                self.force(if_tok, Action::Skip);
                if keeps_parens || test_precedence <= 3 {
                    self.force_text(close, ")?");
                } else {
                    self.force(open, Action::Skip);
                    self.force_text(close, "?");
                }
                self.force_text(cons_semi, ":");
                self.force(else_tok, Action::Skip);
            }
        }
    }

    /// A body that `block_sequence` made one comma expression: the block, its statements (at least two, all expressions), the `;` of the
    /// last one and the braces.
    fn expression_block(&self, s: NodeId) -> Option<(usize, usize, usize)> {
        let NodeKind::Block(list) = self.ast[s].kind else { return None };
        let items = self.ast.list(list);
        let (&last, before) = items.split_last()?;
        if before.is_empty() || !matches!(self.ast[last].kind, NodeKind::ExprStmt(_)) || before.iter().any(|&b| !matches!(self.ast[b].kind, NodeKind::ExprStmt(_))) {
            return None;
        }
        let open = self.first_tok(self.ast[s].span.start);
        let close = self.last_tok(self.ast[s].span.end);
        // Its braces were taken off; the `;` of the last statement is written, so there is one to turn into the closing parenthesis.
        if self.actions[open] != Action::Skip || !self.is(close, Punct::RBrace) || close == 0 || !self.is(close - 1, Punct::Semi) || self.actions[close - 1] != Action::Keep {
            return None;
        }
        Some((open, close - 1, close))
    }

    /// `if (a) { b(); c(); } else { d(); }` is `a?(b(),c()):d();`: each body is one expression, in parentheses when it is a sequence.
    fn if_else_sequences(&mut self, n: NodeId, test: NodeId, cons: NodeId, alt: NodeId) {
        let ast = self.ast;
        // How each body is written: a sequence block, or a single expression statement.
        let cons_block = self.expression_block(cons);
        let alt_block = self.expression_block(alt);
        if cons_block.is_none() && alt_block.is_none() {
            return;
        }
        let (ce, ae) = (self.sole_expression(cons), self.sole_expression(alt));
        if (cons_block.is_none() && ce.is_none_or(|e| self.precedence(e) < 2)) || (alt_block.is_none() && ae.is_none_or(|e| self.precedence(e) < 2)) {
            return;
        }
        let cons_end = match cons_block {
            Some((_, semi, close)) => (semi, close),
            None => match self.sole_statement_semi(cons) {
                Some(semi) => (semi, semi),
                None => return,
            },
        };
        let if_tok = self.first_tok(ast[n].span.start);
        let open = if_tok + 1;
        let close = self.first_tok(ast[cons].span.start).wrapping_sub(1);
        let else_tok = self.last_tok(ast[cons].span.end) + 1;
        if !self.kw(if_tok, Kw::If) || !self.is(open, Punct::LParen) || !self.is(close, Punct::RParen) || self.starts_statement_badly(test) || !self.kw(else_tok, Kw::Else) {
            return;
        }
        // `{` is the first token of a block body, which is where the `(` goes; a body that is a sequence has no braces left.
        let keeps_parens = ast[test].has(flags::PARENTHESIZED);
        self.force(if_tok, Action::Skip);
        if keeps_parens || self.precedence(test) <= 3 {
            self.force_text(close, ")?");
        } else {
            self.force(open, Action::Skip);
            self.force_text(close, "?");
        }
        match cons_block {
            Some((brace, semi, end)) => {
                self.force_text(brace, "(");
                self.force_text(semi, ")");
                self.force(end, Action::Skip);
                self.force_text(else_tok, ":");
            }
            None => {
                self.force_text(cons_end.0, ":");
                self.force(else_tok, Action::Skip);
            }
        }
        if let Some((brace, semi, end)) = alt_block {
            self.force_text(brace, "(");
            self.force_text(semi, ")");
            self.force(end, Action::Semi);
        }
    }

    /// The `;` of the only statement in `s` (a statement, or a block that holds one).
    fn sole_statement_semi(&self, s: NodeId) -> Option<usize> {
        match self.ast[s].kind {
            NodeKind::Block(list) => {
                let [only] = self.ast.list(list) else { return None };
                // Without a `;` of its own, the `}` of the block is what ends the statement.
                self.semi_of(*only).or_else(|| {
                    let close = self.last_tok(self.ast[s].span.end);
                    if self.is(close, Punct::RBrace) { Some(close) } else { None }
                })
            }
            _ => self.semi_of(s),
        }
    }

    /// `if (a) return b; return c;` is `return a?b:c;`, and so is `if (a) return b; else return c;`.
    fn merge_returns(&mut self, list: &[NodeId]) {
        let ast = self.ast;
        // From the end, so that `if (a) return 1; if (b) return 2; return 3;` is `return a?1:b?2:3;`: the pair at the end is joined first, and the
        // statement before it is then followed by a return (the joined pair).
        let mut joined: Vec<NodeId> = Vec::new();
        for (i, &s) in list.iter().enumerate().rev() {
            let NodeKind::If { test, cons, alt } = ast[s].kind else { continue };
            if self.constant(test).is_some() {
                continue;
            }
            // The braces of a one-statement body are about to go (the `if` is visited after its list).
            self.unbrace(cons);
            if let Some(alt) = alt {
                self.unbrace(alt);
            }
            let Some((cons_return, a)) = self.sole_return(cons) else { continue };
            let (other_return, b, else_tok) = match alt {
                Some(alt) => match self.sole_return(alt) {
                    Some((r, b)) => (r, b, Some(self.last_tok(ast[cons].span.end) + 1)),
                    None => continue,
                },
                None => match list.get(i + 1).map(|&next| (next, ast[next].kind)) {
                    Some((next, NodeKind::Return(Some(b)))) => (next, b, None),
                    // The next statement was joined into a return of its own: its `return` is the one that goes.
                    Some((next, NodeKind::If { .. })) if joined.contains(&next) => (next, next, None),
                    _ => continue,
                },
            };
            let chained = other_return == b;
            // `return undefined` is written as `return`, which has no value to put in a condition.
            if self.precedence(a) < 2 || (!chained && (self.precedence(b) < 2 || self.is_nothing(b))) || self.is_nothing(a) {
                continue;
            }
            let if_tok = self.first_tok(ast[s].span.start);
            let open = if_tok + 1;
            let close = self.first_tok(ast[cons].span.start).wrapping_sub(1);
            let Some(cons_semi) = self.sole_statement_semi(cons) else { continue };
            if !self.kw(if_tok, Kw::If) || !self.is(open, Punct::LParen) || !self.is(close, Punct::RParen) || self.starts_statement_badly(test) {
                continue;
            }
            let first_return = self.first_tok(ast[cons_return].span.start);
            let second_return = self.first_tok(ast[other_return].span.start);
            if !self.kw(first_return, Kw::Return) || !(self.kw(second_return, Kw::Return) || (chained && self.kw(second_return, Kw::If))) {
                continue;
            }
            if let Some(e) = else_tok {
                if !self.kw(e, Kw::Else) {
                    continue;
                }
                self.force(e, Action::Skip);
            }
            self.force_text(if_tok, "return");
            if ast[test].has(flags::PARENTHESIZED) || self.precedence(test) <= 3 {
                self.force_text(close, ")?");
            } else {
                self.force(open, Action::Skip);
                self.force_text(close, "?");
            }
            self.force(first_return, Action::Skip);
            self.force_text(cons_semi, ":");
            self.force(second_return, Action::Skip);
            joined.push(s);
        }
    }

    /// `typeof x === "undefined"` is `typeof x>"u"`: of every `typeof` result only "undefined" sorts after "u".
    fn typeof_undefined(&mut self, op: crate::ast::BinOp, left: NodeId, right: NodeId) {
        use crate::ast::BinOp::*;
        let ast = self.ast;
        let equal = matches!(op, StrictEq | Eq);
        if !(equal || matches!(op, StrictNe | Ne)) || !matches!(ast[left].kind, NodeKind::Unary { op: crate::ast::UnaryOp::Typeof, .. }) {
            return;
        }
        let NodeKind::Str(text) = ast[right].kind else { return };
        if ast.atom(text) != "undefined" || ast[right].has(flags::PARENTHESIZED) || ast[left].has(flags::PARENTHESIZED) {
            return;
        }
        let value = self.first_tok(ast[right].span.start);
        let operator = value.wrapping_sub(1);
        let wanted = match (op, self.tokens.get(operator).map(|t| t.kind)) {
            (StrictEq, Some(Kind::Punct(Punct::EqEqEq))) | (Eq, Some(Kind::Punct(Punct::EqEq))) | (StrictNe, Some(Kind::Punct(Punct::NotEqEq))) | (Ne, Some(Kind::Punct(Punct::NotEq))) => true,
            _ => false,
        };
        if wanted && self.tokens.get(value).is_some_and(|t| t.kind == Kind::String) {
            self.set_text(operator, if equal { ">" } else { "<" });
            self.set_text(value, "\"u\"");
        }
    }

    // ---- parentheses that change nothing -------------------------------------------------

    /// Takes the parentheses off an expression that does not need them where it stands.
    fn drop_parens(&mut self, n: NodeId) {
        let ast = self.ast;
        let Some(parent) = self.parent[n.index()] else { return };
        // `true` and `undefined` are written as unary expressions that choose their own parentheses.
        if matches!(ast[n].kind, NodeKind::Bool(_)) || matches!(ast[n].kind, NodeKind::Ident(name) if ast.atom(name) == "undefined") {
            return;
        }
        let open = self.first_tok(ast[n].span.start).wrapping_sub(1);
        let close = self.last_tok(ast[n].span.end) + 1;
        if !self.is(open, Punct::LParen) || !self.is(close, Punct::RParen) {
            return;
        }
        // What starts a statement or an arrow body would be read as something else.
        let first = open + 1;
        let starts_badly = match self.tokens.get(first) {
            Some(t) => matches!(t.kind, Kind::Punct(Punct::LBrace) | Kind::Keyword(Kw::Function | Kw::Class)) || (t.kind == Kind::Ident && matches!(self.text(first), "async" | "let")),
            None => true,
        };
        if starts_badly || self.in_for_head(n) || !self.parens_unneeded(n, parent) {
            return;
        }
        // `((x))`: the outer pair is not this node's, and may be needed.
        if self.is(open.wrapping_sub(1), Punct::LParen) && self.is(close + 1, Punct::RParen) {
            return;
        }
        self.set(open, Action::Skip);
        self.set(close, Action::Skip);
    }

    /// Inside the head of a `for`, an `in` must stay inside parentheses.
    fn in_for_head(&self, mut n: NodeId) -> bool {
        while let Some(p) = self.parent[n.index()] {
            if let NodeKind::For { init: Some(init), .. } = self.ast[p].kind {
                if init == n {
                    return true;
                }
            }
            n = p;
        }
        false
    }

    /// How tightly an expression binds (higher is tighter); the precedence levels of the language.
    fn precedence(&self, n: NodeId) -> u8 {
        use crate::ast::BinOp::*;
        match self.ast[n].kind {
            NodeKind::Sequence(_) => 1,
            NodeKind::Assign { .. } | NodeKind::Yield(_) => 2,
            NodeKind::Function { .. } if self.ast[n].has(flags::ARROW) => 2,
            NodeKind::Conditional { .. } => 3,
            NodeKind::Binary { op, .. } => match op {
                Nullish => 4,
                LogOr => 5,
                LogAnd => 6,
                BitOr => 7,
                BitXor => 8,
                BitAnd => 9,
                Eq | Ne | StrictEq | StrictNe => 10,
                Lt | Le | Gt | Ge | In | InstanceOf => 11,
                Shl | Shr | UShr => 12,
                Add | Sub => 13,
                Mul | Div | Rem => 14,
                Exp => 15,
            },
            NodeKind::Unary { .. } | NodeKind::Await(_) => 16,
            NodeKind::Update { .. } => 17,
            NodeKind::New { .. } | NodeKind::Call { .. } | NodeKind::Member { .. } | NodeKind::TaggedTemplate { .. } | NodeKind::Chain(_) | NodeKind::ImportCall { .. } => 19,
            _ => 20,
        }
    }

    /// Whether `n`, written without its parentheses, still parses as `n` in the place its parent puts it.
    fn parens_unneeded(&self, n: NodeId, parent: NodeId) -> bool {
        let ast = self.ast;
        let p = self.precedence(n);
        // A logical operator mixed with `??` needs them either way.
        let mixes_nullish = |a: crate::ast::BinOp, b: crate::ast::BinOp| {
            use crate::ast::BinOp::*;
            (a == Nullish && matches!(b, LogOr | LogAnd)) || (b == Nullish && matches!(a, LogOr | LogAnd))
        };
        match ast[parent].kind {
            NodeKind::Binary { op, left, .. } => {
                if let NodeKind::Binary { op: inner, .. } = ast[n].kind {
                    if mixes_nullish(op, inner) {
                        return false;
                    }
                }
                let own = self.precedence(parent);
                if op == crate::ast::BinOp::Exp {
                    // `(-1) ** 2` and `(await x) ** 2` are errors without them; the operator groups to the right.
                    if n == left {
                        return p > own && !matches!(ast[n].kind, NodeKind::Unary { .. } | NodeKind::Await(_));
                    }
                    return p >= own;
                }
                // `a + (b + c)` is not `a + b + c` for strings and floats: the right side needs strictly tighter.
                if n == left { p >= own } else { p > own }
            }
            NodeKind::Unary { op, .. } => {
                if p < 16 {
                    return false;
                }
                // `-(-x)` and `+(+x)` would become `--x` and `++x`.
                let first = self.first_tok(ast[n].span.start);
                let sign = |c: char| self.tokens.get(first).is_some_and(|t| self.source[t.start as usize..].starts_with(c));
                !(matches!(op, crate::ast::UnaryOp::Neg) && sign('-') || matches!(op, crate::ast::UnaryOp::Pos) && sign('+'))
            }
            NodeKind::Await(_) => p >= 16,
            NodeKind::Update { .. } => false,
            NodeKind::Conditional { test, .. } => {
                if n == test {
                    p > 3
                } else {
                    p >= 2 && !matches!(ast[n].kind, NodeKind::Sequence(_))
                }
            }
            NodeKind::Assign { target, .. } => n != target && p >= 2,
            // An argument, element or value is an assignment expression: a comma would split it.
            NodeKind::Call { callee, .. } => {
                if n == callee {
                    // `(a?.b)()` and `(a.b)()` differ from `a?.b()` only for chains; a call or member is fine.
                    matches!(ast[n].kind, NodeKind::Ident(_)) || (matches!(ast[n].kind, NodeKind::Member { .. } | NodeKind::Call { .. }) && !self.has_chain_or_new(n))
                } else {
                    p >= 2
                }
            }
            NodeKind::New { callee, .. } => {
                if n == callee {
                    // `new (a.b)()` is `new a.b()`, but a call or an optional link inside would change what is built.
                    matches!(ast[n].kind, NodeKind::Ident(_)) || (matches!(ast[n].kind, NodeKind::Member { .. }) && !self.has_call_or_chain(n))
                } else {
                    p >= 2
                }
            }
            NodeKind::Member { object, prop } => {
                if n == prop {
                    false
                } else if n == object {
                    // `(1).x` would read `1.x`; `(new A).x` is `new A.x`; chains end where the parentheses end;
                    // and under `new`, `new (f()).x()` is not `new f().x()`.
                    matches!(ast[n].kind, NodeKind::Ident(_) | NodeKind::This | NodeKind::Str(_) | NodeKind::Array(_) | NodeKind::Template { .. }) || (matches!(ast[n].kind, NodeKind::Member { .. } | NodeKind::Call { .. }) && !self.has_chain_or_new(n) && !self.under_new(parent))
                } else {
                    false
                }
            }
            NodeKind::Array(_) | NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) => p >= 2,
            NodeKind::Property { value, .. } => n == value && p >= 2 && !ast[parent].has(flags::COMPUTED),
            NodeKind::ExprStmt(_) => !ast[parent].has(flags::DIRECTIVE),
            NodeKind::VarDeclarator { init, .. } => init == Some(n) && p >= 2,
            NodeKind::Function { body, .. } if ast[parent].has(flags::ARROW) && ast[parent].has(flags::EXPR_BODY) => body == n && p >= 2,
            NodeKind::Spread(_) => p >= 2,
            NodeKind::Sequence(_) => p >= 1,
            _ => false,
        }
    }

    /// Whether the member chain `n` is (the end of) what a `new` constructs.
    fn under_new(&self, mut n: NodeId) -> bool {
        while let Some(p) = self.parent[n.index()] {
            match self.ast[p].kind {
                NodeKind::Member { object, .. } if object == n => n = p,
                NodeKind::New { callee, .. } => return callee == n,
                _ => return false,
            }
        }
        false
    }

    /// Whether an optional link or a `new` without arguments sits in the chain of members and calls under `n`.
    fn has_chain_or_new(&self, mut n: NodeId) -> bool {
        loop {
            let node = &self.ast[n];
            if node.has(flags::OPTIONAL) {
                return true;
            }
            match node.kind {
                NodeKind::Member { object, .. } => n = object,
                NodeKind::Call { callee, .. } => n = callee,
                NodeKind::New { args, .. } => return self.ast.list(args).is_empty(),
                NodeKind::Chain(_) => return true,
                _ => return false,
            }
        }
    }

    fn has_call_or_chain(&self, mut n: NodeId) -> bool {
        loop {
            match self.ast[n].kind {
                NodeKind::Member { object, .. } => {
                    if self.ast[n].has(flags::OPTIONAL) {
                        return true;
                    }
                    n = object;
                }
                NodeKind::Ident(_) | NodeKind::This => return false,
                _ => return true,
            }
        }
    }

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
        let [s] = items else { return self.block_sequence(block) };
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

    /// `if (a) { b(); c(); }` is `if (a) b(), c();` and `if (a) { b(); return c; }` is `if (a) return b(), c;`: a body of expression statements
    /// (and perhaps a `return` or `throw` at the end) is one comma expression, which needs no braces.
    fn block_sequence(&mut self, block: NodeId) {
        let ast = self.ast;
        let NodeKind::Block(list) = ast[block].kind else { return };
        let items = ast.list(list);
        let Some((&last, before)) = items.split_last() else { return };
        if before.is_empty() {
            return;
        }
        let mut commas = Vec::with_capacity(before.len());
        for &s in before {
            if !matches!(ast[s].kind, NodeKind::ExprStmt(_)) || ast[s].has(flags::DIRECTIVE) {
                return;
            }
            // A line break that stood for the `;` has no token to become a comma.
            match self.semi_of(s) {
                Some(semi) if self.actions[semi] == Action::Keep => commas.push(semi),
                _ => return,
            }
        }
        if self.starts_statement_badly(before[0]) {
            return;
        }
        // The last statement decides what the sequence is: an expression, a returned value or a thrown one.
        let keyword = match ast[last].kind {
            NodeKind::ExprStmt(_) if !ast[last].has(flags::DIRECTIVE) => None,
            NodeKind::Return(Some(arg)) if !self.is_nothing(arg) => Some(Kw::Return),
            NodeKind::Throw(_) => Some(Kw::Throw),
            _ => return,
        };
        let open = self.first_tok(ast[block].span.start);
        let close = self.last_tok(ast[block].span.end);
        if !self.is(open, Punct::LBrace) || !self.is(close, Punct::RBrace) {
            return;
        }
        if let Some(kw) = keyword {
            let at = self.first_tok(ast[last].span.start);
            if !self.kw(at, kw) || self.actions[at] != Action::Keep {
                return;
            }
            self.set_text(open, if kw == Kw::Return { "return" } else { "throw" });
            self.set(at, Action::Skip);
        } else {
            self.set(open, Action::Skip);
        }
        for semi in commas {
            self.set_text(semi, ",");
        }
        // The last statement's own `;` stays; one that a line break or the `}` stood for is written.
        if self.is(close - 1, Punct::Semi) && close - 1 > open {
            self.set(close, Action::Skip);
        } else {
            self.set(close, Action::Semi);
        }
    }

    /// `var a = 1; var b = 2;` is `var a = 1, b = 2;`.
    fn merge_declarations(&mut self, items: &[NodeId]) {
        for pair in items.windows(2) {
            let (NodeKind::VarDecl { kind: mut k1, .. }, NodeKind::VarDecl { kind: mut k2, .. }) = (self.ast[pair[0]].kind, self.ast[pair[1]].kind) else { continue };
            // A `const` that is written `let` joins its neighbors.
            if k1 == VarKind::Const && const_is_let(self.ast, self.sem, pair[0]) {
                k1 = VarKind::Let;
            }
            if k2 == VarKind::Const && const_is_let(self.ast, self.sem, pair[1]) {
                k2 = VarKind::Let;
            }
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
        if self.ast.list(list).len() > 1 {
            return self.arrow_sequence(f, body);
        }
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

    /// `=> { a(); b(); return c; }` is `=> (a(), b(), c)`.
    fn arrow_sequence(&mut self, f: NodeId, body: NodeId) {
        let ast = self.ast;
        let NodeKind::Block(list) = ast[body].kind else { return };
        let items = ast.list(list);
        let Some((&last, before)) = items.split_last() else { return };
        let NodeKind::Return(Some(arg)) = ast[last].kind else { return };
        // `return undefined` loses its value to `return_nothing`, which would leave the sequence unfinished.
        if self.is_nothing(arg) || self.in_declaration_head(f) {
            return;
        }
        let mut commas = Vec::with_capacity(before.len());
        for &s in before {
            if !matches!(ast[s].kind, NodeKind::ExprStmt(_)) || ast[s].has(flags::DIRECTIVE) {
                return;
            }
            // A line break that stood for the `;` has no token to become a comma.
            match self.semi_of(s) {
                Some(semi) if self.actions[semi] == Action::Keep => commas.push(semi),
                _ => return,
            }
        }
        let open = self.first_tok(ast[body].span.start);
        let close = self.last_tok(ast[body].span.end);
        let ret = self.first_tok(ast[last].span.start);
        if !self.is(open, Punct::LBrace) || !self.is(close, Punct::RBrace) || !self.kw(ret, Kw::Return) {
            return;
        }
        self.set_text(open, "(");
        self.set_text(close, ")");
        for semi in commas {
            self.set_text(semi, ",");
        }
        self.set(ret, Action::Skip);
        if close >= 1 && self.is(close - 1, Punct::Semi) && close - 1 > ret {
            self.set(close - 1, Action::Skip);
        }
    }

    /// `(e) => e` is `e => e`.
    fn arrow_parameter(&mut self, _f: NodeId, params: crate::ast::List) {
        let ast = self.ast;
        let [param] = ast.list(params) else { return };
        if !matches!(ast[*param].kind, NodeKind::BindingIdent(_)) {
            return;
        }
        let name = self.first_tok(ast[*param].span.start);
        if name == 0 || !self.is(name - 1, Punct::LParen) || !self.is(name + 1, Punct::RParen) || !self.is(name + 2, Punct::Arrow) {
            return;
        }
        self.set(name - 1, Action::Skip);
        self.set(name + 1, Action::Skip);
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
    /// `undefined` or `void 0`: what `return_nothing` leaves out.
    fn is_nothing(&self, arg: NodeId) -> bool {
        match self.ast[arg].kind {
            NodeKind::Ident(name) => self.ast.atom(name) == "undefined" && self.is_global_undefined(arg),
            NodeKind::Unary { op: UnaryOp::Void, arg: inner } => matches!(self.ast[inner].kind, NodeKind::Num(v) if v.get() == 0.0),
            _ => false,
        }
    }

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


/// A string literal (quotes included) as a template literal, when that is shorter: a line break or a tab is one byte there where the string
/// needs two, and a quote needs none. `None` when the string has an escape a template does not allow (an octal one), a carriage return (a
/// template would read it as a line feed), a `${` or a backtick that would need an escape of its own more than it saves, or when the result is
/// not shorter.
pub(crate) fn as_template(token: &str) -> Option<String> {
    let body = token.get(1..token.len().checked_sub(1)?)?;
    if !body.contains('\\') || body.contains('\r') {
        return None;
    }
    let mut out = String::with_capacity(token.len());
    out.push('`');
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                q @ ('"' | '\'') => out.push(q),
                // `\0` followed by a digit, and `\1` to `\9`, are octal escapes (or worse) that a template refuses.
                '0' if !chars.peek().is_some_and(char::is_ascii_digit) => out.push_str("\\0"),
                '1'..='9' | '0' => return None,
                // A line continuation: a template reads it the same way, unless a carriage return is involved (ruled out above).
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            '`' => out.push_str("\\`"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            other => out.push(other),
        }
    }
    out.push('`');
    (out.len() < token.len()).then_some(out)
}
