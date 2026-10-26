//! Legal comments: the ones that say who wrote a piece of code and under what terms it may be used (`/*! ... */`, `//! ...`, and any comment
//! with `@license` or `@preserve` in it). Minifying drops comments, and a license that goes with the code must not go with them, so these are
//! collected from the modules of a file and written where `--legal-comments` says.
//!
//! The comments are found by looking for their markers and the comment around each, not by reading the whole text: a marker inside a string
//! is taken for one only if the text around it looks like a comment's, which is the rare case this accepts.

/// A choice of `--legal-comments`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LegalComments {
    /// At the end of each file that holds the code they belong to.
    #[default]
    Eof,
    None,
    /// In a file of their own (`NAME.LEGAL.txt`), named in a comment at the end of the file.
    Linked,
    /// In a file of their own, which the file does not name.
    External,
}

/// The legal comments of a module's text, as written, each once.
pub fn find(source: &str) -> Vec<String> {
    // Most files have none, and the check is a few searches.
    if !(source.contains("/*!") || source.contains("//!") || source.contains("@license") || source.contains("@preserve")) {
        return Vec::new();
    }
    let bytes = source.as_bytes();
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut add = |start: usize, end: usize| {
        if !found.iter().any(|&(s, _)| s == start) {
            found.push((start, end));
        }
    };
    // `/*! ... */`
    let mut from = 0;
    while let Some(at) = source[from..].find("/*!") {
        let start = from + at;
        let end = source[start + 3..].find("*/").map_or(source.len(), |e| start + 3 + e + 2);
        add(start, end);
        from = end.max(start + 3);
    }
    // `/* ... @license ... */` and `// ... @preserve`: the comment around the word.
    for marker in ["@license", "@preserve"] {
        let mut from = 0;
        while let Some(at) = source[from..].find(marker) {
            let position = from + at;
            from = position + marker.len();
            let line_start = source[..position].rfind('\n').map_or(0, |n| n + 1);
            let open = source[..position].rfind("/*").filter(|&o| !source[o + 2..position].contains("*/"));
            match open {
                Some(start) => {
                    let end = source[position..].find("*/").map_or(source.len(), |e| position + e + 2);
                    add(start, end);
                    from = from.max(end);
                }
                None => {
                    // A line comment: `//` before the word on its line, with only text between.
                    if let Some(slashes) = source[line_start..position].find("//") {
                        let start = line_start + slashes;
                        let end = source[position..].find('\n').map_or(source.len(), |e| position + e);
                        add(start, end);
                    }
                }
            }
        }
    }
    // `//! ...` at the start of a line (after blanks), a run of them as one comment.
    let mut at = 0;
    while at < bytes.len() {
        let line_end = source[at..].find('\n').map_or(source.len(), |e| at + e);
        let trimmed = source[at..line_end].trim_start();
        if trimmed.starts_with("//!") {
            add(line_end - trimmed.len(), line_end);
        }
        at = line_end + 1;
    }
    found.sort_unstable();
    let mut out: Vec<String> = Vec::new();
    for (start, end) in found {
        // A comment inside another (found by two markers) is not repeated.
        let text = source[start..end].trim_end().to_string();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_comments_that_say_who_wrote_it() {
        let src = "/*! (c) Someone 2020\n * MIT */\nfunction a() {}\n/** @license ISC\n * terms */\nconst b = 1; // @preserve keep me\n//! one\n//! two\n/* not legal */\n// not legal either\n";
        assert_eq!(find(src), ["/*! (c) Someone 2020\n * MIT */", "/** @license ISC\n * terms */", "// @preserve keep me", "//! one", "//! two"]);
    }

    #[test]
    fn a_file_without_any_has_none_and_a_repeat_is_kept_once() {
        assert!(find("const a = 1; /* plain */ // plain\n").is_empty());
        assert_eq!(find("/*! x */ a(); /*! x */ b();"), ["/*! x */"]);
        assert_eq!(find("/*! @license MIT */"), ["/*! @license MIT */"]);
    }
}
