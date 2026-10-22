use super::*;
use crate::parser::{ParseOptions, parse};

fn record(src: &str, module: bool) -> ModuleRecord {
    let parsed = parse(src, ParseOptions::new(module)).unwrap_or_else(|e| panic!("{}: {src}", e.message));
    ModuleRecord::build(&parsed)
}

fn esm(src: &str) -> ModuleRecord {
    record(src, true)
}

fn specifiers(r: &ModuleRecord) -> Vec<String> {
    r.requests.iter().map(|q| format!("{}:{:?}", q.specifier, q.kind)).collect()
}

#[test]
fn imports_by_kind() {
    let r = esm("import d, { a, b as c, default as e } from 'x'; import * as ns from 'y'; import 'z';");
    assert_eq!(specifiers(&r), ["x:Static", "y:Static", "z:Static"]);
    let names: Vec<_> = r.imports.iter().map(|i| (i.request, i.name.clone())).collect();
    assert_eq!(
        names,
        [
            (0, ImportName::Default),
            (0, ImportName::Named("a".into())),
            (0, ImportName::Named("b".into())),
            (0, ImportName::Default),
            (1, ImportName::Namespace),
        ]
    );
    assert!(r.is_esm);
}

#[test]
fn repeated_specifiers_share_a_request() {
    let r = esm("import a from 'x'; import { b } from 'x'; export { c } from 'x';");
    assert_eq!(r.requests.len(), 1);
}

#[test]
fn attributes_make_distinct_requests() {
    let r = esm("import a from 'x'; import b from 'x' with { type: 'json' };");
    assert_eq!(r.requests.len(), 2);
    assert_eq!(r.requests[1].attrs, [("type".to_string(), "json".to_string())]);
}

#[test]
fn local_exports() {
    let r = esm("export const a = 1, { b, c: [d] } = o; export function f() {} export class K {} let z; export { z as y };");
    let names: Vec<_> = r.exports.iter().map(|e| e.exported.as_str()).collect();
    assert_eq!(names, ["a", "b", "d", "f", "K", "y"]);
    assert!(r.exports.iter().all(|e| matches!(e.source, ExportSource::Local(_))));
}

#[test]
fn default_exports() {
    let r = esm("export default function f() {}");
    assert!(matches!(r.export("default").unwrap().source, ExportSource::Local(_)));
    let r = esm("export default function () {}");
    assert!(matches!(r.export("default").unwrap().source, ExportSource::DefaultValue(_)));
    let r = esm("export default 1 + 2;");
    assert!(matches!(r.export("default").unwrap().source, ExportSource::DefaultValue(_)));
    let r = esm("export default class C {}");
    assert!(matches!(r.export("default").unwrap().source, ExportSource::Local(_)));
}

#[test]
fn re_exports() {
    let r = esm("export { a, b as c, default as d, default } from 'x'; export * from 'y'; export * as ns from 'z';");
    assert_eq!(r.star_exports, [1]);
    let get = |n: &str| match &r.export(n).unwrap().source {
        ExportSource::Indirect { request, name } => (*request, name.clone()),
        other => panic!("{other:?}"),
    };
    assert_eq!(get("a"), (0, ImportName::Named("a".into())));
    assert_eq!(get("c"), (0, ImportName::Named("b".into())));
    assert_eq!(get("d"), (0, ImportName::Default));
    assert_eq!(get("default"), (0, ImportName::Default));
    assert_eq!(get("ns"), (2, ImportName::Namespace));
}

#[test]
fn dynamic_and_require_calls() {
    let r = record("import('a'); const b = require('b'); function g(require) { require('c'); } function h() { var require = f; require('d'); } import(x); require(y);", false);
    // A parameter called `require` is the UMD factory pattern and counts; another local does not.
    assert_eq!(specifiers(&r), ["a:Dynamic", "b:Require", "c:Require"]);
    assert!(!r.is_esm);
    assert!(r.uses_commonjs_globals);
}

#[test]
fn commonjs_detection() {
    assert!(record("module.exports = 1;", false).uses_commonjs_globals);
    assert!(record("exports.a = 1;", false).uses_commonjs_globals);
    assert!(!record("var x = 1;", false).uses_commonjs_globals);
    assert!(!esm("export const exports = 1;").uses_commonjs_globals);
}
