//! Declaration-list rewrites for minified style sheets: longhand properties written as their shorthand, and a
//! longhand that follows its shorthand folded into it.
//!
//! The text is a rule body as the minifier writes it (`a:1;b:2`, no braces). A rewrite is made only where the
//! result computes the same values in every browser that reads the shorthand:
//! - the longhands sit next to each other (nothing between them can read or reset one of them),
//! - every value is one plain token (no `var()`, `env()`, `attr()`, no `!important` unless all share it, no
//!   CSS-wide keyword, which a shorthand cannot mix with other values),
//! - each longhand is there once.
//!
//! `margin-top`..`margin-left`, `padding-*`, `border-*-width`/`-style`/`-color` and the four corner radii are
//! handled; shorthands with support gaps (`inset`, `gap`, the two-value `overflow`) are not made.

use crate::css::skip_to;

/// A shorthand and its longhands in the order its four values take.
struct Family {
    short: &'static str,
    longs: [&'static str; 4],
}

const FAMILIES: &[Family] = &[
    Family { short: "margin", longs: ["margin-top", "margin-right", "margin-bottom", "margin-left"] },
    Family { short: "padding", longs: ["padding-top", "padding-right", "padding-bottom", "padding-left"] },
    Family { short: "border-width", longs: ["border-top-width", "border-right-width", "border-bottom-width", "border-left-width"] },
    Family { short: "border-style", longs: ["border-top-style", "border-right-style", "border-bottom-style", "border-left-style"] },
    Family { short: "border-color", longs: ["border-top-color", "border-right-color", "border-bottom-color", "border-left-color"] },
    Family { short: "border-radius", longs: ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"] },
];

#[derive(Clone)]
struct Decl {
    name: String,
    value: String,
    important: bool,
}

impl Decl {
    fn write(&self, out: &mut String) {
        out.push_str(&self.name);
        out.push(':');
        out.push_str(&self.value);
        if self.important {
            out.push_str("!important");
        }
    }
}

/// The declarations of a body, or `None` when it holds anything but plain `name:value` parts (an at-rule, a nested rule).
fn split(body: &str) -> Option<Vec<Decl>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let end = skip_to(body, i, &[b';', b'{', b'}']);
        if end < body.len() && body.as_bytes()[end] != b';' {
            return None;
        }
        let part = &body[i..end];
        i = (end + 1).min(body.len() + 1);
        if part.is_empty() {
            continue;
        }
        let colon = part.find(':')?;
        let name = part[..colon].trim();
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            return None;
        }
        let (value, important) = match part[colon + 1..].strip_suffix("!important") {
            Some(v) => (v, true),
            None => (&part[colon + 1..], false),
        };
        out.push(Decl { name: name.to_string(), value: value.to_string(), important });
    }
    Some(out)
}

/// The value as parts split at spaces outside parentheses, when it is made only of plain words, numbers, colors and calls.
fn parts(value: &str) -> Option<Vec<&str>> {
    let lower = value.to_ascii_lowercase();
    if value.is_empty() || value.contains(['"', '\'', '\\', '/', '!', '{', '}', ';']) || lower.contains("var(") || lower.contains("env(") || lower.contains("attr(") || lower.contains("--") {
        return None;
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in value.bytes().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b' ' if depth == 0 => {
                if i > start {
                    out.push(&value[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    if depth != 0 {
        return None;
    }
    if start < value.len() {
        out.push(&value[start..]);
    }
    let keyword = |p: &str| matches!(p.to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer");
    if out.is_empty() || out.iter().any(|p| keyword(p)) {
        return None;
    }
    Some(out)
}

/// Four values, written with as few as mean the same (`top right bottom left`).
fn reduce(v: [&str; 4]) -> String {
    // A zero length needs no unit (every longhand of these families is a length, a color or a style keyword).
    fn z(x: &str) -> &str {
        if crate::css::is_zero_length(x) { "0" } else { x }
    }
    let [t, r, b, l] = [z(v[0]), z(v[1]), z(v[2]), z(v[3])];
    if l == r {
        if b == t {
            if r == t {
                return t.to_string();
            }
            return format!("{t} {r}");
        }
        return format!("{t} {r} {b}");
    }
    format!("{t} {r} {b} {l}")
}

/// The four sides of a shorthand value of one to four parts.
fn expand<'a>(p: &[&'a str]) -> Option<[&'a str; 4]> {
    match p.len() {
        1 => Some([p[0]; 4]),
        2 => Some([p[0], p[1], p[0], p[1]]),
        3 => Some([p[0], p[1], p[2], p[1]]),
        4 => Some([p[0], p[1], p[2], p[3]]),
        _ => None,
    }
}

/// The rewritten body; the text itself when nothing applies or the body is not a plain list of declarations.
pub fn fold(body: &str) -> String {
    if !body.contains(':') || (!body.contains("-top") && !body.contains("-right") && !body.contains("-bottom") && !body.contains("-left")) {
        return body.to_string();
    }
    let Some(mut decls) = split(body) else { return body.to_string() };
    let mut changed = false;
    for fam in FAMILIES {
        let mut i = 0;
        while i < decls.len() {
            let name = decls[i].name.to_ascii_lowercase();
            // A shorthand followed by longhands of its own sides: the longhands are written into it.
            if name == fam.short {
                let mut j = i + 1;
                let mut seen = [false; 4];
                while j < decls.len() {
                    let n = decls[j].name.to_ascii_lowercase();
                    match fam.longs.iter().position(|l| *l == n) {
                        Some(k) if !seen[k] => {
                            seen[k] = true;
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if j > i + 1 {
                    let imp = decls[i].important;
                    let sides = {
                        let p = parts(&decls[i].value);
                        p.and_then(|p| expand(&p).map(|a| a.map(str::to_string)))
                    };
                    let ok = sides.is_some() && decls[i + 1..j].iter().all(|d| d.important == imp && parts(&d.value).is_some_and(|p| p.len() == 1));
                    if ok {
                        let mut sides = sides.unwrap();
                        for d in &decls[i + 1..j] {
                            let k = fam.longs.iter().position(|l| *l == d.name.to_ascii_lowercase()).unwrap();
                            sides[k] = d.value.clone();
                        }
                        decls[i].value = reduce([&sides[0], &sides[1], &sides[2], &sides[3]]);
                        decls.drain(i + 1..j);
                        changed = true;
                    }
                }
                i += 1;
                continue;
            }
            // Four longhands in a row: the shorthand.
            if fam.longs.contains(&name.as_str()) {
                let mut j = i;
                while j < decls.len() && fam.longs.contains(&decls[j].name.to_ascii_lowercase().as_str()) {
                    j += 1;
                }
                if j - i == 4 {
                    let imp = decls[i].important;
                    let mut vals: [Option<&str>; 4] = [None; 4];
                    let mut ok = true;
                    for d in &decls[i..j] {
                        let k = fam.longs.iter().position(|l| *l == d.name.to_ascii_lowercase()).unwrap();
                        let single = parts(&d.value).is_some_and(|p| p.len() == 1);
                        if vals[k].is_some() || !single || d.important != imp {
                            ok = false;
                            break;
                        }
                        vals[k] = Some(&d.value);
                    }
                    if ok {
                        let v = [vals[0].unwrap(), vals[1].unwrap(), vals[2].unwrap(), vals[3].unwrap()];
                        let value = reduce(v);
                        decls.splice(i..j, [Decl { name: fam.short.to_string(), value, important: imp }]);
                        changed = true;
                        i += 1;
                        continue;
                    }
                }
                i = j.max(i + 1);
                continue;
            }
            i += 1;
        }
    }
    if !changed {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    for (k, d) in decls.iter().enumerate() {
        if k > 0 {
            out.push(';');
        }
        d.write(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn four_sides_in_a_row_become_the_shorthand() {
        assert_eq!(fold("margin-top:1px;margin-right:2px;margin-bottom:3px;margin-left:4px"), "margin:1px 2px 3px 4px");
        assert_eq!(fold("color:red;padding-left:2px;padding-top:1px;padding-right:2px;padding-bottom:1px;x:y"), "color:red;padding:1px 2px;x:y");
        assert_eq!(fold("margin-top:0;margin-right:0;margin-bottom:0;margin-left:0"), "margin:0");
        assert_eq!(fold("border-top-color:red;border-right-color:red;border-bottom-color:blue;border-left-color:red"), "border-color:red red blue");
        assert_eq!(fold("border-top-left-radius:1px;border-top-right-radius:2px;border-bottom-right-radius:1px;border-bottom-left-radius:2px"), "border-radius:1px 2px");
    }

    #[test]
    fn a_longhand_after_its_shorthand_is_folded_in() {
        assert_eq!(fold("margin:0 auto;margin-top:5px"), "margin:5px auto 0");
        assert_eq!(fold("padding:1px;padding-left:2px;padding-right:2px;color:red"), "padding:1px 2px;color:red");
        assert_eq!(fold("margin:0;margin-top:1px;margin-top:2px"), "margin:1px 0 0;margin-top:2px");
    }

    #[test]
    fn anything_that_could_differ_is_left_alone() {
        // not all four, not next to each other, a var(), a keyword, a different importance, two parts, a gap in between
        for body in [
            "margin-top:1px;margin-right:2px;margin-bottom:3px",
            "margin-top:1px;margin-right:2px;color:red;margin-bottom:3px;margin-left:4px",
            "margin-top:var(--a);margin-right:2px;margin-bottom:3px;margin-left:4px",
            "margin-top:inherit;margin-right:2px;margin-bottom:3px;margin-left:4px",
            "margin-top:1px!important;margin-right:2px;margin-bottom:3px;margin-left:4px",
            "border-top-left-radius:1px 2px;border-top-right-radius:2px;border-bottom-right-radius:1px;border-bottom-left-radius:2px",
            "margin-top:1px;margin-right:2px;margin-top:3px;margin-left:4px",
            "margin:var(--a);margin-top:5px",
            "margin:0!important;margin-top:5px",
            "margin:0 0 0 0 0;margin-top:5px",
            "margin:0;margin-top:5px 6px",
        ] {
            assert_eq!(fold(body), body);
        }
    }

    #[test]
    fn calls_with_spaces_count_as_one_value() {
        assert_eq!(fold("margin-top:calc(1px + 2px);margin-right:0;margin-bottom:calc(1px + 2px);margin-left:0"), "margin:calc(1px + 2px) 0");
    }
}
