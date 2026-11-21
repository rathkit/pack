//! What a build says it wrote is what it wrote: the paths it prints, `--list-files` and the `outputs` of the metafile are compared with the
//! files on disk.

use std::collections::BTreeSet;
use std::process::Command;

use rathkit_pack::json::Json;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-outputs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    // By its real name: the paths a build lists are real paths (a temporary folder is a link on macOS, has a short name on Windows).
    rathkit_pack::resolve::without_verbatim_prefix(&std::fs::canonicalize(&dir).unwrap())
}

fn rkp(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(dir).output().unwrap()
}

/// Every file under `dir`, as a path from `base` with `/`.
fn files_under(base: &std::path::Path, dir: &std::path::Path, found: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(base, &path, found);
        } else {
            found.insert(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

fn page_project(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write("index.html", "<!doctype html><html><head><link rel=\"stylesheet\" href=\"./style.css\"></head><body><script type=\"module\" src=\"./main.js\"></script></body></html>\n");
    write("style.css", "h1 { color: red; }\n");
    write("main.js", "console.log('page');\n");
    std::fs::create_dir_all(dir.join("public")).unwrap();
    write("public/robots.txt", "User-agent: *\n");
    dir
}

#[test]
fn every_path_a_page_build_prints_exists_and_the_stand_in_of_a_style_sheet_is_not_one() {
    let dir = page_project("printed");
    let built = rkp(&dir, &["bundle", "index.html", "--outdir", "dist"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let printed = String::from_utf8_lossy(&built.stdout).to_string();
    let mut paths = 0;
    for line in printed.lines() {
        let path = line.rsplit_once("  ").map_or(line, |(path, _)| path);
        assert!(dir.join(path).is_file(), "{path} is printed and is not there:\n{printed}");
        paths += 1;
    }
    assert!(paths >= 3, "{printed}");
    assert!(!printed.contains("style.js"), "{printed}");
    assert!(!dir.join("dist/style.js").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_list_of_files_is_what_the_build_wrote() {
    let dir = page_project("listed");
    let built = rkp(&dir, &["bundle", "index.html", "--outdir", "dist", "--sourcemap", "--metafile=meta.json", "--list-files=written.txt"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let list = std::fs::read_to_string(dir.join("written.txt")).unwrap();
    let listed: BTreeSet<String> = list.lines().map(|l| std::path::Path::new(l).strip_prefix(&dir).map_or_else(|_| format!("outside:{l}"), |p| p.to_string_lossy().replace('\\', "/"))).collect();
    // Everything in `dist/`, the metafile, and nothing else (the list file is not in the list).
    let mut on_disk = BTreeSet::new();
    files_under(&dir, &dir.join("dist"), &mut on_disk);
    on_disk.insert("meta.json".to_string());
    assert_eq!(listed, on_disk);
    assert!(listed.contains("dist/robots.txt") && listed.contains("dist/index.html") && listed.contains("dist/main.js.map"));
    // The metafile's outputs are the same files but the metafile, each with a kind, named from the working folder.
    let meta = Json::parse(&std::fs::read_to_string(dir.join("meta.json")).unwrap()).unwrap();
    let Some(Json::Object(outputs)) = meta.get("outputs") else { panic!("outputs") };
    let named: BTreeSet<String> = outputs.iter().map(|(k, _)| k.clone()).collect();
    let mut want = on_disk.clone();
    want.remove("meta.json");
    assert_eq!(named, want);
    let kind = |name: &str| outputs.iter().find(|(k, _)| k == name).and_then(|(_, v)| v.get("kind")).and_then(Json::as_str).map(str::to_string);
    assert_eq!(kind("dist/main.js").as_deref(), Some("entry"));
    assert_eq!(kind("dist/main.js.map").as_deref(), Some("map"));
    assert_eq!(kind("dist/index.html").as_deref(), Some("html"));
    assert_eq!(kind("dist/robots.txt").as_deref(), Some("public"));
    assert_eq!(kind("dist/style.css").as_deref(), Some("entry"));
    for (name, value) in outputs {
        assert_eq!(value.get("bytes").and_then(Json::as_f64), Some(std::fs::metadata(dir.join(name)).unwrap().len() as f64), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_single_output_file_is_in_the_list_with_its_map() {
    let dir = scratch("outfile");
    std::fs::write(dir.join("src/main.js"), "console.log(1);\n").unwrap();
    let built = rkp(&dir, &["bundle", "src/main.js", "--outfile", "out/app.js", "--sourcemap", "--list-files=written.txt"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let list = std::fs::read_to_string(dir.join("written.txt")).unwrap();
    let names: Vec<String> = list.lines().map(|l| std::path::Path::new(l).strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(names, ["out/app.js", "out/app.js.map"]);
    let _ = std::fs::remove_dir_all(&dir);
}
