//! Recursive-descent parser for JavaScript modules and scripts.
//!
//! Arrow functions and destructuring assignment use the cover-grammar approach: the parser
//! reads `(a, b)` or `{ a, b }` as an expression first, then rewrites the nodes in place
//! into parameters or patterns if `=>` or `=` follows. No backtracking is needed.
//!
//! Syntax errors stop the parse at the first one; there is no recovery. Not every early
//! error from the specification is checked yet (see the list in `docs`/the README).

mod expr;
mod jsx;
mod pattern;
mod scope;
mod stmt;
mod ts;

use std::fmt;

use crate::ast::{Ast, List, NodeId, NodeKind, RESERVED_CANDIDATES, Span, flags, known};
use crate::lexer::{Kind, Kw, LexError, Lexer, Punct, Token};
use crate::literal;
use crate::semantic::{ScopeBuilder, Semantic};
use scope::BindingCtx;

#[derive(Clone, Copy, Debug)]
pub struct ParseOptions {
    /// Parse as an ES module: always strict, `await` is a keyword at the top level, and
    /// `import`/`export` are allowed. Otherwise parse as a script (CommonJS-style: sloppy
    /// unless a directive says otherwise, top-level `return` allowed).
    pub module: bool,
    /// Accept TypeScript syntax. Types are skipped, not represented: their text is recorded
    /// as erased spans for the bundler to drop.
    pub ts: bool,
    /// Accept JSX elements.
    pub jsx: bool,
    /// Record the declared types of class members and their parameters, for
    /// `emitDecoratorMetadata`.
    pub decorator_metadata: bool,
    /// Whether `null` and `undefined` take part in unions, which changes how a union is
    /// serialized in decorator metadata.
    pub strict_null_checks: bool,
    /// The file is a declaration file (`.d.ts`): every top-level item is a declaration of
    /// types, skipped without building anything.
    pub ambient: bool,
    /// Keep every token the parser consumed (see [`Parsed::tokens`]).
    pub record_tokens: bool,
    /// The text is a stretch of statements cut out of a larger file: names it exports need not
    /// be declared in it, and a `return` may stand at the top level.
    pub fragment: bool,
    /// Note where the comments are that output can do without (see [`Parsed::droppable`]).
    pub comments: bool,
}

impl ParseOptions {
    pub const fn new(module: bool) -> ParseOptions {
        ParseOptions { module, ts: false, jsx: false, decorator_metadata: false, strict_null_checks: false, ambient: false, record_tokens: false, fragment: false, comments: false }
    }

    pub const fn with_recorded_tokens(mut self) -> ParseOptions {
        self.record_tokens = true;
        self
    }

    pub const fn as_fragment(mut self) -> ParseOptions {
        self.fragment = true;
        self
    }

    pub const fn with_decorator_metadata(mut self) -> ParseOptions {
        self.decorator_metadata = true;
        self
    }

    pub const fn declaration_file(mut self) -> ParseOptions {
        self.ts = true;
        self.ambient = true;
        self
    }

    pub const fn typescript(mut self) -> ParseOptions {
        self.ts = true;
        self
    }

    pub const fn with_jsx(mut self) -> ParseOptions {
        self.jsx = true;
        self
    }
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions::new(true)
    }
}

/// A syntax error. Boxed so that `Result<NodeId, ParseError>` is one word wide: nearly every
/// parser function returns one, and errors are rare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(Box<ParseErrorData>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseErrorData {
    pub message: String,
    /// Byte offset the error is reported at.
    pub pos: u32,
}

impl ParseError {
    pub fn new(message: impl Into<String>, pos: u32) -> ParseError {
        ParseError(Box::new(ParseErrorData { message: message.into(), pos }))
    }
}

impl std::ops::Deref for ParseError {
    type Target = ParseErrorData;

    fn deref(&self) -> &ParseErrorData {
        &self.0
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.pos)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::new(e.message, e.pos)
    }
}

type PResult<T> = Result<T, ParseError>;

pub fn parse(src: &str, options: ParseOptions) -> PResult<Parsed> {
    let mut parser = Parser::new(src, options)?;
    parser.parse_program()?;
    let is_ts = parser.ts;
    let Parser { mut ast, sb, tokens, asi, eof_asi, lexer, .. } = parser;
    ast.shrink_to_fit();
    let semantic = sb.finish(&ast).map_err(|e| ParseError::new(e.message, e.pos))?;
    let mut legal = lexer.legal;
    legal.sort_unstable();
    legal.dedup();
    let mut droppable = lexer.droppable;
    droppable.sort_unstable();
    droppable.dedup();
    Ok(Parsed { ast, semantic, is_ts, tokens, asi, eof_asi, legal, droppable })
}

/// A parsed file: its syntax tree and what its identifiers mean.
pub struct Parsed {
    pub ast: Ast,
    pub semantic: Semantic,
    /// Parsed as TypeScript: imports that are never used as values can be dropped.
    pub is_ts: bool,
    /// Every token in the order the parser consumed it, in the form it was finally read
    /// (regular expressions and template parts included), when `record_tokens` was set.
    pub tokens: Vec<Token>,
    /// Indexes into `tokens` of those the parser inserted a semicolon before, because a line
    /// break ended the statement.
    pub asi: Vec<usize>,
    /// Where the legal comments are (`/*! */`, `//!`, `@license`, `@preserve`), in order and once each.
    pub legal: Vec<(u32, u32)>,
    /// Where the comments are that written output leaves out (not legal, not an annotation another tool reads), in order and once each.
    pub droppable: Vec<(u32, u32)>,
    /// The last statement ended at the end of the text with no `;`: whatever is written after
    /// this text needs one in between.
    pub eof_asi: bool,
}

/// Settings that change at function boundaries.
#[derive(Default)]
struct FnContext {
    in_function: bool,
    is_async: bool,
    is_generator: bool,
    in_loop: bool,
    in_switch: bool,
    labels: Vec<crate::ast::Atom>,
    /// `super.x` is allowed (methods, field initializers, static blocks, and arrows in them).
    super_prop: bool,
    /// `super()` is allowed (constructors of derived classes, and arrows in them).
    super_call: bool,
    /// `new.target` is allowed (any non-arrow function, and arrows in one).
    new_target: bool,
    /// Inside a class field initializer or static block, where `arguments` is forbidden.
    /// Arrows inherit this; ordinary functions do not.
    class_init: bool,
    /// Inside a class static block (or an arrow in one), where `await` is reserved.
    static_block: bool,
    /// Directly inside an arrow function. Together with `static_block` this tells a `return`
    /// that belongs to the static block itself (illegal) from one in a nested arrow (fine).
    arrow: bool,
    /// Reading a function's formal parameters, where `await`/`yield` expressions are illegal.
    in_params: bool,
    /// Reading the target of a `let` or `const`, where the name `let` is illegal.
    lexical: bool,
}

/// A construct that is only valid if the enclosing literal becomes a pattern.
type CoverError = (u32, &'static str);

const SHORTHAND_INIT: &str = "Invalid shorthand property initializer";
const DUPLICATE_PROTO: &str = "Duplicate `__proto__` fields are not allowed in object literals";

/// Private names (`#x`) seen in one class body.
#[derive(Default)]
struct PrivateScope {
    /// Declared names, with `PRIVATE_*` bits describing how.
    declared: Vec<(crate::ast::Atom, u8)>,
    /// References not yet matched to a declaration, with their positions.
    referenced: Vec<(crate::ast::Atom, u32)>,
}

const PRIVATE_GETTER: u8 = 1;
const PRIVATE_SETTER: u8 = 2;
const PRIVATE_OTHER: u8 = 4;
const PRIVATE_STATIC: u8 = 8;

struct Parser<'a> {
    src: &'a str,
    lexer: Lexer<'a>,
    tok: Token,
    prev_end: u32,
    ast: Ast,
    /// Shared stack for building child lists without one allocation per list.
    scratch: Vec<NodeId>,
    module: bool,
    strict: bool,
    cx: FnContext,
    /// `in` is not an operator here (the head of a `for` statement).
    no_in: bool,
    /// A `{ a = 1 }` shorthand default (or duplicate `__proto__`) that is only valid if the
    /// enclosing literal turns out to be a destructuring pattern.
    cover_init: Option<CoverError>,
    /// The class body being parsed has an `extends` clause.
    class_extends: bool,
    /// The class body being parsed already has a constructor.
    class_has_constructor: bool,
    private_scopes: Vec<PrivateScope>,
    /// Whether the function body that was parsed last began with a `"use strict"` directive.
    last_body_strict: bool,
    /// Scopes, symbols and references, built as parsing proceeds.
    sb: ScopeBuilder,
    /// What the binding identifiers parsed next declare.
    binding_ctx: BindingCtx,
    /// Names exported so far, to catch duplicates.
    exported: std::collections::HashSet<crate::ast::Atom>,
    /// TypeScript and JSX are accepted.
    ts: bool,
    jsx: bool,
    ambient: bool,
    /// Types of class members are recorded for decorator metadata.
    meta: bool,
    strict_null_checks: bool,
    /// Reading a class member (not a function nested in one), whose types are recorded.
    in_member: bool,
    /// The return type of the function just read, for decorator metadata.
    return_meta: Option<crate::ast::TypeMeta>,
    /// Reading a type: `>` stays a single token so `A<B<C>>` closes twice.
    in_type: bool,
    /// Reading the `extends` operand of a conditional type, where `?` ends the operand.
    no_conditional: bool,
    /// Reading the middle of `a ? b : c`, where `(x): T => y` is not taken as an arrow.
    in_consequent: bool,
    record_tokens: bool,
    tokens: Vec<Token>,
    /// Indexes into `tokens` of the tokens a semicolon was inserted before.
    asi: Vec<usize>,
    eof_asi: bool,
    fragment: bool,
    /// Names exported by `export namespace`, which may share the name with a value.
    exported_namespaces: std::collections::HashSet<crate::ast::Atom>,
    /// Set by `ts_balanced`: the parenthesized group it just skipped holds a type annotation
    /// or an optional-parameter mark at its top level, so it can only be a parameter list.
    balanced_has_types: bool,
    /// The function just read had no body: an overload signature.
    ts_overload: bool,
    /// Parameters with `public`/`private`/`protected`/`readonly` in the list being read.
    param_props_acc: Vec<NodeId>,
    /// What the last finished parameter list declared as properties.
    last_formal_props: Vec<NodeId>,
    /// Decorators read ahead of the class they belong to.
    pending_decorators: Vec<crate::ast::Decorator>,
    /// The subscripts about to be parsed belong to a decorator, which a `[` does not continue:
    /// `@dec [key]() {}` decorates a method with a computed name.
    decorator_subscripts: bool,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, options: ParseOptions) -> PResult<Self> {
        let mut lexer = Lexer::new(src)?.allow_html_comments(!options.module);
        lexer.record_comments = options.comments;
        let tok = lexer.next_token()?;
        let mut parser = Parser {
            src,
            lexer,
            tok,
            prev_end: 0,
            ast: Ast::new(src.len()),
            scratch: Vec::with_capacity(256),
            module: options.module,
            strict: options.module,
            // A script is treated as a CommonJS body, which is itself a function.
            cx: FnContext { new_target: !options.module, ..FnContext::default() },
            no_in: false,
            cover_init: None,
            class_extends: false,
            class_has_constructor: false,
            private_scopes: Vec::new(),
            last_body_strict: false,
            sb: {
                let mut sb = ScopeBuilder::new(options.module, src.len());
                if options.ts {
                    sb.allow_function_merging();
                }
                if options.fragment {
                    sb.allow_unbound_exports();
                }
                sb
            },
            binding_ctx: BindingCtx::None,
            exported: std::collections::HashSet::new(),
            ts: options.ts,
            jsx: options.jsx,
            ambient: options.ambient,
            meta: options.decorator_metadata,
            strict_null_checks: options.strict_null_checks,
            in_member: false,
            return_meta: None,
            in_type: false,
            no_conditional: false,
            in_consequent: false,
            record_tokens: options.record_tokens,
            tokens: Vec::new(),
            asi: Vec::new(),
            eof_asi: false,
            fragment: options.fragment,
            exported_namespaces: Default::default(),
            balanced_has_types: false,
            ts_overload: false,
            param_props_acc: Vec::new(),
            last_formal_props: Vec::new(),
            pending_decorators: Vec::new(),
            decorator_subscripts: false,
        };
        // A leading `>` can never matter, but keep the invariant that `tok` is never a bare Gt.
        parser.join_gt();
        Ok(parser)
    }

    // ---- token plumbing ---------------------------------------------------------------

    fn join_gt(&mut self) {
        if self.tok.is(Punct::Gt) {
            self.tok = self.lexer.rescan_gt(self.tok);
        }
    }

    fn next(&mut self) -> PResult<()> {
        if self.record_tokens {
            self.tokens.push(self.tok);
        }
        self.prev_end = self.tok.end;
        self.tok = self.lexer.next_token()?;
        if !self.in_type {
            self.join_gt();
        }
        Ok(())
    }

    /// The token after the current one, without consuming anything.
    fn peek(&mut self) -> PResult<Token> {
        let mark = self.lexer.pos();
        let result = self.lexer.next_token();
        self.lexer.reset(mark);
        Ok(result?)
    }

    fn text(&self, tok: Token) -> &'a str {
        &self.src[tok.start as usize..tok.end as usize]
    }

    fn at(&self, p: Punct) -> bool {
        self.tok.is(p)
    }

    #[inline]
    fn at_kw(&self, k: Kw) -> bool {
        matches!(self.tok.kind, Kind::Keyword(q) if q == k) && !self.tok.escaped
    }

    /// The current token is the unescaped contextual keyword `word` (`of`, `from`, `async`...).
    fn at_word(&self, word: &str) -> bool {
        self.tok.kind == Kind::Ident && !self.tok.escaped && self.text(self.tok) == word
    }

    fn eat(&mut self, p: Punct) -> PResult<bool> {
        if self.at(p) {
            self.next()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expect(&mut self, p: Punct) -> PResult<()> {
        if self.at(p) {
            self.next()
        } else {
            Err(self.unexpected_expecting(p.as_str()))
        }
    }

    fn expect_kw(&mut self, k: Kw) -> PResult<()> {
        if self.at_kw(k) {
            self.next()
        } else {
            Err(self.unexpected_expecting(k.as_str()))
        }
    }

    fn expect_word(&mut self, word: &str) -> PResult<()> {
        if self.at_word(word) {
            self.next()
        } else {
            Err(self.unexpected_expecting(word))
        }
    }

    fn error_at(&self, message: impl Into<String>, pos: u32) -> ParseError {
        ParseError::new(message, pos)
    }

    fn error(&self, message: impl Into<String>) -> ParseError {
        self.error_at(message, self.tok.start)
    }

    fn unexpected(&self) -> ParseError {
        self.error_at(self.describe_token(), self.tok.start)
    }

    fn unexpected_expecting(&self, wanted: &str) -> ParseError {
        self.error_at(format!("{}, expected `{wanted}`", self.describe_token()), self.tok.start)
    }

    fn describe_token(&self) -> String {
        match self.tok.kind {
            Kind::Eof => "Unexpected end of input".to_string(),
            _ => {
                let text = self.text(self.tok);
                if text.len() > 24 {
                    format!("Unexpected token `{}...`", &text[..text.floor_char_boundary(24)])
                } else {
                    format!("Unexpected token `{text}`")
                }
            }
        }
    }

    /// Statement terminator with automatic semicolon insertion.
    fn semicolon(&mut self) -> PResult<()> {
        if self.eat(Punct::Semi)? {
            return Ok(());
        }
        if self.at(Punct::RBrace) {
            return Ok(());
        }
        if self.tok.kind == Kind::Eof {
            self.eof_asi = true;
            return Ok(());
        }
        if self.tok.newline_before {
            if self.record_tokens {
                // The current token is the next one `next` will record.
                self.asi.push(self.tokens.len());
            }
            return Ok(());
        }
        Err(self.unexpected_expecting(";"))
    }

    // ---- node building ----------------------------------------------------------------

    /// Allocates a node spanning from `start` to the end of the last consumed token.
    fn finish(&mut self, kind: NodeKind, start: u32) -> NodeId {
        self.finish_with(kind, start, 0)
    }

    fn finish_with(&mut self, kind: NodeKind, start: u32, flags: u8) -> NodeId {
        let span = Span { start, end: self.prev_end };
        self.ast.alloc(kind, span, flags)
    }

    fn set_flag(&mut self, id: NodeId, flag: u8) {
        self.ast.node_mut(id).flags |= flag;
    }

    fn mark(&self) -> usize {
        self.scratch.len()
    }

    fn push(&mut self, id: NodeId) {
        self.scratch.push(id);
    }

    /// Moves everything pushed since `mark` into a list.
    fn take_list(&mut self, mark: usize) -> List {
        let list = self.ast.alloc_list(&self.scratch[mark..]);
        self.scratch.truncate(mark);
        list
    }

    fn atom(&mut self, text: &str) -> crate::ast::Atom {
        self.ast.intern(text)
    }

    /// The name an identifier token spells, with `\u` escapes decoded.
    fn name_of(&mut self, tok: Token) -> crate::ast::Atom {
        let text = self.text(tok);
        if tok.escaped {
            let decoded = literal::decode_identifier(text);
            self.ast.intern(&decoded)
        } else {
            self.ast.intern(text)
        }
    }

    /// Runs `f` with `in` allowed as an operator again (inside brackets, calls, etc.).
    fn allow_in<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let saved = std::mem::replace(&mut self.no_in, false);
        // Brackets also end the middle of a conditional: a `:` inside them is not its.
        let saved_consequent = std::mem::replace(&mut self.in_consequent, false);
        let result = f(self);
        self.no_in = saved;
        self.in_consequent = saved_consequent;
        result
    }

    // ---- program ----------------------------------------------------------------------

    fn parse_program(&mut self) -> PResult<()> {
        let mark = self.mark();
        if self.ambient {
            self.ts_ambient_items()?;
        } else {
            self.parse_directives_and_statements(true)?;
        }
        let list = self.take_list(mark);
        let root = self.ast.alloc(NodeKind::Program(list), Span { start: 0, end: self.src.len() as u32 }, 0);
        self.ast.root = root;
        self.sb.set_root_node(root);
        Ok(())
    }

    /// Parses statements up to `}` or end of input, honoring a directive prologue.
    fn parse_directives_and_statements(&mut self, top_level: bool) -> PResult<()> {
        let mut in_prologue = true;
        let mut found_strict = false;
        loop {
            if self.tok.kind == Kind::Eof || (!top_level && self.at(Punct::RBrace)) {
                // Set last so nested function bodies cannot clobber it.
                self.last_body_strict = found_strict;
                return Ok(());
            }
            let is_string = self.tok.kind == Kind::String;
            let raw = self.text(self.tok);
            let tok_span = (self.tok.start, self.tok.end);
            let stmt = if top_level { self.parse_module_item()? } else { self.parse_statement_list_item()? };
            if in_prologue && is_string {
                if self.is_directive(stmt, tok_span) {
                    self.set_flag(stmt, flags::DIRECTIVE);
                    if &raw[1..raw.len() - 1] == "use strict" {
                        self.strict = true;
                        found_strict = true;
                    }
                } else {
                    in_prologue = false;
                }
            } else {
                in_prologue = false;
            }
            self.push(stmt);
        }
    }

    /// A statement that is exactly one string literal.
    fn is_directive(&self, stmt: NodeId, tok_span: (u32, u32)) -> bool {
        let NodeKind::ExprStmt(expr) = self.ast[stmt].kind else { return false };
        let node = &self.ast[expr];
        matches!(node.kind, NodeKind::Str(_)) && !node.parenthesized() && (node.span.start, node.span.end) == tok_span
    }
}

#[cfg(test)]
mod tests;
