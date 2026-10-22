use super::*;
use crate::parser::{ParseOptions, parse};

fn build(src: &str, module: bool) -> (Ast, Semantic) {
    let parsed = parse(src, ParseOptions::new(module)).unwrap_or_else(|e| panic!("{src:?} failed: {e}"));
    (parsed.ast, parsed.semantic)
}

fn all_nodes(ast: &Ast) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![ast.root];
    while let Some(id) = stack.pop() {
        out.push(id);
        ast.for_each_child(id, |c| stack.push(c));
    }
    out.sort_by_key(|&id| (ast[id].span.start, id.index()));
    out
}

/// Every reference identifier, in source order, as `name:SymbolKind@ScopeKind` or
/// `name:global`.
fn refs_in(src: &str, module: bool) -> String {
    let (ast, sem) = build(src, module);
    let mut out = Vec::new();
    for id in all_nodes(&ast) {
        if let NodeKind::Ident(name) = ast[id].kind {
            out.push(match sem.symbol_of(id) {
                Some(sym) => {
                    let s = sem.symbol(sym);
                    format!("{}:{:?}@{:?}", ast.atom(name), s.kind, sem.scope(s.scope).kind)
                }
                None => format!("{}:global", ast.atom(name)),
            });
        }
    }
    out.join(" ")
}

fn refs(src: &str) -> String {
    refs_in(src, true)
}

fn script_refs(src: &str) -> String {
    refs_in(src, false)
}

fn error_in(src: &str, module: bool) -> String {
    match parse(src, ParseOptions::new(module)) {
        Ok(_) => panic!("{src:?} should not parse"),
        Err(e) => e.message.clone(),
    }
}

fn assert_error(src: &str, needle: &str) {
    let message = error_in(src, true);
    assert!(message.contains(needle), "{src:?}: expected {needle:?}, got {message:?}");
}

fn assert_script_error(src: &str, needle: &str) {
    let message = error_in(src, false);
    assert!(message.contains(needle), "{src:?}: expected {needle:?}, got {message:?}");
}

fn ok(src: &str) {
    build(src, true);
}

fn script_ok(src: &str) {
    build(src, false);
}

/// Reads and writes of the first symbol called `name`.
fn counts(src: &str, name: &str) -> (u32, u32) {
    let (ast, sem) = build(src, true);
    let _ = &ast;
    for (_, sym) in sem.symbols() {
        if ast.atom(sym.name) == name {
            return (sym.reads, sym.writes);
        }
    }
    panic!("no symbol {name} in {src:?}");
}

fn symbol_flags_of(src: &str, name: &str) -> u8 {
    let (ast, sem) = build(src, true);
    sem.symbols().find(|(_, s)| ast.atom(s.name) == name).map(|(_, s)| s.flags).expect("symbol not found")
}

#[test]
fn simple_resolution() {
    assert_eq!(refs("let a = 1; a + b;"), "a:Let@Module b:global");
    assert_eq!(refs("const a = 1; var b; a; b;"), "a:Const@Module b:Var@Module");
    assert_eq!(refs("class A {} A;"), "A:Class@Module");
}

#[test]
fn block_scoping_and_shadowing() {
    assert_eq!(refs("let a; { let a; a } a;"), "a:Let@Block a:Let@Module");
    assert_eq!(refs("{ let a; } a;"), "a:global");
    assert_eq!(refs("{ var a; } a;"), "a:Var@Module");
    assert_eq!(refs("{ { var a; } } a;"), "a:Var@Module");
    assert_eq!(refs("if (x) { let y; y }"), "x:global y:Let@Block");
}

#[test]
fn functions_hoist_and_have_their_own_scope() {
    assert_eq!(refs("f(); function f() {}"), "f:LexicalFunction@Module");
    assert_eq!(script_refs("f(); function f() {}"), "f:Function@Script");
    assert_eq!(refs("function f(a) { return a; }"), "a:Param@Function");
    assert_eq!(refs("function f() { var a; { a } }"), "a:Var@Function");
    assert_eq!(refs("var a; function f() { a }"), "a:Var@Module");
    assert_eq!(refs("function f() { var a; } a;"), "a:global");
    assert_eq!(refs("function f() { g(); function g() {} }"), "g:Function@Function");
}

#[test]
fn arrow_functions() {
    assert_eq!(refs("(a, b) => a + b + c;"), "a:Param@Function b:Param@Function c:global");
    assert_eq!(refs("x => { var y; return x + y; }"), "x:Param@Function y:Var@Function");
    assert_eq!(refs("async (a = b, c) => a;"), "b:global a:Param@Function");
}

#[test]
fn parameter_defaults_cannot_see_body_declarations() {
    // With non-simple parameters the body is a separate scope, so `x` in the default
    // refers to the outer `x`, not the body's.
    assert_eq!(refs("var x; function f(a = x) { var x; }"), "x:Var@Module");
    assert_eq!(refs("function f(a = () => x) { let x; }"), "x:global");
    assert_eq!(refs("function f(a = 1) { var a; a }"), "a:Var@Body");
    // With simple parameters there is one scope and `var a` is the parameter itself.
    assert_eq!(refs("function f(a) { var a; a }"), "a:Param@Function");
    assert_eq!(refs("function f({a}) { a }"), "a:Param@Function");
}

#[test]
fn function_expression_names() {
    assert_eq!(refs("(function f() { f });"), "f:FunctionName@Function");
    assert_eq!(refs("(function f(f) { f });"), "f:Param@Function");
    assert_eq!(refs("(function f() { var f; f });"), "f:Var@Function");
    assert_eq!(refs("(function f() { let f; f });"), "f:Let@Function");
    assert_eq!(refs("(function f() {}); f;"), "f:global");
    assert_eq!(refs("function f() { f } "), "f:LexicalFunction@Module");
}

#[test]
fn class_names() {
    assert_eq!(refs("class A { m() { A } } A;"), "A:ClassInner@Class A:Class@Module");
    assert_eq!(refs("(class B { m() { B } }); B;"), "B:ClassInner@Class B:global");
    assert_eq!(refs("class A extends B {} "), "B:global");
    assert_eq!(refs("class A { [k]() {} x = y; }"), "k:global y:global");
    assert_eq!(refs("let k; class A { [k]() {} }"), "k:Let@Module");
}

#[test]
fn class_static_blocks_are_var_scopes() {
    assert_eq!(refs("class A { static { var x; x } } x;"), "x:Var@StaticBlock x:global");
    assert_eq!(refs("class A { static { let x; { x } } }"), "x:Let@StaticBlock");
}

#[test]
fn catch_clauses() {
    assert_eq!(refs("try {} catch (e) { e }"), "e:CatchParam@Catch");
    assert_eq!(refs("try {} catch ({e}) { e }"), "e:CatchPatternParam@Catch");
    assert_eq!(refs("try {} catch ([a, b = c]) { a }"), "c:global a:CatchPatternParam@Catch");
    assert_eq!(refs("try {} catch (e) {} e;"), "e:global");
    assert_eq!(refs("try {} catch { x }"), "x:global");
}

#[test]
fn loops_and_switch_have_scopes() {
    assert_eq!(refs("for (let i = 0; i < 1; i++) i; i;"), "i:Let@For i:Let@For i:Let@For i:global");
    assert_eq!(refs("for (var i = 0; i < 1; i++) {} i;"), "i:Var@Module i:Var@Module i:Var@Module");
    assert_eq!(refs("for (const x of xs) x;"), "xs:global x:Const@For");
    assert_eq!(refs("for (let k in o) k;"), "o:global k:Let@For");
    assert_eq!(refs("let x; for (x of xs) ;"), "x:Let@Module xs:global");
    assert_eq!(refs("switch (a) { case 1: let b; b }"), "a:global b:Let@Switch");
    // The discriminant is evaluated outside the switch's scope.
    assert_eq!(refs("let a; switch (a) { case 1: let a; }"), "a:Let@Module");
}

#[test]
fn arguments_object() {
    assert_eq!(refs("function f() { arguments }"), "arguments:Arguments@Function");
    assert_eq!(refs("function f() { () => arguments }"), "arguments:Arguments@Function");
    assert_eq!(refs("() => arguments"), "arguments:global");
    assert_eq!(refs("arguments"), "arguments:global");
    assert_eq!(script_refs("function f(arguments) { arguments }"), "arguments:Param@Function");
    assert_eq!(script_refs("function f() { var arguments; arguments }"), "arguments:Var@Function");
}

#[test]
fn imports_and_exports() {
    assert_eq!(refs("import a, {b as c} from 'm'; a; c; b;"), "a:Import@Module c:Import@Module b:global");
    assert_eq!(refs("import * as ns from 'm'; ns.x;"), "ns:Import@Module");
    assert_ne!(symbol_flags_of("const a = 1; export {a};", "a") & symbol_flags::EXPORTED, 0);
    assert_ne!(symbol_flags_of("export var a;", "a") & symbol_flags::EXPORTED, 0);
    assert_ne!(symbol_flags_of("export function f() {}", "f") & symbol_flags::EXPORTED, 0);
    assert_ne!(symbol_flags_of("export class C {}", "C") & symbol_flags::EXPORTED, 0);
    assert_ne!(symbol_flags_of("export const [a, {b}] = c;", "b") & symbol_flags::EXPORTED, 0);
    assert_ne!(symbol_flags_of("export default function f() {}", "f") & symbol_flags::EXPORTED, 0);
    assert_eq!(symbol_flags_of("const a = 1; export {a}; const b = 2;", "b") & symbol_flags::EXPORTED, 0);
    // Re-exporting an import is fine, and export order does not matter.
    ok("import a from 'm'; export {a};");
    ok("export {a}; var a;");
    ok("export {a as b, a as c}; let a;");
    ok("export {a} from 'm';");
    ok("export * from 'm'; export * as ns from 'n';");
}

#[test]
fn reads_and_writes_are_counted() {
    assert_eq!(counts("let a = 1; a; a;", "a"), (2, 1));
    assert_eq!(counts("let a; a = 2;", "a"), (0, 1));
    assert_eq!(counts("let a = 1; a += 2;", "a"), (1, 2));
    assert_eq!(counts("let a = 1; a++; --a;", "a"), (2, 3));
    assert_eq!(counts("let a = 1; a &&= 2;", "a"), (1, 2));
    assert_eq!(counts("let a, b; [a, {b}] = c;", "a"), (0, 1));
    assert_eq!(counts("let a, b; [a, {x: b}] = c;", "b"), (0, 1));
    assert_eq!(counts("let a, b; ({a, ...b} = c);", "b"), (0, 1));
    assert_eq!(counts("let a, d; [a = d] = c;", "d"), (1, 0));
    assert_eq!(counts("let a, k; ({[k]: a} = c);", "k"), (1, 0));
    assert_eq!(counts("let a; for (a of xs) ;", "a"), (0, 1));
    assert_eq!(counts("for (const x of xs) ;", "x"), (0, 1));
    assert_eq!(counts("let a; o.a = 1; o[a] = 2;", "a"), (1, 0));
    assert_eq!(counts("let a; function f() { a = 1; }", "a"), (0, 1));
    assert_eq!(counts("var [a, b] = c;", "b"), (0, 1));
}

#[test]
fn unresolved_references_are_collected() {
    let (ast, sem) = build("foo; bar = 1; let x; x;", true);
    let names: Vec<&str> = sem
        .unresolved()
        .iter()
        .map(|&n| match ast[n].kind {
            NodeKind::Ident(a) => ast.atom(a),
            _ => "?",
        })
        .collect();
    assert_eq!(names, ["foo", "bar"]);
}

#[test]
fn direct_eval_marks_enclosing_scopes() {
    let (_, sem) = build("function f() { { eval('x'); } } function g() {}", true);
    let flagged: Vec<(ScopeKind, bool)> = sem.scopes().map(|(_, s)| (s.kind, s.flags & scope_flags::HAS_EVAL != 0)).collect();
    assert_eq!(
        flagged,
        [
            (ScopeKind::Module, true),
            (ScopeKind::Function, true),
            (ScopeKind::Block, true),
            (ScopeKind::Function, false),
        ]
    );
    // `(0, eval)(x)` is indirect and `foo.eval(x)` is a method call.
    let (_, sem) = build("(0, eval)('x'); o.eval('x');", true);
    assert!(sem.scopes().all(|(_, s)| s.flags & scope_flags::HAS_EVAL == 0));
}

#[test]
fn with_marks_symbols_dynamic() {
    let (ast, sem) = build("var a, b; with (o) { a; } b;", false);
    let flags: Vec<(String, bool)> = sem
        .symbols()
        .map(|(_, s)| (ast.atom(s.name).to_string(), s.flags & symbol_flags::DYNAMIC != 0))
        .collect();
    assert_eq!(flags, [("a".to_string(), true), ("b".to_string(), false)]);
    assert_eq!(script_refs("with (o) { a }"), "o:global a:global");
}

#[test]
fn annex_b_block_functions_hoist_in_sloppy_code() {
    assert_eq!(script_refs("{ function f() {} } f;"), "f:Var@Script");
    assert_eq!(script_refs("function g() { { function f() {} } f; }"), "f:Var@Function");
    // Strict code keeps the function in its block.
    assert_eq!(refs("{ function f() {} } f;"), "f:global");
    assert_eq!(script_refs("'use strict'; { function f() {} } f;"), "f:global");
    // A parameter or an enclosing lexical binding blocks the hoist.
    assert_eq!(script_refs("function g(f) { { function f() {} } f; }"), "f:Param@Function");
    assert_eq!(script_refs("let f; { function f() {} } f;"), "f:Let@Script");
    // Async and generator functions are never hoisted.
    assert_eq!(script_refs("{ async function f() {} } f;"), "f:global");
    let (ast, sem) = build("{ function f() {} }", false);
    let block_fn = sem.symbols().find(|(_, s)| s.kind == SymbolKind::BlockFunction).unwrap().1;
    let alias = block_fn.alias.expect("hoisted");
    assert_eq!(ast.atom(sem.symbol(alias).name), "f");
    assert_eq!(sem.symbol(alias).kind, SymbolKind::Var);
}

#[test]
fn annex_b_hoisting_is_cancelled_by_any_lexical_declaration() {
    // A later declaration blocks the hoist; it is not an error.
    script_ok("{ function x() {} } let x;");
    script_ok("if (1) function x() {} let x;");
    script_ok("{ function x() {} } const x = 1;");
    script_ok("{ function x() {} } class x {}");
    script_ok("function f() { { function x() {} } let x; }");
    script_ok("{ let x; { function x() {} } }");
    script_ok("{ { function x() {} } let x; }");
    script_ok("function f(x) { { function x() {} } }");
    assert_eq!(script_refs("{ function x() {} } let x; x;"), "x:Let@Script");
    assert_eq!(script_refs("let x; { function x() {} } x;"), "x:Let@Script");
    assert_eq!(script_refs("{ function x() {} } x; let y;"), "x:Var@Script");
    // Declared later, hoisted anyway when nothing blocks it.
    assert_eq!(script_refs("x; { function x() {} }"), "x:Var@Script");
    // Two block functions with one name share one hoisted var.
    let (_, sem) = build("{ function f() {} } { function f() {} } f;", false);
    let vars = sem.symbols().filter(|(_, s)| s.kind == SymbolKind::Var).count();
    assert_eq!(vars, 1);
}

#[test]
fn scopes_and_symbols_are_deterministic_and_ordered() {
    let (ast, sem) = build("let c; var a; function b() {} class d {}", true);
    let names: Vec<&str> = sem.scope(sem.root_scope()).symbols().iter().map(|&s| ast.atom(sem.symbol(s).name)).collect();
    assert_eq!(names, ["c", "a", "b", "d"]);
    assert_eq!(sem.scope(sem.root_scope()).kind, ScopeKind::Module);
    let (_, script) = build("var a;", false);
    assert_eq!(script.scope(script.root_scope()).kind, ScopeKind::Script);
}

#[test]
fn lookup_walks_outwards() {
    let (ast, sem) = build("let a; function f() { let b; }", true);
    let inner = sem.scopes().find(|(_, s)| s.kind == ScopeKind::Function).unwrap().0;
    let find = |name: &str| {
        let atom = sem.symbols().find(|(_, s)| ast.atom(s.name) == name).map(|(_, s)| s.name).unwrap();
        sem.lookup(inner, atom)
    };
    assert!(find("a").is_some());
    assert!(find("b").is_some());
    assert_eq!(sem.lookup(sem.root_scope(), sem.symbol(find("b").unwrap()).name), None);
}

#[test]
fn redeclaration_errors() {
    for bad in [
        "let a; let a;",
        "let a; const a = 1;",
        "var a; let a;",
        "let a; var a;",
        "const a = 1; var a;",
        "{ let a; var a; }",
        "{ let a; { var a; } }",
        "{ var a; } let a;",
        "{ { var a; } let a; }",
        "class A {} class A {}",
        "class A {} var A;",
        "let A; class A {}",
        "function f(a) { let a; }",
        "function f(a = 1) { let a; }",
        "function f({a}) { const a = 1; }",
        "function f() { let a; var a; }",
        "function f() { var a; let a; }",
        "(a) => { let a; }",
        "try {} catch (e) { let e; }",
        "try {} catch ({e}) { var e; }",
        "try {} catch ([e]) { function e() {} }",
        "try {} catch (e) { function e() {} }",
        "switch (a) { case 1: let x; case 2: let x; }",
        "switch (a) { case 1: let x; default: var x; }",
        "for (let i;;) { var i; }",
        "for (let i of a) { var i; }",
        "import a from 'm'; import a from 'n';",
        "import a from 'm'; var a;",
        "import a from 'm'; let a;",
        "import {b as a} from 'm'; function a() {}",
        "import {a, a} from 'm';",
        "function f() {} function f() {}",
        "var f; function f() {}",
        "function f() {} var f;",
        "let f; function f() {}",
        "function f() {} class f {}",
        "export default function f() {} function f() {}",
        "export function f() {} let f;",
        "{ function f() {} let f; }",
        "{ function f() {} var f; }",
        "{ function* f() {} function f() {} }",
        "{ async function f() {} function f() {} }",
        "{ function f() {} function f() {} }",
        "function f() { function g() {} let g; }",
        "class A { static { let a; var a; } }",
        "class A { static { var a; let a; } }",
        "try {} catch ([a, a]) {}",
    ] {
        assert_error(bad, "already been declared");
    }
}

#[test]
fn allowed_redeclarations() {
    for fine in [
        "var a; var a;",
        "var a; { var a; }",
        "function f() { var a; { let a; } }",
        "function f() { { var a; } var a; }",
        "function f(a) { var a; }",
        "function f(a, b) { { let a; } }",
        "try {} catch (e) { var e; }",
        "try {} catch (e) { { let e; } }",
        "try {} catch (e) {} try {} catch (e) {}",
        "(function f() { var f; })",
        "(function f() { let f; })",
        "(function f(f) {})",
        "(class A { m() { var A; } })",
        "class A { m() { let A; } }",
        "for (let i;;) { let i; }",
        "for (let i;;) { { var j; } }",
        "for (var i;;) { var i; }",
        "let a; { let a; }",
        "let a; function f() { let a; }",
        "switch (a) { case 1: { let x; } case 2: { let x; } }",
        "import a from 'm'; { let a; }",
        "var a; function f() { let a; }",
        "function f() {} { function f() {} }",
        "{ function f() {} } { function f() {} }",
        "class A { static { var a; } static { var a; } }",
        "class A { m() { var a; } n() { var a; } }",
    ] {
        ok(fine);
    }
}

#[test]
fn script_mode_redeclaration_rules_differ() {
    // Top-level functions are var-like in scripts and function bodies.
    script_ok("function f() {} function f() {}");
    script_ok("var f; function f() {}");
    script_ok("function f() {} var f;");
    script_ok("function g() { function f() {} var f; function f() {} }");
    // Sloppy blocks may repeat a plain function, but strict ones may not.
    script_ok("{ function f() {} function f() {} }");
    assert_script_error("'use strict'; { function f() {} function f() {} }", "already been declared");
    assert_script_error("{ function* f() {} function f() {} }", "already been declared");
    assert_script_error("let f; function f() {}", "already been declared");
    assert_script_error("function g() { let f; function f() {} }", "already been declared");
}

#[test]
fn export_of_undeclared_names() {
    assert_error("export {a};", "not defined");
    assert_error("export {a as b};", "not defined");
    assert_error("export {a, b}; var a;", "not defined");
    assert_error("function f() { var a; } export {a};", "not defined");
    assert_error("{ let a; } export {a};", "not defined");
    assert_error("export {undefined};", "not defined");
}

#[test]
fn deeply_nested_input_does_not_overflow_the_stack() {
    // The parser builds this iteratively, so the tree is 200,000 levels deep on the left.
    let chain = format!("let a = 1; a{};", "+a".repeat(200_000));
    let (_, sem) = build(&chain, true);
    let a = sem.symbols().next().unwrap().1;
    assert_eq!(a.reads, 200_001);
    let calls = format!("let f; f{};", "()".repeat(100_000));
    build(&calls, true);
}

#[test]
fn analysis_matches_the_parser_on_a_realistic_module() {
    let src = r#"
        import { readFile } from 'fs';
        import path, * as p from 'path';
        const cache = new Map();
        export async function load(name, { force = false } = {}) {
            if (!force && cache.has(name)) return cache.get(name);
            let data;
            try {
                data = await readFile(path.join(name));
            } catch (error) {
                throw new Error(`cannot read ${name}: ${error.message}`);
            } finally {
                cache.delete(undefined);
            }
            for (const [key, value] of Object.entries(data)) {
                cache.set(key, value);
            }
            return data;
        }
        export class Loader {
            static #count = 0;
            #items = [];
            constructor() { Loader.#count++; }
            add(item) { this.#items.push(item); return this; }
            get size() { return this.#items.length; }
        }
        export default function () { return p; }
    "#;
    let (ast, sem) = build(src, true);
    let unresolved: Vec<&str> = sem
        .unresolved()
        .iter()
        .map(|&n| match ast[n].kind {
            NodeKind::Ident(a) => ast.atom(a),
            _ => "?",
        })
        .collect();
    assert_eq!(unresolved, ["Map", "Error", "undefined", "Object"]);
}

#[test]
fn arrow_parameters_adopt_scopes_created_before_the_arrow_was_known() {
    // The function expression in the default is parsed before `=>` shows this is an arrow
    // function's parameter list, yet it must end up inside the arrow's scope.
    let shape = |src: &str| -> Vec<(ScopeKind, Option<usize>)> {
        let (_, sem) = build(src, true);
        sem.scopes().map(|(_, s)| (s.kind, s.parent.map(|p| p.index()))).collect()
    };
    use ScopeKind::*;
    assert_eq!(shape("(a = function () { var x; }) => 1;"), [(Module, None), (Function, Some(2)), (Function, Some(0))]);
    // Only scopes that hung directly off the enclosing scope move; deeper ones keep their parent.
    assert_eq!(
        shape("(a = function () { { let y; } }) => 1;"),
        [(Module, None), (Function, Some(3)), (Block, Some(1)), (Function, Some(0))]
    );
    // A parenthesized expression that is not an arrow adopts nothing.
    assert_eq!(shape("(function () { var x; });"), [(Module, None), (Function, Some(0))]);
    // Sibling expressions before the arrow are untouched.
    assert_eq!(
        shape("f(function () {}); (a = function () {}) => 1;"),
        [(Module, None), (Function, Some(0)), (Function, Some(3)), (Function, Some(0))]
    );
    // Nested arrows each take their own.
    assert_eq!(
        shape("(a = (b = function () {}) => 1) => 2;"),
        [(Module, None), (Function, Some(2)), (Function, Some(3)), (Function, Some(0))]
    );
}

#[test]
fn arrow_parameter_references_resolve_inside_the_arrow() {
    assert_eq!(refs("(a, b = a) => b;"), "a:Param@Function b:Param@Function");
    assert_eq!(refs("(a = class { m() { return a; } }) => a;"), "a:Param@Function a:Param@Function");
    assert_eq!(refs("((a) => a)(1);"), "a:Param@Function");
    assert_eq!(refs("(a = (b = a) => b) => a;"), "a:Param@Function b:Param@Function a:Param@Function");
    assert_eq!(refs("async (a, b = a + c) => b;"), "a:Param@Function c:global b:Param@Function");
    // A default sees the outer variable, not one the body declares.
    assert_eq!(refs("var x; (a = x) => { var x; };"), "x:Var@Module");
    assert_eq!(refs("(a = () => x) => { let x; };"), "x:global");
    // Not an arrow: references stay in the enclosing scope.
    assert_eq!(refs("let a; (a, function () { return a; });"), "a:Let@Module a:Let@Module");
    assert_eq!(refs("(function () { var x; x; });"), "x:Var@Function");
}

#[test]
fn parameters_of_adopted_arrows_are_declared_in_the_arrow() {
    let (ast, sem) = build("let a = 1; ((a, b) => a + b)(2, 3);", true);
    let names = |scope_kind: ScopeKind| -> Vec<&str> {
        sem.scopes()
            .filter(|(_, s)| s.kind == scope_kind)
            .flat_map(|(_, s)| s.symbols().iter().map(|&id| ast.atom(sem.symbol(id).name)))
            .collect()
    };
    assert_eq!(names(ScopeKind::Function), ["a", "b"]);
    assert_eq!(names(ScopeKind::Module), ["a"]);
    // The arrow flag is set, so it has no `arguments` of its own.
    let (_, sem) = build("function f() { return (a = arguments) => a; }", true);
    let arrow = sem.scopes().find(|(_, s)| s.flags & scope_flags::ARROW != 0).expect("an arrow scope").1;
    assert_eq!(arrow.kind, ScopeKind::Function);
}

#[test]
fn labels_are_not_references() {
    assert_eq!(counts("var a; a: for (;;) break a;", "a"), (0, 0));
    assert_eq!(refs("var a; a: ;"), "");
    assert_eq!(refs("a: b: c;"), "c:global");
}

#[test]
fn async_calls_keep_source_order_of_references() {
    assert_eq!(script_refs("async(a, b);"), "async:global a:global b:global");
    assert_eq!(script_refs("var async; async(a);"), "async:Var@Script a:global");
    assert_eq!(script_refs("async;"), "async:global");
}

#[test]
fn function_declaration_names_follow_their_position() {
    // A declaration's name is bound in the enclosing scope, a parameter in its own.
    assert_eq!(refs("function f(g) { function h() {} h(g); } f(1);"), "h:Function@Function g:Param@Function f:LexicalFunction@Module");
    assert_eq!(refs("if (x) { function f() {} f(); }"), "x:global f:BlockFunction@Block");
}
