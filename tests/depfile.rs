//! `--depfile=FILE`: every file a build looked at, the ones that were not there too.

use std::process::Command;

use rathkit_pack::json::Json;

fn project(root: &std::path::Path) -> std::path::PathBuf {
    let dir = root.join("proj");
    let _ = std::fs::remove_dir_all(root);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("public")).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("package.json", "{\"name\":\"p\",\"type\":\"module\"}\n");
    write("tsconfig.base.json", "{\"compilerOptions\":{\"strict\":true}}\n");
    write("tsconfig.json", "{\"extends\":\"./tsconfig.base.json\"}\n");
    write("index.html", "<link rel=\"stylesheet\" href=\"/src/a.css\"><script type=\"module\" src=\"/src/m.ts\"></script>\n");
    write("src/a.css", "h1 { color: red; }\n");
    write("src/i.svg", "<svg/>\n");
    write("src/m.ts", "import \"./a.css\";\nimport icon from \"./i.svg\";\nimport { n } from \"./n\";\nconsole.log(icon, n, import.meta.env.MODE);\n");
    write("src/n.js", "export const n = 1;\n");
    write("public/robots.txt", "User-agent: *\n");
    write(".env", "VITE_A=1\n");
    write(".browserslistrc", "chrome >= 100\n");
    dir
}

fn list(json: &Json, key: &str) -> Vec<String> {
    json.get(key).and_then(Json::as_array).unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

fn depfile(dir: &std::path::Path) -> Json {
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).args(["bundle", "index.html", "--outdir", "out", "--depfile=deps/build.json"]).output().unwrap();
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    Json::parse(&std::fs::read_to_string(dir.join("deps/build.json")).unwrap()).unwrap()
}

#[test]
fn the_list_has_every_kind_of_file_the_build_read_and_the_ones_it_looked_for() {
    let base = std::env::temp_dir().join(format!("rkp-depfile-{}-kinds", std::process::id()));
    let dir = project(&base);
    let deps = depfile(&dir);
    let read = list(&deps, "read");
    for name in ["index.html", "src/m.ts", "src/a.css", "src/i.svg", "src/n.js", "tsconfig.json", "tsconfig.base.json", "package.json", ".env", ".browserslistrc", "public/", "public/robots.txt"] {
        assert!(read.contains(&name.to_string()), "{name} is read: {read:?}");
    }
    let missing = list(&deps, "missing");
    // A file that does not exist yet and changes the build when it appears: the other .env files, a nearer package.json, a config.
    for name in [".env.local", ".env.production", "src/package.json", "rathkit.config.mjs", "postcss.config.js"] {
        assert!(missing.contains(&name.to_string()), "{name} is probed and missing: {missing:?}");
    }
    // Nothing is in both lists, and the build's own output and the list itself are not read.
    assert!(!missing.iter().any(|m| read.contains(m)));
    assert!(!read.iter().chain(&missing).any(|p| p.starts_with("out/") || p.starts_with("deps/")), "{read:?}");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_same_build_in_two_checkouts_writes_the_same_list() {
    let mut texts = Vec::new();
    for name in ["one", "two"] {
        let base = std::env::temp_dir().join(format!("rkp-depfile-{}-{name}", std::process::id())).join("x");
        let dir = project(&base);
        depfile(&dir);
        texts.push(std::fs::read_to_string(dir.join("deps/build.json")).unwrap());
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
    }
    assert_eq!(texts[0], texts[1]);
}

#[test]
fn a_file_that_appears_moves_from_missing_to_read() {
    let base = std::env::temp_dir().join(format!("rkp-depfile-{}-appears", std::process::id()));
    let dir = project(&base);
    std::fs::write(dir.join(".env.local"), "VITE_B=2\n").unwrap();
    let deps = depfile(&dir);
    assert!(list(&deps, "read").contains(&".env.local".to_string()));
    assert!(!list(&deps, "missing").contains(&".env.local".to_string()));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_config_file_is_read() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let base = std::env::temp_dir().join(format!("rkp-depfile-{}-config", std::process::id()));
    let dir = project(&base);
    std::fs::write(dir.join("rathkit.config.mjs"), "export default {};\n").unwrap();
    let deps = depfile(&dir);
    assert!(list(&deps, "read").contains(&"rathkit.config.mjs".to_string()));
    assert!(!list(&deps, "missing").contains(&"rathkit.config.mjs".to_string()));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_file_and_the_folder_of_the_same_name_that_was_looked_for_are_two_entries() {
    let base = std::env::temp_dir().join(format!("rkp-depfile-{}-folder", std::process::id()));
    let dir = project(&base);
    // `./n.js` is named in full: the resolver also asks whether `n.js` is a folder (it is not there), which is not the file being read.
    std::fs::write(dir.join("src/m.ts"), "import { n } from \"./n.js\";\nconsole.log(n);\n").unwrap();
    let json = depfile(&dir);
    let (read, missing) = (list(&json, "read"), list(&json, "missing"));
    assert!(read.contains(&"src/n.js".to_string()), "{read:?}");
    assert!(!read.contains(&"src/n.js/".to_string()), "{read:?}");
    assert!(missing.contains(&"src/n.js/".to_string()), "{missing:?}");
    let _ = std::fs::remove_dir_all(&base);
}
