//! JSX: `<div className="a">text {expr} <b/></div>`.
//!
//! JSX is lexed differently from JavaScript, so this module reads it from the source text
//! directly: text between tags runs to the next `<` or `{` whatever it contains, attribute
//! strings have no escapes, and names may contain hyphens. Only the expressions inside braces
//! go back through the ordinary parser.

use super::*;

impl<'a> Parser<'a> {
    /// Cursor on the `<` that opens an element or a fragment. Reads through the closing
    /// `>`; if `in_child`, the cursor is left on that `>` so the parent can continue reading
    /// text from where it ends, otherwise the next token is read.
    pub(super) fn parse_jsx_element(&mut self, in_child: bool) -> PResult<NodeId> {
        self.ast.uses_jsx = true;
        let start = self.tok.start;
        let saved = std::mem::replace(&mut self.in_type, true);
        let result = self.parse_jsx_element_inner(start);
        self.in_type = saved;
        let node = result?;
        if !in_child {
            self.next()?;
        }
        Ok(node)
    }

    fn parse_jsx_element_inner(&mut self, start: u32) -> PResult<NodeId> {
        self.next()?; // `<`
        if self.at(Punct::Gt) {
            let children = self.parse_jsx_children(self.tok.end, None)?;
            self.prev_end = self.tok.end;
            return Ok(self.finish(NodeKind::JsxFragment { children }, start));
        }
        let name_start = self.tok.start;
        let tag = self.parse_jsx_tag()?;
        let name_text = &self.src[name_start as usize..self.prev_end as usize];
        if self.ts && self.at(Punct::Lt) {
            // `<Component<string> prop />`
            self.ts_type_args_erase()?;
        }
        let mark = self.mark();
        let self_closing = loop {
            match self.tok.kind {
                Kind::Punct(Punct::Slash) => {
                    self.next()?;
                    if !self.at(Punct::Gt) {
                        return Err(self.unexpected_expecting(">"));
                    }
                    break true;
                }
                Kind::Punct(Punct::Gt) => break false,
                Kind::Punct(Punct::LBrace) => {
                    let attr_start = self.tok.start;
                    self.in_type = false;
                    self.next()?;
                    if !self.at(Punct::Ellipsis) {
                        return Err(self.unexpected_expecting("..."));
                    }
                    self.next()?;
                    let expr = self.allow_in(|p| p.parse_assign());
                    self.in_type = true;
                    let expr = expr?;
                    if !self.at(Punct::RBrace) {
                        return Err(self.unexpected_expecting("}"));
                    }
                    self.next()?;
                    let attr = self.finish(NodeKind::JsxSpreadAttr(expr), attr_start);
                    self.push(attr);
                }
                Kind::Ident | Kind::Keyword(_) => {
                    let attr = self.parse_jsx_attribute()?;
                    self.push(attr);
                }
                _ => return Err(self.unexpected()),
            }
        };
        let attrs = self.take_list(mark);
        let children = if self_closing {
            List::default()
        } else {
            let children = self.parse_jsx_children(self.tok.end, Some(name_text))?;
            children
        };
        self.prev_end = self.tok.end;
        let data = self.ast.alloc_jsx(crate::ast::JsxData { tag, attrs, children });
        Ok(self.finish(NodeKind::JsxElement(data), start))
    }

    /// A tag name: `div`, `my-element`, `ns:name`, `Foo`, `a.b.C` or `this.Foo`. Lowercase
    /// plain names are intrinsic elements and become strings; the rest are references.
    fn parse_jsx_tag(&mut self) -> PResult<NodeId> {
        let first = self.tok;
        if !matches!(first.kind, Kind::Ident | Kind::Keyword(_)) {
            return Err(self.unexpected());
        }
        let end = self.jsx_name_end(first.start);
        let text = &self.src[first.start as usize..end as usize];
        // `ns:name` and hyphenated names are always intrinsic.
        let namespaced = self.src.as_bytes().get(end as usize) == Some(&b':');
        let intrinsic_start = text.starts_with(|c: char| c.is_ascii_lowercase()) || text.contains('-');
        let dotted = self.src.as_bytes().get(end as usize) == Some(&b'.');
        if namespaced {
            let full_end = self.jsx_name_end_after_colon(end);
            let atom = self.ast.intern(&self.src[first.start as usize..full_end as usize]);
            self.jsx_relex_after(full_end)?;
            return Ok(self.finish(NodeKind::Str(atom), first.start));
        }
        if intrinsic_start && !dotted && first.kind != Kind::Keyword(Kw::This) {
            let atom = self.ast.intern(text);
            self.jsx_relex_after(end)?;
            return Ok(self.finish(NodeKind::Str(atom), first.start));
        }
        // A reference: the first name, then `.member`s.
        let mut node = if first.kind == Kind::Keyword(Kw::This) {
            self.next()?;
            self.finish(NodeKind::This, first.start)
        } else {
            let name = self.ast.intern(text);
            self.jsx_relex_after(end)?;
            let ident = self.finish(NodeKind::Ident(name), first.start);
            self.record_ref(ident);
            ident
        };
        while self.at(Punct::Dot) {
            self.next()?;
            let tok = self.tok;
            if !matches!(tok.kind, Kind::Ident | Kind::Keyword(_)) {
                return Err(self.unexpected());
            }
            let name = self.name_of(tok);
            self.next()?;
            let prop = self.finish(NodeKind::PropName(name), tok.start);
            node = self.finish(NodeKind::Member { object: node, prop }, first.start);
        }
        Ok(node)
    }

    /// One past the last character of a name starting at `start`: letters, digits, `_`, `$`
    /// and `-`, which JSX names may contain beyond JavaScript identifiers.
    fn jsx_name_end(&self, start: u32) -> u32 {
        let rest = &self.src[start as usize..];
        let len = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '-')).unwrap_or(rest.len());
        start + len as u32
    }

    fn jsx_name_end_after_colon(&self, colon: u32) -> u32 {
        self.jsx_name_end(colon + 1)
    }

    /// Continues tokenizing at `pos`, after a name the raw reader consumed.
    fn jsx_relex_after(&mut self, pos: u32) -> PResult<()> {
        self.prev_end = pos;
        self.lexer.reset(pos);
        self.tok = self.lexer.next_token()?;
        Ok(())
    }

    fn parse_jsx_attribute(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let mut end = self.jsx_name_end(start);
        // `xlink:href`
        if self.src.as_bytes().get(end as usize) == Some(&b':') {
            end = self.jsx_name_end_after_colon(end);
        }
        let name = self.ast.intern(&self.src[start as usize..end as usize]);
        self.jsx_relex_after(end)?;
        if !self.at(Punct::Eq) {
            return Ok(self.finish(NodeKind::JsxAttr { name, value: None }, start));
        }
        // After `=`: a quoted string (no escapes), `{expression}`, or an element. The text is
        // read raw so the string is not tokenized as JavaScript.
        let bytes = self.src.as_bytes();
        let mut pos = self.tok.end as usize;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let value = match bytes.get(pos) {
            Some(&quote @ (b'"' | b'\'')) => {
                let close = self.src[pos + 1..].find(quote as char).ok_or_else(|| self.error_at("Unterminated JSX string", pos as u32))? + pos + 1;
                let decoded = decode_jsx_entities(&self.src[pos + 1..close]);
                let atom = self.ast.intern(&decoded);
                let node = self.ast.alloc(NodeKind::Str(atom), Span { start: pos as u32, end: close as u32 + 1 }, 0);
                self.jsx_relex_after(close as u32 + 1)?;
                node
            }
            Some(b'{') => {
                self.lexer.reset(pos as u32);
                self.tok = self.lexer.next_token()?;
                self.in_type = false;
                self.next()?;
                let expr = self.allow_in(|p| p.parse_assign());
                self.in_type = true;
                let expr = expr?;
                if !self.at(Punct::RBrace) {
                    return Err(self.unexpected_expecting("}"));
                }
                self.next()?;
                expr
            }
            Some(b'<') => {
                self.lexer.reset(pos as u32);
                self.tok = self.lexer.next_token()?;
                self.parse_jsx_element(false)?
            }
            _ => return Err(self.error_at("Expected a JSX attribute value", pos as u32)),
        };
        Ok(self.finish(NodeKind::JsxAttr { name, value: Some(value) }, start))
    }

    /// Children, from `pos` (just after the `>` of the opening tag) through the closing tag.
    /// Leaves the cursor on the closing tag's `>`.
    fn parse_jsx_children(&mut self, mut pos: u32, open_name: Option<&str>) -> PResult<List> {
        let mark = self.mark();
        let bytes = self.src.as_bytes();
        loop {
            let text_start = pos;
            while (pos as usize) < bytes.len() && bytes[pos as usize] != b'<' && bytes[pos as usize] != b'{' {
                pos += 1;
            }
            if pos as usize >= bytes.len() {
                return Err(self.error_at("Unterminated JSX contents", text_start));
            }
            if pos > text_start {
                if let Some(cleaned) = clean_jsx_text(&self.src[text_start as usize..pos as usize]) {
                    let atom = self.ast.intern(&cleaned);
                    let node = self.ast.alloc(NodeKind::JsxText(atom), Span { start: text_start, end: pos }, 0);
                    self.push(node);
                }
            }
            self.lexer.reset(pos);
            self.tok = self.lexer.next_token()?;
            if self.at(Punct::LBrace) {
                let container = self.tok.start;
                self.in_type = false;
                self.next()?;
                let item = if self.at(Punct::RBrace) {
                    None
                } else if self.at(Punct::Ellipsis) {
                    self.next()?;
                    let expr = self.allow_in(|p| p.parse_expression());
                    match expr {
                        Ok(e) => Some(self.finish(NodeKind::Spread(e), container)),
                        Err(e) => {
                            self.in_type = true;
                            return Err(e);
                        }
                    }
                } else {
                    match self.allow_in(|p| p.parse_expression()) {
                        Ok(e) => Some(e),
                        Err(e) => {
                            self.in_type = true;
                            return Err(e);
                        }
                    }
                };
                self.in_type = true;
                if !self.at(Punct::RBrace) {
                    return Err(self.unexpected_expecting("}"));
                }
                if let Some(item) = item {
                    self.push(item);
                }
                pos = self.tok.end;
                continue;
            }
            // A `<`: a closing tag ends the children, anything else is a nested element.
            let after = self.src[pos as usize + 1..].trim_start();
            if after.starts_with('/') {
                let slash = self.src[pos as usize + 1..].find('/').unwrap() as u32 + pos + 1;
                let name_from = {
                    let rest = &self.src[slash as usize + 1..];
                    slash + 1 + (rest.len() - rest.trim_start().len()) as u32
                };
                let closing_end = {
                    let mut e = name_from;
                    let rest = &self.src[e as usize..];
                    let len = rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '-' | '.' | ':'))).unwrap_or(rest.len());
                    e += len as u32;
                    e
                };
                let closing = &self.src[name_from as usize..closing_end as usize];
                if Some(closing) != open_name.or(Some("")) {
                    let message = match open_name {
                        Some(open) => format!("Expected a closing tag for `<{open}>`"),
                        None => "Expected a closing fragment `</>`".to_string(),
                    };
                    return Err(self.error_at(message, pos));
                }
                let gt = self.src[closing_end as usize..].find('>').ok_or_else(|| self.error_at("Unterminated closing tag", pos))? as u32 + closing_end;
                self.lexer.reset(gt);
                self.tok = self.lexer.next_token()?;
                self.prev_end = self.tok.end;
                return Ok(self.take_list(mark));
            }
            let child = self.parse_jsx_element(true)?;
            self.push(child);
            pos = self.tok.end;
        }
    }

    /// Whether a `<` at the start of an expression begins a generic arrow in a `.tsx` file
    /// (`<T,>(x) => x` or `<T extends U>(x) => x`) and not an element.
    pub(super) fn jsx_generic_arrow_ahead(&mut self) -> bool {
        if !self.ts {
            return false;
        }
        let looks_generic = self
            .ts_look(|p| {
                p.next()?;
                if p.tok.kind != Kind::Ident {
                    return Ok(false);
                }
                p.next()?;
                Ok(p.at(Punct::Comma) || p.at_kw(Kw::Extends))
            })
            .unwrap_or(false);
        looks_generic && self.ts_arrow_ahead(true)
    }
}

/// React's reading of the text between tags: lines are trimmed, blank lines dropped, and the
/// rest joined with single spaces. Returns `None` if nothing is left.
fn clean_jsx_text(raw: &str) -> Option<String> {
    // As Babel does it: tabs are spaces, the lines are trimmed (the first keeps its start, the last its end), and the empty ones go. A text of
    // only spaces on one line is kept: `<a/> <b/>` has a space between the elements.
    let raw = raw.replace('\t', " ");
    let lines: Vec<&str> = raw.split(['\n']).map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let last_non_empty = lines.iter().rposition(|l| l.contains(|c: char| c != ' ')).unwrap_or(0);
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let mut text = *line;
        if i != 0 {
            text = text.trim_start_matches(' ');
        }
        if i != lines.len() - 1 {
            text = text.trim_end_matches(' ');
        }
        if text.is_empty() {
            continue;
        }
        out.push_str(text);
        if i != last_non_empty {
            out.push(' ');
        }
    }
    if out.is_empty() { None } else { Some(decode_jsx_entities(&out)) }
}

/// `&amp;`, `&#65;`, `&#x41;` and the common named entities. Unknown ones stay as written.
fn decode_jsx_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let name = &rest[1..semi];
            let ch = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse::<u32>().ok(),
                };
                code.and_then(char::from_u32)
            } else {
                named_entity(name)
            };
            ch.map(|c| (c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "quot" => '"', "amp" => '&', "apos" => '\'', "lt" => '<', "gt" => '>', "nbsp" => '\u{a0}', "iexcl" => '¡', "cent" => '¢',
        "pound" => '£', "curren" => '¤', "yen" => '¥', "brvbar" => '¦', "sect" => '§', "uml" => '¨', "copy" => '©', "ordf" => 'ª',
        "laquo" => '«', "not" => '¬', "shy" => '\u{ad}', "reg" => '®', "macr" => '¯', "deg" => '°', "plusmn" => '±', "sup2" => '²',
        "sup3" => '³', "acute" => '´', "micro" => 'µ', "para" => '¶', "middot" => '·', "cedil" => '¸', "sup1" => '¹', "ordm" => 'º',
        "raquo" => '»', "frac14" => '¼', "frac12" => '½', "frac34" => '¾', "iquest" => '¿', "times" => '×', "divide" => '÷',
        "ndash" => '–', "mdash" => '—', "lsquo" => '‘', "rsquo" => '’', "ldquo" => '“', "rdquo" => '”', "bull" => '•', "hellip" => '…',
        "trade" => '™', "larr" => '←', "uarr" => '↑', "rarr" => '→', "darr" => '↓', "harr" => '↔', "euro" => '€', "check" => '✓',
        "ne" => '≠', "le" => '≤', "ge" => '≥', "infin" => '∞', "sum" => '∑', "minus" => '−', "radic" => '√', "asymp" => '≈',
        "alpha" => 'α', "beta" => 'β', "gamma" => 'γ', "delta" => 'δ', "pi" => 'π', "sigma" => 'σ', "omega" => 'ω', "lambda" => 'λ',
        "mu" => 'μ', "theta" => 'θ', "Omega" => 'Ω', "Delta" => 'Δ', "Sigma" => 'Σ', "hearts" => '♥', "spades" => '♠', "clubs" => '♣',
        "diams" => '♦', "star" => '☆', "starf" => '★', "zwnj" => '\u{200c}', "zwj" => '\u{200d}', "ensp" => '\u{2002}', "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}', "agrave" => 'à', "aacute" => 'á', "acirc" => 'â', "atilde" => 'ã', "auml" => 'ä', "aring" => 'å',
        "aelig" => 'æ', "ccedil" => 'ç', "egrave" => 'è', "eacute" => 'é', "ecirc" => 'ê', "euml" => 'ë', "igrave" => 'ì', "iacute" => 'í',
        "icirc" => 'î', "iuml" => 'ï', "ntilde" => 'ñ', "ograve" => 'ò', "oacute" => 'ó', "ocirc" => 'ô', "otilde" => 'õ', "ouml" => 'ö',
        "oslash" => 'ø', "ugrave" => 'ù', "uacute" => 'ú', "ucirc" => 'û', "uuml" => 'ü', "yacute" => 'ý', "szlig" => 'ß', "yuml" => 'ÿ',
        "Agrave" => 'À', "Aacute" => 'Á', "Acirc" => 'Â', "Atilde" => 'Ã', "Auml" => 'Ä', "Aring" => 'Å', "AElig" => 'Æ', "Ccedil" => 'Ç',
        "Egrave" => 'È', "Eacute" => 'É', "Ecirc" => 'Ê', "Euml" => 'Ë', "Igrave" => 'Ì', "Iacute" => 'Í', "Icirc" => 'Î', "Iuml" => 'Ï',
        "Ntilde" => 'Ñ', "Ograve" => 'Ò', "Oacute" => 'Ó', "Ocirc" => 'Ô', "Otilde" => 'Õ', "Ouml" => 'Ö', "Oslash" => 'Ø', "Ugrave" => 'Ù',
        "Uacute" => 'Ú', "Ucirc" => 'Û', "Uuml" => 'Ü', "Yacute" => 'Ý',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::clean_jsx_text;

    #[test]
    fn text_between_elements_is_cleaned_as_babel_does() {
        // A space on one line is a space; lines are trimmed and joined; tabs are spaces; empty text is nothing.
        assert_eq!(clean_jsx_text(" ").as_deref(), Some(" "));
        assert_eq!(clean_jsx_text("  \t ").as_deref(), Some("    "));
        assert_eq!(clean_jsx_text("\n   \n").as_deref(), None);
        assert_eq!(clean_jsx_text("\n  one\n  two \n").as_deref(), Some("one two"));
        assert_eq!(clean_jsx_text("a\tb").as_deref(), Some("a b"));
        assert_eq!(clean_jsx_text(" x ").as_deref(), Some(" x "));
    }
}
