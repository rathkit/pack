//! A build that is run again with most files unchanged (watching) keeps what it worked out for them: modules, the scan of a module that
//! imports nothing from the bundle, the pieces a big bundle is minified in. Its output must be the same as that of a build made from
//! nothing, whatever was changed in between.

use rathkit_pack::bundle::{self, Options};
use rathkit_pack::graph::{GraphOptions, ModuleCache};
use rathkit_pack::{link, minify, shake};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-rebuild-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a file and gives it a modification time of its own, so that a change is seen however fast the edits come.
fn write(dir: &Path, name: &str, text: &str, tick: u64) {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + tick)).unwrap();
}

/// A CommonJS module that requires nothing, of `count` functions.
fn library(count: usize, changed: Option<usize>) -> String {
    let mut text = String::from("'use strict';\nvar total = 0;\nexports.flag = typeof FLAG === 'undefined' ? 0 : FLAG;\n");
    for i in 0..count {
        let bump = if changed == Some(i) { 7 } else { 1 };
        text.push_str(&format!("exports.f{i} = function (x, y) {{\n  var t = x * {i} + y;\n  if (t > {i}) {{ t = t - {bump}; }}\n  total += t;\n  return t;\n}};\n"));
    }
    text.push_str("exports.total = function () { return total; };\n");
    text
}

struct Case {
    name: &'static str,
    minify: bool,
    format: bundle::Format,
}

fn build(dir: &Path, cache: Option<Arc<ModuleCache>>, keep: bool, case: &Case, defines: &[(&str, &str)]) -> String {
    let mut graph_options = GraphOptions { cache, threads: 2, ..GraphOptions::default() };
    for (name, value) in defines {
        graph_options.defines.add(rathkit_pack::define::Define::parse(name, value).unwrap());
    }
    let cache = graph_options.cache.clone();
    let graph = rathkit_pack::graph::build(dir, &["main.js".to_string()], &graph_options);
    assert!(!graph.has_errors(), "{:?}", graph.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>());
    let linked = link::link(&graph);
    let shaken = shake::shake(&graph, &linked);
    let mut options = Options { format: case.format, keep_scans: keep, ..Options::default() };
    if case.minify {
        options.minify = minify::Options { whitespace: true, identifiers: true, syntax: true, ..minify::Options::default() };
    }
    // Only the build that keeps things uses (and adds to) what was kept.
    minify::keep_pieces(keep);
    let output = bundle::bundle(&graph, &linked, &shaken, &options).unwrap_or_else(|d| panic!("{:?}", d.iter().map(|d| d.message.clone()).collect::<Vec<_>>()));
    if let Some(cache) = cache {
        cache.store(graph);
    }
    output.code
}

#[test]
fn a_rebuild_that_keeps_what_it_can_writes_what_a_build_from_nothing_writes() {
    // Small pieces and small bodies, so that a program of a few kilobytes is cut up like a big one.
    unsafe {
        std::env::set_var("RATHKIT_FRAGMENT_BYTES", "1500");
        std::env::set_var("RATHKIT_NEST_BYTES", "1000");
    }
    minify::keep_pieces(true);
    let scans = bundle::REUSED_SCANS.load(std::sync::atomic::Ordering::Relaxed);
    let pieces = minify::REUSED_PIECES.load(std::sync::atomic::Ordering::Relaxed);
    for case in [Case { name: "minified", minify: true, format: bundle::Format::Esm }, Case { name: "plain", minify: false, format: bundle::Format::Esm }, Case { name: "minified cjs", minify: true, format: bundle::Format::Cjs }] {
        let dir = scratch(case.name.replace(' ', "-").as_str());
        let cache = Arc::new(ModuleCache::default());
        let main = |extra: &str, imports_b: bool| format!("import {{ a, helper, v0, v1, v2, v3, v4, v5, v6, v7, v8, v9 }} from './a.js';\nimport legacy from './legacy.cjs';\nimport './effects.js';\n{}console.log(a, helper(2), legacy.f3(1, 2), legacy.total());\n{extra}", if imports_b { "import { b } from './b.js';\nconsole.log(b);\n" } else { "" });
        // The names a module has to hand out come first, then enough statements that it is a piece or two of its own.
        let a_js = format!("export const a = 1;\nexport function helper(x) {{ return x + 1; }}\n{}{}", (0..10).map(|i| format!("export const v{i} = {i};\n")).collect::<String>(), (0..70).map(|i| format!("globalThis.k{i} = v{} + {i};\n", i % 10)).collect::<String>());
        let a_js = a_js.as_str();
        let b_js = "import { a } from './a.js';\nexport const b = a + 2;\n";
        // A module that imports nothing, whose pieces name only locals.
        let effects = (0..60).map(|i| format!("(function () {{ var q{i} = {i}; console.log(q{i}); }})();\n")).collect::<String>();
        write(&dir, "effects.js", &effects, 1);
        write(&dir, "a.js", a_js, 1);
        write(&dir, "b.js", b_js, 1);
        write(&dir, "legacy.cjs", &library(120, None), 1);
        write(&dir, "main.js", &main("", true), 1);

        let check = |step: &str, defines: &[(&str, &str)]| {
            let reused = minify::REUSED_PIECES.load(std::sync::atomic::Ordering::Relaxed);
            let warm = build(&dir, Some(cache.clone()), true, &case, defines);
            let reused = minify::REUSED_PIECES.load(std::sync::atomic::Ordering::Relaxed) - reused;
            // What the build did not change, it did not do again.
            let expected = case.minify && !matches!(step, "first build" | "a global is read" | "a define is added" | "the define changes" | "the library is as it was");
            assert!(!expected || reused > 0, "{}: {step}: no piece was reused", case.name);
            let cold = build(&dir, None, false, &case, defines);
            if warm != cold {
                let at = warm.bytes().zip(cold.bytes()).position(|(a, b)| a != b).unwrap_or(warm.len().min(cold.len()));
                let around = |text: &str| text.get(at.saturating_sub(60)..(at + 60).min(text.len())).unwrap_or("").to_string();
                panic!("{}: {step}: the outputs differ at byte {at} ({} and {} bytes long)\n  rebuilt: {}\n  from nothing: {}", case.name, warm.len(), cold.len(), around(&warm), around(&cold));
            }
        };
        check("first build", &[]);
        check("again, nothing changed", &[]);
        write(&dir, "main.js", &main("console.log('one');\n", true), 2);
        check("the entry changed", &[]);
        write(&dir, "legacy.cjs", &library(120, Some(60)), 3);
        check("a function of the library changed", &[]);
        write(&dir, "a.js", &format!("{a_js}export const extra = 5;\n"), 4);
        write(&dir, "main.js", &main("import { extra } from './a.js';\nconsole.log(extra);\n", true), 4);
        check("a dependency gained an export that the entry imports", &[]);
        let many: String = (0..80).map(|i| format!("console.log(a + {i});\n")).collect();
        write(&dir, "main.js", &main(&many, true), 5);
        check("the entry uses a top-level name a lot", &[]);
        // Which names are used most decides which short name each gets: the order is turned round without a line of a.js changing.
        let ascending: String = (0..10).map(|i| format!("{}\n", format!("console.log(v{i});").repeat(i + 1))).collect();
        write(&dir, "main.js", &main(&ascending, true), 5);
        check("names used in one order", &[]);
        let descending: String = (0..10).map(|i| format!("{}\n", format!("console.log(v{i});").repeat(10 - i))).collect();
        write(&dir, "main.js", &main(&descending, true), 6);
        check("names used in the other order", &[]);
        write(&dir, "main.js", &main("", false), 6);
        check("an import was removed", &[]);
        write(&dir, "main.js", &main("console.log(typeof FLAG, typeof e, typeof t, typeof n, typeof r, typeof i);\n", true), 7);
        check("a global is read", &[]);
        check("a define is added", &[("FLAG", "1")]);
        check("the define changes", &[("FLAG", "2")]);
        write(&dir, "legacy.cjs", &library(120, None), 8);
        check("the library is as it was", &[]);
        std::fs::remove_dir_all(&dir).ok();
    }
    // Scans and pieces were taken from the build before, not made again.
    assert!(bundle::REUSED_SCANS.load(std::sync::atomic::Ordering::Relaxed) > scans, "no scan was reused");
    assert!(minify::REUSED_PIECES.load(std::sync::atomic::Ordering::Relaxed) > pieces, "no piece was reused");
    minify::keep_pieces(false);
}
