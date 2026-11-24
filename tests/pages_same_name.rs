//! Pages with the same file name in different folders stay two files.

use std::process::Command;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-pages-same-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    dir
}

fn page(script: &str) -> String {
    format!("<!doctype html><html><head></head><body><script type=\"module\" src=\"{script}\"></script></body></html>\n")
}

fn script_of(html: &str) -> String {
    let at = html.find("src=\"").unwrap() + 5;
    html[at..at + html[at..].find('"').unwrap()].to_string()
}

#[test]
fn two_pages_with_one_file_name_keep_their_folders() {
    let dir = scratch("two");
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write("index.html", &page("./a.js"));
    write("a.js", "console.log('a');\n");
    write("nested/index.html", &page("./b.js"));
    write("nested/b.js", "console.log('b');\n");
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "index.html", "nested/index.html", "--outdir", "out"]).output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let top = std::fs::read_to_string(dir.join("out/index.html")).unwrap();
    let nested = std::fs::read_to_string(dir.join("out/nested/index.html")).unwrap();
    // Each page points at its own script, and the nested one reaches the built file one folder up.
    let top_script = script_of(&top);
    let nested_script = script_of(&nested);
    assert!(std::fs::read_to_string(dir.join("out").join(&top_script)).unwrap().contains("'a'") || std::fs::read_to_string(dir.join("out").join(&top_script)).unwrap().contains("\"a\""), "{top}");
    assert!(nested_script.starts_with("../"), "{nested}");
    let built_b = std::fs::read_to_string(dir.join("out/nested").join(&nested_script)).unwrap();
    assert!(built_b.contains("'b'") || built_b.contains("\"b\""), "{nested}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_page_in_a_folder_is_still_written_at_the_top_of_the_output() {
    let dir = scratch("one");
    std::fs::write(dir.join("nested/index.html"), page("./b.js")).unwrap();
    std::fs::write(dir.join("nested/b.js"), "console.log('b');\n").unwrap();
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "nested/index.html", "--outdir", "out"]).output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    assert!(dir.join("out/index.html").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
