//! The checker against TypeScript 5.9.3's own diagnostics, recorded in oracle-cases.tsv by tools/record-check-cases.mjs, so the test
//! needs no Node. `match` cases must agree exactly; `gap` cases must report nothing the compiler does not.
use rathkit_check::{FileKind, check_source};

fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('u') => {
                assert_eq!(it.next(), Some('{'));
                let hex: String = it.by_ref().take_while(|&d| d != '}').collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            other => panic!("bad escape {other:?}"),
        }
    }
    out
}

#[test]
fn agrees_with_typescript_on_the_recorded_cases() {
    let cases = include_str!("oracle-cases.tsv");
    let (mut matched, mut gaps) = (0, 0);
    for line in cases.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, text, expected, kind) = (f[0], decode(f[1]), f[2], f[3]);
        let report = check_source(&text, FileKind::Ts);
        let got: Vec<String> = report.diagnostics.iter().map(|d| format!("{}:{}:{}", d.code, d.start, d.length)).collect();
        let want: Vec<&str> = expected.split(' ').filter(|s| !s.is_empty()).collect();
        for g in &got {
            assert!(want.contains(&g.as_str()), "{name}: reported {g}, which TypeScript does not (it reports {want:?})");
        }
        if kind == "match" {
            assert_eq!(got, want, "{name}");
            matched += 1;
        } else {
            assert_eq!(kind, "gap", "{name}");
            gaps += 1;
        }
    }
    assert!(matched >= 50, "only {matched} matching cases");
    println!("{matched} cases match exactly, {gaps} are known gaps");
}

#[test]
fn nothing_is_reported_for_valid_code_even_with_regexes_and_templates() {
    let src = "const re = /ab+c/gi;\nconst n = a / b / c;\nconst t = `x${`y${1 + 2}`}`;\nconst o = { a: 1 }; const s = `${o.a}`;\nclass C { #p = 1; m() { return this.#p / 2; } }\n";
    let r = check_source(src, FileKind::Ts);
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
}

#[test]
fn a_slash_that_could_be_either_stops_the_scan_instead_of_guessing() {
    // After `)` a `/` is usually a division and sometimes starts a regular expression: TypeScript's parser decides.
    let r = check_source("if (x) /a/.test(y);\nlet s = \"unterminated\n", FileKind::Ts);
    assert!(r.stopped.is_some());
    assert!(r.diagnostics.is_empty());
}

#[test]
fn diagnostics_after_a_parser_dependent_decision_are_marked_unverified() {
    let r = check_source("let a = x = /abc\n", FileKind::Ts);
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, 1161);
    assert!(!r.diagnostics[0].verified);
    let r = check_source("let a = 0x;\nlet b = 1 / 2;\n", FileKind::Ts);
    assert!(r.diagnostics[0].verified);
}

#[test]
fn positions_count_utf16_code_units() {
    // U+1F600 is two UTF-16 units and four UTF-8 bytes.
    let r = check_source("let s = \"😀\"; let a = 0x;\n", FileKind::Ts);
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].start, 24);
}

#[test]
fn only_typescript_files_are_checked() {
    assert!(FileKind::from_path("a.ts").is_some());
    assert!(FileKind::from_path("a.d.mts").is_some());
    assert!(FileKind::from_path("a.CTS").is_some());
    assert!(FileKind::from_path("a.tsx").is_none());
    assert!(FileKind::from_path("a.js").is_none());
}
