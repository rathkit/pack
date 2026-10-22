//! JavaScript lexer.
//!
//! The lexer is driven by the parser. A few tokens cannot be classified from the
//! characters alone, so the lexer always scans the simplest reading and the parser
//! asks for a rescan when its position in the grammar says otherwise:
//!
//! - `/` and `/=` are always division; call [`Lexer::rescan_regex`] where an
//!   expression may start.
//! - `}` closing a template substitution is always a brace; call
//!   [`Lexer::rescan_template_continuation`] to read the rest of the template.
//! - `>` is always a single `>`; call [`Lexer::rescan_gt`] to join it into `>=`,
//!   `>>`, `>>>` and their assignment forms. Keeping it single is what lets a
//!   TypeScript parser close nested generics like `A<B<C>>`.
//!
//! Tokens carry byte offsets into the source and never copy text.

use std::fmt;

use crate::unicode;

macro_rules! puncts {
    ($($name:ident = $text:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Punct { $($name),* }

        impl Punct {
            pub fn as_str(self) -> &'static str {
                match self { $(Punct::$name => $text),* }
            }
        }
    };
}

puncts! {
    LBrace = "{", RBrace = "}", LParen = "(", RParen = ")", LBracket = "[", RBracket = "]",
    Semi = ";", Comma = ",", Dot = ".", Ellipsis = "...", Colon = ":", At = "@", Arrow = "=>",
    Lt = "<", Gt = ">", LtEq = "<=", GtEq = ">=",
    EqEq = "==", NotEq = "!=", EqEqEq = "===", NotEqEq = "!==",
    Plus = "+", Minus = "-", Star = "*", Slash = "/", Percent = "%", StarStar = "**",
    PlusPlus = "++", MinusMinus = "--",
    LtLt = "<<", GtGt = ">>", GtGtGt = ">>>",
    Amp = "&", Pipe = "|", Caret = "^", Bang = "!", Tilde = "~",
    AmpAmp = "&&", PipePipe = "||", QuestionQuestion = "??", Question = "?", QuestionDot = "?.",
    Eq = "=", PlusEq = "+=", MinusEq = "-=", StarEq = "*=", SlashEq = "/=", PercentEq = "%=",
    StarStarEq = "**=", LtLtEq = "<<=", GtGtEq = ">>=", GtGtGtEq = ">>>=",
    AmpEq = "&=", PipeEq = "|=", CaretEq = "^=",
    AmpAmpEq = "&&=", PipePipeEq = "||=", QuestionQuestionEq = "??=",
}

macro_rules! keywords {
    ($($name:ident = $text:literal),* $(,)?) => {
        /// Reserved words in every context. Contextual words (`let`, `static`, `yield`,
        /// `await`, `async`, `of`, `get`, `set`, and the TypeScript ones) are plain identifiers.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Kw { $($name),* }

        impl Kw {
            pub const ALL: &'static [Kw] = &[$(Kw::$name),*];

            pub fn as_str(self) -> &'static str {
                match self { $(Kw::$name => $text),* }
            }
        }
    };
}

/// Recognizes a reserved word. A `match` over all of them compiles to a comparison against
/// each in turn (a library call per same-length candidate), and this runs for every
/// identifier, so dispatch on the first letter first.
fn keyword(word: &str) -> Option<Kw> {
    use Kw::*;
    let candidates: &[(&str, Kw)] = match *word.as_bytes().first()? {
        b'b' => &[("break", Break)],
        b'c' => &[("case", Case), ("catch", Catch), ("class", Class), ("const", Const), ("continue", Continue)],
        b'd' => &[("debugger", Debugger), ("default", Default), ("delete", Delete), ("do", Do)],
        b'e' => &[("else", Else), ("enum", Enum), ("export", Export), ("extends", Extends)],
        b'f' => &[("false", False), ("finally", Finally), ("for", For), ("function", Function)],
        b'i' => &[("if", If), ("import", Import), ("in", In), ("instanceof", Instanceof)],
        b'n' => &[("new", New), ("null", Null)],
        b'r' => &[("return", Return)],
        b's' => &[("super", Super), ("switch", Switch)],
        b't' => &[("this", This), ("throw", Throw), ("true", True), ("try", Try), ("typeof", Typeof)],
        b'v' => &[("var", Var), ("void", Void)],
        b'w' => &[("while", While), ("with", With)],
        _ => return None,
    };
    candidates.iter().find(|(text, _)| *text == word).map(|&(_, kw)| kw)
}

/// Bytes that can continue a plain ASCII identifier.
static IDENT_BYTE: [bool; 256] = {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 256 {
        table[b] = matches!(b as u8, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$');
        b += 1;
    }
    table
};

keywords! {
    Break = "break", Case = "case", Catch = "catch", Class = "class", Const = "const",
    Continue = "continue", Debugger = "debugger", Default = "default", Delete = "delete",
    Do = "do", Else = "else", Enum = "enum", Export = "export", Extends = "extends",
    False = "false", Finally = "finally", For = "for", Function = "function", If = "if",
    Import = "import", In = "in", Instanceof = "instanceof", New = "new", Null = "null",
    Return = "return", Super = "super", Switch = "switch", This = "this", Throw = "throw",
    True = "true", Try = "try", Typeof = "typeof", Var = "var", Void = "void",
    While = "while", With = "with",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Eof,
    Ident,
    Keyword(Kw),
    /// `#name`, including the `#`.
    PrivateName,
    Number,
    BigInt,
    String,
    /// `` `text` `` with no substitutions.
    NoSubTemplate,
    /// `` `text${ ``
    TemplateHead,
    /// `` }text${ `` (only from [`Lexer::rescan_template_continuation`])
    TemplateMiddle,
    /// `` }text` `` (only from [`Lexer::rescan_template_continuation`])
    TemplateTail,
    /// `/body/flags` (only from [`Lexer::rescan_regex`])
    Regex,
    Punct(Punct),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: u32,
    pub end: u32,
    /// A line terminator sits between the previous token and this one. Needed for
    /// automatic semicolon insertion and restricted productions.
    pub newline_before: bool,
    /// The identifier or keyword was written with a `\u` escape. An escaped keyword is
    /// reported as `Kind::Keyword` so the parser can reject it where a keyword is invalid.
    pub escaped: bool,
}

impl Token {
    #[inline]
    pub fn is(&self, p: Punct) -> bool {
        matches!(self.kind, Kind::Punct(q) if q == p)
    }
}

/// What went wrong and where. Boxed, so that `Result<Token, LexError>` stays small enough to
/// travel in registers on the (hot) success path; errors are rare and allocate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError(Box<LexErrorData>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexErrorData {
    pub message: &'static str,
    /// Byte offset the problem is reported at.
    pub pos: u32,
}

impl LexError {
    pub fn new(message: &'static str, pos: u32) -> LexError {
        LexError(Box::new(LexErrorData { message, pos }))
    }
}

impl std::ops::Deref for LexError {
    type Target = LexErrorData;

    fn deref(&self) -> &LexErrorData {
        &self.0
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.pos)
    }
}

impl std::error::Error for LexError {}

type Result<T> = std::result::Result<T, LexError>;

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    html_comments: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Result<Self> {
        if src.len() > u32::MAX as usize {
            return Err(LexError::new("source files larger than 4 GiB are not supported", 0));
        }
        let mut lexer = Lexer { src, bytes: src.as_bytes(), pos: 0, html_comments: false };
        // A hashbang is only a comment at the very start of the file.
        if lexer.bytes.starts_with(b"#!") {
            lexer.skip_line_comment_body();
        }
        Ok(lexer)
    }

    /// Treat `<!--` and a line-leading `-->` as comments (Annex B). Scripts allow them;
    /// modules do not.
    pub fn allow_html_comments(mut self, allow: bool) -> Self {
        self.html_comments = allow;
        self
    }

    pub fn text(&self, tok: Token) -> &'a str {
        &self.src[tok.start as usize..tok.end as usize]
    }

    /// Current byte offset. Together with [`Lexer::reset`] this gives the parser cheap
    /// backtracking for speculative parses (arrow functions, TypeScript generics).
    pub fn pos(&self) -> u32 {
        self.pos as u32
    }

    pub fn reset(&mut self, pos: u32) {
        self.pos = pos as usize;
    }

    pub fn next_token(&mut self) -> Result<Token> {
        let newline_before = self.skip_trivia()?;
        let start = self.pos;
        let mut escaped = false;
        let kind = match self.peek() {
            None => Kind::Eof,
            Some(b) => self.scan(b, &mut escaped)?,
        };
        Ok(Token { kind, start: start as u32, end: self.pos as u32, newline_before, escaped })
    }

    /// Reread a `/` or `/=` token as a regular expression literal.
    pub fn rescan_regex(&mut self, slash: Token) -> Result<Token> {
        debug_assert!(slash.is(Punct::Slash) || slash.is(Punct::SlashEq));
        let start = slash.start as usize;
        self.pos = start + 1;
        let mut in_class = false;
        loop {
            match self.peek() {
                None | Some(b'\n') | Some(b'\r') => return Err(self.err_at("unterminated regular expression", start)),
                Some(0xE2) if self.at_line_separator() => {
                    return Err(self.err_at("unterminated regular expression", start));
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        None | Some(b'\n') | Some(b'\r') => {
                            return Err(self.err_at("unterminated regular expression", start));
                        }
                        Some(_) => self.advance_char(),
                    }
                }
                Some(b'[') => {
                    in_class = true;
                    self.pos += 1;
                }
                Some(b']') => {
                    in_class = false;
                    self.pos += 1;
                }
                Some(b'/') if !in_class => {
                    self.pos += 1;
                    break;
                }
                Some(_) => self.pos += 1,
            }
        }
        // Flags are identifier characters; the parser validates which ones are real.
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' {
                self.pos += 1;
            } else if b >= 0x80 && is_id_continue(self.char_at(self.pos)) {
                self.advance_char();
            } else {
                break;
            }
        }
        Ok(Token { kind: Kind::Regex, end: self.pos as u32, ..slash })
    }

    /// Reread a `}` that closes a template substitution as the rest of the template.
    pub fn rescan_template_continuation(&mut self, rbrace: Token) -> Result<Token> {
        debug_assert!(rbrace.is(Punct::RBrace));
        self.pos = rbrace.start as usize + 1;
        let kind = self.scan_template_rest(false, rbrace.start as usize)?;
        Ok(Token { kind, end: self.pos as u32, ..rbrace })
    }

    /// Join a single `>` with what follows into `>=`, `>>`, `>>=`, `>>>` or `>>>=`.
    pub fn rescan_gt(&mut self, gt: Token) -> Token {
        debug_assert!(gt.is(Punct::Gt));
        self.pos = gt.start as usize + 1;
        let (punct, extra) = match (self.at(0), self.at(1), self.at(2)) {
            (b'>', b'>', b'=') => (Punct::GtGtGtEq, 3),
            (b'>', b'>', _) => (Punct::GtGtGt, 2),
            (b'>', b'=', _) => (Punct::GtGtEq, 2),
            (b'>', _, _) => (Punct::GtGt, 1),
            (b'=', _, _) => (Punct::GtEq, 1),
            _ => (Punct::Gt, 0),
        };
        self.pos += extra;
        Token { kind: Kind::Punct(punct), end: self.pos as u32, ..gt }
    }

    // ---- cursor helpers -------------------------------------------------------------

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Byte `n` ahead of the cursor, or 0 past the end. The 0 only ever gets compared
    /// against specific ASCII bytes, so it cannot be mistaken for input.
    fn at(&self, n: usize) -> u8 {
        self.bytes.get(self.pos + n).copied().unwrap_or(0)
    }

    /// The cursor must sit on a character boundary before the end of the source.
    fn char_at(&self, pos: usize) -> char {
        self.src[pos..].chars().next().unwrap_or('\0')
    }

    fn advance_char(&mut self) {
        self.pos += self.char_at(self.pos).len_utf8().max(1);
    }

    fn err(&self, message: &'static str) -> LexError {
        LexError::new(message, self.pos as u32)
    }

    fn err_at(&self, message: &'static str, pos: usize) -> LexError {
        LexError::new(message, pos as u32)
    }

    /// U+2028 or U+2029 starts at the cursor (they are `E2 80 A8` and `E2 80 A9`).
    fn at_line_separator(&self) -> bool {
        self.at(0) == 0xE2 && self.at(1) == 0x80 && matches!(self.at(2), 0xA8 | 0xA9)
    }

    // ---- whitespace and comments ----------------------------------------------------

    /// Skips whitespace and comments. Returns whether a line terminator was crossed.
    ///
    /// HTML-like comments (`<!--`, `-->`) are Annex B script syntax and are not valid in
    /// modules, so they are deliberately not recognized.
    fn skip_trivia(&mut self) -> Result<bool> {
        let mut newline = false;
        // `-->` only starts a comment when nothing but whitespace and comments precede it on
        // its line, which includes the very start of the input.
        let at_input_start = self.pos == 0;
        while let Some(b) = self.peek() {
            match b {
                b'<' if self.html_comments && self.at(1) == b'!' && self.at(2) == b'-' && self.at(3) == b'-' => {
                    self.pos += 4;
                    self.skip_line_comment_body();
                }
                b'-' if self.html_comments
                    && (newline || at_input_start)
                    && self.at(1) == b'-'
                    && self.at(2) == b'>' =>
                {
                    self.pos += 3;
                    self.skip_line_comment_body();
                }
                b' ' | b'\t' | 0x0B | 0x0C => self.pos += 1,
                b'\n' | b'\r' => {
                    newline = true;
                    self.pos += 1;
                }
                b'/' => match self.at(1) {
                    b'/' => {
                        self.pos += 2;
                        self.skip_line_comment_body();
                    }
                    b'*' => newline |= self.skip_block_comment()?,
                    _ => break,
                },
                0x80.. => {
                    let c = self.char_at(self.pos);
                    if c == '\u{2028}' || c == '\u{2029}' {
                        newline = true;
                    } else if !is_js_space(c) {
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                _ => break,
            }
        }
        Ok(newline)
    }

    fn skip_line_comment_body(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'\n' || b == b'\r' || (b == 0xE2 && self.at_line_separator()) {
                break;
            }
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) -> Result<bool> {
        let start = self.pos;
        self.pos += 2;
        let mut newline = false;
        loop {
            match self.peek() {
                None => return Err(self.err_at("unterminated comment", start)),
                Some(b'*') if self.at(1) == b'/' => {
                    self.pos += 2;
                    return Ok(newline);
                }
                Some(b'\n') | Some(b'\r') => {
                    newline = true;
                    self.pos += 1;
                }
                Some(0xE2) if self.at_line_separator() => {
                    newline = true;
                    self.pos += 3;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    // ---- tokens ---------------------------------------------------------------------

    fn scan(&mut self, b: u8, escaped: &mut bool) -> Result<Kind> {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'\\' => self.scan_ident(escaped),
            b'0'..=b'9' => self.scan_number(),
            b'.' if self.at(1).is_ascii_digit() => self.scan_number(),
            b'"' | b'\'' => self.scan_string(b),
            b'`' => {
                let start = self.pos;
                self.pos += 1;
                self.scan_template_rest(true, start)
            }
            b'#' => {
                self.pos += 1;
                let starts_name = match self.peek() {
                    Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'\\') => true,
                    Some(0x80..) => is_id_start(self.char_at(self.pos)),
                    _ => false,
                };
                if !starts_name {
                    return Err(self.err("expected a name after `#`"));
                }
                self.scan_ident(escaped)?;
                Ok(Kind::PrivateName)
            }
            0x80.. => {
                if is_id_start(self.char_at(self.pos)) {
                    self.scan_ident(escaped)
                } else {
                    Err(self.err("unexpected character"))
                }
            }
            _ => self.scan_punct(b),
        }
    }

    fn scan_ident(&mut self, escaped: &mut bool) -> Result<Kind> {
        let start = self.pos;
        // Fast path: nearly every identifier is plain ASCII, with nothing to decode.
        let mut end = start;
        while end < self.bytes.len() && IDENT_BYTE[self.bytes[end] as usize] {
            end += 1;
        }
        if !matches!(self.bytes.get(end), Some(b'\\') | Some(0x80..)) {
            self.pos = end;
            return Ok(keyword(&self.src[start..end]).map_or(Kind::Ident, Kind::Keyword));
        }
        // Only built when an escape forces us to decode the name.
        let mut cooked: Option<String> = None;
        loop {
            let first = self.pos == start;
            match self.peek() {
                Some(b) if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' => {
                    if let Some(s) = cooked.as_mut() {
                        s.push(b as char);
                    }
                    self.pos += 1;
                }
                Some(b'\\') => {
                    let escape_start = self.pos;
                    let cooked = cooked.get_or_insert_with(|| self.src[start..escape_start].to_string());
                    let value = self.scan_unicode_escape()?;
                    let c = char::from_u32(value).filter(|&c| if first { is_id_start(c) } else { is_id_continue(c) });
                    match c {
                        Some(c) => cooked.push(c),
                        None => return Err(self.err_at("invalid character in identifier escape", escape_start)),
                    }
                }
                Some(0x80..) => {
                    let c = self.char_at(self.pos);
                    if !(if first { is_id_start(c) } else { is_id_continue(c) }) {
                        break;
                    }
                    if let Some(s) = cooked.as_mut() {
                        s.push(c);
                    }
                    self.pos += c.len_utf8();
                }
                _ => break,
            }
        }
        *escaped = cooked.is_some();
        let name = cooked.as_deref().unwrap_or(&self.src[start..self.pos]);
        Ok(keyword(name).map_or(Kind::Ident, Kind::Keyword))
    }

    /// Cursor is on a `\`. Consumes `\uXXXX` or `\u{X...}` and returns the code point
    /// (or UTF-16 code unit, which may be a lone surrogate; strings allow those).
    fn scan_unicode_escape(&mut self) -> Result<u32> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'\\'));
        self.pos += 1;
        if self.peek() != Some(b'u') {
            return Err(self.err_at("expected `u` after `\\` in identifier", start));
        }
        self.pos += 1;
        self.scan_unicode_escape_digits(start)
    }

    /// Cursor is just past the `u` of a unicode escape.
    fn scan_unicode_escape_digits(&mut self, start: usize) -> Result<u32> {
        if self.peek() == Some(b'{') {
            self.pos += 1;
            let mut value: u32 = 0;
            let mut digits = 0;
            while let Some(d) = self.peek().and_then(hex_value) {
                value = value * 16 + d;
                if value > 0x10FFFF {
                    return Err(self.err_at("unicode escape is out of range", start));
                }
                digits += 1;
                self.pos += 1;
            }
            if digits == 0 || self.peek() != Some(b'}') {
                return Err(self.err_at("malformed unicode escape", start));
            }
            self.pos += 1;
            Ok(value)
        } else {
            let mut value: u32 = 0;
            for _ in 0..4 {
                match self.peek().and_then(hex_value) {
                    Some(d) => {
                        value = value * 16 + d;
                        self.pos += 1;
                    }
                    None => return Err(self.err_at("malformed unicode escape", start)),
                }
            }
            Ok(value)
        }
    }

    fn scan_number(&mut self) -> Result<Kind> {
        let mut kind = Kind::Number;
        let b0 = self.bytes[self.pos];
        if b0 == b'.' {
            self.pos += 1;
            self.scan_digits(10)?;
            self.scan_exponent()?;
        } else if b0 == b'0' && matches!(self.at(1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
            let radix = match self.at(1) {
                b'x' | b'X' => 16,
                b'o' | b'O' => 8,
                _ => 2,
            };
            self.pos += 2;
            if self.scan_digits(radix)? == 0 {
                return Err(self.err("expected digits after the radix prefix"));
            }
            if self.peek() == Some(b'n') {
                self.pos += 1;
                kind = Kind::BigInt;
            }
        } else if b0 == b'0' && self.at(1).is_ascii_digit() {
            // Legacy octal (`017`) or a decimal with a leading zero (`089`). Neither allows
            // separators or a bigint suffix; the parser rejects both forms in strict code.
            let start = self.pos;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
            let octal = self.bytes[start..self.pos].iter().all(|&b| b < b'8');
            if !octal {
                if self.peek() == Some(b'.') {
                    self.pos += 1;
                    while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                        self.pos += 1;
                    }
                }
                self.scan_exponent()?;
            }
            if self.peek() == Some(b'_') {
                return Err(self.err("numeric separators are not allowed in legacy octal literals"));
            }
        } else {
            if b0 == b'0' && self.at(1) == b'_' {
                return Err(self.err("numeric separators are not allowed after a leading zero"));
            }
            self.scan_digits(10)?;
            let mut integer = true;
            if self.peek() == Some(b'.') {
                self.pos += 1;
                integer = false;
                if self.at(0).is_ascii_digit() {
                    self.scan_digits(10)?;
                }
            }
            if self.scan_exponent()? {
                integer = false;
            }
            if integer && self.peek() == Some(b'n') {
                self.pos += 1;
                kind = Kind::BigInt;
            }
        }
        // `3in x` and `0xFFg` are errors rather than two tokens.
        match self.peek() {
            Some(b) if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'\\' => {
                Err(self.err("an identifier or number cannot directly follow a number"))
            }
            Some(0x80..) if is_id_start(self.char_at(self.pos)) => {
                Err(self.err("an identifier cannot directly follow a number"))
            }
            _ => Ok(kind),
        }
    }

    /// Consumes digits of `radix` with `_` separators and returns how many digits it saw.
    /// A separator must sit between two digits.
    fn scan_digits(&mut self, radix: u32) -> Result<usize> {
        let mut count = 0;
        let mut after_separator = false;
        loop {
            match self.peek() {
                Some(b'_') => {
                    if count == 0 || after_separator {
                        return Err(self.err("numeric separators must sit between digits"));
                    }
                    after_separator = true;
                    self.pos += 1;
                }
                Some(b) if is_digit(b, radix) => {
                    count += 1;
                    after_separator = false;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if after_separator {
            return Err(self.err("numeric separators must sit between digits"));
        }
        Ok(count)
    }

    /// Consumes an optional exponent. Returns whether there was one.
    fn scan_exponent(&mut self) -> Result<bool> {
        if !matches!(self.peek(), Some(b'e' | b'E')) {
            return Ok(false);
        }
        self.pos += 1;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        if self.scan_digits(10)? == 0 {
            return Err(self.err("expected digits in the exponent"));
        }
        Ok(true)
    }

    fn scan_string(&mut self, quote: u8) -> Result<Kind> {
        let start = self.pos;
        self.pos += 1;
        // Quotes, backslashes and line breaks are ASCII, so stepping bytewise never lands
        // inside a multi-byte character on a byte we care about.
        loop {
            match self.peek() {
                None | Some(b'\n') | Some(b'\r') => return Err(self.err_at("unterminated string literal", start)),
                Some(b'\\') => {
                    self.pos += 1;
                    self.scan_string_escape(start)?;
                }
                Some(b) if b == quote => {
                    self.pos += 1;
                    return Ok(Kind::String);
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// Cursor is just past a `\` inside a string literal that began at `string_start`.
    fn scan_string_escape(&mut self, string_start: usize) -> Result<()> {
        match self.peek() {
            None => Err(self.err_at("unterminated string literal", string_start)),
            Some(b'\r') => {
                self.pos += 1;
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
                Ok(())
            }
            Some(b'x') => {
                let escape_start = self.pos - 1;
                self.pos += 1;
                for _ in 0..2 {
                    if self.peek().and_then(hex_value).is_none() {
                        return Err(self.err_at("malformed hex escape", escape_start));
                    }
                    self.pos += 1;
                }
                Ok(())
            }
            Some(b'u') => {
                let escape_start = self.pos - 1;
                self.pos += 1;
                self.scan_unicode_escape_digits(escape_start).map(|_| ())
            }
            // Single-character escapes, line continuations, and legacy octal digits (the
            // parser rejects octal escapes in strict code).
            Some(b) if b < 0x80 => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => {
                self.advance_char();
                Ok(())
            }
        }
    }

    /// Scans template characters up to `` ` `` or `${`. `head` is true when scanning from
    /// an opening backtick. Escapes are not validated here: tagged templates may contain
    /// invalid ones, and the parser checks untagged templates.
    fn scan_template_rest(&mut self, head: bool, token_start: usize) -> Result<Kind> {
        loop {
            match self.peek() {
                None => return Err(self.err_at("unterminated template literal", token_start)),
                Some(b'`') => {
                    self.pos += 1;
                    return Ok(if head { Kind::NoSubTemplate } else { Kind::TemplateTail });
                }
                Some(b'$') if self.at(1) == b'{' => {
                    self.pos += 2;
                    return Ok(if head { Kind::TemplateHead } else { Kind::TemplateMiddle });
                }
                Some(b'\\') => {
                    self.pos += 1;
                    if self.peek().is_some() {
                        self.advance_char();
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn scan_punct(&mut self, b: u8) -> Result<Kind> {
        use Punct::*;
        let (punct, len) = match (b, self.at(1), self.at(2)) {
            (b'{', _, _) => (LBrace, 1),
            (b'}', _, _) => (RBrace, 1),
            (b'(', _, _) => (LParen, 1),
            (b')', _, _) => (RParen, 1),
            (b'[', _, _) => (LBracket, 1),
            (b']', _, _) => (RBracket, 1),
            (b';', _, _) => (Semi, 1),
            (b',', _, _) => (Comma, 1),
            (b':', _, _) => (Colon, 1),
            (b'~', _, _) => (Tilde, 1),
            (b'@', _, _) => (At, 1),
            (b'.', b'.', b'.') => (Ellipsis, 3),
            (b'.', _, _) => (Dot, 1),
            (b'?', b'?', b'=') => (QuestionQuestionEq, 3),
            (b'?', b'?', _) => (QuestionQuestion, 2),
            // `a?.5:b` is a conditional, not optional chaining.
            (b'?', b'.', d) if !d.is_ascii_digit() => (QuestionDot, 2),
            (b'?', _, _) => (Question, 1),
            (b'=', b'=', b'=') => (EqEqEq, 3),
            (b'=', b'=', _) => (EqEq, 2),
            (b'=', b'>', _) => (Arrow, 2),
            (b'=', _, _) => (Eq, 1),
            (b'!', b'=', b'=') => (NotEqEq, 3),
            (b'!', b'=', _) => (NotEq, 2),
            (b'!', _, _) => (Bang, 1),
            (b'+', b'+', _) => (PlusPlus, 2),
            (b'+', b'=', _) => (PlusEq, 2),
            (b'+', _, _) => (Plus, 1),
            (b'-', b'-', _) => (MinusMinus, 2),
            (b'-', b'=', _) => (MinusEq, 2),
            (b'-', _, _) => (Minus, 1),
            (b'*', b'*', b'=') => (StarStarEq, 3),
            (b'*', b'*', _) => (StarStar, 2),
            (b'*', b'=', _) => (StarEq, 2),
            (b'*', _, _) => (Star, 1),
            (b'/', b'=', _) => (SlashEq, 2),
            (b'/', _, _) => (Slash, 1),
            (b'%', b'=', _) => (PercentEq, 2),
            (b'%', _, _) => (Percent, 1),
            (b'<', b'<', b'=') => (LtLtEq, 3),
            (b'<', b'<', _) => (LtLt, 2),
            (b'<', b'=', _) => (LtEq, 2),
            (b'<', _, _) => (Lt, 1),
            (b'>', _, _) => (Gt, 1),
            (b'&', b'&', b'=') => (AmpAmpEq, 3),
            (b'&', b'&', _) => (AmpAmp, 2),
            (b'&', b'=', _) => (AmpEq, 2),
            (b'&', _, _) => (Amp, 1),
            (b'|', b'|', b'=') => (PipePipeEq, 3),
            (b'|', b'|', _) => (PipePipe, 2),
            (b'|', b'=', _) => (PipeEq, 2),
            (b'|', _, _) => (Pipe, 1),
            (b'^', b'=', _) => (CaretEq, 2),
            (b'^', _, _) => (Caret, 1),
            _ => return Err(self.err("unexpected character")),
        };
        self.pos += len;
        Ok(Kind::Punct(punct))
    }
}

fn is_digit(b: u8, radix: u32) -> bool {
    match radix {
        2 => matches!(b, b'0' | b'1'),
        8 => matches!(b, b'0'..=b'7'),
        10 => b.is_ascii_digit(),
        _ => b.is_ascii_hexdigit(),
    }
}

fn hex_value(b: u8) -> Option<u32> {
    (b as char).to_digit(16)
}

/// JavaScript WhiteSpace other than line terminators: TAB, VT, FF, SP, NBSP, ZWNBSP and
/// the Unicode Zs category. This is narrower than `char::is_whitespace`, which also
/// accepts U+0085 (NEL) that JavaScript treats as an ordinary, illegal character.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\u{0B}' | '\u{0C}' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' | '\u{FEFF}'
    )
}

fn is_id_start(c: char) -> bool {
    if c.is_ascii() { c == '$' || c == '_' || c.is_ascii_alphabetic() } else { unicode::is_id_start(c) }
}

fn is_id_continue(c: char) -> bool {
    if c.is_ascii() {
        c == '$' || c == '_' || c.is_ascii_alphanumeric()
    } else {
        // ZWNJ and ZWJ are allowed in identifiers by ECMAScript on top of ID_Continue.
        unicode::is_id_continue(c) || c == '\u{200C}' || c == '\u{200D}'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(p: Punct) -> Kind {
        Kind::Punct(p)
    }

    fn kw(k: Kw) -> Kind {
        Kind::Keyword(k)
    }

    /// Lex without any rescans.
    fn lex(src: &str) -> Vec<(Kind, &str)> {
        let mut lexer = Lexer::new(src).unwrap();
        let mut out = Vec::new();
        loop {
            let tok = lexer.next_token().unwrap();
            if tok.kind == Kind::Eof {
                return out;
            }
            out.push((tok.kind, &src[tok.start as usize..tok.end as usize]));
        }
    }

    fn lex_err(src: &str) -> LexError {
        let mut lexer = match Lexer::new(src) {
            Ok(l) => l,
            Err(e) => return e,
        };
        loop {
            match lexer.next_token() {
                Ok(t) if t.kind == Kind::Eof => panic!("expected an error lexing {src:?}"),
                Ok(_) => {}
                Err(e) => return e,
            }
        }
    }

    /// Test sources spell a backslash as `~` so escape sequences survive any tooling that
    /// rewrites `\uXXXX` text before it reaches the compiler.
    fn esc(src: &str) -> String {
        src.replace('~', "\\")
    }

    fn one(src: &str) -> Kind {
        let toks = lex(src);
        assert_eq!(toks.len(), 1, "{src:?} lexed as {toks:?}");
        assert_eq!(toks[0].1, src);
        toks[0].0
    }

    #[test]
    fn keyword_lookup_is_exact() {
        for &kw in Kw::ALL {
            assert_eq!(keyword(kw.as_str()), Some(kw), "{}", kw.as_str());
            // One byte more or less is never a keyword.
            assert_eq!(keyword(&format!("{}x", kw.as_str())), None);
            assert_eq!(keyword(&kw.as_str()[..kw.as_str().len() - 1]).filter(|&k| k == kw), None);
        }
        for word in ["", "a", "b", "Break", "BREAK", "of", "let", "async", "await", "yield", "static", "get", "set", "from", "x1"] {
            assert_eq!(keyword(word), None, "{word}");
        }
        assert_eq!(Kw::ALL.len(), 36);
    }

    #[test]
    fn every_punctuator_round_trips() {
        use Punct::*;
        let all = [
            LBrace, RBrace, LParen, RParen, LBracket, RBracket, Semi, Comma, Dot, Ellipsis, Colon, At, Arrow, Lt,
            LtEq, EqEq, NotEq, EqEqEq, NotEqEq, Plus, Minus, Star, Slash, Percent, StarStar, PlusPlus, MinusMinus,
            LtLt, Amp, Pipe, Caret, Bang, Tilde, AmpAmp, PipePipe, QuestionQuestion, Question, QuestionDot, Eq,
            PlusEq, MinusEq, StarEq, SlashEq, PercentEq, StarStarEq, LtLtEq, AmpEq, PipeEq, CaretEq, AmpAmpEq,
            PipePipeEq, QuestionQuestionEq, Gt,
        ];
        for punct in all {
            assert_eq!(one(punct.as_str()), p(punct), "{}", punct.as_str());
        }
    }

    #[test]
    fn longest_match_wins() {
        assert_eq!(lex("a>>>=b")[1].0, p(Punct::Gt), "`>` is always single until rescanned");
        assert_eq!(lex("a===b")[1].0, p(Punct::EqEqEq));
        assert_eq!(lex("a**=b")[1].0, p(Punct::StarStarEq));
        assert_eq!(lex("a??=b")[1].0, p(Punct::QuestionQuestionEq));
        assert_eq!(lex("a=>b")[1].0, p(Punct::Arrow));
        assert_eq!(lex("...a")[0].0, p(Punct::Ellipsis));
    }

    #[test]
    fn question_dot_before_a_digit_is_a_conditional() {
        let toks = lex("a?.5:b");
        assert_eq!(toks[1].0, p(Punct::Question));
        assert_eq!(toks[2], (Kind::Number, ".5"));
        assert_eq!(lex("a?.b")[1].0, p(Punct::QuestionDot));
    }

    #[test]
    fn rescan_gt_joins_operators() {
        for (src, expected) in [
            (">", Punct::Gt),
            (">=", Punct::GtEq),
            (">>", Punct::GtGt),
            (">>=", Punct::GtGtEq),
            (">>>", Punct::GtGtGt),
            (">>>=", Punct::GtGtGtEq),
        ] {
            let mut lexer = Lexer::new(src).unwrap();
            let gt = lexer.next_token().unwrap();
            let joined = lexer.rescan_gt(gt);
            assert_eq!(joined.kind, p(expected), "{src}");
            assert_eq!(joined.end as usize, src.len());
            assert_eq!(lexer.next_token().unwrap().kind, Kind::Eof);
        }
    }

    #[test]
    fn identifiers_and_keywords() {
        assert_eq!(one("foo"), Kind::Ident);
        assert_eq!(one("$_a1"), Kind::Ident);
        assert_eq!(one("let"), Kind::Ident, "contextual words are identifiers");
        assert_eq!(one("await"), Kind::Ident);
        assert_eq!(one("import"), kw(Kw::Import));
        assert_eq!(one("instanceof"), kw(Kw::Instanceof));
        assert_eq!(one("imports"), Kind::Ident);
        assert_eq!(one("héllo"), Kind::Ident);
        assert_eq!(one("日本語"), Kind::Ident);
        assert_eq!(one("𠮷野家"), Kind::Ident, "astral letters");
    }

    #[test]
    fn identifiers_follow_unicode_id_tables() {
        // Combining marks, Other_ID_Start and Other_ID_Continue, all seen in real bundles.
        assert_eq!(one("\u{939}\u{93F}\u{928}\u{94D}\u{926}\u{940}"), Kind::Ident, "Devanagari with a virama");
        assert_eq!(one("\u{2118}"), Kind::Ident, "script capital P is Other_ID_Start");
        assert_eq!(one("I\u{307}"), Kind::Ident, "a combining dot above continues a name");
        assert_eq!(one("a\u{200D}b"), Kind::Ident, "ZWJ is allowed inside");
        assert_eq!(one("a\u{B7}b"), Kind::Ident, "middle dot is Other_ID_Continue");
        assert_eq!(one("a\u{663}"), Kind::Ident, "Arabic-Indic digits continue a name");
        assert_eq!(one("a\u{ff3f}"), Kind::Ident, "connector punctuation (Pc) continues a name");
    }

    #[test]
    fn non_identifier_characters_stay_out() {
        assert!(lex_err("\u{B2}").message.contains("unexpected"), "superscript two is No, not a letter");
        assert!(lex_err("a\u{B2}").message.contains("unexpected"));
        assert!(lex_err("\u{24B6}").message.contains("unexpected"), "circled A is a symbol");
        assert!(lex_err("\u{663}").message.contains("unexpected"), "a digit cannot start a name");
        assert!(lex_err("\u{B7}").message.contains("unexpected"), "middle dot cannot start a name");
        assert!(lex_err("\u{200D}").message.contains("unexpected"), "ZWJ cannot start a name");
        assert!(lex_err("\u{1F600}").message.contains("unexpected"), "emoji are not identifiers");
        assert!(lex_err("\u{ff3f}x").message.contains("unexpected"), "a connector cannot start a name");
    }

    #[test]
    fn escaped_identifiers() {
        let src = esc("a~u0062c ~u{1d7d8}");
        let mut lexer = Lexer::new(&src).unwrap();
        let first = lexer.next_token().unwrap();
        assert_eq!((first.kind, first.escaped), (Kind::Ident, true));
        // Mathematical digit zero is ID_Continue but not ID_Start.
        assert!(lexer.next_token().is_err());
    }

    #[test]
    fn escaped_keyword_is_flagged_not_hidden() {
        let src = esc("~u0069mport");
        let mut lexer = Lexer::new(&src).unwrap();
        let tok = lexer.next_token().unwrap();
        assert_eq!(tok.kind, kw(Kw::Import));
        assert!(tok.escaped);
        let plain = Lexer::new("import").unwrap().next_token().unwrap();
        assert!(!plain.escaped);
    }

    #[test]
    fn bad_identifier_escapes() {
        assert!(lex_err(&esc("~u0030a")).message.contains("identifier escape"), "digit cannot start a name");
        assert!(lex_err(r"a\u{110000}").message.contains("range"));
        assert!(lex_err(r"a\u12").message.contains("malformed"));
        assert!(lex_err(r"a\x41").message.contains("expected `u`"));
        assert!(lex_err(r"\uD800").message.contains("identifier escape"), "lone surrogate");
    }

    #[test]
    fn private_names() {
        assert_eq!(one("#count"), Kind::PrivateName);
        assert_eq!(one("#_"), Kind::PrivateName);
        assert!(lex_err("# x").message.contains("name after `#`"));
        assert!(lex_err("#1").message.contains("name after `#`"));
    }

    #[test]
    fn numbers() {
        for src in [
            "0", "7", "123", "1_000", "1.", "1.5", ".5", "1e3", "1E+3", "1.5e-3", "1_0.0_1e1_0", "0x1F", "0XfF",
            "0xA_B", "0o17", "0b1010", "0b1_0", "017", "089", "08.5", "0.5", ".5e2",
        ] {
            assert_eq!(one(src), Kind::Number, "{src}");
        }
        for src in ["0n", "123n", "0xFFn", "0b1n", "0o7n", "1_0n"] {
            assert_eq!(one(src), Kind::BigInt, "{src}");
        }
    }

    #[test]
    fn number_member_access() {
        let toks = lex("1..toString()");
        assert_eq!(toks[0], (Kind::Number, "1."));
        assert_eq!(toks[1].0, p(Punct::Dot));
        assert_eq!(lex("1.5.toFixed")[0], (Kind::Number, "1.5"));
    }

    #[test]
    fn bad_numbers() {
        for src in [
            "1_", "1__0", "1._5", "1e", "1e+", "1e_5", "0x", "0o8", "0b2", "0b12", "3in", "0xFFg", "1.5n", "1e3n",
            "01n", "01_1", "0_1", "1.toString", "5a", "0x_1", "09n",
        ] {
            let mut lexer = Lexer::new(src).unwrap();
            let failed = loop {
                match lexer.next_token() {
                    Ok(t) if t.kind == Kind::Eof => break false,
                    Ok(_) => {}
                    Err(_) => break true,
                }
            };
            assert!(failed, "{src:?} should not lex cleanly");
        }
        // A leading underscore makes this an identifier, not a malformed number.
        assert_eq!(one("_1_"), Kind::Ident);
    }

    #[test]
    fn strings() {
        assert_eq!(one(r#""hello""#), Kind::String);
        assert_eq!(one("'it\\'s'"), Kind::String);
        assert_eq!(one(r#""a\nb\x41A\u{1F600}\0""#), Kind::String);
        assert_eq!(one("'line\\\ncontinued'"), Kind::String);
        assert_eq!(one("'crlf\\\r\ncontinued'"), Kind::String);
        assert_eq!(one("'tab\there'"), Kind::String);
        assert_eq!(one("'\u{2028}'"), Kind::String, "U+2028 is allowed in strings");
        assert_eq!(one(&esc("'~u0041~x41~u{41}'")), Kind::String);
        assert_eq!(one(r#""lone \uD83D surrogate""#), Kind::String);
        assert_eq!(one(r#""日本語 \☃""#), Kind::String);
        assert_eq!(one(r#""\7 \12 \8""#), Kind::String, "legacy escapes lex; the parser rejects them in strict code");
    }

    #[test]
    fn bad_strings() {
        assert_eq!(lex_err("'abc").message, "unterminated string literal");
        assert_eq!(lex_err("'abc\n'").message, "unterminated string literal");
        assert_eq!(lex_err("'abc\r'").pos, 0, "reported at the opening quote");
        assert_eq!(lex_err("'abc\\").message, "unterminated string literal");
        assert!(lex_err(r"'\x4'").message.contains("hex"));
        assert!(lex_err(r"'\u{}'").message.contains("unicode"));
        assert!(lex_err(r"'\u{110000}'").message.contains("range"));
        assert!(lex_err(r"'\u00G0'").message.contains("unicode"));
    }

    #[test]
    fn templates() {
        assert_eq!(one("`plain text`"), Kind::NoSubTemplate);
        assert_eq!(one("`multi\nline`"), Kind::NoSubTemplate);
        assert_eq!(one(r"`escaped \` and \${ not a substitution`"), Kind::NoSubTemplate);
        assert_eq!(one(r"`invalid escape allowed for tags \unicode \xZ`"), Kind::NoSubTemplate);
        assert_eq!(one("`$ alone and $$ ok`"), Kind::NoSubTemplate);
        assert_eq!(lex_err("`open").message, "unterminated template literal");
    }

    #[test]
    fn template_with_substitutions() {
        let src = "`a${b}c${d}e`";
        let mut lexer = Lexer::new(src).unwrap();
        let text = |lexer: &Lexer, t| lexer.text(t).to_string();

        let head = lexer.next_token().unwrap();
        assert_eq!((head.kind, text(&lexer, head)), (Kind::TemplateHead, "`a${".into()));
        let b = lexer.next_token().unwrap();
        assert_eq!(b.kind, Kind::Ident);
        let rbrace = lexer.next_token().unwrap();
        assert!(rbrace.is(Punct::RBrace));
        let middle = lexer.rescan_template_continuation(rbrace).unwrap();
        assert_eq!((middle.kind, text(&lexer, middle)), (Kind::TemplateMiddle, "}c${".into()));
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Ident);
        let rbrace = lexer.next_token().unwrap();
        let tail = lexer.rescan_template_continuation(rbrace).unwrap();
        assert_eq!((tail.kind, text(&lexer, tail)), (Kind::TemplateTail, "}e`".into()));
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Eof);
    }

    #[test]
    fn nested_template_substitution() {
        let src = "`a${`b${c}d`}e`";
        let mut lexer = Lexer::new(src).unwrap();
        assert_eq!(lexer.next_token().unwrap().kind, Kind::TemplateHead);
        assert_eq!(lexer.next_token().unwrap().kind, Kind::TemplateHead);
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Ident);
        let rbrace = lexer.next_token().unwrap();
        assert_eq!(lexer.rescan_template_continuation(rbrace).unwrap().kind, Kind::TemplateTail);
        let rbrace = lexer.next_token().unwrap();
        assert_eq!(lexer.rescan_template_continuation(rbrace).unwrap().kind, Kind::TemplateTail);
    }

    fn regex(src: &str) -> std::result::Result<String, LexError> {
        let mut lexer = Lexer::new(src).unwrap();
        let slash = lexer.next_token().unwrap();
        lexer.rescan_regex(slash).map(|t| lexer.text(t).to_string())
    }

    #[test]
    fn regex_literals() {
        assert_eq!(regex("/ab+c/gi").unwrap(), "/ab+c/gi");
        assert_eq!(regex("/=/").unwrap(), "/=/", "starts as `/=`");
        assert_eq!(regex(r"/\/ and \\/").unwrap(), r"/\/ and \\/");
        assert_eq!(regex("/[/]/").unwrap(), "/[/]/", "slash inside a class");
        assert_eq!(regex(r"/[\]/]+/u").unwrap(), r"/[\]/]+/u");
        assert_eq!(regex("/a/dgimsuyv").unwrap(), "/a/dgimsuyv");
        assert_eq!(regex("/a/ b").unwrap(), "/a/", "space ends the flags");
        assert_eq!(regex("/é/").unwrap(), "/é/");
    }

    #[test]
    fn bad_regex() {
        assert!(regex("/abc").is_err());
        assert!(regex("/abc\n/").is_err());
        assert!(regex("/[abc/").is_err(), "unclosed class swallows the slash");
        assert!(regex("/a\\\n/").is_err());
        assert!(regex("/a\u{2028}/").is_err());
    }

    #[test]
    fn division_is_the_default_reading() {
        let toks = lex("a / b /= c");
        assert_eq!(toks[1].0, p(Punct::Slash));
        assert_eq!(toks[3].0, p(Punct::SlashEq));
    }

    #[test]
    fn comments_and_newline_tracking() {
        let mut lexer = Lexer::new("a // c\nb /* x */ c /* x\ny */ d\u{2028}e").unwrap();
        let flags: Vec<bool> = (0..5).map(|_| lexer.next_token().unwrap().newline_before).collect();
        assert_eq!(flags, [false, true, false, true, true]);
        assert_eq!(lexer.next_token().unwrap().kind, Kind::Eof);
    }

    #[test]
    fn line_comment_ends_at_each_terminator() {
        for term in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
            let src = format!("a // hidden{term}b");
            let toks = lex(&src);
            assert_eq!(toks.len(), 2, "{term:?}");
            assert_eq!(toks[1].1, "b");
        }
    }

    #[test]
    fn block_comments() {
        assert_eq!(lex("/**/a/***/").len(), 1);
        assert_eq!(lex("a/* // */b").len(), 2);
        assert_eq!(lex_err("a /* never closed").message, "unterminated comment");
        assert_eq!(lex_err("a /* never closed").pos, 2);
    }

    #[test]
    fn whitespace() {
        let toks = lex("a\u{A0}b\u{FEFF}c\u{3000}d\te\u{0B}f\u{0C}g\u{2003}h");
        assert_eq!(toks.len(), 8);
        // NEL (U+0085) is not JavaScript whitespace even though Unicode says it is.
        assert_eq!(lex_err("a\u{85}b").message, "unexpected character");
        // Zero-width space is not whitespace either.
        assert_eq!(lex_err("a\u{200B}b").message, "unexpected character");
    }

    #[test]
    fn hashbang_only_at_the_start() {
        assert_eq!(lex("#!/usr/bin/env node\nfoo"), vec![(Kind::Ident, "foo")]);
        assert!(lex_err(" #!/usr/bin/env node").message.contains("name after `#`"));
    }

    #[test]
    fn unexpected_characters() {
        assert_eq!(lex_err("a ¬ b").message, "unexpected character");
        assert_eq!(lex_err("a ¬ b").pos, 2);
        assert_eq!(lex_err("\u{0}").message, "unexpected character");
        assert_eq!(lex_err("a \\ b").message, "expected `u` after `\\` in identifier");
    }

    #[test]
    fn token_offsets_are_byte_offsets() {
        let src = "é = 1";
        let mut lexer = Lexer::new(src).unwrap();
        let a = lexer.next_token().unwrap();
        assert_eq!((a.start, a.end), (0, 2));
        let eq = lexer.next_token().unwrap();
        assert_eq!((eq.start, eq.end), (3, 4));
    }

    #[test]
    fn reset_allows_backtracking() {
        let mut lexer = Lexer::new("a b c").unwrap();
        lexer.next_token().unwrap();
        let mark = lexer.pos();
        let b = lexer.next_token().unwrap();
        lexer.next_token().unwrap();
        lexer.reset(mark);
        assert_eq!(lexer.next_token().unwrap(), b);
    }

    #[test]
    fn realistic_snippet() {
        let src = "export const x = (a, b) => a ** b ?? 0; // done\n";
        let kinds: Vec<Kind> = lex(src).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            kinds,
            vec![
                kw(Kw::Export),
                kw(Kw::Const),
                Kind::Ident,
                p(Punct::Eq),
                p(Punct::LParen),
                Kind::Ident,
                p(Punct::Comma),
                Kind::Ident,
                p(Punct::RParen),
                p(Punct::Arrow),
                Kind::Ident,
                p(Punct::StarStar),
                Kind::Ident,
                p(Punct::QuestionQuestion),
                Kind::Number,
                p(Punct::Semi),
            ]
        );
    }
}
