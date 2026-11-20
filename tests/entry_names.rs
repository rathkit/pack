//! `--entry-names` names the file of an entry the same way with and without `--splitting`.

use std::process::Command;

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-entry-names-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.ts"), "import { two } from './two';\nconsole.log(two);\n").unwrap();
    std::fs::write(dir.join("src/other.ts"), "console.log('other');\n").unwrap();
    std::fs::write(dir.join("src/two.ts"), "export const two = 2;\n").unwrap();
    dir
}

/// The names of the files `rkp bundle` writes to `out`.
fn built(dir: &std::path::Path, args: &[&str]) -> Vec<String> {
    let _ = std::fs::remove_dir_all(dir.join("out"));
    let run = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).arg("bundle").args(args).args(["--outdir", "out"]).output().unwrap();
    assert!(run.status.success(), "{args:?}: {}", String::from_utf8_lossy(&run.stderr));
    let mut names: Vec<String> = std::fs::read_dir(dir.join("out")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn one_entry_is_named_the_same_with_and_without_splitting() {
    let dir = project("one");
    for template in ["[name]", "[name]-[hash]", "app-[name]"] {
        let flag = format!("--entry-names={template}");
        let plain = built(&dir, &["src/main.ts", &flag]);
        let split = built(&dir, &["src/main.ts", &flag, "--splitting"]);
        assert_eq!(plain, split, "{template}");
        assert!(plain.iter().any(|n| n.ends_with(".js")) && !plain.contains(&"bundle.js".to_string()), "{template}: {plain:?}");
    }
    assert_eq!(built(&dir, &["src/main.ts", "--entry-names=[name]"]), ["main.js"]);
    assert_eq!(built(&dir, &["src/main.ts", "--entry-names=app-[name]"]), ["app-main.js"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_template_the_one_file_is_still_bundle_js() {
    let dir = project("default");
    assert_eq!(built(&dir, &["src/main.ts"]), ["bundle.js"]);
    assert_eq!(built(&dir, &["src/main.ts", "--chunk-names=[name]-[hash]"]), ["bundle.js"]);
    assert_eq!(built(&dir, &["src/main.ts", "--out-extension:.js=.mjs"]), ["bundle.mjs"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn several_entries_without_splitting_share_one_file_named_bundle() {
    let dir = project("several");
    assert_eq!(built(&dir, &["src/main.ts", "src/other.ts", "--entry-names=[name]-x"]), ["bundle-x.js"]);
    // With splitting each entry has its own file.
    assert_eq!(built(&dir, &["src/main.ts", "src/other.ts", "--entry-names=[name]-x", "--splitting"]).iter().filter(|n| n.ends_with("-x.js")).count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_outfile_names_its_file_itself_and_a_list_of_formats_follows_the_template() {
    let dir = project("outfile");
    let run = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "src/main.ts", "--outfile", "o/app.js", "--entry-names=[name]-x"]).output().unwrap();
    assert!(run.status.success());
    assert!(dir.join("o/app.js").is_file());
    assert_eq!(built(&dir, &["src/main.ts", "--format", "esm,cjs", "--entry-names=[name]-v"]), ["main-v.cjs", "main-v.mjs"]);
    assert_eq!(built(&dir, &["src/main.ts", "--format", "esm,cjs"]), ["main.cjs", "main.mjs"]);
    let _ = std::fs::remove_dir_all(&dir);
}
