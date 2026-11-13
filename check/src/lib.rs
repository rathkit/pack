//! The optional checker for Rathkit Pack. Stage 1: the scanner diagnostics of TypeScript, with TypeScript's codes and positions.
//!
//! What this is not: a type checker. A file that comes back with no diagnostics is a file in which this stage found nothing among the
//! few codes in [`scanner::CODES`]; it says nothing about syntax in general, names, declarations or types. See docs/typechecker.md.

pub mod scanner;

pub use scanner::{CODES, Diagnostic, FileKind, Report, Stop, check_source};

/// The sentence every clean run ends with. It is a contract (a test pins it): a run with no diagnostics must not be readable as
/// "the program is type-correct".
pub const NOT_A_TYPE_CHECK: &str = "This is stage 1 of the checker: it reports only the scanner errors listed by `rkp-check --codes`. It did not check syntax beyond that, names, declarations or types, so no diagnostics does not mean the code is correct.";

/// The version of TypeScript whose diagnostics this checker follows.
pub const TRACKED_TYPESCRIPT: &str = "5.9.3";
