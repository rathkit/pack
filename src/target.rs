//! `--target`: the version of JavaScript the output may use, and which syntax newer than it has to be rewritten.
//!
//! A target is `es5` to `es2025`, `esnext` (nothing is rewritten), engines with versions (`chrome80`, `ie11`), or a list of them. The features
//! below are the ones that are newer than `es5` that this program knows how to rewrite, or knows it cannot, in which case a build for a target
//! that lacks them stops with an error that names the feature. Regular expression syntax and library functions (`Array.prototype.at`) are not looked at.

/// A piece of syntax newer than `es2015`, with the year of the language that has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// `a ** b` and `a **= b`.
    Exponent,
    /// A comma after the last parameter or argument.
    TrailingComma,
    /// `async` functions and `await`.
    AsyncFunction,
    /// `{ ...a }` in an object literal.
    ObjectSpread,
    /// `{ a, ...rest }` in a pattern.
    ObjectRest,
    /// `for await` and async generators.
    AsyncIteration,
    /// `catch { }` with no binding.
    OptionalCatch,
    /// `a?.b`, `a?.[b]` and `a?.()`.
    OptionalChain,
    /// `a ?? b`.
    NullishCoalescing,
    /// `123n`.
    BigInt,
    /// `a ||= b`, `a &&= b` and `a ??= b`.
    LogicalAssignment,
    /// `1_000`.
    NumericSeparator,
    /// `class { x = 1 }`.
    ClassField,
    /// `class { #x }`, private methods and `#x in o`.
    ClassPrivate,
    /// `class { static { } }`.
    ClassStaticBlock,
    /// `await` outside a function.
    TopLevelAwait,
    /// `(a) => a`.
    Arrow,
    /// `class A { }`.
    Class,
    /// `let` and `const`.
    LetConst,
    /// `` `a${b}` `` and tagged templates.
    Template,
    /// `[a, b] = c` and `{ a } = b` in declarations, parameters and assignments.
    Destructuring,
    /// `function (a = 1, ...rest)`.
    Params,
    /// `f(...a)` and `[...a]`.
    Spread,
    /// `for (a of b)`.
    ForOf,
    /// `{ a, b() {}, [c]: 1 }`.
    ObjectLiteral,
    /// `function* () {}` and `yield`.
    Generator,
    /// `new.target`.
    NewTarget,
    /// `0b11` and `0o7`.
    NumberLiteral,
    /// `\u{1F600}` in a string.
    UnicodeEscape,
}

impl Feature {
    /// The year of the language that has it.
    pub fn since(self) -> u16 {
        match self {
            Feature::Exponent => 2016,
            Feature::TrailingComma | Feature::AsyncFunction => 2017,
            Feature::ObjectSpread | Feature::ObjectRest | Feature::AsyncIteration => 2018,
            Feature::OptionalCatch => 2019,
            Feature::OptionalChain | Feature::NullishCoalescing | Feature::BigInt => 2020,
            Feature::LogicalAssignment | Feature::NumericSeparator => 2021,
            Feature::ClassField | Feature::ClassPrivate | Feature::ClassStaticBlock | Feature::TopLevelAwait => 2022,
            Feature::Arrow | Feature::Class | Feature::LetConst | Feature::Template | Feature::Destructuring | Feature::Params | Feature::Spread | Feature::ForOf | Feature::ObjectLiteral | Feature::Generator | Feature::NewTarget | Feature::NumberLiteral | Feature::UnicodeEscape => 2015,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Feature::Exponent => "the exponentiation operator (**)",
            Feature::TrailingComma => "a trailing comma in a parameter or argument list",
            Feature::AsyncFunction => "an async function",
            Feature::ObjectSpread => "object spread ({ ...a })",
            Feature::ObjectRest => "object rest in a pattern ({ a, ...rest })",
            Feature::AsyncIteration => "async iteration (for await, async generators)",
            Feature::OptionalCatch => "catch without a binding",
            Feature::OptionalChain => "optional chaining (?.)",
            Feature::NullishCoalescing => "nullish coalescing (??)",
            Feature::BigInt => "a BigInt literal",
            Feature::LogicalAssignment => "logical assignment (||=, &&=, ??=)",
            Feature::NumericSeparator => "a numeric separator (1_000)",
            Feature::ClassField => "a class field",
            Feature::ClassPrivate => "a private class member (#name)",
            Feature::ClassStaticBlock => "a class static block",
            Feature::TopLevelAwait => "top-level await",
            Feature::Arrow => "an arrow function",
            Feature::Class => "a class",
            Feature::LetConst => "let or const",
            Feature::Template => "a template literal",
            Feature::Destructuring => "destructuring",
            Feature::Params => "a default or rest parameter",
            Feature::Spread => "spread in an array or a call",
            Feature::ForOf => "a for-of loop",
            Feature::ObjectLiteral => "a shorthand property, a method or a computed key in an object literal",
            Feature::Generator => "a generator",
            Feature::NewTarget => "new.target",
            Feature::NumberLiteral => "a binary or octal literal (0b11, 0o7)",
            Feature::UnicodeEscape => "a code point escape (\\u{1F600})",
        }
    }
}

/// Engines and the first version of each that has a feature as written. From the compatibility tables of MDN; where a feature came in
/// parts, the version that has all of what this program rewrites (private members: methods and `#x in o`).
const COMPAT: &[(Feature, &[(&str, (u16, u16))])] = &[
    (Feature::Exponent, &[("chrome", (52, 0)), ("edge", (14, 0)), ("firefox", (52, 0)), ("safari", (10, 1)), ("ios", (10, 3)), ("node", (7, 0)), ("opera", (39, 0))]),
    (Feature::TrailingComma, &[("chrome", (58, 0)), ("edge", (14, 0)), ("firefox", (52, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (8, 0)), ("opera", (45, 0))]),
    (Feature::AsyncFunction, &[("chrome", (55, 0)), ("edge", (15, 0)), ("firefox", (52, 0)), ("safari", (10, 1)), ("ios", (10, 3)), ("node", (7, 6)), ("opera", (42, 0))]),
    (Feature::ObjectSpread, &[("chrome", (60, 0)), ("edge", (79, 0)), ("firefox", (55, 0)), ("safari", (11, 1)), ("ios", (11, 3)), ("node", (8, 3)), ("opera", (47, 0))]),
    (Feature::ObjectRest, &[("chrome", (60, 0)), ("edge", (79, 0)), ("firefox", (55, 0)), ("safari", (11, 1)), ("ios", (11, 3)), ("node", (8, 3)), ("opera", (47, 0))]),
    (Feature::AsyncIteration, &[("chrome", (63, 0)), ("edge", (79, 0)), ("firefox", (57, 0)), ("safari", (12, 0)), ("ios", (12, 0)), ("node", (10, 0)), ("opera", (50, 0))]),
    (Feature::OptionalCatch, &[("chrome", (66, 0)), ("edge", (79, 0)), ("firefox", (58, 0)), ("safari", (11, 1)), ("ios", (11, 3)), ("node", (10, 0)), ("opera", (53, 0))]),
    (Feature::OptionalChain, &[("chrome", (80, 0)), ("edge", (80, 0)), ("firefox", (74, 0)), ("safari", (13, 1)), ("ios", (13, 4)), ("node", (14, 0)), ("opera", (67, 0))]),
    (Feature::NullishCoalescing, &[("chrome", (80, 0)), ("edge", (80, 0)), ("firefox", (72, 0)), ("safari", (13, 1)), ("ios", (13, 4)), ("node", (14, 0)), ("opera", (67, 0))]),
    (Feature::BigInt, &[("chrome", (67, 0)), ("edge", (79, 0)), ("firefox", (68, 0)), ("safari", (14, 0)), ("ios", (14, 0)), ("node", (10, 4)), ("opera", (54, 0))]),
    (Feature::LogicalAssignment, &[("chrome", (85, 0)), ("edge", (85, 0)), ("firefox", (79, 0)), ("safari", (14, 0)), ("ios", (14, 0)), ("node", (15, 0)), ("opera", (71, 0))]),
    (Feature::NumericSeparator, &[("chrome", (75, 0)), ("edge", (79, 0)), ("firefox", (70, 0)), ("safari", (13, 0)), ("ios", (13, 0)), ("node", (12, 5)), ("opera", (62, 0))]),
    (Feature::ClassField, &[("chrome", (72, 0)), ("edge", (79, 0)), ("firefox", (75, 0)), ("safari", (14, 1)), ("ios", (14, 5)), ("node", (12, 0)), ("opera", (60, 0))]),
    (Feature::ClassPrivate, &[("chrome", (91, 0)), ("edge", (91, 0)), ("firefox", (90, 0)), ("safari", (15, 0)), ("ios", (15, 0)), ("node", (16, 4)), ("opera", (77, 0))]),
    (Feature::ClassStaticBlock, &[("chrome", (94, 0)), ("edge", (94, 0)), ("firefox", (93, 0)), ("safari", (16, 4)), ("ios", (16, 4)), ("node", (16, 11)), ("opera", (80, 0))]),
    (Feature::TopLevelAwait, &[("chrome", (89, 0)), ("edge", (89, 0)), ("firefox", (89, 0)), ("safari", (15, 0)), ("ios", (15, 0)), ("node", (14, 8)), ("opera", (75, 0))]),
    (Feature::Arrow, &[("chrome", (45, 0)), ("edge", (12, 0)), ("firefox", (22, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (4, 0)), ("opera", (32, 0))]),
    (Feature::Class, &[("chrome", (49, 0)), ("edge", (13, 0)), ("firefox", (45, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (6, 0)), ("opera", (36, 0))]),
    (Feature::LetConst, &[("chrome", (49, 0)), ("edge", (14, 0)), ("firefox", (44, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (6, 0)), ("opera", (36, 0))]),
    (Feature::Template, &[("chrome", (41, 0)), ("edge", (12, 0)), ("firefox", (34, 0)), ("safari", (9, 0)), ("ios", (9, 0)), ("node", (4, 0)), ("opera", (28, 0))]),
    (Feature::Destructuring, &[("chrome", (51, 0)), ("edge", (18, 0)), ("firefox", (53, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (6, 5)), ("opera", (38, 0))]),
    (Feature::Params, &[("chrome", (49, 0)), ("edge", (14, 0)), ("firefox", (53, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (6, 0)), ("opera", (36, 0))]),
    (Feature::Spread, &[("chrome", (46, 0)), ("edge", (12, 0)), ("firefox", (27, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (5, 0)), ("opera", (33, 0))]),
    (Feature::ForOf, &[("chrome", (38, 0)), ("edge", (12, 0)), ("firefox", (13, 0)), ("safari", (7, 0)), ("ios", (7, 0)), ("node", (0, 12)), ("opera", (25, 0))]),
    (Feature::ObjectLiteral, &[("chrome", (47, 0)), ("edge", (12, 0)), ("firefox", (34, 0)), ("safari", (9, 0)), ("ios", (9, 0)), ("node", (4, 0)), ("opera", (34, 0))]),
    (Feature::Generator, &[("chrome", (39, 0)), ("edge", (13, 0)), ("firefox", (26, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (4, 0)), ("opera", (26, 0))]),
    (Feature::NewTarget, &[("chrome", (46, 0)), ("edge", (14, 0)), ("firefox", (41, 0)), ("safari", (10, 0)), ("ios", (10, 0)), ("node", (5, 0)), ("opera", (33, 0))]),
    (Feature::NumberLiteral, &[("chrome", (41, 0)), ("edge", (12, 0)), ("firefox", (25, 0)), ("safari", (9, 0)), ("ios", (9, 0)), ("node", (4, 0)), ("opera", (28, 0))]),
    (Feature::UnicodeEscape, &[("chrome", (44, 0)), ("edge", (12, 0)), ("firefox", (40, 0)), ("safari", (9, 0)), ("ios", (9, 0)), ("node", (4, 0)), ("opera", (31, 0))]),
];

/// The features that came with es2015.
pub const ES2015: &[Feature] = &[
    Feature::Arrow,
    Feature::Class,
    Feature::LetConst,
    Feature::Template,
    Feature::Destructuring,
    Feature::Params,
    Feature::Spread,
    Feature::ForOf,
    Feature::ObjectLiteral,
    Feature::Generator,
    Feature::NewTarget,
    Feature::NumberLiteral,
    Feature::UnicodeEscape,
];

/// `ie` has none of the features after `es5`, at any version (11 is its last).
const ENGINES: &[&str] = &["chrome", "edge", "firefox", "safari", "ios", "node", "opera", "ie"];

/// The language year `es5` stands for.
const ES5: u16 = 2009;

/// The version of JavaScript the output may use: a year of the language, the engines it must run in (a feature one of them lacks is
/// rewritten), or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    year: u16,
    /// The features some engine of the target lacks, one bit each.
    lacks: u64,
    /// The target as it was written, for messages.
    label: &'static str,
}

impl Target {
    /// Nothing is rewritten.
    pub const ESNEXT: Target = Target { year: u16::MAX, lacks: 0, label: "esnext" };

    /// A list of `es2015` to `es2025` (`es6` is `es2015`), `esnext`, and engines with a version (`chrome80`, `safari13.1`, `node18`,
    /// `firefox78`, `ios14.5`, `edge90`, `opera70`), separated by commas. The output uses only what all of them have.
    pub fn parse(text: &str) -> Result<Target, String> {
        let mut year = u16::MAX;
        let mut lacks = 0u64;
        let mut seen = false;
        for item in text.split(',') {
            let lower: String = item.trim().to_ascii_lowercase().split_whitespace().collect();
            if lower.is_empty() {
                continue;
            }
            seen = true;
            if lower == "esnext" {
                continue;
            }
            if let Some(rest) = lower.strip_prefix("es") {
                let parsed = match rest {
                    "5" => Some(ES5),
                    "6" => Some(2015),
                    other => other.parse::<u16>().ok(),
                };
                match parsed {
                    Some(y @ (ES5 | 2015..=2025)) => year = year.min(y),
                    Some(3 | 2011) => return Err(format!("the target {item} is not supported: es5 is the lowest")),
                    _ => return Err(format!("'{item}' is not a target: use es5, es2015 to es2025, esnext, or an engine with a version such as chrome80 or safari13.1")),
                }
                continue;
            }
            let split = lower.find(|c: char| c.is_ascii_digit()).unwrap_or(lower.len());
            let (engine, version) = lower.split_at(split);
            let engine = if engine == "ios_saf" || engine == "ios-saf" { "ios" } else { engine };
            if !ENGINES.contains(&engine) {
                return Err(format!("'{item}' is not a target: use es5, es2015 to es2025, esnext, or an engine with a version (one of {}), such as chrome80", ENGINES.join(", ")));
            }
            let mut numbers = version.split('.');
            let major = numbers.next().and_then(|n| n.parse::<u16>().ok());
            let minor = numbers.next().map_or(Some(0), |n| n.parse::<u16>().ok());
            let (Some(major), Some(minor)) = (major, minor) else {
                return Err(format!("'{item}' needs a version after the engine, such as {engine}{}", if engine == "node" { "18" } else { "90" }));
            };
            if engine == "ie" {
                lacks |= COMPAT.iter().fold(0, |all, (feature, _)| all | 1 << (*feature as u32));
                continue;
            }
            for (feature, versions) in COMPAT {
                if let Some((_, first)) = versions.iter().find(|(name, _)| *name == engine) {
                    if (major, minor) < *first {
                        lacks |= 1 << (*feature as u32);
                    }
                }
            }
        }
        if !seen {
            return Err("an empty target: use es5, es2015 to es2025, esnext, or an engine with a version such as chrome80".to_string());
        }
        let label: &'static str = Box::leak(text.trim().to_ascii_lowercase().into_boxed_str());
        Ok(Target { year, lacks, label })
    }

    /// Whether the target has the feature as it is: its language year does, and so does every engine in it.
    pub fn supports(self, feature: Feature) -> bool {
        self.year >= feature.since() && self.lacks & (1 << (feature as u32)) == 0
    }

    /// Whether the target lacks any syntax that came with es2015, so the output must be written in es5 (arrow functions, `let`, classes).
    pub fn lacks_es2015(self) -> bool {
        ES2015.iter().any(|&f| !self.supports(f))
    }

    pub fn is_esnext(self) -> bool {
        self.year == u16::MAX && self.lacks == 0
    }

    /// The target as it was written (`es2020`, `chrome80,safari13`), or `esnext`.
    pub fn name(self) -> String {
        self.label.to_string()
    }
}

impl Default for Target {
    fn default() -> Self {
        Target::ESNEXT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_by_year() {
        assert!(!Target::parse("es2020").unwrap().supports(Feature::LogicalAssignment));
        assert!(Target::parse("es2021").unwrap().supports(Feature::LogicalAssignment));
        assert_eq!(Target::parse("ES6").unwrap().year, 2015);
        assert!(Target::parse("esnext").unwrap().is_esnext());
        let es5 = Target::parse("es5").unwrap();
        assert!(!es5.supports(Feature::Arrow) && !es5.supports(Feature::Exponent) && !es5.is_esnext());
        assert!(Target::parse("es3").unwrap_err().contains("lowest"));
        assert!(Target::parse("es2030").is_err());
        assert!(Target::parse("").is_err());
    }

    #[test]
    fn what_a_target_has() {
        let es2019 = Target::parse("es2019").unwrap();
        assert!(es2019.supports(Feature::ObjectSpread) && es2019.supports(Feature::OptionalCatch));
        assert!(!es2019.supports(Feature::OptionalChain) && !es2019.supports(Feature::ClassField));
        assert!(Target::ESNEXT.supports(Feature::ClassStaticBlock));
        assert!(!Target::parse("es2015").unwrap().supports(Feature::Exponent));
        assert!(Target::parse("es2015").unwrap().supports(Feature::Class));
    }

    #[test]
    fn ie_has_nothing_newer_than_es5() {
        let ie11 = Target::parse("ie11").unwrap();
        assert!(!ie11.supports(Feature::Arrow) && !ie11.supports(Feature::LetConst) && !ie11.supports(Feature::Template) && !ie11.supports(Feature::Exponent));
        assert!(!Target::parse("ie 11, chrome110").unwrap().supports(Feature::ForOf));
        assert!(Target::parse("chrome60").unwrap().supports(Feature::Arrow) && !Target::parse("chrome44").unwrap().supports(Feature::Arrow));
    }

    #[test]
    fn engines_say_what_they_have() {
        let chrome80 = Target::parse("chrome80").unwrap();
        assert!(chrome80.supports(Feature::OptionalChain) && chrome80.supports(Feature::NullishCoalescing) && chrome80.supports(Feature::AsyncIteration));
        assert!(!chrome80.supports(Feature::LogicalAssignment) && !chrome80.supports(Feature::ClassPrivate) && !chrome80.supports(Feature::TopLevelAwait));
        let safari13 = Target::parse("safari13").unwrap();
        assert!(!safari13.supports(Feature::OptionalChain) && !safari13.supports(Feature::BigInt) && safari13.supports(Feature::NumericSeparator));
        assert!(Target::parse("safari13.1").unwrap().supports(Feature::OptionalChain));
        // A list needs all of them; a year and an engine both count.
        let both = Target::parse("chrome110, safari13").unwrap();
        assert!(!both.supports(Feature::OptionalChain) && both.supports(Feature::Exponent));
        assert!(!Target::parse("es2018,chrome110").unwrap().supports(Feature::OptionalChain));
        assert!(Target::parse("chrome120").unwrap().is_esnext());
        assert_eq!(Target::parse("Chrome80, ios14.5").unwrap().name(), "chrome80, ios14.5");
        assert!(Target::parse("chrome").unwrap_err().contains("version"));
        assert!(Target::parse("netscape4").unwrap_err().contains("not a target"));
    }
}
