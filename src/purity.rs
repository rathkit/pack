//! Side-effect analysis: can evaluating this code change anything observable?
//!
//! Tree-shaking may drop a top-level statement only if running it does nothing but define
//! values. The answer errs toward "has effects": a call, a property read (it may run a getter
//! or throw on `undefined`), an assignment, anything not understood. Wrongly keeping code costs
//! bytes; wrongly dropping it breaks the program.
//!
//! `/* @__PURE__ */` (or `#__PURE__`) in front of a call or `new` says the author guarantees it does
//! nothing but make a value, so it is droppable when its arguments are. A known gap, on the safe
//! side: a read of a module-level binding is assumed to be free of the temporal dead zone error.

use crate::ast::{Ast, BinOp, NodeId, NodeKind, UnaryOp, flags};
use crate::semantic::Semantic;

/// Built-in globals whose mere mention cannot fail: they exist in every JavaScript
/// environment. Any other unresolved name might not be defined, and reading it would throw.
const SAFE_GLOBALS: &[&str] = &[
    "undefined", "NaN", "Infinity", "globalThis", "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Math", "JSON", "Reflect",
    "Proxy", "Promise", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "Date", "RegExp", "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError",
    "EvalError", "URIError", "ArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array",
    "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Intl", "Atomics", "parseInt", "parseFloat", "isNaN", "isFinite",
    "encodeURIComponent", "decodeURIComponent", "encodeURI", "decodeURI",
];

pub struct Purity<'a> {
    /// The module's text, where the `@__PURE__` comments are.
    pub source: &'a str,
    pub ast: &'a Ast,
    pub semantic: &'a Semantic,
    /// With `--define`: branches that never run do not count.
    pub defines: Option<&'a crate::define::Defines>,
    /// With `--pure`: calls of these names (`styled.div`) are free of effects, as if each carried `/* @__PURE__ */`.
    pub pure_calls: &'a [String],
}

impl Purity<'_> {
    /// Whether a top-level statement can be dropped when nothing it defines is used.
    pub fn statement_is_pure(&self, id: NodeId) -> bool {
        let ast = self.ast;
        match ast[id].kind {
            NodeKind::Empty | NodeKind::Function { .. } => true,
            // Only the branch that runs matters, and a block of pure statements is pure.
            NodeKind::If { test, cons, alt } => match self.defines.and_then(|d| d.eval(ast, self.semantic, test)) {
                Some(value) => {
                    let (live, dead) = if value.truthy() { (Some(cons), alt) } else { (alt, Some(cons)) };
                    // A `var` in the branch that never runs is still declared.
                    !dead.is_some_and(|d| crate::compress::declares_var(ast, d)) && live.is_none_or(|l| self.statement_is_pure(l))
                }
                None => false,
            },
            NodeKind::Block(list) => self.defines.is_some() && ast.list(list).iter().all(|&s| self.statement_is_pure(s)),
            NodeKind::Class { .. } => self.expr_is_pure(id),
            NodeKind::ExprStmt(expr) => self.expr_is_pure(expr),
            NodeKind::VarDecl { decls, .. } => ast.list(decls).iter().all(|&d| {
                let NodeKind::VarDeclarator { target, init } = ast[d].kind else { return false };
                // Destructuring reads properties and runs iterators, so only plain names pass.
                matches!(ast[target].kind, NodeKind::BindingIdent(_)) && init.is_none_or(|i| self.expr_is_pure(i))
            }),
            NodeKind::ExportNamed(export) => ast.export(export).decl.is_none_or(|d| self.statement_is_pure(d)),
            NodeKind::ExportDefault(value) => match ast[value].kind {
                NodeKind::Function { .. } => true,
                _ => self.expr_is_pure(value),
            },
            NodeKind::ExportAll { .. } | NodeKind::Import(_) => true,
            // An enum only fills in an object of its own.
            NodeKind::TsEnum { members, .. } => ast.list(members).iter().all(|&m| match ast[m].kind {
                NodeKind::TsEnumMember { init, .. } => init.is_none_or(|i| self.expr_is_pure(i)),
                _ => false,
            }),
            NodeKind::TsNamespace { body, .. } => ast.list(body).iter().all(|&b| self.statement_is_pure(b)),
            NodeKind::TsExport(decl) => self.statement_is_pure(decl),
            _ => false,
        }
    }

    /// The branch of `id` that never runs, when definitions say so.
    fn dead(&self, id: NodeId) -> Option<NodeId> {
        self.defines?.dead_branch(self.ast, self.semantic, id).map(|d| d.node)
    }

    /// Whether evaluating the expression can have no effect beyond producing a value.
    pub fn expr_is_pure(&self, id: NodeId) -> bool {
        let ast = self.ast;
        if let Some(defines) = self.defines {
            // A defined expression is its value.
            if matches!(ast[id].kind, NodeKind::Ident(_) | NodeKind::Member { .. } | NodeKind::ImportMeta) {
                if let Some(define) = defines.lookup(ast, self.semantic, id) {
                    return define.value.is_some();
                }
            }
            if let Some(dead) = self.dead(id) {
                return match ast[id].kind {
                    NodeKind::Conditional { test, cons, alt } => self.expr_is_pure(test) && self.expr_is_pure(if dead == cons { alt } else { cons }),
                    NodeKind::Binary { left, .. } => self.expr_is_pure(left),
                    _ => false,
                };
            }
        }
        match ast[id].kind {
            NodeKind::Num(_) | NodeKind::Str(_) | NodeKind::Bool(_) | NodeKind::Null | NodeKind::BigInt(_) | NodeKind::Regex | NodeKind::This => true,
            NodeKind::Function { .. } => true,
            NodeKind::Ident(name) => self.semantic.symbol_of(id).is_some() || SAFE_GLOBALS.contains(&ast.atom(name)),
            NodeKind::Template { exprs, .. } => ast.list(exprs).iter().all(|&e| self.is_primitive(e)),
            NodeKind::Array(items) => ast.list(items).iter().all(|&e| match ast[e].kind {
                NodeKind::Hole => true,
                // Spreading runs the iterator protocol.
                NodeKind::Spread(_) => false,
                _ => self.expr_is_pure(e),
            }),
            NodeKind::Object(props) => ast.list(props).iter().all(|&p| {
                let NodeKind::Property { key, value, kind } = ast[p].kind else { return false };
                let computed = ast[p].has(flags::COMPUTED);
                (!computed || self.is_primitive(key)) && (kind != crate::ast::PropKind::Init || self.expr_is_pure(value))
            }),
            NodeKind::Class { superclass, members, .. } if ast.decorators_of(id).is_empty() => {
                superclass.is_none_or(|s| matches!(ast[s].kind, NodeKind::Ident(_)) && self.expr_is_pure(s)) && ast.list(members).iter().all(|&m| self.member_is_pure(m))
            }
            NodeKind::Unary { op, arg } => match op {
                UnaryOp::Not | UnaryOp::Typeof | UnaryOp::Void => self.expr_is_pure(arg) || (op == UnaryOp::Typeof && matches!(ast[arg].kind, NodeKind::Ident(_))),
                UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => matches!(ast[arg].kind, NodeKind::Num(_)),
                UnaryOp::Delete => false,
            },
            NodeKind::Binary { op, left, right } => match op {
                BinOp::StrictEq | BinOp::StrictNe | BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish => self.expr_is_pure(left) && self.expr_is_pure(right),
                BinOp::In | BinOp::InstanceOf => false,
                // Other operators call `valueOf`/`toString` on objects, so both sides must be
                // plain numbers or strings.
                _ => self.is_primitive(left) && self.is_primitive(right),
            },
            NodeKind::Conditional { test, cons, alt } => self.expr_is_pure(test) && self.expr_is_pure(cons) && self.expr_is_pure(alt),
            NodeKind::Sequence(items) => ast.list(items).iter().all(|&e| self.expr_is_pure(e)),
            // A call the author marked as free of effects: only evaluating its arguments can matter.
            NodeKind::Call { args, .. } | NodeKind::New { args, .. } => self.is_marked_pure(id) && ast.list(args).iter().all(|&a| self.expr_is_pure(a)),
            // A tagged template of a name given to --pure (`styled.div`...``): free of effects when what is in it is too.
            NodeKind::TaggedTemplate { tag, quasi } => {
                !self.pure_calls.is_empty()
                    && self.callee_name(tag).is_some_and(|n| self.pure_calls.iter().any(|p| *p == n))
                    && matches!(ast[quasi].kind, NodeKind::Template { exprs, .. } if ast.list(exprs).iter().all(|&e| self.expr_is_pure(e)))
            }
            _ => false,
        }
    }

    /// Whether a `/* @__PURE__ */` or `/* #__PURE__ */` comment comes right before the expression
    /// (with only blanks and opening parentheses between).
    fn is_marked_pure(&self, id: NodeId) -> bool {
        if !self.pure_calls.is_empty() {
            if let NodeKind::Call { callee, .. } | NodeKind::New { callee, .. } = self.ast[id].kind {
                if let Some(name) = self.callee_name(callee) {
                    if self.pure_calls.iter().any(|p| *p == name) {
                        return true;
                    }
                }
            }
        }
        let start = self.ast[id].span.start as usize;
        let before = self.source.get(..start).unwrap_or("").trim_end_matches(|c: char| c.is_whitespace() || c == '(');
        let Some(body) = before.strip_suffix("*/") else { return false };
        let Some(open) = body.rfind("/*") else { return false };
        matches!(body[open + 2..].trim(), "@__PURE__" | "#__PURE__")
    }

    /// `a.b.c` for a callee that is a name or a chain of property reads on one.
    fn callee_name(&self, callee: NodeId) -> Option<String> {
        let ast = self.ast;
        match ast[callee].kind {
            NodeKind::Ident(n) => Some(ast.atom(n).to_string()),
            NodeKind::Member { object, prop } if !ast[callee].has(flags::COMPUTED) && !ast[callee].has(flags::OPTIONAL) => match ast[prop].kind {
                NodeKind::PropName(p) => Some(format!("{}.{}", self.callee_name(object)?, ast.atom(p))),
                _ => None,
            },
            _ => None,
        }
    }

    fn member_is_pure(&self, member: NodeId) -> bool {
        let ast = self.ast;
        let computed_key_ok = |key: NodeId| !ast[member].has(flags::COMPUTED) || self.is_primitive(key);
        if !ast.decorators_of(member).is_empty() {
            return false;
        }
        match ast[member].kind {
            NodeKind::ClassMethod { key, .. } => computed_key_ok(key),
            // An instance field's value runs per construction; a static one runs now.
            NodeKind::ClassField { key, value } => {
                computed_key_ok(key) && (!ast[member].has(flags::STATIC) || value.is_none_or(|v| self.expr_is_pure(v)))
            }
            _ => false,
        }
    }

    /// A value that is certainly a number, string, boolean, null or similar, known without
    /// running anything, so no `valueOf`/`toString` can be invoked on it.
    fn is_primitive(&self, id: NodeId) -> bool {
        let ast = self.ast;
        match ast[id].kind {
            NodeKind::Num(_) | NodeKind::Str(_) | NodeKind::Bool(_) | NodeKind::Null => true,
            NodeKind::Template { exprs, .. } => ast.list(exprs).iter().all(|&e| self.is_primitive(e)),
            NodeKind::Unary { op: UnaryOp::Not | UnaryOp::Typeof | UnaryOp::Void, arg } => self.expr_is_pure(arg),
            NodeKind::Unary { op: UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot, arg } => matches!(ast[arg].kind, NodeKind::Num(_)),
            NodeKind::Binary { op, left, right } => match op {
                BinOp::Add => {
                    let literal = |n: NodeId| matches!(ast[n].kind, NodeKind::Num(_) | NodeKind::Str(_));
                    literal(left) && literal(right)
                }
                BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Exp => {
                    matches!(ast[left].kind, NodeKind::Num(_)) && matches!(ast[right].kind, NodeKind::Num(_))
                }
                BinOp::StrictEq | BinOp::StrictNe => self.expr_is_pure(left) && self.expr_is_pure(right),
                _ => false,
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ParseOptions, parse};

    /// Whether the first statement of `src` is droppable.
    fn pure(src: &str) -> bool {
        let parsed = parse(src, ParseOptions::new(true)).unwrap_or_else(|e| panic!("{}: {src}", e.message));
        let NodeKind::Program(body) = parsed.ast[parsed.ast.root].kind else { unreachable!() };
        let analysis = Purity { source: src, ast: &parsed.ast, semantic: &parsed.semantic, defines: None, pure_calls: &[] };
        analysis.statement_is_pure(parsed.ast.list(body)[0])
    }

    #[test]
    fn marked_calls_are_pure_when_their_arguments_are() {
        for src in ["const a = /* @__PURE__ */ make(1, 'x');", "const b = /*#__PURE__*/ new Thing([1, 2]);", "const c = /*#__PURE__*/(0, make)(1);", "export default /* @__PURE__ */ factory();"] {
            assert!(pure(src), "{src}");
        }
        for src in ["const a = make(1);", "const b = /* @__PURE__ */ make(effect());", "const c = /* not pure */ make();", "const d = /* @__PURE__ */ make(...args);"] {
            assert!(!pure(src), "{src}");
        }
    }

    #[test]
    fn declarations_and_literals_are_pure() {
        for src in [
            "function f() { sideEffect(); }",
            "const a = 1, b = 'x', c = [1, , 2], d = { k: 1, m() {}, get g() { return f(); } };",
            "var e = function () { return f(); }, g = () => f(), h = /re/g, i = null;",
            "const j = typeof missing, k = void 0, l = !0, m = -1, n = 1 + 2, o = 'a' + 'b';",
            "const p = Object, q = Math, r = undefined, s = globalThis;",
            "const t = 1 === 2 || 3, u = a ? 1 : 2; var a;",
            "'use strict';",
            "class A { m() {} static s = 1; x = f(); ['lit'] = 1; }",
            "class B extends Object { constructor() { super(); } }",
            "export const x = 1;",
            "export function f() { f(); }",
            "export default function () { f(); }",
            "export default 1 + 2;",
            "export { a } from 'x';",
            "export * from 'x';",
            "import a from 'x';",
            ";",
        ] {
            assert!(pure(src), "should be pure: {src}");
        }
    }

    #[test]
    fn anything_that_runs_code_is_not() {
        for src in [
            "f();",
            "new Foo();",
            "const a = f();",
            "const a = x.y;",
            "const a = x[0];",
            "const { a } = {};",
            "const [a] = [];",
            "const a = [...b], b = [];",
            "const a = { ...b }, b = {};",
            "const a = { [f()]: 1 };",
            "const a = { [obj]: 1 }, obj = {};",
            "const a = undeclared;",
            "const a = `${obj}`, obj = {};",
            "const a = +obj, obj = {};",
            "const a = obj + 1, obj = {};",
            "const a = 'k' in obj, obj = {};",
            "const a = obj instanceof Object, obj = {};",
            "const a = delete obj.x, obj = {};",
            "x = 1;",
            "x++;",
            "if (x) {} var x;",
            "for (;;) {}",
            "throw 1;",
            "class A extends f() {}",
            "class A { static { f(); } }",
            "class A { static x = f(); }",
            "class A { [f()]() {} }",
            "class A extends (f(), B) {} var B;",
            "label: {}",
            "await 1;",
            "export default f();",
            "export const x = f();",
            "export default class extends f() {}",
        ] {
            assert!(!pure(src), "should have effects: {src}");
        }
    }
}
