//! CSS for the bundler: finding what a style sheet loads, inlining `@import`, pointing `url()`
//! at the bundled assets, local class names for `.module.css`, and shrinking the text.
//!
//! The sheet is not parsed into a tree. One pass over the text finds the pieces that change
//! (imports, `url()` specifiers, class names in selectors, `composes`) and everything between
//! them is copied as it is.

use std::collections::HashMap;

/// What a style sheet asks to load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefKind {
    /// `composes: a from "./other.module.css"`: the other sheet is loaded for its class names.
    Compose,
    /// `@import "x"` or `@import url(x)`.
    Import,
    /// `url(x)` in a value.
    Url,
}

#[derive(Clone, Debug)]
pub struct Ref {
    pub kind: RefKind,
    /// As written, except that a bare `x` in `url(x)` is `./x`.
    pub specifier: String,
    /// For an import: what follows the specifier (`screen`, `layer(a)`, `supports(...)`).
    pub conditions: String,
    /// For a `url()`: the query and fragment that go back after the address of the bundled file (`?#iefix`, `#icon`).
    pub suffix: String,
}

#[derive(Debug)]
enum Piece {
    Raw(usize, usize),
    Comment(usize, usize),
    /// A reference; `Import` pieces stand for the whole rule, `Url` pieces for the specifier text.
    Ref(usize, usize, usize),
    Local(usize, usize),
    Keyframes(usize, usize),
    /// `animation` / `animation-name` values: names that may be keyframes.
    Animation(usize, usize),
    /// Text that is left out (`:global(` and its `)`, a `composes` declaration).
    Drop(usize, usize),
    /// An `@import` of a URL: the browser loads it, so it goes first in the output.
    Hoist(usize, usize),
    Composes(Vec<(usize, usize)>),
    /// `composes: a b from "file"`: the words, and the reference to the file.
    ComposesFrom(Vec<(usize, usize)>, usize),
    /// A use of a name `@value` defines.
    Value(usize, usize),
    /// `@value a, b as c from "file"`: (name there, name here) for each, and the reference to the file; the rule is left out.
    ValueFrom(Vec<(String, String)>, usize),
    /// `:export { a: b }`: the pairs, and the rule is left out.
    Export(Vec<(String, String)>),
}

pub struct Parsed {
    pieces: Vec<Piece>,
    pub refs: Vec<Ref>,
    /// What the sheet's own `@value` rules define (a later one may use an earlier one), as `(name, value)`.
    pub values: Vec<(String, String)>,
}

fn is_ident_char(c: char) -> bool {
    c == '-' || c == '_' || c.is_ascii_alphanumeric() || !c.is_ascii()
}

fn is_ident_start(c: char) -> bool {
    c == '-' || c == '_' || c.is_ascii_alphabetic() || !c.is_ascii() || c == '\\'
}

/// A specifier worth loading: not a URL with a scheme, not `data:`, not a fragment.
fn is_loadable(spec: &str) -> bool {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with('#') || spec.starts_with("//") {
        return false;
    }
    let scheme = spec.split(|c: char| c == '/' || c == '?').next().unwrap_or("");
    !(scheme.contains(':') && !scheme.starts_with('.'))
}

/// CSS escapes undone: `a\.png` is `a.png`, `\41 b` is `Ab`.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some(h) if h.is_ascii_hexdigit() => {
                let mut code = 0u32;
                let mut n = 0;
                while n < 6 {
                    match chars.peek().and_then(|d| d.to_digit(16)) {
                        Some(d) => {
                            code = code * 16 + d;
                            chars.next();
                            n += 1;
                        }
                        None => break,
                    }
                }
                if chars.peek().is_some_and(|w| w.is_ascii_whitespace()) {
                    chars.next();
                }
                out.push(char::from_u32(code).filter(|&c| c != '\0').unwrap_or('\u{fffd}'));
            }
            Some('\n') => {
                chars.next();
            }
            Some(other) => {
                out.push(other);
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// What a `url()` asks for, as `(specifier, suffix)`: the file is looked for without its `#fragment` and `?query`, which go back after the
/// address of the bundled file (`font.eot?#iefix`, `icons.svg#home`). A query that picks a loader (`?raw`, `?url`, `?inline`) belongs to the
/// specifier. A leading `~` (the old way to say "a package") is dropped, and a bare `x` is `./x`.
fn url_target(spec: &str) -> Option<(String, String)> {
    let (rest, fragment) = match spec.find('#') {
        Some(p) => (&spec[..p], &spec[p..]),
        None => (spec, ""),
    };
    let (path, query) = match rest.find('?') {
        Some(p) => (&rest[..p], &rest[p..]),
        None => (rest, ""),
    };
    if path.trim().is_empty() {
        return None;
    }
    let loader_query = query.len() > 1 && query[1..].split('&').any(|part| matches!(part.split('=').next(), Some("raw" | "url" | "inline")));
    let (path, suffix) = if loader_query { (format!("{path}{query}"), fragment.to_string()) } else { (path.to_string(), format!("{query}{fragment}")) };
    let specifier = if let Some(package) = path.strip_prefix('~').filter(|p| !p.starts_with('/')) {
        package.to_string()
    } else if path.starts_with("./") || path.starts_with("../") || path.starts_with('/') {
        path
    } else {
        format!("./{path}")
    };
    Some((specifier, suffix))
}

/// The wrappers an `@import`'s conditions stand for, outermost first: `layer(base) supports(display: grid) screen` is
/// `@layer base`, `@supports (display: grid)` and `@media screen`.
pub fn import_wrappers(conditions: &str) -> Vec<String> {
    let mut rest = conditions.trim();
    let mut out = Vec::new();
    let balanced = |text: &str| -> Option<usize> {
        // `text` starts after the `(`: where its `)` is.
        let mut depth = 1;
        for (i, c) in text.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    };
    if starts_word(rest, "layer") {
        let after = &rest[5..];
        if let Some(inner) = after.strip_prefix('(') {
            if let Some(close) = balanced(inner) {
                out.push(format!("@layer {}", inner[..close].trim()));
                rest = inner[close + 1..].trim_start();
            }
        } else {
            out.push("@layer".to_string());
            rest = after.trim_start();
        }
    }
    if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("supports(") {
        let inner = &rest[9..];
        if let Some(close) = balanced(inner) {
            let condition = inner[..close].trim();
            let lower = condition.to_ascii_lowercase();
            let is_condition = condition.starts_with('(') || (lower.starts_with("not") && condition[3..].starts_with(|c: char| c.is_whitespace() || c == '(')) || ["selector(", "font-tech(", "font-format("].iter().any(|f| lower.starts_with(f));
            out.push(if is_condition { format!("@supports {condition}") } else { format!("@supports ({condition})") });
            rest = inner[close + 1..].trim_start();
        }
    }
    if !rest.is_empty() {
        out.push(format!("@media {rest}"));
    }
    out
}

/// Splits `text`. With `local`, class names, ids and keyframe names in selectors are marked.
pub fn parse(text: &str, local: bool) -> Result<Parsed, String> {
    let bytes = text.as_bytes();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut refs: Vec<Ref> = Vec::new();
    // `@value` names: the ones this sheet defines with what they stand for, and every name a use may be of (those imported too).
    let mut values: Vec<(String, String)> = Vec::new();
    let mut value_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    // The paren depth of an open `image-set(`, where a bare string is a `url()`.
    let mut image_set: Option<usize> = None;
    let mut raw_from = 0;
    let mut i = 0;
    // What a selector would change, found since the last `;`, `{` or `}`; it counts only if a
    // `{` follows.
    let mut pending: Vec<Piece> = Vec::new();
    let mut statement_start = 0;
    // `:global(` / `:local(` groups open: (paren depth, global). `:global` alone holds to the next `,` or `{`.
    let mut wrappers: Vec<(usize, bool)> = Vec::new();
    let mut global_bare = false;
    let mut parens = 0usize;
    let mut depth = 0usize;
    macro_rules! flush {
        ($upto:expr) => {
            if $upto > raw_from {
                pieces.push(Piece::Raw(raw_from, $upto));
            }
        };
    }
    // Moves the pending pieces into the output, in order.
    macro_rules! commit {
        () => {
            for p in pending.drain(..) {
                let (s, e) = match &p {
                    Piece::Local(s, e) | Piece::Keyframes(s, e) | Piece::Drop(s, e) => (*s, *e),
                    _ => unreachable!("only selector pieces wait"),
                };
                flush!(s);
                pieces.push(p);
                raw_from = e;
            }
        };
    }
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..].find("*/").map(|p| i + 2 + p + 2).ok_or_else(|| format!("unterminated comment at {i}"))?;
                // A comment inside a selector does not hold the pending pieces back.
                commit_if_ordered(&mut pending);
                flush!(i);
                pieces.push(Piece::Comment(i, end));
                i = end;
                raw_from = i;
            }
            b'"' | b'\'' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != c {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let j = j.min(bytes.len());
                // `image-set("a.png" 1x)`: a string there is a file, as `url("a.png")` would be.
                if image_set == Some(parens) {
                    let decoded = unescape(&text[i + 1..j]);
                    if let Some((specifier, suffix)) = url_target(&decoded).filter(|_| is_loadable(&decoded)) {
                        flush!(i + 1);
                        refs.push(Ref { kind: RefKind::Url, specifier, conditions: String::new(), suffix });
                        pieces.push(Piece::Ref(refs.len() - 1, i + 1, j));
                        raw_from = j;
                    }
                }
                i = (j + 1).min(bytes.len());
            }
            b'(' => {
                parens += 1;
                let before = text[..i].to_ascii_lowercase();
                if before.ends_with("image-set") && image_set.is_none() {
                    image_set = Some(parens);
                }
                i += 1;
            }
            b')' => {
                if image_set == Some(parens) {
                    image_set = None;
                }
                if wrappers.last().is_some_and(|w| w.0 == parens) {
                    wrappers.pop();
                    pending.push(Piece::Drop(i, i + 1));
                }
                parens = parens.saturating_sub(1);
                i += 1;
            }
            b'@' if text[statement_start..i].trim().is_empty() => {
                let mut j = i + 1;
                while j < bytes.len() && is_ident_char(bytes[j] as char) {
                    j += 1;
                }
                let name = text[i + 1..j].to_ascii_lowercase();
                if name == "charset" {
                    // The one `@charset` goes first in the output, where the browser looks for it.
                    let end = find_rule_end(text, j).unwrap_or(bytes.len());
                    let stop = (end + 1).min(bytes.len());
                    flush!(i);
                    pieces.push(Piece::Hoist(i, stop));
                    raw_from = stop;
                    i = stop;
                    statement_start = i;
                } else if local && name == "value" {
                    let end = value_end(text, j);
                    let body = text[j..end].trim();
                    let stop = if end < bytes.len() && bytes[end] == b';' { end + 1 } else { end };
                    flush!(i);
                    if let Some((names, file)) = body.rsplit_once(" from ").map(|(n, f)| (n, f.trim().trim_matches(|c| c == '"' || c == '\''))) {
                        let pairs: Vec<(String, String)> = names
                            .split(',')
                            .filter_map(|n| {
                                let n = n.trim();
                                let (there, here) = n.split_once(" as ").map_or((n, n), |(a, b)| (a.trim(), b.trim()));
                                (!there.is_empty()).then(|| (there.to_string(), here.to_string()))
                            })
                            .collect();
                        for (_, here) in &pairs {
                            value_names.insert(here.clone());
                        }
                        refs.push(Ref { kind: RefKind::Compose, specifier: file.to_string(), conditions: String::new(), suffix: String::new() });
                        pieces.push(Piece::ValueFrom(pairs, refs.len() - 1));
                    } else if let Some((name, value)) = body.split_once(':') {
                        let name = name.trim().to_string();
                        // A value may use the ones defined before it.
                        let mut resolved = String::new();
                        let mut last = 0;
                        let v = value.trim();
                        let vb = v.as_bytes();
                        let mut k = 0;
                        while k < vb.len() {
                            if is_ident_start(vb[k] as char) && (k == 0 || !is_ident_char(vb[k - 1] as char)) {
                                let mut e = k;
                                while e < vb.len() && is_ident_char(vb[e] as char) {
                                    e += 1;
                                }
                                if let Some((_, known)) = values.iter().rev().find(|(n, _)| n == &v[k..e]) {
                                    resolved.push_str(&v[last..k]);
                                    resolved.push_str(known);
                                    last = e;
                                }
                                k = e;
                            } else {
                                k += 1;
                            }
                        }
                        resolved.push_str(&v[last..]);
                        value_names.insert(name.clone());
                        values.push((name, resolved));
                    }
                    pieces.push(Piece::Drop(i, stop));
                    raw_from = stop;
                    i = stop;
                    statement_start = i;
                } else if name == "import" {
                    let (spec, after) = import_target(text, j)?;
                    let spec = match spec.strip_prefix('~') {
                        Some(package) if !package.starts_with('/') => package.to_string(),
                        _ => spec,
                    };
                    let end = find_rule_end(text, after).ok_or_else(|| format!("@import at {i} has no end"))?;
                    let conditions = text[after..end].trim().to_string();
                    let stop = (end + 1).min(bytes.len());
                    flush!(i);
                    if is_loadable(&spec) {
                        refs.push(Ref { kind: RefKind::Import, specifier: spec, conditions, suffix: String::new() });
                        pieces.push(Piece::Ref(refs.len() - 1, i, stop));
                    } else {
                        pieces.push(Piece::Hoist(i, stop));
                    }
                    raw_from = stop;
                    i = stop;
                    statement_start = i;
                } else if local && name.ends_with("keyframes") {
                    let mut k = j;
                    while k < bytes.len() && (bytes[k] as char).is_whitespace() {
                        k += 1;
                    }
                    let mut e = k;
                    while e < bytes.len() && is_ident_char(bytes[e] as char) {
                        e += 1;
                    }
                    if e > k {
                        pending.push(Piece::Keyframes(k, e));
                    }
                    i = e.max(j);
                } else {
                    i = j;
                }
            }
            b'u' | b'U' if text[i..].len() >= 4 && text[i..i + 4].eq_ignore_ascii_case("url(") && (i == 0 || !is_ident_char(bytes[i - 1] as char)) => {
                let mut j = i + 4;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                let (start, end, close) = if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    let quote = bytes[j];
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k] != quote {
                        if bytes[k] == b'\\' {
                            k += 1;
                        }
                        k += 1;
                    }
                    (j + 1, k.min(bytes.len()), k + 1)
                } else {
                    let mut k = j;
                    while k < bytes.len() && bytes[k] != b')' {
                        if bytes[k] == b'\\' {
                            k += 1;
                        }
                        k += 1;
                    }
                    let k = k.min(bytes.len());
                    (j, j + text[j..k].trim_end().len(), k)
                };
                let decoded = unescape(&text[start..end]);
                if let Some((specifier, suffix)) = url_target(&decoded).filter(|_| is_loadable(&decoded)) {
                    flush!(start);
                    refs.push(Ref { kind: RefKind::Url, specifier, conditions: String::new(), suffix });
                    pieces.push(Piece::Ref(refs.len() - 1, start, end));
                    raw_from = end;
                }
                // Past the closing `)`.
                i = close.min(bytes.len());
                while i < bytes.len() && bytes[i] != b')' {
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
            }
            b'{' => {
                depth += 1;
                commit!();
                statement_start = i + 1;
                wrappers.clear();
                global_bare = false;
                i += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                pending.clear();
                statement_start = i + 1;
                global_bare = false;
                i += 1;
            }
            b';' => {
                pending.clear();
                statement_start = i + 1;
                global_bare = false;
                i += 1;
            }
            b',' => {
                global_bare = false;
                i += 1;
            }
            b':' if local && depth == 0 && starts_word(&text[i + 1..], "export") && text[statement_start..i].trim().is_empty() && text[i + 7..].trim_start().starts_with('{') => {
                // ICSS `:export { name: value }`: the pairs are exported with the class names, and the rule is not written.
                let open = i + 7 + text[i + 7..].find('{').unwrap_or(0);
                let close = text[open..].find('}').map_or(bytes.len(), |p| open + p);
                let pairs: Vec<(String, String)> = text[open + 1..close]
                    .split(';')
                    .filter_map(|decl| decl.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                    .filter(|(k, _)| !k.is_empty())
                    .collect();
                let stop = (close + 1).min(bytes.len());
                flush!(i);
                pieces.push(Piece::Export(pairs));
                pieces.push(Piece::Drop(i, stop));
                raw_from = stop;
                i = stop;
                statement_start = i;
            }
            b':' if local && (starts_word(&text[i + 1..], "global") || starts_word(&text[i + 1..], "local")) => {
                let global = starts_word(&text[i + 1..], "global");
                let after = i + 1 + if global { 6 } else { 5 };
                if bytes.get(after) == Some(&b'(') {
                    parens += 1;
                    wrappers.push((parens, global));
                    pending.push(Piece::Drop(i, after + 1));
                    i = after + 1;
                } else if global {
                    let mut e = after;
                    while e < bytes.len() && (bytes[e] as char).is_whitespace() {
                        e += 1;
                    }
                    pending.push(Piece::Drop(i, e));
                    global_bare = true;
                    i = e;
                } else {
                    i += 1;
                }
            }
            b'.' | b'#' if local && i + 1 < bytes.len() && is_ident_start(bytes[i + 1] as char) => {
                let mut j = i + 1;
                while j < bytes.len() && (is_ident_char(bytes[j] as char) || bytes[j] == b'\\') {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let j = j.min(bytes.len());
                let global = global_bare || wrappers.last().is_some_and(|w| w.1);
                if !global {
                    pending.push(Piece::Local(i + 1, j));
                }
                i = j;
            }
            _ if local && is_ident_start(c as char) && (i == 0 || !is_ident_char(bytes[i - 1] as char)) && (depth > 0 || !value_names.is_empty()) => {
                let mut j = i;
                while j < bytes.len() && is_ident_char(bytes[j] as char) {
                    j += 1;
                }
                let j = j.max(i + 1);
                let word = text[i..j].to_ascii_lowercase();
                let at_start = text[statement_start..i].trim().is_empty();
                let after = text[j..].trim_start();
                if value_names.contains(&text[i..j]) && !(at_start && after.starts_with(':')) && !(depth > 0 && text[statement_start..i].trim_end().ends_with(['.', '#'])) {
                    flush!(i);
                    pieces.push(Piece::Value(i, j));
                    raw_from = j;
                    i = j;
                } else if depth == 0 {
                    i = j;
                } else if at_start && after.starts_with(':') && (word == "animation" || word == "animation-name") {
                    // Names in the value are keyframes if the sheet defines them.
                    let colon = j + text[j..].find(':').unwrap_or(0);
                    let end = value_end(text, colon + 1);
                    let mut k = colon + 1;
                    while k < end {
                        if bytes[k] == b'"' || bytes[k] == b'\'' {
                            let q = bytes[k];
                            k += 1;
                            while k < end && bytes[k] != q {
                                k += 1;
                            }
                            k += 1;
                        } else if is_ident_start(bytes[k] as char) && !is_ident_char(bytes[k - 1] as char) {
                            let mut e = k;
                            while e < end && is_ident_char(bytes[e] as char) {
                                e += 1;
                            }
                            flush!(k);
                            pieces.push(Piece::Animation(k, e));
                            raw_from = e;
                            k = e;
                        } else {
                            k += 1;
                        }
                    }
                    i = end;
                } else if at_start && after.starts_with(':') && word == "composes" {
                    let colon = j + text[j..].find(':').unwrap_or(0);
                    let end = value_end(text, colon + 1);
                    let mut words: Vec<(usize, usize)> = Vec::new();
                    let mut from_file: Option<String> = None;
                    let mut k = colon + 1;
                    while k < end {
                        if (bytes[k] as char).is_whitespace() {
                            k += 1;
                            continue;
                        }
                        let start = k;
                        while k < end && !(bytes[k] as char).is_whitespace() {
                            k += 1;
                        }
                        if &text[start..k] == "from" {
                            let target = text[k..end].trim().trim_start_matches("from").trim().trim_matches(|ch| ch == '"' || ch == '\'');
                            if target != "global" {
                                from_file = Some(target.to_string());
                            }
                            break;
                        }
                        words.push((start, k));
                    }
                    let stop = if end < bytes.len() && bytes[end] == b';' { end + 1 } else { end };
                    flush!(i);
                    match from_file {
                        Some(specifier) => {
                            refs.push(Ref { kind: RefKind::Compose, specifier, conditions: String::new(), suffix: String::new() });
                            pieces.push(Piece::ComposesFrom(words, refs.len() - 1));
                        }
                        None => pieces.push(Piece::Composes(words)),
                    }
                    raw_from = stop;
                    i = stop;
                    statement_start = i;
                } else {
                    i = j;
                }
            }
            _ => i += 1,
        }
    }
    flush!(bytes.len());
    Ok(Parsed { pieces, refs, values })
}

/// A comment between selector parts: nothing to do, the pieces still wait for their `{`.
fn commit_if_ordered(_pending: &mut [Piece]) {}

fn starts_word(rest: &str, word: &str) -> bool {
    rest.len() >= word.len() && rest[..word.len()].eq_ignore_ascii_case(word) && !rest[word.len()..].starts_with(|c: char| is_ident_char(c))
}

/// Where the value that starts at `from` ends: the `;` or the `}`.
fn value_end(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = from;
    let mut parens = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b';' | b'}' if parens == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// The specifier of an `@import` that starts after `at` and where the rest begins.
fn import_target(text: &str, at: usize) -> Result<(String, usize), String> {
    let bytes = text.as_bytes();
    let mut i = at;
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return Err("@import without a specifier".into());
    }
    if bytes[i] == b'"' || bytes[i] == b'\'' {
        let q = bytes[i];
        let end = text[i + 1..].find(q as char).map(|p| i + 1 + p).ok_or("unterminated string in @import")?;
        return Ok((text[i + 1..end].to_string(), end + 1));
    }
    if text[i..].len() >= 4 && text[i..i + 4].eq_ignore_ascii_case("url(") {
        let close = text[i..].find(')').map(|p| i + p).ok_or("unterminated url( in @import")?;
        let inner = text[i + 4..close].trim().trim_matches(|c| c == '"' || c == '\'');
        return Ok((inner.to_string(), close + 1));
    }
    Err(format!("@import at {at} needs a string or url()"))
}

/// The `;` that ends the rule whose text has reached `from`.
fn find_rule_end(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b';' => return Some(i),
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    // A last rule without `;` ends with the text.
    Some(bytes.len())
}

/// What to put in place of each reference, in the order `Parsed::refs` lists them.
#[derive(Clone, Debug)]
pub enum Replacement {
    /// An import whose sheet is part of the output on its own, or a reference left as written.
    Leave,
    /// New specifier text (a `url()`'s bundled address).
    Text(String),
    /// An import that stays an `@import` (external).
    Keep,
    /// For a `composes ... from`: the seed of the other sheet's local names.
    Seed(String),
    /// The same, and what the other sheet's `@value` rules define (for `@value a from "./other.css"`).
    Sheet(String, Vec<(String, String)>),
}

pub struct Rendered {
    pub css: String,
    /// `@import` rules to hoist to the top of the output.
    pub hoisted: Vec<String>,
    /// For local names: (name as written, name in the output) and what each name also carries.
    pub exports: Vec<(String, String)>,
    /// With `render_mapped`: (offset in `css`, byte offset in the source text), increasing.
    pub anchors: Vec<(usize, u32)>,
}

/// Writes the sheet with `replacements` applied. `seed` is mixed into local names so two
/// sheets with the same class do not collide.
pub fn render(text: &str, parsed: &Parsed, replacements: &[Replacement], local: Option<&str>, minify: bool) -> Rendered {
    render_mapped(text, parsed, replacements, local, minify, false)
}

/// `render`, and with `want_map` also the places in the output that stand for places in `text`.
pub fn render_mapped(text: &str, parsed: &Parsed, replacements: &[Replacement], local: Option<&str>, minify: bool, want_map: bool) -> Rendered {
    let mut anchors: Vec<(usize, u32)> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut hoisted = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut keyframes: HashMap<String, String> = HashMap::new();
    let mapped = |names: &mut HashMap<String, String>, order: &mut Vec<String>, raw: &str| -> String {
        if let Some(n) = names.get(raw) {
            return n.clone();
        }
        let n = format!("{}_{}", local.unwrap_or(""), raw);
        names.insert(raw.to_string(), n.clone());
        order.push(raw.to_string());
        n
    };
    // Keyframe names first, so `animation` values can use them wherever they come.
    for piece in &parsed.pieces {
        if let Piece::Keyframes(s, e) = piece {
            let raw = &text[*s..*e];
            let n = format!("{}_{}", local.unwrap_or(""), raw);
            keyframes.insert(raw.to_string(), n);
        }
    }
    // What `@value` names stand for: this sheet's own, and those imported from another.
    let mut value_map: HashMap<String, String> = parsed.values.iter().cloned().collect();
    let mut value_exports: Vec<(String, String)> = parsed.values.clone();
    for piece in &parsed.pieces {
        if let Piece::ValueFrom(pairs, index) = piece {
            if let Replacement::Sheet(_, theirs) = &replacements[*index] {
                for (there, here) in pairs {
                    if let Some((_, v)) = theirs.iter().find(|(n, _)| n == there) {
                        value_map.insert(here.clone(), v.clone());
                        value_exports.push((here.clone(), v.clone()));
                    }
                }
            }
        }
    }
    let mut export_pairs: Vec<(String, String)> = Vec::new();
    // Owners of `composes`: the classes of the rule it is in, in order of appearance.
    let mut rule_stack: Vec<Vec<String>> = vec![Vec::new()];
    let mut extra: HashMap<String, Vec<String>> = HashMap::new();
    // Names composed from other files are already in their final form.
    let mut extra_final: HashMap<String, Vec<String>> = HashMap::new();
    let mut last_local: Vec<String> = Vec::new();
    for piece in &parsed.pieces {
        match piece {
            Piece::Raw(s, e) => {
                let chunk = &text[*s..*e];
                for ch in chunk.chars() {
                    match ch {
                        '{' => {
                            rule_stack.push(std::mem::take(&mut last_local));
                        }
                        '}' => {
                            rule_stack.pop();
                            if rule_stack.is_empty() {
                                rule_stack.push(Vec::new());
                            }
                            last_local.clear();
                        }
                        ';' => last_local.clear(),
                        _ => {}
                    }
                }
                if want_map {
                    // Every token start (after a blank or one of `{};,`) is a place the map can point to.
                    let b = chunk.as_bytes();
                    // Inside a string or an unquoted `url(...)` nothing is a token start.
                    let mut quote: Option<u8> = None;
                    let mut in_url = false;
                    let mut k = 0;
                    while k < b.len() {
                        match quote {
                            Some(q) => {
                                if b[k] == b'\\' {
                                    k += 1;
                                } else if b[k] == q {
                                    quote = None;
                                }
                            }
                            None if in_url => in_url = b[k] != b')',
                            None => {
                                // A chunk can start in the middle of a token (after a rewritten `url()`), so look at what is already written.
                                let before = if k == 0 { out.as_bytes().last().copied() } else { Some(b[k - 1]) };
                                let starts = !b[k].is_ascii_whitespace() && !matches!(b[k], b'}' | b')') && before.is_none_or(|p| p.is_ascii_whitespace() || matches!(p, b'{' | b'}' | b';' | b','));
                                if starts && chunk.is_char_boundary(k) {
                                    anchors.push((out.len() + k, (*s + k) as u32));
                                }
                                if b[k] == b'"' || b[k] == b'\'' {
                                    quote = Some(b[k]);
                                } else if b[k] == b'(' && k >= 3 && chunk[..k].to_ascii_lowercase().ends_with("url") && !matches!(b.get(k + 1), Some(b'"' | b'\'')) {
                                    in_url = true;
                                }
                            }
                        }
                        k += 1;
                    }
                }
                out.push_str(chunk);
            }
            Piece::Comment(s, e) => {
                if !minify || text[*s..*e].starts_with("/*!") {
                    out.push_str(&text[*s..*e]);
                }
            }
            Piece::Ref(index, s, e) => match &replacements[*index] {
                // A dropped import is gone; a `url()` left alone keeps its text.
                Replacement::Leave if parsed.refs[*index].kind == RefKind::Url => out.push_str(&text[*s..*e]),
                Replacement::Leave => {}
                Replacement::Text(t) if parsed.refs[*index].kind == RefKind::Url => {
                    // Written inside `url(...)`, perhaps without quotes: the characters that would end it are escaped.
                    for ch in t.chars() {
                        match ch {
                            ' ' => out.push_str("%20"),
                            '"' => out.push_str("%22"),
                            '\'' => out.push_str("%27"),
                            '(' => out.push_str("%28"),
                            ')' => out.push_str("%29"),
                            '\\' => out.push_str("%5C"),
                            _ => out.push(ch),
                        }
                    }
                    out.push_str(&parsed.refs[*index].suffix);
                }
                Replacement::Text(t) => out.push_str(t),
                Replacement::Seed(_) | Replacement::Sheet(..) => {}
                Replacement::Keep => {
                    let r = &parsed.refs[*index];
                    let cond = if r.conditions.is_empty() { String::new() } else { format!(" {}", r.conditions) };
                    hoisted.push(format!("@import \"{}\"{};", r.specifier.replace('"', "\\\""), cond));
                    let _ = (s, e);
                }
            },
            Piece::Local(s, e) => {
                if want_map {
                    anchors.push((out.len(), *s as u32));
                }
                let raw = &text[*s..*e];
                let n = mapped(&mut names, &mut order, raw);
                out.push_str(&n);
                last_local.push(raw.to_string());
            }
            Piece::Keyframes(s, e) => {
                if want_map {
                    anchors.push((out.len(), *s as u32));
                }
                out.push_str(&keyframes[&text[*s..*e]])
            }
            Piece::Animation(s, e) => {
                if want_map {
                    anchors.push((out.len(), *s as u32));
                }
                let raw = &text[*s..*e];
                out.push_str(keyframes.get(raw).map(String::as_str).unwrap_or(raw));
            }
            Piece::Drop(..) | Piece::ValueFrom(..) => {}
            Piece::Value(s, e) => out.push_str(value_map.get(&text[*s..*e]).map_or(&text[*s..*e], String::as_str)),
            Piece::Export(pairs) => export_pairs.extend(pairs.iter().map(|(k, v)| (k.clone(), v.split(' ').map(|word| value_map.get(word).map_or(word, String::as_str)).collect::<Vec<_>>().join(" ")))),
            Piece::Hoist(s, e) => hoisted.push(text[*s..*e].trim().to_string()),
            Piece::ComposesFrom(composed, index) => {
                let seed = match &replacements[*index] {
                    Replacement::Seed(seed) | Replacement::Sheet(seed, _) => Some(seed.as_str()),
                    _ => None,
                };
                let owners = rule_stack.last().cloned().unwrap_or_default();
                let list: Vec<String> = composed.iter().map(|(s, e)| match seed { Some(seed) => format!("{seed}_{}", &text[*s..*e]), None => text[*s..*e].to_string() }).collect();
                for owner in owners {
                    extra_final.entry(owner).or_default().extend(list.clone());
                }
            }
            Piece::Composes(composed) => {
                // Attach to the classes of the rule this declaration is in.
                let owners = rule_stack.last().cloned().unwrap_or_default();
                let list: Vec<String> = composed.iter().map(|(s, e)| text[*s..*e].to_string()).collect();
                for owner in owners {
                    extra.entry(owner).or_default().extend(list.clone());
                }
            }
        }
    }
    // Class names that only a `composes` mentions still get a local name.
    let mut exports = Vec::new();
    for raw in order {
        let mut value = names[&raw].clone();
        if let Some(more) = extra.get(&raw) {
            for m in more {
                value.push(' ');
                value.push_str(names.get(m).map(String::as_str).unwrap_or(m));
            }
        }
        if let Some(more) = extra_final.get(&raw) {
            for m in more {
                value.push(' ');
                value.push_str(m);
            }
        }
        exports.push((raw, value));
    }
    for (name, value) in value_exports.into_iter().chain(export_pairs) {
        match exports.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => exports.push((name, value)),
        }
    }
    let (css, anchors) = if minify {
        let offsets: Vec<usize> = anchors.iter().map(|a| a.0).collect();
        let (css, moved) = minify_mapped(&out, &offsets);
        let css = if want_map { css } else { shorten_values(&css) };
        (css, moved.into_iter().zip(anchors.iter().map(|a| a.1)).collect())
    } else {
        (out, anchors)
    };
    Rendered { css, hoisted, exports, anchors }
}

/// Removes comments (but not `/*! ... */`) and whitespace that does not matter.
pub fn minify_text(text: &str) -> String {
    shorten_values(&minify_mapped(text, &[]).0)
}

/// Shorter spellings of what stands in declarations of minified text: `#ffffff` is `#fff`, `0.5` is `.5`. Strings and `url()` are copied
/// as they are, as is a declaration that holds `progid` (an old filter that reads colors as text). Only text without a source map goes
/// through this: it moves the positions that a map points to.
fn shorten_values(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = css[i + 2..].find("*/").map_or(bytes.len(), |p| i + 2 + p + 2),
            b'(' if css[..i].to_ascii_lowercase().ends_with("url") => {
                i = url_end(css, i);
            }
            b'{' => {
                i += 1;
                let prelude = &css[start..i];
                if prelude.contains("@media (") {
                    out.push_str(&prelude.replace("@media (", "@media("));
                } else if !prelude.contains('@') {
                    out.push_str(&tighten_combinators(prelude));
                } else {
                    out.push_str(prelude);
                }
                start = i;
            }
            b';' | b'}' => {
                let segment = &css[start..i];
                if segment.contains(':') && !segment.to_ascii_lowercase().contains("progid") {
                    out.push_str(&shorten_declaration(segment));
                } else {
                    out.push_str(segment);
                }
                start = i;
                i += 1;
            }
            _ => i += 1,
        }
    }
    out.push_str(&css[start..]);
    drop_empty_rules(&merge_rules(&out))
}

/// Rules with nothing in them are left out: a style rule with no declaration, and an `@media`, `@supports` or `@container` block that
/// holds nothing else (after its own empty rules are gone). Every other at-rule stays as it is: an empty `@keyframes` still replaces an
/// earlier one of the name, and `@layer` fixes the order of the layers. A rule with a rule inside it (nesting) is kept whole.
fn drop_empty_rules(css: &str) -> String {
    if !css.contains("{}") && !css.contains("{;}") {
        return css.to_string();
    }
    fn walk(css: &str, out: &mut String) {
        let mut i = 0;
        while i < css.len() {
            let open = skip_to(css, i, &[b'{', b'}', b';']);
            if open >= css.len() || css.as_bytes()[open] != b'{' {
                // A statement (`@import ...;`, `@charset ...;`) or the rest.
                let end = (open + 1).min(css.len());
                out.push_str(&css[i..end]);
                i = end;
                continue;
            }
            // The block that starts at `open`, to its matching `}`.
            let mut depth = 0usize;
            let mut j = open;
            loop {
                j = skip_to(css, j + 1, &[b'{', b'}']);
                if j >= css.len() {
                    break;
                }
                if css.as_bytes()[j] == b'{' {
                    depth += 1;
                } else if depth == 0 {
                    break;
                } else {
                    depth -= 1;
                }
            }
            let prelude = &css[i..open];
            let body = &css[open + 1..j.min(css.len())];
            let end = (j + 1).min(css.len());
            let name = prelude.strip_prefix('@').map(|r| r.split(|c: char| !c.is_ascii_alphabetic() && c != '-').next().unwrap_or("").to_ascii_lowercase());
            match name.as_deref() {
                None if body.is_empty() => {}
                Some("media" | "supports" | "container") => {
                    let mut inner = String::new();
                    walk(body, &mut inner);
                    if !inner.is_empty() {
                        out.push_str(prelude);
                        out.push('{');
                        out.push_str(&inner);
                        out.push('}');
                    }
                }
                _ => out.push_str(&css[i..end]),
            }
            i = end;
        }
    }
    let mut out = String::with_capacity(css.len());
    walk(css, &mut out);
    out
}

/// The end of a `url(...)` whose `(` is at `open`: after the `)`, which a quoted address may hold inside its quotes.
fn url_end(text: &str, open: usize) -> usize {
    let b = text.as_bytes();
    let mut i = open + 1;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    if matches!(b.get(i), Some(b'"' | b'\'')) {
        let q = b[i];
        i += 1;
        while i < b.len() && b[i] != q {
            i += if b[i] == b'\\' { 2 } else { 1 };
        }
    }
    text[i.min(b.len())..].find(')').map_or(b.len(), |p| i + p + 1)
}

/// The end of the text that starts at `from` and is closed by `stop` (a byte that is not in a string, a comment or `url()`), or the end of `text`.
pub(crate) fn skip_to(text: &str, from: usize, stops: &[u8]) -> usize {
    let b = text.as_bytes();
    let mut i = from;
    while i < b.len() {
        match b[i] {
            c if stops.contains(&c) => return i,
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'(' if text[..i].to_ascii_lowercase().ends_with("url") => i = url_end(text, i),
            _ => i += 1,
        }
    }
    b.len()
}

/// Whether a selector is made only of what every browser reads: a list of it in which one part is not understood would be dropped whole, so
/// two rules are joined only when both parts are of this kind.
fn selector_is_plain(sel: &str) -> bool {
    const PSEUDO: &[&str] = &["hover", "focus", "active", "visited", "link", "first-child", "last-child", "only-child", "first-of-type", "last-of-type", "only-of-type", "empty", "root", "before", "after", "first-line", "first-letter", "checked", "disabled", "enabled", "target", "not", "nth-child", "nth-of-type", "nth-last-child", "nth-last-of-type", "lang"];
    if sel.contains("::") || sel.contains(['\\', '|']) || !sel.is_ascii() {
        return false;
    }
    let b = sel.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                let end = skip_to(sel, i + 1, &[b[i]]);
                i = end + 1;
            }
            b':' => {
                let name: String = sel[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
                if !PSEUDO.contains(&name.as_str()) {
                    return false;
                }
                i += 1 + name.len();
            }
            _ => i += 1,
        }
    }
    true
}

/// Rules that follow each other: the same selector is one rule with both bodies, and the same body is one rule with both selectors.
fn merge_rules(css: &str) -> String {
    let mut items: Vec<(String, Option<String>)> = Vec::new(); // (header or whole text, body of a plain rule)
    let mut i = 0;
    while i < css.len() {
        let start = i;
        if css[i..].starts_with("/*") {
            i = css[i + 2..].find("*/").map_or(css.len(), |p| i + 2 + p + 2);
            items.push((css[start..i].to_string(), None));
            continue;
        }
        let stop = skip_to(css, i, &[b'{', b';', b'}']);
        if stop >= css.len() || css.as_bytes()[stop] != b'{' {
            // A statement (`@import ...;`) or something that does not belong: kept as it is.
            let end = if stop < css.len() { stop + 1 } else { css.len() };
            items.push((css[start..end].to_string(), None));
            i = end;
            continue;
        }
        let header = &css[start..stop];
        // The block, with its braces matched.
        let mut depth = 0usize;
        let mut j = stop;
        loop {
            j = skip_to(css, j, &[b'{', b'}']);
            if j >= css.len() {
                break;
            }
            if css.as_bytes()[j] == b'{' {
                depth += 1;
            } else {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        let end = (j + 1).min(css.len());
        let inner = &css[stop + 1..j.min(css.len())];
        if header.starts_with('@') {
            let name: String = header[1..].chars().take_while(|c| c.is_ascii_alphabetic() || *c == '-').collect();
            let text = if matches!(name.as_str(), "media" | "supports" | "layer" | "container") { format!("{header}{{{}}}", merge_rules(inner)) } else { css[start..end].to_string() };
            items.push((text, None));
        } else if inner.contains('{') || header.is_empty() {
            items.push((css[start..end].to_string(), None));
        } else {
            items.push((header.to_string(), Some(crate::css_fold::fold(inner))));
        }
        i = end;
    }
    let mut out: Vec<(String, Option<String>)> = Vec::with_capacity(items.len());
    for (header, body) in items {
        if let (Some(body), Some((ph, Some(pb)))) = (&body, out.last_mut()) {
            if *ph == header {
                if pb.is_empty() || body.is_empty() {
                    pb.push_str(body);
                } else {
                    pb.push(';');
                    pb.push_str(body);
                }
                continue;
            }
            if pb == body && selector_is_plain(ph) && selector_is_plain(&header) {
                ph.push(',');
                ph.push_str(&header);
                continue;
            }
        }
        out.push((header, body));
    }
    // A rule written again, header and body alike, later in the same list: the later copy decides everything the earlier one did.
    let mut last: HashMap<(&str, &str), usize> = HashMap::new();
    for (k, (header, body)) in out.iter().enumerate() {
        if let Some(body) = body {
            last.insert((header.as_str(), body.as_str()), k);
        }
    }
    let keep: Vec<bool> = out.iter().enumerate().map(|(k, (header, body))| body.as_ref().is_none_or(|b| last[&(header.as_str(), b.as_str())] == k)).collect();
    let out: Vec<(String, Option<String>)> = out.into_iter().zip(keep).filter_map(|(item, keep)| keep.then_some(item)).collect();
    let mut text = String::with_capacity(css.len());
    for (header, body) in out {
        match body {
            Some(body) => {
                text.push_str(&header);
                text.push('{');
                text.push_str(&body);
                text.push('}');
            }
            None => text.push_str(&header),
        }
    }
    text
}

fn shorten_declaration(decl: &str) -> String {
    let Some(colon) = decl.find(':') else { return decl.to_string() };
    let property = decl[..colon].trim_start_matches([';', '{', '}']).trim().to_ascii_lowercase();
    // A custom property holds whatever its users read from it.
    if property.starts_with("--") {
        return decl.to_string();
    }
    let scanned = scan_value(decl, colon);
    let (name, value) = scanned.split_at(colon + 1);
    format!("{name}{}", refine_value(&property, value))
}

fn scan_value(decl: &str, colon: usize) -> String {
    // Only the value: a selector-like start (`a:hover{` never reaches here, as `{` ends a segment).
    let (name, value) = decl.split_at(colon + 1);
    let b = value.as_bytes();
    let mut out = String::with_capacity(decl.len());
    out.push_str(name);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b'"' | b'\'' | b'(' if c != b'(' || value[..i].to_ascii_lowercase().ends_with("url") => {
                let end = if c == b'(' {
                    url_end(value, i)
                } else {
                    let mut j = i + 1;
                    while j < b.len() && b[j] != c {
                        j += if b[j] == b'\\' { 2 } else { 1 };
                    }
                    (j + 1).min(b.len())
                };
                out.push_str(&value[i..end]);
                i = end;
            }
            b'#' => {
                let hex = value[i + 1..].bytes().take_while(|x| x.is_ascii_hexdigit()).count();
                let after = b.get(i + 1 + hex).copied();
                let ident_after = after.is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_' || x == b'-' || x >= 0x80);
                let digits = &value[i + 1..i + 1 + hex];
                let pairs = digits.as_bytes().chunks(2).all(|p| p.len() == 2 && p[0].eq_ignore_ascii_case(&p[1]));
                if !ident_after && (hex == 6 || hex == 8) && pairs {
                    out.push('#');
                    out.extend(digits.as_bytes().chunks(2).map(|p| p[0].to_ascii_lowercase() as char));
                    i += 1 + hex;
                } else if !ident_after && matches!(hex, 3 | 4 | 6 | 8) {
                    out.push_str(&value[i..i + 1 + hex].to_ascii_lowercase());
                    i += 1 + hex;
                } else {
                    out.push_str(&value[i..i + 1 + hex]);
                    i += 1 + hex;
                }
            }
            b'0' if b.get(i + 1) == Some(&b'.') && b.get(i + 2).is_some_and(u8::is_ascii_digit) => {
                let lead = |x: u8| matches!(x, b' ' | b',' | b'(' | b'/');
                let ok = match i.checked_sub(1).map(|p| b[p]) {
                    None => true,
                    Some(x) if lead(x) => true,
                    Some(b'-' | b'+') => i == 1 || lead(b[i - 2]),
                    _ => false,
                };
                if ok {
                    i += 1;
                } else {
                    out.push('0');
                    i += 1;
                }
            }
            _ => {
                let ch = value[i..].chars().next().expect("a character");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Selector spelling: `a ~ b` and `a + b` lose their spaces, `::before` (and `::after`, `::first-line`, `::first-letter`) is the old `:before`,
/// and `[a="b"]` is `[a=b]` when `b` is an identifier. Strings are not touched.
fn tighten_combinators(prelude: &str) -> String {
    let b = prelude.as_bytes();
    let mut out = String::with_capacity(prelude.len());
    let mut i = 0;
    let ident = |v: &str| {
        let f = v.as_bytes().first().copied();
        let ok_start = f.is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') || (f == Some(b'-') && v.as_bytes().get(1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_'));
        ok_start && v.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    };
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < b.len() && b[j] != q {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                let j = (j + 1).min(b.len());
                // `="value"` of an attribute selector, with a plain identifier inside.
                let inner = &prelude[(i + 1).min(j)..j.saturating_sub(1).max((i + 1).min(j))];
                let after = b.get(j).copied();
                if i > 0 && b[i - 1] == b'=' && j == prelude.len().min(j) && ident(inner) && j > i + 1 && matches!(after, Some(b']' | b' ')) {
                    out.push_str(inner);
                } else {
                    out.push_str(&prelude[i..j]);
                }
                i = j;
            }
            b' ' if matches!(b.get(i + 1), Some(b'~' | b'+')) && b.get(i + 2) == Some(&b' ') => {
                out.push(b[i + 1] as char);
                i += 3;
            }
            b':' if b.get(i + 1) == Some(&b':') && !i.checked_sub(1).is_some_and(|p| b[p] == b':') && ["before", "after", "first-line", "first-letter"].iter().any(|n| prelude[i + 2..].starts_with(n) && !b.get(i + 2 + n.len()).is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))) => {
                out.push(':');
                i += 2;
            }
            _ => {
                let ch = prelude[i..].chars().next().expect("a character");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Value-level rewrites that keep the meaning: no space before `!important`, trailing zeros of numbers gone, `bold` and `normal` as numbers,
/// a color of `rgb()` in hex, `0px` as `0` where a length is certain, quotes off a font name that is plain words, and a box shorthand
/// (`margin`, `padding`, ...) of four or three equal parts written with fewer.
fn refine_value(property: &str, value: &str) -> String {
    let (body, important) = match value.strip_suffix("!important") {
        Some(rest) => (rest.trim_end(), "!important"),
        None => (value, ""),
    };
    let mut body = trim_numbers(body);
    if property == "font-weight" {
        body = match body.as_str() {
            "normal" => "400".into(),
            "bold" => "700".into(),
            _ => body,
        };
    }
    if matches!(property, "transform" | "-webkit-transform" | "-ms-transform") && body.contains(['Z', 'X', 'm']) {
        let mut text = body.replace("rotateZ(", "rotate(").replace("skewX(", "skew(").replace("matrix(-1,0,0,1,0,0)", "scaleX(-1)").replace("matrix(1,0,0,-1,0,0)", "scaleY(-1)");
        text = text.replace(" rotateZ(", " rotate(");
        body = text;
    }
    if body.contains("rgb") || body.contains("RGB") {
        body = rgb_to_hex(&body);
    }
    if body.contains("transparent") && (property.ends_with("color") || matches!(property, "background" | "background-image" | "border" | "border-top" | "border-right" | "border-bottom" | "border-left" | "outline" | "column-rule" | "box-shadow" | "text-shadow")) {
        body = transparent_as_hex(&body);
    }
    if property.ends_with("color") || matches!(property, "background" | "background-image" | "border" | "border-top" | "border-right" | "border-bottom" | "border-left" | "outline" | "column-rule" | "box-shadow" | "text-shadow" | "text-decoration" | "fill" | "stroke") {
        body = shortest_colors(&body);
    }
    let base = property.trim_start_matches("-webkit-").trim_start_matches("-moz-").trim_start_matches("-o-");
    if base == "flex" {
        body = shorter_flex(&body);
    }
    if base == "transition" {
        body = drop_default_ease(&body);
    }
    if base.starts_with("transition") || base.starts_with("animation") {
        body = seconds_for_milliseconds(&body);
    }
    if matches!(base, "border" | "border-top" | "border-right" | "border-bottom" | "border-left") {
        body = drop_current_color(&body);
    }
    if property == "background" && matches!(body.as_str(), "none" | "transparent" | "#0000") {
        body = "0 0".into();
    }
    if let ("background-position" | "-webkit-background-position", Some((x, "50%"))) = (property, body.split_once(' ')) {
        let plain = !x.is_empty() && (x.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '-') || x.ends_with('%')) && !x.contains(['(', ' ']);
        if plain {
            body = x.to_string();
        }
    }
    if property == "transform-origin" && body == "center center" {
        body = "50%".into();
    }
    if matches!(base, "transform") && (body.contains("translateX(") || body.contains(",0)") || body.contains(",0px)")) {
        body = shorter_translate(&body);
    }
    if matches!(property, "box-shadow" | "text-shadow") {
        body = zero_lengths_at_top(&body);
    }
    if matches!(property, "transform" | "-webkit-transform" | "-ms-transform" | "filter" | "backdrop-filter" | "-webkit-backdrop-filter") && body.contains(") ") {
        let b = body.as_bytes();
        let mut text = String::with_capacity(body.len());
        for (i, ch) in body.char_indices() {
            if ch == ' ' && i > 0 && b[i - 1] == b')' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
                continue;
            }
            text.push(ch);
        }
        body = text;
    }
    if property == "font-family" || property == "font" {
        body = unquote_fonts(&body);
    }
    let boxed = matches!(property, "margin" | "padding" | "border-width" | "border-style" | "border-color" | "inset" | "scroll-margin" | "scroll-padding");
    let zeroed = boxed || matches!(property, "top" | "right" | "bottom" | "left" | "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height" | "gap" | "row-gap" | "column-gap" | "border-radius" | "outline-offset" | "text-indent");
    if zeroed && !body.contains(['(', '"', '\'']) {
        body = body.split(' ').map(|t| if is_zero_length(t) { "0" } else { t }).collect::<Vec<_>>().join(" ");
    }
    if boxed && !body.contains(['(', '"', '\'']) {
        let mut parts: Vec<&str> = body.split(' ').collect();
        if parts.len() == 4 && parts[1] == parts[3] {
            parts.pop();
        }
        if parts.len() == 3 && parts[0] == parts[2] {
            parts.pop();
        }
        if parts.len() == 2 && parts[0] == parts[1] {
            parts.pop();
        }
        body = parts.join(" ");
    }
    format!("{body}{important}")
}

pub(crate) fn is_zero_length(t: &str) -> bool {
    t.strip_prefix('0').is_some_and(|unit| matches!(unit, "px" | "em" | "rem" | "pt" | "cm" | "mm" | "in" | "ex" | "ch" | "vh" | "vw" | "vmin" | "vmax"))
}

/// `1.0` is `1` and `0.50` is `0.5` (`.5` is made by the scan before): the trailing zeros of a fraction go.
fn trim_numbers(value: &str) -> String {
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let mut j = i + 1;
            while j < b.len() && b[j] != c {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let j = (j + 1).min(b.len());
            out.push_str(&value[i..j]);
            i = j;
        } else if c == b'(' && value[..i].to_ascii_lowercase().ends_with("url") {
            let j = url_end(value, i);
            out.push_str(&value[i..j]);
            i = j;
        } else if (c.is_ascii_digit() || c == b'.') && !i.checked_sub(1).is_some_and(|p| b[p].is_ascii_alphanumeric() || matches!(b[p], b'_' | b'#' | b'.' | b'\\') || b[p] >= 0x80 || (b[p] == b'-' && i >= 2 && !matches!(b[i - 2], b' ' | b',' | b'(' | b'/'))) {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j < b.len() && b[j] == b'.' && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if matches!(b.get(k), Some(b'e' | b'E')) && b.get(k + 1).is_some_and(|x| x.is_ascii_digit() || matches!(x, b'-' | b'+')) {
                    out.push_str(&value[i..k]);
                } else {
                    let frac = value[j + 1..k].trim_end_matches('0');
                    out.push_str(&value[i..j]);
                    if !frac.is_empty() {
                        out.push('.');
                        out.push_str(frac);
                    }
                }
                i = k;
            } else {
                out.push_str(&value[i..j.max(i + 1)]);
                i = j.max(i + 1);
            }
        } else {
            let ch = value[i..].chars().next().expect("a character");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// `rgb(255, 0, 0)`, `rgb(255 0 0)`, `rgba(255,0,0,.5)` and `rgb(255 0 0 / 50%)` (whole numbers for the channels) are `#f00`, `#ff000080`.
/// An alpha of at most two decimals (or a whole percent) has one 8-bit spelling that a browser writes back as the same decimal (CSS Color 4
/// asks for the shortest decimal of two digits that gives the same byte), so the color keeps its value; other alphas stay as written.
fn rgb_to_hex(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(at) = lower.find("rgb") else { break };
        let name_end = at + if lower[at + 3..].starts_with('a') { 4 } else { 3 };
        let before_ok = at == 0 || !rest.as_bytes()[at - 1].is_ascii_alphanumeric() && !matches!(rest.as_bytes()[at - 1], b'-' | b'_');
        let after_ok = rest.as_bytes().get(name_end) == Some(&b'(');
        let close = if after_ok { rest[name_end..].find(')').map(|p| name_end + p) } else { None };
        let Some(close) = close.filter(|_| before_ok) else {
            out.push_str(&rest[..at + 3]);
            rest = &rest[at + 3..];
            continue;
        };
        match parse_rgb(&rest[name_end + 1..close]) {
            Some(rgba) => {
                out.push_str(&rest[..at]);
                out.push('#');
                let hex = match rgba[3] {
                    255 => format!("{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]),
                    a => format!("{:02x}{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2], a),
                };
                let b = hex.as_bytes();
                if b.chunks(2).all(|p| p[0] == p[1]) {
                    out.extend(b.chunks(2).map(|p| p[0] as char));
                } else {
                    out.push_str(&hex);
                }
                rest = &rest[close + 1..];
            }
            None => {
                out.push_str(&rest[..close + 1]);
                rest = &rest[close + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The channels and alpha byte of the arguments of `rgb(...)`, when they are all plain numbers and one of the two syntaxes.
fn parse_rgb(args: &str) -> Option<[u8; 4]> {
    let channel = |n: &str| -> Option<u8> { if !n.is_empty() && n.bytes().all(|d| d.is_ascii_digit()) { n.parse::<u8>().ok() } else { None } };
    let (chan, alpha): (Vec<&str>, Option<&str>) = if args.contains(',') {
        let p: Vec<&str> = args.split(',').map(str::trim).collect();
        if p.len() == 3 {
            (p, None)
        } else if p.len() == 4 {
            (p[..3].to_vec(), Some(p[3]))
        } else {
            return None;
        }
    } else {
        let (left, right) = match args.split_once('/') {
            Some((l, r)) => (l, Some(r.trim())),
            None => (args, None),
        };
        (left.split(' ').filter(|s| !s.is_empty()).collect(), right)
    };
    if chan.len() != 3 {
        return None;
    }
    let a = match alpha {
        None => 255,
        Some(text) => {
            let hundredths: u32 = if let Some(p) = text.strip_suffix('%') {
                if p.is_empty() || p.len() > 3 || !p.bytes().all(|d| d.is_ascii_digit()) {
                    return None;
                }
                p.parse().ok()?
            } else {
                let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
                let whole_ok = whole.is_empty() && !frac.is_empty() || matches!(whole, "0" | "1");
                if !whole_ok || frac.len() > 2 || !frac.bytes().all(|d| d.is_ascii_digit()) || (whole.is_empty() && frac.is_empty()) {
                    return None;
                }
                let f: u32 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().ok()? };
                (if whole == "1" { 100 } else { 0 }) + f
            };
            if hundredths > 100 {
                return None;
            }
            ((hundredths * 255 + 50) / 100) as u8
        }
    };
    Some([channel(chan[0])?, channel(chan[1])?, channel(chan[2])?, a])
}

/// `flex: 1 1 auto` is `auto`, `0 0 auto` is `none`, and a shrink factor of 1 is the default and goes (`0 1 auto` is `0 auto`).
fn shorter_flex(value: &str) -> String {
    let p: Vec<&str> = value.split(' ').collect();
    let number = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit() || c == b'.') && t.bytes().any(|c| c.is_ascii_digit());
    if p.len() != 3 || !number(p[0]) || !number(p[1]) {
        return value.to_string();
    }
    let basis = p[2];
    // A basis that is not a number: a keyword, a length with a unit or a percentage (a bare number would be read as a factor).
    let is_basis = !basis.is_empty() && (basis.ends_with('%') || basis.bytes().any(|c| c.is_ascii_alphabetic())) && !basis.contains(['!', '\\', '"', '\'']) && !basis.to_ascii_lowercase().contains("var(");
    if !is_basis {
        return value.to_string();
    }
    match (p[0], p[1], basis) {
        ("1", "1", "auto") => "auto".into(),
        ("0", "0", "auto") => "none".into(),
        ("1", "1", b) => b.to_string(),
        (g, "1", b) => format!("{g} {b}"),
        _ => value.to_string(),
    }
}

/// `ease` is the default timing function of a transition: left out of a part that also holds a time.
fn drop_default_ease(value: &str) -> String {
    if !value.contains("ease") || value.contains(['(', '"', '\'']) && value.to_ascii_lowercase().contains("var(") {
        return value.to_string();
    }
    let is_time = |t: &str| (t.ends_with("ms") || t.ends_with('s')) && t.bytes().next().is_some_and(|c| c.is_ascii_digit() || c == b'.' || c == b'-') && !t.contains('(');
    let mut parts_out: Vec<String> = Vec::new();
    for part in value.split(',') {
        // Commas inside a call (`cubic-bezier(.4,0,.2,1)`) are not part boundaries: give up on a value that has any.
        if part.contains('(') || part.contains(')') {
            return value.to_string();
        }
        let toks: Vec<&str> = part.split(' ').filter(|t| !t.is_empty()).collect();
        let at = toks.iter().position(|t| *t == "ease");
        match at {
            Some(k) if toks.iter().any(|t| is_time(t)) && toks.len() > 2 && toks.iter().filter(|t| **t == "ease").count() == 1 => {
                let mut t = toks.clone();
                t.remove(k);
                parts_out.push(t.join(" "));
            }
            _ => parts_out.push(part.to_string()),
        }
    }
    parts_out.join(",")
}

/// A time in whole milliseconds as seconds, where that is shorter: `100ms` is `.1s`, `1500ms` is `1.5s`.
fn seconds_for_milliseconds(value: &str) -> String {
    if !value.contains("ms") {
        return value.to_string();
    }
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < b.len() {
        let at_start = i == 0 || matches!(b[i - 1], b' ' | b',');
        if at_start && (b[i].is_ascii_digit() || b[i] == b'-') {
            let neg = b[i] == b'-';
            let ds = i + neg as usize;
            let mut j = ds;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > ds && value[j..].starts_with("ms") && b.get(j + 2).is_none_or(|c| matches!(c, b' ' | b',')) && !(b[ds] == b'0' && j - ds > 1) {
                let digits = &value[ds..j];
                let secs = if digits == "0" {
                    "0".to_string()
                } else if digits.len() > 3 {
                    let (w, f) = digits.split_at(digits.len() - 3);
                    let f = f.trim_end_matches('0');
                    if f.is_empty() { w.to_string() } else { format!("{w}.{f}") }
                } else {
                    let f = format!("{digits:0>3}");
                    format!(".{}", f.trim_end_matches('0'))
                };
                let new = format!("{}{secs}s", if neg { "-" } else { "" });
                if new.len() < j + 2 - i {
                    out.push_str(&new);
                    i = j + 2;
                    continue;
                }
            }
            // Not a time in milliseconds: copy the number.
            out.push_str(&value[i..j.max(i + 1)]);
            i = j.max(i + 1);
            continue;
        }
        let ch = value[i..].chars().next().expect("a character");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `currentColor` is the initial border color, so a border that names other parts needs no `currentColor`.
fn drop_current_color(value: &str) -> String {
    let toks: Vec<&str> = value.split(' ').collect();
    if toks.len() < 2 || value.contains(['(', '"', '\'']) {
        return value.to_string();
    }
    let kept: Vec<&str> = toks.iter().copied().filter(|t| !t.eq_ignore_ascii_case("currentcolor")).collect();
    if kept.is_empty() || kept.len() == toks.len() {
        return value.to_string();
    }
    kept.join(" ")
}

/// `translateX(v)` is `translate(v)`, and `translate(v,0)` is `translate(v)`.
fn shorter_translate(value: &str) -> String {
    let mut text = value.replace("translateX(", "translate(");
    for tail in [",0)", ",0px)"] {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find("translate(") {
            let open = at + "translate(".len();
            // The call ends at its own closing parenthesis; it must hold exactly one comma, the one before the zero.
            let close = {
                let mut depth = 1;
                let mut end = None;
                for (k, c) in rest[open..].bytes().enumerate() {
                    match c {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(open + k);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            };
            let before_ok = at == 0 || matches!(rest.as_bytes()[at - 1], b' ' | b')');
            match close {
                Some(c) if before_ok && rest[open..=c].ends_with(tail) && rest[open..c].matches(',').count() == 1 => {
                    out.push_str(&rest[..c - (tail.len() - 1)]);
                    out.push(')');
                    rest = &rest[c + 1..];
                }
                Some(c) => {
                    out.push_str(&rest[..c + 1]);
                    rest = &rest[c + 1..];
                }
                None => break,
            }
        }
        out.push_str(rest);
        text = out;
    }
    text
}

/// `transparent` standing alone as a color is `#0000`. Strings and `url()` are copied.
fn transparent_as_hex(value: &str) -> String {
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let end = (skip_to(value, i + 1, &[c]) + 1).min(b.len());
            out.push_str(&value[i..end]);
            i = end;
        } else if c == b'(' && value[..i].to_ascii_lowercase().ends_with("url") {
            let end = url_end(value, i);
            out.push_str(&value[i..end]);
            i = end;
        } else if value[i..].len() >= 11 && value[i..i + 11].eq_ignore_ascii_case("transparent") && i.checked_sub(1).is_none_or(|p| matches!(b[p], b' ' | b',' | b'(')) && b.get(i + 11).is_none_or(|n| matches!(n, b' ' | b',' | b')')) {
            out.push_str("#0000");
            i += 11;
        } else {
            let ch = value[i..].chars().next().expect("a character");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// The named colors of CSS and their values, by name.
const NAMED_COLORS: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4), ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000), ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a), ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e), ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b), ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b), ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc), ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b), ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3), ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969), ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22), ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700), ("goldenrod", 0xdaa520), ("gray", 0x808080), ("green", 0x008000),
    ("greenyellow", 0xadff2f), ("grey", 0x808080), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa), ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6), ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3), ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1), ("lightsalmon", 0xffa07a), ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899), ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32), ("linen", 0xfaf0e6), ("magenta", 0xff00ff),
    ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa), ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371), ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585), ("midnightblue", 0x191970), ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5), ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6),
    ("olive", 0x808000), ("olivedrab", 0x6b8e23), ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f), ("pink", 0xffc0cb), ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399), ("red", 0xff0000), ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513), ("salmon", 0xfa8072), ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee), ("sienna", 0xa0522d), ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa), ("springgreen", 0x00ff7f), ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8), ("tomato", 0xff6347), ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee), ("wheat", 0xf5deb3), ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// A color written the shorter way: a named color as hex where that is shorter (`white` is `#fff`, `yellow` is `#ff0`), and a hex color as its
/// name where that is shorter (`#f00` is `red`, `#ffc0cb` is `pink`); on a tie the spelling stays as it is. Only whole tokens are looked at, so
/// `var(--white)`, a function's name, a string and a `url()` are not touched, and the caller says which properties hold colors.
fn shortest_colors(value: &str) -> String {
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let starts_token = |at: usize| at == 0 || matches!(b[at - 1], b' ' | b',' | b'(' | b'/');
    let ends_token = |at: usize| at >= b.len() || matches!(b[at], b' ' | b',' | b')' | b'/');
    let short_hex = |rgb: u32| -> String {
        let (r, g, bl) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        if [r, g, bl].iter().all(|c| c >> 4 == c & 15) { format!("#{:x}{:x}{:x}", r & 15, g & 15, bl & 15) } else { format!("#{rgb:06x}") }
    };
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' {
            let end = (skip_to(value, i + 1, &[c]) + 1).min(b.len());
            out.push_str(&value[i..end]);
            i = end;
        } else if c == b'(' && value[..i].to_ascii_lowercase().ends_with("url") {
            let end = url_end(value, i);
            out.push_str(&value[i..end]);
            i = end;
        } else if c == b'#' && starts_token(i) {
            let digits = value[i + 1..].bytes().take_while(u8::is_ascii_hexdigit).count();
            let rgb = match digits {
                3 => u32::from_str_radix(&value[i + 1..i + 4], 16).ok().map(|v| ((v >> 8) * 17) << 16 | (((v >> 4) & 15) * 17) << 8 | (v & 15) * 17),
                6 => u32::from_str_radix(&value[i + 1..i + 7], 16).ok(),
                _ => None,
            };
            let name = rgb.and_then(|rgb| NAMED_COLORS.iter().filter(|(_, v)| *v == rgb).map(|(n, _)| *n).min_by_key(|n| n.len()));
            match name {
                Some(name) if name.len() < digits + 1 && ends_token(i + 1 + digits) => out.push_str(name),
                _ => out.push_str(&value[i..i + 1 + digits]),
            }
            i += 1 + digits;
        } else if c.is_ascii_alphabetic() && starts_token(i) {
            let len = b[i..].iter().take_while(|x| x.is_ascii_alphanumeric()).count();
            let word = &value[i..i + len];
            let lower = word.to_ascii_lowercase();
            let found = if ends_token(i + len) { NAMED_COLORS.binary_search_by(|(n, _)| (*n).cmp(lower.as_str())).ok() } else { None };
            match found {
                Some(at) if short_hex(NAMED_COLORS[at].1).len() < len => out.push_str(&short_hex(NAMED_COLORS[at].1)),
                _ => out.push_str(word),
            }
            i += len;
        } else {
            let ch = value[i..].chars().next().expect("a character");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// A zero length written with a unit, `0px`, is `0` where it stands outside any call (a shadow's offsets and blur).
fn zero_lengths_at_top(value: &str) -> String {
    let mut depth = 0i32;
    let mut out = String::with_capacity(value.len());
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        out.push_str(if is_zero_length(token) { "0" } else { token.as_str() });
        token.clear();
    };
    for ch in value.chars() {
        match ch {
            '(' | ')' | '"' | '\'' if depth > 0 || ch == '(' || ch == '"' || ch == '\'' => {
                if depth == 0 {
                    flush(&mut token, &mut out);
                }
                if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                }
                if ch == '"' || ch == '\'' {
                    // A string in a shadow: not touched at all.
                    return value.to_string();
                }
                out.push(ch);
            }
            ' ' | ',' if depth == 0 => {
                flush(&mut token, &mut out);
                out.push(ch);
            }
            _ if depth == 0 => token.push(ch),
            _ => out.push(ch),
        }
    }
    flush(&mut token, &mut out);
    out
}

/// `"Helvetica Neue"` is `Helvetica Neue` when every word is an identifier and the whole is not a keyword.
fn unquote_fonts(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('"') {
        let Some(len) = rest[at + 1..].find('"') else { break };
        let name = &rest[at + 1..at + 1 + len];
        let words_ok = !name.is_empty()
            && name.split(' ').all(|w| {
                let mut chars = w.chars();
                chars.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || (f == '-' && w.len() > 1 && w.as_bytes()[1].is_ascii_alphabetic()))
                    && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            });
        let keyword = matches!(
            name.to_ascii_lowercase().as_str(),
            "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "default" | "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui" | "ui-serif" | "ui-sans-serif" | "ui-monospace" | "ui-rounded" | "math" | "emoji" | "fangsong" | "none"
        );
        let before_ok = at == 0 || matches!(rest.as_bytes()[at - 1], b' ' | b',');
        let after = rest.as_bytes().get(at + len + 2).copied();
        let after_ok = after.is_none_or(|c| matches!(c, b',' | b' ' | b'/'));
        if words_ok && !keyword && before_ok && after_ok {
            out.push_str(&rest[..at]);
            out.push_str(name);
        } else {
            out.push_str(&rest[..at + len + 2]);
        }
        rest = &rest[at + len + 2..];
    }
    out.push_str(rest);
    out
}

/// `minify_text`, also saying where the given offsets of `text` ended up (increasing offsets in,
/// increasing offsets out). An offset inside text that is dropped lands on what follows it.
pub fn minify_mapped(text: &str, anchors: &[usize]) -> (String, Vec<usize>) {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut moved: Vec<usize> = Vec::with_capacity(anchors.len());
    let mut i = 0;
    let mut pending_space = false;
    let push_space = |out: &mut String, pending: &mut bool, next: u8| {
        if *pending {
            let last = out.as_bytes().last().copied();
            let tight = |b: u8| matches!(b, b'{' | b'}' | b';' | b',' | b'>' | b':' | b'(');
            if let Some(l) = last {
                if !tight(l) && !matches!(next, b'{' | b'}' | b';' | b',' | b'>' | b')') {
                    out.push(' ');
                }
            }
            *pending = false;
        }
    };
    // Offsets up to `i` are placed where the text of the token at `i` starts in the output.
    macro_rules! place {
        ($i:expr) => {
            while moved.len() < anchors.len() && anchors[moved.len()] <= $i {
                moved.push(out.len());
            }
        };
    }
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..].find("*/").map_or(bytes.len(), |p| i + 2 + p + 2);
                if text[i..].starts_with("/*!") {
                    push_space(&mut out, &mut pending_space, b'/');
                    place!(i);
                    out.push_str(&text[i..end]);
                } else {
                    pending_space = pending_space || out.as_bytes().last().is_some_and(|b| !matches!(b, b'{' | b'}' | b';'));
                }
                i = end;
            }
            b'"' | b'\'' => {
                push_space(&mut out, &mut pending_space, c);
                place!(i);
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != c {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let j = (j + 1).min(bytes.len());
                out.push_str(&text[i..j]);
                i = j;
            }
            _ if c.is_ascii_whitespace() => {
                pending_space = true;
                i += 1;
            }
            b'}' => {
                // The last `;` of a block is not needed.
                if out.ends_with(';') {
                    out.pop();
                }
                pending_space = false;
                place!(i);
                out.push('}');
                i += 1;
            }
            b'u' | b'U' if text[i..].len() >= 4 && text[i..i + 4].eq_ignore_ascii_case("url(") && (i == 0 || !is_ident_char(text[..i].chars().next_back().unwrap_or(' '))) => {
                push_space(&mut out, &mut pending_space, c);
                place!(i);
                // A quoted address may hold `)`; skip over the string first.
                let mut scan = i + 4;
                while scan < bytes.len() && bytes[scan].is_ascii_whitespace() {
                    scan += 1;
                }
                if matches!(bytes.get(scan), Some(b'"' | b'\'')) {
                    let q = bytes[scan];
                    scan += 1;
                    while scan < bytes.len() && bytes[scan] != q {
                        if bytes[scan] == b'\\' {
                            scan += 1;
                        }
                        scan += 1;
                    }
                }
                let close = text[scan.min(bytes.len())..].find(')').map_or(bytes.len(), |p| scan + p + 1);
                // Quoted contents are copied as they are; unquoted ones lose the edges' blanks.
                let inner = &text[i + 4..close.saturating_sub(1).max(i + 4)];
                out.push_str("url(");
                out.push_str(inner.trim());
                out.push(')');
                i = close;
            }
            _ => {
                push_space(&mut out, &mut pending_space, c);
                place!(i);
                // One whole character: a multi-byte one is copied as it is.
                let ch = text[i..].chars().next().expect("a character");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    while moved.len() < anchors.len() {
        moved.push(out.len());
    }
    (out, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Rendered {
        let parsed = parse(text, false).unwrap();
        let repl = vec![Replacement::Leave; parsed.refs.len()];
        render(text, &parsed, &repl, None, false)
    }

    fn modules(text: &str) -> Rendered {
        let parsed = parse(text, true).unwrap();
        let repl = vec![Replacement::Leave; parsed.refs.len()];
        render(text, &parsed, &repl, Some("h"), false)
    }

    #[test]
    fn finds_imports_and_urls() {
        let text = "@import \"./a.css\";\n@import url(b.css) screen;\n@import \"https://x.test/c.css\";\n.x { background: url(img/p.png) no-repeat, url(\"../q.svg\"), url(data:image/png;base64,AAA), url(#frag); }";
        let parsed = parse(text, false).unwrap();
        let got: Vec<(RefKind, &str, &str)> = parsed.refs.iter().map(|r| (r.kind.clone(), r.specifier.as_str(), r.conditions.as_str())).collect();
        assert_eq!(got, vec![(RefKind::Import, "./a.css", ""), (RefKind::Import, "b.css", "screen"), (RefKind::Url, "./img/p.png", ""), (RefKind::Url, "../q.svg", "")]);
    }

    #[test]
    fn replaces_urls_and_hoists_kept_imports() {
        let text = "@import \"./a.css\";\n.x { background: url(p.png); }\n";
        let parsed = parse(text, false).unwrap();
        let r = render(text, &parsed, &[Replacement::Keep, Replacement::Text("/assets/p-1.png".into())], None, false);
        assert_eq!(r.hoisted, vec!["@import \"./a.css\";".to_string()]);
        assert_eq!(r.css.trim(), ".x { background: url(/assets/p-1.png); }");
        assert_eq!(plain("a{}").css, "a{}");
    }

    #[test]
    fn local_names() {
        let r = modules(".btn { color: red } .btn:hover, #id > .a-b::after { x: .5em } :global(.keep) .btn { y: 1 } @keyframes spin { to { x: 1 } } .s { animation: spin 1s; }");
        assert_eq!(r.css, "h_btn { color: red } h_btn:hover, h_id > h_a-b::after { x: .5em } .keep h_btn { y: 1 } @keyframes h_spin { to { x: 1 } } h_s { animation: h_spin 1s; }".replace("h_btn", ".h_btn").replace("h_id", "#h_id").replace("h_a-b", ".h_a-b").replace("h_s {", ".h_s {").replace(" .h_s {", " .h_s {"));
        let names: Vec<&str> = r.exports.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(names, vec!["btn", "id", "a-b", "s"]);
    }

    #[test]
    fn minify_keeps_quoted_urls_whole() {
        let css = minify_text("a { background: url( \"data:x;t='(1) 2'  )\" ) ; b: 1 }");
        assert_eq!(css, "a{background:url(\"data:x;t='(1) 2'  )\");b:1}");
    }

    #[test]
    fn minify_says_where_offsets_went() {
        let text = "a {\n  color: red;\n}\n\nb { margin: 0 }";
        let (css, moved) = minify_mapped(text, &[0, 6, 20, 25]);
        assert_eq!(css, "a{color:red}b{margin:0}");
        let words: Vec<&str> = moved.iter().map(|&m| &css[m..m + 1]).collect();
        assert_eq!(words, vec!["a", "c", "b", "m"]);
    }

    #[test]
    fn composes_from_another_file_uses_its_seed() {
        let text = ".a { composes: x y from \"./o.module.css\"; composes: z; color: red } .z { x: 1 }";
        let parsed = parse(text, true).unwrap();
        assert_eq!(parsed.refs.len(), 1);
        assert_eq!(parsed.refs[0].kind, RefKind::Compose);
        assert_eq!(parsed.refs[0].specifier, "./o.module.css");
        let r = render(text, &parsed, &[Replacement::Seed("q".into())], Some("h"), false);
        assert_eq!(r.exports[0], ("a".into(), "h_a h_z q_x q_y".into()));
    }

    #[test]
    fn composes_joins_names() {
        let r = modules(".a { color: red } .b { composes: a; margin: 0 }");
        assert_eq!(r.exports, vec![("a".into(), "h_a".into()), ("b".into(), "h_b h_a".into())]);
        assert!(!r.css.contains("composes"));
    }

    #[test]
    fn minifies() {
        assert_eq!(minify_text("/* c */ a  >  b ,\n c {\n  color: red ;\n  margin: 0 auto;\n}\n/*! keep */ d{x:url( a b.png )}"), "a>b,c{color:red;margin:0 auto}/*! keep */ d{x:url(a b.png)}");
        assert_eq!(minify_text("a::before{content:\"a  b\"}"), "a:before{content:\"a  b\"}");
        assert_eq!(minify_text("é { x: 1 ; }"), "é{x:1}");
    }

    #[test]
    fn values_get_their_short_spelling() {
        assert_eq!(minify_text("a { color: #FFFFFF; b: #aabbccdd; c: #abcdef; d: #11223 }"), "a{color:#fff;b:#abcd;c:#abcdef;d:#11223}");
        assert_eq!(minify_text("a { margin: 0.5em -0.25em 10.5px 0; w: calc(1 + 0.5) ; x: a0.5 }"), "a{margin:.5em -.25em 10.5px 0;w:calc(1 + .5);x:a0.5}");
        // Selectors, strings, urls, filters and ids are not values.
        assert_eq!(minify_text("#ffffff, a:not(#aabbcc) { content: \"#ffffff 0.5\"; b: url(#ffffff0.5); filter: progid:X(c=#FFFFFF) }"), "#ffffff,a:not(#aabbcc){content:\"#ffffff 0.5\";b:url(#ffffff0.5);filter:progid:X(c=#FFFFFF)}");
    }

    #[test]
    fn a_rule_with_nothing_in_it_is_left_out() {
        assert_eq!(minify_text("a{} b{color:red} @media print{c{} d{}} @media screen{e{color:red} f{}} @keyframes k{} @layer x{} @font-face{} @supports (a:b){g{}}"), "b{color:red}@media screen{e{color:red}}@keyframes k{}@layer x{}@font-face{}");
        assert_eq!(minify_text("a{color:red;b{}} @import \"x.css\"; .c{content:\"{}\"}"), "a{color:red;b{}}@import \"x.css\";.c{content:\"{}\"}");
    }

    #[test]
    fn a_color_is_written_the_shorter_way() {
        assert_eq!(minify_text("a { color: black; background: white url(white.png) no-repeat; border: 1px solid Red; fill: #ff0000; stroke: yellow; outline: 1px solid #ffc0cb }"), "a{color:#000;background:#fff url(white.png) no-repeat;border:1px solid Red;fill:red;stroke:#ff0;outline:1px solid pink}");
        // A tie stays as written, and what is not a whole color token is left alone.
        assert_eq!(minify_text("a { color: blue; background-color: #0ff; border-color: gray; font-family: white; box-shadow: 0 0 var(--white) 2px #fffa }"), "a{color:blue;background-color:#0ff;border-color:gray;font-family:white;box-shadow:0 0 var(--white) 2px #fffa}");
        assert_eq!(minify_text("a { background: linear-gradient(to right, white, black 50%, #f00) }"), "a{background:linear-gradient(to right,#fff,#000 50%,red)}");
    }

    #[test]
    fn colors_with_alpha_get_their_hex_spelling() {
        assert_eq!(minify_text("a { b: rgba(255, 255, 255, 0.65); c: rgb(0 0 0 / 50%); d: rgba(0,0,0,0); e: rgba(1,2,3,1); f: rgba(17,34,51,.2) }"), "a{b:#ffffffa6;c:#00000080;d:#0000;e:#010203;f:#1233}");
        // A third decimal, a variable, a percentage channel and an unknown syntax keep their text.
        assert_eq!(minify_text("a { b: rgba(0,0,0,.333); c: rgba(var(--c),.5); d: rgb(100% 0% 0% / .5); e: rgba(1 2 3,.5); f: rgb(1 2 3 .5) }"), "a{b:rgba(0,0,0,.333);c:rgba(var(--c),.5);d:rgb(100% 0% 0% / .5);e:rgba(1 2 3,.5);f:rgb(1 2 3 .5)}");
        assert_eq!(minify_text("a { color: transparent; border: 1px solid transparent; background: url(transparent.png) transparent; font-family: transparent; box-shadow: 0px 1px 2px 0px rgba(0,0,0,.5), 0px 0px #fff }"), "a{color:#0000;border:1px solid #0000;background:url(transparent.png)#0000;font-family:transparent;box-shadow:0 1px 2px 0 #00000080,0 0 #fff}".replace(")#0000", ") #0000"));
    }

    #[test]
    fn values_with_a_shorter_equal_form() {
        assert_eq!(minify_text("a { flex: 1 1 auto; b: x } b { flex: 0 0 auto } c { flex: 0 1 100% } d { flex: 1 1 0% } e { flex: 2 1 10px } f { flex: 1 1 1 }"), "a{flex:auto;b:x}b{flex:none}c{flex:0 100%}d{flex:0%}e{flex:2 10px}f{flex:1 1 1}");
        assert_eq!(minify_text("a { transition: opacity 100ms ease, transform 1500ms ease-in; animation-duration: 250ms; transition-delay: -500ms; x: 100ms }"), "a{transition:opacity .1s,transform 1.5s ease-in;animation-duration:.25s;transition-delay:-.5s;x:100ms}");
        assert_eq!(minify_text("a { transition: opacity .3s ease } b { transition: ease .3s } c { transition: opacity .3s cubic-bezier(.4,0,.2,1), color 1s ease }"), "a{transition:opacity .3s}b{transition:ease .3s}c{transition:opacity .3s cubic-bezier(.4,0,.2,1),color 1s ease}");
        assert_eq!(minify_text("a { border: 1px solid currentColor; border-top: currentcolor; background: none; position: relative } b { background: transparent } c { background-position: 0 50%; transform-origin: center center }"), "a{border:1px solid;border-top:currentcolor;background:0 0;position:relative}b{background:0 0}c{background-position:0;transform-origin:50%}");
        assert_eq!(minify_text("a { transform: translateX(-50%) translate(10px, 0) translate(0, 10px) translate(calc(1px + 2px),0px) rotate(1deg) }"), "a{transform:translate(-50%)translate(10px)translate(0,10px)translate(calc(1px + 2px))rotate(1deg)}");
    }

    #[test]
    fn a_rule_that_comes_again_later_is_dropped() {
        assert_eq!(minify_text("a{x:1}b{y:2}a{x:1}"), "b{y:2}a{x:1}");
        assert_eq!(minify_text("a{x:1}b{y:2}a{x:2}"), "a{x:1}b{y:2}a{x:2}");
        assert_eq!(minify_text("@media print{a{x:1}b{y:2}a{x:1}}a{x:1}"), "@media print{b{y:2}a{x:1}}a{x:1}");
    }

    #[test]
    fn longhands_of_a_rule_are_written_as_the_shorthand() {
        assert_eq!(minify_text("a { margin-top: 1px; margin-right: 0px; margin-bottom: 1px; margin-left: 0px }"), "a{margin:1px 0}");
        assert_eq!(minify_text("a { padding: 0; padding-left: 4px }"), "a{padding:0 0 0 4px}");
    }

    #[test]
    fn import_conditions_become_nested_wrappers() {
        assert_eq!(import_wrappers("screen and (min-width: 1px)"), vec!["@media screen and (min-width: 1px)"]);
        assert_eq!(import_wrappers("layer(base) supports(display: grid) print"), vec!["@layer base", "@supports (display: grid)", "@media print"]);
        assert_eq!(import_wrappers("layer"), vec!["@layer"]);
        assert_eq!(import_wrappers("layer print"), vec!["@layer", "@media print"]);
        assert_eq!(import_wrappers("supports(not (display: grid))"), vec!["@supports not (display: grid)"]);
        assert_eq!(import_wrappers("supports(selector(a > b)) screen"), vec!["@supports selector(a > b)", "@media screen"]);
        assert!(import_wrappers("").is_empty());
    }

    #[test]
    fn a_url_keeps_its_fragment_and_query_for_after_the_file_name() {
        assert_eq!(url_target("img/a.png"), Some(("./img/a.png".into(), String::new())));
        assert_eq!(url_target("icons.svg#home"), Some(("./icons.svg".into(), "#home".into())));
        assert_eq!(url_target("font.eot?#iefix"), Some(("./font.eot".into(), "?#iefix".into())));
        assert_eq!(url_target("../a.png?v=3#x"), Some(("../a.png".into(), "?v=3#x".into())));
        assert_eq!(url_target("a.svg?raw#x"), Some(("./a.svg?raw".into(), "#x".into())));
        assert_eq!(url_target("~pkg/a.png"), Some(("pkg/a.png".into(), String::new())));
        assert_eq!(url_target("/fonts/a.woff2"), Some(("/fonts/a.woff2".into(), String::new())));
        assert_eq!(url_target("#grad"), None);
    }

    #[test]
    fn css_escapes_are_undone() {
        assert_eq!(unescape("a\\.png"), "a.png");
        assert_eq!(unescape("\\41 b"), "Ab");
        assert_eq!(unescape("plain"), "plain");
    }

    #[test]
    fn urls_are_found_in_every_form() {
        let text = "a { b: url(x.png#f); c: url( \"y z.png\" ); d: image-set(\"i.png\" 1x, url(j.png) 2x); e: url(a\\.png); f: url(data:x); g: url(#h); k: \"url(no.png)\" } @import \"~pkg/s.css\";";
        let parsed = parse(text, false).unwrap();
        let found: Vec<(&str, &str)> = parsed.refs.iter().map(|r| (r.specifier.as_str(), r.suffix.as_str())).collect();
        assert_eq!(found, vec![("./x.png", "#f"), ("./y z.png", ""), ("./i.png", ""), ("./j.png", ""), ("./a.png", ""), ("pkg/s.css", "")]);
    }

    #[test]
    fn a_rewritten_url_is_escaped_and_keeps_its_suffix() {
        let text = "a { b: url(x.png#f); c: url(\"y.png\") }";
        let parsed = parse(text, false).unwrap();
        let r = render(text, &parsed, &[Replacement::Text("./x (1).png".into()), Replacement::Text("./y.png".into())], None, false);
        assert_eq!(r.css, "a { b: url(./x%20%281%29.png#f); c: url(\"./y.png\") }");
    }

    #[test]
    fn charset_goes_to_the_hoisted_list() {
        let text = "@charset \"utf-8\";\na { b: 1 }";
        let r = render(text, &parse(text, false).unwrap(), &[], None, false);
        assert_eq!(r.hoisted, vec!["@charset \"utf-8\";".to_string()]);
        assert_eq!(r.css.trim(), "a { b: 1 }");
    }

    #[test]
    fn values_and_export_are_exported_and_substituted() {
        let r = modules("@value main: #123; @value both: main 1px; :export { fixed: 3px; token: main }\n.a { color: main; border: both } @media (min-width: main) { .b { x: 1 } }");
        assert_eq!(r.css.trim(), ".h_a { color: #123; border: #123 1px } @media (min-width: #123) { .h_b { x: 1 } }");
        let get = |k: &str| r.exports.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("main"), Some("#123"));
        assert_eq!(get("both"), Some("#123 1px"));
        assert_eq!(get("fixed"), Some("3px"));
        assert_eq!(get("token"), Some("#123"));
    }

    #[test]
    fn a_value_imported_from_another_sheet_is_taken_from_its_definitions() {
        let text = "@value a, b as c from \"./o.css\";\n.x { color: a; margin: c }";
        let parsed = parse(text, true).unwrap();
        assert_eq!(parsed.refs.len(), 1);
        let theirs = vec![("a".to_string(), "red".to_string()), ("b".to_string(), "2px".to_string())];
        let r = render(text, &parsed, &[Replacement::Sheet("q".into(), theirs)], Some("h"), false);
        assert_eq!(r.css.trim(), ".h_x { color: red; margin: 2px }");
    }
}
