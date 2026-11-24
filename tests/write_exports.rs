//! `--write-exports=FILE` names a file that is made from `package.json` when it is missing; a copy of `package.json` leaves the project's own alone.

use std::process::Command;

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-exports-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/index.js"), "export const a = 1;\n").unwrap();
    std::fs::write(dir.join("package.json"), "{\"name\":\"lib\",\"version\":\"1.0.0\"}\n").unwrap();
    dir
}

fn rkp(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).arg("bundle").args(args).output().unwrap()
}

#[test]
fn a_file_that_does_not_exist_is_made_from_the_projects_package_json() {
    let dir = project("missing");
    let before = std::fs::read_to_string(dir.join("package.json")).unwrap();
    let run = rkp(&dir, &["src/index.js", "--outdir", "dist", "--entry-names=[name]", "--write-exports=dist/package.json"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(std::fs::read_to_string(dir.join("package.json")).unwrap(), before);
    let made = std::fs::read_to_string(dir.join("dist/package.json")).unwrap();
    assert!(made.contains("\"main\": \"./index.js\"") && made.contains("\"name\": \"lib\""), "{made}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_and_no_package_json_is_an_error_that_names_both() {
    let dir = project("none");
    std::fs::remove_file(dir.join("package.json")).unwrap();
    let run = rkp(&dir, &["src/index.js", "--outdir", "dist", "--entry-names=[name]", "--write-exports=dist/package.json"]);
    assert!(!run.status.success());
    let message = String::from_utf8_lossy(&run.stderr);
    assert!(message.contains("dist") && message.contains("no package.json"), "{message}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_copy_workflow_leaves_the_projects_package_json_alone() {
    let dir = project("copy");
    let before = std::fs::read_to_string(dir.join("package.json")).unwrap();
    std::fs::create_dir_all(dir.join("dist")).unwrap();
    std::fs::copy(dir.join("package.json"), dir.join("dist/package.json")).unwrap();
    let run = rkp(&dir, &["src/index.js", "--outdir", "dist", "--entry-names=[name]", "--write-exports=dist/package.json"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(std::fs::read_to_string(dir.join("package.json")).unwrap(), before);
    let copy = std::fs::read_to_string(dir.join("dist/package.json")).unwrap();
    // The paths are relative to the copy, which is beside the files.
    assert!(copy.contains("\"main\": \"./index.js\"") && copy.contains("\"name\": \"lib\""), "{copy}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_entry_in_one_format_without_a_name_template_says_what_to_do() {
    let dir = project("bundle");
    let run = rkp(&dir, &["src/index.js", "--outdir", "dist", "--write-exports"]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("--entry-names=[name]"));
    let _ = std::fs::remove_dir_all(&dir);
}
