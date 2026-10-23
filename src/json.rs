//! A small JSON reader, enough for `package.json` and `tsconfig.json`-style files.
//!
//! Strict RFC 8259 plus one leniency: a leading byte-order mark is skipped, as Node does.
//! Objects keep their keys in file order because `package.json` `exports` conditions are
//! matched in that order.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl fmt::Display for Json {
    /// Compact JSON.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Json::Number(n) if n.is_finite() => write!(f, "{n}"),
            Json::Number(_) => f.write_str("null"),
            Json::String(s) => write_string(f, s),
            Json::Array(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Object(entries) => {
                f.write_str("{")?;
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write_string(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    for ch in text.chars() {
        match ch {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            '\u{2028}' => f.write_str("\\u2028")?,
            '\u{2029}' => f.write_str("\\u2029")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub message: &'static str,
    pub pos: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.pos)
    }
}

impl Json {
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut p = Reader { src: text.as_bytes(), pos: 0, depth: 0 };
        p.skip_ws();
        let value = p.value()?;
        p.skip_ws();
        if p.pos != p.src.len() {
            return Err(p.err("Unexpected data after the JSON value"));
        }
        Ok(value)
    }

    /// Parses JSON with comments and trailing commas, which is what `tsconfig.json` is.
    pub fn parse_jsonc(text: &str) -> Result<Json, JsonError> {
        Json::parse(&strip_jsonc(text))
    }

    /// The value for `key` if this is an object. With duplicate keys the last one wins, as
    /// `JSON.parse` does.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(entries) => Some(entries),
            _ => None,
        }
    }
}

/// Replaces comments with spaces and drops trailing commas, leaving strings alone. Offsets in
/// error messages stay right because every byte keeps its position.
fn strip_jsonc(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let (mut i, mut in_string) = (0, false);
    let mut last_comma: Option<usize> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\\' {
                i += 1;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
            last_comma = None;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
            continue;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut j = i + 2;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            let end = (j + 2).min(bytes.len());
            for k in i..end {
                if out[k] != b'\n' {
                    out[k] = b' ';
                }
            }
            i = end;
            continue;
        } else if b == b',' {
            last_comma = Some(i);
        } else if b == b'}' || b == b']' {
            if let Some(c) = last_comma.take() {
                out[c] = b' ';
            }
        } else if !b.is_ascii_whitespace() {
            last_comma = None;
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Deep enough for any real file, shallow enough that recursion cannot overflow the stack.
const MAX_DEPTH: usize = 256;

struct Reader<'a> {
    src: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn err(&self, message: &'static str) -> JsonError {
        JsonError { message, pos: self.pos }
    }

    fn skip_ws(&mut self) {
        while matches!(self.src.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        match self.src.get(self.pos) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("Unexpected character")),
            None => Err(self.err("Unexpected end of input")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.src[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.err("Unexpected character"))
        }
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("Nested too deeply"));
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.src.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_ws();
            if self.src.get(self.pos) != Some(&b'"') {
                return Err(self.err("Expected a string key"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.src.get(self.pos) != Some(&b':') {
                return Err(self.err("Expected `:`"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.err("Expected `,` or `}`")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.src.get(self.pos) == Some(&b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("Expected `,` or `]`")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.src[self.pos] == b'-' {
            self.pos += 1;
        }
        match self.src.get(self.pos) {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.err("Invalid number")),
        }
        if self.src.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.err("Invalid number"));
            }
            self.digits();
        }
        if matches!(self.src.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.src.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.err("Invalid number"));
            }
            self.digits();
        }
        // The slice is ASCII digits and punctuation, so it is valid UTF-8 and a valid float.
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        Ok(Json::Number(text.parse().unwrap()))
    }

    fn digits(&mut self) {
        while matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            // Copy the run of ordinary bytes in one go; the source is valid UTF-8 and the
            // run ends at an ASCII byte, so it cuts on a character boundary.
            let run_start = self.pos;
            while let Some(&b) = self.src.get(self.pos) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(std::str::from_utf8(&self.src[run_start..self.pos]).unwrap());
            match self.src.get(self.pos) {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escape = *self.src.get(self.pos).ok_or_else(|| self.err("Unterminated string"))?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(self.err("Invalid escape")),
                    }
                }
                Some(_) => return Err(self.err("Control character in string")),
                None => return Err(self.err("Unterminated string")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self.src.get(self.pos..self.pos + 4).ok_or_else(|| self.err("Invalid escape"))?;
        let mut value = 0;
        for &d in digits {
            let digit = (d as char).to_digit(16).ok_or_else(|| self.err("Invalid escape"))?;
            value = value * 16 + digit;
        }
        self.pos += 4;
        Ok(value)
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let first = self.hex4()?;
        if (0xd800..0xdc00).contains(&first) {
            // A high surrogate must be followed by a low one to form a character.
            if self.src[self.pos..].starts_with(b"\\u") {
                self.pos += 2;
                let second = self.hex4()?;
                if (0xdc00..0xe000).contains(&second) {
                    let c = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
                    return Ok(char::from_u32(c).unwrap());
                }
            }
            return Ok('\u{fffd}');
        }
        // A lone low surrogate is not a character; Node's JSON.parse keeps it, a Rust string cannot.
        Ok(char::from_u32(first).unwrap_or('\u{fffd}'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Json {
        Json::parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    #[test]
    fn scalars_and_containers() {
        assert_eq!(ok("null"), Json::Null);
        assert_eq!(ok(" true "), Json::Bool(true));
        assert_eq!(ok("-1.5e2"), Json::Number(-150.0));
        assert_eq!(ok("[1, [2], {}]").as_array().unwrap().len(), 3);
        let v = ok(r#"{"a": {"b": ["x"]}, "a": 2}"#);
        assert_eq!(v.get("a"), Some(&Json::Number(2.0)), "the last duplicate wins");
    }

    #[test]
    fn keeps_object_order() {
        let v = ok(r#"{"import": 1, "default": 2, "require": 3}"#);
        let keys: Vec<_> = v.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["import", "default", "require"]);
    }

    #[test]
    fn string_escapes() {
        assert_eq!(ok(r#""a\n\t\"\\\/""#), Json::String("a\n\t\"\\/".into()));
        assert_eq!(ok(r#""\u0041\u00e9""#), Json::String("Aé".into()));
        assert_eq!(ok(r#""\ud83d\ude00""#), Json::String("😀".into()));
        assert_eq!(ok("\"héllo\""), Json::String("héllo".into()));
        assert_eq!(ok(r#""\ud83d""#), Json::String("\u{fffd}".into()));
    }

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        let v = Json::parse_jsonc("{\n // line\n \"a\": [1, 2,], /* block */ \"b\": \"x // not a comment\", \"c\": {\"d\": 1,},\n}").unwrap();
        assert_eq!(v.get("a").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(v.get("b").unwrap().as_str(), Some("x // not a comment"));
        assert!(v.get("c").unwrap().get("d").is_some());
        assert!(Json::parse_jsonc("{\"a\": 1,, }").is_err());
    }

    #[test]
    fn byte_order_mark_is_skipped() {
        assert_eq!(ok("\u{feff}{}"), Json::Object(vec![]));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "", "{", "[1,]", "{\"a\":1,}", "{'a':1}", "01", "1.", "-", "1e", "\"a\nb\"", "\"\\x\"", "tru", "[1] 2", "nul", "{\"a\" 1}", "\"abc",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let text = "[".repeat(10_000);
        assert_eq!(Json::parse(&text).unwrap_err().message, "Nested too deeply");
    }
}
