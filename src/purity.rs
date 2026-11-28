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
use crate::semantic::{Semantic, SymbolId};
use std::cell::Cell;
use std::collections::HashMap;

/// The modules of the bundle, for a name that is imported: the `const` it stands for, in the module that declares it.
pub struct World<'a> {
    pub graph: &'a crate::graph::Graph,
    pub linked: &'a crate::link::Linked,
    /// By module: the `const` declarations of its top level (see `Purity::consts`).
    pub consts: &'a [HashMap<SymbolId, NodeId>],
}

/// Built-in globals whose mere mention cannot fail: they exist in every JavaScript
/// environment. Any other unresolved name might not be defined, and reading it would throw.
const SAFE_GLOBALS: &[&str] = &[
    "undefined", "NaN", "Infinity", "globalThis", "Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Math", "JSON", "Reflect",
    "Proxy", "Promise", "Map", "Set", "WeakMap", "WeakSet", "WeakRef", "Date", "RegExp", "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError",
    "EvalError", "URIError", "ArrayBuffer", "DataView", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array",
    "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "Intl", "Atomics", "parseInt", "parseFloat", "isNaN", "isFinite",
    "encodeURIComponent", "decodeURIComponent", "encodeURI", "decodeURI",
];

/// `Math` functions that take numbers and give a number without running any other code.
const MATH_FUNCTIONS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil", "clz32", "cos", "cosh", "exp", "expm1", "floor", "fround", "hypot", "imul", "log", "log10",
    "log1p", "log2", "max", "min", "pow", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc",
];
const MATH_CONSTANTS: &[&str] = &["E", "LN10", "LN2", "LOG10E", "LOG2E", "PI", "SQRT1_2", "SQRT2"];

/// Built-in constructors that have a `prototype`.
const PROTOTYPE_HOLDERS: &[&str] = &["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "BigInt", "Promise", "Map", "Set", "WeakMap", "WeakSet", "Date", "RegExp", "Error"];

pub struct Purity<'a> {
    /// The module's text, where the `@__PURE__` comments are.
    pub source: &'a str,
    pub ast: &'a Ast,
    pub semantic: &'a Semantic,
    /// With `--define`: branches that never run do not count.
    pub defines: Option<&'a crate::define::Defines>,
    /// With `--pure`: calls of these names (`styled.div`) are free of effects, as if each carried `/* @__PURE__ */`.
    pub pure_calls: &'a [String],
    /// The function declarations of the module's top level, by the symbol they declare: a call of one whose body only makes a value is free of effects.
    pub functions: Option<&'a HashMap<SymbolId, NodeId>>,
    /// The `const` declarations of the module's top level that name one plain name, by symbol: the initializer. A name whose value is
    /// a number or a string is a primitive, so arithmetic on it runs no code.
    pub consts: Option<&'a HashMap<SymbolId, NodeId>>,
    /// The bundle, so that an imported `const` can be followed to its value; with the module this is the analysis of.
    pub world: Option<(&'a World<'a>, crate::graph::ModuleId)>,
    /// How deep the check of a function body is (a body that calls functions checks those too).
    pub depth: Cell<u8>,
}

/// How many functions deep a call is followed.
const CALL_DEPTH: u8 = 4;

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
                    !dead.is_some_and(|d| crate::compress::var_escapes(ast, self.semantic, d)) && live.is_none_or(|l| self.statement_is_pure(l))
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
                // Spreading runs the iterator protocol; of an array that is written out in the module and never changed, that is the array's own.
                NodeKind::Spread(arg) => self.is_constant_array(arg),
                _ => self.expr_is_pure(e),
            }),
            NodeKind::Object(props) => ast.list(props).iter().all(|&p| {
                // `{ ...a }` copies the values of the properties of `a`: free of effects when `a` is an object of the module made of plain values.
                if let NodeKind::Spread(arg) = ast[p].kind {
                    return self.is_constant_object(arg);
                }
                let NodeKind::Property { key, value, kind } = ast[p].kind else { return false };
                let computed = ast[p].has(flags::COMPUTED);
                (!computed || self.is_primitive(key)) && (kind != crate::ast::PropKind::Init || self.expr_is_pure(value))
            }),
            NodeKind::Class { superclass, members, .. } if ast.decorators_of(id).is_empty() => {
                superclass.is_none_or(|s| matches!(ast[s].kind, NodeKind::Ident(_)) && self.expr_is_pure(s)) && ast.list(members).iter().all(|&m| self.member_is_pure(m))
            }
            NodeKind::Unary { op, arg } => match op {
                UnaryOp::Not | UnaryOp::Typeof | UnaryOp::Void => self.expr_is_pure(arg) || (op == UnaryOp::Typeof && matches!(ast[arg].kind, NodeKind::Ident(_))),
                UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => self.is_primitive(arg),
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
            NodeKind::Call { args, .. } | NodeKind::New { args, .. } => (self.is_marked_pure(id) && ast.list(args).iter().all(|&a| self.expr_is_pure(a))) || self.is_known_pure_call(id) || self.is_pure_function_call(id) || (matches!(ast[id].kind, NodeKind::Call { .. }) && self.is_primitive(id)),
            // `Object.keys`, `Array.prototype.indexOf`: reading a member of a built-in that is there in every environment.
            NodeKind::Member { .. } => self.is_builtin_member(id),
            // A tagged template of a name given to --pure (`styled.div`...``): free of effects when what is in it is too.
            NodeKind::TaggedTemplate { tag, quasi } => {
                !self.pure_calls.is_empty()
                    && self.callee_name(tag).is_some_and(|n| self.pure_calls.iter().any(|p| *p == n))
                    && matches!(ast[quasi].kind, NodeKind::Template { exprs, .. } if ast.list(exprs).iter().all(|&e| self.expr_is_pure(e)))
            }
            _ => false,
        }
    }

    /// `f(x)` where `f` is a function declared at the top of this module (and never assigned) whose body only makes a value: it returns, or
    /// declares names with initializers that only make values. Nothing else runs when the arguments are free of effects, so the call can go
    /// when its result is not used. A function that is async or a generator, or has parameters that are not plain names, is not followed.
    fn is_pure_function_call(&self, id: NodeId) -> bool {
        let ast = self.ast;
        let Some(functions) = self.functions else { return false };
        let NodeKind::Call { callee, args } = ast[id].kind else { return false };
        if ast[id].has(flags::OPTIONAL) || !matches!(ast[callee].kind, NodeKind::Ident(_)) || ast[callee].has(flags::PARENTHESIZED) {
            return false;
        }
        let Some(sym) = self.semantic.symbol_of(callee) else { return false };
        let Some(&function) = functions.get(&sym) else { return false };
        let symbol = self.semantic.symbol(sym);
        if symbol.writes != 0 || symbol.flags & crate::semantic::symbol_flags::DYNAMIC != 0 {
            return false;
        }
        if self.depth.get() >= CALL_DEPTH {
            return false;
        }
        if !ast.list(args).iter().all(|&a| !matches!(ast[a].kind, NodeKind::Spread(_)) && self.expr_is_pure(a)) {
            return false;
        }
        self.depth.set(self.depth.get() + 1);
        let pure = self.function_makes_a_value(function);
        self.depth.set(self.depth.get() - 1);
        pure
    }

    fn function_makes_a_value(&self, function: NodeId) -> bool {
        let ast = self.ast;
        let NodeKind::Function { params, body, .. } = ast[function].kind else { return false };
        if ast[function].has(flags::ASYNC) || ast[function].has(flags::GENERATOR) {
            return false;
        }
        if !ast.list(params).iter().all(|&p| matches!(ast[p].kind, NodeKind::BindingIdent(_))) {
            return false;
        }
        let NodeKind::Block(list) = ast[body].kind else { return false };
        ast.list(list).iter().all(|&s| match ast[s].kind {
            NodeKind::Return(value) => value.is_none_or(|v| self.expr_is_pure(v)),
            NodeKind::Function { .. } => true,
            NodeKind::VarDecl { decls, .. } => ast.list(decls).iter().all(|&d| {
                let NodeKind::VarDeclarator { target, init } = ast[d].kind else { return false };
                matches!(ast[target].kind, NodeKind::BindingIdent(_)) && init.is_none_or(|i| self.expr_is_pure(i))
            }),
            _ => false,
        })
    }

    /// A call of the language itself that only makes a value: `Symbol("name")`, and `new Map`, `new Set`, `new WeakMap` or `new WeakSet` with no
    /// argument or with an array of values written out.
    fn is_known_pure_call(&self, id: NodeId) -> bool {
        let ast = self.ast;
        let (callee, args, is_new) = match ast[id].kind {
            NodeKind::Call { callee, args } => (callee, args, false),
            NodeKind::New { callee, args } => (callee, args, true),
            _ => return false,
        };
        if let NodeKind::Member { object, prop } = ast[callee].kind {
            // `Object.create(null)`: an object with no prototype, which only makes a value.
            let is_create = !is_new
                && !ast[callee].has(flags::COMPUTED)
                && !ast[callee].has(flags::OPTIONAL)
                && !ast[id].has(flags::OPTIONAL)
                && matches!(ast[prop].kind, NodeKind::PropName(p) if ast.atom(p) == "create")
                && matches!(ast[object].kind, NodeKind::Ident(o) if ast.atom(o) == "Object" && self.semantic.symbol_of(object).is_none());
            return is_create && matches!(ast.list(args), [only] if matches!(ast[*only].kind, NodeKind::Null));
        }
        let NodeKind::Ident(name) = ast[callee].kind else { return false };
        if self.semantic.symbol_of(callee).is_some() || ast[id].has(flags::OPTIONAL) {
            return false;
        }
        let args = ast.list(args);
        match (ast.atom(name), is_new) {
            // `Symbol(x)` reads `x` as a string, which can run code unless it is a primitive.
            ("Symbol", false) => args.len() <= 1 && args.iter().all(|&a| self.is_primitive(a)),
            ("Map" | "WeakMap", true) => match args {
                [] => true,
                [list] => match ast[*list].kind {
                    NodeKind::Array(items) => ast.list(items).iter().all(|&e| matches!(ast[e].kind, NodeKind::Array(_)) && self.expr_is_pure(e)),
                    _ => false,
                },
                _ => false,
            },
            ("Set" | "WeakSet", true) => match args {
                [] => true,
                [list] => match ast[*list].kind {
                    NodeKind::Array(items) => ast.list(items).iter().all(|&e| !matches!(ast[e].kind, NodeKind::Spread(_) | NodeKind::Hole) && self.expr_is_pure(e)),
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        }
    }

    /// `Object.keys` (a member of a global that is always there), or `Array.prototype.indexOf` (a member of the prototype of a built-in constructor): reading
    /// either cannot throw, because everything before the last name is certain to exist.
    fn is_builtin_member(&self, id: NodeId) -> bool {
        let ast = self.ast;
        let plain = |n: NodeId| -> Option<&str> {
            match ast[n].kind {
                NodeKind::Member { prop, .. } if !ast[n].has(flags::COMPUTED) && !ast[n].has(flags::OPTIONAL) => match ast[prop].kind {
                    NodeKind::PropName(p) => Some(ast.atom(p)),
                    _ => None,
                },
                _ => None,
            }
        };
        let root_of = |n: NodeId| -> Option<&str> {
            match ast[n].kind {
                NodeKind::Ident(a) if self.semantic.symbol_of(n).is_none() => Some(ast.atom(a)),
                _ => None,
            }
        };
        let NodeKind::Member { object, .. } = ast[id].kind else { return false };
        if plain(id).is_none() {
            return false;
        }
        if let Some(root) = root_of(object) {
            return root != "undefined" && SAFE_GLOBALS.contains(&root);
        }
        // `X.prototype.name`: `X.prototype` is there for the built-in constructors.
        if let NodeKind::Member { object: inner, .. } = ast[object].kind {
            if plain(object) == Some("prototype") {
                return root_of(inner).is_some_and(|r| PROTOTYPE_HOLDERS.contains(&r));
            }
        }
        // `X.prototype`.
        plain(id) == Some("prototype") && root_of(object).is_some_and(|r| PROTOTYPE_HOLDERS.contains(&r))
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

    /// `name` of `Math.name` (the global `Math`, a plain property read).
    fn math_member(&self, member: NodeId) -> Option<&str> {
        let ast = self.ast;
        let NodeKind::Member { object, prop } = ast[member].kind else { return None };
        if ast[member].has(flags::COMPUTED) || ast[member].has(flags::OPTIONAL) {
            return None;
        }
        let (NodeKind::Ident(o), NodeKind::PropName(p)) = (ast[object].kind, ast[prop].kind) else { return None };
        (ast.atom(o) == "Math" && self.semantic.symbol_of(object).is_none()).then(|| ast.atom(p))
    }

    /// A value that is certainly a number, string, boolean, null or similar, known without
    /// running anything, so no `valueOf`/`toString` can be invoked on it.
    /// The initializer of the `const` that the name `sym` of this module stands for, and the analysis of the module that has it: this module for a
    /// name it declares, another for a name it imports.
    fn with_const<R>(&self, sym: SymbolId, f: impl FnOnce(&Purity, NodeId) -> R) -> Option<R> {
        if let Some(&init) = self.consts.and_then(|c| c.get(&sym)) {
            return Some(f(self, init));
        }
        let (world, module) = self.world?;
        let crate::link::Binding::Symbol { module: other, symbol } = world.linked.binding(module, sym)? else { return None };
        let parsed = world.graph.module(*other).parsed.as_ref()?;
        let init = *world.consts.get(other.index())?.get(symbol)?;
        let there = Purity { source: &world.graph.module(*other).source, ast: &parsed.ast, semantic: &parsed.semantic, defines: self.defines, pure_calls: self.pure_calls, functions: None, consts: Some(&world.consts[other.index()]), world: Some((world, *other)), depth: Cell::new(self.depth.get()) };
        Some(f(&there, init))
    }

    /// A name that nothing assigns, declared with `const` at the top of the module to an object literal of plain values: no accessor (copying it
    /// would run the getter) and no key that is computed from something that could run code.
    fn is_constant_object(&self, id: NodeId) -> bool {
        let NodeKind::Ident(_) = self.ast[id].kind else { return false };
        let Some(sym) = self.semantic.symbol_of(id) else { return false };
        if self.depth.get() >= CALL_DEPTH {
            return false;
        }
        self.with_const(sym, |at, init| {
            let NodeKind::Object(props) = at.ast[init].kind else { return false };
            at.depth.set(at.depth.get() + 1);
            let plain = at.ast.list(props).iter().all(|&p| match at.ast[p].kind {
                NodeKind::Spread(arg) => at.is_constant_object(arg),
                NodeKind::Property { key, value, kind } => (!at.ast[p].has(flags::COMPUTED) || at.is_primitive(key)) && kind == crate::ast::PropKind::Init && at.expr_is_pure(value),
                _ => false,
            });
            at.depth.set(at.depth.get() - 1);
            plain
        })
        .unwrap_or(false)
    }

    /// A name that nothing assigns, declared with `const` at the top of the module to an array literal of values with no effects (and no
    /// holes): reading it through `...` or `.includes` calls only the array's own iterator.
    fn is_constant_array(&self, id: NodeId) -> bool {
        let ast = self.ast;
        let NodeKind::Ident(_) = ast[id].kind else { return false };
        let Some(sym) = self.semantic.symbol_of(id) else { return false };
        if self.depth.get() >= CALL_DEPTH {
            return false;
        }
        self.with_const(sym, |at, init| {
            let NodeKind::Array(items) = at.ast[init].kind else { return false };
            at.depth.set(at.depth.get() + 1);
            let pure = at.ast.list(items).iter().all(|&e| match at.ast[e].kind {
                NodeKind::Hole => false,
                NodeKind::Spread(arg) => at.is_constant_array(arg),
                _ => at.expr_is_pure(e),
            });
            at.depth.set(at.depth.get() - 1);
            pure
        })
        .unwrap_or(false)
    }

    fn is_primitive(&self, id: NodeId) -> bool {
        let ast = self.ast;
        match ast[id].kind {
            NodeKind::Num(_) | NodeKind::Str(_) | NodeKind::Bool(_) | NodeKind::Null => true,
            NodeKind::Template { exprs, .. } => ast.list(exprs).iter().all(|&e| self.is_primitive(e)),
            NodeKind::Unary { op: UnaryOp::Not | UnaryOp::Typeof | UnaryOp::Void, arg } => self.expr_is_pure(arg),
            NodeKind::Unary { op: UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot, arg } => self.is_primitive(arg),
            NodeKind::Binary { op, left, right } => match op {
                // Walked along the left side in a loop: a sum of 50,000 terms is a tree that deep, and this runs on a worker's small stack.
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Exp | BinOp::BitOr | BinOp::BitAnd | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::UShr => {
                    if !self.is_primitive(right) {
                        return false;
                    }
                    let mut at = left;
                    while let NodeKind::Binary { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::Exp | BinOp::BitOr | BinOp::BitAnd | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::UShr, left, right } = ast[at].kind {
                        if !self.is_primitive(right) {
                            return false;
                        }
                        at = left;
                    }
                    self.is_primitive(at)
                }
                BinOp::StrictEq | BinOp::StrictNe => self.expr_is_pure(left) && self.expr_is_pure(right),
                _ => false,
            },
            // A name that nothing assigns, declared with `const` at the top of the module to a primitive.
            NodeKind::Ident(name) => match self.semantic.symbol_of(id) {
                Some(sym) => self
                    .with_const(sym, |at, init| {
                        at.depth.get() < CALL_DEPTH && {
                            at.depth.set(at.depth.get() + 1);
                            let primitive = at.is_primitive(init);
                            at.depth.set(at.depth.get() - 1);
                            primitive
                        }
                    })
                    .unwrap_or(false),
                None => matches!(ast.atom(name), "Infinity" | "NaN" | "undefined"),
            },
            // `Math.pow(2, 10)` and `Math.PI`: the global `Math` is not shadowed, and its numeric functions run no code on primitives.
            NodeKind::Call { callee, args } => self.math_member(callee).is_some_and(|n| MATH_FUNCTIONS.contains(&n)) && !ast[id].has(flags::OPTIONAL) && ast.list(args).iter().all(|&a| !matches!(ast[a].kind, NodeKind::Spread(_)) && self.is_primitive(a)),
            NodeKind::Member { .. } => self.math_member(id).is_some_and(|n| MATH_CONSTANTS.contains(&n)),
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
        let analysis = Purity { source: src, ast: &parsed.ast, semantic: &parsed.semantic, defines: None, pure_calls: &[], functions: None, consts: None, world: None, depth: Cell::new(0) };
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
    fn bitwise_arithmetic_on_primitives_is_pure() {
        for src in ["const a = ~(1 | 2 | 4), b = 1 << 3, c = (5 & 3) ^ 1, d = -1 >>> 1, e = 8 >> 1;"] {
            assert!(pure(src), "{src}");
        }
        for src in ["const a = x | 1;", "const b = f() << 1;"] {
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
            "Symbol('name');",
            "Symbol();",
            "const s = Symbol(`x`);",
            "new Map; new Set; new WeakMap; new WeakSet;",
            "const m = new Map([[1, 2], ['a', [3]]]), t = new Set(['a', 'b', 1]);",
            "Object.keys; Array.prototype.indexOf; Math.PI; String.prototype;",
            "Object.create(null);",
            "const a = /* @__PURE__ */ Object.assign(Object.create(null), { x: 1 });",
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
            "Symbol(obj); var obj;",
            "Symbol(...a); var a;",
            "Symbol('x'); function Symbol() {}",
            "new Set(items); var items;",
            "new Set([...items]); var items;",
            "new Set([f()]);",
            "new Map([1]);",
            "new Map(entries); var entries;",
            "new Set('abc');",
            "Object.a.b;",
            "undefined.x;",
            "Math.prototype.x;",
            "foo.prototype.x;",
            "Object[key]; var key;",
            "Object?.keys;",
            "Object.create(proto); var proto;",
            "Object.create(null, props); var props;",
            "Object.create(null) + 1;",
            "Object.create(undefined);",
            "Object.create(null); function Object() {}",
            "Object['create'](null);",
            "Object.assign(Object.create(null), { x: 1 });",
        ] {
            assert!(!pure(src), "should have effects: {src}");
        }
    }
}
