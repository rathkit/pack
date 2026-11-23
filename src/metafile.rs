//! `--metafile=FILE`: what went into a build and what came out, as JSON in the shape of esbuild's metafile, so the analyzers that read
//! that format (bundle-buddy, esbuild's analyze page) can show it.
//!
//! `inputs` has every module of the graph (its size, and what it imports); `outputs` has every file the build wrote, named by its path from
//! the working folder (`dist/main.js`, as esbuild's metafile does), with its size, its `kind` (see [`crate::written::Kind`]) and, for the
//! scripts and style sheets, the modules in it, what it imports and exports, and the entry it stands for. Rathkit Pack does not measure what
//! each module became in the output: the `bytesInOutput` of a module is its original size scaled so that the modules of a file add up to the
//! file's size, an estimate.
//!
//! The metafile is made in two steps. [`build`] describes the scripts and style sheets the bundler made, which happens before anything is
//! written; [`finish`] runs when the build is over and sets that against what was written (a file the build made and then did not write is
//! left out; maps, pages, copied files, declarations and the rest are added), so the list is what is on disk. The metafile itself is not in
//! its `outputs`: it cannot hold its own size.

use std::path::Path;

use crate::bundle::Output;
use crate::written::{Kind, Written, kind_of};
use crate::graph::{Edge, Graph, ModuleKind};
use crate::json::Json;
use crate::module::RequestKind;

fn number(n: usize) -> Json {
    Json::Number(n as f64)
}

fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A path as the metafile writes it: relative to the project when it is inside, with `/` between the parts. Both are compared
/// as text with `/` and without a `\\?\` prefix, and without regard to case on Windows, where one folder is written in several ways.
fn shown(cwd: &Path, path: &str) -> String {
    let plain = |text: &str| text.replace('\\', "/").trim_start_matches("//?/").to_string();
    let (path, root) = (plain(path), plain(&cwd.to_string_lossy()));
    let root = root.trim_end_matches('/');
    let inside = |p: &str, r: &str| p.len() > r.len() && p.is_char_boundary(r.len()) && p.as_bytes()[r.len()] == b'/' && if cfg!(windows) { p[..r.len()].eq_ignore_ascii_case(r) } else { &p[..r.len()] == r };
    if !root.is_empty() && inside(&path, root) {
        return path[root.len() + 1..].to_string();
    }
    // Windows writes one folder with a short name (`RUNNER~1`) or a long one; the files' real names agree.
    if let (Ok(real_path), Ok(real_root)) = (Path::new(&path).canonicalize(), cwd.canonicalize()) {
        let (real_path, real_root) = (plain(&real_path.to_string_lossy()), plain(&real_root.to_string_lossy()));
        let real_root = real_root.trim_end_matches('/');
        if !real_root.is_empty() && inside(&real_path, real_root) {
            return real_path[real_root.len() + 1..].to_string();
        }
    }
    // Outside the project: from the project folder with `..`, so the same build in two checkouts of the same layout writes the same metafile.
    // What is not a place on disk (a made-up module's id) or is on another drive stays as it is.
    let (from, to) = (Path::new(root), Path::new(&path));
    if !root.is_empty() && from.is_absolute() && to.is_absolute() && from.components().next() == to.components().next() {
        return crate::sourcemap::relative(from, to);
    }
    path
}

fn kind_name(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Static => "import-statement",
        RequestKind::Dynamic => "dynamic-import",
        RequestKind::Require => "require-call",
        RequestKind::Resolve => "require-resolve",
        RequestKind::AssetUrl | RequestKind::WorkerUrl => "file-loader",
    }
}

/// The project folder by its real path: modules are named by theirs, so a folder given by a short or linked name (Windows 8.3 names, a
/// symlinked parent) would otherwise leave every path absolute.
fn real_folder(cwd: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(cwd).map(|c| crate::resolve::without_verbatim_prefix(&c)).unwrap_or_else(|_| cwd.to_path_buf())
}

/// `dir` joined to `name`, made absolute from `cwd`, with `.` and `..` folded away (nothing is looked up on disk, so a folder that does not
/// exist yet is fine).
fn lexical(cwd: &Path, dir: &Path, name: &str) -> String {
    let joined = cwd.join(dir).join(name);
    let mut parts: Vec<std::path::Component> = Vec::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if matches!(parts.last(), Some(std::path::Component::Normal(_))) => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.iter().collect::<std::path::PathBuf>().to_string_lossy().into_owned()
}

/// What the bundler made, as the metafile's JSON, for `finish` to complete. `dir` is the folder the files go to (none when nothing is
/// written: the dev server, or the code on standard output), and the keys of `outputs` are the paths the files will have in it.
pub fn build(graph: &Graph, output: &Output, cwd: &Path, dir: Option<&Path>) -> Json {
    let cwd = real_folder(cwd);
    let cwd = cwd.as_path();
    let key = |name: &str| match dir {
        Some(dir) => shown(cwd, &lexical(cwd, dir, name)),
        None => name.to_string(),
    };
    let mut inputs: Vec<(String, Json)> = Vec::new();
    for module in &graph.modules {
        // A made-up module (a plugin's virtual module, the federation runtime) has no place in the project.
        let mut imports: Vec<Json> = Vec::new();
        for (request, edge) in module.record.requests.iter().zip(&module.edges) {
            let (path, external) = match edge {
                Edge::Module(id) => (shown(cwd, &graph.module(*id).path.to_string_lossy()), false),
                Edge::External(spec) => (spec.clone(), true),
                Edge::Unresolved => continue,
            };
            let mut fields = vec![("path", Json::String(path)), ("kind", Json::String(kind_name(request.kind).to_string()))];
            if external {
                fields.push(("external", Json::Bool(true)));
            }
            imports.push(object(fields));
        }
        let mut fields = vec![("bytes", number(module.source.len())), ("imports", Json::Array(imports))];
        match module.kind {
            ModuleKind::Esm => fields.push(("format", Json::String("esm".into()))),
            ModuleKind::CommonJs => fields.push(("format", Json::String("cjs".into()))),
            _ => {}
        }
        inputs.push((shown(cwd, &module.path.to_string_lossy()), object(fields)));
    }
    let mut outputs: Vec<(String, Json)> = Vec::new();
    for file in &output.files {
        let bytes = file.data.as_ref().map_or(file.code.len(), |d| d.len());
        let stem = file.name.rsplit_once('.').map_or(file.name.as_str(), |(s, _)| s);
        let is_css = file.name.ends_with(".css");
        let info = output.chunks.iter().find(|c| if is_css { c.file.rsplit_once('.').map_or(c.file.as_str(), |(s, _)| s) == stem } else { c.file == file.name });
        let mut contributed: Vec<(String, usize)> = Vec::new();
        let mut imports: Vec<Json> = Vec::new();
        let mut exports: Vec<Json> = Vec::new();
        let mut entry_point = None;
        if let Some(info) = info {
            for (path, size) in &info.modules {
                let is_sheet = graph.modules.iter().any(|m| m.path.to_string_lossy() == path.as_str() && m.css.is_some());
                if is_sheet == is_css {
                    contributed.push((shown(cwd, path), *size));
                }
            }
            if !is_css {
                for name in &info.imports {
                    let external = !output.files.iter().any(|f| f.name == *name);
                    let mut fields = vec![("path", Json::String(if external { name.clone() } else { key(name) })), ("kind", Json::String("import-statement".into()))];
                    if external {
                        fields.push(("external", Json::Bool(true)));
                    }
                    imports.push(object(fields));
                }
                for name in &info.dynamic_imports {
                    let path = if output.files.iter().any(|f| f.name == *name) { key(name) } else { name.clone() };
                    imports.push(object(vec![("path", Json::String(path)), ("kind", Json::String("dynamic-import".into()))]));
                }
                exports = info.exports.iter().map(|e| Json::String(e.clone())).collect();
                if info.is_entry {
                    entry_point = info.facade_module.as_deref().map(|m| shown(cwd, m));
                }
            }
        }
        let total: usize = contributed.iter().map(|(_, s)| *s).sum();
        let by_module: Vec<(String, Json)> = contributed
            .iter()
            .map(|(path, size)| (path.clone(), object(vec![("bytesInOutput", number(if total == 0 { 0 } else { (*size as f64 * bytes as f64 / total as f64).round() as usize }))])))
            .collect();
        let mut fields = vec![("bytes", number(bytes)), ("inputs", Json::Object(by_module)), ("imports", Json::Array(imports)), ("exports", Json::Array(exports))];
        if let Some(entry) = entry_point {
            fields.push(("entryPoint", Json::String(entry)));
        }
        fields.push(("kind", Json::String(kind_of(output, file).name().to_string())));
        outputs.push((key(&file.name), object(fields)));
    }
    Json::Object(vec![("inputs".to_string(), Json::Object(inputs)), ("outputs".to_string(), Json::Object(outputs))])
}

/// The metafile's text: what `build` described, set against `written`. With `listed` false nothing was written (the dev server), and the
/// scripts and style sheets stand as they are.
pub fn finish(described: Json, written: &[Written], cwd: &Path, listed: bool) -> String {
    let cwd = real_folder(cwd);
    let cwd = cwd.as_path();
    let Json::Object(parts) = described else { return String::new() };
    let mut result: Vec<(String, Json)> = Vec::new();
    for (name, value) in parts {
        if name != "outputs" || !listed {
            result.push((name, value));
            continue;
        }
        let Json::Object(made) = value else { continue };
        let files: Vec<(String, &Written)> = written.iter().filter(|w| w.kind != Kind::Metafile).map(|w| (shown(cwd, &w.path.to_string_lossy()), w)).collect();
        let mut outputs: Vec<(String, Json)> = Vec::new();
        for (key, info) in made {
            // A file the build made and did not write (the empty script of a style sheet a page links) is not an output.
            if files.iter().any(|(k, _)| *k == key) {
                outputs.push((key, info));
            }
        }
        for (key, file) in files {
            if !outputs.iter().any(|(k, _)| *k == key) {
                let none = Json::Object(Vec::new());
                let fields = vec![("bytes", number(file.bytes as usize)), ("inputs", none), ("imports", Json::Array(Vec::new())), ("exports", Json::Array(Vec::new())), ("kind", Json::String(file.kind.name().to_string()))];
                outputs.push((key, object(fields)));
            }
        }
        result.push((name, Json::Object(outputs)));
    }
    let mut text = Json::Object(result).to_string();
    text.push('\n');
    text
}

#[cfg(test)]
mod shown_tests {
    use super::shown;
    use std::path::Path;

    #[test]
    fn a_folder_written_in_two_ways_still_holds_its_files() {
        // A path without a drive letter is not a place on disk on Windows.
        let d = if cfg!(windows) { "C:" } else { "" };
        assert_eq!(shown(Path::new(r"\\?\C:\work\project"), "C:/work/project/src/a.js"), "src/a.js");
        assert_eq!(shown(Path::new(r"C:\work\project"), r"C:\work\project\src\a.js"), "src/a.js");
        assert_eq!(shown(Path::new(&format!("{d}/work/project")), &format!("{d}/work/project/a.js")), "a.js");
        // A sibling folder with the same beginning is not inside: it is reached through `..`.
        assert_eq!(shown(Path::new(&format!("{d}/work/project")), &format!("{d}/work/project-two/a.js")), "../project-two/a.js");
        assert_eq!(shown(Path::new(&format!("{d}/work/project")), &format!("{d}/work/shared/lib/b.js")), "../shared/lib/b.js");
        assert_eq!(shown(Path::new(&format!("{d}/work/project")), &format!("{d}/elsewhere/c.js")), "../../elsewhere/c.js");
        // A made-up module has no place to be relative to.
        assert_eq!(shown(Path::new("/work/project"), "\0virtual:x"), "\0virtual:x");
    }
}
