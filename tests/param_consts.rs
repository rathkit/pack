//! A parameter that every call leaves out is `undefined` in the function, and a program that has it that way prints what it printed: the
//! functions here are called as written and as bundled with `--minify`, and the output is compared.

use std::process::Command;

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-params-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        std::fs::write(dir.join(path), text).unwrap();
    }
    dir
}

fn node(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("node").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn bundle(dir: &std::path::Path, entry: &str) -> String {
    let run = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).args(["bundle", entry, "--minify", "--format=esm", "--outfile", "o.mjs"]).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    std::fs::read_to_string(dir.join("o.mjs")).unwrap()
}

const LIB: &str = "
export function a(x, options) { return options?.k || x; }
export function b(x, options) { return options?.k || x; }
export function c(x, options = { k: 7 }) { return options.k || x; }
export function d(x, options) { return arguments.length + (options?.k || x); }
export function e(x, options) { return options ? 'given' : 'none'; }
export function f(x, o) { return o === undefined ? 'u' : 'v'; }
export function g(x, o) { return typeof o + (o ?? 'nullish'); }
export function h(x, o) { o = o || { k: 'set' }; return o.k + x; }
export function i(x, o) { return (o && o.k) || (o ?? 'fallback') || x; }
export function j(x, o, p) { return j2(x, o) + (p === null ? 'n' : 'm'); }
function j2(x, o) { return o?.deep ?? x; }
";

#[test]
fn a_parameter_that_no_call_passes_is_undefined_and_the_program_prints_the_same() {
    let main = "
import { a, b, c, d, e, f, g, h, i, j } from './lib.js';
console.log(a(1), a(2), b(1), b(1, { k: 5 }), c(1), c(1, { k: 0 }), d(1), e(1), [1, 2].map(e).join(','), f(1), f(2, undefined), f(3, null), g(1), h(1), h(2, { k: 'x' }), i(1), j(1, undefined, null), j(2, { deep: 'd' }, 1));
";
    let dir = project("same", &[("lib.js", LIB), ("main.js", main)]);
    // As written (Node reads both files as modules).
    std::fs::write(dir.join("package.json"), "{\"type\":\"module\"}").unwrap();
    let expected = node(&dir, &["main.js"]);
    let out = bundle(&dir, "main.js");
    assert_eq!(node(&dir, &["o.mjs"]), expected);
    // `a` is called twice with one argument: its `options?.k` is gone. `b` has a call that passes the options, and `d` reads `arguments`.
    assert_eq!(out.matches("?.k").count(), 2, "{out}");
}

#[test]
fn a_function_that_leaves_the_bundle_keeps_its_parameters() {
    let dir = project("exported", &[("lib.js", LIB), ("main.js", "export * from './lib.js';\nimport { a } from './lib.js';\nconsole.log(a(1));\n"), ("package.json", "{\"type\":\"module\"}")]);
    bundle(&dir, "main.js");
    let out = node(&dir, &["--input-type=module", "-e", "import * as m from './o.mjs'; console.log(m.a(1, { k: 'given' }), m.f(1, 'x'), m.g(1, 5))"]);
    assert_eq!(out, "1\ngiven v number5\n");
}

#[test]
fn a_function_used_as_a_value_or_called_with_a_spread_keeps_its_parameters() {
    let lib = "export function k(x, o) { return o?.v || x; }\nexport function s(x, o) { return o?.v || x; }\n";
    let main = "import { k, s } from './lib.js';\nconst args = [1, { v: 'spread' }];\nconsole.log([[1, { v: 'cb' }]].map((p) => k(...p)).join(), s(...args), [3].map(k).join());\n";
    let dir = project("value", &[("lib.js", lib), ("main.js", main), ("package.json", "{\"type\":\"module\"}")]);
    let expected = node(&dir, &["main.js"]);
    let out = bundle(&dir, "main.js");
    assert_eq!(node(&dir, &["o.mjs"]), expected);
    assert_eq!(out.matches("?.v").count(), 2, "{out}");
}

#[test]
fn a_default_export_called_under_another_name_counts() {
    // `import add from './add.js'` is the same function: a call that passes the second argument there is a call.
    let add = "export default function add(x, o) { return o?.by ?? x + 1; }\n";
    let main = "import add from './add.js';\nimport { add as named } from './named.js';\nconsole.log(add(1), add(1, { by: 10 }), named(2));\n";
    let named = "import add from './add.js';\nexport { add };\n";
    let dir = project("default", &[("add.js", add), ("main.js", main), ("named.js", named), ("package.json", "{\"type\":\"module\"}")]);
    let expected = node(&dir, &["main.js"]);
    let out = bundle(&dir, "main.js");
    assert_eq!(node(&dir, &["o.mjs"]), expected);
    assert!(out.contains("?.by"), "{out}");
}
