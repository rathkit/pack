//! A name declared inside a function that nothing reads goes, with what only it used; every program here is run in Node as written and as
//! minified and must print the same lines. `GONE` strings must not be in the output, `KEPT` strings must be.

use std::process::Command;

struct Case {
    name: &'static str,
    source: &'static str,
    gone: &'static [&'static str],
    kept: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "an unused const, let, var, function and class",
        source: "function f(x) { const a = 'GONE_a'; let b = 'GONE_b'; var c = 'GONE_c'; function d() { return 'GONE_d'; } class E { m() { return 'GONE_e'; } } return x + 1; }\nconsole.log(f(1));",
        gone: &["GONE_a", "GONE_b", "GONE_c", "GONE_d", "GONE_e"],
        kept: &[],
    },
    Case {
        name: "a chain where each name only feeds the next",
        source: "function f(x) { const a = 'GONE_a'; const b = [a]; const c = { b }; function g() { return c; } return x; }\nconsole.log(f(7));",
        gone: &["GONE_a"],
        kept: &[],
    },
    Case {
        name: "a name that is read stays",
        source: "function f() { const a = 'KEPT_a'; const b = 'GONE_b'; return a; }\nconsole.log(f());",
        gone: &["GONE_b"],
        kept: &["KEPT_a"],
    },
    Case {
        name: "a name read by a closure stays",
        source: "function f() { const a = 'KEPT_a'; return () => a; }\nconsole.log(f()());",
        gone: &[],
        kept: &["KEPT_a"],
    },
    Case {
        name: "a name that is assigned later stays (the assignment would lose its declaration)",
        source: "function f() { let q = 'KEPT_q'; q = 'KEPT_r'; return 1; }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_q"],
    },
    Case {
        name: "an initializer with an effect stays",
        source: "const log = [];\nfunction f() { const a = log.push('KEPT_a'); let b = log.push('KEPT_b'), c = 'GONE_c'; return 1; }\nconsole.log(f(), log.length);",
        gone: &["GONE_c"],
        kept: &["KEPT_a", "KEPT_b"],
    },
    Case {
        name: "one name of a declaration with several",
        source: "function f() { let a = 'GONE_a', b = 'KEPT_b', c = 'GONE_c'; return b; }\nconsole.log(f());",
        gone: &["GONE_a", "GONE_c"],
        kept: &["KEPT_b"],
    },
    Case {
        name: "a var that is declared twice stays",
        source: "function f() { var a = 'KEPT_a'; var a = 'KEPT_b'; return 1; }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_a", "KEPT_b"],
    },
    Case {
        name: "a direct eval can read any name",
        source: "function f() { const a = 'KEPT_a'; return eval('a'); }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_a"],
    },
    Case {
        name: "a function that calls itself is read by itself and stays",
        source: "function f(n) { function r(k) { return k ? r(k - 1) : 'KEPT_r'; } return n; }\nconsole.log(f(1));",
        gone: &[],
        kept: &["KEPT_r"],
    },
    Case {
        name: "names in a switch case and a block",
        source: "function f(x) { switch (x) { case 1: { const a = 'GONE_a'; let b = 'GONE_b'; return 'one'; } default: let c = 'GONE_c'; return 'other'; } }\nconsole.log(f(1), f(2));",
        gone: &["GONE_a", "GONE_b", "GONE_c"],
        kept: &[],
    },
    Case {
        name: "a statement that is the body of an if is not a list and stays",
        source: "function f(x) { if (x) var a = 'GONE_a'; else var b = 'GONE_b'; return 1; }\nconsole.log(f(1), f(0));",
        gone: &[],
        kept: &[],
    },
    Case {
        name: "a name read only in the removed code goes with it, one read elsewhere does not",
        source: "function f() { const keep = 'KEPT_k'; const drop = 'GONE_d'; function use() { return keep + drop; } const alsoKeep = 'KEPT_a'; return alsoKeep + keep; }\nconsole.log(f());",
        gone: &["GONE_d"],
        kept: &["KEPT_k", "KEPT_a"],
    },
    Case {
        name: "a class with a static block or computed key has effects and stays",
        source: "const out = [];\nfunction f() { class A { static { out.push('KEPT_s'); } } class B { [out.push('KEPT_c')] = 1; } return out.length; }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_s", "KEPT_c"],
    },
    Case {
        name: "an arrow function body and a method body",
        source: "const g = (x) => { const a = 'GONE_a'; return x * 2; };\nconst o = { m(x) { let b = 'GONE_b'; return x + 1; } };\nconsole.log(g(2), o.m(2));",
        gone: &["GONE_a", "GONE_b"],
        kept: &[],
    },
    Case {
        name: "a name used by typeof and by delete-free reads stays",
        source: "function f() { const a = 'KEPT_a'; return typeof a; }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_a"],
    },
    Case {
        name: "a name with one value decides the branches it is read in and goes with them",
        source: "function f(x, options) { const mode = 'fast'; const flag = null; const level = 3; if (mode === 'slow') { x = 'GONE_slow' + x; } flag && x.push('GONE_flag'); if (level > 2) { x = x + 'KEPT_level'; } return x; }\nconsole.log(f('a'));",
        gone: &["GONE_slow", "GONE_flag", "fast"],
        kept: &["KEPT_level"],
    },
    Case {
        name: "a name read by a function declaration that can run before it is declared is not taken for its value",
        source: "function f() { const out = [g()]; var h = 'KEPT_h'; function g() { return h; } out.push(g()); return out.join(); }\nconsole.log(f());",
        gone: &[],
        kept: &["KEPT_h"],
    },
    Case {
        name: "a shadowing name does not keep the outer one",
        source: "function f() { const a = 'GONE_a'; { const a = 'KEPT_inner'; console.log(a); } return 1; }\nconsole.log(f());",
        gone: &["GONE_a"],
        kept: &["KEPT_inner"],
    },
];

fn run(dir: &std::path::Path, file: &str) -> String {
    let out = Command::new("node").current_dir(dir).arg(file).output().unwrap();
    assert!(out.status.success(), "{file}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn unused_locals_go_and_the_program_prints_the_same() {
    let root = std::env::temp_dir().join(format!("rkp-unused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (i, case) in CASES.iter().enumerate() {
        let dir = root.join(i.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        // The cases are sloppy scripts (`arguments` aliasing, `eval`): Node runs them as CommonJS.
        std::fs::write(dir.join("a.cjs"), case.source).unwrap();
        let pack = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.cjs", "--minify", "--format=cjs", "--outfile", "o.cjs"]).output().unwrap();
        assert!(pack.status.success(), "{}: {}", case.name, String::from_utf8_lossy(&pack.stderr));
        let (expected, got) = (run(&dir, "a.cjs"), run(&dir, "o.cjs"));
        assert_eq!(expected, got, "{}", case.name);
        let output = std::fs::read_to_string(dir.join("o.cjs")).unwrap();
        for g in case.gone {
            assert!(!output.contains(g), "{}: `{g}` is still in\n{output}", case.name);
        }
        for k in case.kept {
            assert!(output.contains(k), "{}: `{k}` is gone from\n{output}", case.name);
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}
