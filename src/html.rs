//! Just enough of an HTML page for a build: where its module scripts point.

/// A `<script type="module" src="...">` whose source is a file of the project.
#[derive(Debug, PartialEq)]
pub struct ScriptRef {
    /// Where the value of `src` is in the page (without the quotes).
    pub start: usize,
    pub end: usize,
    pub src: String,
}

/// The module scripts of a page that point at local files, in order.
pub fn module_scripts(html: &str) -> Vec<ScriptRef> {
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find("<script") {
        let tag_start = at + found;
        let after = tag_start + "<script".len();
        // `<scripty>` is not a script.
        if !bytes.get(after).is_some_and(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/') {
            at = after;
            continue;
        }
        // The tag ends at the first `>` outside quotes.
        let mut i = after;
        let mut quote = 0u8;
        while i < bytes.len() {
            match bytes[i] {
                q @ (b'"' | b'\'') if quote == 0 => quote = q,
                q if q == quote => quote = 0,
                b'>' if quote == 0 => break,
                _ => {}
            }
            i += 1;
        }
        let attrs = &html[after..i.min(html.len())];
        let (mut module, mut src) = (false, None);
        for (name, value, value_start) in attributes(attrs) {
            match name.as_str() {
                "type" => module = value.eq_ignore_ascii_case("module"),
                "src" => src = Some((value_start + after, value)),
                _ => {}
            }
        }
        if let (true, Some((start, value))) = (module, src) {
            let remote = value.starts_with("//") || value.contains("://") || value.starts_with("data:");
            if !remote && !value.is_empty() {
                out.push(ScriptRef { start, end: start + value.len(), src: value });
            }
        }
        at = i.max(after);
    }
    out
}

/// `name="value"`, `name='value'`, `name=value` and bare `name`, with the value's place in the text.
fn attributes(text: &str) -> Vec<(String, String, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' {
            i += 1;
        }
        if start == i {
            break;
        }
        let name = text[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            out.push((name, String::new(), i));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let (value_start, value_end) = match bytes.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                let s = i + 1;
                let e = text[s..].find(q as char).map_or(bytes.len(), |n| s + n);
                i = (e + 1).min(bytes.len());
                (s, e)
            }
            _ => {
                let s = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                (s, i)
            }
        };
        out.push((name, text[value_start..value_end].to_string(), value_start));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_local_module_scripts() {
        let html = "<html><script type=\"module\" src=\"/src/main.ts\"></script><script src=\"classic.js\"></script>\n<script type='module' src='./b.js'></script><script type=module src=https://x.test/a.js></script></html>";
        let found = module_scripts(html);
        assert_eq!(found.iter().map(|s| s.src.as_str()).collect::<Vec<_>>(), ["/src/main.ts", "./b.js"]);
        assert_eq!(&html[found[0].start..found[0].end], "/src/main.ts");
    }

    #[test]
    fn ignores_inline_scripts_and_lookalikes() {
        assert!(module_scripts("<script type=\"module\">import './a.js'</script><scripty src=\"a.js\" type=\"module\">").is_empty());
    }
}
