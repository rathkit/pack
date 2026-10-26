//! Legal comments: the ones that say who wrote a piece of code and under what terms it may be used (`/*! ... */`, `//! ...`, and any comment
//! with `@license` or `@preserve` in it). Minifying drops comments, and a license that goes with the code must not go with them, so these are
//! collected from the modules of a file and written where `--legal-comments` says.
//!
//! The lexer notes where they are as it reads a module (it passes every comment anyway), so finding them costs nothing extra and a marker in a
//! string is not taken for a comment.

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

#[cfg(test)]
mod tests {
    use crate::parser::{ParseOptions, parse};

    /// The legal comments the parser notes, as written.
    fn legal(source: &str) -> Vec<String> {
        parse(source, ParseOptions::new(true)).unwrap().legal.iter().map(|&(s, e)| source[s as usize..e as usize].trim_end().to_string()).collect()
    }

    #[test]
    fn the_comments_that_say_who_wrote_it() {
        let src = "/*! (c) Someone 2020\n * MIT */\nfunction a() {}\n/** @license ISC\n * terms */\nconst b = 1; // @preserve keep me\n//! one\n//! two\n/* not legal */\n// not legal either\n";
        assert_eq!(legal(src), ["/*! (c) Someone 2020\n * MIT */", "/** @license ISC\n * terms */", "// @preserve keep me", "//! one", "//! two"]);
    }

    #[test]
    fn a_file_without_any_has_none_and_one_met_twice_is_noted_once() {
        assert!(legal("const a = 1; /* plain */ // plain\n").is_empty());
        // A regular expression, an arrow function and a type cast make the parser go back over what it has read.
        assert_eq!(legal("const r = /*! x */ (a) => /b/g.test(a);\nconst t = /*! y */ x < 1 > (z);\n").len(), 2);
        // A marker in a string is not a comment.
        assert!(legal("const s = '/*! no */'; const t = `//! no`;").is_empty());
    }
}
