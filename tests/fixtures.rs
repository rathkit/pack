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
