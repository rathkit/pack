use super::*;
use crate::graph::{GraphOptions, build_with_fs};
use crate::resolve::MemFs;
use std::path::Path;

fn graph(files: &[(&str, &str)]) -> Graph {
    let mut fs = MemFs::new();
    for (path, text) in files {
        fs = fs.with(&format!("/p/{path}"), text);
    }
    let g = build_with_fs(Box::new(fs), Path::new("/p"), &["main.js".to_string()], &GraphOptions { threads: 1, ..GraphOptions::default() });
    assert!(!g.has_errors(), "{:?}", g.diagnostics);
    g
}

fn id(g: &Graph, file: &str) -> ModuleId {
    g.modules.iter().find(|m| m.path == Path::new("/p").join(file)).unwrap().id
}

/// Describes where the import named `local` in `main.js` leads, as `file:name`.
fn target(g: &Graph, linked: &Linked, local: &str) -> String {
    let main = g.module(id(g, "main.js"));
    let parsed = main.parsed.as_ref().unwrap();
    let sym = parsed.semantic.symbols().find(|(_, s)| parsed.ast.atom(s.name) == local).map(|(i, _)| i).unwrap();
    describe(g, linked.binding(main.id, sym))
}

fn describe(g: &Graph, binding: Option<&Binding>) -> String {
    let file = |m: &ModuleId| g.module(*m).path.file_name().unwrap().to_string_lossy().into_owned();
    match binding {
        None => "unlinked".to_string(),
        Some(Binding::Symbol { module, symbol }) => {
            let parsed = g.module(*module).parsed.as_ref().unwrap();
            format!("{}:{}", file(module), parsed.ast.atom(parsed.semantic.symbol(*symbol).name))
        }
        Some(Binding::DefaultValue { module, .. }) => format!("{}:<default value>", file(module)),
        Some(Binding::Namespace(m)) => format!("{}:*", file(m)),
        Some(Binding::CommonJs { module, name }) => format!("{}:cjs {name:?}", file(module)),
        Some(Binding::External { specifier, name }) => format!("external {specifier} {name:?}"),
    }
}

#[test]
fn direct_imports() {
    let g = graph(&[
        ("main.js", "import d, { a as b } from './a.js'; import * as ns from './a.js'; use(d, b, ns);"),
        ("a.js", "export default function f() {} export const a = 1;"),
    ]);
    let linked = link(&g);
    assert!(!linked.has_errors(), "{:?}", linked.diagnostics);
    assert_eq!(target(&g, &linked, "d"), "a.js:f");
    assert_eq!(target(&g, &linked, "b"), "a.js:a");
    assert_eq!(target(&g, &linked, "ns"), "a.js:*");
}

#[test]
fn re_export_chains_collapse() {
    let g = graph(&[
        ("main.js", "import { x, y, z } from './a.js'; use(x, y, z);"),
        ("a.js", "export { b as x } from './b.js'; export * from './c.js'; import { q as y } from './b.js'; export { y }; export { default as z } from './b.js';"),
        ("b.js", "export const b = 1; export const q = 2; export default 3;"),
        ("c.js", "export const k = 1;"),
    ]);
    let linked = link(&g);
    assert!(!linked.has_errors(), "{:?}", linked.diagnostics);
    assert_eq!(target(&g, &linked, "x"), "b.js:b");
    assert_eq!(target(&g, &linked, "y"), "b.js:q", "an imported name that is exported again is a re-export");
    assert_eq!(target(&g, &linked, "z"), "b.js:<default value>");
}

#[test]
fn star_exports() {
    let g = graph(&[
        ("main.js", "import { a, b, same } from './index.js'; use(a, b, same);"),
        ("index.js", "export * from './a.js'; export * from './b.js'; export * from './c.js'; export * from './d.js';"),
        ("a.js", "export const a = 1; export const same = 1; export default 'a';"),
        ("b.js", "export const b = 1;"),
        ("c.js", "export { same } from './a.js';"),
        ("d.js", "export * from './index.js';"),
    ]);
    let linked = link(&g);
    assert!(!linked.has_errors(), "{:?}", linked.diagnostics);
    assert_eq!(target(&g, &linked, "a"), "a.js:a");
    assert_eq!(target(&g, &linked, "b"), "b.js:b");
    assert_eq!(target(&g, &linked, "same"), "a.js:same", "two paths to one binding are not a conflict");
}

#[test]
fn star_export_does_not_forward_default() {
    let g = graph(&[("main.js", "import d from './index.js';"), ("index.js", "export * from './a.js';"), ("a.js", "export default 1;")]);
    let linked = link(&g);
    assert_eq!(linked.diagnostics.len(), 1);
    assert!(linked.diagnostics[0].message.contains("No matching export in './index.js' for import 'default'"));
}

#[test]
fn ambiguous_star_exports() {
    let g = graph(&[
        ("main.js", "import { x, ok } from './index.js'; use(x, ok);"),
        ("index.js", "export * from './a.js'; export * from './b.js';"),
        ("a.js", "export const x = 1; export const ok = 1;"),
        ("b.js", "export const x = 2;"),
    ]);
    let linked = link(&g);
    assert_eq!(linked.diagnostics.len(), 1, "{:?}", linked.diagnostics);
    assert!(linked.diagnostics[0].message.contains("'x' from './index.js' is ambiguous"));
    // A local export beats a star export instead of conflicting with it.
    let g = graph(&[("main.js", "import { x } from './index.js'; use(x);"), ("index.js", "export * from './a.js'; export const x = 3;"), ("a.js", "export const x = 1;")]);
    let linked = link(&g);
    assert!(!linked.has_errors());
    assert_eq!(target(&g, &linked, "x"), "index.js:x");
}

#[test]
fn missing_exports_are_errors_with_positions() {
    let g = graph(&[("main.js", "import { nope } from './a.js';\nexport { gone } from './a.js';"), ("a.js", "export const a = 1;")]);
    let linked = link(&g);
    assert_eq!(linked.diagnostics.len(), 2);
    assert_eq!(linked.diagnostics[0].pos, Some(9), "points at the specifier in the import");
    assert!(linked.diagnostics[0].message.contains("for import 'nope'"));
    assert!(linked.diagnostics[1].message.contains("for import 'gone'"));
}

#[test]
fn circular_re_exports_terminate() {
    let g = graph(&[
        ("main.js", "import { x, loop } from './a.js'; use(x, loop);"),
        ("a.js", "export * from './b.js'; export { loop } from './b.js';"),
        ("b.js", "export * from './a.js'; export { loop } from './a.js'; export const x = 1;"),
    ]);
    let linked = link(&g);
    assert_eq!(target(&g, &linked, "x"), "b.js:x");
    // `loop` is declared nowhere, so each place that mentions it fails: both re-exports and the import.
    assert_eq!(linked.diagnostics.len(), 3, "{:?}", linked.diagnostics);
    assert!(linked.diagnostics.iter().all(|d| d.message.contains("'loop'")));
}

#[test]
fn commonjs_json_and_external_targets_are_dynamic() {
    let g = graph(&[
        ("main.js", "import c, { named } from './c.cjs'; import data, { k } from './d.json'; import { e } from 'fs'; import { viaStar } from './star.js'; use(c, named, data, k, e, viaStar);"),
        ("c.cjs", "module.exports = { named: 1 };"),
        ("d.json", r#"{"k": 1}"#),
        ("star.js", "export * from 'fs';"),
    ]);
    let linked = link(&g);
    assert!(!linked.has_errors(), "{:?}", linked.diagnostics);
    assert_eq!(target(&g, &linked, "c"), "c.cjs:cjs Default");
    assert_eq!(target(&g, &linked, "named"), "c.cjs:cjs Named(\"named\")");
    assert_eq!(target(&g, &linked, "k"), "d.json:cjs Named(\"k\")");
    assert_eq!(target(&g, &linked, "e"), "external fs Named(\"e\")");
    assert_eq!(target(&g, &linked, "viaStar"), "external fs Named(\"viaStar\")");
}

#[test]
fn a_real_declaration_beats_a_dynamic_star() {
    let g = graph(&[
        ("main.js", "import { x } from './index.js'; use(x);"),
        ("index.js", "export * from './c.cjs'; export * from './a.js';"),
        ("c.cjs", "exports.x = 1;"),
        ("a.js", "export const x = 2;"),
    ]);
    let linked = link(&g);
    assert_eq!(target(&g, &linked, "x"), "a.js:x");
}

#[test]
fn namespace_names() {
    let g = graph(&[
        ("main.js", "import * as ns from './index.js'; use(ns);"),
        ("index.js", "export * from './a.js'; export * from './b.js'; export const own = 1; export default 2;"),
        ("a.js", "export const a = 1; export const clash = 1; export default 3;"),
        ("b.js", "export const b = 1; export const clash = 2;"),
    ]);
    assert_eq!(export_names(&g, id(&g, "index.js")), ["a", "b", "default", "own"]);
}
