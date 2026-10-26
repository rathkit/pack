//! `import.meta.env`, as Vite makes it: the build's mode and `.env` files, as definitions of `import.meta.env.NAME`.
//!
//! The files are `.env`, `.env.local`, `.env.[mode]` and `.env.[mode].local` in the project folder (a later one wins), and only names
//! that start with a prefix (`VITE_`) are given to the program: the rest of a `.env` file is for tools, and stays out of the output.
//! A variable of that kind in the real environment wins over the files. `MODE`, `DEV`, `PROD`, `SSR` and `BASE_URL` are always there.

use std::path::{Path, PathBuf};

use crate::define::Define;
use crate::json::Json;

/// What a build asks of `import.meta.env`.
pub struct EnvOptions<'a> {
    pub dir: &'a Path,
    pub mode: &'a str,
    /// `serve` (a development server) rather than a build: decides `DEV` and `PROD` for a mode that says neither.
    pub serve: bool,
    pub prefixes: &'a [String],
    pub base: &'a str,
    /// Read the files (false: only the built-in names).
    pub files: bool,
}

/// The `.env` files that are looked for, in the order they are read.
pub fn env_files(dir: &Path, mode: &str) -> Vec<PathBuf> {
    [".env".to_string(), ".env.local".to_string(), format!(".env.{mode}"), format!(".env.{mode}.local")].iter().map(|n| dir.join(n)).collect()
}

/// One variable of a `.env` file: its name, its text, and whether `$NAME` in it is to be replaced (not in single quotes).
pub type Entry = (String, String, bool);

/// `KEY=VALUE` lines: comments, `export`, quotes (a double-quoted value may span lines and has `\n`), and inline comments after unquoted
/// values. A name set again replaces the earlier one, in this file and in the ones read before it. Nothing is expanded yet: see `resolve`.
pub fn parse_dotenv(text: &str, known: &mut Vec<Entry>) {
    let chars: Vec<char> = text.replace("\r\n", "\n").replace('\r', "\n").chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // A line: spaces, an optional `export `, the name.
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '\n' || chars[i] == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if chars[i..].starts_with(&['e', 'x', 'p', 'o', 'r', 't', ' ']) {
            i += 7;
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
        }
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '-')) {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        if name.is_empty() || chars.get(i) != Some(&'=') {
            // Not an assignment: the rest of the line is skipped.
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        let (value, expand) = match chars.get(i) {
            Some(&q @ ('"' | '\'' | '`')) => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != q {
                    j += 1;
                }
                if j >= chars.len() {
                    // No closing quote: the line is the value, quote included.
                    let mut k = i;
                    while k < chars.len() && chars[k] != '\n' {
                        k += 1;
                    }
                    let raw: String = chars[i..k].iter().collect();
                    i = k;
                    (raw.trim().to_string(), true)
                } else {
                    let mut raw: String = chars[i + 1..j].iter().collect();
                    if q == '"' {
                        raw = raw.replace("\\n", "\n").replace("\\r", "\r");
                    }
                    i = j + 1;
                    // What follows the closing quote on the line (a comment) is dropped.
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    (raw, q != '\'')
                }
            }
            _ => {
                let mut k = i;
                while k < chars.len() && chars[k] != '\n' {
                    k += 1;
                }
                let mut raw: String = chars[i..k].iter().collect();
                // ` #` starts a comment after an unquoted value.
                if let Some(at) = raw.find(" #").or_else(|| raw.starts_with('#').then_some(0)) {
                    raw.truncate(at);
                }
                i = k;
                (raw.trim().to_string(), true)
            }
        };
        known.retain(|(k, _, _)| *k != name);
        known.push((name, value, expand));
    }
}

/// The values with `$NAME`, `${NAME}` and `${NAME:-default}` replaced, as Vite does it: once all the files are merged, so a value
/// refers to the final value of another (which may refer to a third), and then to `lookup` (the real environment).
pub fn resolve(entries: &[Entry], lookup: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    entries.iter().map(|(name, value, expand)| (name.clone(), if *expand { expand_value(value, entries, lookup, 0) } else { value.clone() })).collect()
}

fn expand_value(value: &str, entries: &[Entry], lookup: &dyn Fn(&str) -> Option<String>, depth: usize) -> String {
    // A value that refers to itself (through others) stops being expanded where it started repeating.
    let find = |name: &str| match entries.iter().find(|(k, _, _)| k == name) {
        Some((_, v, expand)) if *expand && depth < 10 => Some(expand_value(v, entries, lookup, depth + 1)),
        Some((_, v, _)) => Some(v.clone()),
        None => lookup(name),
    };
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if chars.get(i + 1) == Some(&'$') => {
                out.push('$');
                i += 2;
            }
            '$' if chars.get(i + 1) == Some(&'{') => match chars[i + 2..].iter().position(|&c| c == '}') {
                Some(end) => {
                    let inner: String = chars[i + 2..i + 2 + end].iter().collect();
                    let (name, default) = match inner.split_once(":-") {
                        Some((n, d)) => (n.to_string(), Some(d.to_string())),
                        None => (inner, None),
                    };
                    out.push_str(&find(&name).filter(|v| !v.is_empty()).or(default).unwrap_or_default());
                    i += end + 3;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            },
            '$' if chars.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_') => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                out.push_str(&find(&name).unwrap_or_default());
                i = j;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// `DEV` and `PROD`: a mode that says `development` or `production` decides; any other follows the command.
pub fn dev_and_prod(mode: &str, serve: bool) -> (bool, bool) {
    match mode {
        "development" => (true, false),
        "production" => (false, true),
        _ => (serve, !serve),
    }
}

/// The definitions of `import.meta.env.NAME` for each name, and of `import.meta.env` itself as an object of them all.
/// `overrides`: what the user defined as `import.meta.env.NAME` (name, expression); they are in the object too.
pub fn definitions(options: &EnvOptions, real: &dyn Fn(&str) -> Option<String>, overrides: &[(String, String)]) -> Result<(Vec<Define>, Vec<PathBuf>), String> {
    let (dev, prod) = dev_and_prod(options.mode, options.serve);
    let mut values: Vec<(String, String)> = Vec::new();
    let mut read: Vec<PathBuf> = Vec::new();
    if options.files {
        let mut entries: Vec<Entry> = Vec::new();
        for file in env_files(options.dir, options.mode) {
            if let Ok(text) = std::fs::read_to_string(&file) {
                parse_dotenv(&text, &mut entries);
                read.push(file);
            }
        }
        for (name, value) in resolve(&entries, real) {
            if options.prefixes.iter().any(|p| name.starts_with(p.as_str())) {
                values.push((name, value));
            }
        }
    }
    // The real environment wins, for the names that are given to the program.
    let mut from_environment: Vec<String> = std::env::vars().map(|(k, _)| k).filter(|k| options.prefixes.iter().any(|p| k.starts_with(p.as_str()))).collect();
    from_environment.sort();
    for name in from_environment {
        if let Some(value) = real(&name) {
            values.retain(|(k, _)| *k != name);
            values.push((name, value));
        }
    }
    let mut object: Vec<(String, Json, String)> = Vec::new();
    let mut add = |name: &str, json: Json, text: String| object.push((name.to_string(), json, text));
    add("BASE_URL", Json::String(options.base.to_string()), Json::String(options.base.to_string()).to_string());
    add("MODE", Json::String(options.mode.to_string()), Json::String(options.mode.to_string()).to_string());
    add("DEV", Json::Bool(dev), dev.to_string());
    add("PROD", Json::Bool(prod), prod.to_string());
    add("SSR", Json::Bool(false), "false".to_string());
    for (name, value) in &values {
        if matches!(name.as_str(), "BASE_URL" | "MODE" | "DEV" | "PROD" | "SSR") {
            continue;
        }
        add(name, Json::String(value.clone()), Json::String(value.clone()).to_string());
    }
    for (name, text) in overrides {
        match object.iter_mut().find(|(n, _, _)| n == name) {
            Some(entry) => entry.2 = text.clone(),
            None => object.push((name.clone(), Json::Null, text.clone())),
        }
    }
    let mut definitions = Vec::new();
    for (name, _, text) in &object {
        definitions.push(Define::parse(&format!("import.meta.env.{name}"), text).map_err(|m| format!("import.meta.env.{name}: {m}"))?);
    }
    let whole: Vec<String> = object.iter().map(|(name, _, text)| format!("{}:{text}", Json::String(name.clone()))).collect();
    definitions.push(Define::parse("import.meta.env", &format!("{{{}}}", whole.join(","))).map_err(|m| format!("import.meta.env: {m}"))?);
    Ok((definitions, read))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<(String, String)> {
        let mut known = Vec::new();
        parse_dotenv(text, &mut known);
        resolve(&known, &|n| (n == "FROM_ENV").then(|| "outside".to_string()))
    }

    fn get<'a>(list: &'a [(String, String)], name: &str) -> Option<&'a str> {
        list.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    #[test]
    fn plain_quoted_and_commented_values() {
        let v = parse("# a comment\nA=1\n  B = two words  \nexport C=3\nD=\"quoted # not a comment\" # a comment\nE='single $A'\nF=unquoted # comment\nG=\n\n");
        assert_eq!(get(&v, "A"), Some("1"));
        assert_eq!(get(&v, "B"), Some("two words"));
        assert_eq!(get(&v, "C"), Some("3"));
        assert_eq!(get(&v, "D"), Some("quoted # not a comment"));
        assert_eq!(get(&v, "E"), Some("single $A"), "single quotes do not expand");
        assert_eq!(get(&v, "F"), Some("unquoted"));
        assert_eq!(get(&v, "G"), Some(""));
    }

    #[test]
    fn multiline_and_escapes() {
        let v = parse("A=\"line one\nline two\"\nB=\"a\\nb\"\nC=`back ticks \"inside\"`\nD=after\n");
        assert_eq!(get(&v, "A"), Some("line one\nline two"));
        assert_eq!(get(&v, "B"), Some("a\nb"));
        assert_eq!(get(&v, "C"), Some("back ticks \"inside\""));
        assert_eq!(get(&v, "D"), Some("after"));
    }

    #[test]
    fn expansion() {
        let v = parse("A=1\nB=$A-${A}-${MISSING:-fallback}-${FROM_ENV}-\\$A\nC=\"x${A}\"\nWINDOWS=\r\nD=1\r\n");
        assert_eq!(get(&v, "B"), Some("1-1-fallback-outside-$A"));
        assert_eq!(get(&v, "C"), Some("x1"));
        assert_eq!(get(&v, "D"), Some("1"));
    }

    #[test]
    fn a_value_refers_to_the_final_value_of_another_across_files() {
        let mut known = Vec::new();
        parse_dotenv("B=base\nREF=${B}-ref\nDEEP=${REF}!\n", &mut known);
        parse_dotenv("B=local\n", &mut known);
        let v = resolve(&known, &|_| None);
        assert_eq!(get(&v, "REF"), Some("local-ref"));
        assert_eq!(get(&v, "DEEP"), Some("local-ref!"));
        let mut loop_ = Vec::new();
        parse_dotenv("A=${B}\nB=${A}\n", &mut loop_);
        assert_eq!(resolve(&loop_, &|_| None).len(), 2, "a loop ends");
    }

    #[test]
    fn a_later_value_replaces_an_earlier_one() {
        let v = parse("A=1\nB=2\nA=3\n");
        assert_eq!(get(&v, "A"), Some("3"));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn dev_and_prod_by_mode() {
        assert_eq!(dev_and_prod("development", false), (true, false));
        assert_eq!(dev_and_prod("production", true), (false, true));
        assert_eq!(dev_and_prod("staging", false), (false, true));
        assert_eq!(dev_and_prod("staging", true), (true, false));
    }
}
