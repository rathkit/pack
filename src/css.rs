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
}

pub struct Parsed {
    pieces: Vec<Piece>,
    pub refs: Vec<Ref>,
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

/// Splits `text`. With `local`, class names, ids and keyframe names in selectors are marked.
pub fn parse(text: &str, local: bool) -> Result<Parsed, String> {
    let bytes = text.as_bytes();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut refs: Vec<Ref> = Vec::new();
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
                i = (j + 1).min(bytes.len());
            }
            b'(' => {
                parens += 1;
                i += 1;
            }
            b')' => {
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
                if name == "import" {
                    let (spec, after) = import_target(text, j)?;
                    let end = find_rule_end(text, after).ok_or_else(|| format!("@import at {i} has no end"))?;
                    let conditions = text[after..end].trim().to_string();
                    let stop = (end + 1).min(bytes.len());
                    flush!(i);
                    if is_loadable(&spec) {
                        refs.push(Ref { kind: RefKind::Import, specifier: spec, conditions });
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
                        k += 1;
                    }
                    (j, j + text[j..k].trim_end().len(), k)
                };
                let spec = &text[start..end];
                if is_loadable(spec) {
                    let specifier = if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') { spec.to_string() } else { format!("./{spec}") };
                    flush!(start);
                    refs.push(Ref { kind: RefKind::Url, specifier, conditions: String::new() });
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
            _ if local && depth > 0 && is_ident_start(c as char) && (i == 0 || !is_ident_char(bytes[i - 1] as char)) => {
                let mut j = i;
                while j < bytes.len() && is_ident_char(bytes[j] as char) {
                    j += 1;
                }
                let j = j.max(i + 1);
                let word = text[i..j].to_ascii_lowercase();
                let at_start = text[statement_start..i].trim().is_empty();
                let after = text[j..].trim_start();
                if at_start && after.starts_with(':') && (word == "animation" || word == "animation-name") {
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
                            refs.push(Ref { kind: RefKind::Compose, specifier, conditions: String::new() });
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
    Ok(Parsed { pieces, refs })
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
                Replacement::Text(t) => out.push_str(t),
                Replacement::Seed(_) => {}
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
            Piece::Drop(..) => {}
            Piece::Hoist(s, e) => hoisted.push(text[*s..*e].trim().to_string()),
            Piece::ComposesFrom(composed, index) => {
                let seed = match &replacements[*index] {
                    Replacement::Seed(seed) => Some(seed.as_str()),
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
    let (css, anchors) = if minify {
        let offsets: Vec<usize> = anchors.iter().map(|a| a.0).collect();
        let (css, moved) = minify_mapped(&out, &offsets);
        (css, moved.into_iter().zip(anchors.iter().map(|a| a.1)).collect())
    } else {
        (out, anchors)
    };
    Rendered { css, hoisted, exports, anchors }
}

/// Removes comments (but not `/*! ... */`) and whitespace that does not matter.
pub fn minify_text(text: &str) -> String {
    minify_mapped(text, &[]).0
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
        assert_eq!(minify_text("a::before{content:\"a  b\"}"), "a::before{content:\"a  b\"}");
        assert_eq!(minify_text("é { x: 1 ; }"), "é{x:1}");
    }
}
