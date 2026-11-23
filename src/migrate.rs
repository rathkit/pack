//! `rkp migrate [DIR] [--write]`: which of esbuild, Vite, webpack and Rollup a project uses, and the `rkp` commands that do the same build.
//!
//! Configuration files are JavaScript, which is not run here: the literals the commands need (`outDir: 'build'`, `entry: './src/a.js'`,
//! `format: 'cjs'`) are read from the text of the file, and what is not a literal is left to `--config`, which reads the file for real
//! (docs/migrate/ says which parts of each config `rkp` reads itself). The commands are the ones of the guides in docs/migrate/.

use crate::json::Json;
use std::path::Path;

/// One tool found in the project and what replaces its commands.
#[derive(Debug, PartialEq)]
pub struct Finding {
    pub tool: &'static str,
    /// Where it was seen: the config file, a package.json script or a dependency.
    pub evidence: Vec<String>,
    /// `(script name, command)`: the script to add to package.json and the `rkp` command it runs.
    pub commands: Vec<(String, String)>,
    pub notes: Vec<String>,
}

const CONFIG_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "ts", "mts", "cts"];
/// esbuild options that `rkp bundle` stops with an error on (docs/esbuild-conformance.md: the rows that are gaps).
const ESBUILD_GAPS: &[&str] = &[
    "--outbase", "--supported:", "--mangle-props", "--reserve-props", "--mangle-quoted", "--mangle-cache", "--line-limit", "--charset=ascii", "--sourcemap=both", "--source-root",
    "--sourcefile", "--drop-labels", "--ignore-annotations", "--jsx=preserve", "--jsx-side-effects", "--analyze", "--abs-paths", "--log-override", "--servedir", "--serve=",
    "--serve-fallback", "--certfile", "--keyfile", "--stdin", "--watch=forever",
];
/// Folders the search for a config file does not enter.
const SEARCH_SKIP: &[&str] = &["node_modules", "dist", "out", "target", "coverage", "test", "tests", "__tests__", "fixtures", "examples", "example", "docs", "e2e", "vendor"];
/// Rollup plugins whose work `rkp` does itself, and the option that stands for it.
const ROLLUP_BUILT_IN: &[(&str, &str)] = &[
    ("@rollup/plugin-node-resolve", "resolving is built in"),
    ("@rollup/plugin-commonjs", "CommonJS is read as it is; the plugin does not run here"),
    ("@rollup/plugin-replace", "--define:NAME=VALUE"),
    ("@rollup/plugin-terser", "--minify"),
    ("@rollup/plugin-json", "JSON is read as it is"),
    ("@rollup/plugin-alias", "--alias:NAME=PATH"),
    ("@rollup/plugin-typescript", "TypeScript is read as it is"),
];

/// Finds the tools of the project in `dir`, in the order esbuild, Vite, webpack, Rollup, rspack.
pub fn detect(dir: &Path) -> Vec<Finding> {
    detect_with(dir, true)
}

/// `detect`, and `run_config` says whether a Rollup config whose values are not plain strings may be run to read them.
pub fn detect_with(dir: &Path, run_config: bool) -> Vec<Finding> {
    let package = std::fs::read_to_string(dir.join("package.json")).ok().and_then(|t| Json::parse(&t).ok());
    let mut found = Vec::new();
    found.extend(esbuild(dir, package.as_ref()));
    found.extend(vite(dir, package.as_ref()));
    found.extend(webpack(dir, package.as_ref()));
    found.extend(rollup(dir, package.as_ref(), run_config));
    found.extend(rspack(dir, package.as_ref()));
    found
}

/// A note for a config file that is not in the root: its relative paths are the ones the command runs with, from the root.
fn subfolder_note(finding: &mut Finding, config: Option<&String>) {
    if let Some(c) = config.filter(|c| c.contains('/')) {
        finding.notes.push(format!("the config is {c}, in a subfolder: the commands run from the project folder, so check that the paths in them (entry, output folder) are the ones you build with"));
    }
}

fn has_dependency(package: Option<&Json>, name: &str) -> bool {
    package.is_some_and(|p| ["dependencies", "devDependencies"].iter().any(|k| p.get(k).and_then(|d| d.get(name)).is_some()))
}

/// The config file of a tool as a path of the project (`vite.config.ts`, or `build/rollup.config.js`), looked for in this order: the root;
/// a path that a package.json script names (`rollup -c build/rollup.config.js`); a search of the subfolders, two levels deep, that skips
/// dependencies, build output, tests and examples (a config there is not the project's own build), the shallowest and then the first by name.
fn config_file(dir: &Path, stem: &str, package: Option<&Json>) -> Option<String> {
    let names: Vec<String> = CONFIG_EXTENSIONS.iter().map(|e| format!("{stem}.{e}")).collect();
    if let Some(found) = names.iter().find(|f| dir.join(f).is_file()) {
        return Some(found.clone());
    }
    for (_, script) in package.and_then(|p| p.get("scripts")).and_then(Json::as_object).unwrap_or(&[]) {
        for word in script.as_str().unwrap_or("").split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '=')) {
            let word = word.trim_start_matches("./");
            let inside = !word.is_empty() && !word.starts_with('/') && !word.split('/').any(|part| part == "..");
            if inside && names.iter().any(|n| word == n || word.ends_with(&format!("/{n}"))) && dir.join(word).is_file() {
                return Some(word.to_string());
            }
        }
    }
    let mut level = vec![String::new()];
    for _ in 0..2 {
        let mut next = Vec::new();
        for prefix in &level {
            let mut folders: Vec<String> = std::fs::read_dir(dir.join(prefix)).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| !n.starts_with('.') && !SEARCH_SKIP.contains(&n.as_str())).collect();
            folders.sort();
            for folder in folders {
                let path = if prefix.is_empty() { folder } else { format!("{prefix}/{folder}") };
                if let Some(found) = names.iter().map(|n| format!("{path}/{n}")).find(|f| dir.join(f).is_file()) {
                    return Some(found);
                }
                next.push(path);
            }
        }
        level = next;
    }
    None
}

/// The first string written after `key:` in the text (`'dist'`, `"dist"` or a template without `${`), where `key` is a whole word.
pub fn literal_after(text: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find(key) {
        let start = from + at;
        let end = start + key.len();
        from = end;
        let word_before = text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.');
        if word_before {
            continue;
        }
        let rest = text[end..].trim_start();
        let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else { continue };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) else { continue };
        let body = &rest[1..];
        let close = body.find(quote)?;
        let value = &body[..close];
        if value.contains("${") {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// Whether `key: true` is written in the text.
fn flag_after(text: &str, key: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(key) {
        let start = from + at;
        from = start + key.len();
        if text[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if let Some(rest) = text[from..].trim_start().strip_prefix(':') {
            if rest.trim_start().starts_with("true") {
                return true;
            }
        }
    }
    false
}

/// The module names a config file imports (`from 'x'`, `require('x')`).
fn imports_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["from '", "from \"", "require('", "require(\""] {
        let quote = marker.chars().last().unwrap();
        let mut from = 0;
        while let Some(at) = text[from..].find(marker) {
            let begin = from + at + marker.len();
            from = begin;
            if let Some(close) = text[begin..].find(quote) {
                out.push(text[begin..begin + close].to_string());
            }
        }
    }
    out
}

fn esbuild(dir: &Path, package: Option<&Json>) -> Option<Finding> {
    let mut finding = Finding { tool: "esbuild", evidence: Vec::new(), commands: Vec::new(), notes: Vec::new() };
    if has_dependency(package, "esbuild") {
        finding.evidence.push("dependency esbuild".to_string());
    }
    // package.json scripts that run the command line: `esbuild ...` at the start of a command.
    for (name, value) in package.and_then(|p| p.get("scripts")).and_then(Json::as_object).unwrap_or(&[]) {
        let Some(script) = value.as_str() else { continue };
        let (rewritten, changed) = rewrite_esbuild_commands(script);
        if !changed {
            continue;
        }
        finding.evidence.push(format!("script {name}"));
        finding.commands.push((format!("{name}:rkp"), rewritten));
        for gap in ESBUILD_GAPS.iter().filter(|g| script.contains(*g)) {
            finding.notes.push(format!("script {name}: {} is not supported by rkp (see docs/esbuild-conformance.md)", gap.trim_end_matches([':', '='])));
        }
        if !script.contains("--bundle") {
            finding.notes.push(format!("script {name}: esbuild without --bundle only transforms files; rkp always bundles"));
        }
    }
    // Files that call the JavaScript API: the scripts of the root and the files that a package.json script runs (`node scripts/build.mjs`).
    let mut files: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_file()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| CONFIG_EXTENSIONS.iter().any(|x| n.ends_with(&format!(".{x}")))).collect();
    for (_, script) in package.and_then(|p| p.get("scripts")).and_then(Json::as_object).unwrap_or(&[]) {
        for word in script.as_str().unwrap_or("").split_whitespace() {
            let word = word.trim_matches(['\'', '"']).trim_start_matches("./");
            let inside = !word.starts_with('/') && !word.split('/').any(|part| part == "..");
            if inside && CONFIG_EXTENSIONS.iter().any(|x| word.ends_with(&format!(".{x}"))) && dir.join(word).is_file() && !files.iter().any(|f| f == word) {
                files.push(word.to_string());
            }
        }
    }
    files.sort();
    for file in files {
        let Ok(text) = std::fs::read_to_string(dir.join(&file)) else { continue };
        if !imports_of(&text).iter().any(|m| m == "esbuild") {
            continue;
        }
        finding.evidence.push(format!("API in {file}"));
        finding.notes.extend(esbuild_api_notes(&file, &text));
    }
    (!finding.evidence.is_empty()).then_some(finding)
}

/// Whether the line has `what` (a call such as `context(` or an option such as `write: false`) as a word of its own: not the end of a longer name.
fn mentions(line: &str, what: &str) -> bool {
    line.match_indices(what).any(|(i, _)| !line[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$'))
}

/// What changes in a script that uses esbuild's JavaScript API, line by line: the import of `esbuild` (the one change `--write` cannot make
/// for you: the script is yours), and each call or option that `@rathkit/pack` does not have (docs/esbuild-conformance.md).
fn esbuild_api_notes(file: &str, text: &str) -> Vec<String> {
    // (what is written, why it does not carry over)
    const GAPS: &[(&str, &str)] = &[
        ("context(", "`context()` has no equivalent: there is no context object (`rebuild`, `watch`, `dispose`); use `serve(options)` for a server or `rkp bundle --watch`"),
        ("transform(", "`transform()` has no equivalent: there is no transform API; the build is `build(options)` on files"),
        ("transformSync(", "`transformSync()` has no equivalent: there is no transform API; the build is `build(options)` on files"),
        ("buildSync(", "`buildSync()` has no equivalent: there is no synchronous build; `await build(options)`, or run `rkp bundle`"),
        ("formatMessages(", "`formatMessages()` is not in the package (`rkp analyze` makes the HTML report of a metafile)"),
        ("analyzeMetafile(", "`analyzeMetafile()` is not in the package (`rkp analyze` makes the HTML report of a metafile)"),
        ("analyzeMetafileSync(", "`analyzeMetafileSync()` is not in the package (`rkp analyze` makes the HTML report of a metafile)"),
        ("write: false", "`write: false` and `outputFiles` are not done: the build always writes"),
        ("stdin:", "`stdin` is not done: the entry is a file"),
        ("mangleProps", "`mangleProps`: mangling properties is not done"),
        ("mangleCache", "`mangleCache`: mangling properties is not done"),
        ("bundle: false", "`bundle: false` is a TypeError: rkp always bundles"),
        ("outbase", "`outbase` is not done"),
        ("supported:", "`supported` is not done"),
        ("dropLabels", "`dropLabels` is not done"),
        ("metafile: true", "`metafile` is a file name, not `true`"),
    ];
    let mut notes = Vec::new();
    let mut imports = Vec::new();
    let mut calls: Vec<usize> = Vec::new();
    let mut plugins = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }
        if ["'esbuild'", "\"esbuild\""].iter().any(|q| trimmed.contains(&format!("from {q}")) || trimmed.contains(&format!("require({q})"))) {
            let changed = trimmed.replace("'esbuild'", "'@rathkit/pack'").replace("\"esbuild\"", "\"@rathkit/pack\"");
            imports.push(format!("{file}:{n}: `{trimmed}` becomes `{changed}`"));
        }
        // `esbuild.build(` and `build(`: the one call that is the same.
        if mentions(line, "build(") && !trimmed.starts_with("import") {
            calls.push(n);
        }
        for (what, why) in GAPS {
            if mentions(line, what) {
                notes.push(format!("{file}:{n}: {why} (docs/esbuild-conformance.md)"));
            }
        }
        if let Some(module) = imports_of(line).into_iter().find(|m| m.starts_with("esbuild-plugin") || m.starts_with("@esbuild-plugins/")) {
            plugins.push(format!("{file}:{n}: the plugin {module} stays in `plugins`: it runs through rkp's plugin API (the table in docs/esbuild-conformance.md says which hooks)"));
        }
    }
    let mut out: Vec<String> = imports;
    if !calls.is_empty() {
        out.push(format!("{file}: build() at line{} {} is the same call in @rathkit/pack (options: entryPoints as file names, bundle: true, outfile, outdir, format, platform, define, loader, alias, minify, sourcemap, ...)", if calls.len() > 1 { "s" } else { "" }, calls.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")));
    }
    out.extend(notes);
    out.extend(plugins);
    out
}

/// A script with each command that starts with `esbuild` made `rkp bundle`, and whether one was.
fn rewrite_esbuild_commands(script: &str) -> (String, bool) {
    let mut out = String::new();
    let mut changed = false;
    let mut command_start = true;
    let mut rest = script;
    while !rest.is_empty() {
        if command_start {
            let trimmed = rest.trim_start();
            out.push_str(&rest[..rest.len() - trimmed.len()]);
            rest = trimmed;
            if let Some(after) = rest.strip_prefix("esbuild").filter(|a| a.starts_with(' ') || a.is_empty()) {
                out.push_str("rkp bundle");
                rest = after;
                changed = true;
            }
            command_start = false;
            continue;
        }
        let separator = ["&&", "||", ";", "|"].iter().filter_map(|s| rest.find(s).map(|i| (i, s.len()))).min();
        match separator {
            Some((i, len)) => {
                out.push_str(&rest[..i + len]);
                rest = &rest[i + len..];
                command_start = true;
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    (out, changed)
}

fn vite(dir: &Path, package: Option<&Json>) -> Option<Finding> {
    let mut config = config_file(dir, "vite.config", package);
    // A vite.config that only configures Vitest (`from 'vitest/config'`) is a test setup, not a build.
    if config.as_ref().and_then(|c| std::fs::read_to_string(dir.join(c)).ok()).is_some_and(|t| imports_of(&t).iter().any(|m| m == "vitest/config") && !t.contains("build:") && !t.contains("plugins:")) {
        config = None;
    }
    let mut evidence: Vec<String> = config.iter().cloned().collect();
    // `vite` in the dependencies alone (a library that tests with Vitest) is a build only when there is a page to build.
    if has_dependency(package, "vite") && (config.is_some() || dir.join("index.html").is_file()) {
        evidence.push("dependency vite".to_string());
    }
    if evidence.is_empty() {
        return None;
    }
    let text = config.as_ref().and_then(|c| std::fs::read_to_string(dir.join(c)).ok()).unwrap_or_default();
    let mut finding = Finding { tool: "vite", evidence, commands: Vec::new(), notes: Vec::new() };
    subfolder_note(&mut finding, config.as_ref());
    let page = std::fs::read_to_string(dir.join("index.html")).ok();
    let mut build = "rkp bundle index.html".to_string();
    if let Some(c) = &config {
        build.push_str(&format!(" --config {c}"));
    }
    build.push_str(&format!(" --outdir {}", literal_after(&text, "outDir").unwrap_or_else(|| "dist".to_string())));
    build.push_str(" --minify");
    if flag_after(&text, "sourcemap") {
        build.push_str(" --sourcemap");
    }
    if let Some(base) = literal_after(&text, "base").filter(|b| b != "/") {
        build.push_str(&format!(" --public-path={base}"));
    } else if text.contains("base:") && literal_after(&text, "base").is_none() {
        finding.notes.push("base is not a plain string in the config: add --public-path=BASE with its value (Vite's default is /, rkp's links are relative without it)".to_string());
    }
    if page.is_none() {
        finding.notes.push("there is no index.html here: give the page (or the script) that Vite was given".to_string());
    }
    finding.commands.push(("build:rkp".to_string(), build));
    // `rkp serve` takes the script, and serves index.html with it pointed at the build.
    if let Some(script) = page.as_deref().and_then(first_module_script) {
        let mut serve = format!("rkp serve {script}");
        if let Some(c) = &config {
            serve.push_str(&format!(" --config {c}"));
        }
        serve.push_str(" --hmr");
        finding.commands.push(("dev:rkp".to_string(), serve));
    } else {
        finding.notes.push("no <script type=\"module\" src> in index.html: `rkp serve SCRIPT --hmr` needs the script".to_string());
    }
    if text.contains("lib:") {
        finding.notes.push("build.lib: a library is `rkp bundle ENTRY --format esm,cjs` (README: library mode), not a page".to_string());
    }
    if text.contains("ssr:") {
        finding.notes.push("build.ssr is not supported".to_string());
    }
    finding.notes.push("build.outDir, base, minify and sourcemap are not read from the config: they are on the command above (docs/migrate/vite.md)".to_string());
    Some(finding)
}

/// The `src` of the first `<script type="module" src="...">` of a page, as a path of the project.
fn first_module_script(html: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = html[from..].find("<script") {
        let start = from + at;
        let end = start + html[start..].find('>')?;
        from = end;
        let tag = &html[start..end];
        if !tag.contains("type=\"module\"") && !tag.contains("type='module'") {
            continue;
        }
        let src_at = tag.find("src=")? + 4;
        let quote = tag[src_at..].chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let value = &tag[src_at + 1..];
        let src = &value[..value.find(quote)?];
        return Some(src.trim_start_matches('/').trim_start_matches("./").to_string());
    }
    None
}

fn webpack(dir: &Path, package: Option<&Json>) -> Option<Finding> {
    let config = config_file(dir, "webpack.config", package);
    let mut evidence: Vec<String> = config.iter().cloned().collect();
    if has_dependency(package, "webpack") {
        evidence.push("dependency webpack".to_string());
    }
    if evidence.is_empty() {
        return None;
    }
    let text = config.as_ref().and_then(|c| std::fs::read_to_string(dir.join(c)).ok()).unwrap_or_default();
    let mut finding = Finding { tool: "webpack", evidence, commands: Vec::new(), notes: Vec::new() };
    subfolder_note(&mut finding, config.as_ref());
    // The entry: a plain string, webpack's default when the config names none, or the config's own (then it is read from the file).
    let entry = match literal_after(&text, "entry") {
        Some(entry) => Some(entry.trim_start_matches("./").to_string()),
        None if text.contains("entry") => {
            finding.notes.push("the entry is read from the config; name it on the command line when it is not a plain string".to_string());
            None
        }
        None => {
            finding.notes.push("the config names no entry: webpack's default src/index.js is used".to_string());
            Some("src/index.js".to_string())
        }
    };
    let mut build = "rkp bundle".to_string();
    if let Some(entry) = &entry {
        build.push_str(&format!(" {entry}"));
    }
    if let Some(c) = &config {
        build.push_str(&format!(" --config {c}"));
    }
    if literal_after(&text, "target").is_none_or(|t| t.starts_with("web")) {
        build.push_str(" --platform=browser");
    }
    if literal_after(&text, "mode").is_none_or(|m| m == "production") {
        build.push_str(" --minify");
    }
    if literal_after(&text, "devtool").is_some_and(|d| d.contains("source-map")) {
        build.push_str(if literal_after(&text, "devtool").is_some_and(|d| d.contains("inline")) { " --sourcemap=inline" } else { " --sourcemap" });
    }
    // output: { path: path.resolve(__dirname, 'dist'), filename: 'main.js' }
    let output = text.find("output").map(|i| &text[i..text[i..].find('}').map_or(text.len(), |e| i + e)]).unwrap_or("");
    let folder = output.find("__dirname,").and_then(|i| literal_after(&format!("x:{}", &output[i + "__dirname,".len()..]), "x")).or_else(|| literal_after(output, "path")).unwrap_or_else(|| "dist".to_string());
    let filename = literal_after(output, "filename").unwrap_or_else(|| "main.js".to_string());
    if filename.contains('[') {
        finding.notes.push(format!("output.filename {filename}: a name with a hash is --entry-names=[name]-[hash] (with --outdir and --splitting)"));
        build.push_str(&format!(" --outfile {}/main.js", folder.trim_end_matches('/')));
    } else {
        build.push_str(&format!(" --outfile {}/{filename}", folder.trim_end_matches('/')));
    }
    finding.commands.push(("build:rkp".to_string(), build));
    let mut serve = "rkp serve".to_string();
    if let Some(entry) = &entry {
        serve.push_str(&format!(" {entry}"));
    }
    if let Some(c) = &config {
        serve.push_str(&format!(" --config {c}"));
    }
    serve.push_str(" --hmr");
    finding.commands.push(("dev:rkp".to_string(), serve));
    if text.contains("splitChunks") {
        finding.notes.push("optimization.splitChunks.cacheGroups shape the chunks with --splitting (test, name, chunks, priority, minChunks, minSize, enforce); size splitting (maxSize) and runtimeChunk do not apply, and webpack's implicit groups are not added".to_string());
    }
    finding.notes.push("mode, output and devtool are not read from the config: they are on the command above (docs/migrate/webpack.md)".to_string());
    Some(finding)
}

fn rollup(dir: &Path, package: Option<&Json>, run: bool) -> Option<Finding> {
    let config = config_file(dir, "rollup.config", package);
    let mut evidence: Vec<String> = config.iter().cloned().collect();
    if has_dependency(package, "rollup") {
        evidence.push("dependency rollup".to_string());
    }
    if evidence.is_empty() {
        return None;
    }
    let text = config.as_ref().and_then(|c| std::fs::read_to_string(dir.join(c)).ok()).unwrap_or_default();
    let mut finding = Finding { tool: "rollup", evidence, commands: Vec::new(), notes: Vec::new() };
    subfolder_note(&mut finding, config.as_ref());
    let mut build = "rkp bundle".to_string();
    let mut no_input = false;
    // What the text of the config does not give (an input or an output built in code) is read by running it, when that is safe.
    let rolled = if run { config.as_deref().filter(|c| is_script(c)).filter(|_| !text_is_complete(&text)).and_then(|c| run_rollup_config(dir, c)) } else { None };
    if rolled.is_some() {
        no_input = true;
    } else {
        let mut input_name = None;
        match literal_after(&text, "input") {
            Some(input) => {
                input_name = Some(input.trim_start_matches("./").to_string());
                build.push_str(&format!(" {}", input.trim_start_matches("./")));
            }
            None => {
                finding.notes.push("the config's input is not a plain string (or there is no config): no command is written; run `rkp bundle ENTRY... --outfile FILE` with the entries of the config".to_string());
                no_input = true;
            }
        }
        // The settings of `output: { ... }`, and not the `name` or `format` of a plugin written beside it.
        let output = text.find("output").map(|i| &text[i..text[i..].find('}').map_or(text.len(), |e| i + e)]).unwrap_or("");
        match (literal_after(output, "dir"), literal_after(output, "file")) {
            (_, Some(file)) => build.push_str(&format!(" --outfile {file}")),
            (Some(dir), None) => build.push_str(&format!(" --outdir {dir} --splitting")),
            _ => {
                // A config that builds the file name or the output list in code: without an output `rkp bundle` prints the bundle and writes nothing.
                let stem = input_name.as_deref().and_then(|i| i.rsplit('/').next()).map(|n| n.rsplit_once('.').map_or(n, |(a, _)| a).to_string()).unwrap_or_else(|| "index".to_string());
                build.push_str(&format!(" --outfile dist/{stem}.js"));
                finding.notes.push("the output file is not a plain string in the config: --outfile dist/... is a stand-in, set the file you publish".to_string());
            }
        }
        match literal_after(output, "format").as_deref() {
            Some("cjs" | "commonjs") => build.push_str(" --format=cjs"),
            Some("iife") => build.push_str(" --format=iife"),
            Some("es" | "esm" | "module") | None => {}
            Some(other) => finding.notes.push(format!("output.format '{other}' is not supported (esm, cjs and iife are); iife with --global-name is the form for a page")),
        }
        if let Some(name) = literal_after(output, "name") {
            build.push_str(&format!(" --global-name={name}"));
        }
        if literal_after(output, "sourcemap").is_some_and(|s| s == "inline") {
            build.push_str(" --sourcemap=inline");
        } else if flag_after(output, "sourcemap") {
            build.push_str(" --sourcemap");
        }
    }
    let modules = imports_of(&text);
    if modules.iter().any(|m| m == "@rollup/plugin-terser" || m == "rollup-plugin-terser") {
        build.push_str(" --minify");
    }
    let mut kept = Vec::new();
    for module in modules.iter().filter(|m| m.starts_with("@rollup/plugin-") || m.starts_with("rollup-plugin-") || m.contains("rollup-plugin")) {
        match ROLLUP_BUILT_IN.iter().find(|(n, _)| n == module) {
            Some((_, what)) => finding.notes.push(format!("{module}: not needed ({what})")),
            None => kept.push(module.clone()),
        }
    }
    if !kept.is_empty() {
        finding.notes.push(format!("plugins to keep, in rathkit.config.mjs as `export default {{ plugins: [...] }}` (found by name): {}", kept.join(", ")));
    }
    if text.contains("plugins") && kept.is_empty() {
        finding.notes.push("plugins written in the config stay: put them in rathkit.config.mjs as `export default { plugins: [...] }` (found by name)".to_string());
    }
    if text.contains("manualChunks") {
        finding.notes.push("output.manualChunks is not supported".to_string());
    }
    finding.notes.push("add --platform=browser for a page (it makes process.env.NODE_ENV \"production\"), and the values given to @rollup/plugin-replace as --define:NAME=VALUE (docs/migrate/rollup.md)".to_string());
    if !no_input {
        finding.commands.push(("build:rkp".to_string(), build));
    }
    if let Some(items) = &rolled {
        let minify = modules.iter().any(|m| m == "@rollup/plugin-terser" || m == "rollup-plugin-terser");
        let config = config.as_deref().unwrap_or("the config");
        finding.notes.push(format!("the values of {config} were read by running it (Node's permission model: no writes, no child processes)"));
        let mut commands: Vec<String> = Vec::new();
        for item in items {
            for command in rolled_commands(item, minify, &mut finding.notes) {
                if !commands.contains(&command) {
                    commands.push(command);
                }
            }
        }
        if commands.is_empty() {
            finding.notes.push("running the config gave no input that is a file name: no command is written; run `rkp bundle ENTRY... --outfile FILE` with the entries of the config".to_string());
        }
        for (i, command) in commands.into_iter().enumerate() {
            finding.commands.push((if i == 0 { "build:rkp".to_string() } else { format!("build:rkp-{}", i + 1) }, command));
        }
    }
    Some(finding)
}

/// Whether the text of a Rollup config gives the input and the output file or folder as plain strings (nothing to run then).
fn text_is_complete(text: &str) -> bool {
    let output = text.find("output").map(|i| &text[i..text[i..].find('}').map_or(text.len(), |e| i + e)]).unwrap_or("");
    literal_after(text, "input").is_some() && (literal_after(output, "file").is_some() || literal_after(output, "dir").is_some())
}

/// Whether a config file is JavaScript that Node can import (TypeScript is not run).
fn is_script(file: &str) -> bool {
    [".js", ".mjs", ".cjs"].iter().any(|e| file.ends_with(e))
}

/// Imports the config, calls it when it is a function, and prints what the commands need as one JSON line after a marker (a plugin may print
/// too): `[{"input": [files] | null, "outputs": [{"file","dir","format","name","sourcemap"}], "plugins": [names]}]`. Absolute paths inside
/// the project are made relative to it.
const ROLLUP_PROBE: &str = r#"
import path from 'node:path';
import { pathToFileURL } from 'node:url';
// Paths are written with `/` (path.join on Windows gives `\\`).
const rel = (v) => (typeof v !== 'string' ? v : (path.isAbsolute(v) && !path.relative(process.cwd(), v).startsWith('..') ? path.relative(process.cwd(), v) : v).replaceAll('\\', '/'));
const str = (v) => (typeof v === 'string' ? rel(v) : undefined);
const mod = await import(pathToFileURL(process.argv[1]).href);
let config = mod.default ?? mod;
if (typeof config === 'function') config = await config({});
config = await config;
const items = [];
for (const item of [config].flat(Infinity)) {
  const c = await item;
  if (!c || typeof c !== 'object') continue;
  const input = typeof c.input === 'string' ? [c.input] : Array.isArray(c.input) ? c.input : c.input && typeof c.input === 'object' ? Object.values(c.input) : [];
  const outputs = [c.output].flat().filter((o) => o && typeof o === 'object').map((o) => ({ file: str(o.file), dir: str(o.dir), format: str(o.format), name: str(o.name), sourcemap: o.sourcemap === true ? true : str(o.sourcemap) }));
  const plugins = [c.plugins].flat(Infinity).map((p) => p && p.name).filter((n) => typeof n === 'string');
  items.push({ input: input.every((i) => typeof i === 'string') && input.length ? input.map(rel) : null, outputs, plugins });
}
console.log('\nRKP-ROLLUP-CONFIG:' + JSON.stringify(items));
process.exit(0);
"#;

/// Runs the Rollup config `file` of `dir` in Node's permission model (reads only: no file writes, no child processes, no workers or
/// addons) for at most 20 seconds, and returns what it says, or None when it cannot be run (no Node, Node without the permission
/// model, or the config throws, for instance because its plugins are not installed).
fn run_rollup_config(dir: &Path, file: &str) -> Option<Vec<Json>> {
    let node = std::env::var_os("RATHKIT_NODE").unwrap_or_else(|| "node".into());
    let path = dir.join(file).canonicalize().ok()?;
    let cwd = dir.canonicalize().ok()?;
    for flag in ["--permission", "--experimental-permission"] {
        let child = std::process::Command::new(&node)
            .args([flag, "--allow-fs-read=*", "--no-warnings", "--input-type=module", "-e", ROLLUP_PROBE])
            .arg(&path)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return None };
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() > std::time::Duration::from_secs(20) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        let mut out = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        }
        if status.is_some_and(|s| s.success()) {
            let line = out.lines().find_map(|l| l.strip_prefix("RKP-ROLLUP-CONFIG:"))?;
            return Json::parse(line).ok()?.as_array().map(|a| a.to_vec());
        }
    }
    None
}

/// The commands for one config of the list a probe returned (one for each output).
fn rolled_commands(item: &Json, minify: bool, notes: &mut Vec<String>) -> Vec<String> {
    let Some(inputs) = item.get("input").and_then(Json::as_array) else {
        notes.push("a config in the list has no input that is a file name (or a name that is a path): no command for it".to_string());
        return Vec::new();
    };
    let inputs: Vec<&str> = inputs.iter().filter_map(Json::as_str).map(|i| i.trim_start_matches("./")).collect();
    let terser = minify || item.get("plugins").and_then(Json::as_array).is_some_and(|p| p.iter().filter_map(Json::as_str).any(|n| n == "terser"));
    let mut outputs: Vec<&Json> = item.get("outputs").and_then(Json::as_array).map(|o| o.iter().collect()).unwrap_or_default();
    let none = Json::Object(Vec::new());
    if outputs.is_empty() {
        outputs.push(&none);
    }
    let mut commands = Vec::new();
    for output in outputs {
        let text = |key: &str| output.get(key).and_then(Json::as_str);
        let mut command = format!("rkp bundle {}", inputs.join(" "));
        match (text("file"), text("dir")) {
            (Some(file), _) => command.push_str(&format!(" --outfile {file}")),
            (None, Some(dir)) => command.push_str(&format!(" --outdir {dir} --splitting")),
            _ => {
                let stem = inputs[0].rsplit('/').next().map(|n| n.rsplit_once('.').map_or(n, |(a, _)| a)).unwrap_or("index");
                command.push_str(&format!(" --outfile dist/{stem}.js"));
                notes.push("the config names no output file or folder: --outfile dist/... is a stand-in, set the file you publish".to_string());
            }
        }
        let mut global_name = text("name");
        match text("format") {
            Some("cjs" | "commonjs") => {
                command.push_str(" --format=cjs");
                global_name = None;
            }
            Some("iife") => command.push_str(" --format=iife"),
            Some("es" | "esm" | "module") | None => global_name = None,
            // UMD is the format of a library that is loaded by a script tag or `require`: the iife with the name is the nearest that rkp writes.
            Some(other @ ("umd" | "amd" | "system")) => {
                if global_name.is_some() && other == "umd" {
                    command.push_str(" --format=iife");
                    notes.push("output.format 'umd' is written as --format=iife with --global-name (no AMD or CommonJS wrapper); build a second command with --format=cjs when `require` has to work".to_string());
                } else {
                    notes.push(format!("output.format '{other}' is not supported (esm, cjs and iife are): no command for {}", text("file").or(text("dir")).unwrap_or("that output")));
                    continue;
                }
            }
            Some(other) => {
                notes.push(format!("output.format '{other}' is not supported (esm, cjs and iife are): no command for it"));
                continue;
            }
        }
        if let Some(name) = global_name.take() {
            command.push_str(&format!(" --global-name={name}"));
        }
        match output.get("sourcemap") {
            Some(Json::Bool(true)) => command.push_str(" --sourcemap"),
            Some(Json::String(s)) if s == "inline" => command.push_str(" --sourcemap=inline"),
            _ => {}
        }
        if terser {
            command.push_str(" --minify");
        }
        commands.push(command);
    }
    commands
}

/// rspack: its config is webpack's, so the command is webpack's with the output folder.
fn rspack(dir: &Path, package: Option<&Json>) -> Option<Finding> {
    let config = config_file(dir, "rspack.config", package);
    let mut evidence: Vec<String> = config.iter().cloned().collect();
    if has_dependency(package, "@rspack/core") || has_dependency(package, "@rspack/cli") {
        evidence.push("dependency @rspack/core".to_string());
    }
    if evidence.is_empty() {
        return None;
    }
    let text = config.as_ref().and_then(|c| std::fs::read_to_string(dir.join(c)).ok()).unwrap_or_default();
    let mut finding = Finding { tool: "rspack", evidence, commands: Vec::new(), notes: Vec::new() };
    subfolder_note(&mut finding, config.as_ref());
    let entry = literal_after(&text, "main").or_else(|| literal_after(&text, "entry")).map(|e| e.trim_start_matches("./").to_string());
    let mut build = "rkp bundle".to_string();
    match entry.or_else(|| dir.join("src/index.js").is_file().then(|| "src/index.js".to_string())) {
        Some(e) => build.push_str(&format!(" {e}")),
        None => finding.notes.push("the entry is not a plain string in the config: name it on the command line".to_string()),
    }
    if let Some(c) = &config {
        build.push_str(&format!(" --config {c}"));
    }
    build.push_str(" --outdir dist");
    finding.commands.push(("build:rkp".to_string(), build));
    finding.notes.push("rspack's config is read like webpack's (docs/migrate/webpack.md); mode and output are on the command above".to_string());
    Some(finding)
}

/// The text printed for the findings.
pub fn report(dir: &Path, found: &[Finding]) -> String {
    let mut out = String::new();
    if found.is_empty() {
        out.push_str(&format!("{}: no esbuild, Vite, webpack, Rollup or rspack found (no config file, dependency or script)\n", dir.display()));
        return out;
    }
    for f in found {
        out.push_str(&format!("{} ({})\n", f.tool, f.evidence.join(", ")));
        for (name, command) in &f.commands {
            out.push_str(&format!("  {name}: {command}\n"));
        }
        for note in &f.notes {
            out.push_str(&format!("  note: {note}\n"));
        }
        out.push_str(&format!("  guide: docs/migrate/{}.md\n", if f.tool == "rspack" { "webpack" } else { f.tool }));
    }
    out
}

/// Adds the commands as scripts to the package.json of `dir` (named `NAME:rkp`, with the tool in the name when there are several tools),
/// never changing a script that is there. Returns what was added and what was left.
pub fn write(dir: &Path, found: &[Finding]) -> Result<Vec<String>, String> {
    let path = dir.join("package.json");
    let mut text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let existing: Vec<String> = Json::parse(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .get("scripts")
        .and_then(Json::as_object)
        .map(|s| s.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    let several = found.len() > 1;
    let mut log = Vec::new();
    let mut added: Vec<(String, String)> = Vec::new();
    for f in found {
        for (name, command) in &f.commands {
            // `build:rkp` is `build:rkp-vite` when there are several tools, and `build:rkp-2` is `build:rkp-vite-2`.
            let name = if several && name.contains(":rkp") { name.replacen(":rkp", &format!(":rkp-{}", f.tool), 1) } else { name.clone() };
            if existing.contains(&name) || added.iter().any(|(n, _)| *n == name) {
                log.push(format!("left {name}: a script of that name is there"));
            } else {
                log.push(format!("added {name}"));
                added.push((name, command.clone()));
            }
        }
    }
    if added.is_empty() {
        return Ok(log);
    }
    text = insert_scripts(&text, &added, !existing.is_empty())?;
    Json::parse(&text).map_err(|e| format!("the edited package.json is not valid ({e}); nothing was written"))?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(log)
}

/// The package.json text with the scripts put first in the `scripts` object (made when there is none). An object written on one line is
/// written again with one script to a line; otherwise nothing that was there is touched.
fn insert_scripts(text: &str, scripts: &[(String, String)], has_scripts: bool) -> Result<String, String> {
    let quote = |s: &str| Json::String(s.to_string()).to_string();
    // The indent of the file: that of its first indented line, two spaces when it has none.
    let unit: String = text.lines().find(|l| l.starts_with([' ', '\t'])).map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect()).unwrap_or_else(|| "  ".to_string());
    let entry = |level: usize, name: &str, command: &str| format!("{}{}: {}", unit.repeat(level), quote(name), quote(command));
    if !has_scripts {
        let open = text.find('{').ok_or("package.json is not an object")?;
        let lines: Vec<String> = scripts.iter().map(|(n, c)| entry(2, n, c)).collect();
        let insert = format!("\n{unit}\"scripts\": {{\n{}\n{unit}}},", lines.join(",\n"));
        return Ok(format!("{}{}{}", &text[..open + 1], insert, &text[open + 1..]));
    }
    let key = text.find("\"scripts\"").ok_or("package.json has no top-level \"scripts\" to add to")?;
    let open = key + text[key..].find('{').ok_or("package.json: \"scripts\" is not an object")?;
    let after = &text[open + 1..];
    if after.trim_start_matches([' ', '\t']).starts_with(['\n', '\r']) {
        // One script to a line already: new lines go first, with the indent of the line that follows.
        let body_empty = after.trim_start().starts_with('}');
        let line_indent: String = after.trim_start_matches(['\r', '\n']).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let line_indent = if line_indent.is_empty() { unit.repeat(2) } else { line_indent };
        let mut insert = String::new();
        for (i, (name, command)) in scripts.iter().enumerate() {
            let last = i + 1 == scripts.len();
            insert.push_str(&format!("\n{line_indent}{}: {}{}", quote(name), quote(command), if last && body_empty { "" } else { "," }));
        }
        return Ok(format!("{}{}{}", &text[..open + 1], insert, &text[open + 1..]));
    }
    // On one line: written again, the new scripts first and the old ones as they were.
    let parsed = Json::parse(text).map_err(|e| e.to_string())?;
    let old = parsed.get("scripts").and_then(Json::as_object).ok_or("package.json: \"scripts\" is not an object")?;
    let close = matching_brace(text, open).ok_or("package.json: \"scripts\" is not closed")?;
    let mut lines: Vec<String> = scripts.iter().map(|(n, c)| entry(2, n, c)).collect();
    for (name, value) in old {
        lines.push(format!("{}{}: {}", unit.repeat(2), quote(name), value));
    }
    Ok(format!("{}{{\n{}\n{unit}}}{}", &text[..open], lines.join(",\n"), &text[close + 1..]))
}

/// The index of the `}` that closes the `{` at `open`, skipping strings.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (i, c) in text[open..].char_indices() {
        match (in_string, c) {
            (true, _) if escaped => escaped = false,
            (true, '\\') => escaped = true,
            (true, '"') => in_string = false,
            (true, _) => {}
            (false, '"') => in_string = true,
            (false, '{') => depth += 1,
            (false, '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_read_from_config_text() {
        let text = "export default { build: { outDir: 'build', other: x }, base: \"/app/\", entry: `./a.js`, nested.outDir: 'no', xoutDir: 'no' }";
        assert_eq!(literal_after(text, "outDir").as_deref(), Some("build"));
        assert_eq!(literal_after(text, "base").as_deref(), Some("/app/"));
        assert_eq!(literal_after(text, "entry").as_deref(), Some("./a.js"));
        assert_eq!(literal_after(text, "other"), None);
        assert_eq!(literal_after("a: `x${y}`", "a"), None);
        assert!(flag_after("{ sourcemap: true }", "sourcemap") && !flag_after("{ sourcemap: false }", "sourcemap"));
    }

    #[test]
    fn esbuild_commands_become_rkp_commands() {
        assert_eq!(rewrite_esbuild_commands("esbuild a.js --bundle && echo esbuild x; esbuild b.js"), ("rkp bundle a.js --bundle && echo esbuild x; rkp bundle b.js".to_string(), true));
        assert_eq!(rewrite_esbuild_commands("node build.mjs"), ("node build.mjs".to_string(), false));
        assert!(!rewrite_esbuild_commands("esbuild-wasm a.js").1);
    }

    #[test]
    fn the_first_module_script_of_a_page() {
        assert_eq!(first_module_script("<script src=\"x.js\"></script><script type=\"module\" src=\"/src/main.ts\"></script>").as_deref(), Some("src/main.ts"));
        assert_eq!(first_module_script("<p>none</p>"), None);
    }

    #[test]
    fn scripts_are_inserted_in_package_json_text() {
        let with = "{\n  \"name\": \"a\",\n  \"scripts\": {\n    \"build\": \"x\"\n  }\n}\n";
        let out = insert_scripts(with, &[("build:rkp".into(), "rkp bundle \"a\"".into())], true).unwrap();
        assert!(Json::parse(&out).is_ok(), "{out}");
        assert!(out.contains("    \"build:rkp\": \"rkp bundle \\\"a\\\"\",\n    \"build\": \"x\""));
        let without = "{\n  \"name\": \"a\"\n}\n";
        let out = insert_scripts(without, &[("b:rkp".into(), "c".into())], false).unwrap();
        assert!(Json::parse(&out).is_ok(), "{out}");
        let empty = "{\"scripts\": {}}";
        assert!(Json::parse(&insert_scripts(empty, &[("b:rkp".into(), "c".into())], true).unwrap()).is_ok());
        let inline = "{\n  \"scripts\": { \"dev\": \"vite {x}\", \"build\": \"vite build\" },\n  \"n\": 1\n}\n";
        let out = insert_scripts(inline, &[("b:rkp".into(), "c".into())], true).unwrap();
        let parsed = Json::parse(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
        assert_eq!(parsed.get("scripts").and_then(Json::as_object).unwrap().len(), 3);
        assert_eq!(parsed.get("n").and_then(Json::as_f64), Some(1.0));
    }
}
