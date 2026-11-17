//! esbuild's spellings of command line options, rewritten into the ones `rkp bundle` reads: `--outfile=FILE` for `--outfile FILE`,
//! `--external:NAME`, `--alias:NAME=PATH`, `--conditions=a,b`, `--main-fields=a,b`, `--platform=browser`, `--jsx=automatic`,
//! `--tree-shaking=false`, `--sources-content=false`, and the options that name a setting this program has no switch for (`--bundle`,
//! `--log-level=`, `--color=`, `--log-limit=`, `--charset=utf8`), which are accepted. See docs/esbuild-conformance.md for what each does.

/// Options that take their value as the next word, and that esbuild writes as `--name=value`.
const VALUE: &[&str] = &[
    "outfile", "outdir", "tsconfig", "config", "mode", "port", "host", "env-dir", "env-prefix", "asset-names", "public-path", "global-name", "jsx-import-source", "jsx-factory", "jsx-fragment", "federation", "cors-origin", "global",
];
/// Options that take the next word as their value whatever it looks like, so it is left alone.
const TAKES_NEXT: &[&str] = &[
    "outfile", "outdir", "tsconfig", "config", "mode", "port", "host", "env-dir", "env-prefix", "asset-names", "public-path", "global-name", "jsx-import-source", "jsx-factory", "jsx-fragment", "federation", "cors-origin", "global", "external", "condition", "main-field", "alias",
    "jsx", "banner", "footer", "define", "loader", "inject", "pure", "drop", "out-extension", "target", "format", "tsconfig-raw", "metafile", "packages", "resolve-extensions", "legal-comments", "entry-names", "chunk-names", "browserslist", "exports",
];

/// The arguments with esbuild's spellings turned into this program's, or what is wrong with one of them.
pub fn esbuild_spellings(args: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            out.push(arg.clone());
            continue;
        }
        let Some(flag) = arg.strip_prefix("--") else {
            out.push(arg.clone());
            continue;
        };
        let (name, value) = match flag.find(['=', ':']) {
            Some(i) => (&flag[..i], Some((flag.as_bytes()[i] as char, &flag[i + 1..]))),
            None => (flag, None),
        };
        if value.is_none() && TAKES_NEXT.contains(&name) {
            skip_next = true;
        }
        let Some((sep, value)) = value else {
            match name {
                "bundle" => {}
                _ => out.push(arg.clone()),
            }
            continue;
        };
        match (name, sep) {
            ("bundle", '=') => {
                if value != "true" {
                    return Err("--bundle=false: this program always bundles".to_string());
                }
            }
            (n, '=') if VALUE.contains(&n) => out.extend([format!("--{n}"), value.to_string()]),
            ("external", _) => out.extend(["--external".to_string(), value.to_string()]),
            ("alias", ':') => out.extend(["--alias".to_string(), value.to_string()]),
            ("conditions", '=') => value.split(',').filter(|v| !v.is_empty()).for_each(|v| out.extend(["--condition".to_string(), v.to_string()])),
            ("main-fields", '=') => value.split(',').filter(|v| !v.is_empty()).for_each(|v| out.extend(["--main-field".to_string(), v.to_string()])),
            ("platform", '=') => match value {
                "browser" | "neutral" => out.push("--platform-browser".to_string()),
                "node" => {}
                other => return Err(format!("--platform is browser, node or neutral, not '{other}'")),
            },
            ("jsx", '=') => match value {
                "automatic" | "classic" => out.extend(["--jsx".to_string(), value.to_string()]),
                "transform" => out.extend(["--jsx".to_string(), "classic".to_string()]),
                other => return Err(format!("--jsx is automatic, transform or classic, not '{other}' (preserve is not supported)")),
            },
            ("tree-shaking", '=') => match value {
                "true" => {}
                "false" => out.push("--no-treeshake".to_string()),
                other => return Err(format!("--tree-shaking is true or false, not '{other}'")),
            },
            ("sources-content", '=') => match value {
                "true" => {}
                "false" => out.push("--no-sources-content".to_string()),
                other => return Err(format!("--sources-content is true or false, not '{other}'")),
            },
            ("keep-names", '=') => out.push(if value == "false" { "--no-keep-names" } else { "--keep-names" }.to_string()),
            // The output keeps non-ASCII text as it is (esbuild's `utf8`); escaping it (`ascii`, esbuild's default) is not done.
            ("charset", '=') => match value {
                "utf8" => {}
                other => return Err(format!("--charset={other}: the output always keeps non-ASCII characters as they are (--charset=utf8)")),
            },
            // Settings of how messages are shown: nothing in the output depends on them.
            ("log-level" | "color" | "log-limit", '=') => {}
            _ => out.push(arg.clone()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::esbuild_spellings;

    fn run(args: &[&str]) -> Result<Vec<String>, String> {
        esbuild_spellings(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn esbuild_spellings_become_ours() {
        assert_eq!(run(&["a.js", "--bundle", "--outfile=o.js", "--external:react", "--alias:x=./y", "--conditions=a,b", "--main-fields=module,main"]).unwrap(), ["a.js", "--outfile", "o.js", "--external", "react", "--alias", "x=./y", "--condition", "a", "--condition", "b", "--main-field", "module", "--main-field", "main"]);
        assert_eq!(run(&["--platform=browser", "--jsx=transform", "--tree-shaking=false", "--sources-content=false", "--log-level=silent"]).unwrap(), ["--platform-browser", "--jsx", "classic", "--no-treeshake", "--no-sources-content"]);
    }

    #[test]
    fn our_spellings_and_values_are_left_alone() {
        assert_eq!(run(&["--outfile", "--outdir=x", "--banner", "--outfile=y", "--define:A=1"]).unwrap(), ["--outfile", "--outdir=x", "--banner", "--outfile=y", "--define:A=1"]);
        assert_eq!(run(&["--banner:js=--bundle", "--format=cjs"]).unwrap(), ["--banner:js=--bundle", "--format=cjs"]);
    }

    #[test]
    fn a_setting_it_cannot_honor_is_an_error() {
        assert!(run(&["--jsx=preserve"]).is_err());
        assert!(run(&["--platform=deno"]).is_err());
        assert!(run(&["--bundle=false"]).is_err());
        assert!(run(&["--charset=ascii"]).is_err());
        assert!(run(&["--charset=utf8"]).unwrap().is_empty());
    }
}
