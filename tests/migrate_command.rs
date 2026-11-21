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
    assert!(out.contains("watch.mjs:1: `import * as esbuild from 'esbuild';` becomes `import * as esbuild from '@rathkit/pack';`"), "{out}");
    assert!(out.contains("watch.mjs:2: `context()` has no equivalent: there is no context object"), "{out}");
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
    assert!(String::from_utf8_lossy(&out.stdout).contains("no esbuild, Vite, webpack, Rollup or rspack found"));
    assert_eq!(rkp(&dir, &["migrate", "--bogus"]).status.code(), Some(2));
}

#[test]
fn vitest_only_and_inputless_projects_get_no_build_command() {
    let base = std::env::temp_dir().join(format!("rkp-migrate-corpus-{}", std::process::id()));
    // vite as a dependency of the test setup, no page, no config: not a Vite build.
    let lib = base.join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("package.json"), "{\"devDependencies\":{\"vite\":\"1\"}}").unwrap();
    assert_eq!(rkp(&lib, &["migrate"]).status.code(), Some(1));
    // vite.config that only configures Vitest.
    std::fs::write(lib.join("vite.config.ts"), "import { defineConfig } from \"vitest/config\";\nexport default defineConfig({ test: {} });\n").unwrap();
    assert_eq!(rkp(&lib, &["migrate"]).status.code(), Some(1));
    // rollup without a plain input: no command that would print the usage text.
    let roll = base.join("roll");
    std::fs::create_dir_all(&roll).unwrap();
    std::fs::write(roll.join("package.json"), "{\"devDependencies\":{\"rollup\":\"1\"}}").unwrap();
    let out = stdout(&rkp(&roll, &["migrate"]));
    assert!(out.starts_with("rollup (") && !out.contains("build:rkp"), "{out}");
}

#[test]
fn rspack_is_found() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-rspack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{\"scripts\":{}}").unwrap();
    std::fs::write(dir.join("rspack.config.js"), "export default { entry: { main: './index.js' } };\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.starts_with("rspack (rspack.config.js)"), "{out}");
    assert!(out.contains("build:rkp: rkp bundle index.js --config rspack.config.js --outdir dist"), "{out}");
}

#[test]
fn rollup_with_the_output_built_in_code_gets_an_outfile() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-rollup-code-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{\"scripts\":{}}").unwrap();
    std::fs::write(dir.join("rollup.config.js"), "const output = (file) => ({ input: 'index.js', output: { name: 'X', file } });\nexport default [output('x.js')];\n").unwrap();
    std::fs::write(dir.join("index.js"), "export const x = 1;\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle index.js --global-name=X --outfile dist/index.js") || out.contains("--outfile dist/index.js"), "{out}");
}

#[test]
fn vite_base_from_a_variable_is_a_note() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-vite-base-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{\"devDependencies\":{\"vite\":\"1\"}}").unwrap();
    std::fs::write(dir.join("index.html"), "<script type=module src=/src/main.js></script>").unwrap();
    std::fs::write(dir.join("vite.config.js"), "const b = process.env.BASE;\nexport default { base: b };\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("note: base is not a plain string"), "{out}");
}

#[test]
fn a_config_in_a_subfolder_is_found_by_the_script_or_by_a_search() {
    // Named by a package.json script (`rollup -c build/rollup.config.js`).
    let dir = copy("rollup-subfolder");
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.starts_with("rollup (build/rollup.config.js"), "{out}");
    assert!(out.contains("build:rkp: rkp bundle src/index.js --outfile dist/index.js --format=cjs"), "{out}");
    assert!(out.contains("note: the config is build/rollup.config.js, in a subfolder"), "{out}");
    // Not named by a script: found by the search, two levels deep; a config of a test folder is not the project's.
    let dir = std::env::temp_dir().join(format!("rkp-migrate-search-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("tests/x")).unwrap();
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(dir.join("package.json"), "{\"scripts\":{\"build\":\"make\"}}").unwrap();
    std::fs::write(dir.join("tests/x/rollup.config.js"), "export default { input: 'no.js', output: { file: 'no.js' } };\n").unwrap();
    assert_eq!(rkp(&dir, &["migrate"]).status.code(), Some(1));
    std::fs::write(dir.join("config/rollup.config.mjs"), "export default { input: 'a.js', output: { file: 'out/a.js' } };\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.starts_with("rollup (config/rollup.config.mjs)") && out.contains("rkp bundle a.js --outfile out/a.js"), "{out}");
    // A path that leaves the project in a script is not followed.
    std::fs::remove_file(dir.join("config/rollup.config.mjs")).unwrap();
    std::fs::write(dir.join("package.json"), "{\"scripts\":{\"build\":\"rollup -c ../rollup.config.js\"}}").unwrap();
    assert_eq!(rkp(&dir, &["migrate"]).status.code(), Some(1));
}

#[test]
fn esbuild_api_scripts_get_the_lines_that_change() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-esbuild-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    // A script of a subfolder that a package.json script runs, in the style of a library's release build.
    std::fs::write(dir.join("package.json"), "{\"scripts\":{\"build\":\"node scripts/build.mjs\"},\"devDependencies\":{\"esbuild\":\"1\"}}").unwrap();
    std::fs::write(
        dir.join("scripts/build.mjs"),
        "import * as esbuild from 'esbuild';\nimport { umdWrapper } from 'esbuild-plugin-umd-wrapper';\n// build( in a comment\nawait esbuild.build({ entryPoints: ['a.js'], bundle: true, outfile: 'x.js', plugins: [umdWrapper()] });\nconst r = await esbuild.build({ entryPoints: ['b.js'], write: false });\nconst code = esbuild.transformSync('x');\n",
    )
    .unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.starts_with("esbuild (dependency esbuild, API in scripts/build.mjs)"), "{out}");
    assert!(out.contains("note: scripts/build.mjs:1: `import * as esbuild from 'esbuild';` becomes `import * as esbuild from '@rathkit/pack';`"), "{out}");
    assert!(out.contains("note: scripts/build.mjs: build() at lines 4, 5 is the same call in @rathkit/pack"), "{out}");
    assert!(out.contains("note: scripts/build.mjs:5: `write: false` and `outputFiles` are not done"), "{out}");
    assert!(out.contains("note: scripts/build.mjs:6: `transformSync()` has no equivalent: there is no transform API"), "{out}");
    assert!(out.contains("note: scripts/build.mjs:2: the plugin esbuild-plugin-umd-wrapper stays in `plugins`"), "{out}");
    assert!(!out.contains("build.mjs:3"), "{out}");
}
