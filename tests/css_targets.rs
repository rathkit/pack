//! The CSS shorthands that older browsers do not have (`inset`, `gap`, `margin-inline`, two-value `overflow`) are written only when the
//! target names browsers that all have them.

use std::process::Command;

const SHEET: &str = ".a{top:0;right:0;bottom:0;left:0;row-gap:1px;column-gap:1px;margin-inline-start:2px;margin-inline-end:2px;overflow-x:hidden;overflow-y:hidden}\n";

fn build(name: &str, target: Option<&str>) -> String {
    let dir = std::env::temp_dir().join(format!("rkp-csstarget-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.css"), SHEET).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rkp"));
    cmd.current_dir(&dir).args(["bundle", "a.css", "--minify", "--outfile", "out.css"]);
    if let Some(t) = target {
        cmd.arg(format!("--target={t}"));
    }
    let run = cmd.output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    std::fs::read_to_string(dir.join("out.css")).unwrap().trim().to_string()
}

#[test]
fn with_no_browsers_named_the_longhands_stay() {
    let written = SHEET.trim().to_string();
    for (name, target) in [("none", None), ("esnext", Some("esnext")), ("year", Some("es2020")), ("node", Some("node20")), ("ie", Some("ie11"))] {
        assert_eq!(build(name, target), written, "{name}");
    }
}

#[test]
fn browsers_that_all_have_them_get_the_shorthands() {
    assert_eq!(build("new", Some("chrome100,safari15,firefox100")), ".a{inset:0;gap:1px;margin-inline:2px;overflow:hidden}");
}

#[test]
fn one_older_browser_keeps_back_what_it_lacks() {
    // Safari 13.1 has the two-value overflow, and none of the other three.
    assert_eq!(build("old", Some("chrome100,safari13.1")), ".a{top:0;right:0;bottom:0;left:0;row-gap:1px;column-gap:1px;margin-inline-start:2px;margin-inline-end:2px;overflow:hidden}");
    // Safari 14 (not 14.1) is the same: 14.1 is where inset, the logical shorthands and flex gap came in.
    assert_eq!(build("safari14", Some("safari14")), build("old", Some("chrome100,safari13.1")));
    assert_eq!(build("safari141", Some("safari14.1")), ".a{inset:0;gap:1px;margin-inline:2px;overflow:hidden}");
}
