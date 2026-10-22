use super::*;

fn resolver(fs: MemFs) -> Resolver {
    Resolver::with_fs(Box::new(fs), ResolveOptions::default())
}

fn resolve(r: &Resolver, from: &str, spec: &str) -> Result<String, ResolveError> {
    r.resolve(Path::new(from), spec, ResolveKind::Import).map(|r| r.path.to_string_lossy().into_owned())
}

#[test]
fn relative_files_and_extensions() {
    let r = resolver(MemFs::new().with("/p/a.js", "").with("/p/b/index.js", "").with("/p/c.json", "{}").with("/p/d.x.js", ""));
    assert_eq!(resolve(&r, "/p", "./a").unwrap(), "/p/a.js");
    assert_eq!(resolve(&r, "/p", "./a.js").unwrap(), "/p/a.js");
    assert_eq!(resolve(&r, "/p", "./b").unwrap(), "/p/b/index.js");
    assert_eq!(resolve(&r, "/p", "./b/").unwrap(), "/p/b/index.js");
    assert_eq!(resolve(&r, "/p/b", "..").unwrap_err(), ResolveError::NotFound { specifier: "..".into(), from: "/p/b".into() });
    assert_eq!(resolve(&r, "/p/b", "../c").unwrap(), "/p/c.json");
    assert_eq!(resolve(&r, "/p", "./d.x").unwrap(), "/p/d.x.js");
    assert_eq!(resolve(&r, "/p", "/p/a").unwrap(), "/p/a.js");
    assert!(matches!(resolve(&r, "/p", "./missing"), Err(ResolveError::NotFound { .. })));
}

#[test]
fn a_file_wins_over_a_directory_of_the_same_name() {
    let r = resolver(MemFs::new().with("/p/a.js", "").with("/p/a/index.js", ""));
    assert_eq!(resolve(&r, "/p", "./a").unwrap(), "/p/a.js");
    assert_eq!(resolve(&r, "/p", "./a/").unwrap(), "/p/a/index.js");
}

#[test]
fn directory_entry_from_main_fields() {
    let r = resolver(
        MemFs::new()
            .with("/p/lib/package.json", r#"{"main": "./cjs.js", "module": "esm"}"#)
            .with("/p/lib/cjs.js", "")
            .with("/p/lib/esm/index.js", ""),
    );
    assert_eq!(resolve(&r, "/p", "./lib").unwrap(), "/p/lib/esm/index.js", "module is tried before main");
    let r = resolver(MemFs::new().with("/p/lib/package.json", r#"{"main": "missing.js"}"#).with("/p/lib/index.js", ""));
    assert_eq!(resolve(&r, "/p", "./lib").unwrap(), "/p/lib/index.js", "an unusable main falls back to index");
}

#[test]
fn bare_specifiers_walk_up_node_modules() {
    let r = resolver(
        MemFs::new()
            .with("/p/node_modules/dep/package.json", r#"{"main": "main.js"}"#)
            .with("/p/node_modules/dep/main.js", "")
            .with("/p/node_modules/dep/sub/x.js", "")
            .with("/p/node_modules/@s/pkg/index.js", "")
            .with("/p/src/deep/node_modules/dep/index.js", ""),
    );
    assert_eq!(resolve(&r, "/p/src", "dep").unwrap(), "/p/node_modules/dep/main.js");
    assert_eq!(resolve(&r, "/p/src", "dep/sub/x").unwrap(), "/p/node_modules/dep/sub/x.js");
    assert_eq!(resolve(&r, "/p/src", "@s/pkg").unwrap(), "/p/node_modules/@s/pkg/index.js");
    assert_eq!(resolve(&r, "/p/src/deep", "dep").unwrap(), "/p/src/deep/node_modules/dep/index.js", "the nearest wins");
    assert!(matches!(resolve(&r, "/p/src", "nope"), Err(ResolveError::NotFound { .. })));
    assert!(matches!(resolve(&r, "/p/src", "@s"), Err(ResolveError::InvalidSpecifier { .. })));
}

fn exports_fs(exports: &str) -> Resolver {
    resolver(
        MemFs::new()
            .with("/p/node_modules/pkg/package.json", &format!(r#"{{"name": "pkg", "exports": {exports}}}"#))
            .with("/p/node_modules/pkg/index.js", "")
            .with("/p/node_modules/pkg/esm.mjs", "")
            .with("/p/node_modules/pkg/cjs.cjs", "")
            .with("/p/node_modules/pkg/lib/a.js", "")
            .with("/p/node_modules/pkg/lib/b.js", "")
            .with("/p/node_modules/pkg/internal/secret.js", ""),
    )
}

#[test]
fn exports_string_and_conditions() {
    let r = exports_fs(r#""./index.js""#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/index.js");
    assert!(matches!(resolve(&r, "/p", "pkg/lib/a"), Err(ResolveError::NotExported { .. })));

    let r = exports_fs(r#"{"import": "./esm.mjs", "require": "./cjs.cjs", "default": "./index.js"}"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/esm.mjs");
    let required = r.resolve(Path::new("/p"), "pkg", ResolveKind::Require).unwrap();
    assert_eq!(required.path, Path::new("/p/node_modules/pkg/cjs.cjs"));
}

#[test]
fn condition_order_is_the_file_order() {
    let r = exports_fs(r#"{"default": "./index.js", "import": "./esm.mjs"}"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/index.js", "default came first");
    let r = exports_fs(r#"{"module": "./esm.mjs", "default": "./index.js"}"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/esm.mjs", "module is a configured condition");
    let r = exports_fs(r#"{"browser": "./esm.mjs", "default": "./index.js"}"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/index.js", "browser is not configured");
}

#[test]
fn nested_conditions_and_arrays() {
    let r = exports_fs(r#"{"node": {"import": "./esm.mjs"}, "import": {"types": "./nope.d.ts", "default": "./index.js"}}"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/index.js", "an unmatched inner condition falls through");
    let r = exports_fs(r#"["./missing-condition-not-checked.js", "./index.js"]"#);
    assert_eq!(resolve(&r, "/p", "pkg").unwrap(), "/p/node_modules/pkg/missing-condition-not-checked.js", "arrays take the first valid target without checking it exists");
}

#[test]
fn subpaths_patterns_and_null() {
    let r = exports_fs(r#"{".": "./index.js", "./lib/*": "./lib/*.js", "./lib/b": null, "./internal/*": null, "./feature": {"import": "./esm.mjs"}}"#);
    assert_eq!(resolve(&r, "/p", "pkg/lib/a").unwrap(), "/p/node_modules/pkg/lib/a.js");
    assert_eq!(resolve(&r, "/p", "pkg/feature").unwrap(), "/p/node_modules/pkg/esm.mjs");
    assert!(matches!(resolve(&r, "/p", "pkg/lib/b"), Err(ResolveError::NotExported { .. })), "an exact null beats the pattern");
    assert!(matches!(resolve(&r, "/p", "pkg/internal/secret"), Err(ResolveError::NotExported { .. })));
    assert!(matches!(resolve(&r, "/p", "pkg/unlisted"), Err(ResolveError::NotExported { .. })));
}

#[test]
fn the_longest_pattern_prefix_wins() {
    let r = exports_fs(r#"{"./*": "./index.js", "./lib/*": "./lib/*.js"}"#);
    assert_eq!(resolve(&r, "/p", "pkg/lib/a").unwrap(), "/p/node_modules/pkg/lib/a.js");
    assert_eq!(resolve(&r, "/p", "pkg/other").unwrap(), "/p/node_modules/pkg/index.js");
}

#[test]
fn targets_cannot_escape_the_package() {
    let r = exports_fs(r#"{".": "../outside.js"}"#);
    assert!(matches!(resolve(&r, "/p", "pkg"), Err(ResolveError::InvalidTarget { .. })));
    let r = exports_fs(r#"{"./*": "./lib/*.js"}"#);
    assert!(matches!(resolve(&r, "/p", "pkg/../secret"), Err(ResolveError::InvalidTarget { .. }) | Err(ResolveError::NotExported { .. })));
    let r = exports_fs(r#"{".": "./node_modules/x.js"}"#);
    assert!(matches!(resolve(&r, "/p", "pkg"), Err(ResolveError::InvalidTarget { .. })));
}

#[test]
fn exports_hide_files_even_when_they_exist() {
    let r = exports_fs(r#"{".": "./index.js"}"#);
    assert!(matches!(resolve(&r, "/p", "pkg/internal/secret.js"), Err(ResolveError::NotExported { .. })));
}

#[test]
fn package_imports() {
    let r = resolver(
        MemFs::new()
            .with("/p/package.json", r##"{"name": "app", "imports": {"#util": "./src/util.js", "#dep": "dep", "#c/*": {"import": "./src/c/*.js"}}}"##)
            .with("/p/src/util.js", "")
            .with("/p/src/c/x.js", "")
            .with("/p/src/main.js", "")
            .with("/p/node_modules/dep/index.js", ""),
    );
    assert_eq!(resolve(&r, "/p/src", "#util").unwrap(), "/p/src/util.js");
    assert_eq!(resolve(&r, "/p/src", "#dep").unwrap(), "/p/node_modules/dep/index.js");
    assert_eq!(resolve(&r, "/p/src", "#c/x").unwrap(), "/p/src/c/x.js");
    assert!(matches!(resolve(&r, "/p/src", "#nope"), Err(ResolveError::ImportNotDefined { .. })));
}

#[test]
fn a_package_can_import_itself() {
    let r = resolver(
        MemFs::new()
            .with("/p/package.json", r#"{"name": "me", "exports": {".": "./index.js", "./x": "./x.js"}}"#)
            .with("/p/index.js", "")
            .with("/p/x.js", "")
            .with("/p/src/a.js", ""),
    );
    assert_eq!(resolve(&r, "/p/src", "me/x").unwrap(), "/p/x.js");
}

#[test]
fn package_metadata() {
    let r = resolver(
        MemFs::new()
            .with("/p/node_modules/lib/package.json", r#"{"name": "lib", "version": "1.2.3", "type": "module", "sideEffects": ["./polyfill.js", "*.css", "src/**/*.side.js"]}"#)
            .with("/p/node_modules/lib/index.js", "")
            .with("/p/node_modules/lib/polyfill.js", "")
            .with("/p/node_modules/lib/src/deep/a.side.js", ""),
    );
    let found = r.resolve(Path::new("/p"), "lib", ResolveKind::Import).unwrap();
    let package = found.package.unwrap();
    assert_eq!((package.name.as_deref(), package.version.as_deref(), package.is_module), (Some("lib"), Some("1.2.3"), true));
    assert!(!package.file_has_side_effects(Path::new("/p/node_modules/lib/index.js")));
    assert!(package.file_has_side_effects(Path::new("/p/node_modules/lib/polyfill.js")));
    assert!(package.file_has_side_effects(Path::new("/p/node_modules/lib/src/deep/a.side.js")));
    assert!(package.file_has_side_effects(Path::new("/p/node_modules/lib/styles/site.css")));
}

#[test]
fn side_effects_flag_values() {
    let none = Package::from_json(Path::new("/x"), Json::parse(r#"{"sideEffects": false}"#).unwrap());
    assert!(!none.file_has_side_effects(Path::new("/x/a.js")));
    let all = Package::from_json(Path::new("/x"), Json::parse(r#"{"sideEffects": true}"#).unwrap());
    assert!(all.file_has_side_effects(Path::new("/x/a.js")));
    let absent = Package::from_json(Path::new("/x"), Json::parse("{}").unwrap());
    assert!(absent.file_has_side_effects(Path::new("/x/a.js")));
}

#[test]
fn broken_package_json_is_reported() {
    let r = resolver(MemFs::new().with("/p/node_modules/bad/package.json", "{oops").with("/p/node_modules/bad/index.js", ""));
    assert!(matches!(resolve(&r, "/p", "bad"), Err(ResolveError::BadPackageJson { .. })));
}

#[test]
fn glob_matching() {
    for (pattern, path, expect) in [
        ("*.css", "a/b/c.css", true),
        ("*.css", "a/b/c.js", false),
        ("src/*.js", "src/a.js", true),
        ("src/*.js", "src/x/a.js", false),
        ("src/**/*.js", "src/x/y/a.js", true),
        ("src/**/*.js", "src/a.js", true),
        ("polyfill.js", "deep/polyfill.js", true),
        ("a/polyfill.js", "deep/a/polyfill.js", false),
        ("*.{css,less}", "x.less", true),
        ("*.{css,less}", "x.scss", false),
        ("fil?.js", "file.js", true),
    ] {
        assert_eq!(glob_matches(pattern, path), expect, "{pattern} vs {path}");
    }
}

#[test]
fn normalize_paths() {
    assert_eq!(normalize(Path::new("/a/b/../c/./d")), Path::new("/a/c/d"));
    assert_eq!(normalize(Path::new("/../a")), Path::new("/a"));
    assert_eq!(normalize(Path::new("../../a/../b")), Path::new("../../b"));
    assert_eq!(normalize(Path::new("a/..")), Path::new(""));
}

#[test]
fn a_package_json_that_does_not_parse_does_not_block_resolution() {
    let r = resolver(MemFs::new().with("/p/tmpl/package.json", "{ <%= name %> }").with("/p/tmpl/a.js", "").with("/p/node_modules/dep/index.js", ""));
    let found = r.resolve(Path::new("/p/tmpl"), "./a", ResolveKind::Import).unwrap();
    assert!(found.package.is_none());
    assert_eq!(r.resolve(Path::new("/p/tmpl"), "dep", ResolveKind::Import).unwrap().path, Path::new("/p/node_modules/dep/index.js"));
    assert!(matches!(r.package_for(Path::new("/p/tmpl/a.js")), Err(ResolveError::BadPackageJson { .. })), "the problem is still reported to those who ask");
}

#[test]
fn aliases_and_base_url() {
    let fs = MemFs::new()
        .with("/p/src/lib/util.ts", "")
        .with("/p/src/components/button/index.tsx", "")
        .with("/p/shared/a.js", "")
        .with("/p/node_modules/real/index.js", "")
        .with("/p/node_modules/other/index.js", "");
    let options = ResolveOptions {
        alias: vec![
            Alias { pattern: "@/*".into(), replacements: vec!["/p/src/*".into()], exact_only: true },
            Alias { pattern: "shared".into(), replacements: vec!["/p/shared".into()], exact_only: false },
            Alias { pattern: "fake".into(), replacements: vec!["real".into()], exact_only: true },
            Alias { pattern: "multi/*".into(), replacements: vec!["/p/missing/*".into(), "/p/src/lib/*".into()], exact_only: true },
        ],
        base_url: Some(PathBuf::from("/p/src")),
        ..ResolveOptions::default()
    };
    let r = Resolver::with_fs(Box::new(fs), options);
    assert_eq!(resolve(&r, "/p/x", "@/lib/util").unwrap(), "/p/src/lib/util.ts");
    assert_eq!(resolve(&r, "/p/x", "@/components/button").unwrap(), "/p/src/components/button/index.tsx");
    assert_eq!(resolve(&r, "/p/x", "shared/a").unwrap(), "/p/shared/a.js", "a plain alias also covers what is below it");
    assert_eq!(resolve(&r, "/p/x", "fake").unwrap(), "/p/node_modules/real/index.js", "an alias may name a package");
    assert_eq!(resolve(&r, "/p/x", "multi/util").unwrap(), "/p/src/lib/util.ts", "the next replacement is tried when one misses");
    assert_eq!(resolve(&r, "/p/x", "lib/util").unwrap(), "/p/src/lib/util.ts", "baseUrl");
    assert_eq!(resolve(&r, "/p/x", "other").unwrap(), "/p/node_modules/other/index.js", "packages still resolve");
    assert!(matches!(resolve(&r, "/p/x", "@/missing"), Err(ResolveError::NotFound { .. })));
}

#[test]
fn typescript_extension_aliases() {
    let r = resolver(MemFs::new().with("/p/a.ts", "").with("/p/b.tsx", "").with("/p/c.js", "").with("/p/c.ts", "").with("/p/d.mts", ""));
    assert_eq!(resolve(&r, "/p", "./a.js").unwrap(), "/p/a.ts", "a.js that does not exist is a.ts");
    assert_eq!(resolve(&r, "/p", "./b.js").unwrap(), "/p/b.tsx");
    assert_eq!(resolve(&r, "/p", "./c.js").unwrap(), "/p/c.js", "a file that exists wins");
    assert_eq!(resolve(&r, "/p", "./d.mjs").unwrap(), "/p/d.mts");
    assert_eq!(resolve(&r, "/p", "./a").unwrap(), "/p/a.ts");
}
