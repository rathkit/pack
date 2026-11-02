//! `import.meta.glob("./dir/*.js", { eager, import, query, base })` (Vite's): the pattern is expanded
//! against the file system when the module is read, and the call becomes an object of imports.
//!
//! The arguments must be literals. Patterns start with `./`, `../` or `/` (the project root); a pattern
//! that starts with `!` takes files out. `*`, `**`, `?`, `[a-z]` and `{a,b}` are understood; names
//! that start with a dot and `node_modules` are not matched unless the pattern names them.

use std::path::{Path, PathBuf};

use crate::resolve::{Alias, FileSystem};

/// One piece of a name pattern.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Lit(char),
    Any,
    One,
    Class(Vec<(char, char)>, bool),
}

/// A path pattern: segments between `/`, where `**` stands for any number of directories.
#[derive(Clone, Debug)]
struct Glob {
    segments: Vec<Option<Vec<Tok>>>,
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let Some(open) = chars.iter().position(|&c| c == '{') else { return vec![pattern.to_string()] };
    let mut depth = 0;
    let mut close = None;
    let mut commas = Vec::new();
    for (i, &c) in chars.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            ',' if depth == 1 => commas.push(i),
            _ => {}
        }
    }
    let Some(close) = close else { return vec![pattern.to_string()] };
    let head: String = chars[..open].iter().collect();
    let tail: String = chars[close + 1..].iter().collect();
    let mut bounds = vec![open];
    bounds.extend(commas);
    bounds.push(close);
    let mut out = Vec::new();
    for pair in bounds.windows(2) {
        let alternative: String = chars[pair[0] + 1..pair[1]].iter().collect();
        for expanded in expand_braces(&format!("{head}{alternative}{tail}")) {
            out.push(expanded);
        }
    }
    out
}

fn compile_segment(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                if out.last() != Some(&Tok::Any) {
                    out.push(Tok::Any);
                }
            }
            '?' => out.push(Tok::One),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                out.push(Tok::Lit(chars[i]));
            }
            '[' => {
                let mut j = i + 1;
                let negated = matches!(chars.get(j), Some('!' | '^'));
                if negated {
                    j += 1;
                }
                let start = j;
                let mut ranges = Vec::new();
                while j < chars.len() && (chars[j] != ']' || j == start) {
                    if j + 2 < chars.len() && chars[j + 1] == '-' && chars[j + 2] != ']' {
                        ranges.push((chars[j], chars[j + 2]));
                        j += 3;
                    } else {
                        ranges.push((chars[j], chars[j]));
                        j += 1;
                    }
                }
                if j < chars.len() {
                    out.push(Tok::Class(ranges, negated));
                    i = j;
                } else {
                    out.push(Tok::Lit('['));
                }
            }
            c => out.push(Tok::Lit(c)),
        }
        i += 1;
    }
    out
}

fn match_name(tokens: &[Tok], name: &[char]) -> bool {
    match tokens.split_first() {
        None => name.is_empty(),
        Some((Tok::Any, rest)) => (0..=name.len()).any(|n| match_name(rest, &name[n..])),
        Some((Tok::One, rest)) => !name.is_empty() && match_name(rest, &name[1..]),
        Some((Tok::Lit(c), rest)) => name.first() == Some(c) && match_name(rest, &name[1..]),
        Some((Tok::Class(ranges, negated), rest)) => name.first().is_some_and(|c| ranges.iter().any(|(a, b)| a <= c && c <= b) != *negated) && match_name(rest, &name[1..]),
    }
}

impl Glob {
    /// A pattern with braces becomes several globs.
    fn compile(pattern: &str) -> Vec<Glob> {
        expand_braces(pattern)
            .into_iter()
            .map(|p| Glob { segments: p.split('/').filter(|s| !s.is_empty() && *s != ".").map(|s| if s == "**" { None } else { Some(compile_segment(s)) }).collect() })
            .collect()
    }

    /// Whether the names (directories, then the file) match. Names with a leading dot, and `node_modules`,
    /// match only a segment that spells them out.
    fn is_match(&self, names: &[&str]) -> bool {
        fn go(segments: &[Option<Vec<Tok>>], names: &[&str]) -> bool {
            match segments.split_first() {
                None => names.is_empty(),
                Some((None, rest)) => {
                    // `**` takes any number of plain directories.
                    if go(rest, names) {
                        return true;
                    }
                    match names.split_first() {
                        Some((first, tail)) if !first.starts_with('.') && *first != "node_modules" => go(segments, tail),
                        _ => false,
                    }
                }
                Some((Some(tokens), rest)) => match names.split_first() {
                    Some((first, tail)) => {
                        let spelled = matches!(tokens.first(), Some(Tok::Lit('.'))) || (*first == "node_modules" && tokens.iter().all(|t| matches!(t, Tok::Lit(_))));
                        (!first.starts_with('.') && *first != "node_modules" || spelled) && match_name(tokens, &first.chars().collect::<Vec<_>>()) && go(rest, tail)
                    }
                    None => false,
                },
            }
        }
        go(&self.segments, names)
    }

    /// The directories before the first segment that is not a plain name: where to start looking.
    fn prefix(&self) -> Vec<String> {
        let mut out = Vec::new();
        for segment in &self.segments {
            match segment {
                Some(tokens) if tokens.iter().all(|t| matches!(t, Tok::Lit(_))) => out.push(tokens.iter().map(|t| if let Tok::Lit(c) = t { *c } else { ' ' }).collect()),
                _ => break,
            }
        }
        out
    }
}

/// Files under `dir` (relative names, `/`-separated) in sorted order, reading only what the pattern can reach.
fn walk(fs: &dyn FileSystem, dir: &Path, rel: &mut Vec<String>, globs: &[Glob], depth_left: usize, out: &mut Vec<String>, dirs: &mut Vec<PathBuf>) {
    let Ok(mut entries) = fs.read_dir(dir) else { return };
    dirs.push(dir.to_path_buf());
    entries.sort();
    for (name, is_dir) in entries {
        rel.push(name.clone());
        if is_dir {
            let hidden = name == "node_modules" || name.starts_with('.');
            // A directory like that is entered only when a pattern spells such a name.
            let spelled = globs.iter().any(|g| g.segments.iter().any(|s| matches!(s, Some(t) if (t.first() == Some(&Tok::Lit('.')) && name.starts_with('.')) || (name == "node_modules" && t.iter().all(|t| matches!(t, Tok::Lit(_))) && t.len() == name.len()))));
            if depth_left > 0 && (!hidden || spelled) {
                walk(fs, &dir.join(&name), rel, globs, depth_left - 1, out, dirs);
            }
        } else {
            let names: Vec<&str> = rel.iter().map(String::as_str).collect();
            if globs.iter().any(|g| g.is_match(&names)) {
                out.push(rel.join("/"));
            }
        }
        rel.pop();
    }
}

/// `pattern` with a leading alias replaced: a path under the project becomes `/inside/the/project` and any other `../..` from the module.
fn through_aliases(pattern: &str, aliases: &[Alias], root: &Path, module_dir: &Path) -> String {
    let (mark, text) = match pattern.strip_prefix('!') {
        Some(rest) => ("!", rest),
        None => ("", pattern),
    };
    if text.starts_with("./") || text.starts_with("../") || text.starts_with('/') {
        return pattern.to_string();
    }
    let Some(replaced) = aliases.iter().find_map(|a| a.substitute(text)) else { return pattern.to_string() };
    let absolute = Path::new(&replaced);
    if let Ok(inside) = absolute.strip_prefix(root) {
        return format!("{mark}/{}", inside.to_string_lossy().replace('\\', "/"));
    }
    // Outside the project: as many `..` as it takes from the module's folder.
    let mut up = String::new();
    let mut base = module_dir.to_path_buf();
    while !absolute.starts_with(&base) {
        if !base.pop() {
            return pattern.to_string();
        }
        up.push_str("../");
    }
    format!("{mark}{up}{}", absolute.strip_prefix(&base).unwrap_or(absolute).to_string_lossy().replace('\\', "/"))
}

/// The files that a list of patterns reaches (a `!` pattern takes files out), as absolute paths, found from `base_dir` or, for a pattern
/// that starts with `/`, from the project `root`.
fn expand(patterns: &[String], base_dir: &Path, root: &Path, fs: &dyn FileSystem, dirs: &mut Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
        let mut positive: Vec<(PathBuf, Glob)> = Vec::new();
        let mut negative: Vec<(PathBuf, Glob)> = Vec::new();
        for pattern in patterns {
            let (negated, text) = match pattern.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pattern.as_str()),
            };
            let from = if text.starts_with('/') {
                root.to_path_buf()
            } else if text.starts_with("./") || text.starts_with("../") {
                base_dir.to_path_buf()
            } else {
                return Err(format!("the pattern \"{pattern}\" must start with ./, ../ or /"));
            };
            let target = if text.starts_with('/') { text.trim_start_matches('/').to_string() } else { text.to_string() };
            // `..` segments move the directory, so the glob only sees names.
            let mut dir = from;
            let mut rest = target.as_str();
            loop {
                if let Some(r) = rest.strip_prefix("../") {
                    dir.pop();
                    rest = r;
                } else if let Some(r) = rest.strip_prefix("./") {
                    rest = r;
                } else {
                    break;
                }
            }
            for glob in Glob::compile(rest) {
                if negated { negative.push((dir.clone(), glob)) } else { positive.push((dir.clone(), glob)) }
            }
        }
        // Every file the positive patterns reach, as an absolute path.
        let mut files: Vec<PathBuf> = Vec::new();
        for (dir, glob) in &positive {
            let prefix = glob.prefix();
            // A pattern with no wildcard names one file.
            let (start, skip) = if prefix.len() == glob.segments.len() { (dir.join(prefix[..prefix.len().saturating_sub(1)].join("/")), prefix.len().saturating_sub(1)) } else { (dir.join(prefix.join("/")), prefix.len()) };
            let mut found = Vec::new();
            let mut rel: Vec<String> = Vec::new();
            let shifted = Glob { segments: glob.segments[skip..].to_vec() };
            walk(fs, &start, &mut rel, std::slice::from_ref(&shifted), 64, &mut found, dirs);
            for name in found {
                let path = start.join(name);
                if !files.contains(&path) {
                    files.push(path);
                }
            }
        }
        files.retain(|f| {
            !negative.iter().any(|(dir, glob)| {
                let Ok(rel) = f.strip_prefix(dir) else { return false };
                let names: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                glob.is_match(&names)
            })
        });
        Ok(files)
}

/// What a call asks for.
#[derive(Default)]
struct Options {
    eager: bool,
    import: Option<String>,
    query: String,
    base: Option<String>,
}

/// A literal in the arguments.
#[derive(Debug, Clone)]
enum Lit {
    Str(String),
    Bool(bool),
    Num(f64),
    Arr(Vec<Lit>),
    Obj(Vec<(String, Lit)>),
}

struct Reader<'a> {
    text: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        loop {
            while self.pos < self.text.len() && self.text[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.text[self.pos..].starts_with(b"//") {
                while self.pos < self.text.len() && self.text[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else if self.text[self.pos..].starts_with(b"/*") {
                match find(&self.text[self.pos + 2..], b"*/") {
                    Some(n) => self.pos += n + 4,
                    None => self.pos = self.text.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.pos).copied()
    }

    fn string(&mut self, quote: u8) -> Result<String, String> {
        self.pos += 1;
        let mut out = Vec::new();
        while let Some(&b) = self.text.get(self.pos) {
            self.pos += 1;
            match b {
                b if b == quote => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let Some(&e) = self.text.get(self.pos) else { break };
                    self.pos += 1;
                    out.push(match e {
                        b'n' => b'\n',
                        b't' => b'\t',
                        other => other,
                    });
                }
                b'$' if quote == b'`' && self.text.get(self.pos) == Some(&b'{') => return Err("a template with ${} cannot be a pattern".into()),
                b => out.push(b),
            }
        }
        Err("an unterminated string".into())
    }

    fn value(&mut self) -> Result<Lit, String> {
        match self.peek() {
            Some(q @ (b'"' | b'\'' | b'`')) => self.string(q).map(Lit::Str),
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    match self.peek() {
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Lit::Arr(items));
                        }
                        Some(b',') => self.pos += 1,
                        Some(_) => items.push(self.value()?),
                        None => return Err("an unterminated array".into()),
                    }
                }
            }
            Some(b'{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                loop {
                    match self.peek() {
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(Lit::Obj(fields));
                        }
                        Some(b',') => self.pos += 1,
                        Some(q @ (b'"' | b'\'')) => {
                            let key = self.string(q)?;
                            self.colon()?;
                            fields.push((key, self.value()?));
                        }
                        Some(b) if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                            let start = self.pos;
                            while self.pos < self.text.len() && (self.text[self.pos].is_ascii_alphanumeric() || matches!(self.text[self.pos], b'_' | b'$')) {
                                self.pos += 1;
                            }
                            let key = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
                            self.colon()?;
                            fields.push((key, self.value()?));
                        }
                        _ => return Err("an object that is not made of plain keys and literals".into()),
                    }
                }
            }
            Some(b) if b.is_ascii_alphabetic() => {
                let start = self.pos;
                while self.pos < self.text.len() && self.text[self.pos].is_ascii_alphanumeric() {
                    self.pos += 1;
                }
                match &self.text[start..self.pos] {
                    b"true" => Ok(Lit::Bool(true)),
                    b"false" => Ok(Lit::Bool(false)),
                    _ => Err("a value that is not a literal".into()),
                }
            }
            Some(b) if b.is_ascii_digit() || b == b'-' => {
                let start = self.pos;
                self.pos += 1;
                while self.pos < self.text.len() && (self.text[self.pos].is_ascii_digit() || self.text[self.pos] == b'.') {
                    self.pos += 1;
                }
                String::from_utf8_lossy(&self.text[start..self.pos]).parse().map(Lit::Num).map_err(|_| "a bad number".to_string())
            }
            _ => Err("a value that is not a literal".into()),
        }
    }

    fn colon(&mut self) -> Result<(), String> {
        if self.peek() == Some(b':') {
            self.pos += 1;
            Ok(())
        } else {
            Err("an object key without a value".into())
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Positions of `import.meta.glob` outside strings and comments.
fn calls(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = find(&bytes[i + 2..], b"*/").map_or(bytes.len(), |n| i + n + 4),
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'i' if bytes[i..].starts_with(b"import.meta.glob") && !bytes.get(i + 16).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') => {
                found.push(i);
                i += 16;
            }
            _ => i += 1,
        }
    }
    found
}

fn quote(text: &str) -> String {
    crate::json::Json::String(text.to_string()).to_string()
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn relative_from(dir: &Path, file: &Path) -> String {
    let (dir, file) = (normalize(dir), normalize(file));
    let d: Vec<_> = dir.components().collect();
    let f: Vec<_> = file.components().collect();
    let common = d.iter().zip(&f).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); d.len() - common];
    parts.extend(f[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    let joined = parts.join("/");
    if joined.starts_with("..") { joined } else { format!("./{joined}") }
}

/// Replaces every `import.meta.glob(...)` in `source`. `None` when there is none. `dirs` collects the
/// directories that were read, so a file added to one starts another build when watching.
pub fn rewrite(source: &str, module: &Path, root: &Path, fs: &dyn FileSystem, aliases: &[Alias], dirs: &mut Vec<PathBuf>) -> Result<Option<String>, String> {
    let starts = calls(source);
    if starts.is_empty() {
        return Ok(None);
    }
    let module_dir = module.parent().unwrap_or(Path::new("/"));
    let mut out = String::new();
    let mut last = 0;
    let mut hoisted = String::new();
    let mut counter = 0;
    for start in starts {
        let mut reader = Reader { text: source.as_bytes(), pos: start + "import.meta.glob".len() };
        // A type argument (`import.meta.glob<Module>(...)`) is TypeScript's.
        if reader.peek() == Some(b'<') {
            let mut depth = 0;
            while reader.pos < reader.text.len() {
                match reader.text[reader.pos] {
                    b'<' => depth += 1,
                    b'>' => {
                        depth -= 1;
                        if depth == 0 {
                            reader.pos += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                reader.pos += 1;
            }
        }
        if reader.peek() != Some(b'(') {
            continue;
        }
        reader.pos += 1;
        let fail = |what: String| format!("import.meta.glob: {what}");
        let patterns = match reader.value().map_err(&fail)? {
            Lit::Str(s) => vec![s],
            Lit::Arr(items) => items.into_iter().map(|i| if let Lit::Str(s) = i { Ok(s) } else { Err(fail("patterns must be strings".into())) }).collect::<Result<_, _>>()?,
            _ => return Err(fail("the first argument must be a string or an array of strings".into())),
        };
        let mut options = Options::default();
        if reader.peek() == Some(b',') {
            reader.pos += 1;
            if reader.peek() != Some(b')') {
                let Lit::Obj(fields) = reader.value().map_err(&fail)? else { return Err(fail("the options must be an object".into())) };
                for (key, value) in fields {
                    match (key.as_str(), value) {
                        ("eager", Lit::Bool(b)) => options.eager = b,
                        ("import", Lit::Str(s)) => options.import = Some(s),
                        ("base", Lit::Str(s)) => options.base = Some(s),
                        ("query", Lit::Str(s)) => options.query = if s.starts_with('?') { s } else { format!("?{s}") },
                        ("query", Lit::Obj(pairs)) => {
                            let parts: Vec<String> = pairs
                                .into_iter()
                                .map(|(k, v)| match v {
                                    Lit::Str(s) => format!("{k}={s}"),
                                    Lit::Bool(true) => k,
                                    Lit::Bool(false) => format!("{k}=false"),
                                    Lit::Num(n) => format!("{k}={n}"),
                                    _ => k,
                                })
                                .collect();
                            options.query = format!("?{}", parts.join("&"));
                        }
                        // The older spelling: `as: "raw"` is `query: "?raw", import: "default"`.
                        ("as", Lit::Str(s)) => {
                            options.query = format!("?{s}");
                            options.import = Some("default".to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
        if reader.peek() == Some(b',') {
            reader.pos += 1;
        }
        if reader.peek() != Some(b')') {
            return Err(fail("expected the end of the call".into()));
        }
        reader.pos += 1;
        // Where the patterns start from, and what the keys are relative to.
        let base_dir = match &options.base {
            Some(b) if b.starts_with('/') => normalize(&root.join(b.trim_start_matches('/'))),
            Some(b) => normalize(&module_dir.join(b)),
            None => module_dir.to_path_buf(),
        };
        // A pattern may begin with an alias (`@/views/**/*.vue`), as in Vite: it stands for the path it is an alias of.
        let patterns: Vec<String> = patterns.iter().map(|p| through_aliases(p, aliases, root, module_dir)).collect();
        let mut files = expand(&patterns, &base_dir, root, fs, dirs).map_err(&fail)?;
        files.retain(|f| f != module);
        files.sort();
        // The object.
        let mut object = String::from("{");
        for (n, file) in files.iter().enumerate() {
            let key = if patterns.iter().any(|p| p.trim_start_matches('!').starts_with('/')) && file.starts_with(root) && options.base.is_none() {
                format!("/{}", file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/"))
            } else {
                relative_from(&base_dir, file)
            };
            let specifier = format!("{}{}", relative_from(module_dir, file), options.query);
            let import = options.import.as_deref();
            if n > 0 {
                object.push_str(", ");
            }
            object.push_str(&quote(&key));
            object.push_str(": ");
            if options.eager {
                let name = format!("__rk_glob_{counter}_{n}");
                match import {
                    None | Some("*") => hoisted.push_str(&format!("import * as {name} from {};\n", quote(&specifier))),
                    Some(i) => hoisted.push_str(&format!("import {{ {} as {name} }} from {};\n", if i == "default" { "default".to_string() } else { i.to_string() }, quote(&specifier))),
                }
                object.push_str(&name);
            } else {
                match import {
                    None | Some("*") => object.push_str(&format!("() => import({})", quote(&specifier))),
                    Some(i) => object.push_str(&format!("() => import({}).then((m) => m[{}])", quote(&specifier), quote(i))),
                }
            }
        }
        object.push('}');
        counter += 1;
        out.push_str(&source[last..start]);
        out.push_str(&object);
        last = reader.pos;
    }
    out.push_str(&source[last..]);
    // Imports may stand anywhere at the top level; at the end they move no line or column of the source.
    if !hoisted.is_empty() {
        out.push('\n');
        out.push_str(&hoisted);
    }
    Ok(Some(out))
}

// ---- `import(\`./dir/${name}.js\`)` ---------------------------------------------------------------------------------------------------

/// What a scan of JavaScript text is inside of: the text of a template literal, or the code of one of its `${ }` (with the `{` it holds).
enum Frame {
    Text,
    Code(u32),
}

/// Skips a comment or a string at `i` (a quote or a slash) and returns where it ends; `None` when `i` is neither.
fn skip_literal(bytes: &[u8], i: usize) -> Option<usize> {
    match bytes[i] {
        b'/' if bytes.get(i + 1) == Some(&b'/') => Some(bytes[i..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |n| i + n)),
        b'/' if bytes.get(i + 1) == Some(&b'*') => Some(find(&bytes[i + 2..], b"*/").map_or(bytes.len(), |n| i + n + 4)),
        q @ (b'"' | b'\'') => {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != q && bytes[j] != b'\n' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            Some(j + 1)
        }
        _ => None,
    }
}

/// Steps one byte of a scan: the frames of template literals are kept, and strings and comments are skipped. Returns the next position
/// and whether the byte at `i` is code (not text of a template, not inside a comment or string).
fn step(bytes: &[u8], i: usize, frames: &mut Vec<Frame>) -> (usize, bool) {
    match frames.last_mut() {
        Some(Frame::Text) => match bytes[i] {
            b'\\' => (i + 2, false),
            b'`' => {
                frames.pop();
                (i + 1, false)
            }
            b'$' if bytes.get(i + 1) == Some(&b'{') => {
                frames.push(Frame::Code(0));
                (i + 2, false)
            }
            _ => (i + 1, false),
        },
        top => {
            if let Some(next) = skip_literal(bytes, i) {
                return (next, false);
            }
            match bytes[i] {
                b'`' => {
                    frames.push(Frame::Text);
                    (i + 1, false)
                }
                b'{' => {
                    if let Some(Frame::Code(depth)) = top {
                        *depth += 1;
                    }
                    (i + 1, true)
                }
                b'}' => {
                    match top {
                        Some(Frame::Code(0)) => {
                            // The end of a `${ }`: the template text goes on.
                            frames.pop();
                            return (i + 1, false);
                        }
                        Some(Frame::Code(depth)) => *depth -= 1,
                        _ => {}
                    }
                    (i + 1, true)
                }
                _ => (i + 1, true),
            }
        }
    }
}

/// Where `import(` is a call in `source`: code, not a string, comment or template text, and not `x.import(` or `myimport(`. The place is the
/// position of the word `import`.
fn import_calls(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let (next, code) = step(bytes, i, &mut frames);
        if code
            && bytes[i..].starts_with(b"import")
            && !(i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'_' | b'$' | b'.')))
            && !bytes.get(i + 6).is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
            && bytes[i + 6..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'(')
        {
            found.push(i);
            i += 6;
            continue;
        }
        i = next;
    }
    found
}

/// The `)` that closes a call whose arguments start at `i`, when there is one argument; `None` for none or several.
fn argument_end(source: &str, mut i: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut frames: Vec<Frame> = Vec::new();
    let mut depth = 0u32;
    while i < bytes.len() {
        let (next, code) = step(bytes, i, &mut frames);
        // Only code at the top level, not inside a template's `${ }`, counts its brackets.
        if code && frames.is_empty() {
            match bytes[i] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        return (bytes[i] == b')').then_some(i);
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => return None,
                _ => {}
            }
        }
        i = next;
    }
    None
}

/// One piece of the argument of `import(...)`: text that is there as written, or an expression that is not.
#[derive(Debug, PartialEq, Clone)]
enum Part {
    Text(String),
    Value,
}

/// The pieces of `\`./a/${x}.js\``, or of `'./a/' + x + '.js'`; `None` for any other expression (and when there is nothing to vary).
fn argument_parts(arg: &str) -> Option<Vec<Part>> {
    let arg = arg.trim();
    let bytes = arg.as_bytes();
    if bytes.first() == Some(&b'`') {
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut i = 1;
        let mut closed = false;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => {
                    // The escapes of a path are rare; one that is here makes the text uncertain.
                    return None;
                }
                b'`' => {
                    closed = i == bytes.len() - 1;
                    break;
                }
                b'$' if bytes.get(i + 1) == Some(&b'{') => {
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    // The expression ends at the `}` that closes it; strings and other templates inside are followed.
                    let mut depth = 0;
                    let mut j = i + 2;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'{' => depth += 1,
                            b'}' if depth == 0 => break,
                            b'}' => depth -= 1,
                            b'`' => return None,
                            q @ (b'"' | b'\'') => {
                                j += 1;
                                while j < bytes.len() && bytes[j] != q {
                                    if bytes[j] == b'\\' {
                                        j += 1;
                                    }
                                    j += 1;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    if j >= bytes.len() {
                        return None;
                    }
                    parts.push(Part::Value);
                    i = j + 1;
                    continue;
                }
                _ => {
                    let ch = arg[i..].chars().next()?;
                    text.push(ch);
                    i += ch.len_utf8();
                    continue;
                }
            }
        }
        if !closed {
            return None;
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        return parts.iter().any(|p| *p == Part::Value).then_some(parts);
    }
    // `'./a/' + x + '.js'`: split at the `+` that are outside brackets and strings.
    let mut pieces: Vec<&str> = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'`' => return None,
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'+' if depth == 0 => {
                pieces.push(&arg[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    pieces.push(&arg[start..]);
    if pieces.len() < 2 {
        return None;
    }
    let mut parts = Vec::new();
    for piece in pieces {
        let piece = piece.trim();
        let b = piece.as_bytes();
        let quoted = b.len() >= 2 && matches!(b[0], b'"' | b'\'') && b[b.len() - 1] == b[0] && !piece[1..piece.len() - 1].contains(['\\', b[0] as char]);
        parts.push(if quoted { Part::Text(piece[1..piece.len() - 1].to_string()) } else { Part::Value });
    }
    parts.iter().any(|p| *p == Part::Value).then_some(parts)
}

/// Why a pattern from the pieces cannot be turned into files; the one-segment glob when it can.
fn dynamic_pattern(parts: &[Part]) -> Result<String, String> {
    let first = match parts.first() {
        Some(Part::Text(t)) => t,
        _ => return Err("the path must start with ./ or ../ as text".into()),
    };
    if !(first.starts_with("./") || first.starts_with("../")) {
        // Not a relative path: a package name or an address, which this does not try to look for.
        return Err(String::new());
    }
    let mut pattern = String::new();
    for part in parts {
        match part {
            Part::Text(t) => {
                if t.contains(['*', '?', '[', ']', '{', '}', '!']) {
                    return Err("the text of the path has a character a pattern uses (* ? [ ] { } !)".into());
                }
                pattern.push_str(t);
            }
            // One name in a path: what a variable stands for does not cross a `/`.
            Part::Value => {
                if pattern.ends_with('*') {
                    return Err("two variables next to each other".into());
                }
                pattern.push('*');
            }
        }
    }
    let tail = pattern.rsplit('*').next().unwrap_or("");
    if !tail.contains('.') || tail.contains('/') || tail.ends_with('.') {
        return Err("the file extension must be written out in the path (`.js` after the last variable) so that the files can be told".into());
    }
    if pattern.starts_with("*") {
        return Err("the path must start with ./ or ../".into());
    }
    Ok(pattern)
}

/// Each `import(\`./dir/${name}.js\`)` whose path can be known (it starts with ./ or ../, a variable stands for one name, and the extension is
/// written out) becomes a call of a function that has an `import()` of each file the pattern reaches, and chooses by the path:
/// the same rules as `@rollup/plugin-dynamic-import-vars`. What does not fit is left as it is; `warnings` says why, for a relative path.
pub fn rewrite_dynamic_imports(source: &str, module: &Path, root: &Path, fs: &dyn FileSystem, dirs: &mut Vec<PathBuf>, warnings: &mut Vec<String>) -> Option<String> {
    let calls = import_calls(source);
    if calls.is_empty() {
        return None;
    }
    let module_dir = module.parent().unwrap_or(Path::new("/"));
    let mut edits: Vec<(usize, String)> = Vec::new();
    let mut helpers = String::new();
    let mut counter = 0;
    for start in calls {
        let open = start + source[start..].find('(')?;
        // One argument: it ends at the `)` that closes the call.
        let Some(end) = argument_end(source, open + 1) else { continue };
        let argument = &source[open + 1..end];
        let Some(parts) = argument_parts(argument) else { continue };
        let pattern = match dynamic_pattern(&parts) {
            Ok(p) => p,
            Err(why) => {
                if !why.is_empty() {
                    warnings.push(format!("import({}) is left as it is: {why}", argument.trim()));
                }
                continue;
            }
        };
        let files = match expand(std::slice::from_ref(&pattern), module_dir, root, fs, dirs) {
            Ok(files) => files.into_iter().filter(|f| f != module).collect::<Vec<_>>(),
            Err(why) => {
                warnings.push(format!("import({}) is left as it is: {why}", argument.trim()));
                continue;
            }
        };
        if files.is_empty() {
            warnings.push(format!("import({}) is left as it is: no file matches {pattern}", argument.trim()));
            continue;
        }
        if files.len() > 2000 {
            warnings.push(format!("import({}) is left as it is: {} files match {pattern}", argument.trim(), files.len()));
            continue;
        }
        let name = format!("__rkDynamicImport{counter}");
        counter += 1;
        let table: Vec<String> = files.iter().map(|f| {
            let key = relative_from(module_dir, f);
            format!("{}: () => import({})", quote(&key), quote(&key))
        }).collect();
        helpers.push_str(&format!(
            "\nfunction {name}(path) {{ var table = {{ {} }}; var load = table[path]; return load ? load() : Promise.reject(new Error(\"Unknown variable dynamic import: \" + path)); }}",
            table.join(", ")
        ));
        edits.push((start, name));
    }
    if edits.is_empty() {
        return None;
    }
    // `import` becomes the helper's name, last edit first so the places stay true.
    let mut out = source.to_string();
    for (start, name) in edits.into_iter().rev() {
        out.replace_range(start..start + "import".len(), &name);
    }
    out.push_str(&helpers);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, path: &str) -> bool {
        let names: Vec<&str> = path.split('/').collect();
        Glob::compile(pattern).iter().any(|g| g.is_match(&names))
    }

    #[test]
    fn patterns() {
        assert!(m("*.js", "a.js"));
        assert!(!m("*.js", "a/b.js"));
        assert!(m("**/*.js", "a/b/c.js"));
        assert!(m("**/*.js", "c.js"));
        assert!(m("a/**/c.js", "a/c.js"));
        assert!(m("a/**/c.js", "a/b/b/c.js"));
        assert!(m("*.{js,ts}", "a.ts"));
        assert!(!m("*.{js,ts}", "a.css"));
        assert!(m("[a-c]?.js", "bz.js"));
        assert!(!m("[!a-c]?.js", "bz.js"));
        assert!(!m("*.js", ".hidden.js"));
        assert!(m(".*.js", ".hidden.js"));
        assert!(!m("**/*.js", "node_modules/x/a.js"));
        assert!(m("node_modules/*/a.js", "node_modules/x/a.js"));
    }

    #[test]
    fn braces() {
        assert_eq!(expand_braces("a{b,c}d"), vec!["abd", "acd"]);
        assert_eq!(expand_braces("a{b,{c,d}}"), vec!["ab", "ac", "ad"]);
        assert_eq!(expand_braces("plain"), vec!["plain"]);
    }

    #[test]
    fn finds_calls_outside_strings() {
        let source = "// import.meta.glob('x')\nconst a = 'import.meta.glob(1)'; import.meta.glob('./a.js');";
        assert_eq!(calls(source).len(), 1);
    }

    #[test]
    fn import_calls_are_found_in_code_only() {
        let src = "import('./a.js'); x.import('./b.js'); myimport('c'); // import('d')\nconst s = \"import('e')\"; const t = `text import('f') ${ import('g') } more`; /* import('h') */ import ( './i.js' );";
        let found: Vec<&str> = import_calls(src).into_iter().map(|i| src[i..].split(')').next().unwrap()).collect();
        assert_eq!(found, ["import('./a.js'", "import('g'", "import ( './i.js' "]);
    }

    #[test]
    fn the_argument_ends_at_its_closing_paren() {
        for (call, argument) in [("import(`./a/${x}.js`)", "`./a/${x}.js`"), ("import('./a' + f(1, 2) + '.js')", "'./a' + f(1, 2) + '.js'"), ("import(`./a/${ `${y}` }.js`)", "`./a/${ `${y}` }.js`"), ("import(a ? 'x' : \"y)\")", "a ? 'x' : \"y)\"")] {
            let end = argument_end(call, 7).unwrap();
            assert_eq!(&call[7..end], argument, "{call}");
        }
        assert_eq!(argument_end("import('a', { with: { type: 'json' } })", 7), None, "a second argument");
    }

    #[test]
    fn what_a_variable_import_can_be() {
        let pattern = |arg: &str| argument_parts(arg).ok_or_else(|| "no parts".to_string()).and_then(|p| dynamic_pattern(&p));
        assert_eq!(pattern("`./mods/${n}.js`"), Ok("./mods/*.js".to_string()));
        assert_eq!(pattern("'./mods/' + n + '.js'"), Ok("./mods/*.js".to_string()));
        assert_eq!(pattern("`../x/${a}/${b}.json`"), Ok("../x/*/*.json".to_string()));
        assert_eq!(pattern("`./mods/${n}`").unwrap_err().contains("extension"), true);
        assert_eq!(pattern("`./mods/${a}${b}.js`").unwrap_err().contains("next to each other"), true);
        assert_eq!(pattern("`./mods/*/${n}.js`").unwrap_err().contains("character a pattern uses"), true);
        // Not paths of this kind: left alone without a word.
        assert_eq!(dynamic_pattern(&argument_parts("`pkg/${n}.js`").unwrap()), Err(String::new()));
        assert_eq!(argument_parts("n"), None);
        assert_eq!(argument_parts("`./static.js`"), None);
        assert_eq!(argument_parts("'./a' + '.js'"), None);
    }

    #[test]
    fn reads_literals() {
        let mut r = Reader { text: b"{ eager: true, 'import': \"default\", query: { a: 1, b: 'x' }, /* c */ list: ['a', 'b',], }", pos: 0 };
        let Ok(Lit::Obj(fields)) = r.value() else { panic!("not an object") };
        assert_eq!(fields.len(), 4);
    }
}
