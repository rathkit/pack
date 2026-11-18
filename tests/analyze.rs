//! `rkp analyze`: a metafile from a real build becomes a self-contained HTML page.

use std::process::Command;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-analyze-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn rkp(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(dir).output().unwrap()
}

#[test]
fn a_build_metafile_becomes_a_report() {
    let dir = scratch("build");
    std::fs::write(dir.join("src/main.js"), "import { two } from './two.js';\nconsole.log(two());\n").unwrap();
    std::fs::write(dir.join("src/two.js"), "export function two() { return 2; }\n").unwrap();
    let built = rkp(&dir, &["bundle", "src/main.js", "--outfile", "dist/app.js", "--metafile=meta.json"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));

    let out = rkp(&dir, &["analyze", "meta.json", "--outfile", "report.html"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let page = std::fs::read_to_string(dir.join("report.html")).unwrap();
    assert!(page.starts_with("<!doctype html>"));
    // Self-contained: the modules are inside it and nothing is fetched.
    assert!(page.contains("src/two.js") && page.contains("\"app.js\""));
    assert!(!page.contains("http://") && !page.contains("https://") && !page.contains("src=\""));

    // Without --outfile the page is on the standard output.
    let out = rkp(&dir, &["analyze", "meta.json"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), page);
}

#[test]
fn a_file_that_is_not_a_metafile_is_an_error() {
    let dir = scratch("bad");
    std::fs::write(dir.join("x.json"), "{\"name\":\"not a metafile\"}").unwrap();
    let out = rkp(&dir, &["analyze", "x.json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a metafile"));
    assert_eq!(rkp(&dir, &["analyze"]).status.code(), Some(2));
    assert_eq!(rkp(&dir, &["analyze", "missing.json"]).status.code(), Some(2));
}
