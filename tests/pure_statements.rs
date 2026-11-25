//! A declaration that is left out because nothing reads it leaves nothing behind when what it was set to has no effect: an array written
//! with `...` of an array of the module, arithmetic on constants, a template of strings.

use std::process::Command;

fn minified(name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("rkp-pure-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.js"), source).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.js", "--minify", "--format=esm", "--outfile", "o.js"]).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    std::fs::read_to_string(dir.join("o.js")).unwrap()
}

#[test]
fn a_spread_of_an_array_of_the_module_that_nothing_reads_is_gone() {
    let out = minified("spread", "const U = ['a', 'b'];\nconst V = [...U, 'c'];\nconst W = [...V, 'd'];\nexport const f = (e) => U.includes(e);\n");
    assert!(!out.contains("..."), "{out}");
}

#[test]
fn a_spread_of_anything_else_stays() {
    // `g()` may return anything, and a function call can do anything: both keep their code.
    let out = minified("kept", "const U = g();\nconst V = [...U, 'c'];\nlet n = 0;\nconst W = [...[1, 2], n++];\nexport const f = () => 1;\n");
    assert!(out.contains("...") && out.contains("++"), "{out}");
}

#[test]
fn bitwise_arithmetic_on_constants_that_nothing_reads_is_gone() {
    let out = minified("bits", "const A = 1 << 1, B = 1 << 2;\nconst MASK = ~(A | B);\nexport const f = () => A;\n");
    assert!(!out.contains('~'), "{out}");
}
