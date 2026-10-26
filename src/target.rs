//! `--target`: the version of JavaScript the output may use, and which syntax newer than it has to be rewritten.
//!
//! A target is `es2015` to `es2025`, or `esnext` (nothing is rewritten). The features below are the ones that are newer than `es2015`
//! (the lowest target: arrow functions, classes, `let` and `const`, template literals, destructuring and generators are never rewritten) and
//! that this program knows how to rewrite, or knows it cannot, in which case a build for a target that lacks them stops with an error that
//! names the feature. Regular expression syntax and library functions (`Array.prototype.at`) are not looked at.

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
        }
    }
}

/// The version of JavaScript the output may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target(u16);

impl Target {
    /// Nothing is rewritten.
    pub const ESNEXT: Target = Target(u16::MAX);

    /// `es2015` to `es2025` (`es6` is `es2015`) or `esnext`.
    pub fn parse(text: &str) -> Result<Target, String> {
        let lower = text.trim().to_ascii_lowercase();
        if lower == "esnext" {
            return Ok(Target::ESNEXT);
        }
        let year = match lower.strip_prefix("es") {
            Some("6") => Some(2015),
            Some(year) => year.parse::<u16>().ok(),
            None => None,
        };
        match year {
            Some(2015..=2025) => Ok(Target(year.unwrap())),
            Some(1 | 3 | 5 | 2009 | 2011) => Err(format!("the target {text} is not supported: es2015 is the lowest (functions, classes and let are not rewritten)")),
            _ => Err(format!("'{text}' is not a target: use es2015 to es2025, or esnext (engine targets such as chrome80 are not supported)")),
        }
    }

    /// Whether the language of this target has the feature as it is.
    pub fn supports(self, feature: Feature) -> bool {
        self.0 >= feature.since()
    }

    pub fn is_esnext(self) -> bool {
        self == Target::ESNEXT
    }

    /// `es2020`, or `esnext`.
    pub fn name(self) -> String {
        if self.is_esnext() { "esnext".to_string() } else { format!("es{}", self.0) }
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
        assert_eq!(Target::parse("es2020"), Ok(Target(2020)));
        assert_eq!(Target::parse("ES6"), Ok(Target(2015)));
        assert_eq!(Target::parse("esnext"), Ok(Target::ESNEXT));
        assert!(Target::parse("es5").unwrap_err().contains("lowest"));
        assert!(Target::parse("chrome80").unwrap_err().contains("engine targets"));
        assert!(Target::parse("es2030").is_err());
    }

    #[test]
    fn what_a_target_has() {
        let es2019 = Target::parse("es2019").unwrap();
        assert!(es2019.supports(Feature::ObjectSpread) && es2019.supports(Feature::OptionalCatch));
        assert!(!es2019.supports(Feature::OptionalChain) && !es2019.supports(Feature::ClassField));
        assert!(Target::ESNEXT.supports(Feature::ClassStaticBlock));
        assert!(!Target::parse("es2015").unwrap().supports(Feature::Exponent));
    }
}
