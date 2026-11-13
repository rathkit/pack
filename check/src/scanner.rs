//! A scanner that reports what TypeScript's scanner reports, with the same codes, start and length (UTF-16 code units).
//!
//! TypeScript's scanner is steered by its parser (is this `/` a division or a regular expression; does this `}` end a template
//! substitution). Stage 1 has no parser, so it decides from the previous token. Where that decision could go either way, or where
//! the scanner would report something this stage does not implement, the scan stops (see [`Stop`]) and nothing after that point is
//! reported. A stop is output, not a silent skip: the caller says which file stopped, where and why.

/// A diagnostic as TypeScript reports it: `start` and `length` count UTF-16 code units from the start of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: u32,
    pub start: u32,
    pub length: u32,
    pub message: String,
    /// True when every token before this diagnostic was found without a decision that TypeScript's parser makes in TypeScript (is this
    /// `/` a division or a regular expression; does this `}` end a template substitution). After such a decision the checker follows a
    /// heuristic, which is right for code that is valid up to that point and can be wrong after a syntax error the checker does not
    /// see, so later diagnostics are `verified: false` and are not printed unless asked for.
    pub verified: bool,
}

/// Why a scan stopped before the end of the file. Positions are UTF-16 code units, like [`Diagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub at: u32,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    /// Set when the rest of the file was not scanned.
    pub stopped: Option<Stop>,
}

/// The kinds of file stage 1 scans: TypeScript, not JSX (the scanner's JSX modes need the parser), not JavaScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Ts,
}

impl FileKind {
    /// `None` for a file this stage does not check.
    pub fn from_path(path: &str) -> Option<FileKind> {
        let lower = path.to_ascii_lowercase();
        [".ts", ".mts", ".cts"].iter().any(|e| lower.ends_with(e)).then_some(FileKind::Ts)
    }
}

/// Every code this stage can report, with TypeScript's message text.
pub const CODES: &[(u32, &str)] = &[
    (1002, "Unterminated string literal."),
    (1010, "'*/' expected."),
    (1124, "Digit expected."),
    (1125, "Hexadecimal digit expected."),
    (1126, "Unexpected end of text."),
    (1127, "Invalid character."),
    (1160, "Unterminated template literal."),
    (1161, "Unterminated regular expression literal."),
    (1177, "Binary digit expected."),
    (1178, "Octal digit expected."),
    (1121, "Octal literals are not allowed. Use the syntax '{0}'."),
    (1198, "An extended Unicode escape value must be between 0x0 and 0x10FFFF inclusive."),
    (1199, "Unterminated Unicode escape sequence."),
    (1487, "Octal escape sequences are not allowed. Use the syntax '{0}'."),
    (1488, "Escape sequence '{0}' is not allowed."),
    (1489, "Decimals with leading zeros are not allowed."),
    (1351, "An identifier or keyword cannot immediately follow a numeric literal."),
    (1352, "A bigint literal cannot use exponential notation."),
    (1353, "A bigint literal must be an integer."),
    (6188, "Numeric separators are not allowed here."),
    (6189, "Multiple consecutive numeric separators are not permitted."),
];

fn message(code: u32) -> &'static str {
    CODES.iter().find(|(c, _)| *c == code).map(|(_, m)| *m).unwrap_or("")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing, or a token after which an expression starts: `/` begins a regular expression.
    Expr,
    /// A token that ends an operand: `/` divides.
    Operand,
    /// `)`, `}`, `++`, `--`, `>` and a few contextual keywords: either reading is possible.
    Either,
}

struct Scanner<'a> {
    text: &'a str,
    s: &'a [u8],
    pos: usize,
    diags: Vec<(u32, usize, usize, bool, Option<String>)>,
    /// The previous token was a single `-` (TypeScript includes it in the range of an octal literal error).
    prev_minus: bool,
    /// Set by the first parser-dependent decision.
    decided: bool,
    stop: Option<(usize, &'static str)>,
}

fn is_id_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_id_part(b: u8) -> bool {
    is_id_start(b) || b.is_ascii_digit()
}

/// Whitespace TypeScript skips that is not ASCII, and the line terminators U+2028 and U+2029.
fn non_ascii_trivia(c: char) -> bool {
    matches!(c, '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200b}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

impl<'a> Scanner<'a> {
    fn b(&self, i: usize) -> Option<u8> {
        self.s.get(i).copied()
    }

    fn cur(&self) -> Option<u8> {
        self.b(self.pos)
    }

    fn char_at(&self, i: usize) -> Option<char> {
        self.text.get(i..).and_then(|t| t.chars().next())
    }

    fn report(&mut self, code: u32, start: usize, end: usize) {
        self.report_with(code, start, end, None);
    }

    fn report_with(&mut self, code: u32, start: usize, end: usize, text: Option<String>) {
        // TypeScript drops an error at the same position as the previous one.
        if self.diags.last().is_none_or(|d| d.1 != start) {
            self.diags.push((code, start, end, !self.decided, text));
        }
    }

    fn halt(&mut self, at: usize, reason: &'static str) {
        if self.stop.is_none() {
            self.stop = Some((at, reason));
        }
    }

    /// At a line break (by byte, or U+2028/U+2029)?
    fn line_break_len(&self, i: usize) -> usize {
        match self.b(i) {
            Some(b'\n') | Some(b'\r') => 1,
            Some(0xe2) => match self.char_at(i) {
                Some('\u{2028}') | Some('\u{2029}') => 3,
                _ => 0,
            },
            _ => 0,
        }
    }

    fn run(&mut self) {
        let mut prev = Prev::Expr;
        let mut after_dot = false;
        let mut templates: Vec<u32> = Vec::new(); // open `${`: the `{` depth inside each
        if self.s.starts_with(b"#!") {
            while self.pos < self.s.len() && self.line_break_len(self.pos) == 0 {
                self.pos += 1;
            }
        }
        let mut line_start = true;
        while self.pos < self.s.len() && self.stop.is_none() {
            let start = self.pos;
            let c = self.s[start];
            let was_line_start = line_start;
            line_start = false;
            match c {
                b' ' | b'\t' | 0x0b | 0x0c => {
                    self.pos += 1;
                    line_start = was_line_start;
                    continue;
                }
                b'\n' | b'\r' => {
                    self.pos += 1;
                    line_start = true;
                    continue;
                }
                _ => {}
            }
            if was_line_start && matches!(c, b'<' | b'>' | b'=' | b'|') && self.s[start..].iter().take(7).filter(|&&x| x == c).count() == 7 {
                self.halt(start, "a merge conflict marker");
                break;
            }
            let was_dot = std::mem::replace(&mut after_dot, false);
            let mut minus_now = false;
            match c {
                0x80.. => {
                    let ch = self.char_at(start).unwrap_or('\u{fffd}');
                    if non_ascii_trivia(ch) {
                        self.pos += ch.len_utf8();
                        line_start = was_line_start || ch == '\u{2028}' || ch == '\u{2029}';
                    } else {
                        self.halt(start, "a non-ASCII character in code (the Unicode identifier tables are not carried in stage 1)");
                    }
                }
                b'"' | b'\'' => {
                    self.pos += 1;
                    self.string(c);
                    prev = Prev::Operand;
                }
                b'`' => {
                    self.pos += 1;
                    self.template(&mut templates);
                    prev = Prev::Operand;
                }
                b'{' => {
                    if let Some(d) = templates.last_mut() {
                        *d += 1;
                    }
                    self.pos += 1;
                    prev = Prev::Expr;
                }
                b'}' => {
                    self.pos += 1;
                    match templates.last_mut() {
                        Some(0) => {
                            self.decided = true;
                            templates.pop();
                            self.template(&mut templates);
                            prev = Prev::Operand;
                        }
                        Some(d) => {
                            *d -= 1;
                            prev = Prev::Either;
                        }
                        None => prev = Prev::Either,
                    }
                }
                b'/' => match self.b(start + 1) {
                    Some(b'/') => {
                        while self.pos < self.s.len() && self.line_break_len(self.pos) == 0 {
                            self.pos += 1;
                        }
                        line_start = was_line_start;
                    }
                    Some(b'*') => {
                        self.pos += 2;
                        match self.text[self.pos..].find("*/") {
                            Some(i) => self.pos += i + 2,
                            None => {
                                self.pos = self.s.len();
                                self.report(1010, self.pos, self.pos);
                            }
                        }
                        line_start = was_line_start;
                    }
                    _ => {
                        self.decided = true;
                        let regex = match prev {
                            Prev::Expr => true,
                            Prev::Operand => false,
                            Prev::Either => {
                                if self.regex_end(start).is_some() {
                                    self.halt(start, "a `/` that could begin a regular expression or divide, and the parser decides which in TypeScript");
                                    break;
                                }
                                false
                            }
                        };
                        if regex {
                            self.regex(start);
                            prev = Prev::Operand;
                        } else {
                            self.pos += if self.b(start + 1) == Some(b'=') { 2 } else { 1 };
                            prev = Prev::Expr;
                        }
                    }
                },
                b'0'..=b'9' => {
                    self.number();
                    prev = Prev::Operand;
                }
                b'.' if self.b(start + 1).is_some_and(|d| d.is_ascii_digit()) => {
                    self.number();
                    prev = Prev::Operand;
                }
                b'.' if self.b(start + 1) == Some(b'.') && self.b(start + 2) == Some(b'.') => {
                    self.pos += 3;
                    prev = Prev::Expr;
                }
                b'.' => {
                    self.pos += 1;
                    after_dot = true;
                    prev = Prev::Expr;
                }
                b'?' if self.b(start + 1) == Some(b'.') && !self.b(start + 2).is_some_and(|d| d.is_ascii_digit()) => {
                    self.pos += 2;
                    after_dot = true;
                    prev = Prev::Expr;
                }
                b')' => {
                    self.pos += 1;
                    prev = Prev::Either;
                }
                b']' => {
                    self.pos += 1;
                    prev = Prev::Operand;
                }
                b'+' | b'-' if self.b(start + 1) == Some(c) => {
                    self.pos += 2;
                    prev = Prev::Either;
                }
                b'>' => {
                    let mut end = start;
                    let mut has_eq = false;
                    while let Some(x @ (b'>' | b'=')) = self.b(end) {
                        has_eq |= x == b'=';
                        end += 1;
                    }
                    self.pos = end;
                    prev = if has_eq { Prev::Expr } else { Prev::Either };
                }
                b'=' if self.b(start + 1) == Some(b'>') => {
                    self.pos += 2;
                    prev = Prev::Expr;
                }
                b'#' => {
                    if start == 0 && self.b(1) == Some(b'!') {
                        // handled above
                    } else if self.b(start + 1) == Some(b'!') {
                        self.halt(start, "a `#!` after the first character");
                    } else if self.b(start + 1).is_some_and(is_id_start) {
                        self.pos += 1;
                        self.ident_rest();
                        prev = Prev::Operand;
                    } else if self.b(start + 1).is_some_and(|d| d >= 0x80 || d == b'\\') {
                        self.halt(start, "`#` before a non-ASCII character or an escape");
                    } else {
                        self.pos += 1;
                        self.report(1127, start, start + 1);
                        prev = Prev::Expr;
                    }
                }
                b'\\' => match self.b(start + 1) {
                    Some(b'u') => self.halt(start, "an identifier with a Unicode escape"),
                    _ => {
                        self.pos += 1;
                        self.report(1127, start, start);
                        prev = Prev::Expr;
                    }
                },
                _ if is_id_start(c) => {
                    self.ident_rest();
                    if self.stop.is_none() {
                        let word = &self.text[start..self.pos];
                        prev = if was_dot { Prev::Operand } else { keyword_prev(word) };
                    }
                }
                0..=0x1f | 0x7f => {
                    self.pos += 1;
                    self.report(1127, start, start + 1);
                    prev = Prev::Expr;
                }
                _ => {
                    self.pos += 1;
                    prev = Prev::Expr;
                    minus_now = c == b'-' && !matches!(self.cur(), Some(b'-' | b'=')) && self.b(start.wrapping_sub(1)) != Some(b'-');
                }
            }
            self.prev_minus = minus_now;
        }
    }

    fn ident_rest(&mut self) {
        while self.cur().is_some_and(is_id_part) {
            self.pos += 1;
        }
        match self.cur() {
            Some(b) if b >= 0x80 => {
                let ch = self.char_at(self.pos).unwrap_or('\u{fffd}');
                if !non_ascii_trivia(ch) {
                    self.halt(self.pos, "a non-ASCII character in an identifier (the Unicode identifier tables are not carried in stage 1)");
                }
            }
            Some(b'\\') => self.halt(self.pos, "an identifier with a Unicode escape"),
            _ => {}
        }
    }

    // ---- strings and templates ----

    /// Scans an escape sequence after the backslash at `self.pos - 1`. Returns false when scanning must stop.
    fn escape(&mut self, in_template: bool) -> bool {
        let Some(c) = self.cur() else {
            self.report(1126, self.pos, self.pos);
            return false;
        };
        match c {
            b'0' if !self.b(self.pos + 1).is_some_and(|d| d.is_ascii_digit()) => self.pos += 1,
            b'0'..=b'9' if in_template => {
                self.halt(self.pos - 1, "an octal or decimal escape in a template (an error only when the template is untagged)");
                return false;
            }
            b'8' | b'9' => {
                let start = self.pos - 1;
                self.pos += 1;
                self.report_with(1488, start, self.pos, Some(format!("Escape sequence '{}' is not allowed.", &self.text[start..self.pos])));
            }
            b'0'..=b'7' => {
                let start = self.pos - 1;
                let first = c;
                self.pos += 1;
                // `\0` and `\1` to `\3` take up to two more octal digits, `\4` to `\7` one.
                let more = if first <= b'3' { 2 } else { 1 };
                for _ in 0..more {
                    if self.cur().is_some_and(|d| (b'0'..=b'7').contains(&d)) {
                        self.pos += 1;
                    }
                }
                let value = u32::from_str_radix(&self.text[start + 1..self.pos], 8).unwrap_or(0);
                self.report_with(1487, start, self.pos, Some(format!("Octal escape sequences are not allowed. Use the syntax '\\x{value:02x}'.")));
            }
            b'x' => {
                self.pos += 1;
                self.hex_exact(2);
            }
            b'u' => {
                self.pos += 1;
                if self.cur() == Some(b'{') {
                    self.pos += 1;
                    if !self.extended_escape(in_template) {
                        return false;
                    }
                } else {
                    self.hex_exact(4);
                }
            }
            b'\r' => {
                self.pos += 1;
                if self.cur() == Some(b'\n') {
                    self.pos += 1;
                }
            }
            _ => {
                let len = match self.char_at(self.pos) {
                    Some(ch) => ch.len_utf8(),
                    None => 1,
                };
                self.pos += len;
            }
        }
        // In a template, a malformed escape is an error only when the template is untagged, which needs the parser.
        true
    }

    fn hex_exact(&mut self, n: usize) {
        for _ in 0..n {
            if self.cur().is_some_and(|d| d.is_ascii_hexdigit()) {
                self.pos += 1;
            } else {
                self.report(1125, self.pos, self.pos);
                return;
            }
        }
    }

    fn extended_escape(&mut self, in_template: bool) -> bool {
        let start = self.pos;
        let mut value: u64 = 0;
        let mut digits = 0;
        while let Some(d) = self.cur().filter(u8::is_ascii_hexdigit) {
            value = (value * 16 + (d as char).to_digit(16).unwrap() as u64).min(0xffff_ffff);
            digits += 1;
            self.pos += 1;
        }
        if in_template {
            // Untagged and tagged templates differ here; stage 1 has no parser to tell them apart.
            if digits == 0 || value > 0x10ffff || self.cur() != Some(b'}') {
                self.halt(start, "a malformed `\\u{...}` escape in a template (an error only when the template is untagged)");
                return false;
            }
            self.pos += 1;
            return true;
        }
        if digits == 0 {
            self.report(1125, self.pos, self.pos);
        } else if value > 0x10ffff {
            self.report(1198, start, self.pos);
        }
        match self.cur() {
            None => self.report(1126, self.pos, self.pos),
            Some(b'}') => self.pos += 1,
            Some(_) => self.report(1199, self.pos, self.pos),
        }
        true
    }

    fn string(&mut self, quote: u8) {
        loop {
            let Some(c) = self.cur() else {
                self.report(1002, self.pos, self.pos);
                return;
            };
            if c == quote {
                self.pos += 1;
                return;
            }
            if c == b'\\' {
                self.pos += 1;
                if !self.escape(false) {
                    // `\` at the end of the file: TypeScript reports 1126 and no unterminated string.
                    if self.stop.is_none() {
                        self.pos = self.s.len();
                    }
                    return;
                }
                continue;
            }
            // U+2028 and U+2029 are allowed in a string (they end a `//` comment and a regular expression, not a string).
            if c == b'\n' || c == b'\r' {
                self.report(1002, self.pos, self.pos);
                return;
            }
            self.pos += 1;
        }
    }

    /// Scans template characters up to the closing backtick or a `${`, which is pushed on `templates`.
    fn template(&mut self, templates: &mut Vec<u32>) {
        loop {
            let Some(c) = self.cur() else {
                self.report(1160, self.pos, self.pos);
                return;
            };
            match c {
                b'`' => {
                    self.pos += 1;
                    return;
                }
                b'$' if self.b(self.pos + 1) == Some(b'{') => {
                    self.pos += 2;
                    templates.push(0);
                    return;
                }
                b'\\' => {
                    self.pos += 1;
                    if !self.escape(true) {
                        if self.stop.is_none() {
                            self.pos = self.s.len();
                        }
                        return;
                    }
                }
                _ => self.pos += 1,
            }
        }
    }

    // ---- regular expressions ----

    /// Where a regular expression starting at `start` would end (after the closing `/`), or `None` when it has no end on its line.
    fn regex_end(&self, start: usize) -> Option<usize> {
        let mut p = start + 1;
        let (mut escaped, mut class) = (false, false);
        loop {
            let c = self.b(p)?;
            if self.line_break_len(p) > 0 {
                return None;
            }
            if escaped {
                escaped = false;
            } else if c == b'/' && !class {
                return Some(p + 1);
            } else if c == b'[' {
                class = true;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b']' {
                class = false;
            }
            p += 1;
        }
    }

    fn regex(&mut self, start: usize) {
        match self.regex_end(start) {
            Some(end) => {
                self.pos = end;
                while self.cur().is_some_and(is_id_part) {
                    self.pos += 1;
                }
                if self.cur().is_some_and(|b| b >= 0x80 || b == b'\\') {
                    self.halt(self.pos, "a non-ASCII character or escape after a regular expression");
                }
                // The body and the flags are not validated: TypeScript 5.9 reports malformed regular expressions (1499 to 1520).
            }
            None => {
                // TypeScript 5.9 `reScanSlashToken`: the token of an unterminated expression ends at the first unbalanced closer of
                // the body and loses trailing whitespace and semicolons.
                let mut end_body = start + 1;
                let (mut escaped, mut class) = (false, false);
                while end_body < self.s.len() && self.line_break_len(end_body) == 0 {
                    let c = self.s[end_body];
                    if escaped {
                        escaped = false;
                    } else if c == b'[' {
                        class = true;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b']' {
                        class = false;
                    }
                    end_body += 1;
                }
                let _ = class;
                let mut p = start + 1;
                escaped = false;
                let (mut depth_class, mut in_quant, mut groups) = (0u32, false, 0u32);
                while p < end_body {
                    let c = self.s[p];
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'[' {
                        depth_class += 1;
                    } else if c == b']' && depth_class > 0 {
                        depth_class -= 1;
                    } else if depth_class == 0 {
                        if c == b'{' {
                            in_quant = true;
                        } else if c == b'}' && in_quant {
                            in_quant = false;
                        } else if !in_quant {
                            if c == b'(' {
                                groups += 1;
                            } else if c == b')' && groups > 0 {
                                groups -= 1;
                            } else if matches!(c, b')' | b']' | b'}') {
                                break;
                            }
                        }
                    }
                    p += 1;
                }
                // Trailing whitespace (ASCII only here; a wider character stops the scan) and semicolons are not part of the token.
                while p > start + 1 && matches!(self.s[p - 1], b' ' | b'\t' | 0x0b | 0x0c | b'\n' | b'\r' | b';') {
                    p -= 1;
                }
                self.pos = end_body;
                if self.s[start + 1..end_body].iter().any(|&b| b >= 0x80) {
                    self.halt(start, "non-ASCII text in an unterminated regular expression (its length depends on Unicode whitespace)");
                    return;
                }
                self.report(1161, start, p);
            }
        }
    }

    // ---- numbers ----

    /// TypeScript's `scanNumberFragment`: digits with `_` separators. True when it scanned any digit.
    fn fragment(&mut self) -> bool {
        let (mut allow, mut prev_sep, mut any) = (false, false, false);
        loop {
            match self.cur() {
                Some(b'_') => {
                    if allow {
                        allow = false;
                        prev_sep = true;
                    } else if prev_sep {
                        self.report(6189, self.pos, self.pos + 1);
                    } else {
                        self.report(6188, self.pos, self.pos + 1);
                    }
                    self.pos += 1;
                }
                Some(d) if d.is_ascii_digit() => {
                    allow = true;
                    prev_sep = false;
                    any = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if self.pos > 0 && self.s[self.pos - 1] == b'_' {
            self.report(6188, self.pos - 1, self.pos);
        }
        any
    }

    /// `scanHexDigits` and `scanBinaryOrOctalDigits`: digits of a radix with separators. True when it scanned any digit.
    fn radix_digits(&mut self, ok: fn(u8) -> bool) -> bool {
        let (mut allow, mut prev_sep, mut any) = (false, false, false);
        loop {
            let Some(c) = self.cur() else { break };
            if c == b'_' {
                if allow {
                    allow = false;
                    prev_sep = true;
                } else if prev_sep {
                    self.report(6189, self.pos, self.pos + 1);
                } else {
                    self.report(6188, self.pos, self.pos + 1);
                }
                self.pos += 1;
                continue;
            }
            allow = true;
            if !ok(c) {
                break;
            }
            any = true;
            prev_sep = false;
            self.pos += 1;
        }
        if self.pos > 0 && self.s[self.pos - 1] == b'_' {
            self.report(6188, self.pos - 1, self.pos);
        }
        any
    }

    /// TypeScript's `scan` for a numeric literal (the `0x`, `0b` and `0o` cases) and `scanNumber`.
    fn number(&mut self) {
        let start = self.pos;
        let minus = self.prev_minus;
        if self.s[start] == b'0' && start + 2 < self.s.len() {
            let radix: Option<(fn(u8) -> bool, u32)> = match self.s[start + 1] {
                b'x' | b'X' => Some((|c| c.is_ascii_hexdigit(), 1125)),
                b'b' | b'B' => Some((|c| c == b'0' || c == b'1', 1177)),
                b'o' | b'O' => Some((|c| (b'0'..=b'7').contains(&c), 1178)),
                _ => None,
            };
            if let Some((ok, code)) = radix {
                self.pos += 2;
                if !self.radix_digits(ok) {
                    self.report(code, self.pos, self.pos);
                }
                if self.cur() == Some(b'n') {
                    self.pos += 1;
                }
                return;
            }
        }
        let mut leading_zero = false;
        if self.s[start] == b'0' {
            self.pos += 1;
            if self.cur() == Some(b'_') {
                self.report(6188, self.pos, self.pos + 1);
                self.pos = start;
                self.fragment();
            } else {
                let digits_start = self.pos;
                let mut octal = true;
                while let Some(d) = self.cur().filter(u8::is_ascii_digit) {
                    octal &= d <= b'7';
                    self.pos += 1;
                }
                if self.pos == digits_start {
                    // just `0`
                } else if octal {
                    let digits = self.text[digits_start..self.pos].trim_start_matches('0');
                    let literal = format!("{}0o{}", if minus { "-" } else { "" }, if digits.is_empty() { "0" } else { digits });
                    let from = if minus { start - 1 } else { start };
                    self.report_with(1121, from, self.pos, Some(format!("Octal literals are not allowed. Use the syntax '{literal}'.")));
                    return;
                } else {
                    leading_zero = true;
                }
            }
        } else {
            self.fragment();
        }
        let mut decimal = false;
        let mut scientific = false;
        if self.cur() == Some(b'.') {
            self.pos += 1;
            decimal = true;
            self.fragment();
        }
        let mut end = self.pos;
        if matches!(self.cur(), Some(b'e' | b'E')) {
            self.pos += 1;
            scientific = true;
            if matches!(self.cur(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.fragment() {
                self.report(1124, self.pos, self.pos);
            } else {
                end = self.pos;
            }
        }
        if leading_zero {
            self.report(1489, start, end);
            return;
        }
        if decimal || scientific {
            self.after_number(start, !decimal && scientific);
        } else {
            if self.cur() == Some(b'n') {
                self.pos += 1;
            }
            self.after_number(start, false);
        }
    }

    /// `checkForIdentifierStartAfterNumericLiteral`.
    fn after_number(&mut self, num_start: usize, scientific: bool) {
        let id_start = self.pos;
        match self.cur() {
            Some(b) if is_id_start(b) => {}
            Some(b) if b >= 0x80 || b == b'\\' => {
                if self.char_at(self.pos).is_none_or(|c| !non_ascii_trivia(c)) {
                    self.halt(id_start, "a non-ASCII character or escape right after a number literal");
                }
                return;
            }
            _ => return,
        }
        let mut end = id_start;
        while self.b(end).is_some_and(is_id_part) {
            end += 1;
        }
        if self.b(end).is_some_and(|b| b >= 0x80 || b == b'\\') {
            self.halt(end, "a non-ASCII character or escape right after a number literal");
            return;
        }
        if end - id_start == 1 && self.s[id_start] == b'n' {
            self.report(if scientific { 1352 } else { 1353 }, num_start, id_start + 1);
            self.pos = id_start + 1;
        } else {
            self.report(1351, id_start, end);
            self.pos = id_start;
        }
    }
}

/// What a bare word does to the next `/`.
fn keyword_prev(word: &str) -> Prev {
    match word {
        "return" | "typeof" | "instanceof" | "in" | "new" | "delete" | "void" | "throw" | "case" | "do" | "else" | "default" | "extends" => Prev::Expr,
        "of" | "yield" | "await" => Prev::Either,
        _ => Prev::Operand,
    }
}

/// UTF-16 code units in `text[..byte]`.
fn utf16_offset(text: &str, byte: usize) -> u32 {
    let byte = byte.min(text.len());
    text[..byte].encode_utf16().count() as u32
}

/// Scans `text` as TypeScript and returns what TypeScript's scanner would report, as far as stage 1 can tell.
pub fn check_source(text: &str, _kind: FileKind) -> Report {
    let mut sc = Scanner { prev_minus: false, text, s: text.as_bytes(), pos: 0, diags: Vec::new(), stop: None, decided: false };
    sc.run();
    // One pass over the offsets: they are in increasing order except lengths, which are computed from the end offset.
    let diagnostics = sc
        .diags
        .iter()
        .map(|(code, start, end, verified, msg)| {
            let (code, start, end, verified) = (*code, *start, *end, *verified);
            let s = utf16_offset(text, start);
            Diagnostic { code, start: s, length: utf16_offset(text, end).saturating_sub(s), message: msg.clone().unwrap_or_else(|| message(code).to_string()), verified }
        })
        .collect();
    let stopped = sc.stop.map(|(at, reason)| Stop { at: utf16_offset(text, at), reason });
    Report { diagnostics, stopped }
}
