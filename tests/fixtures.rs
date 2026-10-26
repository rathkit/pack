//! Runs the differential fixture suite: every program under `tests/fixtures` must print the
//! same thing when run by Node as written and when run bundled (with and without
//! tree-shaking). Skipped when `node` is not installed.

use std::process::Command;

#[test]
fn bundled_programs_behave_like_the_originals() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the fixture suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-fixtures.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the fixture script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "fixtures disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn typescript_features_match_the_compilers_own_transform() {
    // JSX and decorators have no Node-native reference, so TypeScript's own compiler is the
    // reference. Set TS_MODULE to a `typescript` package to run this.
    let Some(module) = std::env::var_os("TS_MODULE") else {
        eprintln!("TS_MODULE not set; skipping the TypeScript comparison");
        return;
    };
    let output = Command::new("node")
        .arg("tools/run-jsx.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .env("TS_MODULE", module)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the JSX script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "programs disagree with tsc:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn split_chunks_behave_like_the_originals() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the splitting suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-split.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the splitting script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "split programs disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn defined_constants_behave_like_the_values_they_stand_for() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the define suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-defines.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the define script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "define programs disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn loaders_and_style_sheets_produce_what_they_should() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the loader suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-loaders.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the loader script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "loader programs disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn plugins_of_every_kind_run_in_the_host() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the plugin suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-plugins.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the plugin script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "plugin programs disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn watch_mode_rebuilds_and_tells_plugins() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the watch suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-watch.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the watch script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "watch mode misbehaves:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn estree_matches_acorn() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the ESTree check");
        return;
    }
    let output = Command::new("node")
        .args(["tools/check-estree.mjs", "tests", "tools"])
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the ESTree script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "ESTree output differs from acorn's:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_dev_server_serves_and_tells_pages_about_changes() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the dev server suite");
        return;
    }
    let output = Command::new("node")
        .arg("tools/run-serve.mjs")
        .env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the serve script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the dev server misbehaves:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn an_unknown_option_is_an_error_and_not_a_file_name() {
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).output().expect("rkp starts");
    for args in [&["bundle", "main.js", "--target", "es2015"][..], &["serve", "main.js", "--nope"][..], &["bundle", "-x", "main.js"][..]] {
        let out = run(args);
        let text = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {text}");
        assert!(text.contains("unknown option"), "{args:?}: {text}");
    }
    // The forms with the value attached are options, not mistakes.
    let out = run(&["bundle", "--define:A=1", "--sourcemap=inline", "--loader:.txt=text", "missing.js", "--outfile", "/dev/null"]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("unknown option"));
}

/// A fresh folder for one test.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp folder");
    dir
}

/// Builds `main.js` in `dir` with `args` and returns what the bundle prints under Node (skipped without Node).
fn build_and_run(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).envs(envs.iter().copied()).args(["bundle", "main.js", "--outfile", "out.mjs"]).args(args).output().expect("rkp starts");
    assert!(built.status.success(), "{args:?}: {}", String::from_utf8_lossy(&built.stderr));
    let run = Command::new("node").current_dir(dir).arg("out.mjs").output().expect("node starts");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn import_meta_env_has_the_mode_and_the_prefixed_names_of_the_env_files() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("env");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write(".env", "VITE_A=file\nVITE_B=base\nSECRET=hidden\nVITE_REF=${VITE_B}-ref\nAPP_X=app\n");
    write(".env.local", "VITE_B=local\n");
    write(".env.staging", "VITE_A=staging\n");
    write(
        "main.js",
        "const e = import.meta.env;\nconsole.log([e.MODE, e.DEV, e.PROD, e.SSR, e.BASE_URL, e.VITE_A, e.VITE_B, e.VITE_REF, e.SECRET, e.APP_X, Object.keys(e).length].map(String).join(' '));\n",
    );
    // A build: production, the files merged, the names without the prefix left out, a value referring to the final value of another.
    assert_eq!(build_and_run(&dir, &[], &[]), "production false true false / file local local-ref undefined undefined 8");
    // A mode of its own reads its file; a custom mode follows the command (a build is production).
    assert_eq!(build_and_run(&dir, &["--mode", "staging"], &[]), "staging false true false / staging local local-ref undefined undefined 8");
    assert_eq!(build_and_run(&dir, &["--mode", "development"], &[]), "development true false false / file local local-ref undefined undefined 8");
    // The real environment wins over the files, and --define wins over both.
    assert_eq!(build_and_run(&dir, &["--define:import.meta.env.VITE_B=\"defined\""], &[("VITE_A", "real")]), "production false true false / real defined local-ref undefined undefined 8");
    // Another prefix, another folder, and no files at all.
    assert_eq!(build_and_run(&dir, &["--env-prefix", "APP_"], &[]), "production false true false / undefined undefined undefined undefined app 6");
    assert_eq!(build_and_run(&dir, &["--no-env-files"], &[]), "production false true false / undefined undefined undefined undefined undefined 5");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn banner_footer_and_dropped_calls() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("banner");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("s.css", "p { color: red }\n");
    write("a.js", "import './s.css';\nexport const answer = () => { throw new Error('boom'); };\n");
    write("main.js", "import { answer } from './a.js';\nconsole.log('dropped');\ndebugger;\nfunction f(console) { return console.log('shadowed'); }\nprocess.stdout.write(f({ log: (m) => m }) + '\\n');\nanswer();\n");
    let rkp = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js"]).args(args).output().expect("rkp starts");
    for minify in [false, true] {
        let mut args = vec!["--outfile", "out.mjs", "--sourcemap", "--banner:js=/* one\ntwo */", "--footer:js=// the end", "--banner:css=/* css top */", "--footer:css=/* css end */", "--drop:console", "--drop:debugger"];
        if minify {
            args.push("--minify");
        }
        let built = rkp(&args);
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let js = std::fs::read_to_string(dir.join("out.mjs")).unwrap();
        assert!(js.starts_with("/* one\ntwo */\n"), "minify={minify}: {js}");
        assert!(js.contains("// the end\n"), "minify={minify}: {js}");
        assert!(!js.contains("dropped") && !js.contains("debugger"), "minify={minify}: {js}");
        let css = std::fs::read_to_string(dir.join("out.css")).unwrap();
        assert!(css.starts_with("/* css top */\n") && css.contains("/* css end */"), "{css}");
        // The banner moved every line, and the map says so: the failure is reported at its place in a.js.
        let run = Command::new("node").current_dir(&dir).args(["--enable-source-maps", "out.mjs"]).output().expect("node starts");
        let text = String::from_utf8_lossy(&run.stderr);
        assert!(String::from_utf8_lossy(&run.stdout).starts_with("shadowed"), "{text}");
        assert!(text.contains("a.js:2"), "minify={minify}: a banner broke the map:\n{text}");
    }
    // Only console and debugger can be dropped; a banner is for js or css.
    assert_eq!(rkp(&["--outfile", "x.mjs", "--drop:everything"]).status.code(), Some(2));
    assert_eq!(rkp(&["--outfile", "x.mjs", "--banner:html=x"]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn packages_external_resolve_extensions_and_symlinks() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("packages");
    std::fs::create_dir_all(dir.join("node_modules/dep")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("node_modules/dep/package.json", "{\"name\":\"dep\",\"main\":\"index.js\"}");
    write("node_modules/dep/index.js", "module.exports = 'dep-value';\n");
    write("src/aliased.js", "export default 'aliased';\n");
    write("noext.mjs", "export const x = 'by-extension';\n");
    write("main.js", "import dep from 'dep';\nimport aliased from '@src/aliased.js';\nimport { x } from './noext';\nconsole.log(dep, aliased, x);\n");
    let args = ["--alias", "@src=./src", "--resolve-extensions=.mjs,.js"];
    let bundled = build_and_run(&dir, &args, &[]);
    assert_eq!(bundled, "dep-value aliased by-extension");
    let text_of_bundle = std::fs::read_to_string(dir.join("out.mjs")).unwrap();
    assert!(text_of_bundle.contains("dep-value"), "a package is bundled by default");
    // External: the package stays an import (and is found at run time, next to the bundle); the alias and the relative path are bundled.
    let external = build_and_run(&dir, &["--alias", "@src=./src", "--resolve-extensions", ".mjs,.js", "--packages=external", "--preserve-symlinks"], &[]);
    assert_eq!(external, "dep-value aliased by-extension");
    let text_of_bundle = std::fs::read_to_string(dir.join("out.mjs")).unwrap();
    assert!(text_of_bundle.contains("from \"dep\"") && !text_of_bundle.contains("dep-value"), "{text_of_bundle}");
    let _ = std::fs::remove_dir_all(&dir);
}
