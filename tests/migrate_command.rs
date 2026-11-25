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
    // The file name is built by a function: running the config gives it exactly (no stand-in).
    assert!(out.contains("build:rkp: rkp bundle index.js --outfile x.js\n") && !out.contains("stand-in"), "{out}");
    // Without running it, the stand-in is all the text gives.
    let out = stdout(&rkp(&dir, &["migrate", "--no-run"]));
    assert!(out.contains("--outfile dist/index.js") && out.contains("stand-in"), "{out}");
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
fn rollup_values_built_in_code_are_read_by_running_the_config() {
    let dir = copy("rollup-code");
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle src/index.js --outfile dist/sample.js\n"), "{out}");
    assert!(out.contains("build:rkp-2: rkp bundle src/index.js --outfile dist/sample.cjs --format=cjs --sourcemap\n"), "{out}");
    assert!(out.contains("build:rkp-3: rkp bundle src/index.js --outfile dist/sample.umd.js --format=iife --global-name=Sample\n") , "{out}");
    assert!(out.contains("rkp bundle src/a.js src/b.js --outdir dist/split --splitting\n"), "{out}");
    assert!(out.contains("note: the values of rollup.config.js were read by running it"), "{out}");
    // Every printed command builds.
    for line in out.lines().filter_map(|l| l.trim_start().strip_prefix("build:rkp")) {
        let command = line.split_once(": ").unwrap().1;
        let args = words(command);
        let built = rkp(&dir, &args[1..].iter().map(String::as_str).collect::<Vec<_>>());
        assert!(built.status.success(), "{command}: {}", String::from_utf8_lossy(&built.stderr));
    }
    assert!(dir.join("dist/sample.cjs").is_file() && dir.join("dist/split").is_dir());
    // `--write` adds every command under a name of its own.
    stdout(&rkp(&dir, &["migrate", "--write"]));
    assert!(script(&dir, "build:rkp-2").contains("--format=cjs"));
    // `--no-run` reads the text only: the input is not a plain string, so there is no command.
    let out = stdout(&rkp(&dir, &["migrate", "--no-run"]));
    assert!(!out.contains("build:rkp") && !out.contains("were read by running it"), "{out}");
}

#[test]
fn a_config_that_writes_or_runs_programs_is_not_run_to_completion() {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-unsafe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{\"type\":\"module\",\"devDependencies\":{\"rollup\":\"1\"}}").unwrap();
    // Writes a file and starts a program while loading; under the permission model both are refused, so the config is not run and the
    // text is all there is (the input is a variable: no command).
    std::fs::write(dir.join("rollup.config.js"), "import fs from 'node:fs';\nimport { execSync } from 'node:child_process';\nconst input = 'a.js';\nfs.writeFileSync('written.txt', 'x');\nexecSync('echo hi > spawned.txt');\nexport default { input, output: { file: 'out.js' } };\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(!out.contains("were read by running it"), "{out}");
    assert!(!dir.join("written.txt").exists() && !dir.join("spawned.txt").exists());
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

/// A project with Vite, a page, the config text and the extra files given as (name, text).
fn vite_project(label: &str, config: &str, files: &[(&str, &str)]) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rkp-migrate-vite-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{\"devDependencies\":{\"vite\":\"1\"}}").unwrap();
    std::fs::write(dir.join("index.html"), "<script type=module src=/main.js></script>").unwrap();
    std::fs::write(dir.join("vite.config.js"), config).unwrap();
    for (name, text) in files {
        if let Some(parent) = Path::new(name).parent() {
            std::fs::create_dir_all(dir.join(parent)).unwrap();
        }
        std::fs::write(dir.join(name), text).unwrap();
    }
    dir
}

#[test]
fn vite_base_from_the_environment_or_a_constant_is_mapped_to_public_path() {
    // A variable that a .env file of the project sets.
    let dir = vite_project("env", "export default { base: process.env.VITE_BASE };\n", &[(".env.production", "VITE_BASE=\"/app/\"\n")]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("--minify --public-path=/app/") && !out.contains("note: base"), "{out}");
    // `loadEnv` and a fallback when nothing sets it.
    let dir = vite_project("load-env", "export default ({ mode }) => { const env = loadEnv(mode, process.cwd(), ''); return { base: env.APP_BASE || '/fallback/' }; };\n", &[]);
    assert!(stdout(&rkp(&dir, &["migrate"])).contains("--public-path=/fallback/"));
    // A variable that nothing sets: the note names it.
    let dir = vite_project("unset", "export default { base: import.meta.env.APP_BASE };\n", &[]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("note: base is not a plain string in the config: it is read from the environment variable APP_BASE") && !out.contains("--minify --public-path"), "{out}");
    // A constant of a file that the config imports, or of the config itself. An explicit `/` is kept: Vite's links are then absolute,
    // and rkp's are relative without the flag.
    let imported = |value: &str| vite_project("const", "import { VITE_BASE_PATH } from './config/constant';\nexport default { base: VITE_BASE_PATH };\n", &[("config/constant.ts", &format!("export const PORT = 1;\nexport const VITE_BASE_PATH = '{value}';\n"))]);
    assert!(stdout(&rkp(&imported("/sub/"), &["migrate"])).contains("--public-path=/sub/"));
    let out = stdout(&rkp(&imported("/"), &["migrate"]));
    assert!(out.contains("--minify --public-path=/\n") && !out.contains("note: base"), "{out}");
    let dir = vite_project("local", "const BASE = '/here/';\nexport default { base: BASE };\n", &[]);
    assert!(stdout(&rkp(&dir, &["migrate"])).contains("--public-path=/here/"));
    // A function result: the note says what the expression is.
    let dir = vite_project("call", "export default { base: computeBase() };\n", &[]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("note: base is not a plain string in the config (computeBase(), from the config)"), "{out}");
}

#[test]
fn vite_build_lib_is_mapped_to_library_commands() {
    let dir = copy("vite-lib");
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(!out.contains("rkp bundle index.html") && !out.contains("dev:rkp"), "{out}");
    assert!(out.contains("build:rkp: rkp bundle src/index.js --outfile lib/sample.js --external vue --config vite.config.js\n"), "{out}");
    assert!(out.contains("build:rkp-2: rkp bundle src/index.js --format=cjs --outfile lib/sample.cjs --external vue --config vite.config.js\n"), "{out}");
    assert!(out.contains("build:rkp-3: rkp bundle src/index.js --format=iife --global-name=SampleLib --global vue=Vue --outfile lib/sample.umd.cjs --external vue --config vite.config.js\n"), "{out}");
    assert!(out.contains("note: build.lib format umd: rkp has no UMD wrapper"), "{out}");
    for line in out.lines().filter_map(|l| l.trim_start().strip_prefix("build:rkp")) {
        let command = line.split_once(": ").unwrap().1;
        let args = words(command);
        let built = rkp(&dir, &args[1..].iter().map(String::as_str).collect::<Vec<_>>());
        assert!(built.status.success(), "{command}: {}", String::from_utf8_lossy(&built.stderr));
    }
    for file in ["sample.js", "sample.cjs", "sample.umd.cjs"] {
        assert!(dir.join("lib").join(file).is_file(), "{file}");
    }
    // Several entries and the default formats (es and cjs): one command with an output folder.
    let dir = vite_project("lib-many", "export default { build: { lib: { entry: ['src/a.js', 'src/b.js'] } } };\n", &[]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle src/a.js src/b.js --format esm,cjs --outdir dist --config vite.config.js\n"), "{out}");
}

#[test]
fn vite_multi_page_input_is_mapped_to_several_entries() {
    let dir = copy("vite-multipage");
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle index.html about.html --config vite.config.js --outdir dist --minify\n"), "{out}");
    let args = words(out.lines().find_map(|l| l.trim_start().strip_prefix("build:rkp: ")).unwrap());
    let built = rkp(&dir, &args[1..].iter().map(String::as_str).collect::<Vec<_>>());
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    assert!(dir.join("dist/index.html").is_file() && dir.join("dist/about.html").is_file());
    // Pages with the same file name in different folders keep their folders in the output.
    let dir = vite_project("clash", "export default { build: { rollupOptions: { input: { a: 'index.html', b: 'nested/index.html' } } } };\n", &[]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle index.html nested/index.html --config vite.config.js"), "{out}");
}

#[test]
fn a_pattern_in_the_input_is_not_a_page_and_a_config_in_a_folder_finds_the_page_beside_it() {
    let dir = vite_project("glob", "export default { build: { rollupOptions: { input: ['*.html', 'pages/**/*.html'] } } };\n", &[]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle index.html --config vite.config.js") && out.contains("does not expand patterns"), "{out}");
    // The project has no `index.html` of its own: the page is the one beside the config.
    let dir = std::env::temp_dir().join(format!("rkp-migrate-vite-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("demo")).unwrap();
    std::fs::write(dir.join("package.json"), "{\"devDependencies\":{\"vite\":\"1\"},\"scripts\":{\"dev\":\"vite --config demo/vite.config.js\"}}").unwrap();
    std::fs::write(dir.join("demo/index.html"), "<script type=module src=/demo/main.js></script>").unwrap();
    std::fs::write(dir.join("demo/vite.config.js"), "export default {};\n").unwrap();
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle demo/index.html --config demo/vite.config.js"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-migrate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    dir
}

#[test]
fn an_esbuild_script_that_builds_with_plain_strings_gets_a_command_for_each_format_it_can_write() {
    let script = "import * as esbuild from 'esbuild';\nfunction config(options) {\n  return { entryPoints: ['src/lib.ts'], bundle: true, minify: true, ...options };\n}\n\nawait esbuild.build(config({\n  format: 'esm',\n  outfile: 'lib/lib.esm.js',\n}));\n\nawait esbuild.build(config({\n  format: 'cjs',\n  outfile: 'lib/lib.cjs',\n}));\n\nawait esbuild.build(config({\n  format: 'umd',\n  outfile: 'lib/lib.umd.js',\n}));\n";
    let dir = project("esbuild-plain-strings", &[("package.json", "{\"scripts\":{\"build\":\"node esbuild.config.js\"},\"devDependencies\":{\"esbuild\":\"1\"}}"), ("esbuild.config.js", script)]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.contains("build:rkp: rkp bundle src/lib.ts --outfile lib/lib.esm.js --minify\n"), "{out}");
    assert!(out.contains("build:rkp-2: rkp bundle src/lib.ts --format=cjs --outfile lib/lib.cjs --minify\n"), "{out}");
    // The format it cannot write has no command, and says why.
    assert!(!out.contains("lib.umd.js --minify") && out.contains("format 'umd'"), "{out}");
    assert!(out.contains("the commands come from the entry points and the plain strings of the build() calls"), "{out}");
}

#[test]
fn a_rollup_config_that_is_a_function_gets_its_commands_from_the_script_that_calls_it() {
    let config = "const { terser } = require('rollup-plugin-terser')\nmodule.exports = (config) => {\n  const { input, fileName, name } = config\n  return { input: { input, plugins: [terser()] }, output: { file: fileName, format: 'umd', name: name || 'lib' } }\n}\n";
    let driver = "const configFactory = require('./rollup.config')\nfor (const l of locales) {\n  build(configFactory({ input: `./src/locale/${l}`, fileName: `./locale/${l}` }))\n}\nbuild(configFactory({\n  input: './src/index.js',\n  fileName: './lib.min.js'\n}))\n";
    let dir = project("rollup-driven-by-script", &[("package.json", "{\"scripts\":{\"build\":\"node build\"},\"devDependencies\":{\"rollup\":\"4\"}}"), ("build/rollup.config.js", config), ("build/index.js", driver)]);
    let out = stdout(&rkp(&dir, &["migrate", "--no-run"]));
    // The call whose names are made in a loop is left out; the one with plain strings is the command.
    assert!(out.contains("build:rkp: rkp bundle src/index.js --outfile lib.min.js --format=iife --global-name=lib --minify\n"), "{out}");
    assert!(!out.contains("locale") && !out.contains("stand-in"), "{out}");
}

#[test]
fn a_package_with_a_source_field_gets_a_command_for_each_file_it_names() {
    let core = "{\"name\":\"core\",\"source\":\"src/index.ts\",\"main\":\"dist/core.js\",\"module\":\"dist/core.module.js\",\"unpkg\":\"dist/core.min.js\",\"types\":\"dist/core.d.ts\",\"exports\":{\".\":{\"types\":\"./dist/core.d.ts\",\"import\":\"./dist/core.mjs\",\"require\":\"./dist/core.js\"}}}";
    let dir = project("package-source-field", &[("package.json", "{\"private\":true,\"devDependencies\":{\"microbundle\":\"0.15.1\"}}"), ("pnpm-workspace.yaml", "packages:\n  - \"packages/*\"\n"), ("packages/core/package.json", core), ("packages/core/src/index.ts", "export const a = 1;\n"), ("packages/other/package.json", "{\"name\":\"other\"}")]);
    let out = stdout(&rkp(&dir, &["migrate"]));
    assert!(out.starts_with("microbundle (dependency microbundle, source in packages/core/package.json)"), "{out}");
    assert!(out.contains("build:rkp: rkp bundle packages/core/src/index.ts --outfile packages/core/dist/core.module.js\n"), "{out}");
    assert!(out.contains("build:rkp-2: rkp bundle packages/core/src/index.ts --outfile packages/core/dist/core.js --format=cjs\n"), "{out}");
    assert!(out.contains("build:rkp-3: rkp bundle packages/core/src/index.ts --outfile packages/core/dist/core.mjs\n"), "{out}");
    assert!(out.contains("dist/core.min.js is a umd file"), "{out}");
    // The built files are the same as what the commands say: run one.
    let built = rkp(&dir, &["bundle", "packages/core/src/index.ts", "--outfile", "packages/core/dist/core.module.js"]);
    assert!(built.status.success() && dir.join("packages/core/dist/core.module.js").is_file());
}
