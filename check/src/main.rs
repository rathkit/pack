use std::process::ExitCode;

use rathkit_check::{CODES, Diagnostic, FileKind, NOT_A_TYPE_CHECK, Report, TRACKED_TYPESCRIPT, check_source};

const USAGE: &str = "usage: rkp-check [--json] [--unverified] FILE.ts...\n       rkp-check --codes | --version";

/// Line and column (both 1-based, the column in UTF-16 code units, as TypeScript prints them) of a UTF-16 offset.
fn line_col(text: &str, offset: u32) -> (u32, u32) {
    let (mut line, mut col, mut at) = (1, 1, 0u32);
    let mut chars = text.chars().peekable();
    while at < offset {
        let Some(c) = chars.next() else { break };
        let units = c.len_utf16() as u32;
        at += units;
        match c {
            '\n' | '\u{2028}' | '\u{2029}' => (line, col) = (line + 1, 1),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    at += 1;
                }
                (line, col) = (line + 1, 1);
            }
            _ => col += units,
        }
    }
    (line, col)
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_line(file: &str, report: &Report) -> String {
    let diags: Vec<String> = report
        .diagnostics
        .iter()
        .map(|d: &Diagnostic| format!("{{\"code\":{},\"start\":{},\"length\":{},\"verified\":{}}}", d.code, d.start, d.length, d.verified))
        .collect();
    let stopped = match &report.stopped {
        Some(s) => format!("{{\"at\":{},\"reason\":\"{}\"}}", s.at, json_escape(s.reason)),
        None => "null".to_string(),
    };
    format!("{{\"file\":\"{}\",\"diagnostics\":[{}],\"stopped\":{}}}", json_escape(file), diags.join(","), stopped)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rkp-check {} (stage 1: scanner diagnostics; follows TypeScript {TRACKED_TYPESCRIPT})", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--codes") {
        for (code, text) in CODES {
            println!("TS{code}: {text}");
        }
        return ExitCode::SUCCESS;
    }
    let json = args.iter().any(|a| a == "--json");
    let unverified = args.iter().any(|a| a == "--unverified");
    let files: Vec<&String> = args.iter().filter(|a| *a != "--json" && *a != "--unverified").collect();
    if files.is_empty() || files.iter().any(|f| f.starts_with('-') && f.len() > 1) {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let (mut checked, mut skipped, mut stopped, mut errors, mut withheld, mut unreadable) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    for file in files {
        let Some(kind) = FileKind::from_path(file) else {
            skipped += 1;
            if !json {
                println!("{file}: not checked (stage 1 scans .ts, .mts and .cts; not JavaScript or JSX)");
            }
            continue;
        };
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{file}: cannot read: {e}");
                unreadable += 1;
                continue;
            }
        };
        let report = check_source(&text, kind);
        checked += 1;
        // The JSON form carries every diagnostic with its `verified` flag; the text form prints the verified ones, and the rest on request.
        let shown = |d: &&Diagnostic| json || unverified || d.verified;
        errors += report.diagnostics.iter().filter(shown).count();
        withheld += report.diagnostics.iter().filter(|d| !shown(d)).count();
        stopped += usize::from(report.stopped.is_some());
        if json {
            println!("{}", json_line(file, &report));
            continue;
        }
        for d in report.diagnostics.iter().filter(shown) {
            let (line, col) = line_col(&text, d.start);
            println!("{file}({line},{col}): error TS{}: {}{}", d.code, d.message, if d.verified { "" } else { " (unverified)" });
        }
        if let Some(s) = &report.stopped {
            let (line, col) = line_col(&text, s.at);
            println!("{file}({line},{col}): note: stage 1 stopped scanning here, so the rest of this file was not checked: {}", s.reason);
        }
    }
    if !json {
        println!(
            "rkp-check stage 1: scanned {checked} file(s), {errors} diagnostic(s), {withheld} withheld as unverified (see --unverified), {stopped} file(s) stopped early, {skipped} not checked.\n{NOT_A_TYPE_CHECK}"
        );
    }
    if unreadable > 0 {
        ExitCode::from(2)
    } else if errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
