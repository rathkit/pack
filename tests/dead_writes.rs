//! A property written to an object that nothing that stays reads is not written (`vShow.getSSRProps = ...` where `vShow` is not used): the
//! object goes, and so does what only the written values use. Each program is run in Node as written and as bundled and minified and must
//! print the same lines; `GONE` strings must not be in the output, `KEPT` strings must be.

use std::process::Command;

struct Case {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    gone: &'static [&'static str],
    kept: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "a write in a function to an object nothing reads",
        files: &[("main.mjs", "const unused = { a: 'GONE_a' };\nfunction init() { unused.b = () => 'GONE_b'; }\ninit();\nconsole.log('ran');\n")],
        gone: &["GONE_a", "GONE_b"],
        kept: &["ran"],
    },
    Case {
        name: "a write at the top of the module to an object nothing reads",
        files: &[("main.mjs", "const unused = { a: 'GONE_a' };\nunused.b = () => 'GONE_b';\nunused['c'] = 'GONE_c';\nconsole.log('ran');\n")],
        gone: &["GONE_a", "GONE_b", "GONE_c"],
        kept: &["ran"],
    },
    Case {
        name: "the same object read somewhere that stays keeps the writes",
        files: &[("main.mjs", "const obj = { a: 'KEPT_a' };\nfunction init() { obj.b = () => 'KEPT_b'; }\ninit();\nobj.c = 'KEPT_c';\nconsole.log(obj.a, obj.b(), obj.c);\n")],
        gone: &[],
        kept: &["KEPT_a", "KEPT_b", "KEPT_c"],
    },
    Case {
        name: "a function and a class that carry properties",
        files: &[("main.mjs", "function Unused() {}\nUnused.displayName = 'GONE_display';\nclass Gone { m() { return 'GONE_class'; } }\nfunction setup() { Gone.tag = 'GONE_tag'; }\nsetup();\nfunction Used() { return 'KEPT_used'; }\nUsed.displayName = 'KEPT_display';\nconsole.log(Used(), Used.displayName);\n")],
        gone: &["GONE_display", "GONE_class", "GONE_tag"],
        kept: &["KEPT_used", "KEPT_display"],
    },
    Case {
        name: "objects that only write to each other are all gone, and one that is read brings in what its writes use",
        files: &[("main.mjs", "const d = { n: 'GONE_d' }, e = { n: 'GONE_e' }, c = { n: 'KEPT_c' }, b = { n: 'KEPT_b' };\nfunction init() { d.link = () => e.n; e.link = () => d.n; c.link = () => b.n; }\ninit();\nconsole.log(c.n, c.link());\n")],
        gone: &["GONE_d", "GONE_e"],
        kept: &["KEPT_c", "KEPT_b"],
    },
    Case {
        name: "a name that another name stands for is not fresh: the write can be seen through the other",
        files: &[("main.mjs", "const shared = { n: 0 };\nconst alias = shared;\nfunction init() { alias.seen = 'KEPT_seen'; }\ninit();\nconsole.log(shared.seen);\n")],
        gone: &[],
        kept: &["KEPT_seen"],
    },
    Case {
        name: "a write that could run a setter stays: a class with a static accessor, and an object with an accessor",
        files: &[("main.mjs", "const log = [];\nclass A { static set p(v) { log.push('KEPT_set'); } }\nconst o = { set q(v) { log.push('KEPT_q'); } };\nfunction init() { A.p = 1; o.q = 2; }\ninit();\nconsole.log(log.join());\n")],
        gone: &[],
        kept: &["KEPT_set", "KEPT_q"],
    },
    Case {
        name: "a value with an effect, and a compound assignment, stay",
        files: &[("main.mjs", "const log = [];\nconst o = { n: 1 };\nfunction init() { o.a = log.push('KEPT_a'); o.n += log.push('KEPT_n'); }\ninit();\nconsole.log(log.join());\n")],
        gone: &[],
        kept: &["KEPT_a", "KEPT_n"],
    },
    Case {
        name: "an object another module imports and reads keeps its writes",
        files: &[
            ("shared.mjs", "export const registry = { items: 'KEPT_items' };\nexport function register() { registry.extra = 'KEPT_extra'; }\nexport const unused = { x: 'GONE_x' };\nunused.y = 'GONE_y';\n"),
            ("main.mjs", "import { registry, register } from './shared.mjs';\nregister();\nconsole.log(registry.items, registry.extra);\n"),
        ],
        gone: &["GONE_x", "GONE_y"],
        kept: &["KEPT_items", "KEPT_extra"],
    },
    Case {
        name: "a write inside an if, a loop and a switch case",
        files: &[("main.mjs", "const o = { n: 'GONE_o' };\nfunction f(x) { if (x) o.a = 'GONE_a'; else o.b = 'GONE_b'; for (let i = 0; i < 2; i++) o.c = i; switch (x) { case 1: o.d = 'GONE_d'; break; default: o.e = 'GONE_e'; } return x; }\nconsole.log(f(1), f(0));\n")],
        gone: &["GONE_o", "GONE_a", "GONE_b", "GONE_d", "GONE_e"],
        kept: &[],
    },
];

fn run(dir: &std::path::Path, file: &str) -> String {
    let out = Command::new("node").current_dir(dir).arg(file).output().unwrap();
    assert!(out.status.success(), "{file}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn writes_to_objects_nothing_reads_are_left_out() {
    let root = std::env::temp_dir().join(format!("rkp-deadwrites-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (i, case) in CASES.iter().enumerate() {
        let dir = root.join(i.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in case.files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let pack = Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(&dir).args(["bundle", "main.mjs", "--minify", "--format=esm", "--outfile", "o.mjs"]).output().unwrap();
        assert!(pack.status.success(), "{}: {}", case.name, String::from_utf8_lossy(&pack.stderr));
        let (expected, got) = (run(&dir, "main.mjs"), run(&dir, "o.mjs"));
        assert_eq!(expected, got, "{}", case.name);
        let output = std::fs::read_to_string(dir.join("o.mjs")).unwrap();
        for g in case.gone {
            assert!(!output.contains(g), "{}: `{g}` is still in\n{output}", case.name);
        }
        for k in case.kept {
            assert!(output.contains(k), "{}: `{k}` is gone from\n{output}", case.name);
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}
