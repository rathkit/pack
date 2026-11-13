//! What the command line says. A clean run must never read as "no errors" or "type-correct".
use std::process::Command;

fn run(args: &[&str], file: Option<(&str, &str)>) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!("rkp-check-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut all: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    if let Some((name, text)) = file {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        all.push(p.display().to_string());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_rkp-check")).args(&all).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn a_clean_run_says_what_it_did_not_check() {
    let (code, out) = run(&[], Some(("clean.ts", "let a: number = 'x';\n")));
    assert_eq!(code, 0);
    let lower = out.to_lowercase();
    assert!(out.contains(rathkit_check::NOT_A_TYPE_CHECK), "{out}");
    assert!(lower.contains("types"), "{out}");
    for banned in ["no errors", "0 errors", "type-correct", "passed", "all good", "ok"] {
        assert!(!lower.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == banned) && !lower.contains(&format!("{banned}.")), "output reads as a pass ({banned}): {out}");
    }
}

#[test]
fn a_diagnostic_is_printed_like_tsc_with_line_and_column() {
    let (code, out) = run(&[], Some(("bad.ts", "let a = 1;\nlet b = 0x;\n")));
    assert_eq!(code, 1);
    assert!(out.contains("(2,11): error TS1125: Hexadecimal digit expected."), "{out}");
}

#[test]
fn unverified_diagnostics_are_withheld_unless_asked_for() {
    let text = "let a = x = /abc\n";
    let (code, out) = run(&[], Some(("u.ts", text)));
    assert_eq!(code, 0);
    assert!(out.contains("1 withheld as unverified"), "{out}");
    assert!(!out.contains("TS1161"));
    let (code, out) = run(&["--unverified"], Some(("u.ts", text)));
    assert_eq!(code, 1);
    assert!(out.contains("TS1161") && out.contains("(unverified)"), "{out}");
}

#[test]
fn files_it_does_not_scan_are_named_not_silently_passed() {
    let (code, out) = run(&[], Some(("c.tsx", "const a = <p>don't</p>;\n")));
    assert_eq!(code, 0);
    assert!(out.contains("not checked"), "{out}");
}

#[test]
fn version_names_the_typescript_it_follows() {
    let (_, out) = run(&["--version"], None);
    assert!(out.contains("TypeScript 5.9.3"), "{out}");
}
