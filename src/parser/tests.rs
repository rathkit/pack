use super::*;
use crate::dump::dump_node;

fn module(src: &str) -> Ast {
    parse(src, ParseOptions::new(true)).unwrap_or_else(|e| panic!("{src:?} failed to parse: {e}")).ast
}

fn script(src: &str) -> Ast {
    parse(src, ParseOptions::new(false)).unwrap_or_else(|e| panic!("{src:?} failed to parse as a script: {e}")).ast
}

fn top_level(ast: &Ast) -> Vec<NodeId> {
    let NodeKind::Program(list) = ast[ast.root].kind else { panic!("root is not a program") };
    ast.list(list).to_vec()
}

/// Every top-level statement, joined by ` | `.
fn stmts(src: &str) -> String {
    let ast = module(src);
    top_level(&ast).iter().map(|&s| dump_node(&ast, src, s)).collect::<Vec<_>>().join(" | ")
}

/// The expression in a single expression statement.
fn expr(src: &str) -> String {
    let ast = module(src);
    let items = top_level(&ast);
    let NodeKind::ExprStmt(e) = ast[items[0]].kind else { panic!("{src:?} is not an expression statement") };
    dump_node(&ast, src, e)
}

fn err(src: &str) -> ParseError {
    match parse(src, ParseOptions::new(true)) {
        Ok(_) => panic!("{src:?} should not parse"),
        Err(e) => e,
    }
}

fn assert_err(src: &str, needle: &str) {
    let e = err(src);
    assert!(e.message.contains(needle), "{src:?}: expected an error containing {needle:?}, got {:?}", e.message);
}

/// Test sources spell a backslash as `~` (see the lexer tests for why).
fn esc(src: &str) -> String {
    src.replace('~', "\\")
}

#[test]
fn binary_precedence_and_associativity() {
    assert_eq!(expr("a + b * c;"), "(+ a (* b c))");
    assert_eq!(expr("a * b + c;"), "(+ (* a b) c)");
    assert_eq!(expr("a - b - c;"), "(- (- a b) c)");
    assert_eq!(expr("a ** b ** c;"), "(** a (** b c))");
    assert_eq!(expr("(a + b) * c;"), "(* (+ a b)^ c)");
    assert_eq!(expr("a < b == c > d;"), "(== (< a b) (> c d))");
    assert_eq!(expr("a || b && c;"), "(|| a (&& b c))");
    assert_eq!(expr("a | b ^ c & d;"), "(| a (^ b (& c d)))");
    assert_eq!(expr("a << 1 + 2;"), "(<< a (+ 1 2))");
    assert_eq!(expr("a >>> b >> c;"), "(>> (>>> a b) c)");
    assert_eq!(expr("a instanceof B;"), "(instanceof a B)");
    assert_eq!(expr("a in b;"), "(in a b)");
}

#[test]
fn unary_and_update() {
    assert_eq!(expr("-a + +b;"), "(+ (u- a) (u+ b))");
    assert_eq!(expr("!a;"), "(u! a)");
    assert_eq!(expr("typeof a === 'x';"), "(=== (utypeof a) \"x\")");
    assert_eq!(expr("a++ + ++b;"), "(+ (UpdateInc a) (UpdateInc[prefix] b))");
    assert_eq!(expr("void 0;"), "(uvoid 0)");
    assert_eq!(expr("delete a.b;"), "(udelete (Member a :b))");
}

#[test]
fn exponent_and_nullish_restrictions() {
    assert_err("-a ** b;", "unary operator");
    assert_eq!(expr("(-a) ** b;"), "(** (u- a)^ b)");
    assert_eq!(expr("a ** -b;"), "(** a (u- b))");
    assert_err("a ?? b || c;", "cannot be mixed");
    assert_err("a || b ?? c;", "cannot be mixed");
    assert_err("a && b ?? c;", "cannot be mixed");
    assert_eq!(expr("(a ?? b) || c;"), "(|| (?? a b)^ c)");
    assert_eq!(expr("a ?? (b || c);"), "(?? a (|| b c)^)");
}

#[test]
fn assignment_and_conditional() {
    assert_eq!(expr("a = b = c;"), "(= a (= b c))");
    assert_eq!(expr("a += 1;"), "(+= a 1)");
    assert_eq!(expr("a ??= b;"), "(??= a b)");
    assert_eq!(expr("a ||= b;"), "(||= a b)");
    assert_eq!(expr("a ? b : c ? d : e;"), "(Conditional a b (Conditional c d e))");
    assert_eq!(expr("a ? b = 1 : c;"), "(Conditional a (= b 1) c)");
    assert_eq!(expr("a, b, c;"), "(Sequence a b c)");
    assert_err("1 = 2;", "Invalid");
    assert_err("a + b = 2;", "Invalid");
    assert_err("a?.b = 1;", "Invalid");
}

#[test]
fn member_call_and_new() {
    assert_eq!(expr("a.b.c;"), "(Member (Member a :b) :c)");
    assert_eq!(expr("a[b];"), "(Member[computed] a b)");
    assert_eq!(expr("a.if.class;"), "(Member (Member a :if) :class)");
    assert_eq!(expr("f(a, ...b);"), "(Call f a (Spread b))");
    assert_eq!(expr("f(a,);"), "(Call f a)");
    assert_eq!(expr("f()();"), "(Call (Call f))");
    assert_eq!(expr("new Foo(1);"), "(New Foo 1)");
    assert_eq!(expr("new Foo;"), "(New Foo)");
    assert_eq!(expr("new a.b();"), "(New (Member a :b))");
    assert_eq!(expr("new a.b().c;"), "(Member (New (Member a :b)) :c)");
    assert_eq!(expr("new new X()();"), "(New (New X))");
    assert_eq!(expr("new (f())();"), "(New (Call f)^)");
    assert_eq!(expr("super_;"), "super_");
}

#[test]
fn optional_chaining() {
    assert_eq!(expr("a?.b;"), "(Chain (Member[?] a :b))");
    assert_eq!(expr("a?.b.c(d);"), "(Chain (Call (Member (Member[?] a :b) :c) d))");
    assert_eq!(expr("a?.[b];"), "(Chain (Member[computed][?] a b))");
    assert_eq!(expr("a?.();"), "(Chain (Call[?] a))");
    assert_eq!(expr("(a?.b).c;"), "(Member (Chain (Member[?] a :b))^ :c)");
    assert_eq!(expr("a?.b ?? c;"), "(?? (Chain (Member[?] a :b)) c)");
    assert_eq!(expr("a ? .5 : 1;"), "(Conditional a 0.5 1)");
    assert_err("a?.`x`;", "optional chain");
    assert_err("new a?.b();", "callee");
}

#[test]
fn literals() {
    assert_eq!(expr("1;"), "1");
    assert_eq!(expr("0xFF;"), "255");
    assert_eq!(expr("1_000;"), "1000");
    assert_eq!(expr("1.5e2;"), "150");
    assert_eq!(expr("10n;"), "10n");
    assert_eq!(expr("0xFFn;"), "0xFFn");
    assert_eq!(expr("'a\"b';"), "\"a\\\"b\"");
    assert_eq!(expr(&esc("'a~nb~x41';")), "\"a\\nbA\"");
    assert_eq!(expr("true;"), "true");
    assert_eq!(expr("null;"), "null");
    assert_eq!(expr("this;"), "this");
    assert_eq!(expr("x = /ab+c/gi;"), "(= x /ab+c/gi)");
    assert_eq!(expr("a / b / c;"), "(/ (/ a b) c)");
    assert_eq!(expr("a /= 2;"), "(/= a 2)");
    assert_eq!(expr("x = /=/;"), "(= x /=/)");
    assert_err("/a/gg;", "flags");
    assert_err("/a/x;", "flags");
}

#[test]
fn templates() {
    assert_eq!(expr("`plain`;"), "(Template \"plain\")");
    assert_eq!(expr("`a${b}c`;"), "(Template \"a\" b \"c\")");
    assert_eq!(expr("`${a}${b}`;"), "(Template \"\" a \"\" b \"\")");
    assert_eq!(expr("`a${`b${c}d`}e`;"), "(Template \"a\" (Template \"b\" c \"d\") \"e\")");
    assert_eq!(expr("`${ {a: 1}.a }`;"), "(Template \"\" (Member (Object (PropInit :a 1)) :a) \"\")");
    assert_eq!(expr("f`x${y}`;"), "(TaggedTemplate f (Template \"x\" y \"\"))");
    assert_eq!(expr(&esc("`a~nb`;")), "(Template \"a\\nb\")");
    assert_eq!(expr(&esc("f`~unicode`;")), "(TaggedTemplate f (Template <invalid>))");
    assert_err(&esc("`~unicode`;"), "escape");
}

#[test]
fn arrays_and_objects() {
    assert_eq!(expr("[a, , b];"), "(Array a <hole> b)");
    assert_eq!(expr("[,];"), "(Array <hole>)");
    assert_eq!(expr("[a,];"), "(Array a)");
    assert_eq!(expr("[...a, b];"), "(Array (Spread a) b)");
    assert_eq!(expr("({});"), "(Object)^");
    assert_eq!(expr("({a, b: 1, [c]: 2, 'd': 3, 4: 5});"), "(Object (PropInit[short] :a a) (PropInit :b 1) (PropInit[computed] c 2) (PropInit \"d\" 3) (PropInit 4 5))^");
    assert_eq!(expr("({a() {}, get b() {}, set c(v) {}, async d() {}, *e() {}});"), "(Object (PropMethod :a (Function () (Block))) (PropGet :b (Function () (Block))) (PropSet :c (Function ($v) (Block))) (PropMethod :d (Function[async] () (Block))) (PropMethod :e (Function[gen] () (Block))))^");
    assert_eq!(expr("({...a, b});"), "(Object (Spread a) (PropInit[short] :b b))^");
    assert_eq!(expr("({get: 1, set: 2, async: 3, static: 4});"), "(Object (PropInit :get 1) (PropInit :set 2) (PropInit :async 3) (PropInit :static 4))^");
    assert_eq!(expr("({get, set});"), "(Object (PropInit[short] :get get) (PropInit[short] :set set))^");
    assert_eq!(expr("({if: 1, class: 2});"), "(Object (PropInit :if 1) (PropInit :class 2))^");
    assert_err("({if});", "");
    assert_err("({'a'});", "");
}

#[test]
fn arrow_functions() {
    assert_eq!(expr("x => x + 1;"), "(Function[arrow][expr] ($x) (+ x 1))");
    assert_eq!(expr("(a, b) => {};"), "(Function[arrow] ($a $b) (Block))");
    assert_eq!(expr("() => 1;"), "(Function[arrow][expr] () 1)");
    assert_eq!(expr("(a = 1, {b}, [c], ...d) => a;"), "(Function[arrow][expr] ((AssignPattern $a 1) (ObjectPattern (PatternProp[short] :b $b)) (ArrayPattern $c) (Rest $d)) a)");
    assert_eq!(expr("async x => x;"), "(Function[async][arrow][expr] ($x) x)");
    assert_eq!(expr("async (a) => a;"), "(Function[async][arrow][expr] ($a) a)");
    assert_eq!(expr("async () => {};"), "(Function[async][arrow] () (Block))");
    assert_eq!(expr("a => b => c;"), "(Function[arrow][expr] ($a) (Function[arrow][expr] ($b) c))");
    assert_eq!(expr("f(x => x, y);"), "(Call f (Function[arrow][expr] ($x) x) y)");
    assert_eq!(expr("a ? x => 1 : 2;"), "(Conditional a (Function[arrow][expr] ($x) 1) 2)");
    assert_eq!(expr("x = () => ({});"), "(= x (Function[arrow][expr] () (Object)^))");
    assert_eq!(expr("(a, b);"), "(Sequence a b)^");
    assert_eq!(expr("async;"), "async");
    assert_eq!(expr("async(a, b);"), "(Call async a b)");
    assert_eq!(expr("async => async;"), "(Function[arrow][expr] ($async) async)");
    assert_err("(a, b) + 1 => 2;", "");
    assert_err("() ;", "");
    assert_err("(a,) ;", "");
    assert_err("(...a) ;", "");
    assert_err("(a, ...b, c) => 1;", "rest");
    assert_err("(1) => 1;", "Invalid");
    assert_err("((a)) => 1;", "Invalid");
    assert_err("(a.b) => 1;", "Invalid");
}

#[test]
fn arrow_function_asi() {
    // An arrow's body ends at the newline here, so `(foo)` is a new statement, not a call.
    assert_eq!(stmts("const f = x => {}\n(foo)"), "(const (VarDeclarator $f (Function[arrow] ($x) (Block)))) | (ExprStmt foo^)");
    assert_err("x\n=> 1;", "");
}

#[test]
fn destructuring_assignment() {
    assert_eq!(expr("[a, b] = c;"), "(= (ArrayPattern a b) c)");
    assert_eq!(expr("[a = 1, , ...b] = c;"), "(= (ArrayPattern (AssignPattern a 1) <hole> (Rest b)) c)");
    assert_eq!(expr("({a, b: c, ...d} = e);"), "(= (ObjectPattern (PatternProp[short] :a a) (PatternProp :b c) (Rest d)) e)^");
    assert_eq!(expr("({a = 1} = b);"), "(= (ObjectPattern (PatternProp[short] :a (AssignPattern a 1))) b)^");
    assert_eq!(expr("[a.b, c[d]] = e;"), "(= (ArrayPattern (Member a :b) (Member[computed] c d)) e)");
    assert_eq!(expr("[{a}, [b]] = c;"), "(= (ArrayPattern (ObjectPattern (PatternProp[short] :a a)) (ArrayPattern b)) c)");
    assert_eq!(expr("({a: [b = 1]} = c);"), "(= (ObjectPattern (PatternProp :a (ArrayPattern (AssignPattern b 1)))) c)^");
    assert_eq!(expr("[(a)] = b;"), "(= (ArrayPattern a^) b)");
    assert_err("({a = 1});", "shorthand");
    assert_err("f({a = 1});", "shorthand");
    assert_err("[{a = 1}];", "shorthand");
    assert_err("({a = 1}).x = 2;", "shorthand");
    assert_err("[...a, b] = c;", "rest");
    assert_err("[...a = 1] = c;", "default");
    assert_err("({...{a}} = c);", "rest");
    assert_err("[a + b] = c;", "Invalid");
    assert_err("({a() {}} = c);", "Invalid");
    assert_err("([a]) = c;", "Invalid");
    assert_err("[f()] = c;", "Invalid");
}

#[test]
fn variable_declarations() {
    assert_eq!(stmts("var a;"), "(var (VarDeclarator $a))");
    assert_eq!(stmts("let x = 1, y;"), "(let (VarDeclarator $x 1) (VarDeclarator $y))");
    assert_eq!(stmts("const {a, b: [c]} = d;"), "(const (VarDeclarator (ObjectPattern (PatternProp[short] :a $a) (PatternProp :b (ArrayPattern $c))) d))");
    assert_eq!(stmts("const [a, , b = 2, ...c] = d;"), "(const (VarDeclarator (ArrayPattern $a <hole> (AssignPattern $b 2) (Rest $c)) d))");
    assert_eq!(stmts("var {a = 1, ...r} = o;"), "(var (VarDeclarator (ObjectPattern (PatternProp[short] :a (AssignPattern $a 1)) (Rest $r)) o))");
    assert_err("const a;", "initializer");
    assert_err("let [a];", "initializer");
    assert_err("var [...a, b] = c;", "rest");
    assert_err("var 1;", "");
    assert_err("let let = 1;", "");
}

#[test]
fn control_flow() {
    assert_eq!(stmts("if (a) b; else c;"), "(If a (ExprStmt b) (ExprStmt c))");
    assert_eq!(stmts("if (a) { b }"), "(If a (Block (ExprStmt b)))");
    assert_eq!(stmts("while (a) b;"), "(While a (ExprStmt b))");
    assert_eq!(stmts("do a; while (b)"), "(DoWhile (ExprStmt a) b)");
    assert_eq!(stmts("do a; while (b) c"), "(DoWhile (ExprStmt a) b) | (ExprStmt c)");
    assert_eq!(stmts("for (let i = 0; i < n; i++) x;"), "(For (let (VarDeclarator $i 0)) (< i n) (UpdateInc i) (ExprStmt x))");
    assert_eq!(stmts("for (;;) {}"), "(For (Block))");
    assert_eq!(stmts("for (a, b; ;) ;"), "(For (Sequence a b) (Empty))");
    assert_eq!(stmts("for (const x of xs) y;"), "(ForOf (const (VarDeclarator $x)) xs (ExprStmt y))");
    assert_eq!(stmts("for (var k in o) ;"), "(ForIn (var (VarDeclarator $k)) o (Empty))");
    assert_eq!(stmts("for (x in o) ;"), "(ForIn x o (Empty))");
    assert_eq!(stmts("for (x.y of z) ;"), "(ForOf (Member x :y) z (Empty))");
    assert_eq!(stmts("for ([a, b] of c) ;"), "(ForOf (ArrayPattern a b) c (Empty))");
    assert_eq!(stmts("for ({a = 1} of c) ;"), "(ForOf (ObjectPattern (PatternProp[short] :a (AssignPattern a 1))) c (Empty))");
    assert_eq!(stmts("for (const [a, b] of c) ;"), "(ForOf (const (VarDeclarator (ArrayPattern $a $b))) c (Empty))");
    assert_eq!(stmts("for await (const x of y) ;"), "(ForOf[await] (const (VarDeclarator $x)) y (Empty))");
    assert_eq!(stmts("for (var i = 0, n = a.length; i < n; i++) ;"), "(For (var (VarDeclarator $i 0) (VarDeclarator $n (Member a :length))) (< i n) (UpdateInc i) (Empty))");
    assert_err("for (const a, b of c) ;", "one variable");
    assert_err("for (let a = 1 of c) ;", "initializer");
    assert_err("for (const a;;) ;", "initializer");
    assert_err("for ({a = 1};;) ;", "shorthand");
    assert_err("for await (x in y) ;", "");
}

#[test]
fn in_operator_inside_for_heads() {
    // `in` is not an operator in the initializer, but is again inside brackets.
    assert_eq!(stmts("for (var i = (a in b) ? 1 : 2; ;) ;"), "(For (var (VarDeclarator $i (Conditional (in a b)^ 1 2))) (Empty))");
    assert_eq!(stmts("for (var i = [a in b]; ;) ;"), "(For (var (VarDeclarator $i (Array (in a b)))) (Empty))");
    assert_eq!(stmts("for (var f = () => { a in b }; ;) ;").len() > 0, true);
}

#[test]
fn jumps_labels_and_misc() {
    assert_eq!(stmts("a: for (;;) break a;"), "(Labeled a (For (Break a)))");
    assert_eq!(stmts("a: { continue_: 1 }"), "(Labeled a (Block (Labeled continue_ (ExprStmt 1))))");
    assert_eq!(stmts("while (1) { continue; }"), "(While 1 (Block (Continue)))");
    assert_eq!(stmts("switch (a) { case 1: b; break; default: c }"), "(Switch a (Case 1 (ExprStmt b) (Break)) (Case <none> (ExprStmt c)))".replace("<none> ", ""));
    assert_eq!(stmts("throw a;"), "(Throw a)");
    assert_eq!(stmts("try { a } catch (e) { b } finally { c }"), "(Try (Block (ExprStmt a)) (Catch $e (Block (ExprStmt b))) (Block (ExprStmt c)))");
    assert_eq!(stmts("try { a } catch { b }"), "(Try (Block (ExprStmt a)) (Catch (Block (ExprStmt b))))");
    assert_eq!(stmts("try { } catch ({message}) { }"), "(Try (Block) (Catch (ObjectPattern (PatternProp[short] :message $message)) (Block)))");
    assert_eq!(stmts("debugger;;"), "(Debugger) | (Empty)");
    assert_err("break;", "Illegal");
    assert_err("continue;", "Illegal");
    assert_err("while (1) { break nope; }", "label");
    assert_err("a: a: ;", "already");
    assert_err("throw\na;", "newline");
    assert_err("try { }", "catch");
    assert_err("switch (a) { default: default: }", "default");
    assert_err("return 1;", "return");
    assert_err("if (a) function f() {}", "position");
    assert_err("with (a) {}", "strict");
}

#[test]
fn automatic_semicolon_insertion() {
    assert_eq!(stmts("a\nb"), "(ExprStmt a) | (ExprStmt b)");
    assert_eq!(stmts("a\n++b"), "(ExprStmt a) | (ExprStmt (UpdateInc[prefix] b))");
    assert_eq!(stmts("a = b\n(c)"), "(ExprStmt (= a (Call b c)))");
    assert_eq!(stmts("a = 1\n+b"), "(ExprStmt (= a (+ 1 b)))");
    assert_eq!(stmts("function f() { return\n1 }"), "(Function[decl] :f () (Block (Return) (ExprStmt 1)))".replace(":f", "$f"));
    assert_eq!(stmts("{ a }"), "(Block (ExprStmt a))");
    assert_eq!(stmts("{ a; b }"), "(Block (ExprStmt a) (ExprStmt b))");
    assert_err("a b", "");
    assert_err("let x = 1 let y = 2", "");
}

#[test]
fn functions() {
    assert_eq!(stmts("function f(a, b = 1, ...c) { return a; }"), "(Function[decl] $f ($a (AssignPattern $b 1) (Rest $c)) (Block (Return a)))");
    assert_eq!(stmts("function* g() { yield 1; yield* h(); const x = yield; }"), "(Function[gen][decl] $g () (Block (ExprStmt (Yield 1)) (ExprStmt (Yield[*] (Call h))) (const (VarDeclarator $x (Yield)))))");
    assert_eq!(stmts("async function f() { await a; }"), "(Function[async][decl] $f () (Block (ExprStmt (Await a))))");
    assert_eq!(stmts("async function* f() { for await (const x of y) ; }").len() > 0, true);
    assert_eq!(expr("(function () {})();"), "(Call (Function () (Block))^)");
    assert_eq!(expr("(function named() {});"), "(Function $named () (Block))^");
    assert_eq!(expr("(async function () {});"), "(Function[async] () (Block))^");
    assert_eq!(expr("(function ({a}, [b]) {});"), "(Function ((ObjectPattern (PatternProp[short] :a $a)) (ArrayPattern $b)) (Block))^");
    assert_err("function f(...a, b) {}", "rest");
    assert_err("function () {}", "");
    assert_err("function f(a,,) {}", "");
}

#[test]
fn contextual_await_and_yield() {
    assert_eq!(stmts("await a;"), "(ExprStmt (Await a))");
    assert_eq!(stmts("const x = await a + b;"), "(const (VarDeclarator $x (+ (Await a) b)))");
    assert_err("function f() { await a; }", "");
    assert_err("var await;", "reserved");
    assert_err("var yield;", "reserved");
    assert_err("function* g() { var yield; }", "reserved");
    assert_err("function* g() { function yield() {} }", "reserved");
    assert_err("var let;", "reserved");
    assert_err("var static;", "reserved");
    assert_err("await => 1;", "");
    assert_err("-await ** 2;", "");
}

#[test]
fn classes() {
    assert_eq!(stmts("class A {}"), "(Class[decl] $A)");
    assert_eq!(stmts("class A extends B {}"), "(Class[decl] $A B)");
    assert_eq!(stmts("class A extends B.c(d) {}"), "(Class[decl] $A (Call (Member B :c) d))");
    assert_eq!(
        stmts("class A { constructor() {} m() {} static s() {} get g() {} set g(v) {} async a() {} *gen() {} async *ag() {} }"),
        "(Class[decl] $A (MethodConstructor :constructor (Function () (Block))) (MethodMethod :m (Function () (Block))) (MethodMethod[static] :s (Function () (Block))) (MethodGet :g (Function () (Block))) (MethodSet :g (Function ($v) (Block))) (MethodMethod :a (Function[async] () (Block))) (MethodMethod :gen (Function[gen] () (Block))) (MethodMethod :ag (Function[async][gen] () (Block))))"
    );
    assert_eq!(stmts("class A { x = 1; y; static z = 2; #p = 3; [k] = 4; 'q' = 5 }"), "(Class[decl] $A (Field :x 1) (Field :y) (Field[static] :z 2) (Field #p 3) (Field[computed] k 4) (Field \"q\" 5))");
    assert_eq!(stmts("class A { static { a; } static {} }"), "(Class[decl] $A (StaticBlock[static]? (ExprStmt a)) (StaticBlock[static]?))".replace("[static]?", ""));
    assert_eq!(stmts("class A { static = 1; get = 2; set; async; static static() {} }").len() > 0, true);
    assert_eq!(stmts("class A { get\nx() {} }"), "(Class[decl] $A (MethodGet :x (Function () (Block))))");
    assert_eq!(stmts("class A { #x; m() { return #x in this; } }"), "(Class[decl] $A (Field #x) (MethodMethod :m (Function () (Block (Return (in #x this))))))");
    assert_eq!(stmts("class A { #x; #y; m() { this.#x; a?.#y; } }").len() > 0, true);
    assert_eq!(expr("(class {});"), "(Class)^");
    assert_eq!(expr("(class B extends C {});"), "(Class $B C)^");
    assert_eq!(stmts("class A { ; ; }"), "(Class[decl] $A)");
    assert_err("class {}", "");
    assert_err("class A { x = }", "");
    assert_err("class A { 1 2 }", "");
    assert_err("class A { foo bar }", "");
}

#[test]
fn class_bodies_are_strict() {
    assert_err("class A { m() { with (a) {} } }", "strict");
    assert_err("class A { m() { var static; } }", "reserved");
}

#[test]
fn imports() {
    assert_eq!(stmts("import 'm';"), "(Import \"m\")");
    assert_eq!(stmts("import a from 'm';"), "(Import \"m\" (ImportDefault $a))");
    assert_eq!(stmts("import * as ns from 'm';"), "(Import \"m\" (ImportNamespace $ns))");
    assert_eq!(stmts("import {a, b as c} from 'm';"), "(Import \"m\" (ImportSpecifier a $a) (ImportSpecifier b $c))");
    assert_eq!(stmts("import a, {b} from 'm';"), "(Import \"m\" (ImportDefault $a) (ImportSpecifier b $b))");
    assert_eq!(stmts("import a, * as ns from 'm';"), "(Import \"m\" (ImportDefault $a) (ImportNamespace $ns))");
    assert_eq!(stmts("import {default as d, 'str name' as s} from 'm';"), "(Import \"m\" (ImportSpecifier default $d) (ImportSpecifier str name $s))");
    assert_eq!(stmts("import {a,} from 'm';"), "(Import \"m\" (ImportSpecifier a $a))");
    assert_eq!(stmts("import {} from 'm';"), "(Import \"m\")");
    assert_eq!(stmts("import j from './a.json' with { type: 'json' };"), "(Import \"./a.json\" (ImportDefault $j) (ImportAttr type=\"json\"))");
    assert_eq!(stmts("import {from} from 'm';"), "(Import \"m\" (ImportSpecifier from $from))");
    assert_eq!(stmts("import from from 'm';"), "(Import \"m\" (ImportDefault $from))");
    assert_eq!(expr("import('x');"), "(ImportCall \"x\")");
    assert_eq!(expr("import('x', {with: {}});"), "(ImportCall \"x\" (Object (PropInit :with (Object))))");
    assert_eq!(expr("import.meta.url;"), "(Member import.meta :url)");
    assert_err("import {if} from 'm';", "");
    assert_err("import {'a'} from 'm';", "as");
    assert_err("import a from b;", "");
    assert_err("import;", "");
    assert_err("{ import a from 'm'; }", "top level");
    assert_err("function f() { import a from 'm'; }", "top level");
    assert_err("new import('x');", "");
}

#[test]
fn exports() {
    assert_eq!(stmts("export * from 'm';"), "(ExportAll from \"m\")");
    assert_eq!(stmts("export * as ns from 'm';"), "(ExportAll as ns from \"m\")");
    // Exported names must exist, so each list is preceded by declarations (the first statement).
    assert_eq!(stmts("var a, b; export {a, b as c};"), "(var (VarDeclarator $a) (VarDeclarator $b)) | (ExportNamed (ExportSpecifier a as a) (ExportSpecifier b as c))");
    assert_eq!(stmts("var a, b; export {a as default, b as 'x y'};"), "(var (VarDeclarator $a) (VarDeclarator $b)) | (ExportNamed (ExportSpecifier a as default) (ExportSpecifier b as x y))");
    assert_eq!(stmts("export {a} from 'm';"), "(ExportNamed from \"m\" (ExportSpecifier a as a))");
    assert_eq!(stmts("export {};"), "(ExportNamed)");
    assert_eq!(stmts("export const x = 1, y = 2;"), "(ExportNamed (const (VarDeclarator $x 1) (VarDeclarator $y 2)))");
    assert_eq!(stmts("export let z;"), "(ExportNamed (let (VarDeclarator $z)))");
    assert_eq!(stmts("export function f() {}"), "(ExportNamed (Function[decl] $f () (Block)))");
    assert_eq!(stmts("export async function f() {}"), "(ExportNamed (Function[async][decl] $f () (Block)))");
    assert_eq!(stmts("export class C {}"), "(ExportNamed (Class[decl] $C))");
    assert_eq!(stmts("export default function () {}"), "(ExportDefault (Function[decl] () (Block)))");
    assert_eq!(stmts("export default function f() {}"), "(ExportDefault (Function[decl] $f () (Block)))");
    assert_eq!(stmts("export default async function () {}"), "(ExportDefault (Function[async][decl] () (Block)))");
    assert_eq!(stmts("export default class {}"), "(ExportDefault (Class[decl]))");
    assert_eq!(stmts("export default class K extends B {}"), "(ExportDefault (Class[decl] $K B))");
    assert_eq!(stmts("export default 1 + 2;"), "(ExportDefault (+ 1 2))");
    assert_eq!(stmts("export default (a, b);"), "(ExportDefault (Sequence a b)^)");
    assert_eq!(stmts("export default async;").len() > 0, true);
    assert_err("export 1;", "declaration");
    assert_err("export default;", "");
    assert_err("{ export const a = 1; }", "top level");
    assert_err("export * from;", "");
}

#[test]
fn directives_and_strictness() {
    assert_eq!(stmts("'use strict'; a"), "(ExprStmt[directive] \"use strict\") | (ExprStmt a)");
    assert_eq!(stmts("'a'; 'b'; c"), "(ExprStmt[directive] \"a\") | (ExprStmt[directive] \"b\") | (ExprStmt c)");
    assert_eq!(stmts("a; 'not a directive'"), "(ExprStmt a) | (ExprStmt \"not a directive\")");
    assert_eq!(stmts("('paren');"), "(ExprStmt \"paren\"^)");
    assert_eq!(stmts("'a' + 'b'"), "(ExprStmt (+ \"a\" \"b\"))");
    assert_eq!(stmts("function f() { 'use strict'; return 1 }"), "(Function[decl] $f () (Block (ExprStmt[directive] \"use strict\") (Return 1)))");
}

#[test]
fn script_mode_is_sloppy() {
    let src = "var let_ = 1; with (a) {} var yield = 2; var await = 3; var static = 4; var x = 010;";
    script(src);
    script("return 1;");
    script("var let = 1;");
    script("let\nx = 1");
    script("function await() {} await(1);");
    let ast = script("'use strict'; a");
    assert_eq!(top_level(&ast).len(), 2);
    assert!(parse("'use strict'; with (a) {}", ParseOptions::new(false)).is_err());
    assert!(parse("'use strict'; var x = 010;", ParseOptions::new(false)).is_err());
    assert!(parse("import a from 'm';", ParseOptions::new(false)).is_err());
    assert!(parse("export const a = 1;", ParseOptions::new(false)).is_err());
    assert!(parse("import.meta", ParseOptions::new(false)).is_err());
    // Dynamic import is fine in scripts.
    script("import('x').then(f);");
}

#[test]
fn strict_mode_checks() {
    assert_err("010;", "octal");
    assert_err("089;", "octal");
    assert_err(&esc("'~01';"), "octal");
    assert_err("delete a;", "strict");
    assert_err("eval = 1;", "strict");
    assert_err("arguments++;", "strict");
    assert_err("var eval;", "strict");
    assert_err("function f(eval) {}", "strict");
    assert_eq!(expr("delete a.b;"), "(udelete (Member a :b))");
    assert_eq!(expr("delete (a, b);").len() > 0, true);
}

#[test]
fn escaped_names() {
    assert_eq!(expr(&esc("~u0061bc;")), "abc");
    assert_eq!(expr(&esc("a.~u0062;")), "(Member a :b)");
    assert_eq!(expr(&esc("({~u0061: 1});")), "(Object (PropInit :a 1))^");
    assert_err(&esc("~u0069f (a) {}"), "");
    assert_err(&esc("var ~u0069f;"), "");
}

#[test]
fn regex_after_close_paren_and_brace() {
    // Division vs regex is decided by the grammar, not by guessing from the previous token.
    assert_eq!(stmts("if (a) /re/.test(b);"), "(If a (ExprStmt (Call (Member /re/ :test) b)))");
    assert_eq!(stmts("{}\n/re/.test(a)"), "(Block) | (ExprStmt (Call (Member /re/ :test) a))");
    assert_eq!(stmts("a = (b) / c / d;"), "(ExprStmt (= a (/ (/ b^ c) d)))");
    assert_eq!(stmts("x = a++ / 2;"), "(ExprStmt (= x (/ (UpdateInc a) 2)))");
    assert_eq!(stmts("function f() {}\n/re/.test(a)"), "(Function[decl] $f () (Block)) | (ExprStmt (Call (Member /re/ :test) a))");
}

#[test]
fn spans_cover_the_source() {
    let src = "const x = foo(1, 2);";
    let ast = module(src);
    let stmt = top_level(&ast)[0];
    assert_eq!((ast[stmt].span.start, ast[stmt].span.end), (0, src.len() as u32));
    let NodeKind::VarDecl { decls, .. } = ast[stmt].kind else { panic!() };
    let declarator = ast.list(decls)[0];
    let NodeKind::VarDeclarator { init: Some(call), .. } = ast[declarator].kind else { panic!() };
    let span = ast[call].span;
    assert_eq!(&src[span.start as usize..span.end as usize], "foo(1, 2)");
}

#[test]
fn errors_report_positions() {
    let e = err("let a = ;");
    assert_eq!(e.pos, 8);
    assert!(e.message.contains("Unexpected token"));
    let e = err("a +");
    assert!(e.message.contains("end of input"));
    let e = err("x = 'unterminated");
    assert!(e.message.contains("unterminated string"));
    assert_eq!(e.pos, 4);
}

#[test]
fn hashbang_and_comments() {
    assert_eq!(stmts("#!/usr/bin/env node\na"), "(ExprStmt a)");
    assert_eq!(stmts("/* c */ a // d\n/* e */ b"), "(ExprStmt a) | (ExprStmt b)");
}

#[test]
fn empty_and_whitespace_only() {
    assert_eq!(stmts(""), "");
    assert_eq!(stmts("   \n\t "), "");
    assert_eq!(stmts(";"), "(Empty)");
}

#[test]
fn deeply_nested_input_does_not_overflow_for_realistic_depth() {
    // Real code nests a few dozen levels at most; make sure a few hundred is fine.
    let src = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    module(&format!("{src};"));
    let src = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    module(&format!("{src};"));
}

#[test]
fn call_expressions_as_assignment_targets_match_engines() {
    // Specified as early errors, but every engine accepts these and throws at run time.
    for ok in ["f() = 1;", "f()++;", "++f();", "f() += 1;", "for (f() of x);", "for (f() in x);", "(a) = 1;", "(a.b) = 1;"] {
        module(ok);
    }
    for bad in ["f() &&= 1;", "f() ||= 1;", "f() ??= 1;", "[f()] = 1;", "({a: f()} = 1);", "[...f()] = 1;", "(a, b) = 1;", "this = 1;", "1++;"] {
        assert_err(bad, "Invalid");
    }
}

#[test]
fn arrow_functions_cannot_be_operands() {
    for bad in ["a & x => 1;", "a || x => 1;", "!x => 1;", "new x => 1;", "async function f() { await x => 1; }", "typeof x => 1;"] {
        assert_err(bad, "Malformed arrow");
    }
    for ok in ["a ? b : x => 1;", "f(x => 1);", "x = y => z;", "[x => 1];", "({a: x => 1});", "a ? x => 1 : y => 2;", "(x => 1) & 2;", "(x => 1)();"] {
        module(ok);
    }
}

#[test]
fn super_is_context_checked() {
    module("class A extends B { constructor() { super(); super.x; } m() { super.m(); } static s() { super.s; } x = super.y; static { super.z; } }");
    module("class A extends B { constructor() { (() => super())(); } }");
    module("({ m() { super.x; }, get g() { return super.y; } });");
    module("class A extends B { m() { return () => { return () => super.m(); }; } }");
    for bad in [
        "super.x;",
        "function f() { super.x; }",
        "class A { constructor() { super(); } }",
        "class A extends B { m() { super(); } }",
        "class A extends B { constructor() { function f() { super(); } } }",
        "class A extends B { x = super(); }",
        "({ m: function () { super.x; } });",
        "super;",
    ] {
        assert_err(bad, "super");
    }
}

#[test]
fn new_target_is_context_checked() {
    module("function f() { return new.target; }");
    module("function f() { return () => new.target; }");
    module("class A { x = new.target; static { new.target; } }");
    assert_err("new.target;", "new.target");
    assert_err("() => new.target;", "new.target");
    script("new.target;");
}

#[test]
fn private_names_must_be_declared() {
    module("class A { #x; m() { return this.#x; } }");
    module("class A { #x; m() { return #x in this; } }");
    module("class A { m() { return this.#x; } #x; }");
    module("class A { #x; m() { class B { n() { return this.#x; } } } }");
    module("class A { get #x() {} set #x(v) {} }");
    module("class A { static get #x() {} static set #x(v) {} }");
    module("class A { #x() {} static m(o) { o.#x; } }");
    for bad in [
        "class A { m() { return this.#y; } }",
        "this.#x;",
        "#x in a;",
        "class A { m() { return #x in this; } }",
        "class A { #x; } class B { m() { this.#x; } }",
        "class A extends (this.#x) { #x; }",
    ] {
        assert_err(bad, "must be declared");
    }
    for bad in [
        "class A { #x; #x; }",
        "class A { get #x() {} get #x() {} }",
        "class A { static get #x() {} set #x(v) {} }",
        "class A { #x; m() {} #x() {} }",
    ] {
        assert_err(bad, "already declared");
    }
}

fn assert_script_err(src: &str, needle: &str) {
    match parse(src, ParseOptions::new(false)) {
        Ok(_) => panic!("{src:?} should not parse as a script"),
        Err(e) => assert!(e.message.contains(needle), "{src:?}: expected {needle:?}, got {:?}", e.message),
    }
}

#[test]
fn let_cannot_name_a_lexical_binding() {
    for bad in ["let let = 1;", "const let = 1;", "let [let] = a;", "let {let} = a;", "let {a: let} = b;"] {
        assert_script_err(bad, "let");
    }
    script("var let = 1;");
    script("let\nx = 1;");
}

#[test]
fn function_expression_names_use_their_own_rules() {
    assert_err("(function* yield() {});", "reserved");
    assert_script_err("(function* yield() {});", "reserved");
    assert_err("(async function await() {});", "reserved");
    assert_script_err("(async function await() {});", "reserved");
    assert_script_err("(async function* await() {});", "reserved");
    script("(function yield() {});");
    script("(function await() {});");
    script("function* g() { (function yield() {}); }");
}

#[test]
fn class_static_blocks_and_field_initializers() {
    assert_err("class A { static { await } }", "await");
    assert_err("class A { static { var await } }", "await");
    assert_script_err("class A { static { await } }", "await");
    assert_err("class A { static { return } }", "return");
    assert_err("class A { x = arguments }", "arguments");
    assert_err("class A { static { arguments } }", "arguments");
    assert_err("class A { x = () => arguments }", "arguments");
    module("class A { m() { arguments } }");
    module("class A { x = function () { arguments } }");
    module("class A { static { (function () { return arguments; })(); } }");
}

#[test]
fn parameter_rules() {
    assert_err("function f(a, a) {}", "Duplicate");
    script("function f(a, a) {}");
    assert_script_err("function f(a, a) { 'use strict' }", "Duplicate");
    assert_script_err("function f(a = 1, a) {}", "Duplicate");
    assert_script_err("function f([a], a) {}", "Duplicate");
    assert_script_err("(a, a) => 1;", "Duplicate");
    assert_script_err("({ m(a, a) {} });", "Duplicate");
    assert_script_err("class A { m(a, a) {} }", "Duplicate");
    module("function f(a, b) {}");
    assert_err("function f(a = 1) { 'use strict' }", "non-simple");
    assert_err("function f({a}) { 'use strict' }", "non-simple");
    assert_err("function f(...a) { 'use strict' }", "non-simple");
    assert_err("(a = 1) => { 'use strict' }", "non-simple");
    module("function f(a) { 'use strict' }");
    module("(a) => { 'use strict' }");
    module("(a = 1) => { function g() { 'use strict' } }");
    module("(a = 1) => 'use strict';");
    assert_err("async function f(a = await 1) {}", "formal parameters");
    assert_err("function* f(a = yield) {}", "formal parameters");
    assert_err("async (a = await 1) => a;", "arrow function parameters");
    assert_err("function* g() { (a = yield) => 1; }", "arrow function parameters");
    module("async function f(a = async () => await 1) {}");
    module("async (a = async () => await 1) => a;");
    module("function* g() { function f(a = function* () { yield 1; }) {} }");
}

#[test]
fn duplicate_proto_and_cover_errors() {
    assert_err("({ __proto__: 1, __proto__: 2 });", "__proto__");
    assert_err("({ '__proto__': 1, __proto__: 2 });", "__proto__");
    module("({ __proto__: a, __proto__: b } = c);");
    module("({ __proto__: 1, ['__proto__']: 2 });");
    module("({ __proto__, __proto__ });");
    module("({ __proto__() {}, __proto__: 1 });");
    module("({ __proto__: 1, __proto__() {} });");
    assert_err("f({ __proto__: 1, __proto__: 2 });", "__proto__");
}

#[test]
fn delete_and_rest_restrictions() {
    assert_err("class A { #a; m() { delete this.#a } }", "cannot be deleted");
    assert_err("class A { #a; m() { delete this?.#a } }", "cannot be deleted");
    assert_err("class A { #a; m() { delete (this.#a) } }", "cannot be deleted");
    module("class A { #a; m() { delete this.a } }");
    assert_err("[...a,] = b;", "rest");
    assert_err("({...a,} = b);", "rest");
    assert_err("(...a,) => 1;", "rest");
    assert_err("async (...a,) => 1;", "rest");
    module("[...a,];");
    module("({...a,});");
    module("f(...a,);");
}

#[test]
fn class_member_rules() {
    assert_err("class A { constructor() {} constructor() {} }", "one constructor");
    assert_err("class A { constructor() {} 'constructor'() {} }", "one constructor");
    assert_err("class A { get constructor() {} }", "accessor");
    assert_err("class A { set constructor(v) {} }", "accessor");
    assert_err("class A { *constructor() {} }", "generator");
    assert_err("class A { async constructor() {} }", "generator or async");
    assert_err("class A { constructor = 1 }", "field named");
    assert_err("class A { 'constructor' = 1 }", "field named");
    assert_err("class A { static constructor = 1 }", "field named");
    assert_err("class A { static prototype() {} }", "prototype");
    assert_err("class A { static prototype = 1 }", "prototype");
    assert_err("class A { static get prototype() {} }", "prototype");
    assert_err("class A { #constructor }", "#constructor");
    module("class A { static constructor() {} constructor() {} }");
    module("class A { prototype() {} }");
    module("class A { ['constructor']() {} ['constructor']() {} }");
    module("class A { static ['prototype']() {} }");
    module("class A { m() {} } class B { constructor() {} }");
}

#[test]
fn accessor_parameter_counts() {
    for bad in ["({ get a(v) {} });", "({ set a() {} });", "({ set a(...v) {} });", "({ set a(v, w) {} });", "class A { get a(v) {} }", "class A { set a() {} }", "class A { static set a(...v) {} }"] {
        assert_err(bad, "");
    }
    module("({ get a() {}, set a(v) {} });");
    module("({ set a({b}) {} });");
    module("({ set a(v = 1) {} });");
    module("class A { get a() {} set a(v) {} }");
}

#[test]
fn export_name_rules() {
    assert_err("export {a, a};", "Duplicate");
    assert_err("export {a as b, c as b};", "Duplicate");
    assert_err("export default 1; export default 2;", "Duplicate");
    assert_err("export var a; export {a};", "Duplicate");
    assert_err("export function f() {} export {f};", "Duplicate");
    assert_err("export * as ns from 'a'; export * as ns from 'b';", "Duplicate");
    assert_err("export {if};", "reserved");
    assert_err("export {'a'};", "from");
    assert_err("export {a as 'b'}; export {c as 'b'};", "Duplicate");
    module("export {if} from 'x';");
    module("export {'a'} from 'x';");
    module("export {a as b} from 'x'; export {a as c} from 'x';");
    module("export * from 'a'; export * from 'b';");
    module("export var a; export default a; export {a as b};");
    module("export var [a, {b}] = c;");
    assert_err("export var [a, {b}] = c; export {b};", "Duplicate");
}

#[test]
fn import_attribute_rules() {
    assert_err("import 'a' with {type: 'json', type: 'css'};", "Duplicate");
    assert_err("import 'a' assert {type: 'json'};", "");
    module("import 'a' with {type: 'json', other: 'x'};");
}

#[test]
fn private_accessor_pairs() {
    assert_err("class A { get #a() {} set #a(v) {} get #a() {} }", "already declared");
    assert_err("class A { get #a() {} set #a(v) {} set #a(v) {} }", "already declared");
    module("class A { set #a(v) {} get #a() {} }");
}

#[test]
fn annex_b_forms_are_script_only() {
    script("if (a) function f() {}");
    script("if (a) function f() {} else function g() {}");
    assert_err("if (a) function f() {}", "position");
    assert_script_err("if (a) function* f() {}", "Generators");
    assert_script_err("'use strict'; if (a) function f() {}", "position");
    script("for (var i = 1 in a) {}");
    assert_err("for (var i = 1 in a) {}", "initializer");
    assert_script_err("for (let i = 1 in a) {}", "initializer");
    assert_script_err("for (var [i] = 1 in a) {}", "initializer");
    assert_script_err("for (var i = 1 of a) {}", "initializer");
    assert_script_err("'use strict'; for (var i = 1 in a) {}", "initializer");
}

#[test]
fn html_like_comments_are_script_only() {
    script("<!-- comment\nx");
    script("x\n--> comment\ny");
    script("--> comment at the very start\nx");
    script("/* c */ --> comment after a block comment\nx");
    script("x <!-- comment");
    // `-->` mid-line is a decrement and a comparison, not a comment.
    script("a --> b");
    assert!(parse("<!-- comment\nx", ParseOptions::new(true)).is_err());
    assert!(parse("x\n--> comment", ParseOptions::new(true)).is_err());
    let ast = script("<!-- c\nx\n--> c\ny");
    assert_eq!(top_level(&ast).len(), 2);
}

#[test]
fn regressions_found_by_differential_fuzzing() {
    // `from` is mandatory after a default import.
    assert_err("import a 'b';", "from");
    assert_err("import a, {b} 'c';", "from");
    assert_err("import * as a 'b';", "from");
    assert_err("import {a} 'b';", "from");
    // Legacy octal keys are strict-mode errors just like legacy octal values.
    assert_err("({ 020: 1 });", "octal");
    assert_err("class A { 08() {} }", "octal");
    script("({ 020: 1 });");
    // An arrow function cannot be a class heritage.
    assert_err("class A extends x => y {}", "Malformed arrow");
    assert_err("(class extends x => y {});", "Malformed arrow");
}

#[test]
fn return_inside_functions_nested_in_static_blocks() {
    module("class A { static { (() => { return 1; })(); } }");
    module("class A { static { (function () { return 1; })(); } }");
    module("class A { static { ({ m() { return 1; } }); } }");
    module("class A { static { class B { m() { return 1; } } } }");
    assert_err("class A { static { return 1; } }", "return");
    assert_err("class A { static { if (a) { return 1; } } }", "return");
    // `await` stays reserved in arrows inside a static block, but not in plain functions.
    assert_err("class A { static { (() => await)(); } }", "await");
    script("class A { static { (function () { var await; })(); } }");
}

#[test]
fn fragments_are_pieces_of_a_larger_file() {
    let fragment = ParseOptions::new(true).as_fragment();
    // A name exported here may be declared elsewhere, and a wrapper function may end with `return`.
    assert!(parse("export { a, b as c };", fragment).is_ok());
    assert!(parse("return a;", fragment).is_ok());
    assert!(parse("export { a };", ParseOptions::new(true)).is_err());
    assert!(parse("return a;", ParseOptions::new(true)).is_err());
}

#[test]
fn the_end_of_the_text_can_end_a_statement_without_a_semicolon() {
    let end = |src: &str| parse(src, ParseOptions::new(true)).unwrap().eof_asi;
    assert!(end("a = 1"));
    assert!(end("a = 1\n"));
    assert!(end("if (a) b = 1"));
    assert!(!end("a = 1;"));
    assert!(!end("function f() {}"));
    assert!(!end("if (a) { b = 1 }"));
}
