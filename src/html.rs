//! Just enough of an HTML page for a build: where its module scripts and style sheets point.

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
    references(html, "<script", |attrs| {
        let module = attrs.iter().any(|(n, v, _)| n == "type" && v.eq_ignore_ascii_case("module"));
        if module { attrs.iter().filter(|(n, _, _)| n == "src").cloned().collect() } else { Vec::new() }
    })
}

/// The `<link rel="stylesheet" href="...">` tags of a page that point at local files, in order.
pub fn style_sheets(html: &str) -> Vec<ScriptRef> {
    references(html, "<link", |attrs| {
        let sheet = attrs.iter().any(|(n, v, _)| n == "rel" && v.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("stylesheet")));
        if sheet { attrs.iter().filter(|(n, _, _)| n == "href").cloned().collect() } else { Vec::new() }
    })
}

/// The other files a page names: images, media, icons and manifests, and the `url()`s in its `<style>` blocks. Local
/// files only; whether one exists is for the caller to see.
pub fn assets(html: &str) -> Vec<ScriptRef> {
    let mut out = Vec::new();
    for tag in ["<img", "<source", "<video", "<audio", "<track", "<embed"] {
        out.extend(references(html, tag, |attrs| attrs.iter().filter(|(n, _, _)| matches!(n.as_str(), "src" | "poster")).cloned().collect()));
    }
    // Links that name a file the page uses; `preconnect` and the like name a place, not a file.
    out.extend(references(html, "<link", |attrs| {
        let rel: Vec<String> = attrs.iter().filter(|(n, _, _)| n == "rel").flat_map(|(_, v, _)| v.split_ascii_whitespace().map(str::to_ascii_lowercase).collect::<Vec<_>>()).collect();
        let files = rel.iter().any(|r| matches!(r.as_str(), "icon" | "apple-touch-icon" | "mask-icon" | "manifest" | "preload" | "prefetch" | "modulepreload"));
        if files && !rel.iter().any(|r| r == "stylesheet") { attrs.iter().filter(|(n, _, _)| n == "href").cloned().collect() } else { Vec::new() }
    }));
    // `url(...)` inside `<style>` blocks.
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(found) = lower[at..].find("<style") {
        let open_end = match lower[at + found..].find('>') {
            Some(n) => at + found + n + 1,
            None => break,
        };
        let close = lower[open_end..].find("</style").map_or(html.len(), |n| open_end + n);
        let mut i = open_end;
        while let Some(n) = lower[i..close].find("url(") {
            let mut start = i + n + 4;
            while html.as_bytes().get(start).is_some_and(|b| b.is_ascii_whitespace()) {
                start += 1;
            }
            let quote = html.as_bytes().get(start).copied().filter(|b| *b == b'"' || *b == b'\'');
            if quote.is_some() {
                start += 1;
            }
            let end = html[start..close].find(|c: char| Some(c as u8) == quote && quote.is_some() || (quote.is_none() && (c == ')' || c.is_ascii_whitespace()))).map_or(close, |m| start + m);
            let value = html[start..end].to_string();
            if !value.is_empty() && !value.starts_with("//") && !value.contains("://") && !value.starts_with("data:") {
                out.push(ScriptRef { start, end, src: value });
            }
            i = end.max(i + n + 4);
        }
        at = close;
    }
    out.sort_by_key(|r| r.start);
    out
}

/// The local file a tag points at: `pick` chooses the attribute from the tag's attributes.
fn references(html: &str, open: &str, pick: impl Fn(&[(String, String, usize)]) -> Vec<(String, String, usize)>) -> Vec<ScriptRef> {
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(open) {
        let tag_start = at + found;
        let after = tag_start + open.len();
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
        let attrs: Vec<(String, String, usize)> = attributes(&html[after..i.min(html.len())]).into_iter().map(|(n, v, p)| (n, v, p + after)).collect();
        for (_, value, start) in pick(&attrs) {
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
    fn finds_local_style_sheets() {
        let html = "<link rel=\"stylesheet\" href=\"/a.css\"><link rel=icon href=i.png><link href='b.css' rel='preload stylesheet'><link rel=\"stylesheet\" href=\"https://x.test/c.css\">";
        assert_eq!(style_sheets(html).iter().map(|s| s.src.as_str()).collect::<Vec<_>>(), ["/a.css", "b.css"]);
    }

    #[test]
    fn finds_the_other_files() {
        let html = "<link rel=icon href=/i.png><link rel=preconnect href=https://a.test><img src=\"a.png\" alt><video poster=p.jpg src=v.mp4></video><style>a{background:url('b.png')}c{background:url(d.png) , url(//x.test/e.png)}</style><img src=https://x.test/f.png>";
        let found: Vec<String> = assets(html).into_iter().map(|r| r.src).collect();
        assert_eq!(found, ["/i.png", "a.png", "p.jpg", "v.mp4", "b.png", "d.png"]);
    }

    #[test]
    fn ignores_inline_scripts_and_lookalikes() {
        assert!(module_scripts("<script type=\"module\">import './a.js'</script><scripty src=\"a.js\" type=\"module\">").is_empty());
    }
}
