//! TypeScript syntax.
//!
//! Types are not parsed into the tree. TypeScript is erased when bundled, so what matters is
//! where each type *ends*, and that is all the skipper below works out. The skipped text is
//! recorded as an erased span, and the bundler leaves it out of the output.
//!
//! Anything that changes what the program does is different: `enum`, parameter properties and
//! `import x = require()` become real nodes (or side tables) that the bundler writes out as
//! JavaScript.

use super::*;
use crate::ast::VarKind;
use crate::lexer::Token;
use crate::semantic::{ScopeKind, SymbolKind};

/// Where the lexer and parser were, to come back to after a lookahead that did not pan out.
#[derive(Clone, Copy)]
pub(super) struct Mark {
    pos: u32,
    tok: Token,
    prev_end: u32,
    erased: usize,
}

/// What opens a bracketed run the skipper is stepping over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Closer {
    Paren,
    Bracket,
    Brace,
    /// A `${` inside a template; closed by the `}` that the lexer must re-read.
    Template,
}

impl<'a> Parser<'a> {
    // ---- lookahead ----------------------------------------------------------------------

    pub(super) fn ts_mark(&self) -> Mark {
        Mark { pos: self.lexer.pos(), tok: self.tok, prev_end: self.prev_end, erased: self.ast.erased_len() }
    }

    pub(super) fn ts_reset(&mut self, mark: Mark) {
        self.lexer.reset(mark.pos);
        self.tok = mark.tok;
        self.prev_end = mark.prev_end;
        self.ast.truncate_erased(mark.erased);
    }

    /// Runs `f`; if it fails or answers `false`, the parser is put back where it was.
    pub(super) fn ts_try(&mut self, f: impl FnOnce(&mut Self) -> PResult<bool>) -> bool {
        let mark = self.ts_mark();
        let saved = (self.in_type, self.no_conditional);
        match f(self) {
            Ok(true) => true,
            _ => {
                (self.in_type, self.no_conditional) = saved;
                self.ts_reset(mark);
                false
            }
        }
    }

    /// Asks `f` a question and always puts the parser back.
    pub(super) fn ts_look<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> Option<T> {
        let mark = self.ts_mark();
        let saved = (self.in_type, self.no_conditional);
        let result = f(self).ok();
        (self.in_type, self.no_conditional) = saved;
        self.ts_reset(mark);
        result
    }

    /// Marks `start..` up to the last consumed token as text to leave out.
    pub(super) fn ts_erase_from(&mut self, start: u32) {
        let end = self.prev_end;
        self.ast.erase(start, end);
    }

    /// Reads with `>` kept as a single token, which is what a type needs.
    pub(super) fn ts_in_type<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let saved = std::mem::replace(&mut self.in_type, true);
        let result = f(self);
        self.in_type = saved;
        if !saved {
            // The token after the type was read as a lone `>`; read it again as an operator.
            self.join_gt();
        }
        result
    }

    // ---- type annotations ---------------------------------------------------------------

    /// Cursor on `:`. Erases the colon and the type.
    pub(super) fn ts_annotation(&mut self) -> PResult<()> {
        let start = self.tok.start;
        self.ts_in_type(|p| {
            p.next()?;
            p.ts_type()
        })?;
        self.ts_erase_from(start);
        Ok(())
    }

    /// Cursor on `:` after a function's parameters. The type may be a predicate.
    pub(super) fn ts_return_annotation(&mut self) -> PResult<()> {
        let start = self.tok.start;
        self.ts_in_type(|p| {
            p.next()?;
            p.ts_return_type()
        })?;
        self.ts_erase_from(start);
        Ok(())
    }

    /// Cursor on `<` of a declaration's type parameters: `<T extends U = V, const W>`.
    pub(super) fn ts_type_params_erase(&mut self) -> PResult<()> {
        let start = self.tok.start;
        self.ts_in_type(|p| p.ts_type_params())?;
        self.ts_erase_from(start);
        Ok(())
    }

    /// Cursor on `<` of type arguments in an expression or heritage clause.
    pub(super) fn ts_type_args_erase(&mut self) -> PResult<()> {
        let start = self.tok.start;
        self.ts_in_type(|p| p.ts_type_args())?;
        self.ts_erase_from(start);
        Ok(())
    }

    // ---- the type grammar, skipped ------------------------------------------------------

    /// A type inside brackets, where `a extends b ? c : d` is allowed again even if the
    /// brackets sit in the `extends` operand of an enclosing conditional.
    fn ts_type_allowing_conditional(&mut self) -> PResult<()> {
        let saved = std::mem::replace(&mut self.no_conditional, false);
        let result = self.ts_type();
        self.no_conditional = saved;
        result
    }

    pub(super) fn ts_type(&mut self) -> PResult<()> {
        if self.ts_at_function_type()? {
            return self.ts_function_type();
        }
        self.ts_union()?;
        if !self.no_conditional && self.at_kw(Kw::Extends) && !self.tok.newline_before {
            self.next()?;
            let saved = std::mem::replace(&mut self.no_conditional, true);
            let operand = self.ts_type();
            self.no_conditional = saved;
            operand?;
            self.expect(Punct::Question)?;
            let saved = std::mem::replace(&mut self.no_conditional, false);
            let branches = self.ts_type().and_then(|_| self.expect(Punct::Colon)).and_then(|_| self.ts_type());
            self.no_conditional = saved;
            branches?;
        }
        Ok(())
    }

    /// The type after `=>` or `:` on a function: `x is T`, `asserts x`, or any type.
    fn ts_return_type(&mut self) -> PResult<()> {
        if self.at_word("asserts") {
            let next = self.peek()?;
            if !next.newline_before && (next.kind == Kind::Ident || next.kind == Kind::Keyword(Kw::This)) {
                self.next()?;
                self.next()?;
                if self.at_word("is") && !self.tok.newline_before {
                    self.next()?;
                    self.ts_type()?;
                }
                return Ok(());
            }
        }
        if self.tok.kind == Kind::Ident || self.at_kw(Kw::This) {
            let next = self.peek()?;
            if next.kind == Kind::Ident && !next.newline_before && self.text(next) == "is" {
                self.next()?;
                self.next()?;
                return self.ts_type();
            }
        }
        self.ts_type()
    }

    fn ts_at_function_type(&mut self) -> PResult<bool> {
        Ok(match self.tok.kind {
            Kind::Punct(Punct::Lt) => true,
            Kind::Keyword(Kw::New) => true,
            Kind::Ident if self.at_word("abstract") => self.peek()?.kind == Kind::Keyword(Kw::New),
            // Only a parameter list can start with what follows `(`: `)`, `...`, a name followed
            // by `:` `,` `?` `=` or `)`, or a pattern. `(() => void)` is a parenthesized type.
            Kind::Punct(Punct::LParen) => self
                .ts_look(|p| {
                    p.next()?;
                    let starts_parameter = match p.tok.kind {
                        Kind::Punct(Punct::RParen | Punct::Ellipsis) => true,
                        Kind::Ident | Kind::Keyword(Kw::This) => {
                            let next = p.peek()?;
                            matches!(next.kind, Kind::Punct(Punct::Colon | Punct::Comma | Punct::Question | Punct::Eq | Punct::RParen))
                        }
                        Kind::Punct(Punct::LBrace | Punct::LBracket) => true,
                        _ => false,
                    };
                    if !starts_parameter {
                        return Ok(false);
                    }
                    // Back at the `(`, step over the whole list and look for the arrow.
                    Ok(true)
                })
                .unwrap_or(false)
                && self
                    .ts_look(|p| {
                        p.ts_balanced()?;
                        Ok(p.at(Punct::Arrow))
                    })
                    .unwrap_or(false),
            _ => false,
        })
    }

    fn ts_function_type(&mut self) -> PResult<()> {
        if self.at_word("abstract") {
            self.next()?;
        }
        if self.at_kw(Kw::New) {
            self.next()?;
        }
        if self.at(Punct::Lt) {
            self.ts_type_params()?;
        }
        if !self.at(Punct::LParen) {
            return Err(self.unexpected_expecting("("));
        }
        self.ts_balanced()?;
        self.expect(Punct::Arrow)?;
        let saved = std::mem::replace(&mut self.no_conditional, false);
        let result = self.ts_return_type();
        self.no_conditional = saved;
        result
    }

    fn ts_union(&mut self) -> PResult<()> {
        self.eat(Punct::Pipe)?;
        self.ts_intersection()?;
        while self.at(Punct::Pipe) {
            self.next()?;
            self.ts_intersection()?;
        }
        Ok(())
    }

    fn ts_intersection(&mut self) -> PResult<()> {
        self.eat(Punct::Amp)?;
        self.ts_operator()?;
        while self.at(Punct::Amp) {
            self.next()?;
            self.ts_operator()?;
        }
        Ok(())
    }

    /// Whether the token can begin a type, which tells `keyof T` from a type named `keyof`.
    fn ts_starts_type(&self, tok: Token) -> bool {
        matches!(
            tok.kind,
            Kind::Ident
                | Kind::Keyword(_)
                | Kind::String
                | Kind::Number
                | Kind::BigInt
                | Kind::NoSubTemplate
                | Kind::TemplateHead
                | Kind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace | Punct::Lt | Punct::Minus)
        )
    }

    fn ts_operator(&mut self) -> PResult<()> {
        if self.tok.kind == Kind::Ident && !self.tok.escaped {
            match self.text(self.tok) {
                "keyof" | "unique" | "readonly" => {
                    let next = self.peek()?;
                    if self.ts_starts_type(next) {
                        self.next()?;
                        return self.ts_operator();
                    }
                }
                "infer" => {
                    let next = self.peek()?;
                    if next.kind == Kind::Ident {
                        self.next()?;
                        self.next()?;
                        if self.at_kw(Kw::Extends) {
                            // `infer U extends X` has a constraint, unless a `?` follows it,
                            // in which case the `extends` belongs to the enclosing conditional.
                            let outer = self.no_conditional;
                            let constrained = self.ts_try(|p| {
                                p.next()?;
                                let saved = std::mem::replace(&mut p.no_conditional, true);
                                let result = p.ts_type();
                                p.no_conditional = saved;
                                result?;
                                // Inside the operand of another `extends`, a `?` after it cannot
                                // belong to a conditional of its own, so it is a constraint.
                                Ok(outer || !p.at(Punct::Question))
                            });
                            let _ = constrained;
                        }
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        self.ts_postfix()
    }

    fn ts_postfix(&mut self) -> PResult<()> {
        self.ts_primary()?;
        while self.at(Punct::LBracket) && !self.tok.newline_before {
            self.ts_balanced()?;
        }
        Ok(())
    }

    fn ts_primary(&mut self) -> PResult<()> {
        let tok = self.tok;
        match tok.kind {
            Kind::Ident => {
                self.next()?;
                self.ts_entity_rest()
            }
            Kind::Keyword(kw) => match kw {
                Kw::This | Kw::Void | Kw::Null | Kw::True | Kw::False | Kw::Const => self.next(),
                Kw::Typeof => {
                    self.next()?;
                    if self.at_kw(Kw::Import) {
                        return self.ts_import_type();
                    }
                    if !(self.tok.kind == Kind::Ident || self.at_kw(Kw::This)) {
                        return Err(self.unexpected());
                    }
                    self.next()?;
                    self.ts_entity_rest()
                }
                Kw::Import => self.ts_import_type(),
                _ => Err(self.unexpected()),
            },
            Kind::String | Kind::Number | Kind::BigInt | Kind::NoSubTemplate => self.next(),
            Kind::TemplateHead => self.ts_template_type(),
            Kind::Punct(Punct::Minus) => {
                self.next()?;
                if matches!(self.tok.kind, Kind::Number | Kind::BigInt) {
                    self.next()
                } else {
                    Err(self.unexpected())
                }
            }
            Kind::Punct(Punct::LParen) => {
                self.next()?;
                self.ts_type_allowing_conditional()?;
                self.expect(Punct::RParen)
            }
            Kind::Punct(Punct::LBracket | Punct::LBrace) => self.ts_balanced(),
            _ => Err(self.unexpected()),
        }
    }

    /// After the first name of a type reference: `.More.Names` and `<Arguments>`.
    fn ts_entity_rest(&mut self) -> PResult<()> {
        while self.at(Punct::Dot) {
            self.next()?;
            if !matches!(self.tok.kind, Kind::Ident | Kind::Keyword(_)) {
                return Err(self.unexpected());
            }
            self.next()?;
        }
        if self.at(Punct::Lt) && !self.tok.newline_before {
            self.ts_type_args()?;
        }
        Ok(())
    }

    /// `import("module").Name<Args>` as a type (cursor on `import`).
    fn ts_import_type(&mut self) -> PResult<()> {
        self.next()?;
        if !self.at(Punct::LParen) {
            return Err(self.unexpected_expecting("("));
        }
        self.ts_balanced()?;
        self.ts_entity_rest_after_dot()
    }

    fn ts_entity_rest_after_dot(&mut self) -> PResult<()> {
        self.ts_entity_rest()
    }

    fn ts_template_type(&mut self) -> PResult<()> {
        loop {
            // On a template head or middle: skip the type inside `${ ... }`.
            self.next()?;
            self.ts_type_allowing_conditional()?;
            if !self.at(Punct::RBrace) {
                return Err(self.unexpected_expecting("}"));
            }
            self.tok = self.lexer.rescan_template_continuation(self.tok)?;
            if self.tok.kind == Kind::TemplateTail {
                return self.next();
            }
        }
    }

    /// `<` type, type ... `>`
    pub(super) fn ts_type_args(&mut self) -> PResult<()> {
        self.expect(Punct::Lt)?;
        while !self.at(Punct::Gt) {
            self.ts_type_allowing_conditional()?;
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        self.expect(Punct::Gt)
    }

    /// `<T extends U = V, const W, in out X>`
    pub(super) fn ts_type_params(&mut self) -> PResult<()> {
        self.expect(Punct::Lt)?;
        while !self.at(Punct::Gt) {
            // `const`, `in` and `out` are modifiers when a name or another modifier follows.
            loop {
                let is_modifier = self.at_kw(Kw::Const) || self.at_kw(Kw::In) || self.at_word("out");
                if !is_modifier {
                    break;
                }
                let next = self.peek()?;
                if !(next.kind == Kind::Ident || matches!(next.kind, Kind::Keyword(Kw::In | Kw::Const))) {
                    break;
                }
                self.next()?;
            }
            if self.tok.kind != Kind::Ident {
                return Err(self.unexpected());
            }
            self.next()?;
            if self.at_kw(Kw::Extends) {
                self.next()?;
                self.ts_type()?;
            }
            if self.eat(Punct::Eq)? {
                self.ts_type()?;
            }
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        self.expect(Punct::Gt)
    }

    /// Steps over a bracketed run of tokens, `( ... )`, `[ ... ]` or `{ ... }`, without
    /// reading what is inside beyond what finding the end takes: nested brackets, template
    /// substitutions and regular expressions.
    pub(super) fn ts_balanced(&mut self) -> PResult<()> {
        let mut stack: Vec<Closer> = Vec::new();
        // Whether the previous token could end an expression, which decides what `/` means.
        let mut after_value = false;
        // For `balanced_has_types`: a `:` at the top level of a parenthesized group that no
        // `?` of a conditional accounts for is a type annotation.
        let (mut ternaries, mut question_pending) = (0u32, false);
        self.balanced_has_types = false;
        loop {
            let tok = self.tok;
            if question_pending {
                question_pending = false;
                match tok.kind {
                    Kind::Punct(Punct::Colon | Punct::Comma | Punct::RParen) => self.balanced_has_types = true,
                    _ => ternaries += 1,
                }
            }
            let top_level = stack.len() == 1 && stack[0] == Closer::Paren;
            match tok.kind {
                Kind::Eof => return Err(self.unexpected()),
                Kind::Punct(Punct::Question) if top_level => question_pending = true,
                Kind::Punct(Punct::Colon) if top_level => {
                    if ternaries > 0 {
                        ternaries -= 1;
                    } else {
                        self.balanced_has_types = true;
                    }
                }
                Kind::Punct(Punct::LParen) => stack.push(Closer::Paren),
                Kind::Punct(Punct::LBracket) => stack.push(Closer::Bracket),
                Kind::Punct(Punct::LBrace) => stack.push(Closer::Brace),
                Kind::Punct(close @ (Punct::RParen | Punct::RBracket | Punct::RBrace)) => {
                    let want = match close {
                        Punct::RParen => Closer::Paren,
                        Punct::RBracket => Closer::Bracket,
                        _ => Closer::Brace,
                    };
                    match stack.last() {
                        Some(&top) if top == want => {
                            stack.pop();
                        }
                        Some(Closer::Template) if close == Punct::RBrace => {
                            // The `}` of a substitution: the template carries on after it.
                            self.tok = self.lexer.rescan_template_continuation(tok)?;
                            if self.tok.kind == Kind::TemplateTail {
                                stack.pop();
                            }
                            after_value = true;
                            if stack.is_empty() {
                                return self.next();
                            }
                            self.next()?;
                            continue;
                        }
                        _ => return Err(self.unexpected()),
                    }
                    if stack.is_empty() {
                        return self.next();
                    }
                }
                Kind::TemplateHead => stack.push(Closer::Template),
                Kind::Punct(Punct::Slash | Punct::SlashEq) if !after_value => {
                    self.tok = self.lexer.rescan_regex(tok)?;
                }
                _ => {}
            }
            after_value = matches!(
                self.tok.kind,
                Kind::Ident
                    | Kind::Number
                    | Kind::BigInt
                    | Kind::String
                    | Kind::Regex
                    | Kind::NoSubTemplate
                    | Kind::TemplateTail
                    | Kind::Keyword(Kw::This | Kw::Super | Kw::Null | Kw::True | Kw::False)
                    | Kind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace)
            );
            self.next()?;
        }
    }
}

// ---- declarations and expressions ---------------------------------------------------------

impl<'a> Parser<'a> {
    /// An empty statement standing where TypeScript-only text was.
    pub(super) fn ts_empty_statement(&mut self, start: u32) -> NodeId {
        self.ts_erase_from(start);
        self.ast.alloc(NodeKind::Empty, Span { start, end: self.prev_end }, 0)
    }

    /// After a type annotation's `?` or `:` on a parameter or class property.
    pub(super) fn ts_optional_marker(&mut self) -> PResult<()> {
        if self.at(Punct::Question) {
            let start = self.tok.start;
            self.next()?;
            self.ts_erase_from(start);
        }
        Ok(())
    }

    /// `x!` after a declared name: a definite assignment assertion.
    pub(super) fn ts_definite_marker(&mut self) -> PResult<()> {
        if self.at(Punct::Bang) && !self.tok.newline_before {
            let start = self.tok.start;
            self.next()?;
            self.ts_erase_from(start);
        }
        Ok(())
    }

    /// Whether `<T>(...) => ` or `(...) => ` or `(...): T =>` begins at the cursor.
    pub(super) fn ts_arrow_ahead(&mut self, generic: bool) -> bool {
        let in_consequent = self.in_consequent;
        self.ts_look(|p| {
            if generic {
                p.ts_in_type(|p| p.ts_type_params())?;
            }
            if !p.at(Punct::LParen) {
                return Ok(false);
            }
            p.ts_balanced()?;
            let typed = p.balanced_has_types;
            if p.tok.newline_before {
                return Ok(false);
            }
            match p.tok.kind {
                Kind::Punct(Punct::Arrow) => Ok(true),
                // In `a ? (b) : c => d` the colon belongs to the conditional, unless the
                // parentheses hold annotations and so cannot be an expression.
                Kind::Punct(Punct::Colon) if !in_consequent || typed => {
                    p.ts_in_type(|p| {
                        p.next()?;
                        p.ts_return_type()
                    })?;
                    Ok(p.at(Punct::Arrow) && !p.tok.newline_before)
                }
                _ => Ok(false),
            }
        })
        .unwrap_or(false)
    }

    /// An arrow function whose parameters carry TypeScript syntax. The cursor is on `(` or
    /// the `<` of its type parameters. Unlike the JavaScript path, the parameters are read as
    /// parameters from the start.
    pub(super) fn parse_ts_arrow(&mut self, start: u32, is_async: bool) -> PResult<NodeId> {
        let (scope_mark, ref_mark) = (self.sb.scope_count(), self.sb.ref_count());
        if self.at(Punct::Lt) {
            self.ts_type_params_erase()?;
        }
        self.push_arrow_scope(scope_mark, ref_mark);
        let params = self.parse_formal_params()?;
        if self.at(Punct::Colon) {
            self.ts_return_annotation()?;
        }
        if !self.at(Punct::Arrow) || self.tok.newline_before {
            return Err(self.unexpected_expecting("=>"));
        }
        self.parse_arrow_body(start, params, is_async)
    }

    /// Cursor on `<` after an expression. Skips type arguments if they are what this is:
    /// `f<T>(x)`, `new C<T>`, `` tag<T>`x` ``, or an instantiation expression `f<T>;`.
    pub(super) fn ts_try_type_args_expr(&mut self) -> bool {
        self.ts_try(|p| {
            let start = p.tok.start;
            p.ts_in_type(|p| p.ts_type_args())?;
            let follows = match p.tok.kind {
                Kind::Punct(Punct::LParen) | Kind::NoSubTemplate | Kind::TemplateHead => true,
                Kind::Punct(Punct::Lt | Punct::Gt | Punct::Plus | Punct::Minus) => false,
                _ => p.tok.newline_before || p.binary_op().is_some() || !p.ts_starts_expression(),
            };
            if follows {
                p.ts_erase_from(start);
            }
            Ok(follows)
        })
    }

    /// Whether the current token could begin an expression.
    fn ts_starts_expression(&self) -> bool {
        match self.tok.kind {
            Kind::Eof => false,
            Kind::Ident
            | Kind::Number
            | Kind::BigInt
            | Kind::String
            | Kind::NoSubTemplate
            | Kind::TemplateHead
            | Kind::PrivateName
            | Kind::Regex => true,
            Kind::Keyword(kw) => !matches!(kw, Kw::In | Kw::Instanceof | Kw::Extends),
            Kind::Punct(p) => matches!(
                p,
                Punct::LParen | Punct::LBracket | Punct::LBrace | Punct::Lt | Punct::Plus | Punct::Minus | Punct::Bang | Punct::Tilde | Punct::PlusPlus | Punct::MinusMinus | Punct::Slash | Punct::At
            ),
            _ => false,
        }
    }

    /// `x as T` and `x satisfies T` after an operand. The cursor is on `as` or `satisfies`.
    pub(super) fn ts_cast(&mut self) -> PResult<()> {
        let start = self.tok.start;
        self.ts_in_type(|p| {
            p.next()?;
            p.ts_type()
        })?;
        self.ts_erase_from(start);
        Ok(())
    }

    /// Cursor on `<` at the start of an operand in a `.ts` file: `<T>(x) => x` or `<T>expr`.
    pub(super) fn ts_angle_prefix(&mut self) -> PResult<Option<NodeId>> {
        let start = self.tok.start;
        if self.ts_arrow_ahead(true) {
            return self.parse_ts_arrow(start, false).map(Some);
        }
        // A type assertion: the type is dropped and the operand stays.
        self.ts_type_args_erase()?;
        Ok(None)
    }

    // ---- statements ---------------------------------------------------------------------

    /// A statement that starts with a TypeScript word: `type`, `interface`, `declare`,
    /// `abstract class`. Returns `None` when the word is just an identifier here.
    pub(super) fn ts_statement(&mut self) -> PResult<Option<NodeId>> {
        if self.tok.kind != Kind::Ident || self.tok.escaped {
            return Ok(None);
        }
        let start = self.tok.start;
        let word = self.text(self.tok);
        let next = self.peek()?;
        let same_line = !next.newline_before;
        match word {
            "type" if same_line && next.kind == Kind::Ident => {
                self.ts_in_type(|p| {
                    p.next()?; // type
                    p.next()?; // name
                    if p.at(Punct::Lt) {
                        p.ts_type_params()?;
                    }
                    p.expect(Punct::Eq)?;
                    p.ts_type()
                })?;
                self.semicolon()?;
                Ok(Some(self.ts_empty_statement(start)))
            }
            "interface" if same_line && next.kind == Kind::Ident => {
                self.ts_interface()?;
                Ok(Some(self.ts_empty_statement(start)))
            }
            "declare" if same_line && matches!(next.kind, Kind::Ident | Kind::Keyword(_)) => {
                self.next()?;
                self.ts_declared()?;
                Ok(Some(self.ts_empty_statement(start)))
            }
            "namespace" | "module" if same_line && next.kind == Kind::Ident => self.parse_ts_namespace(start).map(Some),
            "abstract" if same_line && next.kind == Kind::Keyword(Kw::Class) => {
                self.next()?;
                self.ts_erase_from(start);
                let class_start = self.tok.start;
                self.parse_class(class_start, true, true).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// `interface Name<T> extends A, B<C> { ... }` with the cursor on `interface`.
    fn ts_interface(&mut self) -> PResult<()> {
        self.ts_in_type(|p| {
            p.next()?; // interface
            p.next()?; // name
            if p.at(Punct::Lt) {
                p.ts_type_params()?;
            }
            if p.at_kw(Kw::Extends) {
                p.next()?;
                loop {
                    p.ts_type()?;
                    if !p.eat(Punct::Comma)? {
                        break;
                    }
                }
            }
            if !p.at(Punct::LBrace) {
                return Err(p.unexpected_expecting("{"));
            }
            p.ts_balanced()
        })
    }

    /// The rest of a `declare ...` statement, with the cursor past `declare`. Nothing it names
    /// exists at run time, so none of it is bound or referenced.
    pub(super) fn ts_declared(&mut self) -> PResult<()> {
        self.ts_in_type(|p| p.ts_declared_inner())?;
        Ok(())
    }

    fn ts_declared_inner(&mut self) -> PResult<()> {
        let word = if self.tok.kind == Kind::Ident { self.text(self.tok) } else { "" };
        let const_enum = self.at_kw(Kw::Const) && self.peek()?.kind == Kind::Keyword(Kw::Enum);
        match self.tok.kind {
            Kind::Keyword(Kw::Function) => {
                self.next()?;
                self.eat(Punct::Star)?;
                if self.tok.kind == Kind::Ident {
                    self.next()?; // the name; `export default function (...)` has none
                }
                self.ts_signature_tail()
            }
            Kind::Keyword(Kw::Class) => self.ts_class_like(),
            Kind::Keyword(Kw::Const) if const_enum => {
                self.next()?;
                self.ts_class_like()
            }
            Kind::Keyword(Kw::Enum) => self.ts_class_like(),
            Kind::Keyword(Kw::Var | Kw::Const) => self.ts_declared_variables(),
            Kind::Ident => match word {
                "let" => self.ts_declared_variables(),
                "abstract" => {
                    self.next()?;
                    self.ts_class_like()
                }
                "namespace" | "module" | "global" => self.ts_class_like(),
                "type" => {
                    self.next()?;
                    self.next()?;
                    if self.at(Punct::Lt) {
                        self.ts_type_params()?;
                    }
                    self.expect(Punct::Eq)?;
                    self.ts_type()?;
                    self.semicolon()
                }
                "interface" => self.ts_interface(),
                "async" => Err(self.unexpected()),
                _ => Err(self.unexpected()),
            },
            _ => Err(self.unexpected()),
        }
    }

    /// `<T>(params): ret;` after a declared function's name.
    fn ts_signature_tail(&mut self) -> PResult<()> {
        if self.at(Punct::Lt) {
            self.ts_type_params()?;
        }
        if self.at(Punct::LParen) {
            self.ts_balanced()?;
        }
        if self.at(Punct::Colon) {
            self.next()?;
            self.ts_return_type()?;
        }
        self.semicolon()
    }

    /// A header followed by a braced body: `class A<T> extends B implements C { }`,
    /// `enum E { }`, `namespace N.M { }`, `module "m" { }`, `global { }`. The cursor is on the
    /// keyword. A module declaration may also end in `;` with no body.
    fn ts_class_like(&mut self) -> PResult<()> {
        self.next()?; // class, enum, namespace, module, global
        let mut depth_ok = true;
        // Past `extends` or `implements`, a `<` opens type arguments; before, type parameters.
        let mut heritage = false;
        while depth_ok {
            match self.tok.kind {
                Kind::Punct(Punct::LBrace) => {
                    self.ts_balanced()?;
                    return Ok(());
                }
                Kind::Punct(Punct::Semi) => {
                    self.next()?;
                    return Ok(());
                }
                Kind::Eof => return Err(self.unexpected()),
                // Type arguments in a heritage clause may contain braces of their own.
                Kind::Punct(Punct::Lt) => {
                    if heritage {
                        self.ts_type_args()?;
                    } else {
                        self.ts_type_params()?;
                    }
                }
                _ => {
                    if self.at_kw(Kw::Extends) || self.at_word("implements") {
                        heritage = true;
                    }
                    if self.tok.newline_before && matches!(self.tok.kind, Kind::Ident | Kind::Keyword(_)) && self.ts_header_over() {
                        return Ok(());
                    }
                    self.next()?;
                }
            }
            depth_ok = !matches!(self.tok.kind, Kind::Eof);
        }
        Ok(())
    }

    /// A `module "m";` shorthand has no braces, so a header that runs onto a line that cannot
    /// continue it is over.
    fn ts_header_over(&self) -> bool {
        !matches!(self.text(self.tok), "extends" | "implements")
    }

    /// `const a: T, b = 1;` or `let [x]: T;` after `declare`.
    fn ts_declared_variables(&mut self) -> PResult<()> {
        self.next()?; // var, let, const
        loop {
            match self.tok.kind {
                Kind::Punct(Punct::LBrace | Punct::LBracket) => self.ts_balanced()?,
                _ => self.next()?,
            }
            if self.at(Punct::Colon) {
                self.next()?;
                self.ts_type()?;
            }
            if self.at(Punct::Eq) {
                self.next()?;
                // `declare const x = 1;` or `= Enum.Member`: a literal or a name, no calls.
                loop {
                    match self.tok.kind {
                        Kind::Eof | Kind::Punct(Punct::Comma | Punct::Semi) => break,
                        Kind::Punct(Punct::LBrace | Punct::LBracket | Punct::LParen) => self.ts_balanced()?,
                        _ => self.next()?,
                    }
                    if self.tok.newline_before {
                        break;
                    }
                }
            }
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        self.semicolon()
    }
}

// ---- parameters and class members -----------------------------------------------------------

impl<'a> Parser<'a> {
    /// At the start of a parameter: modifiers, decorators and `this`. Returns `true` if the
    /// whole parameter was a `this` parameter and has been erased.
    pub(super) fn ts_param_prefix(&mut self) -> PResult<bool> {
        let start = self.tok.start;
        if self.at_kw(Kw::This) {
            let next = self.peek()?;
            if next.kind == Kind::Punct(Punct::Colon) || next.kind == Kind::Punct(Punct::Comma) || next.kind == Kind::Punct(Punct::RParen) {
                self.next()?;
                if self.at(Punct::Colon) {
                    self.ts_annotation()?;
                }
                self.eat(Punct::Comma)?;
                self.ts_erase_from(start);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A parameter with TypeScript syntax: modifiers, `?`, a type, a default.
    pub(super) fn parse_ts_param(&mut self) -> PResult<NodeId> {
        let decorators = if self.at(Punct::At) { self.parse_decorators()? } else { Vec::new() };
        let param = self.parse_ts_param_inner()?;
        self.ast.add_decorators(param, decorators);
        Ok(param)
    }

    fn parse_ts_param_inner(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        // `public`, `private`, `protected` and `readonly` make a parameter property;
        // `override` alone does not.
        let mut makes_property = false;
        while self.tok.kind == Kind::Ident && !self.tok.escaped && matches!(self.text(self.tok), "public" | "private" | "protected" | "readonly" | "override") {
            let next = self.peek()?;
            if !(matches!(next.kind, Kind::Ident | Kind::Punct(Punct::LBrace | Punct::LBracket)) || next.kind == Kind::Keyword(Kw::This)) {
                break;
            }
            makes_property |= self.text(self.tok) != "override";
            let word_start = self.tok.start;
            self.next()?;
            self.ts_erase_from(word_start);
        }
        let target = self.parse_binding_target()?;
        if makes_property {
            if !matches!(self.ast[target].kind, NodeKind::BindingIdent(_)) {
                return Err(self.error_at("A parameter property may not be a destructuring pattern", self.ast[target].span.start));
            }
            self.param_props_acc.push(target);
        }
        self.ts_optional_marker()?;
        if self.at(Punct::Colon) {
            if self.meta && self.in_member {
                let meta = self.ts_annotation_meta()?;
                self.ast.set_type_meta(target, meta);
            } else {
                self.ts_annotation()?;
            }
        }
        if !self.eat(Punct::Eq)? {
            return Ok(target);
        }
        // The default is an ordinary expression: nothing in it declares into this pattern.
        let ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
        let default = self.allow_in(|p| p.parse_assign());
        self.binding_ctx = ctx;
        let default = default?;
        Ok(self.finish(NodeKind::AssignPattern { target, default }, start))
    }
}

impl<'a> Parser<'a> {
    /// A TypeScript modifier in front of a class member: consumed and erased. Returns the
    /// modifier's name. A word is only a modifier if a member name follows it.
    pub(super) fn ts_member_modifier(&mut self) -> PResult<Option<&'a str>> {
        if self.tok.kind != Kind::Ident || self.tok.escaped {
            return Ok(None);
        }
        let word = self.text(self.tok);
        if !matches!(word, "public" | "private" | "protected" | "readonly" | "abstract" | "override" | "declare") {
            return Ok(None);
        }
        let next = self.peek()?;
        let name_follows = matches!(
            next.kind,
            Kind::Ident | Kind::Keyword(_) | Kind::String | Kind::Number | Kind::BigInt | Kind::PrivateName | Kind::Punct(Punct::LBracket | Punct::Star)
        );
        if !name_follows {
            return Ok(None);
        }
        let start = self.tok.start;
        self.next()?;
        self.ts_erase_from(start);
        Ok(Some(word))
    }

    /// Cursor on `[`: whether this is an index signature, `[key: string]: T`.
    pub(super) fn ts_index_signature_ahead(&mut self) -> bool {
        self.ts_look(|p| {
            p.next()?;
            if p.tok.kind != Kind::Ident {
                return Ok(false);
            }
            p.next()?;
            Ok(p.at(Punct::Colon))
        })
        .unwrap_or(false)
    }
}

impl<'a> Parser<'a> {
    /// After `import`: whether the word `type` that follows makes this a type-only import.
    /// It does if a binding follows it, and `from` alone is the default import's name.
    /// Consumes the `type` when it does.
    pub(super) fn ts_import_type_keyword(&mut self) -> PResult<bool> {
        if !self.at_word("type") {
            return Ok(false);
        }
        let next = self.peek()?;
        let type_only = match next.kind {
            Kind::Punct(Punct::LBrace | Punct::Star) => true,
            Kind::Ident => self.text(next) != "from",
            _ => false,
        };
        if type_only {
            self.next()?;
        }
        Ok(type_only)
    }

    /// Cursor on `type` inside `{ ... }`: whether it modifies the specifier after it, as in
    /// `{ type A }` and `{ type A as B }`, rather than being a name.
    pub(super) fn ts_type_specifier(&mut self) -> PResult<bool> {
        let next = self.peek()?;
        Ok(match next.kind {
            Kind::Ident if self.text(next) == "as" => {
                // `type as as x` has the modifier; `type as x` imports `type` under another name.
                self.ts_look(|p| {
                    p.next()?;
                    p.next()?;
                    Ok(p.at_word("as"))
                })
                .unwrap_or(false)
            }
            Kind::Ident | Kind::String | Kind::Keyword(_) => true,
            _ => false,
        })
    }
}

// ---- declaration files ----------------------------------------------------------------------

impl<'a> Parser<'a> {
    /// A `.d.ts` file: nothing in it runs, so every item is skipped.
    pub(super) fn ts_ambient_items(&mut self) -> PResult<()> {
        while self.tok.kind != Kind::Eof {
            self.ts_ambient_item()?;
        }
        Ok(())
    }

    fn ts_ambient_item(&mut self) -> PResult<()> {
        match self.tok.kind {
            Kind::Punct(Punct::Semi) => self.next(),
            Kind::Keyword(Kw::Import) => self.ts_ambient_import(),
            Kind::Keyword(Kw::Export) => {
                self.next()?;
                self.ts_ambient_export()
            }
            Kind::String => self.ts_skip_to_statement_end(),
            _ => {
                if self.at_word("declare") {
                    self.next()?;
                }
                if self.ts_starts_declaration() {
                    self.ts_declared()
                } else {
                    // Not a declaration (a stray expression): the compiler parses and ignores it.
                    self.ts_skip_to_statement_end()
                }
            }
        }
    }

    fn ts_starts_declaration(&self) -> bool {
        match self.tok.kind {
            Kind::Keyword(Kw::Function | Kw::Class | Kw::Var | Kw::Const | Kw::Enum) => true,
            Kind::Ident => matches!(
                self.text(self.tok),
                "let" | "abstract" | "namespace" | "module" | "global" | "type" | "interface"
            ),
            _ => false,
        }
    }

    /// `import ... from "m"`, `import x = require("m")`, `import x = A.B`.
    fn ts_ambient_import(&mut self) -> PResult<()> {
        self.next()?;
        let mut done = false;
        loop {
            match self.tok.kind {
                Kind::Eof => return Ok(()),
                Kind::Punct(Punct::Semi) => return self.next(),
                Kind::Punct(Punct::LBrace) => self.ts_balanced()?,
                Kind::Punct(Punct::LParen) => {
                    self.ts_balanced()?;
                    done = true;
                }
                Kind::String => {
                    self.next()?;
                    done = true;
                    if self.at_kw(Kw::With) {
                        self.next()?;
                        self.ts_balanced()?;
                    }
                }
                _ => {
                    if done && self.tok.newline_before {
                        return Ok(());
                    }
                    self.next()?;
                    // `import a = B.C` ends after its last name.
                    if matches!(self.tok.kind, Kind::Ident) && self.tok.newline_before {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// After `export` in a declaration file.
    fn ts_ambient_export(&mut self) -> PResult<()> {
        let type_exports = self.at_word("type") && matches!(self.peek()?.kind, Kind::Punct(Punct::LBrace | Punct::Star));
        match self.tok.kind {
            Kind::Punct(Punct::Eq) => {
                // `export = name;`
                self.next()?;
                self.ts_skip_to_statement_end()
            }
            Kind::Punct(Punct::Star) => {
                self.next()?;
                if self.at_word("as") {
                    self.next()?;
                    self.next()?;
                }
                if self.at_word("from") {
                    self.next()?;
                    self.next()?;
                    if self.at_kw(Kw::With) {
                        self.next()?;
                        self.ts_balanced()?;
                    }
                }
                self.semicolon()
            }
            Kind::Punct(Punct::LBrace) => {
                self.ts_balanced()?;
                if self.at_word("from") {
                    self.next()?;
                    self.next()?;
                    if self.at_kw(Kw::With) {
                        self.next()?;
                        self.ts_balanced()?;
                    }
                }
                self.semicolon()
            }
            Kind::Keyword(Kw::Default) => {
                self.next()?;
                match self.tok.kind {
                    Kind::Keyword(Kw::Class | Kw::Function) => self.ts_declared(),
                    Kind::Ident if matches!(self.text(self.tok), "abstract" | "interface") => self.ts_declared(),
                    _ => self.ts_skip_to_statement_end(),
                }
            }
            Kind::Keyword(Kw::Import) => self.ts_ambient_import(),
            Kind::Ident if type_exports => {
                self.next()?;
                self.ts_ambient_export()
            }
            Kind::Ident if self.at_word("as") => {
                // `export as namespace Name;`
                self.next()?;
                self.next()?;
                self.next()?;
                self.semicolon()
            }
            _ => {
                if self.at_word("declare") {
                    self.next()?;
                }
                self.ts_declared()
            }
        }
    }

    /// Steps over an expression-ish tail up to its `;` or the end of its line.
    fn ts_skip_to_statement_end(&mut self) -> PResult<()> {
        loop {
            match self.tok.kind {
                Kind::Eof => return Ok(()),
                Kind::Punct(Punct::Semi) => return self.next(),
                Kind::Punct(Punct::LBrace | Punct::LParen | Punct::LBracket) => self.ts_balanced()?,
                _ => {
                    self.next()?;
                    if self.tok.newline_before {
                        return Ok(());
                    }
                }
            }
        }
    }
}

// ---- enums, namespaces and CommonJS-style imports -------------------------------------------

impl<'a> Parser<'a> {
    /// Cursor on `enum`. `start` is where the declaration begins (before `const`).
    pub(super) fn parse_ts_enum(&mut self, start: u32, is_const: bool) -> PResult<NodeId> {
        self.next()?;
        let name = self.parse_binding_ident_plain()?;
        let NodeKind::BindingIdent(atom) = self.ast[name].kind else { unreachable!() };
        // An enum is a variable; two enums of one name merge.
        self.sb.declare_namespace(atom, name).map_err(|r| self.redeclared(r))?;
        self.expect(Punct::LBrace)?;
        // Inside the body the members are visible by their bare names.
        self.push_scope(ScopeKind::Block);
        let mark = self.mark();
        while !self.at(Punct::RBrace) {
            let tok = self.tok;
            let member_name = match tok.kind {
                Kind::Ident | Kind::Keyword(_) => {
                    let atom = self.name_of(tok);
                    self.next()?;
                    self.finish(NodeKind::BindingIdent(atom), tok.start)
                }
                Kind::String => {
                    let atom = self.string_atom(tok)?;
                    self.next()?;
                    self.finish(NodeKind::Str(atom), tok.start)
                }
                _ => return Err(self.unexpected()),
            };
            let init = if self.eat(Punct::Eq)? { Some(self.allow_in(|p| p.parse_assign())?) } else { None };
            // The member becomes visible to the initializers after it, not its own.
            if let NodeKind::BindingIdent(atom) = self.ast[member_name].kind {
                self.sb.declare_lexical(atom, SymbolKind::Const, member_name, true).map_err(|r| self.redeclared(r))?;
            }
            let member = self.finish(NodeKind::TsEnumMember { name: member_name, init }, tok.start);
            self.push(member);
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        self.expect(Punct::RBrace)?;
        let members = self.take_list(mark);
        let flag = if is_const { flags::TS_CONST } else { 0 };
        let node = self.finish_with(NodeKind::TsEnum { name, members }, start, flag);
        self.sb.pop(node);
        Ok(node)
    }

    /// Cursor on `namespace` or `module`. `A.B.C` is a namespace `A` that exports `B`, ...
    /// A namespace of only types binds nothing, so a class of the same name can follow it.
    pub(super) fn parse_ts_namespace(&mut self, start: u32) -> PResult<NodeId> {
        self.next()?;
        let mut names = Vec::new();
        loop {
            let name = self.parse_binding_ident_plain()?;
            names.push(name);
            if !self.eat(Punct::Dot)? {
                break;
            }
        }
        // One function scope per segment.
        for _ in &names {
            self.push_scope(ScopeKind::Function);
        }
        let body = self.parse_ts_namespace_body()?;
        // Close from the innermost segment out, declaring each name in the scope it lives in
        // once it is known to have a value.
        let mut node = self.finish(NodeKind::TsNamespace { name: *names.last().unwrap(), body }, start);
        self.sb.pop(node);
        for index in (0..names.len()).rev() {
            if self.ast.ts_namespace_has_values(node) {
                let name = names[index];
                let NodeKind::BindingIdent(atom) = self.ast[name].kind else { unreachable!() };
                self.sb.declare_namespace(atom, name).map_err(|r| self.redeclared(r))?;
            }
            if index > 0 {
                let exported = self.finish(NodeKind::TsExport(node), start);
                let list = self.ast.alloc_list(&[exported]);
                node = self.finish(NodeKind::TsNamespace { name: names[index - 1], body: list }, start);
                self.sb.pop(node);
            }
        }
        Ok(node)
    }

    fn parse_ts_namespace_body(&mut self) -> PResult<List> {
        self.expect(Punct::LBrace)?;
        let mark = self.mark();
        while !self.at(Punct::RBrace) {
            if self.tok.kind == Kind::Eof {
                return Err(self.unexpected());
            }
            let item_start = self.tok.start;
            if self.at_kw(Kw::Export) {
                self.next()?;
                let decl = self.parse_statement_list_item()?;
                if matches!(self.ast[decl].kind, NodeKind::Empty) {
                    // Types only: nothing is left of it.
                    self.ts_erase_from(item_start);
                    continue;
                }
                let exported = self.finish(NodeKind::TsExport(decl), item_start);
                self.push(exported);
            } else {
                let stmt = self.parse_statement_list_item()?;
                if !matches!(self.ast[stmt].kind, NodeKind::Empty) {
                    self.push(stmt);
                }
            }
        }
        self.expect(Punct::RBrace)?;
        Ok(self.take_list(mark))
    }

    /// Cursor on the name after `import`: whether `import x = ...` follows.
    pub(super) fn ts_import_equals_ahead(&mut self) -> PResult<bool> {
        if self.tok.kind != Kind::Ident {
            return Ok(false);
        }
        let next = self.peek()?;
        if next.kind == Kind::Punct(Punct::Eq) {
            return Ok(true);
        }
        // `import type x = require("m")`
        Ok(self.at_word("type") && next.kind == Kind::Ident && self.ts_look(|p| {
            p.next()?;
            p.next()?;
            Ok(p.at(Punct::Eq))
        }) == Some(true))
    }

    /// `import x = require("m");` and `import x = A.B.C;`, with the cursor past `import`.
    /// They become a `const` or `var` declaration; the first is CommonJS.
    pub(super) fn parse_ts_import_equals(&mut self, start: u32) -> PResult<NodeId> {
        let type_only = self.at_word("type") && self.peek()?.kind == Kind::Ident;
        if type_only {
            self.next()?;
        }
        let name_tok = self.tok;
        let name = self.name_of(name_tok);
        self.next()?;
        self.expect(Punct::Eq)?;
        let target_ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
        let target = self.ast.alloc(NodeKind::BindingIdent(name), Span { start: name_tok.start, end: name_tok.end }, 0);
        self.binding_ctx = target_ctx;
        let requires = self.at_word("require") && self.peek()?.kind == Kind::Punct(Punct::LParen);
        let init = if requires {
            self.ast.ts_commonjs = true;
            self.parse_assign()?
        } else {
            self.parse_lhs()?
        };
        self.semicolon()?;
        let kind = if requires { VarKind::Const } else { VarKind::Var };
        if type_only {
            return Ok(self.ts_empty_statement(start));
        }
        // Declared after the initializer is read, so it cannot see itself.
        let strict = self.strict;
        if requires {
            self.sb.declare_lexical(name, SymbolKind::Const, target, strict).map_err(|r| self.redeclared(r))?;
        } else {
            self.sb.declare_var(name, SymbolKind::Var, target).map_err(|r| self.redeclared(r))?;
        }
        self.sb.add_write(target);
        let declarator = self.ast.alloc(NodeKind::VarDeclarator { target, init: Some(init) }, Span { start: name_tok.start, end: self.prev_end }, 0);
        let decls = self.ast.alloc_list(&[declarator]);
        self.ast.rewrite(start, name_tok.start, if requires { "const " } else { "var " });
        Ok(self.finish(NodeKind::VarDecl { kind, decls }, start))
    }
}

// ---- decorators ------------------------------------------------------------------------------

impl<'a> Parser<'a> {
    /// `@a @b(1) @c.d`, with the cursor on the first `@`.
    pub(super) fn parse_decorators(&mut self) -> PResult<Vec<crate::ast::Decorator>> {
        let mut out = Vec::new();
        while self.at(Punct::At) {
            let start = self.tok.start;
            self.next()?;
            // `@(expression)`, or a name with members and an optional call.
            let expr = self.parse_decorator_expression()?;
            out.push(crate::ast::Decorator { span: Span { start, end: self.prev_end }, expr });
        }
        Ok(out)
    }
}

// ---- decorator metadata -----------------------------------------------------------------------

impl<'a> Parser<'a> {
    /// Cursor on `:`. Skips and erases the annotation like [`Parser::ts_annotation`] and works
    /// out what the type is at run time, for `emitDecoratorMetadata`.
    pub(super) fn ts_annotation_meta(&mut self) -> PResult<crate::ast::TypeMeta> {
        self.ts_annotation_meta_for(false)
    }

    /// As [`Parser::ts_annotation_meta`]; for a rest parameter `...xs: T[]` the type is `T`.
    pub(super) fn ts_annotation_meta_for(&mut self, rest: bool) -> PResult<crate::ast::TypeMeta> {
        let colon = self.tok.start;
        let mut meta = crate::ast::TypeMeta::Object;
        self.ts_in_type(|p| {
            p.next()?;
            meta = p.ts_classify_type_with(rest)?;
            Ok(())
        })?;
        self.ts_erase_from(colon);
        Ok(meta)
    }

    /// With the cursor on the first token of a type, reads the whole type and returns how
    /// TypeScript serializes it. A reference to a name becomes a real reference in the tree,
    /// so the import it names is used and its binding resolved.
    pub(super) fn ts_classify_type_public(&mut self) -> PResult<crate::ast::TypeMeta> {
        self.ts_classify_type_with(false)
    }

    fn ts_classify_type_with(&mut self, rest: bool) -> PResult<crate::ast::TypeMeta> {
        use crate::ast::TypeMeta;
        let start = self.ts_mark();
        self.ts_return_type()?;
        let end = self.ts_mark();
        let limit = self.prev_end;
        // Read the tokens again, noting the ones outside any brackets.
        self.ts_reset(start);
        let mut tokens: Vec<(Token, usize)> = Vec::new();
        let mut depth = 0usize;
        while self.tok.start < limit && self.tok.kind != Kind::Eof {
            let tok = self.tok;
            match tok.kind {
                Kind::TemplateHead | Kind::NoSubTemplate => {
                    self.ts_reset(end);
                    return Ok(TypeMeta::String);
                }
                Kind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace | Punct::Lt) => {
                    tokens.push((tok, depth));
                    depth += 1;
                }
                Kind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::Gt) => {
                    depth = depth.saturating_sub(1);
                    tokens.push((tok, depth));
                }
                _ => tokens.push((tok, depth)),
            }
            self.next()?;
        }
        self.ts_reset(end);
        if rest && tokens.len() >= 3 {
            let n = tokens.len();
            if tokens[n - 1].0.kind == Kind::Punct(Punct::RBracket) && tokens[n - 2].0.kind == Kind::Punct(Punct::LBracket) && tokens[n - 2].1 == 0 {
                tokens.truncate(n - 2);
            }
        }
        // Alternatives of a union: `Foo | null` is `Foo`.
        let top: Vec<(Token, usize)> = tokens.iter().copied().collect();
        let mut parts: Vec<&[(Token, usize)]> = Vec::new();
        let mut from = 0;
        for (i, (tok, d)) in top.iter().enumerate() {
            if *d == 0 && tok.kind == Kind::Punct(Punct::Pipe) {
                if i > from {
                    parts.push(&top[from..i]);
                }
                from = i + 1;
            }
        }
        if from < top.len() {
            parts.push(&top[from..]);
        }
        let mut kinds: Vec<TypeMeta> = Vec::new();
        for part in &parts {
            let k = self.ts_classify_part(part);
            // Without strict null checks `null` and `undefined` are left out of a union.
            if k != TypeMeta::Void || parts.len() == 1 || self.strict_null_checks {
                kinds.push(k);
            }
        }
        let first = kinds.first().copied().unwrap_or(TypeMeta::Object);
        let same = kinds.iter().all(|k| *k == first);
        // References to different names are not "the same type".
        Ok(if same && !matches!(first, TypeMeta::Ref(_)) || kinds.len() == 1 { first } else { TypeMeta::Object })
    }

    fn ts_classify_part(&mut self, part: &[(Token, usize)]) -> crate::ast::TypeMeta {
        use crate::ast::TypeMeta;
        let Some(&(first, _)) = part.first() else { return TypeMeta::Object };
        let text = |t: Token| &self.src[t.start as usize..t.end as usize];
        // `x is T` is a boolean, `asserts x` is nothing.
        if part.len() >= 3 && part[1].0.kind == Kind::Ident && text(part[1].0) == "is" {
            return TypeMeta::Boolean;
        }
        if first.kind == Kind::Ident && text(first) == "asserts" && part.len() >= 2 {
            return TypeMeta::Void;
        }
        // `T[]`: brackets at the very end.
        if part.len() >= 3 {
            let n = part.len();
            if part[n - 1].0.kind == Kind::Punct(Punct::RBracket) && part[n - 2].0.kind == Kind::Punct(Punct::LBracket) && part[n - 2].1 == 0 {
                return TypeMeta::Array;
            }
        }
        match first.kind {
            Kind::Punct(Punct::LBracket) => TypeMeta::Array,
            Kind::Punct(Punct::LBrace) => TypeMeta::Object,
            Kind::Punct(Punct::LParen) => {
                // A function type is `( ... ) =>`; a parenthesized type is not worth following.
                let close = part.iter().position(|(t, d)| *d == 0 && t.kind == Kind::Punct(Punct::RParen));
                match close.and_then(|c| part.get(c + 1)) {
                    Some((t, _)) if t.kind == Kind::Punct(Punct::Arrow) => TypeMeta::Function,
                    _ => TypeMeta::Object,
                }
            }
            Kind::Punct(Punct::Lt) | Kind::Keyword(Kw::New) => TypeMeta::Function,
            Kind::Punct(Punct::Minus) => TypeMeta::Number,
            Kind::String => TypeMeta::String,
            Kind::Number => TypeMeta::Number,
            Kind::BigInt => TypeMeta::BigInt,
            Kind::Keyword(Kw::True | Kw::False) => TypeMeta::Boolean,
            Kind::Keyword(Kw::Void | Kw::Null) => TypeMeta::Void,
            Kind::Ident => {
                let name = text(first);
                if part.len() == 1 || part.get(1).is_some_and(|(t, _)| t.kind == Kind::Punct(Punct::Lt) || t.kind == Kind::Punct(Punct::Dot)) {
                    match name {
                        "string" => return TypeMeta::String,
                        "number" => return TypeMeta::Number,
                        "boolean" => return TypeMeta::Boolean,
                        "symbol" => return TypeMeta::Symbol,
                        "bigint" => return TypeMeta::BigInt,
                        "any" | "unknown" | "never" | "object" => return TypeMeta::Object,
                        "undefined" => return TypeMeta::Void,
                        "readonly" => {}
                        _ => {}
                    }
                }
                if name == "readonly" {
                    return self.ts_classify_part(&part[1..]);
                }
                self.ts_reference_meta(part)
            }
            _ => TypeMeta::Object,
        }
    }

    /// `Foo`, `Foo<T>` or `ns.Foo`: a reference to a value that may or may not exist.
    fn ts_reference_meta(&mut self, part: &[(Token, usize)]) -> crate::ast::TypeMeta {
        use crate::ast::TypeMeta;
        // Only a plain name, dotted names and type arguments make a reference.
        let mut i = 0;
        let mut node: Option<NodeId> = None;
        let mut first_start = 0;
        while i < part.len() {
            let (tok, depth) = part[i];
            if depth != 0 {
                return TypeMeta::Object;
            }
            match tok.kind {
                Kind::Ident | Kind::Keyword(_) if node.is_none() || part[i - 1].0.kind == Kind::Punct(Punct::Dot) => {
                    let name = self.name_of(tok);
                    match node {
                        None => {
                            first_start = tok.start;
                            let ident = self.ast.alloc(NodeKind::Ident(name), Span { start: tok.start, end: tok.end }, 0);
                            self.record_ref(ident);
                            node = Some(ident);
                        }
                        Some(object) => {
                            let prop = self.ast.alloc(NodeKind::PropName(name), Span { start: tok.start, end: tok.end }, 0);
                            node = Some(self.ast.alloc(NodeKind::Member { object, prop }, Span { start: first_start, end: tok.end }, 0));
                        }
                    }
                    i += 1;
                }
                Kind::Punct(Punct::Dot) => i += 1,
                // Type arguments end the name; they follow it and nothing else may.
                Kind::Punct(Punct::Lt) => {
                    let close = part.iter().rposition(|(t, d)| *d == 0 && t.kind == Kind::Punct(Punct::Gt));
                    if close == Some(part.len() - 1) {
                        break;
                    }
                    return TypeMeta::Object;
                }
                _ => return TypeMeta::Object,
            }
        }
        node.map_or(TypeMeta::Object, TypeMeta::Ref)
    }
}
