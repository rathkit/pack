//! Source map (version 3) output: the mapping from positions in a generated file back to the
//! files it was built from, in the base64 VLQ form debuggers and stack-trace tools read.
//!
//! Columns are counted in UTF-16 code units, which is what JavaScript engines report.

use std::fmt::Write;

/// Where a generated position came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub source: u32,
    pub line: u32,
    pub column: u32,
    pub name: Option<u32>,
}

/// Where the mappings written so far leave off: each segment is written relative to the one
/// before, so this is what the next one needs to know.
#[derive(Clone, Copy, Default)]
pub struct Encoder {
    line: u32,
    last_column: u32,
    last_source: i64,
    last_origin_line: i64,
    last_origin_column: i64,
    last_name: i64,
    /// A segment has been written on the current line.
    started: bool,
    /// The previous segment on record says where the text came from (not `unmapped`).
    mapped: bool,
}

impl Encoder {
    /// Records that the text at (`line`, `column`) of the generated file starts at `origin`,
    /// or is generated code with no origin when that is `None`, writing the segment to `out`
    /// when `WRITE` (otherwise only the state moves on). Calls come in generated order.
    pub fn add<const WRITE: bool>(&mut self, out: &mut String, line: u32, column: u32, origin: Option<Origin>) {
        while self.line < line {
            if WRITE {
                out.push(';');
            }
            self.line += 1;
            self.last_column = 0;
            self.started = false;
        }
        // Text with no origin only needs saying after text that has one.
        if origin.is_none() && !self.mapped {
            return;
        }
        if WRITE && self.started {
            out.push(',');
        }
        self.started = true;
        if WRITE {
            vlq(out, column as i64 - self.last_column as i64);
        }
        self.last_column = column;
        match origin {
            None => self.mapped = false,
            Some(o) => {
                self.mapped = true;
                if WRITE {
                    vlq(out, o.source as i64 - self.last_source);
                    vlq(out, o.line as i64 - self.last_origin_line);
                    vlq(out, o.column as i64 - self.last_origin_column);
                }
                self.last_source = o.source as i64;
                self.last_origin_line = o.line as i64;
                self.last_origin_column = o.column as i64;
                if let Some(n) = o.name {
                    if WRITE {
                        vlq(out, n as i64 - self.last_name);
                    }
                    self.last_name = n as i64;
                }
            }
        }
    }
}

/// Collects the mappings of one generated file, in generated order.
#[derive(Default)]
pub struct Builder<'a> {
    pub sources: Vec<String>,
    pub contents: Vec<Option<std::borrow::Cow<'a, str>>>,
    /// Texts of sources that were already escaped for JSON (with their quotes), in place of `contents`.
    ready: Vec<Option<String>>,
    pub names: Vec<String>,
    name_index: std::collections::HashMap<String, u32>,
    mappings: String,
    encoder: Encoder,
}

impl<'a> Builder<'a> {
    pub fn add_source(&mut self, path: String, content: Option<std::borrow::Cow<'a, str>>) -> u32 {
        self.sources.push(path);
        self.contents.push(content);
        self.ready.push(None);
        (self.sources.len() - 1) as u32
    }

    /// A source whose text was escaped ahead of time with [`escape_texts`]: it is written as it is.
    pub fn add_source_escaped(&mut self, path: String, escaped: String) -> u32 {
        self.sources.push(path);
        self.contents.push(None);
        self.ready.push(Some(escaped));
        (self.sources.len() - 1) as u32
    }

    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.name_index.get(name) {
            return i;
        }
        self.names.push(name.to_string());
        let i = (self.names.len() - 1) as u32;
        self.name_index.insert(name.to_string(), i);
        i
    }

    /// Records one segment; see [`Encoder::add`].
    pub fn add(&mut self, line: u32, column: u32, origin: Option<Origin>) {
        self.encoder.add::<true>(&mut self.mappings, line, column, origin);
    }

    /// Takes segments written elsewhere by an encoder that started where this one left off and
    /// ended in `end`.
    pub fn append(&mut self, text: &str, end: Encoder) {
        self.mappings.push_str(text);
        self.encoder = end;
    }

    /// Where the segments so far leave off.
    pub fn state(&self) -> Encoder {
        self.encoder
    }

    /// The finished map as JSON. `file` is the generated file's name.
    pub fn finish(&self, file: &str) -> String {
        let size = self.contents.iter().flatten().map(|c| c.len() + 16).sum::<usize>() + self.ready.iter().flatten().map(|c| c.len() + 1).sum::<usize>() + self.mappings.len() + 4096;
        let mut out = String::with_capacity(size);
        out.push_str("{\"version\":3,\"file\":");
        out.push_str(&json_string(file));
        out.push_str(",\"sources\":[");
        for (i, s) in self.sources.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_string(s));
        }
        if self.contents.iter().any(|c| c.is_some()) || self.ready.iter().any(|r| r.is_some()) {
            out.push_str("],\"sourcesContent\":[");
            // Escaping the texts is most of the work for a big bundle: share it out (those escaped ahead of time are done).
            let texts: Vec<Option<&str>> = self.contents.iter().zip(&self.ready).map(|(c, r)| if r.is_some() { None } else { c.as_deref() }).collect();
            let escaped = escape_all(&texts);
            for (i, c) in escaped.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                match (&self.ready[i], c) {
                    (Some(text), _) | (None, Some(text)) => out.push_str(text),
                    (None, None) => out.push_str("null"),
                }
            }
        }
        out.push_str("],\"names\":[");
        for (i, n) in self.names.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_string(n));
        }
        out.push_str("],");
        // Frames in dependencies are noise in a debugger's stack, so say which sources they are.
        let ignored: Vec<String> = self.sources.iter().enumerate().filter(|(_, s)| s.contains("node_modules/")).map(|(i, _)| i.to_string()).collect();
        if !ignored.is_empty() {
            out.push_str("\"x_google_ignoreList\":[");
            out.push_str(&ignored.join(","));
            out.push_str("],");
        }
        out.push_str("\"mappings\":");
        // Base64 digits, `,` and `;`: nothing to escape.
        out.push('"');
        out.push_str(&self.mappings);
        out.push('"');
        out.push('}');
        out
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Appends `value` as a base64 variable-length quantity: five bits per digit, the lowest bit
/// of the first digit the sign, and the sixth bit of each digit saying more follow.
pub fn vlq(out: &mut String, value: i64) {
    let mut rest = if value < 0 { ((-value) as u64) << 1 | 1 } else { (value as u64) << 1 };
    loop {
        let mut digit = (rest & 31) as usize;
        rest >>= 5;
        if rest > 0 {
            digit |= 32;
        }
        out.push(BASE64[digit] as char);
        if rest == 0 {
            break;
        }
    }
}

/// Reads one quantity written by `vlq`, for tests.
pub fn read_vlq(text: &[u8], at: &mut usize) -> Option<i64> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let c = *text.get(*at)?;
        *at += 1;
        let digit = BASE64.iter().position(|&b| b == c)? as u64;
        result |= (digit & 31) << shift;
        shift += 5;
        if digit & 32 == 0 {
            break;
        }
    }
    let magnitude = (result >> 1) as i64;
    Some(if result & 1 == 1 { -magnitude } else { magnitude })
}

/// `json_string` of every text, on all cores when there is much to do; for starting early, beside other work.
pub fn escape_texts(texts: &[&str]) -> Vec<String> {
    let some: Vec<Option<&str>> = texts.iter().map(|t| Some(*t)).collect();
    escape_all(&some).into_iter().map(|t| t.expect("escaped")).collect()
}

/// `json_string` of every text that is there, on all cores when there is much to do.
fn escape_all(texts: &[Option<&str>]) -> Vec<Option<String>> {
    let total: usize = texts.iter().flatten().map(|t| t.len()).sum();
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    if total < (1 << 20) || workers < 2 || texts.len() < 2 {
        return texts.iter().map(|t| t.map(json_string)).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done: std::sync::Mutex<Vec<Option<String>>> = std::sync::Mutex::new(vec![None; texts.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers.min(texts.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(text) = texts.get(i) else { break };
                if let Some(text) = text {
                    let escaped = json_string(text);
                    done.lock().expect("escaped")[i] = Some(escaped);
                }
            });
        }
    });
    done.into_inner().expect("escaped")
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    let bytes = text.as_bytes();
    let mut run = 0;
    for (i, &b) in bytes.iter().enumerate() {
        // Only quotes, backslashes, control characters and U+2028/9 (E2 80 A8/A9) need escaping.
        let escape: Option<&str> = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0..=0x1f => None,
            0xE2 if bytes.get(i + 1) == Some(&0x80) && matches!(bytes.get(i + 2), Some(0xA8 | 0xA9)) => None,
            _ => continue,
        };
        out.push_str(&text[run..i]);
        match escape {
            Some(e) => {
                out.push_str(e);
                run = i + 1;
            }
            None if b == 0xE2 => {
                out.push_str(if bytes[i + 2] == 0xA8 { "\\u2028" } else { "\\u2029" });
                run = i + 3;
            }
            None => {
                let _ = write!(out, "\\u{:04x}", b);
                run = i + 1;
            }
        }
    }
    out.push_str(&text[run..]);
    out.push('"');
    out
}

/// A source map that came with a file, read so that positions in that file can be traced
/// further back to what it was made from.
pub struct Input {
    lines: Vec<Vec<InputSegment>>,
    /// Absolute-ish path and text (when the map or the disk has it) of each original file.
    pub sources: Vec<(String, Option<String>)>,
    pub names: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct InputSegment {
    column: u32,
    /// `u32::MAX`: the segment says the text has no origin.
    pub source: u32,
    pub line: u32,
    pub origin_column: u32,
    pub name: Option<u32>,
}

impl Input {
    /// The map `text` (the source of the file at `path`) points to with a trailing
    /// `//# sourceMappingURL=` comment, if there is one and it can be read.
    pub fn find(path: &std::path::Path, text: &str) -> Option<Input> {
        let at = text.rfind("sourceMappingURL=")?;
        let before = text[..at].trim_end_matches([' ', '\t']);
        if !(before.ends_with("//#") || before.ends_with("//@")) {
            return None;
        }
        let url = text[at + "sourceMappingURL=".len()..].split_whitespace().next()?;
        let dir = path.parent()?;
        let (json, map_dir) = if let Some(data) = url.strip_prefix("data:") {
            let (head, body) = data.split_once(',')?;
            let bytes = if head.ends_with(";base64") { base64_decode(body)? } else { percent_decode(body) };
            (String::from_utf8(bytes).ok()?, dir.to_path_buf())
        } else {
            let file = dir.join(percent_decode_str(url));
            (std::fs::read_to_string(&file).ok()?, file.parent()?.to_path_buf())
        };
        let map = crate::json::Json::parse(&json).ok()?;
        let root = map.get("sourceRoot").and_then(|r| r.as_str()).unwrap_or("");
        let contents = map.get("sourcesContent").and_then(|c| c.as_array());
        let mut sources = Vec::new();
        for (i, s) in map.get("sources")?.as_array()?.iter().enumerate() {
            let name = s.as_str().unwrap_or("");
            let file = map_dir.join(root).join(name);
            let content = contents.and_then(|c| c.get(i)).and_then(|c| c.as_str()).map(str::to_string).or_else(|| std::fs::read_to_string(&file).ok());
            sources.push((normalize(&file), content));
        }
        let names = map.get("names").and_then(|n| n.as_array()).map(|n| n.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        let mappings = map.get("mappings")?.as_str()?;
        let (mut source, mut line, mut column, mut name) = (0i64, 0i64, 0i64, 0i64);
        let mut lines = Vec::new();
        for group in mappings.split(';') {
            let mut segments = Vec::new();
            let mut generated = 0i64;
            for segment in group.split(',').filter(|g| !g.is_empty()) {
                let bytes = segment.as_bytes();
                let mut at = 0;
                let mut fields = [0i64; 5];
                let mut count = 0;
                while at < bytes.len() && count < 5 {
                    fields[count] = read_vlq(bytes, &mut at)?;
                    count += 1;
                }
                generated += fields[0];
                if count == 1 {
                    segments.push(InputSegment { column: generated as u32, source: u32::MAX, line: 0, origin_column: 0, name: None });
                    continue;
                }
                source += fields[1];
                line += fields[2];
                column += fields[3];
                let named = if count == 5 {
                    name += fields[4];
                    Some(name as u32)
                } else {
                    None
                };
                segments.push(InputSegment { column: generated as u32, source: source as u32, line: line as u32, origin_column: column as u32, name: named });
            }
            lines.push(segments);
        }
        Some(Input { lines, sources, names })
    }

    /// The segment that starts at or before (`line`, `column`) on its line.
    pub fn lookup(&self, line: u32, column: u32) -> Option<InputSegment> {
        let segments = self.lines.get(line as usize)?;
        let i = segments.partition_point(|s| s.column <= column);
        segments.get(i.checked_sub(1)?).copied()
    }
}

fn normalize(path: &std::path::Path) -> String {
    let mut parts: Vec<std::path::Component> = Vec::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir if matches!(parts.last(), Some(std::path::Component::Normal(_))) => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            c => parts.push(c),
        }
    }
    parts.iter().collect::<std::path::PathBuf>().display().to_string()
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for b in text.bytes() {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        let d = BASE64.iter().position(|&x| x == b)? as u32;
        acc = acc << 6 | d;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn percent_decode_str(text: &str) -> String {
    String::from_utf8_lossy(&percent_decode(text)).into_owned()
}

/// `path` as seen from the directory `base`, with `/` separators: `../src/a.ts`.
pub fn relative(base: &std::path::Path, path: &std::path::Path) -> String {
    let base: Vec<_> = base.components().collect();
    let path_parts: Vec<_> = path.components().collect();
    let common = base.iter().zip(&path_parts).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base.len() {
        parts.push("..".to_string());
    }
    for c in &path_parts[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// Line and UTF-16 column of byte offsets in one text. Asking in increasing order is cheap.
#[derive(Clone)]
pub struct Positions<'a> {
    text: &'a str,
    ascii: bool,
    line_starts: Vec<u32>,
    /// The last answer, so the next question can continue from it.
    cache: (u32, usize, u32),
    /// The line the last offset was on.
    hint: usize,
}

/// Where the lines that begin inside `from..to` of `bytes` begin.
fn breaks(bytes: &[u8], from: usize, to: usize) -> Vec<u32> {
    let mut starts = Vec::new();
    let mut i = from;
    while i < to {
        match bytes[i] {
            b'\n' => starts.push(i as u32 + 1),
            b'\r' => {
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                starts.push(i as u32 + 1);
            }
            // U+2028 and U+2029 are three bytes, E2 80 A8 and E2 80 A9.
            0xE2 if bytes.get(i + 1) == Some(&0x80) && matches!(bytes.get(i + 2), Some(0xA8 | 0xA9)) => {
                i += 2;
                starts.push(i as u32 + 1);
            }
            _ => {}
        }
        i += 1;
    }
    starts
}

impl<'a> Positions<'a> {
    pub fn new(text: &'a str) -> Self {
        let bytes = text.as_bytes();
        let mut line_starts = vec![0u32];
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        if bytes.len() < (4 << 20) || workers < 2 {
            line_starts.extend(breaks(bytes, 0, bytes.len()));
        } else {
            // Parts of the text are scanned side by side. A cut never falls inside a character or
            // between the `\r` and `\n` of one line break.
            let mut cuts = vec![0usize];
            for k in 1..workers {
                let mut at = bytes.len() / workers * k;
                while at < bytes.len() && (!text.is_char_boundary(at) || bytes[at - 1] == b'\r') {
                    at += 1;
                }
                if at > *cuts.last().expect("cut") {
                    cuts.push(at);
                }
            }
            cuts.push(bytes.len());
            let parts: Vec<Vec<u32>> = std::thread::scope(|scope| {
                let handles: Vec<_> = cuts.windows(2).map(|w| scope.spawn(move || breaks(bytes, w[0], w[1]))).collect();
                handles.into_iter().map(|h| h.join().expect("scan")).collect()
            });
            for part in parts {
                line_starts.extend(part);
            }
        }
        Positions { text, ascii: text.is_ascii(), line_starts, cache: (0, 0, 0), hint: 0 }
    }

    /// (line, column) of `offset`, which must sit on a character boundary.
    pub fn at(&mut self, offset: u32) -> (u32, u32) {
        let starts = &self.line_starts;
        let at = |line: usize| starts[line];
        let ends_after = |line: usize, offset: u32| line + 1 >= starts.len() || offset < starts[line + 1];
        // Offsets mostly come in increasing order: try the last line and the next one first.
        let line = if at(self.hint) <= offset && ends_after(self.hint, offset) {
            self.hint
        } else if self.hint + 1 < starts.len() && at(self.hint + 1) <= offset && ends_after(self.hint + 1, offset) {
            self.hint + 1
        } else {
            match starts.binary_search(&offset) {
                Ok(i) => i,
                Err(i) => i - 1,
            }
        };
        self.hint = line;
        let start = self.line_starts[line];
        if self.ascii {
            return (line as u32, offset - start);
        }
        let (cached_offset, cached_line, cached_column) = self.cache;
        let (from, mut column) = if cached_line == line && cached_offset <= offset && cached_offset >= start { (cached_offset, cached_column) } else { (start, 0) };
        for c in self.text[from as usize..offset as usize].chars() {
            column += c.len_utf16() as u32;
        }
        self.cache = (offset, line, column);
        (line as u32, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_round_trips_and_matches_the_spec() {
        let mut s = String::new();
        for v in [0, 1, -1, 15, 16, -16, 123, -12345, 1 << 20] {
            vlq(&mut s, v);
        }
        let mut at = 0;
        for v in [0, 1, -1, 15, 16, -16, 123, -12345, 1 << 20] {
            assert_eq!(read_vlq(s.as_bytes(), &mut at), Some(v));
        }
        // Known encodings from the specification's examples.
        let mut s = String::new();
        vlq(&mut s, 0);
        vlq(&mut s, 1);
        vlq(&mut s, -1);
        vlq(&mut s, 16);
        assert_eq!(s, "ACDgB");
    }

    #[test]
    fn columns_count_utf16_units() {
        let text = "ab\n\u{1F600}x\u{e9}y\r\nz";
        let mut p = Positions::new(text);
        assert_eq!(p.at(0), (0, 0));
        assert_eq!(p.at(3), (1, 0));
        // After the emoji (4 bytes, 2 units).
        assert_eq!(p.at(7), (1, 2));
        // After `x`, then after `é` (2 bytes, 1 unit).
        assert_eq!(p.at(8), (1, 3));
        assert_eq!(p.at(10), (1, 4));
        let z = text.find('z').unwrap() as u32;
        assert_eq!(p.at(z), (2, 0));
        // Going back works too.
        assert_eq!(p.at(7), (1, 2));
    }

    #[test]
    fn unmapped_text_is_only_written_after_mapped_text() {
        let mut b = Builder::default();
        b.add_source("a.js".to_string(), None);
        b.add(0, 0, None);
        b.add(0, 2, Some(Origin { source: 0, line: 0, column: 0, name: None }));
        b.add(0, 5, None);
        b.add(0, 9, None);
        b.add(1, 0, None);
        let json = b.finish("out.js");
        assert!(json.contains("\"mappings\":\"EAAA,G;"), "{json}");
        assert!(json.contains("\"names\":[]"));
    }

    #[test]
    fn sources_escaped_ahead_of_time_read_the_same_as_those_escaped_at_the_end() {
        let texts = ["a \"quoted\"\nline\t\\", "plain", "\u{2028}"];
        let escaped = escape_texts(&texts);
        let mut late = Builder::default();
        let mut early = Builder::default();
        for (i, text) in texts.iter().enumerate() {
            late.add_source(format!("s{i}.js"), Some(std::borrow::Cow::Borrowed(*text)));
            // The middle one is mixed in the other way: escaped late.
            if i == 1 {
                early.add_source(format!("s{i}.js"), Some(std::borrow::Cow::Borrowed(*text)));
            } else {
                early.add_source_escaped(format!("s{i}.js"), escaped[i].clone());
            }
        }
        assert_eq!(late.finish("o.js"), early.finish("o.js"));
        assert!(late.finish("o.js").contains("\\u2028"));
    }
}
