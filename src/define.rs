//! `--define`: global expressions replaced by constants (`process.env.NODE_ENV` by
//! `"production"`), and the folding of constants that tells which branch of an `if`, a `?:` or
//! a `&&` can never run. Code behind a branch that never runs is left out of the bundle, and
//! so are the modules only it loads.

use crate::ast::{flags, Ast, BinOp, NodeId, NodeKind, UnaryOp};
use crate::semantic::Semantic;

/// A value known while bundling.
#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl Const {
    pub fn truthy(&self) -> bool {
        match self {
            Const::Undefined | Const::Null => false,
            Const::Bool(b) => *b,
            Const::Num(n) => *n != 0.0 && !n.is_nan(),
            Const::Str(s) => !s.is_empty(),
        }
    }

    fn nullish(&self) -> bool {
        matches!(self, Const::Undefined | Const::Null)
    }

    fn type_name(&self) -> &'static str {
        match self {
            Const::Undefined => "undefined",
            Const::Null => "object",
            Const::Bool(_) => "boolean",
            Const::Num(_) => "number",
            Const::Str(_) => "string",
        }
    }

    /// `===`. Numbers compare as numbers, so NaN differs from itself.
    fn strictly_equals(&self, other: &Const) -> bool {
        match (self, other) {
            (Const::Num(a), Const::Num(b)) => a == b,
            (a, b) => a == b,
        }
    }

    /// `==`, where the answer is plain; `None` where the types differ in a way that needs
    /// JavaScript's conversions.
    fn loosely_equals(&self, other: &Const) -> Option<bool> {
        if std::mem::discriminant(self) == std::mem::discriminant(other) {
            return Some(self.strictly_equals(other));
        }
        match (self.nullish(), other.nullish()) {
            (true, true) => Some(true),
            (true, false) | (false, true) => Some(false),
            _ => None,
        }
    }
}

/// One definition: the expression `path` stands for `text`.
#[derive(Clone, Debug)]
pub struct Define {
    path: Vec<String>,
    /// The replacement, as source.
    pub text: String,
    /// The replacement is not a single token or name, so it goes in parentheses.
    pub wrap: bool,
    /// What the replacement is, when it is a constant.
    pub value: Option<Const>,
}

impl Define {
    /// `key` is a name or a dotted chain (`process.env.NODE_ENV`, `import.meta.env.MODE`);
    /// `value` is a JSON-like constant, another name, or any other expression.
    pub fn parse(key: &str, value: &str) -> Result<Define, String> {
        let mut path: Vec<String> = Vec::new();
        for part in key.trim().split('.') {
            if !is_name(part) {
                return Err(format!("'{key}' is not a name or a dotted chain of names"));
            }
            path.push(part.to_string());
        }
        // `import.meta` is one root.
        if path.len() >= 2 && path[0] == "import" && path[1] == "meta" {
            path.splice(0..2, ["import.meta".to_string()]);
        } else if path[0] == "import" {
            return Err(format!("'{key}' is not a name or a dotted chain of names"));
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("'{key}' needs a value"));
        }
        let (text, wrap, constant) = match value {
            "true" => ("true".to_string(), false, Some(Const::Bool(true))),
            "false" => ("false".to_string(), false, Some(Const::Bool(false))),
            "null" => ("null".to_string(), false, Some(Const::Null)),
            "undefined" => ("void 0".to_string(), true, Some(Const::Undefined)),
            _ => {
                let bytes = value.as_bytes();
                let quoted = bytes.len() >= 2 && matches!(bytes[0], b'"' | b'\'') && bytes[bytes.len() - 1] == bytes[0];
                if quoted {
                    match crate::literal::cook_string(&value[1..value.len() - 1], false) {
                        Ok(s) => (quote(&s), false, Some(Const::Str(s))),
                        Err(e) => return Err(format!("the value of '{key}': {e}")),
                    }
                } else if let Some(n) = value.parse::<f64>().ok().filter(|n| n.is_finite()) {
                    (value.to_string(), true, Some(Const::Num(n)))
                } else {
                    (value.to_string(), !value.split('.').all(is_name), None)
                }
            }
        };
        Ok(Define { path, text, wrap, value: constant })
    }

    /// The replacement as it is written in place of an expression.
    pub fn replacement(&self) -> String {
        if self.wrap {
            format!("({})", self.text)
        } else {
            self.text.clone()
        }
    }
}

fn is_name(part: &str) -> bool {
    let mut chars = part.chars();
    matches!(chars.next(), Some(c) if c == '_' || c == '$' || c.is_ascii_alphabetic()) && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
}

fn quote(text: &str) -> String {
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

/// The definitions of a build.
#[derive(Clone, Debug, Default)]
pub struct Defines {
    list: Vec<Define>,
}

/// A branch that never runs, and what stands in its place.
#[derive(Clone, Copy, Debug)]
pub struct Dead {
    pub node: NodeId,
    pub replacement: &'static str,
}

impl Defines {
    pub fn add(&mut self, define: Define) {
        // A later definition of the same expression wins.
        self.list.retain(|d| d.path != define.path);
        self.list.push(define);
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The definition that `node` is an instance of: a name nothing declares, or a chain of
    /// property reads on one, written as a definition's key.
    pub fn lookup(&self, ast: &Ast, sem: &Semantic, node: NodeId) -> Option<&Define> {
        if self.list.is_empty() {
            return None;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut at = node;
        loop {
            match ast[at].kind {
                NodeKind::Member { object, prop } => {
                    if ast[at].has(flags::OPTIONAL) {
                        return None;
                    }
                    let name = match ast[prop].kind {
                        NodeKind::PropName(a) if !ast[at].has(flags::COMPUTED) => a,
                        NodeKind::Str(a) if ast[at].has(flags::COMPUTED) => a,
                        _ => return None,
                    };
                    path.push(ast.atom(name));
                    at = object;
                }
                NodeKind::Ident(a) => {
                    if sem.symbol_of(at).is_some() {
                        return None;
                    }
                    path.push(ast.atom(a));
                    break;
                }
                NodeKind::ImportMeta => {
                    path.push("import.meta");
                    break;
                }
                _ => return None,
            }
        }
        self.list.iter().find(|d| d.path.len() == path.len() && d.path.iter().rev().zip(&path).all(|(a, b)| a == b))
    }

    /// The value of `node` when it is a constant: a literal, a defined expression whose value is
    /// one, or an operator on those.
    pub fn eval(&self, ast: &Ast, sem: &Semantic, node: NodeId) -> Option<Const> {
        match ast[node].kind {
            NodeKind::Null => Some(Const::Null),
            NodeKind::Bool(b) => Some(Const::Bool(b)),
            NodeKind::Num(n) => Some(Const::Num(n.get())),
            NodeKind::Str(a) => Some(Const::Str(ast.atom(a).to_string())),
            NodeKind::Ident(a) if ast.atom(a) == "undefined" && sem.symbol_of(node).is_none() && self.lookup(ast, sem, node).is_none() => Some(Const::Undefined),
            NodeKind::Ident(_) | NodeKind::Member { .. } | NodeKind::ImportMeta => self.lookup(ast, sem, node)?.value.clone(),
            NodeKind::Unary { op, arg } => match op {
                UnaryOp::Not => Some(Const::Bool(!self.eval(ast, sem, arg)?.truthy())),
                UnaryOp::Typeof => Some(Const::Str(self.eval(ast, sem, arg)?.type_name().to_string())),
                UnaryOp::Void => self.eval(ast, sem, arg).map(|_| Const::Undefined),
                UnaryOp::Neg => match self.eval(ast, sem, arg)? {
                    Const::Num(n) => Some(Const::Num(-n)),
                    _ => None,
                },
                _ => None,
            },
            NodeKind::Binary { op, left, right } => match op {
                BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish => {
                    let l = self.eval(ast, sem, left)?;
                    let keeps_left = match op {
                        BinOp::LogAnd => !l.truthy(),
                        BinOp::LogOr => l.truthy(),
                        _ => !l.nullish(),
                    };
                    if keeps_left {
                        Some(l)
                    } else {
                        self.eval(ast, sem, right)
                    }
                }
                BinOp::StrictEq | BinOp::StrictNe | BinOp::Eq | BinOp::Ne => {
                    let (l, r) = (self.eval(ast, sem, left)?, self.eval(ast, sem, right)?);
                    let equal = if matches!(op, BinOp::StrictEq | BinOp::StrictNe) { l.strictly_equals(&r) } else { l.loosely_equals(&r)? };
                    Some(Const::Bool(equal == matches!(op, BinOp::StrictEq | BinOp::Eq)))
                }
                _ => None,
            },
            NodeKind::Conditional { test, cons, alt } => {
                let test = self.eval(ast, sem, test)?;
                self.eval(ast, sem, if test.truthy() { cons } else { alt })
            }
            _ => None,
        }
    }

    /// The part of `node` that can never run, when `node` is an `if`, a `?:` or a `&&`, `||`,
    /// `??` whose condition is a constant.
    pub fn dead_branch(&self, ast: &Ast, sem: &Semantic, node: NodeId) -> Option<Dead> {
        match ast[node].kind {
            NodeKind::If { test, cons, alt } => {
                let taken = self.eval(ast, sem, test)?.truthy();
                let dead = if taken { alt? } else { cons };
                // A `var` in it would still be declared.
                if crate::compress::declares_var(ast, dead) {
                    return None;
                }
                Some(Dead { node: dead, replacement: "{}" })
            }
            NodeKind::Conditional { test, cons, alt } => {
                let taken = self.eval(ast, sem, test)?.truthy();
                Some(Dead { node: if taken { alt } else { cons }, replacement: "0" })
            }
            NodeKind::Binary { op: op @ (BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish), left, right } => {
                let l = self.eval(ast, sem, left)?;
                let skips_right = match op {
                    BinOp::LogAnd => !l.truthy(),
                    BinOp::LogOr => l.truthy(),
                    _ => !l.nullish(),
                };
                skips_right.then_some(Dead { node: right, replacement: "0" })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse, ParseOptions};
    use crate::ast::NodeKind;

    fn defines(pairs: &[(&str, &str)]) -> Defines {
        let mut d = Defines::default();
        for (k, v) in pairs {
            d.add(Define::parse(k, v).expect("define"));
        }
        d
    }

    /// The constant value of the program `source`, an expression statement.
    fn value(d: &Defines, source: &str) -> Option<Const> {
        let parsed = parse(source, ParseOptions::new(true)).expect("parse");
        let NodeKind::Program(list) = parsed.ast[parsed.ast.root].kind else { panic!() };
        let stmt = parsed.ast.list(list)[0];
        let NodeKind::ExprStmt(e) = parsed.ast[stmt].kind else { panic!("{:?}", parsed.ast[stmt].kind) };
        d.eval(&parsed.ast, &parsed.semantic, e)
    }

    #[test]
    fn values_are_read_as_constants() {
        let prod = defines(&[("process.env.NODE_ENV", "\"production\""), ("DEBUG", "false"), ("import.meta.env.MODE", "'x'"), ("N", "3")]);
        assert_eq!(value(&prod, "process.env.NODE_ENV"), Some(Const::Str("production".into())));
        assert_eq!(value(&prod, "process.env.NODE_ENV === 'production'"), Some(Const::Bool(true)));
        assert_eq!(value(&prod, "process.env['NODE_ENV'] !== 'production'"), Some(Const::Bool(false)));
        assert_eq!(value(&prod, "!DEBUG"), Some(Const::Bool(true)));
        assert_eq!(value(&prod, "DEBUG || N"), Some(Const::Num(3.0)));
        assert_eq!(value(&prod, "import.meta.env.MODE == 'x'"), Some(Const::Bool(true)));
        assert_eq!(value(&prod, "typeof DEBUG"), Some(Const::Str("boolean".into())));
        assert_eq!(value(&prod, "process.env.OTHER"), None);
        assert_eq!(value(&prod, "process.env.NODE_ENV === other"), None);
        assert_eq!(value(&prod, "N == null"), Some(Const::Bool(false)));
        assert_eq!(value(&prod, "undefined == null"), Some(Const::Bool(true)));
    }

    #[test]
    fn a_declared_name_is_not_the_global() {
        let d = defines(&[("DEBUG", "true")]);
        let parsed = parse("function f(DEBUG) { return DEBUG; } DEBUG;", ParseOptions::new(true)).expect("parse");
        let mut seen = Vec::new();
        for i in 1..=parsed.ast.node_count() {
            let id = parsed.ast.node_id(i);
            if matches!(parsed.ast[id].kind, NodeKind::Ident(_)) {
                seen.push(d.lookup(&parsed.ast, &parsed.semantic, id).is_some());
            }
        }
        assert_eq!(seen, vec![false, true]);
    }

    #[test]
    fn branches_that_never_run_are_found() {
        let d = defines(&[("PROD", "true")]);
        let parsed = parse("if (PROD) a(); else b(); PROD ? c() : d(); PROD && e(); !PROD && f();", ParseOptions::new(true)).expect("parse");
        let mut dead = Vec::new();
        for i in 1..=parsed.ast.node_count() {
            let id = parsed.ast.node_id(i);
            if let Some(x) = d.dead_branch(&parsed.ast, &parsed.semantic, id) {
                let span = parsed.ast[x.node].span;
                dead.push((&"if (PROD) a(); else b(); PROD ? c() : d(); PROD && e(); !PROD && f();"[span.start as usize..span.end as usize], x.replacement));
            }
        }
        dead.sort();
        assert_eq!(dead, vec![("b();", "{}"), ("d()", "0"), ("f()", "0")]);
    }

    #[test]
    fn parsing_values() {
        assert_eq!(Define::parse("a.b", "\"x\\ny\"").unwrap().text, "\"x\\ny\"");
        assert_eq!(Define::parse("a", "undefined").unwrap().replacement(), "(void 0)");
        assert_eq!(Define::parse("a", "other.name").unwrap().replacement(), "other.name");
        assert_eq!(Define::parse("a", "{\"k\":1}").unwrap().replacement(), "({\"k\":1})");
        assert!(Define::parse("a-b", "1").is_err());
        assert!(Define::parse("a", "").is_err());
        assert!(Define::parse("import.x", "1").is_err());
    }
}
