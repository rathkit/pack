use super::pattern::PatternMode;
use super::*;
use crate::semantic::{ScopeKind, SymbolKind};
use crate::ast::{Atom, AssignOp, BinOp, MethodKind, Num, PropKind, UnaryOp, UpdateOp};

/// The contents of a parenthesized list, read before we know whether it is an arrow
/// function's parameters, a call's arguments, or a plain parenthesized expression.
pub(super) struct ParenItems {
    pub list: List,
    pub has_rest: bool,
    pub trailing_comma: bool,
    /// Where the first item's first token is, parentheses of its own included.
    pub first_start: u32,
    /// Where the last item ends, parentheses of its own included.
    pub last_end: u32,
    /// A `{ a = 1 }` inside that is only legal if this turns out to be arrow parameters.
    pub cover_error: Option<CoverError>,
    /// How many scopes and references existed before the `(`. If an arrow follows, whatever
    /// the parameters created after these marks belongs inside the arrow's scope.
    pub scope_mark: usize,
    pub ref_mark: usize,
}

impl<'a> Parser<'a> {
    // ---- identifiers and reserved words -----------------------------------------------

    pub(super) fn reserved_reason(&self, name: Atom) -> Option<&'static str> {
        // The words that can be reserved were interned first, so most identifiers fail this
        // single comparison and never reach the match.
        if name.index() > RESERVED_CANDIDATES {
            return None;
        }
        if name == known::YIELD {
            (self.strict || self.cx.is_generator).then_some("`yield` is a reserved word here")
        } else if name == known::AWAIT {
            (self.module || self.cx.is_async || self.cx.static_block).then_some("`await` is a reserved word here")
        } else if name.index() <= known::PUBLIC.index() {
            self.strict.then_some("reserved word in strict mode")
        } else {
            None
        }
    }

    /// The atom for an identifier used as a reference, rejecting reserved words.
    pub(super) fn check_ident_ref(&mut self, tok: Token) -> PResult<Atom> {
        let atom = self.name_of(tok);
        if let Some(message) = self.reserved_reason(atom) {
            return Err(self.error_at(message, tok.start));
        }
        if self.cx.class_init && atom == known::ARGUMENTS {
            let message = "`arguments` is not allowed in a class field initializer or static block";
            return Err(self.error_at(message, tok.start));
        }
        Ok(atom)
    }

    pub(super) fn is_arrow(&self, id: NodeId) -> bool {
        let node = &self.ast[id];
        matches!(node.kind, NodeKind::Function { .. }) && node.has(flags::ARROW) && !node.parenthesized()
    }

    /// `a.#x` or `a?.b.#x`, looking through the optional-chain wrapper.
    fn is_private_member(&self, mut id: NodeId) -> bool {
        loop {
            match self.ast[id].kind {
                NodeKind::Chain(inner) => id = inner,
                NodeKind::Member { prop, .. } => return matches!(self.ast[prop].kind, NodeKind::PrivateName(_)),
                _ => return false,
            }
        }
    }

    /// An arrow function is a whole assignment expression; it cannot be an operand.
    fn reject_arrow(&self, id: NodeId) -> PResult<()> {
        if self.is_arrow(id) {
            return Err(self.error_at("Malformed arrow function parameter list", self.ast[id].span.start));
        }
        Ok(())
    }

    fn await_is_keyword(&self) -> bool {
        self.cx.is_async || (self.module && !self.cx.in_function)
    }

    // ---- comma, assignment, conditional -------------------------------------------------

    pub(super) fn parse_expression(&mut self) -> PResult<NodeId> {
        // The first item may start with a parenthesis of its own, which the sequence includes.
        let start = self.tok.start;
        let first = self.parse_assign()?;
        if !self.at(Punct::Comma) {
            return Ok(first);
        }
        let mark = self.mark();
        self.push(first);
        while self.eat(Punct::Comma)? {
            let item = self.parse_assign()?;
            self.push(item);
        }
        let list = self.take_list(mark);
        Ok(self.finish(NodeKind::Sequence(list), start))
    }

    /// An assignment expression that must be a finished value: a leftover `{ a = 1 }`
    /// shorthand default is an error here.
    pub(super) fn parse_assign(&mut self) -> PResult<NodeId> {
        let outer = self.cover_init.take();
        let id = self.parse_assign_cover()?;
        if let Some((pos, message)) = self.cover_init.take() {
            return Err(self.error_at(message, pos));
        }
        self.cover_init = outer;
        Ok(id)
    }

    /// An assignment expression that may still become a pattern or parameter list.
    /// Pending shorthand defaults are left in `cover_init` for the caller to resolve.
    pub(super) fn parse_assign_cover(&mut self) -> PResult<NodeId> {
        if self.cx.is_generator && self.at_word("yield") {
            return self.parse_yield();
        }
        let start = self.tok.start;
        let outer = self.cover_init.take();
        let left = self.parse_conditional()?;
        let Some(op) = self.assign_op() else {
            self.cover_init = outer.or(self.cover_init);
            return Ok(left);
        };
        if self.is_arrow(left) {
            return Err(self.unexpected());
        }
        let target = if op == AssignOp::Assign {
            if let Some((pos, message)) = self.cover_init {
                if self.is_literal_pattern(left) {
                    self.cover_init = None;
                } else {
                    return Err(self.error_at(message, pos));
                }
            }
            self.to_assign_target(left)?;
            left
        } else {
            if let Some((pos, message)) = self.cover_init {
                return Err(self.error_at(message, pos));
            }
            // Logical assignment is stricter: a call is never a valid target for it.
            let logical = matches!(op, AssignOp::Op(BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish));
            if logical && matches!(self.ast[left].kind, NodeKind::Call { .. }) {
                return Err(self.error_at("Invalid left-hand side in assignment", self.ast[left].span.start));
            }
            self.check_simple_target(left)?;
            left
        };
        self.next()?;
        let value = self.parse_assign()?;
        self.cover_init = outer;
        Ok(self.finish(NodeKind::Assign { op, target, value }, start))
    }

    fn assign_op(&self) -> Option<AssignOp> {
        use BinOp as B;
        let Kind::Punct(p) = self.tok.kind else { return None };
        Some(match p {
            Punct::Eq => AssignOp::Assign,
            Punct::PlusEq => AssignOp::Op(B::Add),
            Punct::MinusEq => AssignOp::Op(B::Sub),
            Punct::StarEq => AssignOp::Op(B::Mul),
            Punct::SlashEq => AssignOp::Op(B::Div),
            Punct::PercentEq => AssignOp::Op(B::Rem),
            Punct::StarStarEq => AssignOp::Op(B::Exp),
            Punct::LtLtEq => AssignOp::Op(B::Shl),
            Punct::GtGtEq => AssignOp::Op(B::Shr),
            Punct::GtGtGtEq => AssignOp::Op(B::UShr),
            Punct::AmpEq => AssignOp::Op(B::BitAnd),
            Punct::PipeEq => AssignOp::Op(B::BitOr),
            Punct::CaretEq => AssignOp::Op(B::BitXor),
            Punct::AmpAmpEq => AssignOp::Op(B::LogAnd),
            Punct::PipePipeEq => AssignOp::Op(B::LogOr),
            Punct::QuestionQuestionEq => AssignOp::Op(B::Nullish),
            _ => return None,
        })
    }

    fn is_literal_pattern(&self, id: NodeId) -> bool {
        let node = &self.ast[id];
        matches!(node.kind, NodeKind::Array(_) | NodeKind::Object(_)) && !node.parenthesized()
    }

    /// Target of `+=`, `++` and friends: a plain reference or member access. Like every
    /// major engine, a call is accepted here and fails at run time instead (the
    /// specification calls it an early error, but real code relies on the web-compat rule).
    pub(super) fn check_simple_target(&mut self, id: NodeId) -> PResult<()> {
        let node = self.ast[id];
        match node.kind {
            NodeKind::Ident(name) => {
                self.check_assign_name(name, node.span.start)?;
                self.mark_assigned(id, true);
                Ok(())
            }
            NodeKind::Member { .. } | NodeKind::Call { .. } => Ok(()),
            _ => Err(self.error_at("Invalid left-hand side in assignment", node.span.start)),
        }
    }

    fn parse_yield(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        if self.cx.in_params {
            return Err(self.error("`yield` expressions are not allowed in formal parameters"));
        }
        self.next()?;
        let ends = self.tok.newline_before
            || matches!(
                self.tok.kind,
                Kind::Eof
                    | Kind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::Comma | Punct::Colon | Punct::Semi)
            )
            || self.at_kw(Kw::In)
            || self.at_word("of");
        if ends {
            return Ok(self.finish(NodeKind::Yield(None), start));
        }
        let delegate = self.eat(Punct::Star)?;
        let arg = self.parse_assign()?;
        let flag = if delegate { flags::DELEGATE } else { 0 };
        Ok(self.finish_with(NodeKind::Yield(Some(arg)), start, flag))
    }

    fn parse_conditional(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let test = self.parse_binary(0)?;
        if self.is_arrow(test) || !self.at(Punct::Question) {
            return Ok(test);
        }
        self.next()?;
        let cons = self.allow_in(|p| {
            p.in_consequent = true;
            p.parse_assign()
        })?;
        self.expect(Punct::Colon)?;
        let alt = self.parse_assign()?;
        Ok(self.finish(NodeKind::Conditional { test, cons, alt }, start))
    }

    // ---- binary and unary ---------------------------------------------------------------

    pub(super) fn binary_op(&self) -> Option<(BinOp, u8)> {
        use BinOp::*;
        Some(match self.tok.kind {
            Kind::Punct(p) => match p {
                Punct::QuestionQuestion => (Nullish, 1),
                Punct::PipePipe => (LogOr, 2),
                Punct::AmpAmp => (LogAnd, 3),
                Punct::Pipe => (BitOr, 4),
                Punct::Caret => (BitXor, 5),
                Punct::Amp => (BitAnd, 6),
                Punct::EqEq => (Eq, 7),
                Punct::NotEq => (Ne, 7),
                Punct::EqEqEq => (StrictEq, 7),
                Punct::NotEqEq => (StrictNe, 7),
                Punct::Lt => (Lt, 8),
                Punct::LtEq => (Le, 8),
                Punct::Gt => (Gt, 8),
                Punct::GtEq => (Ge, 8),
                Punct::LtLt => (Shl, 9),
                Punct::GtGt => (Shr, 9),
                Punct::GtGtGt => (UShr, 9),
                Punct::Plus => (Add, 10),
                Punct::Minus => (Sub, 10),
                Punct::Star => (Mul, 11),
                Punct::Slash => (Div, 11),
                Punct::Percent => (Rem, 11),
                Punct::StarStar => (Exp, 12),
                _ => return None,
            },
            Kind::Keyword(Kw::In) if !self.no_in && !self.tok.escaped => (In, 8),
            Kind::Keyword(Kw::Instanceof) if !self.tok.escaped => (InstanceOf, 8),
            _ => return None,
        })
    }

    fn is_unparenthesized(&self, id: NodeId, f: impl Fn(&NodeKind) -> bool) -> bool {
        let node = &self.ast[id];
        !node.parenthesized() && f(&node.kind)
    }

    fn parse_binary(&mut self, min_prec: u8) -> PResult<NodeId> {
        let start = self.tok.start;
        let mut left = if self.tok.kind == Kind::PrivateName {
            // `#x in obj` is the one place a private name stands alone.
            let name = self.private_name_atom(self.tok);
            self.note_private_reference(name, start)?;
            self.next()?;
            if !(self.at_kw(Kw::In) && !self.no_in && min_prec <= 8) {
                return Err(self.unexpected());
            }
            self.finish(NodeKind::PrivateName(name), start)
        } else {
            self.parse_unary()?
        };
        if self.is_arrow(left) {
            return Ok(left);
        }
        loop {
            if self.ts && min_prec <= 8 && self.tok.kind == Kind::Ident && !self.tok.newline_before && !self.tok.escaped {
                let word = self.text(self.tok);
                if word == "as" || word == "satisfies" {
                    self.ts_cast()?;
                    continue;
                }
            }
            let Some((op, prec)) = self.binary_op() else { break };
            if prec < min_prec {
                break;
            }
            let op_pos = self.tok.start;
            if op == BinOp::Exp && self.is_unparenthesized(left, |k| matches!(k, NodeKind::Unary { .. } | NodeKind::Await(_))) {
                return Err(self.error_at("A unary operator cannot be the left operand of `**`; add parentheses", op_pos));
            }
            self.next()?;
            let right = self.parse_binary(if op == BinOp::Exp { prec } else { prec + 1 })?;
            self.reject_arrow(right)?;
            let mixes = |k: &NodeKind, banned: &[BinOp]| matches!(k, NodeKind::Binary { op, .. } if banned.contains(op));
            let bad_mix = match op {
                BinOp::Nullish => {
                    self.is_unparenthesized(left, |k| mixes(k, &[BinOp::LogAnd, BinOp::LogOr]))
                        || self.is_unparenthesized(right, |k| mixes(k, &[BinOp::LogAnd, BinOp::LogOr]))
                }
                BinOp::LogAnd | BinOp::LogOr => {
                    self.is_unparenthesized(left, |k| mixes(k, &[BinOp::Nullish]))
                        || self.is_unparenthesized(right, |k| mixes(k, &[BinOp::Nullish]))
                }
                _ => false,
            };
            if bad_mix {
                return Err(self.error_at("`??` cannot be mixed with `||` or `&&` without parentheses", op_pos));
            }
            left = self.finish(NodeKind::Binary { op, left, right }, start);
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let op = match self.tok.kind {
            Kind::Punct(Punct::Bang) => Some(UnaryOp::Not),
            Kind::Punct(Punct::Tilde) => Some(UnaryOp::BitNot),
            Kind::Punct(Punct::Plus) => Some(UnaryOp::Pos),
            Kind::Punct(Punct::Minus) => Some(UnaryOp::Neg),
            Kind::Keyword(Kw::Typeof) if !self.tok.escaped => Some(UnaryOp::Typeof),
            Kind::Keyword(Kw::Void) if !self.tok.escaped => Some(UnaryOp::Void),
            Kind::Keyword(Kw::Delete) if !self.tok.escaped => Some(UnaryOp::Delete),
            _ => None,
        };
        if let Some(op) = op {
            self.next()?;
            let arg = self.parse_unary()?;
            self.reject_arrow(arg)?;
            if op == UnaryOp::Delete && self.is_private_member(arg) {
                return Err(self.error_at("Private fields cannot be deleted", start));
            }
            if op == UnaryOp::Delete && self.strict && matches!(self.ast[arg].kind, NodeKind::Ident(_)) {
                return Err(self.error_at("Deleting an unqualified identifier is not allowed in strict mode", start));
            }
            return Ok(self.finish(NodeKind::Unary { op, arg }, start));
        }
        if self.ts && !self.jsx && self.at(Punct::Lt) {
            if let Some(arrow) = self.ts_angle_prefix()? {
                return Ok(arrow);
            }
            return self.parse_unary();
        }
        if let Kind::Punct(p @ (Punct::PlusPlus | Punct::MinusMinus)) = self.tok.kind {
            self.next()?;
            let arg = self.parse_unary()?;
            self.check_simple_target(arg)?;
            let op = if p == Punct::PlusPlus { UpdateOp::Inc } else { UpdateOp::Dec };
            return Ok(self.finish_with(NodeKind::Update { op, arg }, start, flags::PREFIX));
        }
        if self.at_word("await") && self.await_is_keyword() {
            if self.cx.in_params {
                return Err(self.error("`await` expressions are not allowed in formal parameters"));
            }
            self.next()?;
            let arg = self.parse_unary()?;
            self.reject_arrow(arg)?;
            return Ok(self.finish(NodeKind::Await(arg), start));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let expr = self.parse_lhs()?;
        if self.is_arrow(expr) {
            return Ok(expr);
        }
        if let Kind::Punct(p @ (Punct::PlusPlus | Punct::MinusMinus)) = self.tok.kind {
            if !self.tok.newline_before {
                self.check_simple_target(expr)?;
                self.next()?;
                let op = if p == Punct::PlusPlus { UpdateOp::Inc } else { UpdateOp::Dec };
                return Ok(self.finish(NodeKind::Update { op, arg: expr }, start));
            }
        }
        Ok(expr)
    }

    // ---- member access, calls, new ------------------------------------------------------

    pub(super) fn parse_lhs(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let base = if self.at_kw(Kw::New) { self.parse_new()? } else { self.parse_primary()? };
        self.parse_subscripts(base, start, true)
    }

    fn parse_new(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        if self.eat(Punct::Dot)? {
            self.expect_word("target")?;
            if !self.cx.new_target {
                return Err(self.error_at("`new.target` is not allowed here", start));
            }
            return Ok(self.finish(NodeKind::NewTarget, start));
        }
        let callee_start = self.tok.start;
        let base = if self.at_kw(Kw::New) {
            self.parse_new()?
        } else if self.at_kw(Kw::Import) {
            return Err(self.error("Cannot use `new` with `import`"));
        } else {
            self.parse_primary()?
        };
        // Member accesses bind tighter than `new`'s argument list; calls do not.
        let callee = self.parse_subscripts(base, callee_start, false)?;
        self.reject_arrow(callee)?;
        if self.at(Punct::QuestionDot) {
            return Err(self.error("An optional chain cannot be the callee of `new`"));
        }
        let args = if self.at(Punct::LParen) { self.parse_arguments()? } else { List::default() };
        Ok(self.finish(NodeKind::New { callee, args }, start))
    }

    /// A decorator's expression: a primary with members and calls, but no `[index]`.
    pub(super) fn parse_decorator_expression(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let base = if self.at_kw(Kw::New) { self.parse_new()? } else { self.parse_primary()? };
        self.decorator_subscripts = true;
        self.parse_subscripts(base, start, true)
    }

    fn parse_subscripts(&mut self, mut expr: NodeId, start: u32, allow_call: bool) -> PResult<NodeId> {
        let no_index = std::mem::take(&mut self.decorator_subscripts);
        let mut chained = false;
        loop {
            if self.is_arrow(expr) {
                break;
            }
            match self.tok.kind {
                Kind::Punct(Punct::Dot) => {
                    self.next()?;
                    let prop = self.parse_member_name()?;
                    expr = self.finish(NodeKind::Member { object: expr, prop }, start);
                }
                Kind::Punct(Punct::QuestionDot) if allow_call => {
                    chained = true;
                    self.next()?;
                    expr = match self.tok.kind {
                        Kind::Punct(Punct::LParen) => {
                            let args = self.parse_arguments()?;
                            self.note_call(expr);
                            self.finish_with(NodeKind::Call { callee: expr, args }, start, flags::OPTIONAL)
                        }
                        Kind::Punct(Punct::LBracket) => self.parse_computed_member(expr, start, flags::OPTIONAL)?,
                        Kind::NoSubTemplate | Kind::TemplateHead => {
                            return Err(self.error("A tagged template cannot be used in an optional chain"));
                        }
                        _ => {
                            let prop = self.parse_member_name()?;
                            self.finish_with(NodeKind::Member { object: expr, prop }, start, flags::OPTIONAL)
                        }
                    };
                }
                Kind::Punct(Punct::LBracket) if !no_index => expr = self.parse_computed_member(expr, start, 0)?,
                Kind::Punct(Punct::Bang) if self.ts && !self.tok.newline_before => {
                    // `x!`: a non-null assertion, gone from the output.
                    let bang = self.tok.start;
                    self.next()?;
                    self.ts_erase_from(bang);
                }
                Kind::Punct(Punct::Lt) if self.ts => {
                    if !self.ts_try_type_args_expr() {
                        break;
                    }
                }
                Kind::Punct(Punct::LParen) if allow_call => {
                    let args = self.parse_arguments()?;
                    self.note_call(expr);
                    expr = self.finish(NodeKind::Call { callee: expr, args }, start);
                }
                Kind::NoSubTemplate | Kind::TemplateHead => {
                    if chained {
                        return Err(self.error("A tagged template cannot be used in an optional chain"));
                    }
                    let quasi = self.parse_template(true)?;
                    expr = self.finish(NodeKind::TaggedTemplate { tag: expr, quasi }, start);
                }
                _ => break,
            }
        }
        if chained {
            expr = self.finish(NodeKind::Chain(expr), start);
        }
        Ok(expr)
    }

    /// A call to `eval` by that name can see (and change) every variable around it.
    fn note_call(&mut self, callee: NodeId) {
        if matches!(self.ast[callee].kind, NodeKind::Ident(n) if n == known::EVAL) {
            self.sb.mark_direct_eval();
        }
    }

    fn parse_computed_member(&mut self, object: NodeId, start: u32, extra: u8) -> PResult<NodeId> {
        self.next()?;
        let prop = self.allow_in(|p| p.parse_expression())?;
        self.expect(Punct::RBracket)?;
        Ok(self.finish_with(NodeKind::Member { object, prop }, start, flags::COMPUTED | extra))
    }

    fn parse_member_name(&mut self) -> PResult<NodeId> {
        let tok = self.tok;
        match tok.kind {
            Kind::PrivateName => {
                let name = self.private_name_atom(tok);
                self.note_private_reference(name, tok.start)?;
                self.next()?;
                Ok(self.finish(NodeKind::PrivateName(name), tok.start))
            }
            Kind::Ident | Kind::Keyword(_) => {
                let name = self.name_of(tok);
                self.next()?;
                Ok(self.finish(NodeKind::PropName(name), tok.start))
            }
            _ => Err(self.unexpected()),
        }
    }

    fn note_private_reference(&mut self, name: Atom, pos: u32) -> PResult<()> {
        match self.private_scopes.last_mut() {
            Some(scope) => {
                scope.referenced.push((name, pos));
                Ok(())
            }
            None => Err(self.undeclared_private(name, pos)),
        }
    }

    fn undeclared_private(&self, name: Atom, pos: u32) -> ParseError {
        self.error_at(format!("Private field `#{}` must be declared in an enclosing class", self.ast.atom(name)), pos)
    }

    /// A name may be declared twice only as a getter and a setter with the same staticness.
    fn declare_private(&mut self, name: Atom, how: u8, pos: u32) -> PResult<()> {
        let scope = self.private_scopes.last_mut().expect("class members are parsed inside a private scope");
        if let Some(entry) = scope.declared.iter_mut().find(|(n, _)| *n == name) {
            let (old, new) = (entry.1, how);
            let same_static = old & PRIVATE_STATIC == new & PRIVATE_STATIC;
            // Only a lone getter followed by a lone setter (or the reverse) may share a name.
            let (old_accessor, new_accessor) = (old & (PRIVATE_GETTER | PRIVATE_SETTER), new & (PRIVATE_GETTER | PRIVATE_SETTER));
            let accessor_pair = old_accessor != 0
                && new_accessor != 0
                && old_accessor & new_accessor == 0
                && old_accessor != PRIVATE_GETTER | PRIVATE_SETTER
                && old & PRIVATE_OTHER == 0;
            if !(same_static && accessor_pair) {
                return Err(self.error_at(format!("Private name `#{}` is already declared", self.ast.atom(name)), pos));
            }
            entry.1 |= new;
            return Ok(());
        }
        scope.declared.push((name, how));
        Ok(())
    }

    /// At the end of a class body: references that matched nothing here may still match a
    /// class that encloses this one.
    fn close_private_scope(&mut self) -> PResult<()> {
        let scope = self.private_scopes.pop().expect("a class body pushed a private scope");
        for (name, pos) in scope.referenced {
            if scope.declared.iter().any(|(n, _)| *n == name) {
                continue;
            }
            match self.private_scopes.last_mut() {
                Some(outer) => outer.referenced.push((name, pos)),
                None => return Err(self.undeclared_private(name, pos)),
            }
        }
        Ok(())
    }

    /// `#name` without the `#`.
    pub(super) fn private_name_atom(&mut self, tok: Token) -> Atom {
        let text = &self.src[tok.start as usize + 1..tok.end as usize];
        if tok.escaped {
            let decoded = literal::decode_identifier(text);
            self.ast.intern(&decoded)
        } else {
            self.ast.intern(text)
        }
    }

    pub(super) fn parse_arguments(&mut self) -> PResult<List> {
        self.expect(Punct::LParen)?;
        self.allow_in(|p| {
            let mark = p.mark();
            while !p.at(Punct::RParen) {
                let start = p.tok.start;
                let arg = if p.eat(Punct::Ellipsis)? {
                    let inner = p.parse_assign()?;
                    p.finish(NodeKind::Spread(inner), start)
                } else {
                    p.parse_assign()?
                };
                p.push(arg);
                if !p.eat(Punct::Comma)? {
                    break;
                }
            }
            p.expect(Punct::RParen)?;
            Ok(p.take_list(mark))
        })
    }

    // ---- primary expressions ------------------------------------------------------------

    fn parse_primary(&mut self) -> PResult<NodeId> {
        let tok = self.tok;
        let start = tok.start;
        if self.ts && tok.kind == Kind::Punct(Punct::At) {
            // A decorated class expression: `@dec class {}`.
            self.pending_decorators = self.parse_decorators()?;
            if !self.at_kw(Kw::Class) {
                return Err(self.unexpected());
            }
            return self.parse_class(start, false, false);
        }
        match tok.kind {
            Kind::Ident => self.parse_ident_primary(),
            Kind::Keyword(kw) if !tok.escaped => match kw {
                Kw::This => {
                    self.next()?;
                    Ok(self.finish(NodeKind::This, start))
                }
                Kw::Null => {
                    self.next()?;
                    Ok(self.finish(NodeKind::Null, start))
                }
                Kw::True | Kw::False => {
                    self.next()?;
                    Ok(self.finish(NodeKind::Bool(kw == Kw::True), start))
                }
                Kw::Function => self.parse_function(start, false, false, false),
                Kw::Class => self.parse_class(start, false, false),
                Kw::Super => {
                    self.next()?;
                    let allowed = match self.tok.kind {
                        Kind::Punct(Punct::LParen) => self.cx.super_call,
                        Kind::Punct(Punct::Dot | Punct::LBracket) => self.cx.super_prop,
                        _ => false,
                    };
                    if !allowed {
                        return Err(self.error_at("'super' keyword unexpected here", start));
                    }
                    Ok(self.finish(NodeKind::Super, start))
                }
                Kw::Import => self.parse_import_expr(),
                Kw::New => self.parse_new(),
                _ => Err(self.unexpected()),
            },
            Kind::Number => {
                let text = self.text(tok);
                self.check_legacy_octal(text, start)?;
                self.next()?;
                Ok(self.finish(NodeKind::Num(Num::new(literal::parse_number(text))), start))
            }
            Kind::BigInt => {
                let digits = literal::bigint_digits(self.text(tok));
                let atom = self.atom(&digits);
                self.next()?;
                Ok(self.finish(NodeKind::BigInt(atom), start))
            }
            Kind::String => {
                let atom = self.string_atom(tok)?;
                self.next()?;
                Ok(self.finish(NodeKind::Str(atom), start))
            }
            Kind::Punct(Punct::Slash | Punct::SlashEq) => {
                let regex = self.lexer.rescan_regex(tok)?;
                self.validate_regex_flags(regex)?;
                self.tok = regex;
                self.next()?;
                Ok(self.finish(NodeKind::Regex, start))
            }
            Kind::NoSubTemplate | Kind::TemplateHead => self.parse_template(false),
            Kind::Punct(Punct::LParen) => self.parse_paren(),
            Kind::Punct(Punct::LBracket) => self.parse_array(),
            Kind::Punct(Punct::LBrace) => self.parse_object(),
            Kind::Punct(Punct::Lt) if self.jsx => {
                if self.jsx_generic_arrow_ahead() {
                    return self.parse_ts_arrow(start, false);
                }
                self.parse_jsx_element(false)
            }
            _ => Err(self.unexpected()),
        }
    }

    /// `017` and `089` are only legal in sloppy code.
    fn check_legacy_octal(&self, text: &str, pos: u32) -> PResult<()> {
        if self.strict && (literal::is_legacy_octal(text) || literal::is_leading_zero_decimal(text)) {
            return Err(self.error_at("Legacy octal literals are not allowed in strict mode", pos));
        }
        Ok(())
    }

    pub(super) fn string_atom(&mut self, tok: Token) -> PResult<Atom> {
        let text = self.text(tok);
        let inner = &text[1..text.len() - 1];
        if !inner.contains('\\') {
            return Ok(self.ast.intern(inner));
        }
        match literal::cook_string(inner, self.strict) {
            Ok(value) => Ok(self.ast.intern(&value)),
            Err(message) => Err(self.error_at(message, tok.start)),
        }
    }

    fn validate_regex_flags(&self, tok: Token) -> PResult<()> {
        let text = self.text(tok);
        let flags = &text[text.rfind('/').map_or(0, |i| i + 1)..];
        let mut seen = String::new();
        for c in flags.chars() {
            if !"dgimsuvy".contains(c) || seen.contains(c) {
                return Err(self.error_at("Invalid regular expression flags", tok.start));
            }
            seen.push(c);
        }
        if seen.contains('u') && seen.contains('v') {
            return Err(self.error_at("The `u` and `v` regular expression flags cannot be combined", tok.start));
        }
        Ok(())
    }

    fn parse_ident_primary(&mut self) -> PResult<NodeId> {
        let tok = self.tok;
        let start = tok.start;
        if !tok.escaped && self.text(tok) == "async" {
            if let Some(node) = self.try_parse_async(start)? {
                return Ok(node);
            }
        }
        let name = self.check_ident_ref(tok)?;
        self.next()?;
        let id = self.finish(NodeKind::Ident(name), start);
        if self.at(Punct::Arrow) && !self.tok.newline_before {
            // `x => ...`: the identifier was a parameter all along.
            self.check_binding_name(name, start)?;
            self.ast.node_mut(id).kind = NodeKind::BindingIdent(name);
            self.push_arrow_scope(self.sb.scope_count(), self.sb.ref_count());
            self.sb.declare_param(name, id);
            let params = self.ast.alloc_list(&[id]);
            return self.parse_arrow_body(start, params, false);
        }
        self.record_ref(id);
        Ok(id)
    }

    /// Cursor is on `async`. Handles `async function`, `async x => ...`, `async (...) => ...`
    /// and the call `async(...)`. Returns `None` when `async` is just an identifier.
    fn try_parse_async(&mut self, start: u32) -> PResult<Option<NodeId>> {
        let next = self.peek()?;
        if next.newline_before {
            return Ok(None);
        }
        match next.kind {
            Kind::Keyword(Kw::Function) if !next.escaped => {
                self.next()?;
                self.parse_function(start, true, false, false).map(Some)
            }
            Kind::Ident => {
                self.next()?;
                let ptok = self.tok;
                let name = self.name_of(ptok);
                self.check_binding_name(name, ptok.start)?;
                self.next()?;
                if !self.at(Punct::Arrow) || self.tok.newline_before {
                    return Err(self.unexpected_expecting("=>"));
                }
                let param = self.finish(NodeKind::BindingIdent(name), ptok.start);
                self.push_arrow_scope(self.sb.scope_count(), self.sb.ref_count());
                self.sb.declare_param(name, param);
                let params = self.ast.alloc_list(&[param]);
                self.parse_arrow_body(start, params, true).map(Some)
            }
            Kind::Punct(Punct::Lt) if self.ts => {
                // `async <T>(x: T) => x`, or the comparison `async < b`.
                let mark = self.ts_mark();
                self.next()?;
                if self.ts_arrow_ahead(true) {
                    return self.parse_ts_arrow(start, true).map(Some);
                }
                self.ts_reset(mark);
                Ok(None)
            }
            Kind::Punct(Punct::LParen) => {
                let async_name = known::ASYNC;
                let callee = self.ast.alloc(NodeKind::Ident(async_name), Span { start, end: start + 5 }, 0);
                self.next()?;
                if self.ts && self.ts_arrow_ahead(false) {
                    return self.parse_ts_arrow(start, true).map(Some);
                }
                let items = self.parse_paren_items()?;
                if self.at(Punct::Arrow) && !self.tok.newline_before {
                    if items.has_rest && items.trailing_comma {
                        return Err(self.error_at("A rest parameter must be last", start));
                    }
                    return self.parse_arrow_from_items(start, &items, true).map(Some);
                }
                if let Some((pos, message)) = items.cover_error {
                    return Err(self.error_at(message, pos));
                }
                // It was a call, so `async` is an ordinary reference. It comes before its
                // arguments in the source, so it goes before theirs in the list.
                self.sb.insert_ref(items.ref_mark, callee);
                Ok(Some(self.finish(NodeKind::Call { callee, args: items.list }, start)))
            }
            _ => Ok(None),
        }
    }

    // ---- parentheses and arrows ---------------------------------------------------------

    /// Cursor on `(`; consumes through the matching `)`.
    pub(super) fn parse_paren_items(&mut self) -> PResult<ParenItems> {
        let (scope_mark, ref_mark) = (self.sb.scope_count(), self.sb.ref_count());
        self.expect(Punct::LParen)?;
        let outer = self.cover_init.take();
        let saved_no_in = std::mem::replace(&mut self.no_in, false);
        let mark = self.mark();
        let (mut has_rest, mut trailing_comma) = (false, false);
        let first_start = self.tok.start;
        while !self.at(Punct::RParen) {
            let start = self.tok.start;
            let item = if self.eat(Punct::Ellipsis)? {
                has_rest = true;
                let arg = self.parse_assign_cover()?;
                self.finish(NodeKind::Spread(arg), start)
            } else {
                self.parse_assign_cover()?
            };
            self.push(item);
            if self.eat(Punct::Comma)? {
                if self.at(Punct::RParen) {
                    trailing_comma = true;
                }
            } else {
                break;
            }
        }
        let last_end = self.prev_end;
        self.expect(Punct::RParen)?;
        self.no_in = saved_no_in;
        let cover_error = self.cover_init.take();
        self.cover_init = outer;
        let list = self.take_list(mark);
        Ok(ParenItems { list, has_rest, trailing_comma, first_start, last_end, cover_error, scope_mark, ref_mark })
    }

    fn parse_paren(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        if self.ts && self.ts_arrow_ahead(false) {
            return self.parse_ts_arrow(start, false);
        }
        let items = self.parse_paren_items()?;
        if self.at(Punct::Arrow) && !self.tok.newline_before {
            if items.has_rest && items.trailing_comma {
                return Err(self.error_at("A rest parameter must be last", start));
            }
            return self.parse_arrow_from_items(start, &items, false);
        }
        if items.list.is_empty() || items.has_rest || items.trailing_comma {
            return Err(self.error_at("Unexpected token in parenthesized expression", start));
        }
        if let Some((pos, message)) = items.cover_error {
            return Err(self.error_at(message, pos));
        }
        let ids = self.ast.list(items.list);
        let inner = if ids.len() == 1 {
            ids[0]
        } else {
            let span = Span { start: items.first_start, end: items.last_end };
            self.ast.alloc(NodeKind::Sequence(items.list), span, 0)
        };
        self.set_flag(inner, flags::PARENTHESIZED);
        Ok(inner)
    }

    /// `await` and `yield` expressions cannot appear in arrow parameters. The parameters were
    /// read as ordinary expressions before the `=>` was seen, so look for them afterwards.
    /// Nested functions have their own rules and are skipped.
    fn find_await_or_yield(&self, roots: &[NodeId]) -> Option<(u32, &'static str)> {
        let mut stack: Vec<NodeId> = roots.to_vec();
        while let Some(id) = stack.pop() {
            let node = &self.ast[id];
            match node.kind {
                NodeKind::Await(_) => return Some((node.span.start, "`await` expressions are not allowed in arrow function parameters")),
                NodeKind::Yield(_) => return Some((node.span.start, "`yield` expressions are not allowed in arrow function parameters")),
                NodeKind::Function { .. } => continue,
                _ => {}
            }
            self.ast.for_each_child(id, |child| stack.push(child));
        }
        None
    }

    /// Cursor on `=>`; the parameters were read as expressions and are rewritten here.
    fn parse_arrow_from_items(&mut self, start: u32, items: &ParenItems, is_async: bool) -> PResult<NodeId> {
        let ids: Vec<NodeId> = self.ast.list(items.list).to_vec();
        if let Some((pos, message)) = self.find_await_or_yield(&ids) {
            return Err(self.error_at(message, pos));
        }
        // Now we know this is an arrow, so open its scope; anything the parameter expressions
        // created meanwhile (function expressions, references) moves inside it.
        self.push_arrow_scope(items.scope_mark, items.ref_mark);
        let mark = self.mark();
        for (i, &id) in ids.iter().enumerate() {
            let param = match self.ast[id].kind {
                NodeKind::Spread(arg) => {
                    if i != ids.len() - 1 {
                        return Err(self.error_at("A rest parameter must be last", self.ast[id].span.start));
                    }
                    self.to_pattern(arg, PatternMode::Binding)?;
                    self.ast.node_mut(id).kind = NodeKind::Rest(arg);
                    id
                }
                _ => {
                    self.to_pattern_element(id, PatternMode::Binding)?;
                    id
                }
            };
            self.push(param);
        }
        let params = self.take_list(mark);
        self.parse_arrow_body(start, params, is_async)
    }

    /// Cursor on `=>`. The arrow's scope is the current one; this closes it.
    pub(super) fn parse_arrow_body(&mut self, start: u32, params: List, is_async: bool) -> PResult<NodeId> {
        self.next()?; // `=>`
        let simple = self.params_are_simple(params);
        // Arrows have no `super` or `new.target` of their own; they see the enclosing ones.
        let context = FnContext {
            in_function: true,
            is_async,
            super_prop: self.cx.super_prop,
            super_call: self.cx.super_call,
            new_target: self.cx.new_target,
            class_init: self.cx.class_init,
            static_block: self.cx.static_block,
            arrow: true,
            ..FnContext::default()
        };
        let saved = std::mem::replace(&mut self.cx, context);
        let result = if self.at(Punct::LBrace) {
            // Non-simple parameters keep the body in a scope of its own.
            if !simple {
                self.push_scope(ScopeKind::Body);
            }
            let body = self.parse_function_body();
            if let (false, Ok(body)) = (simple, &body) {
                self.sb.pop(*body);
            }
            body.map(|body| (body, 0))
        } else {
            // An expression body has no directive prologue; clear what nested bodies left.
            let body = self.parse_assign();
            self.last_body_strict = false;
            body.map(|body| (body, flags::EXPR_BODY))
        };
        self.cx = saved;
        let (body, body_flag) = result?;
        self.check_params(params, true)?;
        let async_flag = if is_async { flags::ASYNC } else { 0 };
        let node = self.finish_with(NodeKind::Function { name: None, params, body }, start, flags::ARROW | async_flag | body_flag);
        self.sb.pop(node);
        Ok(node)
    }

    fn params_are_simple(&self, params: List) -> bool {
        self.ast.list(params).iter().all(|&p| matches!(self.ast[p].kind, NodeKind::BindingIdent(_)))
    }

    // ---- functions ----------------------------------------------------------------------

    /// Cursor on `function`. `start` is where the node begins (before `async` if present).
    pub(super) fn parse_function(&mut self, start: u32, is_async: bool, decl: bool, name_required: bool) -> PResult<NodeId> {
        self.expect_kw(Kw::Function)?;
        let is_generator = self.eat(Punct::Star)?;
        let name = if self.tok.kind == Kind::Ident {
            // A function expression's name is bound inside the function, so `yield` and
            // `await` are judged by the function's own kind, not the surrounding one.
            let saved = (self.cx.is_async, self.cx.is_generator);
            if !decl {
                (self.cx.is_async, self.cx.is_generator) = (is_async, is_generator);
            }
            let name = self.parse_binding_ident_plain();
            (self.cx.is_async, self.cx.is_generator) = saved;
            let name = name?;
            if decl {
                self.declare_function_decl_name(name, is_async, is_generator)?;
            }
            Some(name)
        } else if name_required {
            return Err(self.unexpected());
        } else {
            None
        };
        let decl_flag = if decl { flags::DECL } else { 0 };
        self.parse_function_rest(start, name, is_async, is_generator, decl_flag, false, false)
    }

    /// Cursor on `(`: parameters and body.
    pub(super) fn parse_function_rest(
        &mut self,
        start: u32,
        name: Option<NodeId>,
        is_async: bool,
        is_generator: bool,
        extra: u8,
        super_prop: bool,
        super_call: bool,
    ) -> PResult<NodeId> {
        if self.ts && self.at(Punct::Lt) {
            self.ts_type_params_erase()?;
        }
        self.push_scope(ScopeKind::Function);
        if extra & flags::DECL == 0 {
            // A function expression's own name is visible only inside it.
            if let Some(n) = name {
                if let NodeKind::BindingIdent(atom) = self.ast[n].kind {
                    self.sb.declare_function_expr_name(atom, n);
                }
            }
        }
        let context =
            FnContext { in_function: true, is_async, is_generator, super_prop, super_call, new_target: true, ..FnContext::default() };
        let saved = std::mem::replace(&mut self.cx, context);
        let result = self.parse_params_and_body();
        self.cx = saved;
        let (params, body, props) = result?;
        if self.ts && std::mem::take(&mut self.ts_overload) {
            // A signature with no body (an overload or an abstract method): text only.
            self.eat(Punct::Semi)?;
            let node = self.ast.alloc(NodeKind::Empty, Span { start, end: self.prev_end }, 0);
            self.sb.pop(node);
            return Ok(node);
        }
        // Methods behave like strict-parameter functions: duplicates are always errors.
        self.check_params(params, super_prop)?;
        let mut node_flags = extra;
        if is_async {
            node_flags |= flags::ASYNC;
        }
        if is_generator {
            node_flags |= flags::GENERATOR;
        }
        let node = self.finish_with(NodeKind::Function { name, params, body }, start, node_flags);
        if !props.is_empty() {
            self.ast.add_param_props(node, props);
        }
        if let Some(meta) = self.return_meta.take() {
            self.ast.set_type_meta(node, meta);
        }
        self.sb.pop(node);
        Ok(node)
    }

    /// Parameters, then the body: in the same scope if the parameters are simple names, in a
    /// scope of its own if they have defaults or patterns (whose expressions must not see
    /// the body's declarations). Also returns which parameters were parameter properties.
    fn parse_params_and_body(&mut self) -> PResult<(List, NodeId, Vec<NodeId>)> {
        let params = self.parse_formal_params()?;
        let props = std::mem::take(&mut self.last_formal_props);
        if self.ts {
            if self.at(Punct::Colon) {
                if self.meta && self.in_member {
                    let colon = self.tok.start;
                    let mut meta = crate::ast::TypeMeta::Object;
                    self.ts_in_type(|p| {
                        p.next()?;
                        meta = p.ts_classify_type_public()?;
                        Ok(())
                    })?;
                    self.ts_erase_from(colon);
                    self.return_meta = Some(meta);
                } else {
                    self.ts_return_annotation()?;
                }
            }
            if !self.at(Punct::LBrace) {
                // No body follows: an overload signature or an abstract method.
                self.ts_overload = true;
                let empty = self.ast.alloc(NodeKind::Block(List::default()), Span { start: self.prev_end, end: self.prev_end }, 0);
                return Ok((params, empty, props));
            }
        }
        if self.params_are_simple(params) {
            let body = self.parse_function_body()?;
            return Ok((params, body, props));
        }
        self.push_scope(ScopeKind::Body);
        let body = self.parse_function_body()?;
        self.sb.pop(body);
        Ok((params, body, props))
    }

    pub(super) fn parse_formal_params(&mut self) -> PResult<List> {
        self.expect(Punct::LParen)?;
        self.cx.in_params = true;
        let ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::Param);
        let outer_props = std::mem::take(&mut self.param_props_acc);
        let result = self.parse_formal_params_inner();
        self.last_formal_props = std::mem::replace(&mut self.param_props_acc, outer_props);
        self.binding_ctx = ctx;
        self.cx.in_params = false;
        result
    }

    fn parse_formal_params_inner(&mut self) -> PResult<List> {
        self.allow_in(|p| {
            let mark = p.mark();
            while !p.at(Punct::RParen) {
                let start = p.tok.start;
                if p.ts && p.ts_param_prefix()? {
                    // A `this` parameter, comma and all, is gone.
                    continue;
                }
                if p.eat(Punct::Ellipsis)? {
                    let target = p.parse_binding_target()?;
                    if p.ts && p.at(Punct::Colon) {
                        if p.meta && p.in_member {
                            let meta = p.ts_annotation_meta_for(true)?;
                            p.ast.set_type_meta(target, meta);
                        } else {
                            p.ts_annotation()?;
                        }
                    }
                    let rest = p.finish(NodeKind::Rest(target), start);
                    p.push(rest);
                    if !p.at(Punct::RParen) {
                        return Err(p.error_at("A rest parameter must be last", start));
                    }
                    break;
                }
                let param = if p.ts { p.parse_ts_param()? } else { p.parse_binding_element()? };
                p.push(param);
                if !p.eat(Punct::Comma)? {
                    break;
                }
            }
            p.expect(Punct::RParen)?;
            Ok(p.take_list(mark))
        })
    }

    /// Checks that apply once a function's parameters and body are both known.
    /// `unique` means duplicate names are errors even in sloppy code (methods, arrows).
    fn check_params(&mut self, params: List, unique: bool) -> PResult<()> {
        let ids: Vec<NodeId> = self.ast.list(params).to_vec();
        let simple = ids.iter().all(|&p| matches!(self.ast[p].kind, NodeKind::BindingIdent(_)));
        let strict_directive = self.last_body_strict;
        if strict_directive && !simple {
            let pos = ids.first().map_or(self.prev_end, |&p| self.ast[p].span.start);
            return Err(self.error_at("A `use strict` directive is not allowed with non-simple parameters", pos));
        }
        if self.strict || strict_directive || unique || !simple {
            let mut names = Vec::new();
            for &p in &ids {
                self.bound_names(p, &mut names);
            }
            for (i, name) in names.iter().enumerate() {
                if names[..i].contains(name) {
                    let pos = ids.first().map_or(self.prev_end, |&p| self.ast[p].span.start);
                    return Err(self.error_at("Duplicate parameter name", pos));
                }
            }
        }
        Ok(())
    }

    pub(super) fn parse_function_body(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.expect(Punct::LBrace)?;
        let saved_strict = self.strict;
        let saved_no_in = std::mem::replace(&mut self.no_in, false);
        // A function body is a fresh context: a `:` in it never belongs to an outer `?`.
        let saved_consequent = std::mem::replace(&mut self.in_consequent, false);
        let mark = self.mark();
        let result = self.parse_directives_and_statements(false).and_then(|_| self.expect(Punct::RBrace));
        self.strict = saved_strict;
        self.no_in = saved_no_in;
        self.in_consequent = saved_consequent;
        result?;
        let list = self.take_list(mark);
        Ok(self.finish(NodeKind::Block(list), start))
    }

    // ---- classes ------------------------------------------------------------------------

    pub(super) fn parse_class(&mut self, start: u32, decl: bool, name_required: bool) -> PResult<NodeId> {
        self.expect_kw(Kw::Class)?;
        let saved_strict = std::mem::replace(&mut self.strict, true);
        let result = self.parse_class_rest(start, decl, name_required);
        self.strict = saved_strict;
        result
    }

    fn parse_class_rest(&mut self, start: u32, decl: bool, name_required: bool) -> PResult<NodeId> {
        let outer_decorators = std::mem::take(&mut self.pending_decorators);
        let result = self.parse_class_rest_inner(start, decl, name_required, outer_decorators);
        result
    }

    fn parse_class_rest_inner(&mut self, start: u32, decl: bool, name_required: bool, decorators: Vec<crate::ast::Decorator>) -> PResult<NodeId> {
        let name = if self.tok.kind == Kind::Ident {
            Some(self.parse_binding_ident_plain()?)
        } else if name_required {
            return Err(self.unexpected());
        } else {
            None
        };
        if self.ts && self.at(Punct::Lt) {
            self.ts_type_params_erase()?;
        }
        // A class declaration binds its name outside, and every class sees its own name inside.
        let name_atom = name.and_then(|n| match self.ast[n].kind {
            NodeKind::BindingIdent(atom) => Some((n, atom)),
            _ => None,
        });
        if let (true, Some((n, atom))) = (decl, name_atom) {
            let strict = self.strict;
            self.sb.declare_lexical(atom, SymbolKind::Class, n, strict).map_err(|r| self.redeclared(r))?;
        }
        self.push_scope(ScopeKind::Class);
        if let Some((n, atom)) = name_atom {
            self.sb.declare_class_inner(atom, n, !decl);
        }
        let superclass = if self.at_kw(Kw::Extends) {
            self.next()?;
            let heritage = self.parse_lhs()?;
            self.reject_arrow(heritage)?;
            if self.ts && self.at(Punct::Lt) {
                self.ts_type_args_erase()?;
            }
            Some(heritage)
        } else {
            None
        };
        if self.ts && self.at_word("implements") {
            let implements = self.tok.start;
            self.ts_in_type(|p| {
                p.next()?;
                loop {
                    p.ts_type()?;
                    if !p.eat(Punct::Comma)? {
                        return Ok(());
                    }
                }
            })?;
            self.ts_erase_from(implements);
        }
        let body_open = self.tok.start;
        self.expect(Punct::LBrace)?;
        let saved_extends = std::mem::replace(&mut self.class_extends, superclass.is_some());
        let saved_constructor = std::mem::replace(&mut self.class_has_constructor, false);
        self.private_scopes.push(PrivateScope::default());
        let mark = self.mark();
        while !self.at(Punct::RBrace) {
            if self.eat(Punct::Semi)? {
                continue;
            }
            if let Some(member) = self.parse_class_member()? {
                self.push(member);
            }
        }
        self.class_extends = saved_extends;
        self.class_has_constructor = saved_constructor;
        self.close_private_scope()?;
        let body_close = self.tok.start;
        self.expect(Punct::RBrace)?;
        let members = self.take_list(mark);
        let decl_flag = if decl { flags::DECL } else { 0 };
        let node = self.finish_with(NodeKind::Class { name, superclass, members }, start, decl_flag);
        self.sb.pop(node);
        self.ast.add_decorators(node, decorators);
        if self.ts {
            self.ast.set_class_body(node, body_open, body_close);
        }
        Ok(node)
    }

    /// `accessor` is a modifier only when a member name follows it on the same line.
    fn accessor_keyword_follows(&mut self) -> PResult<bool> {
        let next = self.peek()?;
        Ok(!next.newline_before
            && matches!(next.kind, Kind::Ident | Kind::Keyword(_) | Kind::String | Kind::Number | Kind::BigInt | Kind::PrivateName | Kind::Punct(Punct::LBracket)))
    }

    /// The current word (`static`, `async`, `get`, `set`) is a modifier only if what follows
    /// can start a member name. Otherwise it is the member's name itself.
    fn is_modifier(&mut self, no_newline: bool) -> PResult<bool> {
        let next = self.peek()?;
        if no_newline && next.newline_before {
            return Ok(false);
        }
        // In TypeScript a member can also be named `get` and then continue with `<T>` or `?`.
        if self.ts && matches!(next.kind, Kind::Punct(Punct::Lt | Punct::Question | Punct::Bang)) {
            return Ok(false);
        }
        Ok(!matches!(
            next.kind,
            Kind::Eof | Kind::Punct(Punct::LParen | Punct::Comma | Punct::Colon | Punct::RBrace | Punct::Eq | Punct::Semi)
        ))
    }

    /// A class member, or `None` for TypeScript-only members that leave no trace: index
    /// signatures, overload signatures, and abstract or `declare`d members.
    fn parse_class_member(&mut self) -> PResult<Option<NodeId>> {
        let decorators = if self.ts && self.at(Punct::At) { self.parse_decorators()? } else { Vec::new() };
        let outer_member = std::mem::replace(&mut self.in_member, true);
        let member = self.parse_class_member_inner();
        self.in_member = outer_member;
        let member = member?;
        if let Some(node) = member {
            self.ast.add_decorators(node, decorators);
        }
        Ok(member)
    }

    fn parse_class_member_inner(&mut self) -> PResult<Option<NodeId>> {
        let start = self.tok.start;
        let mut member_flags = 0u8;
        // `abstract` and `declare` members exist only as types.
        let mut type_only = false;
        if self.ts {
            loop {
                if self.at_word("static") && self.is_modifier(false)? {
                    self.next()?;
                    if self.at(Punct::LBrace) {
                        return self.parse_static_block(start).map(Some);
                    }
                    member_flags |= flags::STATIC;
                } else if let Some(word) = self.ts_member_modifier()? {
                    type_only |= word == "abstract" || word == "declare";
                } else if self.at_word("accessor") && self.accessor_keyword_follows()? {
                    // `accessor x = 1`: an auto-accessor, a field with a getter and setter.
                    self.next()?;
                    member_flags |= flags::ACCESSOR;
                } else {
                    break;
                }
            }
            if self.at(Punct::LBracket) && self.ts_index_signature_ahead() {
                self.ts_balanced()?;
                if self.at(Punct::Colon) {
                    self.ts_annotation()?;
                }
                if !self.eat(Punct::Semi)? {
                    self.eat(Punct::Comma)?;
                }
                self.ts_erase_from(start);
                return Ok(None);
            }
        } else if self.at_word("static") && self.is_modifier(false)? {
            self.next()?;
            if self.at(Punct::LBrace) {
                return self.parse_static_block(start).map(Some);
            }
            member_flags |= flags::STATIC;
        }
        let (is_async, is_generator, accessor) = self.parse_member_modifiers()?;
        let (key, computed) = self.parse_property_key(true)?;
        if self.ts {
            // `name?` (optional) or `name!` (definitely assigned).
            self.ts_optional_marker()?;
            self.ts_definite_marker()?;
        }
        if computed {
            member_flags |= flags::COMPUTED;
        }
        let is_static = member_flags & flags::STATIC != 0;
        let is_constructor_name = self.key_is(key, computed, known::CONSTRUCTOR);
        let is_prototype_name = self.key_is(key, computed, known::PROTOTYPE);
        if let NodeKind::PrivateName(name) = self.ast[key].kind {
            if name == known::CONSTRUCTOR {
                return Err(self.error_at("A class cannot have a private name `#constructor`", self.ast[key].span.start));
            }
            let how = match accessor {
                Some(PropKind::Get) => PRIVATE_GETTER,
                Some(PropKind::Set) => PRIVATE_SETTER,
                _ => PRIVATE_OTHER,
            };
            let is_static = if member_flags & flags::STATIC != 0 { PRIVATE_STATIC } else { 0 };
            self.declare_private(name, how | is_static, self.ast[key].span.start)?;
        }
        if accessor.is_some() || is_async || is_generator || self.at(Punct::LParen) || (self.ts && self.at(Punct::Lt)) {
            let kind = match accessor {
                Some(PropKind::Get) => MethodKind::Get,
                Some(PropKind::Set) => MethodKind::Set,
                _ if member_flags & (flags::STATIC | flags::COMPUTED) == 0 && self.is_named(key, known::CONSTRUCTOR) => {
                    MethodKind::Constructor
                }
                _ => MethodKind::Method,
            };
            if is_static && is_prototype_name {
                return Err(self.error_at("A class cannot have a static method named `prototype`", start));
            }
            if kind == MethodKind::Constructor {
                if is_generator || is_async {
                    return Err(self.error_at("A class constructor cannot be a generator or async", start));
                }
                // Overload signatures of a constructor have no body and do not count.
                if !self.ts && std::mem::replace(&mut self.class_has_constructor, true) {
                    return Err(self.error_at("A class may only have one constructor", start));
                }
            } else if !is_static && is_constructor_name {
                return Err(self.error_at("A class constructor cannot be an accessor", start));
            }
            let super_call = kind == MethodKind::Constructor && self.class_extends;
            let value = self.parse_function_rest(self.tok.start, None, is_async, is_generator, 0, true, super_call)?;
            if self.ts && matches!(self.ast[value].kind, NodeKind::Empty) {
                self.ts_erase_from(start);
                return Ok(None);
            }
            if self.ts && kind == MethodKind::Constructor && std::mem::replace(&mut self.class_has_constructor, true) {
                return Err(self.error_at("A class may only have one constructor", start));
            }
            self.check_accessor(accessor, value)?;
            return Ok(Some(self.finish_with(NodeKind::ClassMethod { key, value, kind }, start, member_flags)));
        }
        if is_constructor_name {
            return Err(self.error_at("A class cannot have a field named `constructor`", start));
        }
        if is_static && is_prototype_name {
            return Err(self.error_at("A class cannot have a static field named `prototype`", start));
        }
        let mut field_meta = None;
        if self.ts && self.at(Punct::Colon) {
            if self.meta {
                field_meta = Some(self.ts_annotation_meta()?);
            } else {
                self.ts_annotation()?;
            }
        }
        let value = if self.eat(Punct::Eq)? {
            let context =
                FnContext { in_function: true, super_prop: true, new_target: true, class_init: true, ..FnContext::default() };
            let saved = std::mem::replace(&mut self.cx, context);
            let value = self.allow_in(|p| p.parse_assign());
            self.cx = saved;
            Some(value?)
        } else {
            None
        };
        self.semicolon()?;
        if type_only {
            self.ts_erase_from(start);
            return Ok(None);
        }
        let field = self.finish_with(NodeKind::ClassField { key, value }, start, member_flags);
        if let Some(meta) = field_meta {
            self.ast.set_type_meta(field, meta);
        }
        Ok(Some(field))
    }

    fn parse_static_block(&mut self, start: u32) -> PResult<NodeId> {
        self.expect(Punct::LBrace)?;
        self.push_scope(ScopeKind::StaticBlock);
        let context = FnContext {
            in_function: true,
            super_prop: true,
            new_target: true,
            class_init: true,
            static_block: true,
            ..FnContext::default()
        };
        let saved = std::mem::replace(&mut self.cx, context);
        let mark = self.mark();
        let mut result = Ok(());
        while !self.at(Punct::RBrace) && self.tok.kind != Kind::Eof {
            match self.parse_statement_list_item() {
                Ok(stmt) => self.push(stmt),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.cx = saved;
        result?;
        self.expect(Punct::RBrace)?;
        let list = self.take_list(mark);
        let node = self.finish_with(NodeKind::StaticBlock(list), start, flags::STATIC);
        self.sb.pop(node);
        Ok(node)
    }

    /// Whether `key` is the non-computed name `name`.
    fn is_named(&self, key: NodeId, name: Atom) -> bool {
        matches!(self.ast[key].kind, NodeKind::PropName(a) | NodeKind::Str(a) if a == name)
    }

    /// `async`, `*`, `get` and `set` in front of a method name.
    fn parse_member_modifiers(&mut self) -> PResult<(bool, bool, Option<PropKind>)> {
        let mut is_async = false;
        if self.at_word("async") && self.is_modifier(true)? {
            self.next()?;
            is_async = true;
        }
        let is_generator = self.eat(Punct::Star)?;
        let mut accessor = None;
        if !is_async && !is_generator && (self.at_word("get") || self.at_word("set")) && self.is_modifier(false)? {
            accessor = Some(if self.at_word("get") { PropKind::Get } else { PropKind::Set });
            self.next()?;
        }
        Ok((is_async, is_generator, accessor))
    }

    /// A property key. Returns the key node and whether it was `[computed]`.
    pub(super) fn parse_property_key(&mut self, allow_private: bool) -> PResult<(NodeId, bool)> {
        let tok = self.tok;
        let start = tok.start;
        match tok.kind {
            Kind::Ident | Kind::Keyword(_) => {
                let name = self.name_of(tok);
                self.next()?;
                Ok((self.finish(NodeKind::PropName(name), start), false))
            }
            Kind::String => {
                let atom = self.string_atom(tok)?;
                self.next()?;
                Ok((self.finish(NodeKind::Str(atom), start), false))
            }
            Kind::Number => {
                let text = self.text(tok);
                self.check_legacy_octal(text, start)?;
                let value = literal::parse_number(text);
                self.next()?;
                Ok((self.finish(NodeKind::Num(Num::new(value)), start), false))
            }
            Kind::BigInt => {
                let digits = literal::bigint_digits(self.text(tok));
                let atom = self.atom(&digits);
                self.next()?;
                Ok((self.finish(NodeKind::BigInt(atom), start), false))
            }
            Kind::Punct(Punct::LBracket) => {
                self.next()?;
                let expr = self.allow_in(|p| p.parse_assign())?;
                self.expect(Punct::RBracket)?;
                Ok((expr, true))
            }
            Kind::PrivateName if allow_private => {
                let name = self.private_name_atom(tok);
                self.next()?;
                Ok((self.finish(NodeKind::PrivateName(name), start), false))
            }
            _ => Err(self.unexpected()),
        }
    }

    // ---- array and object literals ------------------------------------------------------

    fn parse_array(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        self.allow_in(|p| {
            let mark = p.mark();
            let mut rest_comma = false;
            while !p.at(Punct::RBracket) {
                if p.at(Punct::Comma) {
                    let hole = p.ast.alloc(NodeKind::Hole, Span { start: p.tok.start, end: p.tok.start }, 0);
                    p.push(hole);
                    p.next()?;
                    continue;
                }
                let item_start = p.tok.start;
                let item = if p.eat(Punct::Ellipsis)? {
                    let arg = p.parse_assign_cover()?;
                    p.finish(NodeKind::Spread(arg), item_start)
                } else {
                    p.parse_assign_cover()?
                };
                p.push(item);
                rest_comma |= matches!(p.ast[item].kind, NodeKind::Spread(_)) && p.at(Punct::Comma);
                if !p.at(Punct::RBracket) {
                    p.expect(Punct::Comma)?;
                }
            }
            p.expect(Punct::RBracket)?;
            let list = p.take_list(mark);
            Ok(p.finish_with(NodeKind::Array(list), start, if rest_comma { flags::REST_COMMA } else { 0 }))
        })
    }

    fn parse_object(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        self.allow_in(|p| {
            let mark = p.mark();
            let (mut rest_comma, mut protos) = (false, 0);
            while !p.at(Punct::RBrace) {
                let member = p.parse_object_member()?;
                p.push(member);
                rest_comma |= matches!(p.ast[member].kind, NodeKind::Spread(_)) && p.at(Punct::Comma);
                if p.is_proto_setter(member) {
                    protos += 1;
                    if protos == 2 {
                        // Legal only if this literal becomes a destructuring pattern.
                        let pos = p.ast[member].span.start;
                        p.cover_init.get_or_insert((pos, DUPLICATE_PROTO));
                    }
                }
                if !p.at(Punct::RBrace) {
                    p.expect(Punct::Comma)?;
                }
            }
            p.expect(Punct::RBrace)?;
            let list = p.take_list(mark);
            Ok(p.finish_with(NodeKind::Object(list), start, if rest_comma { flags::REST_COMMA } else { 0 }))
        })
    }

    /// `__proto__: value` (not shorthand, computed, or a method), which sets the prototype.
    fn is_proto_setter(&self, member: NodeId) -> bool {
        let node = &self.ast[member];
        let NodeKind::Property { key, kind: PropKind::Init, .. } = node.kind else { return false };
        !node.has(flags::COMPUTED)
            && !node.has(flags::SHORTHAND)
            && matches!(self.ast[key].kind, NodeKind::PropName(a) | NodeKind::Str(a) if a == known::PROTO)
    }

    /// Whether `key` is the plain (not computed) name `name`.
    fn key_is(&self, key: NodeId, computed: bool, name: Atom) -> bool {
        !computed && self.is_named(key, name)
    }

    /// Getters take no parameters; setters take exactly one, and not a rest parameter.
    fn check_accessor(&self, accessor: Option<PropKind>, function: NodeId) -> PResult<()> {
        let NodeKind::Function { params, .. } = self.ast[function].kind else { return Ok(()) };
        let params = self.ast.list(params);
        let pos = self.ast[function].span.start;
        match accessor {
            Some(PropKind::Get) if !params.is_empty() => Err(self.error_at("A getter must not have parameters", pos)),
            Some(PropKind::Set) if params.len() != 1 => Err(self.error_at("A setter must have exactly one parameter", pos)),
            Some(PropKind::Set) if matches!(self.ast[params[0]].kind, NodeKind::Rest(_)) => {
                Err(self.error_at("A setter parameter cannot be a rest parameter", pos))
            }
            _ => Ok(()),
        }
    }

    fn parse_object_member(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        if self.eat(Punct::Ellipsis)? {
            let arg = self.parse_assign_cover()?;
            return Ok(self.finish(NodeKind::Spread(arg), start));
        }
        let (is_async, is_generator, accessor) = self.parse_member_modifiers()?;
        let key_tok = self.tok;
        let (key, computed) = self.parse_property_key(false)?;
        let mut prop_flags = if computed { flags::COMPUTED } else { 0 };
        if accessor.is_some() || is_async || is_generator || self.at(Punct::LParen) || (self.ts && self.at(Punct::Lt)) {
            let kind = accessor.unwrap_or(PropKind::Method);
            let value = self.parse_function_rest(self.tok.start, None, is_async, is_generator, 0, true, false)?;
            self.check_accessor(accessor, value)?;
            return Ok(self.finish_with(NodeKind::Property { key, value, kind }, start, prop_flags));
        }
        if self.eat(Punct::Colon)? {
            let value = self.parse_assign_cover()?;
            return Ok(self.finish_with(NodeKind::Property { key, value, kind: PropKind::Init }, start, prop_flags));
        }
        // Shorthand: `{ a }` or, if this becomes a pattern, `{ a = 1 }`.
        if key_tok.kind != Kind::Ident || computed {
            return Err(self.unexpected_expecting(":"));
        }
        let name = self.check_ident_ref(key_tok)?;
        let ident = self.ast.alloc(NodeKind::Ident(name), Span { start: key_tok.start, end: key_tok.end }, 0);
        self.record_ref(ident);
        let value = if self.at(Punct::Eq) {
            self.cover_init.get_or_insert((self.tok.start, SHORTHAND_INIT));
            self.next()?;
            let default = self.parse_assign()?;
            self.finish(NodeKind::Assign { op: AssignOp::Assign, target: ident, value: default }, key_tok.start)
        } else {
            ident
        };
        prop_flags |= flags::SHORTHAND;
        Ok(self.finish_with(NodeKind::Property { key, value, kind: PropKind::Init }, start, prop_flags))
    }

    // ---- templates ----------------------------------------------------------------------

    /// Cursor on a template head or no-substitution template.
    pub(super) fn parse_template(&mut self, tagged: bool) -> PResult<NodeId> {
        let start = self.tok.start;
        let mut quasis = Vec::new();
        let mut exprs = Vec::new();
        loop {
            let tok = self.tok;
            let text = self.text(tok);
            let trailing = match tok.kind {
                Kind::NoSubTemplate | Kind::TemplateTail => 1,
                _ => 2,
            };
            let raw = &text[1..text.len() - trailing];
            let element = if !raw.contains('\\') && !raw.contains('\r') {
                let atom = self.ast.intern(raw);
                NodeKind::TemplateElement { cooked: Some(atom), raw: atom }
            } else {
                let cooked = literal::cook_template(raw);
                if cooked.is_none() && !tagged {
                    return Err(self.error_at("Invalid escape sequence in template", tok.start));
                }
                let cooked = cooked.map(|c| self.ast.intern(&c));
                let raw = self.ast.intern(&literal::template_raw(raw));
                NodeKind::TemplateElement { cooked, raw }
            };
            quasis.push(self.ast.alloc(element, Span { start: tok.start, end: tok.end }, 0));
            match tok.kind {
                Kind::NoSubTemplate | Kind::TemplateTail => {
                    self.next()?;
                    break;
                }
                _ => {
                    self.next()?;
                    let expr = self.allow_in(|p| p.parse_expression())?;
                    exprs.push(expr);
                    if !self.at(Punct::RBrace) {
                        return Err(self.unexpected_expecting("}"));
                    }
                    self.tok = self.lexer.rescan_template_continuation(self.tok)?;
                }
            }
        }
        let quasis = self.ast.alloc_list(&quasis);
        let exprs = self.ast.alloc_list(&exprs);
        Ok(self.finish(NodeKind::Template { quasis, exprs }, start))
    }

    // ---- import() and import.meta -------------------------------------------------------

    fn parse_import_expr(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        if self.eat(Punct::Dot)? {
            self.expect_word("meta")?;
            if !self.module {
                return Err(self.error_at("`import.meta` is only allowed in modules", start));
            }
            return Ok(self.finish(NodeKind::ImportMeta, start));
        }
        if !self.at(Punct::LParen) {
            return Err(self.unexpected_expecting("("));
        }
        self.next()?;
        self.allow_in(|p| {
            let source = p.parse_assign()?;
            let mut options = None;
            if p.eat(Punct::Comma)? && !p.at(Punct::RParen) {
                options = Some(p.parse_assign()?);
                p.eat(Punct::Comma)?;
            }
            p.expect(Punct::RParen)?;
            Ok(p.finish(NodeKind::ImportCall { source, options }, start))
        })
    }
}
