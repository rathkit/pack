//! `rkp migrate`: the tool of each project in tests/migrate is found, the commands it prints are the ones of docs/migrate/, `--write`
//! adds them to package.json without touching what is there, and the commands build the project.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/migrate").join(name)
}

/// A copy of the fixture in a folder of its own.
fn copy(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let to = std::env::temp_dir().join(format!("rkp-migrate-{}-{name}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&to);
    copy_dir(&fixture(name), &to);
    to
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn rkp(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(dir).output().unwrap()
}

fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut word, mut quoted, mut any) = (String::new(), false, false);
    for c in command.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                any = true;
            }
            ' ' if !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            _ => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

fn stdout(out: &std::process::Output) -> String {
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// The command of the script `name` in the package.json of `dir`.
fn script(dir: &Path, name: &str) -> String {
    let package = rathkit_pack::json::Json::parse(&std::fs::read_to_string(dir.join("package.json")).unwrap()).unwrap();
    package.get("scripts").and_then(|s| s.get(name)).and_then(|s| s.as_str()).unwrap_or_else(|| panic!("no script {name}")).to_string()
}

#[test]
fn each_tool_is_found_with_the_command_of_its_guide() {
    let expected = [
        ("esbuild", "build:rkp: rkp bundle src/main.js --bundle --outfile=dist/main.js --format=esm --minify --sourcemap --platform=browser --define:__APP_VERSION__='\"1.2.3\"' --loader:.txt=text --alias:@=./src"),
        ("vite", "build:rkp: rkp bundle index.html --config vite.config.js --outdir dist --minify --sourcemap"),
        ("vite", "dev:rkp: rkp serve src/main.js --config vite.config.js --hmr"),
        ("webpack", "build:rkp: rkp bundle src/main.js --config webpack.config.js --platform=browser --minify --sourcemap --outfile dist/main.js"),
        ("rollup", "build:rkp: rkp bundle src/main.js --outfile dist/main.js --sourcemap --minify"),
    ];
    for (tool, line) in expected {
        let out = stdout(&rkp(&fixture(tool), &["migrate"]));
        assert!(out.starts_with(&format!("{tool} (")), "{tool}: {out}");
        assert!(out.contains(line), "{tool}: {out}");
        assert!(out.contains(&format!("docs/migrate/{tool}.md")));
    }
}

#[test]
fn rollup_says_which_plugins_are_not_needed() {
    let out = stdout(&rkp(&fixture("rollup"), &["migrate"]));
    for plugin in ["plugin-node-resolve", "plugin-commonjs", "plugin-replace", "plugin-terser"] {
        assert!(out.contains(&format!("@rollup/{plugin}: not needed")), "{out}");
    }
    assert!(out.contains("plugins written in the config stay"));
}

#[test]
fn esbuild_api_files_and_unsupported_flags_are_named() {
    let dir = copy("esbuild");
    std::fs::write(dir.join("package.json"), "{\n  \"scripts\": { \"b\": \"esbuild a.js --outdir=out --mangle-props=^_ --serve=8000\" },\n  \"devDependencies\": { \"esbuild\": \"1\" }\n}\n").unwrap();
    std::fs::write(dir.join("watch.mjs"), "import * as esbuild from 'esbuild';\nconst ctx = await esbuild.context({});\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("b:rkp: rkp bundle a.js --outdir=out"), "{out}");
    assert!(out.contains("--mangle-props is not supported") && out.contains("--serve is not supported"), "{out}");
    assert!(out.contains("watch.mjs: context is not in @rathkit/pack"), "{out}");
    assert!(out.contains("without --bundle"), "{out}");
}

#[test]
fn write_adds_scripts_and_builds_the_project() {
    for tool in ["esbuild", "vite", "webpack"] {
        let dir = copy(tool);
        let before = std::fs::read_to_string(dir.join("package.json")).unwrap();
        let out = stdout(&rkp(&dir, &["migrate", "--write"]));
        assert!(out.contains("added build:rkp"), "{tool}: {out}");
        let after = std::fs::read_to_string(dir.join("package.json")).unwrap();
        // Everything that was there is still there, with the same values.
        let (old, new) = (rathkit_pack::json::Json::parse(&before).unwrap(), rathkit_pack::json::Json::parse(&after).unwrap());
        for (key, value) in old.get("scripts").and_then(|s| s.as_object()).unwrap_or(&[]) {
            assert_eq!(new.get("scripts").and_then(|s| s.get(key)), Some(value), "{tool}: {key}");
        }
        assert_eq!(old.get("devDependencies"), new.get("devDependencies"), "{tool}");
        // The command it wrote builds the project.
        let command = script(&dir, "build:rkp");
        let args = words(&command);
        assert_eq!(args[0], "rkp");
        let built = rkp(&dir, &args[1..].iter().map(String::as_str).collect::<Vec<_>>());
        assert!(built.status.success(), "{tool}: {command}\n{}", String::from_utf8_lossy(&built.stderr));
        let produced = if tool == "vite" { "dist/index.html" } else { "dist/main.js" };
        assert!(dir.join(produced).is_file(), "{tool}: {produced} was not written by {command}");
        // A second run leaves the scripts it wrote.
        let again = stdout(&rkp(&dir, &["migrate", "--write"]));
        assert!(again.contains("left build:rkp"), "{tool}: {again}");
        assert_eq!(std::fs::read_to_string(dir.join("package.json")).unwrap(), after);
    }
}

#[test]
fn several_tools_get_names_of_their_own() {
    let dir = copy("vite");
    std::fs::write(dir.join("webpack.config.js"), "module.exports = { entry: './src/main.js' };\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate", "--write"]));
    assert!(out.contains("added build:rkp-vite") && out.contains("added build:rkp-webpack"), "{out}");
    assert!(script(&dir, "dev:rkp-vite").starts_with("rkp serve "));
}

#[test]
fn a_project_with_none_of_them_says_so() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-none-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = rkp(&dir, &["migrate"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("no esbuild, Vite, webpack or Rollup found"));
    assert_eq!(rkp(&dir, &["migrate", "--bogus"]).status.code(), Some(2));
}
