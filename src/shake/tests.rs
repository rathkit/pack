use super::*;
use crate::graph::{GraphOptions, build_with_fs};
use crate::link::link;
use crate::resolve::MemFs;
use std::path::Path;

struct Run {
    graph: Graph,
    shaken: Shaken,
}

fn run(files: &[(&str, &str)]) -> Run {
    let mut fs = MemFs::new();
    for (path, text) in files {
        fs = fs.with(&format!("/p/{path}"), text);
    }
    let graph = build_with_fs(Box::new(fs), Path::new("/p"), &["main.js".to_string()], &GraphOptions { threads: 1, ..GraphOptions::default() });
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    let linked = link(&graph);
    assert!(!linked.has_errors(), "{:?}", linked.diagnostics);
    let shaken = shake(&graph, &linked);
    Run { graph, shaken }
}

impl Run {
    fn index(&self, file: &str) -> usize {
        self.graph.modules.iter().position(|m| m.path == Path::new("/p").join(file)).unwrap_or_else(|| panic!("no module {file}"))
    }

    fn included(&self, file: &str) -> bool {
        self.graph.modules.iter().position(|m| m.path == Path::new("/p").join(file)).is_some_and(|i| self.shaken.modules[i].included)
    }

    /// Names declared by the live statements of `file`, in source order.
    fn live(&self, file: &str) -> Vec<String> {
        let i = self.index(file);
        let parsed = self.graph.modules[i].parsed.as_ref().unwrap();
        let shake = &self.shaken.modules[i];
        let mut names = Vec::new();
        for (part, &live) in shake.parts.iter().zip(&shake.live) {
            if live {
                if part.declares.is_empty() {
                    names.push("<stmt>".to_string());
                }
                for &s in &part.declares {
                    names.push(parsed.ast.atom(parsed.semantic.symbol(s).name).to_string());
                }
            }
        }
        names
    }
}

#[test]
fn unused_exports_are_dropped_and_used_ones_kept() {
    let r = run(&[
        ("main.js", "import { used } from './lib.js'; used();"),
        ("lib.js", "export function used() { return helper(); } function helper() { return K; } const K = 1; export function unused() { return dead(); } function dead() {} export const alsoUnused = 2;"),
    ]);
    assert_eq!(r.live("lib.js"), ["used", "helper", "K"]);
}

#[test]
fn entry_exports_are_all_kept() {
    let r = run(&[("main.js", "export const a = 1; export function b() {} const hidden = 2; export default 3;")]);
    assert_eq!(r.live("main.js"), ["a", "b", "<stmt>"].map(String::from), "`hidden` is not exported, so it goes");
}

#[test]
fn statements_with_effects_stay_even_if_nothing_uses_them() {
    let r = run(&[
        ("main.js", "import './lib.js';"),
        ("lib.js", "const pure = 1; function f() {} register(); export const x = 2; console.log(x);"),
    ]);
    assert_eq!(r.live("lib.js"), ["<stmt>", "x", "<stmt>"].map(String::from), "register() and console.log(x) stay; and x because console.log uses it");
}

#[test]
fn side_effects_false_drops_modules_imported_only_for_effects() {
    let files = |flag: &str| {
        vec![
            ("main.js".to_string(), "import 'lib'; import { used } from 'lib2'; used();".to_string()),
            ("node_modules/lib/package.json".to_string(), format!(r#"{{"main": "i.js", "sideEffects": {flag}}}"#)),
            ("node_modules/lib/i.js".to_string(), "sideEffect();".to_string()),
            ("node_modules/lib2/package.json".to_string(), r#"{"main": "i.js", "sideEffects": false}"#.to_string()),
            ("node_modules/lib2/i.js".to_string(), "export function used() {} export function other() {} track();".to_string()),
        ]
    };
    for (flag, kept) in [("true", true), ("false", false)] {
        let owned = files(flag);
        let borrowed: Vec<(&str, &str)> = owned.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let r = run(&borrowed);
        assert_eq!(r.included("node_modules/lib/i.js"), kept, "sideEffects: {flag}");
        // A used export pulls its module in whatever the flag says, and the module's own
        // effects come along.
        assert_eq!(r.live("node_modules/lib2/i.js"), ["used", "<stmt>"]);
    }
}

#[test]
fn barrel_files_only_pull_in_what_is_used() {
    let r = run(&[
        ("main.js", "import { a } from 'lib'; a();"),
        ("node_modules/lib/package.json", r#"{"main": "index.js", "sideEffects": false}"#),
        ("node_modules/lib/index.js", "export { a } from './a.js'; export { b } from './b.js'; export * from './c.js';"),
        ("node_modules/lib/a.js", "export function a() {}"),
        ("node_modules/lib/b.js", "export function b() {}"),
        ("node_modules/lib/c.js", "export function c() {}"),
    ]);
    assert!(r.included("node_modules/lib/a.js"));
    assert!(!r.included("node_modules/lib/b.js"));
    assert!(!r.included("node_modules/lib/c.js"));
    assert!(!r.included("node_modules/lib/index.js"), "nothing in the barrel itself is needed");
}

#[test]
fn namespace_member_access_keeps_only_that_export() {
    let r = run(&[
        ("main.js", "import * as ns from './lib.js'; ns.a(); ns.b;"),
        ("lib.js", "export function a() {} export function b() {} export function c() {}"),
    ]);
    assert_eq!(r.live("lib.js"), ["a", "b"]);
    // Using the namespace as a value needs everything.
    let r = run(&[
        ("main.js", "import * as ns from './lib.js'; use(ns); ns.a();"),
        ("lib.js", "export function a() {} export function b() {} export function c() {}"),
    ]);
    assert_eq!(r.live("lib.js"), ["a", "b", "c"]);
    // So does a computed access.
    let r = run(&[("main.js", "import * as ns from './lib.js'; ns[key]();"), ("lib.js", "export function a() {} export function b() {}")]);
    assert_eq!(r.live("lib.js"), ["a", "b"]);
}

#[test]
fn dynamic_imports_and_require_keep_the_target_whole() {
    let r = run(&[
        ("main.js", "import('./d.js'); require('./r.js');"),
        ("d.js", "export function a() {} export function b() {}"),
        ("r.js", "export function c() {} function hidden() {}"),
    ]);
    assert_eq!(r.live("d.js"), ["a", "b"]);
    assert_eq!(r.live("r.js"), ["c", "hidden"]);
}

#[test]
fn commonjs_modules_are_kept_with_what_they_require() {
    let r = run(&[
        ("main.js", "import c from './c.cjs'; c();"),
        ("c.cjs", "const d = require('./d.js'); module.exports = d.f;"),
        ("d.js", "export function f() {} export function g() {}"),
    ]);
    assert!(r.included("c.cjs"));
    assert_eq!(r.live("d.js"), ["f", "g"], "the require could use any export");
}

#[test]
fn default_expression_exports() {
    let r = run(&[
        ("main.js", "import v, { other } from './lib.js'; use(v);"),
        ("lib.js", "export default compute(); export const other = 1; export const unused = 2;"),
    ]);
    // `other` is imported but never used in main.js, so only the default's statement stays.
    assert_eq!(r.live("lib.js"), ["<stmt>"]);
}

#[test]
fn unused_import_bindings_do_not_keep_code() {
    let r = run(&[
        ("main.js", "import { x } from './lib.js'; console.log(1);"),
        ("lib.js", "export const x = 1; export const y = 2;"),
    ]);
    assert_eq!(r.live("lib.js"), Vec::<String>::new());
}

#[test]
fn a_function_used_only_inside_an_unused_function_is_dropped() {
    let r = run(&[
        ("main.js", "import { a } from './lib.js'; a();"),
        ("lib.js", "export function a() {} export function b() { return c(); } function c() { return d; } const d = 1;"),
    ]);
    assert_eq!(r.live("lib.js"), ["a"]);
}

#[test]
fn variable_redeclared_in_several_statements_keeps_all() {
    let r = run(&[("main.js", "import { v } from './lib.js'; use(v);"), ("lib.js", "export var v = 1; var v = 2;")]);
    assert_eq!(r.live("lib.js"), ["v", "v"]);
}

#[test]
fn cycles_between_modules_terminate() {
    let r = run(&[
        ("main.js", "import { a } from './a.js'; a();"),
        ("a.js", "import { b } from './b.js'; export function a() { b(); }"),
        ("b.js", "import { a } from './a.js'; export function b() { a(); }"),
    ]);
    assert_eq!(r.live("a.js"), ["a"]);
    assert_eq!(r.live("b.js"), ["b"]);
}
