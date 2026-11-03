//! Runs the differential fixture suite: every program under `tests/fixtures` must print the
//! same thing when run by Node as written and when run bundled (with and without
//! tree-shaking). Skipped when `node` is not installed.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// How long one script of `tools/` may run in these tests. They take seconds to a minute; a hang (a race, a port that is never
/// answered) must cost this, with what the script printed, and not the 30 minutes a CI job is allowed.
const SCRIPT_LIMIT: Duration = Duration::from_secs(300);

trait OutputWithin {
    /// Like `output`, but a run that is not over in `SCRIPT_LIMIT` is killed (with what it started) and is an error that holds what it printed.
    fn output_within(&mut self) -> std::io::Result<Output> {
        self.output_for(SCRIPT_LIMIT)
    }
    fn output_for(&mut self, limit: Duration) -> std::io::Result<Output>;
}

impl OutputWithin for Command {
    fn output_for(&mut self, limit: Duration) -> std::io::Result<Output> {
        self.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(self, 0);
        let mut child = self.spawn()?;
        let read = |mut pipe: Box<dyn Read + Send>| std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        });
        let stdout = read(Box::new(child.stdout.take().expect("piped")));
        let stderr = read(Box::new(child.stderr.take().expect("piped")));
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() > limit {
                break None;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if status.is_none() {
            // The script's own children (servers, browsers) are in its group, so they end with it and close the pipes.
            #[cfg(unix)]
            let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).status();
            // Windows has no groups here: end the script and everything it started, which also closes the pipes.
            #[cfg(windows)]
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &child.id().to_string()]).status();
            let _ = child.kill();
            let _ = child.wait();
        }
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        match status {
            Some(status) => Ok(Output { status, stdout, stderr }),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("not over in {} seconds, and killed\nstdout:\n{}\nstderr:\n{}", limit.as_secs(), String::from_utf8_lossy(&stdout), String::from_utf8_lossy(&stderr)),
            )),
        }
    }
}

#[test]
fn a_script_that_hangs_is_killed_and_what_it_printed_is_kept() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    // A script that prints, starts a child that would outlive it, and then waits for ever.
    let script = "console.log('before the hang'); require('node:child_process').spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'inherit' }); setInterval(() => {}, 1000);";
    let started = Instant::now();
    let error = Command::new("node").args(["-e", script]).output_for(Duration::from_secs(2)).expect_err("it never ends");
    assert!(started.elapsed() < Duration::from_secs(30), "the wait for the pipes did not end: its child was left running");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("before the hang"), "what it printed is in the error: {error}");
}

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
        .output_within()
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
        .output_within()
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
        .output_within()
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
        .output_within()
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
        .output_within()
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
        .output_within()
        .expect("run the plugin script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "plugin programs disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_page_checker_finds_what_a_visitor_would_hit() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node not found; skipping the page checker");
        return;
    }
    let output = Command::new("node")
        .arg("tools/test-browser-check.mjs")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output_within()
        .expect("run the page checker script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the page checker misbehaves:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
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
        .output_within()
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
        .output_within()
        .expect("run the serve script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the dev server misbehaves:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn an_unknown_option_is_an_error_and_not_a_file_name() {
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).output().expect("rkp starts");
    for args in [&["bundle", "main.js", "--bogus", "es2015"][..], &["serve", "main.js", "--nope"][..], &["bundle", "-x", "main.js"][..]] {
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

#[test]
fn the_metafile_lists_inputs_outputs_and_what_connects_them() {
    use rathkit_pack::json::Json;
    let dir = scratch("metafile");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("main.js", "import './s.css';\nimport { a } from './lib/a.js';\nexport const load = () => import('./lazy.js');\nconsole.log(a);\n");
    write("lib/a.js", "export const a = 1;\n");
    write("lazy.js", "export const l = 'lazy';\n");
    write("s.css", "p { color: red }\n");
    let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js", "--outdir", "out", "--splitting", "--metafile=meta/build.json"]).output().expect("rkp starts");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let meta = Json::parse(&std::fs::read_to_string(dir.join("meta/build.json")).unwrap()).unwrap();
    let inputs = meta.get("inputs").unwrap();
    for name in ["main.js", "lib/a.js", "lazy.js", "s.css"] {
        assert!(inputs.get(name).is_some(), "input {name} is among {}", meta.get("inputs").map_or(String::new(), |i| match i { Json::Object(f) => f.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", "), _ => String::new() }));
    }
    let imports: Vec<String> = inputs.get("main.js").unwrap().get("imports").and_then(Json::as_array).unwrap().iter().map(|i| format!("{}:{}", i.get("path").and_then(Json::as_str).unwrap(), i.get("kind").and_then(Json::as_str).unwrap())).collect();
    assert_eq!(imports, ["s.css:import-statement", "lib/a.js:import-statement", "lazy.js:dynamic-import"]);
    let outputs = meta.get("outputs").unwrap();
    let main = outputs.get("main.js").expect("the entry file");
    assert_eq!(main.get("entryPoint").and_then(Json::as_str), Some("main.js"));
    assert_eq!(main.get("bytes").and_then(Json::as_f64), Some(std::fs::metadata(dir.join("out/main.js")).unwrap().len() as f64));
    assert!(main.get("inputs").unwrap().get("lib/a.js").is_some());
    let lazy = match outputs {
        Json::Object(fields) => fields.iter().find(|(k, _)| k.starts_with("lazy-")).map(|(_, v)| v),
        _ => None,
    };
    assert!(lazy.is_some_and(|l| l.get("inputs").unwrap().get("lazy.js").is_some()), "the lazy file holds lazy.js");
    assert!(main.get("imports").and_then(Json::as_array).unwrap().iter().any(|i| i.get("kind").and_then(Json::as_str) == Some("dynamic-import")));
    assert!(outputs.get("main.css").unwrap().get("inputs").unwrap().get("s.css").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_call_given_to_pure_is_removed_when_unused() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("pure");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("lib.js", "export const styled = { div: (s) => { console.log('styled.div ran'); return s; } };\nexport function make(x) { console.log('make ran'); return x; }\n");
    write(
        "main.js",
        "import { styled, make } from './lib.js';\nconst A = styled.div`unused`;\nconst B = make(1);\nconst C = make(effect());\nconst D = make(2);\nconsole.log('used', D);\nfunction effect() { console.log('effect ran'); return 2; }\n",
    );
    // Nothing is pure by default: every call runs.
    assert_eq!(build_and_run(&dir, &[], &[]), "styled.div ran\nmake ran\neffect ran\nmake ran\nmake ran\nused 2");
    // A name, and a chain of names, given to --pure: unused calls of them go; one whose arguments have effects stays whole; a used one stays.
    assert_eq!(build_and_run(&dir, &["--pure:styled.div", "--pure", "make"], &[]), "effect ran\nmake ran\nmake ran\nused 2");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_are_named_by_templates_and_the_hash_follows_what_they_run() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("names");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("shared.js", "export const shared = 'shared';\n");
    write("a.js", "import './a.css';\nimport { shared } from './shared.js';\nexport const lazy = () => import('./lazy.js');\nconsole.log('a', shared);\nlazy().then((m) => console.log(m.default));\n");
    write("b.js", "import { shared } from './shared.js';\nconsole.log('b', shared);\n");
    write("lazy.js", "import { shared } from './shared.js';\nexport default 'lazy ' + shared;\n");
    write("a.css", "p { color: red }\n");
    let flags = ["--outdir", "out", "--splitting", "--entry-names=[name]-[hash]", "--chunk-names=asset-[name]-[hash]", "--out-extension:.js=.mjs", "--out-extension:.css=.styles"];
    let build = |out: &str| {
        let mut args = vec!["bundle", "a.js", "b.js"];
        args.extend(flags.iter().map(|f| if *f == "out" { out } else { *f }));
        let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(args).output().expect("rkp starts");
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let mut names: Vec<String> = std::fs::read_dir(dir.join(out)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };
    let first = build("out1");
    // Entries `name-hash`, the rest `asset-name-hash`, the scripts `.mjs`, the sheet `.styles`.
    let shape = |name: &str, prefix: &str, ext: &str| name.starts_with(prefix) && name.ends_with(ext) && name.len() == prefix.len() + 8 + ext.len();
    assert!(first.iter().any(|n| shape(n, "a-", ".mjs")) && first.iter().any(|n| shape(n, "b-", ".mjs")) && first.iter().any(|n| shape(n, "a-", ".styles")), "{first:?}");
    assert!(first.iter().any(|n| shape(n, "asset-chunk-", ".mjs")) && first.iter().any(|n| shape(n, "asset-lazy-", ".mjs")), "{first:?}");
    assert_eq!(first.len(), 5, "{first:?}");
    // The program runs under those names (the files load one another by them).
    let entry = first.iter().find(|n| shape(n, "a-", ".mjs")).unwrap();
    let run = Command::new("node").current_dir(dir.join("out1")).arg(entry).output().expect("node starts");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "a shared\nlazy shared\n", "{}", String::from_utf8_lossy(&run.stderr));
    // The same build has the same names; a change in a module two files share changes the names of the files that run it, entry and lazy chunk alike.
    assert_eq!(build("out2"), first);
    write("shared.js", "export const shared = 'changed';\n");
    let changed = build("out3");
    assert!(first.iter().all(|n| !changed.contains(n)), "{first:?} -> {changed:?}");
    // Not supported: a directory in a name.
    let bad = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "a.js", "--outdir", "x", "--splitting", "--chunk-names=sub/[name]"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn legal_comments_survive_minifying_where_they_are_told_to() {
    let dir = scratch("legal");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("a.js", "/*! lib-a v1 (c) A. MIT */\nexport const a = () => 'a';\n");
    write("b.js", "/**\n * @license B\n * Apache-2.0\n */\nexport const b = () => 'b';\n");
    write("main.js", "// a plain comment\nimport { a } from './a.js';\nimport { b } from './b.js';\nconsole.log(a(), b());\n");
    let build = |extra: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js", "--outfile", "out.mjs"]).args(extra).output().expect("rkp starts");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        (std::fs::read_to_string(dir.join("out.mjs")).unwrap(), std::fs::read_to_string(dir.join("out.mjs.LEGAL.txt")).ok())
    };
    let _ = std::fs::remove_file(dir.join("out.mjs.LEGAL.txt"));
    for minify in [&["--minify"][..], &[][..]] {
        let with = |flag: &str| {
            let mut args = minify.to_vec();
            if !flag.is_empty() {
                args.push(flag);
            }
            let _ = std::fs::remove_file(dir.join("out.mjs.LEGAL.txt"));
            build(&args)
        };
        // By default the comments come after the code, each once, and the plain comment does not.
        let (code, file) = with("");
        assert!(code.contains("/*! lib-a v1 (c) A. MIT */") && code.contains("@license B") && !code.contains("a plain comment"), "{minify:?}: {code}");
        assert_eq!(code.matches("lib-a v1").count(), 1, "{code}");
        assert!(file.is_none());
        let (code, file) = with("--legal-comments=none");
        assert!(!code.contains("lib-a") && !code.contains("@license"), "{code}");
        assert!(file.is_none());
        let (code, file) = with("--legal-comments=linked");
        assert!(code.contains("For license information please see out.mjs.LEGAL.txt") && !code.contains("Apache-2.0"), "{code}");
        assert!(file.is_some_and(|f| f.contains("lib-a v1") && f.contains("@license B")));
        let (code, file) = with("--legal-comments=external");
        assert!(!code.contains("LEGAL") && !code.contains("Apache-2.0"), "{code}");
        assert!(file.is_some_and(|f| f.contains("Apache-2.0")));
    }
    let bad = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js", "--outfile", "x.mjs", "--legal-comments=inline"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_injected_export_stands_in_for_the_global_of_its_name() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("inject");
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("shim.js", "export const Buffer = { from: (s) => 'shim:' + s };\nexport const Unused = 'not used anywhere';\n");
    write("esm.js", "console.log(Buffer.from('esm'));\n");
    write("cjs.js", "console.log(Buffer.from('cjs'));\nmodule.exports = 1;\n");
    write("local.js", "const Buffer = { from: (s) => 'local:' + s };\nconsole.log(Buffer.from('local'));\n");
    write("main.js", "import './esm.js';\nimport './local.js';\nrequire('./cjs.js');\n");
    assert_eq!(build_and_run(&dir, &["--inject:./shim.js"], &[]), "shim:esm\nlocal:local\nshim:cjs");
    // A program that uses none of the names does not have the file in its bundle.
    write("main.js", "console.log(typeof process);\n");
    assert_eq!(build_and_run(&dir, &["--inject", "./shim.js"], &[]), "object");
    assert!(!std::fs::read_to_string(dir.join("out.mjs")).unwrap().contains("shim:"));
    // One that uses the global without the option gets Node's own.
    write("main.js", "console.log(typeof Buffer.from('x'));\n");
    assert_eq!(build_and_run(&dir, &[], &[]), "object");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn jsx_dev_tells_the_runtime_where_each_element_is_and_a_raw_tsconfig_replaces_the_file() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("jsxdev");
    std::fs::create_dir_all(dir.join("node_modules/react")).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("node_modules/react/package.json", "{\"name\":\"react\",\"exports\":{\"./jsx-dev-runtime\":\"./dev.js\",\"./jsx-runtime\":\"./prod.js\"}}");
    write(
        "node_modules/react/dev.js",
        "export const Fragment = Symbol('Fragment');\nexport function jsxDEV(type, props, key, isStatic, source) { return [typeof type === 'string' ? type : 'fragment', key, isStatic, source.fileName.replace(/^.*[\\\\/]/, ''), source.lineNumber, source.columnNumber].join(' '); }\n",
    );
    write("node_modules/react/prod.js", "export const Fragment = Symbol('Fragment');\nexport const jsx = () => 'prod';\nexport const jsxs = () => 'prods';\n");
    write("main.jsx", "console.log(<div id=\"x\" />);\nconsole.log(<ul key=\"k\">{'a'}{'b'}</ul>);\nconsole.log(\n    <>\n      <i>one</i>\n    </>);\n");
    let dev = "div  false main.jsx 1 13\nul k true main.jsx 2 13\nfragment  false main.jsx 4 5";
    let run = |args: &[&str]| {
        let built = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.jsx", "--outfile", "out.mjs"]).args(args).output().expect("rkp starts");
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        String::from_utf8_lossy(&Command::new("node").current_dir(&dir).arg("out.mjs").output().unwrap().stdout).trim().to_string()
    };
    assert_eq!(run(&["--jsx-dev"]), dev);
    assert_eq!(run(&[]), "prod\nprods\nprod");
    // A tsconfig.json that says react-jsxdev does the same; a raw tsconfig is used in its place.
    write("tsconfig.json", "{ \"compilerOptions\": { \"jsx\": \"react-jsxdev\" } }");
    assert_eq!(run(&[]), dev);
    assert_eq!(run(&["--tsconfig-raw", "{\"compilerOptions\":{\"jsx\":\"react-jsx\"}}"]), "prod\nprods\nprod");
    let bad = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.jsx", "--outfile", "x.mjs", "--tsconfig-raw", "{not json"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wasm_init_instantiates_a_copied_module() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let dir = scratch("wasm");
    // (module (func (export "add") (param i32 i32) (result i32) local.get 0 local.get 1 i32.add))
    let module = [0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, 1, 7, 1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 3, 2, 1, 0, 7, 7, 1, 3, 0x61, 0x64, 0x64, 0, 0, 10, 9, 1, 7, 0, 0x20, 0, 0x20, 1, 0x6a, 0x0b];
    std::fs::write(dir.join("add.wasm"), module).unwrap();
    std::fs::write(dir.join("main.js"), "import init from './add.wasm?init';\nimport url from './add.wasm';\nconst instance = await init({});\nconsole.log(instance.exports.add(2, 3), typeof url, url.endsWith('.wasm'));\n").unwrap();
    for extra in [&[][..], &["--minify"][..]] {
        assert_eq!(build_and_run(&dir, extra, &[]), "5 string true");
        // The module is copied next to the bundle, and found from there whatever the working directory is.
        assert!(std::fs::read_dir(&dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("add-")));
        let elsewhere = Command::new("node").current_dir(std::env::temp_dir()).arg(dir.join("out.mjs")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&elsewhere.stdout).trim(), "5 string true", "{}", String::from_utf8_lossy(&elsewhere.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn syntax_newer_than_the_target_is_rewritten_and_checked() {
    // Programs run as written and bundled for each target must print the same, and the bundle must parse as its target's language.
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let output = Command::new("node").arg("tools/run-target.mjs").env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp")).current_dir(env!("CARGO_MANIFEST_DIR")).output_within().expect("run the target script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "target builds disagree:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn plugin_objects_given_to_the_js_api_run_in_the_calling_process() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let output = Command::new("node").arg("tools/test-plugin-objects.mjs").env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp")).current_dir(env!("CARGO_MANIFEST_DIR")).output_within().expect("run the plugin objects script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "plugin objects are wrong:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn the_dev_server_speaks_https_with_a_certificate_of_its_own_or_a_given_one() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let output = Command::new("node").arg("tools/run-https.mjs").env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp")).current_dir(env!("CARGO_MANIFEST_DIR")).output_within().expect("run the https script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "https is wrong:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn changed_modules_are_replaced_in_the_page_with_hmr() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let output = Command::new("node").arg("tools/run-hmr.mjs").env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp")).current_dir(env!("CARGO_MANIFEST_DIR")).output_within().expect("run the hmr script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "hmr is wrong:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn declaration_files_are_written_for_the_entries_and_checked_by_a_consumer() {
    if Command::new("node").arg("--version").output().is_err() {
        return;
    }
    let output = Command::new("node").arg("tools/run-dts.mjs").env("RATHKIT_PACK_BIN", env!("CARGO_BIN_EXE_rkp")).current_dir(env!("CARGO_MANIFEST_DIR")).output_within().expect("run the dts script");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "declarations are wrong:\n{text}\n{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn what_a_target_cannot_have_stops_the_build_and_is_named() {
    let dir = scratch("target-errors");
    let rkp = |source: &str, target: &str| {
        std::fs::write(dir.join("main.js"), source).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js", "--outfile", "out.mjs", "--target", target]).output().expect("rkp starts");
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    for (source, target, words) in [
        ("export const f = 1n;", "es2019", "a BigInt literal is not available in the target es2019"),
        ("export const t = await Promise.resolve(1);", "es2021", "top-level await is not available in the target es2021"),
        ("export const q = 1; for await (const x of y) {}", "es2017", "top-level await is not available in the target es2017"),
        ("const o = { ...a, __proto__: null }; export { o };", "es2017", "a __proto__ property after a spread cannot be rewritten"),
        ("const x = 1; class A { y = x; constructor(x) { this.z = x; } } export { A };", "es2021", "a parameter of the constructor would hide"),
        ("class B { static m() {} } export class A extends B { static f = super.m(); }", "es2021", "a static field that uses super cannot be rewritten"),
        ("class B { m() {} } export class A extends B { #p() { return super.m(); } }", "es2021", "a private method that uses super cannot be rewritten"),
        ("export class A { #t(s) { return s; } r() { return this.#t`hi`; } }", "es2021", "a tagged template of a private member cannot be rewritten"),
    ] {
        let (code, stderr) = rkp(source, target);
        assert_eq!(code, Some(1), "{source}: {stderr}");
        assert!(stderr.contains(words), "{source}: {stderr}");
    }
    // The same text is fine for a target that has it, and the target must be one.
    assert_eq!(rkp("export const f = 1n;", "es2020").0, Some(0));
    // An engine target says the same by version: Chrome 80 has `??` and `?.` but not `??=`, and Safari 13.0 has neither.
    for (engine, source) in [("chrome80", "let a, b; a ??= b; export { a };"), ("safari13", "export const f = 1n;")] {
        let (code, stderr) = rkp(source, engine);
        let expected_error = engine == "safari13";
        assert_eq!(code, Some(if expected_error { 1 } else { 0 }), "{engine}: {stderr}");
        if expected_error {
            assert!(stderr.contains("a BigInt literal is not available in the target safari13"), "{stderr}");
        }
    }
    let (code, stderr) = rkp("export const a = null ?? 1; export const b = a?.x;", "chrome80,safari12");
    assert_eq!(code, Some(0), "{stderr}");
    // Top-level await is rewritten when the program is one file with nothing exported, and refused otherwise.
    assert_eq!(rkp("const t = await Promise.resolve(1); console.log(t);", "es2021").0, Some(0));
    assert_eq!(rkp("const t = await Promise.resolve(1); console.log(t);", "es2015").0, Some(0));
    std::fs::write(dir.join("main.js"), "const t = await Promise.resolve(1); console.log(t);").unwrap();
    let split = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.js", "--outdir", "out", "--splitting", "--target", "es2021"]).output().expect("rkp starts");
    assert_eq!(split.status.code(), Some(1), "{}", String::from_utf8_lossy(&split.stderr));
    assert!(String::from_utf8_lossy(&split.stderr).contains("the output is split"), "{}", String::from_utf8_lossy(&split.stderr));
    for bad in ["es5", "chrome", "netscape4", "es2099"] {
        let (code, stderr) = rkp("export const x = 1;", bad);
        assert_eq!(code, Some(2), "{bad}: {stderr}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_page_has_node_env_as_a_constant_and_a_program_for_node_keeps_it() {
    let dir = std::env::temp_dir().join(format!("rkp-node-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><script type=\"module\" src=\"/main.js\"></script>").unwrap();
    std::fs::write(dir.join("main.js"), "import legacy from './legacy.cjs';\nif (process.env.NODE_ENV === 'production') console.log('PROD_BRANCH', legacy); else console.log('DEV_BRANCH');\n").unwrap();
    // A CommonJS file that looks at `require` makes a Node program load Node's `module`; a page cannot.
    std::fs::write(dir.join("legacy.cjs"), "module.exports = typeof require;\n").unwrap();
    let build = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(&dir).env_remove("NODE_ENV").output().expect("rkp starts");
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let read = |path: &str| std::fs::read_to_string(dir.join(path)).unwrap();
    // A page: a browser has no `process`, so the build decides, and the branch that is not taken is gone.
    build(&["bundle", "index.html", "--outdir", "page"]);
    let page = std::fs::read_dir(dir.join("page")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "js")).expect("a script was written");
    let script = std::fs::read_to_string(page).unwrap();
    assert!(!script.contains("process.env") && script.contains("PROD_BRANCH") && !script.contains("DEV_BRANCH"), "{script}");
    assert!(!script.contains("node:module") && !script.contains("node:url"), "a page may not import Node's modules: {script}");
    // What the user defines wins.
    build(&["bundle", "index.html", "--outdir", "page2", "--define", "process.env.NODE_ENV=\"development\""]);
    let page = std::fs::read_dir(dir.join("page2")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "js")).unwrap();
    let script = std::fs::read_to_string(page).unwrap();
    assert!(script.contains("DEV_BRANCH") && !script.contains("PROD_BRANCH"), "{script}");
    // A program for Node reads the variable when it runs.
    build(&["bundle", "main.js", "--outfile", "node.mjs"]);
    assert!(read("node.mjs").contains("process.env.NODE_ENV") && read("node.mjs").contains("node:module"), "{}", read("node.mjs"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_page_gets_an_empty_module_for_a_node_builtin_and_a_program_for_node_keeps_the_import() {
    let dir = std::env::temp_dir().join(format!("rkp-empty-builtin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><script type=\"module\" src=\"/main.js\"></script>").unwrap();
    // A library that reaches for Node's `crypto` when it is there, and an import of `node:fs` that a page never calls.
    std::fs::write(dir.join("main.js"), "import fs from 'node:fs';\nimport { resolve } from 'path';\nimport { EventEmitter } from 'events';\nimport lib from './lib.cjs';\nconsole.log('LIB', lib, typeof fs, typeof resolve, typeof EventEmitter);\n").unwrap();
    std::fs::write(dir.join("lib.cjs"), "let c; try { c = require('crypto'); } catch (e) {}\nmodule.exports = typeof c;\n").unwrap();
    let build = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(&dir).output().expect("rkp starts");
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    build(&["bundle", "index.html", "--outdir", "page"]);
    let page = std::fs::read_dir(dir.join("page")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "js")).expect("a script was written");
    let script = std::fs::read_to_string(page).unwrap();
    assert!(!script.contains("\"crypto\"") && !script.contains("\"node:fs\"") && !script.contains("from \"fs\""), "a page cannot load a bare Node module: {script}");
    build(&["bundle", "main.js", "--outfile", "node.mjs"]);
    let node = std::fs::read_to_string(dir.join("node.mjs")).unwrap();
    assert!(node.contains("node:fs") && node.contains("require('crypto')"), "{node}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_page_follows_the_browser_field_of_a_package_and_a_program_for_node_does_not() {
    let dir = std::env::temp_dir().join(format!("rkp-browser-field-{}", std::process::id()));
    let pkg = dir.join("node_modules").join("lib");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><script type=\"module\" src=\"/main.js\"></script>").unwrap();
    std::fs::write(dir.join("main.js"), "import { which } from 'lib';\nimport cond from 'lib/cond';\nconsole.log(which, cond);\n").unwrap();
    // `./node.js` becomes `./web.js` and `fs` is nothing, as the field says; `exports` has a `browser` condition.
    std::fs::write(pkg.join("package.json"), r#"{"name":"lib","main":"index.js","browser":{"./node.js":"./web.js","fs":false},"exports":{".":"./index.js","./cond":{"browser":"./cond-web.js","default":"./cond-node.js"}}}"#).unwrap();
    std::fs::write(pkg.join("index.js"), "import node from './node.js';\nexport const which = node;\n").unwrap();
    std::fs::write(pkg.join("node.js"), "import util from 'util';\nutil.inherits(function A() {}, function B() {});\nexport default 'NODE_FILE';\n").unwrap();
    std::fs::write(pkg.join("web.js"), "export default 'WEB_FILE';\n").unwrap();
    std::fs::write(pkg.join("cond-web.js"), "export default 'COND_WEB';\n").unwrap();
    std::fs::write(pkg.join("cond-node.js"), "export default 'COND_NODE';\n").unwrap();
    let build = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(args).current_dir(&dir).output().expect("rkp starts");
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    build(&["bundle", "index.html", "--outdir", "page"]);
    let page = std::fs::read_dir(dir.join("page")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "js")).expect("a script was written");
    let script = std::fs::read_to_string(page).unwrap();
    assert!(script.contains("WEB_FILE") && !script.contains("NODE_FILE") && script.contains("COND_WEB") && !script.contains("COND_NODE"), "{script}");
    build(&["bundle", "main.js", "--outfile", "node.mjs"]);
    let node = std::fs::read_to_string(dir.join("node.mjs")).unwrap();
    assert!(node.contains("NODE_FILE") && node.contains("COND_NODE") && !node.contains("WEB_FILE"), "{node}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_namespace_member_declared_behind_files_of_reexports_is_kept_when_the_build_is_split() {
    let dir = std::env::temp_dir().join(format!("rkp-ns-split-{}", std::process::id()));
    let pkg = dir.join("node_modules").join("pkg").join("es").join("discrete").join("src");
    std::fs::create_dir_all(&pkg).unwrap();
    let root = dir.join("node_modules").join("pkg");
    std::fs::write(root.join("package.json"), r#"{"name":"pkg","version":"1.0.0","sideEffects":false,"module":"es/index.mjs","main":"es/index.mjs"}"#).unwrap();
    // `export *` through two files that hold nothing but re-exports, as naive-ui does.
    std::fs::write(root.join("es/index.mjs"), "export * from './components.mjs';\nexport { darkTheme } from './themes.mjs';\n").unwrap();
    std::fs::write(root.join("es/components.mjs"), "export * from './discrete/index.mjs';\n").unwrap();
    std::fs::write(root.join("es/discrete/index.mjs"), "export { createDiscreteApi } from './src/discrete.mjs';\n").unwrap();
    std::fs::write(root.join("es/discrete/src/discrete.mjs"), "export function createDiscreteApi(a) { return 'DISCRETE' + a; }\n").unwrap();
    std::fs::write(root.join("es/themes.mjs"), "export const darkTheme = 'DARK';\n").unwrap();
    std::fs::write(dir.join("util.js"), "import * as N from 'pkg';\nexport function setup() { return [N.darkTheme, N.createDiscreteApi(1)]; }\n").unwrap();
    // `darkTheme` is also wanted by a lazy chunk, so the build is split.
    std::fs::write(dir.join("main.js"), "import { darkTheme } from 'pkg';\nimport { setup } from './util.js';\nconsole.log(darkTheme, setup());\nimport('./lazy.js');\n").unwrap();
    std::fs::write(dir.join("lazy.js"), "import { darkTheme } from 'pkg';\nconsole.log('lazy', darkTheme);\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(["bundle", "main.js", "--outdir", "out", "--splitting", "--format", "esm"]).current_dir(&dir).output().expect("rkp starts");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let all: String = std::fs::read_dir(dir.join("out")).unwrap().filter_map(|e| e.ok()).map(|e| std::fs::read_to_string(e.path()).unwrap()).collect();
    assert!(all.contains("function createDiscreteApi"), "the function is declared nowhere:\n{all}");
    let run = std::process::Command::new("node").arg("out/main.js").current_dir(&dir).output();
    if let Ok(run) = run {
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_name_that_becomes_a_parenthesized_expression_does_not_join_the_line_before_it() {
    let dir = std::env::temp_dir().join(format!("rkp-asi-{}", std::process::id()));
    let pkg = dir.join("node_modules").join("cj");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("package.json"), r#"{"name":"cj","main":"index.js"}"#).unwrap();
    std::fs::write(pkg.join("index.js"), "exports.run = function () { globalThis.n = (globalThis.n || 0) + 1 }; exports.val = 5\n").unwrap();
    // No semicolons: `run()` on the next line is read as the call `val(...)`'s continuation unless the bundle writes the `;` the author left out.
    std::fs::write(dir.join("main.js"), "import { run, val } from 'cj'\nfunction f() {\n  const x = val\n  run()\n  const y = [1]\n  run()\n  return x\n}\nconst z = f()\nrun()\nif (z)\n  run()\nconsole.log(z, globalThis.n)\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rkp")).args(["bundle", "main.js", "--outfile", "o.mjs"]).current_dir(&dir).output().expect("rkp starts");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    if let Ok(run) = std::process::Command::new("node").arg("o.mjs").current_dir(&dir).output() {
        assert!(run.status.success() && String::from_utf8_lossy(&run.stdout).trim() == "5 4", "{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    }
    let text = std::fs::read_to_string(dir.join("o.mjs")).unwrap();
    assert!(text.contains(";(0, require_cj().run)()"), "{text}");
    // The body of an `if` is not a statement after another one: a `;` there would end the `if`.
    assert!(text.contains("if (z)\n  (0, require_cj().run)()"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}
