//! A page that links a style sheet which one of its scripts imports too.

use std::process::Command;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-page-styles-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn project(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write("index.html", "<!doctype html><html><head><link rel=\"stylesheet\" href=\"/src/a.css\"></head><body><script type=\"module\" src=\"/src/m.ts\"></script></body></html>\n");
    write("src/a.css", "h1 { color: red; }\n");
    write("src/m.ts", "import \"./a.css\";\nconsole.log(1);\n");
    dir
}

fn css_files(dir: &std::path::Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".css")).collect();
    found.sort();
    found
}

fn check(dir: &std::path::Path, extra: &[&str]) {
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).args(["bundle", "index.html", "--outdir", "out"]).args(extra).output().unwrap();
    assert!(built.status.success(), "{extra:?}: {}", String::from_utf8_lossy(&built.stderr));
    let out = dir.join("out");
    // The style sheet is written once, holds the rule, and the page links it once.
    let sheets = css_files(&out);
    assert_eq!(sheets.len(), 1, "{extra:?}: {sheets:?}");
    assert!(std::fs::read_to_string(out.join(&sheets[0])).unwrap().contains("color:red") || std::fs::read_to_string(out.join(&sheets[0])).unwrap().contains("color: red"));
    let page = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert_eq!(page.matches("rel=\"stylesheet\"").count(), 1, "{extra:?}: {page}");
    assert!(page.contains(&format!("href=\"./{}\"", sheets[0])), "{page}");
    // The empty script the style sheet got as an entry is gone.
    assert!(!out.join("a.js").exists(), "{extra:?}");
}

#[test]
fn a_page_whose_script_imports_the_style_sheet_it_links_builds() {
    let dir = project("esm");
    check(&dir, &[]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_page_builds_as_one_script() {
    let dir = project("iife");
    check(&dir, &["--format", "iife"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn another_sheet_the_script_imports_is_still_linked() {
    let dir = project("two");
    std::fs::write(dir.join("src/b.css"), "p { margin: 0; }\n").unwrap();
    std::fs::write(dir.join("src/m.ts"), "import \"./a.css\";\nimport \"./b.css\";\nconsole.log(1);\n").unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "index.html", "--outdir", "out"]).output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let page = std::fs::read_to_string(dir.join("out/index.html")).unwrap();
    let all: String = css_files(&dir.join("out")).iter().map(|f| std::fs::read_to_string(dir.join("out").join(f)).unwrap()).collect();
    assert!(all.contains("red") && all.contains("margin"), "{all}");
    for sheet in css_files(&dir.join("out")) {
        assert_eq!(page.matches(&format!("href=\"./{sheet}\"")).count(), 1, "{sheet} in {page}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
