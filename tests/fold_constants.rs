//! An expression of constants is written as the value it has, and the program prints what it printed: every expression here is run in Node as
//! written and as minified, and the lines are compared.

use std::process::Command;

const EXPRESSIONS: &[&str] = &[
    "1 << 4", "1 << 31", "1 << 32", "-1 >>> 0", "-1 >> 1", "-1 >>> 28", "5 | 2", "5 & 3", "5 ^ 1", "~5", "~-1", "~4294967296", "2147483648 | 0", "4294967297 | 0", "1.9 | 0", "-1.9 | 0",
    "2 * 3 + 1", "10 / 4", "10 / 3", "1 / 0", "-1 / 0", "0 / 0", "7 % 4", "-7 % 4", "7 % -4", "5.5 % 2", "2 ** 5", "2 ** -1", "(-2) ** 3", "0 * -1", "1e21 + 1", "1e21 * 10",
    "0.1 + 0.2", "9007199254740993 + 1", "'a' + 'b'", "'a' + 1", "1 + 'a'", "1 + 2 + 'a'", "'a' + 1 + 2", "'a' + 1.5", "'a' + true", "'a' + null", "'a' + undefined", "'a' + -0",
    "`x${'y'}z`", "`n${1}`", "`n${1.5}`", "`${true}${null}${undefined}`", "`a\\nb${'c'}`", "`${'a'}${'b'}`", "'it\\'s' + '\"q\"'", "'\\u2028' + 'x'", "'\\0' + '1'", "'\\x7f' + 'é'",
    "typeof 1", "typeof 'a'", "typeof undefined", "typeof null", "!1", "!!'a'", "!0 + 1", "-(-3)", "- -3", "+3", "+'3'", "void 0 === undefined", "null ?? 7", "0 || 5", "1 && 2", "0 && f()",
    "void 0 || ('p') || 1", "(void 0 && 1) || (void 0 ?? 'q') || f()", "0 || (0 ?? 'z') || 5", "(null ?? 0) || 6", "1 && (2 && 3)", "0 && (f() || f())", "undefined ?? (undefined ?? 'r')",
    "3 > 2", "3 >= 3", "2 < 1", "1 <= 1", "'a' === 'a'", "1 == '1'", "null == undefined", "NaN === NaN", "true ? 1 : 2", "false ? 1 : 2 + 3", "1 - 2", "1 - -1", "2 - 1 - 1",
];

fn run(dir: &std::path::Path, file: &str) -> String {
    let out = Command::new("node").current_dir(dir).arg(file).output().unwrap();
    assert!(out.status.success(), "{file}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn constant_expressions_give_the_same_values_when_folded() {
    let dir = std::env::temp_dir().join(format!("rkp-fold-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut source = String::from("const f = () => { throw new Error('ran'); };\n");
    for e in EXPRESSIONS {
        source.push_str(&format!("console.log(JSON.stringify([{e}], (k, v) => (typeof v === 'number' && !Number.isFinite(v) ? String(v) : Object.is(v, -0) ? '-0' : v)));\n"));
    }
    std::fs::write(dir.join("a.js"), &source).unwrap();
    let run_pack = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.js", "--minify", "--format=esm", "--outfile", "o.mjs"]).output().unwrap();
    assert!(run_pack.status.success(), "{}", String::from_utf8_lossy(&run_pack.stderr));
    let expected = run(&dir, "a.js");
    let got = run(&dir, "o.mjs");
    for (i, (a, b)) in expected.lines().zip(got.lines()).enumerate() {
        assert_eq!(a, b, "`{}`", EXPRESSIONS[i]);
    }
    assert_eq!(expected.lines().count(), got.lines().count());
    // The ones that are shorter as a value are folded.
    let minified = std::fs::read_to_string(dir.join("o.mjs")).unwrap();
    for gone in ["1<<4", "'a'+'b'", "2**5", "2*3+1", "5|2"] {
        assert!(!minified.contains(gone), "{gone} is still there: {minified}");
    }
}

#[test]
fn a_tagged_template_is_not_folded() {
    let dir = std::env::temp_dir().join(format!("rkp-fold-tag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.js"), "const t = (s, ...v) => s.raw.join('|') + v.join(',');\nconsole.log(t`a${1}b${2}c`, t`x`);\n").unwrap();
    let run_pack = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.js", "--minify", "--format=esm", "--outfile", "o.mjs"]).output().unwrap();
    assert!(run_pack.status.success());
    assert_eq!(run(&dir, "a.js"), run(&dir, "o.mjs"));
}

#[test]
fn a_sum_of_many_terms_is_built_and_not_a_stack_overflow() {
    // 50,000 terms are a tree that deep; the evaluator gives up past a few levels instead of walking it (the worst-case star graph did this).
    let dir = std::env::temp_dir().join(format!("rkp-fold-deep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let terms: Vec<String> = (0..50000).map(|i| (i % 7).to_string()).collect();
    std::fs::write(dir.join("a.js"), format!("console.log({});\n", terms.join(" + "))).unwrap();
    let run_pack = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.js", "--minify", "--format=esm", "--outfile", "o.mjs"]).output().unwrap();
    assert!(run_pack.status.success(), "{}", String::from_utf8_lossy(&run_pack.stderr));
    assert_eq!(run(&dir, "a.js"), run(&dir, "o.mjs"));
}
