//! A property of an object that nothing writes is the value it was given: `config.debug` is `false` where no code in the bundle assigns to
//! `config`, passes it on, spreads it or exports it. Each program is run in Node as written and as bundled and minified and must print the
//! same lines; `GONE` strings must not be in the output, `KEPT` strings must be.

use std::process::Command;

struct Case {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    gone: &'static [&'static str],
    kept: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "a flag that is false across modules drops the code it guards, and what only that code uses",
        files: &[
            ("config.mjs", "export const config = { debug: false, quiet: null, handler: undefined, level: 3 };\n"),
            ("trace.mjs", "import { config } from './config.mjs';\nconst tags = ['GONE_tag'];\nexport function trace(x) { if (config.debug) { return tags.concat(helper(x)); } return x; }\nfunction helper(x) { return ['GONE_helper', x]; }\n"),
            ("main.mjs", "import { config } from './config.mjs';\nimport { trace } from './trace.mjs';\nconsole.log(trace(1), config.level, config.quiet ?? 'KEPT_default', config.handler === undefined);\n"),
        ],
        gone: &["GONE_tag", "GONE_helper"],
        kept: &["KEPT_default"],
    },
    Case {
        name: "an object whose every read was folded is not kept",
        files: &[
            ("config.mjs", "export const config = { onStop: null, tag: 'GONE_cfg', verbose: false };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nfunction stop(f) { const h = config.onStop; if (config.verbose) { console.log('GONE_verbose'); } return h ? h(f) : f; }\nconsole.log(stop(4));\n"),
        ],
        gone: &["GONE_cfg", "GONE_verbose"],
        kept: &[],
    },
    Case {
        name: "a var in the branch that never runs goes with it, one the rest of the function reads does not",
        files: &[
            ("config.mjs", "export const config = { on: false };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nlet shared = null;\nfunction run(cb) { if (!config.on) { cb(); } else { var root = !shared; if (root) { shared = { n: 1 }; } cb(); if (root) { shared = null; } } return typeof root; }\nconsole.log(run(() => console.log('cb')), shared);\n"),
        ],
        gone: &[],
        kept: &[],
    },
    Case {
        name: "an object another module writes to is left alone",
        files: &[
            ("config.mjs", "export const config = { debug: false };\n"),
            ("turn-on.mjs", "import { config } from './config.mjs';\nexport function turnOn() { config.debug = true; }\n"),
            ("main.mjs", "import { config } from './config.mjs';\nimport { turnOn } from './turn-on.mjs';\nconsole.log(config.debug ? 'KEPT_a' : 'KEPT_b');\nturnOn();\nconsole.log(config.debug ? 'KEPT_c' : 'KEPT_d');\n"),
        ],
        gone: &[],
        kept: &["KEPT_a", "KEPT_c"],
    },
    Case {
        name: "an object that is passed on, spread, or read by a computed key is left alone",
        files: &[
            ("config.mjs", "export const config = { debug: false, other: false };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nconst copy = { ...config };\nconst key = 'debug';\nObject.assign(config, { debug: true });\nconsole.log(config.debug ? 'KEPT_on' : 'KEPT_off', copy.debug, config[key], Object.keys(config).length);\n"),
        ],
        gone: &[],
        kept: &["KEPT_on", "KEPT_off"],
    },
    Case {
        name: "an update, a delete and a destructuring assignment write the property",
        files: &[
            ("config.mjs", "export const config = { a: false, b: false, c: false };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nconfig.a++;\ndelete config.b;\n[config.c] = [true];\nconsole.log(config.a ? 'KEPT_a' : 'KEPT_na', config.b ? 'KEPT_b' : 'KEPT_nb', config.c ? 'KEPT_c' : 'KEPT_nc');\n"),
        ],
        gone: &[],
        kept: &["KEPT_a", "KEPT_na", "KEPT_b", "KEPT_nb", "KEPT_c", "KEPT_nc"],
    },
    Case {
        name: "an object with a getter, or one that an entry exports, is left alone",
        files: &[
            ("config.mjs", "export const config = { get debug() { return 'KEPT_getter'; } };\nexport const settings = { quiet: false };\n"),
            ("main.mjs", "import { config, settings } from './config.mjs';\nconsole.log(config.debug, settings.quiet ? 'KEPT_q' : 'KEPT_nq');\nexport { settings };\n"),
        ],
        gone: &[],
        kept: &["KEPT_getter", "KEPT_q", "KEPT_nq"],
    },
    Case {
        name: "a name that is called as a function, and a property that is a function, still work",
        files: &[
            ("config.mjs", "export const config = { debug: false, log: (m) => 'KEPT_log:' + m };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nconsole.log(config.log('x'), config.debug);\n"),
        ],
        gone: &[],
        kept: &["KEPT_log"],
    },
    Case {
        name: "a property read on the folded value behaves as it did",
        files: &[
            ("config.mjs", "export const config = { n: null, u: undefined, b: false };\n"),
            ("main.mjs", "import { config } from './config.mjs';\nfor (const get of [() => config.n.x, () => config.u.x, () => config.b.x]) { try { console.log(get()); } catch (e) { console.log(e.constructor.name); } }\nconsole.log(typeof config.u, config.u?.x, config.n?.x);\n"),
        ],
        gone: &[],
        kept: &[],
    },
];

fn run(dir: &std::path::Path, file: &str) -> String {
    let out = Command::new("node").current_dir(dir).arg(file).output().unwrap();
    assert!(out.status.success(), "{file}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn properties_of_objects_nothing_writes_are_their_values() {
    let root = std::env::temp_dir().join(format!("rkp-frozen-{}", std::process::id()));
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
