//! Loaders: how a file that is not JavaScript becomes a module.
//!
//! Text, base64, data URL, binary, file and empty files become a small ES module of their own
//! (`export default ...`) that is read like any other source. A `file` is also copied to the
//! output under a name that carries a hash of its contents. A style sheet is a module that
//! exports its local class names (if it has any) and whose CSS goes to a file of its own.

/// What to do with a file, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loader {
    /// The file's text as a string.
    Text,
    /// The file's bytes as a base64 string.
    Base64,
    /// A `data:` URL of the file.
    DataUrl,
    /// The file's bytes as a `Uint8Array`.
    Binary,
    /// The file is copied to the output; the module is its address.
    File,
    /// An empty object.
    Empty,
    /// A style sheet; class names stay as written.
    Css,
    /// A style sheet whose class names are local to it and exported.
    LocalCss,
}

impl Loader {
    pub fn parse(name: &str) -> Option<Loader> {
        Some(match name {
            "text" => Loader::Text,
            "base64" => Loader::Base64,
            "dataurl" => Loader::DataUrl,
            "binary" => Loader::Binary,
            "file" => Loader::File,
            "empty" => Loader::Empty,
            "css" => Loader::Css,
            "local-css" => Loader::LocalCss,
            _ => return None,
        })
    }

    /// The loader a file gets when no option names one. `None` for the extensions the JavaScript
    /// parser reads.
    pub fn by_default(file_name: &str) -> Option<Loader> {
        let name = file_name.to_ascii_lowercase();
        if name.ends_with(".module.css") {
            return Some(Loader::LocalCss);
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e)?;
        Some(match ext {
            "css" => Loader::Css,
            "txt" | "md" | "html" | "htm" => Loader::Text,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "ico" | "bmp" | "svg" | "woff" | "woff2" | "ttf" | "otf" | "eot" | "mp3" | "mp4" | "webm" | "ogg" | "wav" | "pdf" | "wasm" => Loader::File,
            _ => return None,
        })
    }

    pub fn is_css(self) -> bool {
        matches!(self, Loader::Css | Loader::LocalCss)
    }

    pub fn needs_bytes(self) -> bool {
        matches!(self, Loader::Base64 | Loader::DataUrl | Loader::Binary | Loader::File)
    }
}

/// A file copied to the output.
#[derive(Clone, Debug)]
pub struct Asset {
    /// The name in the output directory.
    pub name: String,
    /// What the program and style sheets refer to it by.
    pub url: String,
    pub data: std::sync::Arc<Vec<u8>>,
    /// The file it was copied from.
    pub original: String,
}

/// The module text for a loader that makes a plain value.
pub fn synthesize(loader: Loader, bytes: &[u8], file_name: &str, public_url: Option<&str>) -> String {
    match loader {
        Loader::Text => format!("export default {};\n", quote(&String::from_utf8_lossy(bytes))),
        Loader::Base64 => format!("export default \"{}\";\n", base64(bytes)),
        Loader::DataUrl => format!("export default \"data:{};base64,{}\";\n", mime(file_name), base64(bytes)),
        Loader::Binary => format!("export default Uint8Array.from(atob(\"{}\"), (c) => c.charCodeAt(0));\n", base64(bytes)),
        Loader::File => format!("export default {};\n", quote(public_url.unwrap_or(""))),
        Loader::Empty | Loader::Css | Loader::LocalCss => "export default {};\n".to_string(),
    }
}

/// The module text of a style sheet with local names: the object of names, and each name that is
/// an identifier as a named export too.
pub fn local_names_module(exports: &[(String, String)]) -> String {
    let mut out = String::from("export default {");
    for (i, (name, value)) in exports.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{}:{}", quote(name), quote(value)));
    }
    out.push_str("};\n");
    for (name, value) in exports {
        if is_export_name(name) {
            out.push_str(&format!("export const {name} = {};\n", quote(value)));
        }
    }
    out
}

fn is_export_name(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "default", "break", "case", "catch", "class", "const", "continue", "debugger", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "function",
        "if", "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with", "yield", "let",
        "static", "await", "async", "of", "arguments", "eval", "undefined", "NaN", "Infinity",
    ];
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c == '$' || c.is_ascii_alphabetic()) && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric()) && !RESERVED.contains(&name)
}

pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn base64(data: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(DIGITS[(n >> 18) as usize & 63] as char);
        out.push(DIGITS[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { DIGITS[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { DIGITS[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The bytes a base64 string stands for; characters outside the alphabet are skipped.
pub fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut bits = 0u32;
    let mut count = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        bits = (bits << 6) | v;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out
}

fn mime(file_name: &str) -> &'static str {
    let ext = file_name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "json" => "application/json",
        "js" | "mjs" => "text/javascript",
        _ => "application/octet-stream",
    }
}

/// A short stable hash for names: 64-bit FNV-1a written in lowercase base 32.
pub fn hash(data: &[u8], length: usize) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Mix the bits once more so short inputs spread over the whole word.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    const DIGITS: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    for k in 0..length.min(12) {
        out.push(DIGITS[((h >> (5 * k)) & 31) as usize] as char);
    }
    out
}

/// Fills in `[name]`, `[hash]` and `[ext]` in an output name template; the extension is added when the
/// template does not say where it goes.
pub fn asset_name(template: &str, file_name: &str, data: &[u8]) -> String {
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e),
        _ => (file_name, ""),
    };
    let mut name = template.replace("[name]", stem).replace("[hash]", &hash(data, 8)).replace("[ext]", ext);
    if !template.contains("[ext]") && !ext.is_empty() {
        name.push('.');
        name.push_str(ext);
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modules_for_values() {
        assert_eq!(synthesize(Loader::Text, "a\"b\n".as_bytes(), "a.txt", None), "export default \"a\\\"b\\n\";\n");
        assert_eq!(synthesize(Loader::Base64, b"hello", "a.bin", None), "export default \"aGVsbG8=\";\n");
        assert_eq!(synthesize(Loader::DataUrl, b"hi", "a.png", None), "export default \"data:image/png;base64,aGk=\";\n");
        assert_eq!(synthesize(Loader::File, b"x", "a.png", Some("./a-xyz.png")), "export default \"./a-xyz.png\";\n");
    }

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\xfe hello"] {
            assert_eq!(base64_decode(&base64(data)), data);
        }
    }

    #[test]
    fn defaults_by_extension() {
        assert_eq!(Loader::by_default("a.PNG"), Some(Loader::File));
        assert_eq!(Loader::by_default("a.module.css"), Some(Loader::LocalCss));
        assert_eq!(Loader::by_default("a.css"), Some(Loader::Css));
        assert_eq!(Loader::by_default("a.js"), None);
        assert_eq!(Loader::by_default("Makefile"), None);
    }

    #[test]
    fn names_carry_a_hash() {
        let a = asset_name("[name]-[hash]", "logo.png", b"one");
        let b = asset_name("[name]-[hash]", "logo.png", b"two");
        assert!(a.starts_with("logo-") && a.ends_with(".png") && a != b);
        assert_eq!(asset_name("assets/[name].[ext]", "f.woff2", b""), "assets/f.woff2");
    }

    #[test]
    fn local_names_become_exports() {
        let text = local_names_module(&[("btn".into(), "h_btn".into()), ("a-b".into(), "h_a-b".into()), ("class".into(), "h_class".into())]);
        assert!(text.contains("export default {\"btn\":\"h_btn\",\"a-b\":\"h_a-b\",\"class\":\"h_class\"};"));
        assert!(text.contains("export const btn = \"h_btn\";") && !text.contains("export const class") && !text.contains("a-b ="));
    }
}
