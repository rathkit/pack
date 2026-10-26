use super::*;
use crate::resolve::MemFs;

fn build(fs: MemFs, entries: &[&str], threads: usize) -> Graph {
    let entries: Vec<String> = entries.iter().map(|e| e.to_string()).collect();
    build_with_fs(Box::new(fs), Path::new("/p"), &entries, &GraphOptions { threads, ..GraphOptions::default() })
}

fn names(graph: &Graph) -> Vec<String> {
    graph.modules.iter().map(|m| m.path.to_string_lossy().trim_start_matches("/p/").to_string()).collect()
}

fn edges(graph: &Graph, file: &str) -> Vec<Edge> {
    graph.modules.iter().find(|m| m.path == Path::new("/p").join(file)).unwrap().edges.clone()
}

fn project() -> MemFs {
    MemFs::new()
        .with("/p/main.js", "import a from './a.js'; import './b.js'; export const m = a;")
        .with("/p/a.js", "import { c } from './c'; export default c;")
        .with("/p/b.js", "import './c.js'; import fs from 'fs'; import 'node:path';")
        .with("/p/c.js", "export const c = 1;")
}

#[test]
#[cfg(unix)] // in-memory files at /p/...: not absolute paths on Windows
fn modules_are_numbered_depth_first_from_the_entry() {
    let graph = build(project(), &["main.js"], 1);
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    assert_eq!(names(&graph), ["main.js", "a.js", "c.js", "b.js"]);
    assert_eq!(graph.entries, [ModuleId(0)]);
    assert_eq!(edges(&graph, "main.js"), [Edge::Module(ModuleId(1)), Edge::Module(ModuleId(3))]);
    assert_eq!(edges(&graph, "b.js"), [Edge::Module(ModuleId(2)), Edge::External("fs".into()), Edge::External("node:path".into())]);
}

#[test]
fn the_graph_is_the_same_for_any_thread_count() {
    let reference = names(&build(project(), &["main.js"], 1));
    for threads in [2, 4, 8] {
        for _ in 0..20 {
            assert_eq!(names(&build(project(), &["main.js"], threads)), reference);
        }
    }
}

#[test]
#[cfg(unix)] // in-memory files at /p/...: not absolute paths on Windows
fn cycles_and_shared_dependencies_load_once() {
    let fs = MemFs::new()
        .with("/p/a.js", "import './b.js'; export const a = 1;")
        .with("/p/b.js", "import './a.js'; import './c.js'; export const b = 1;")
        .with("/p/c.js", "import './b.js';");
    let graph = build(fs, &["a.js"], 3);
    assert_eq!(names(&graph), ["a.js", "b.js", "c.js"]);
    assert_eq!(edges(&graph, "c.js"), [Edge::Module(ModuleId(1))]);
}

#[test]
fn module_kinds() {
    let fs = MemFs::new()
        .with("/p/main.js", "import './esm.js'; require('./cjs.js'); import('./data.json'); import './x.mjs'; require('./y.cjs');")
        .with("/p/esm.js", "export const a = 1;")
        .with("/p/cjs.js", "module.exports = { a: 1 };")
        .with("/p/data.json", r#"{"k": [1, 2]}"#)
        .with("/p/x.mjs", "export const x = 1;")
        .with("/p/y.cjs", "exports.y = 1; with (o) { f() }");
    let graph = build(fs, &["main.js"], 2);
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    let kind = |file: &str| graph.modules.iter().find(|m| m.path.ends_with(file)).unwrap().kind;
    assert_eq!(kind("main.js"), ModuleKind::Esm);
    assert_eq!(kind("esm.js"), ModuleKind::Esm);
    assert_eq!(kind("cjs.js"), ModuleKind::CommonJs);
    assert_eq!(kind("data.json"), ModuleKind::Json);
    assert_eq!(kind("x.mjs"), ModuleKind::Esm);
    assert_eq!(kind("y.cjs"), ModuleKind::CommonJs, ".cjs is a script, so `with` is allowed");
}

#[test]
fn package_type_decides_ambiguous_files() {
    let fs = MemFs::new()
        .with("/p/package.json", r#"{"type": "module"}"#)
        .with("/p/main.js", "const x = 1;")
        .with("/p/sloppy/package.json", r#"{"type": "commonjs"}"#)
        .with("/p/sloppy/a.js", "with (o) { f() }");
    let graph = build(fs, &["main.js", "sloppy/a.js"], 1);
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics);
    assert_eq!(graph.modules[0].kind, ModuleKind::Esm, "a file in a type: module package is an ES module even without import or export");
    assert_eq!(graph.modules[1].kind, ModuleKind::CommonJs);
}

#[test]
#[cfg(unix)] // in-memory files at /p/...: not absolute paths on Windows
fn errors_are_collected_not_fatal() {
    let fs = MemFs::new()
        .with("/p/main.js", "import './broken.js'; import './missing.js'; import './ok.js'; import './style.xyz'; try { require('./optional') } catch {}")
        .with("/p/broken.js", "let = ;")
        .with("/p/ok.js", "export const ok = 1;")
        .with("/p/style.xyz", "a{}");
    let graph = build(fs, &["main.js"], 2);
    assert!(graph.has_errors());
    let errors: Vec<_> = graph.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    let warnings: Vec<_> = graph.diagnostics.iter().filter(|d| d.severity == Severity::Warning).collect();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(warnings.len(), 1, "an unresolved require is only a warning");
    assert!(errors.iter().any(|d| d.message.contains("No loader for '.xyz'")));
    assert!(errors.iter().any(|d| d.file.as_deref() == Some(Path::new("/p/broken.js"))));
    // The files that did load are still in the graph, with dead edges marked.
    assert_eq!(names(&graph), ["main.js", "ok.js"]);
    assert_eq!(edges(&graph, "main.js")[0], Edge::Unresolved);
}

#[test]
fn missing_entry_is_reported() {
    let graph = build(MemFs::new(), &["nope.js"], 1);
    assert!(graph.has_errors());
    assert!(graph.modules.is_empty());
}

#[test]
fn side_effects_come_from_the_package() {
    let fs = MemFs::new()
        .with("/p/main.js", "import 'lib'; import './local.js';")
        .with("/p/local.js", "")
        .with("/p/node_modules/lib/package.json", r#"{"main": "i.js", "sideEffects": false}"#)
        .with("/p/node_modules/lib/i.js", "export const x = 1;");
    let graph = build(fs, &["main.js"], 1);
    let side_effects = |file: &str| graph.modules.iter().find(|m| m.path.ends_with(file)).unwrap().side_effects;
    assert!(side_effects("main.js"));
    assert!(side_effects("local.js"));
    assert!(!side_effects("i.js"));
}

#[test]
fn externals() {
    let fs = MemFs::new().with("/p/main.js", "import 'react'; import 'react/jsx-runtime'; import '@org/x/y'; import 'reactish';");
    let entries = vec!["main.js".to_string()];
    let options = GraphOptions { external: vec!["react".into(), "react/*".into(), "@org/*".into()], threads: 1, ..GraphOptions::default() };
    let graph = build_with_fs(Box::new(fs), Path::new("/p"), &entries, &options);
    let edges = &graph.modules[0].edges;
    assert_eq!(edges[0], Edge::External("react".into()));
    assert_eq!(edges[1], Edge::External("react/jsx-runtime".into()));
    assert_eq!(edges[2], Edge::External("@org/x/y".into()));
    assert_eq!(edges[3], Edge::Unresolved, "'reactish' only shares a prefix");
}

#[test]
fn node_builtin_names() {
    for yes in ["fs", "fs/promises", "node:test", "path", "worker_threads"] {
        assert!(is_node_builtin(yes), "{yes}");
    }
    for no in ["react", "./fs", "fsx", "@a/fs"] {
        assert!(!is_node_builtin(no), "{no}");
    }
}

#[test]
fn a_query_is_split_off_a_module_name_but_not_off_a_windows_verbatim_path() {
    assert_eq!(split_query(Path::new("/p/logo.svg?raw")), (PathBuf::from("/p/logo.svg"), "raw".to_string()));
    assert_eq!(split_query(Path::new("/p/a.js")), (PathBuf::from("/p/a.js"), String::new()));
    // The `?` of `\\?\C:\...` starts nothing: what follows it is a path.
    let verbatim = PathBuf::from(r"\\?\D:\a\pack\main.js");
    assert_eq!(split_query(&verbatim), (verbatim.clone(), String::new()));
    let with_query = PathBuf::from(r"\\?\D:\a\logo.svg?raw");
    assert_eq!(split_query(&with_query), (PathBuf::from(r"\\?\D:\a\logo.svg"), "raw".to_string()));
}
