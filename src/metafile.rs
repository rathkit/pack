//! `--metafile=FILE`: what went into a build and what came out, as JSON in the shape of esbuild's metafile, so the analyzers that read
//! that format (bundle-buddy, esbuild's analyze page) can show it.
//!
//! `inputs` has every module of the graph (its size, and what it imports); `outputs` has every file written (its size, the modules in it,
//! what it imports and exports, and the entry it stands for). Rathkit Pack does not measure what each module became in the output: the
//! `bytesInOutput` of a module is its original size scaled so that the modules of a file add up to the file's size, an estimate.

use std::path::Path;

use crate::bundle::Output;
use crate::graph::{Edge, Graph, ModuleKind};
use crate::json::Json;
use crate::module::RequestKind;

fn number(n: usize) -> Json {
    Json::Number(n as f64)
}

fn object(fields: Vec<(&str, Json)>) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A path as the metafile writes it: relative to the project when it is inside, with `/` between the parts.
fn shown(cwd: &Path, path: &str) -> String {
    let p = Path::new(path);
    p.strip_prefix(cwd).unwrap_or(p).to_string_lossy().replace('\\', "/")
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

pub fn build(graph: &Graph, output: &Output, cwd: &Path) -> String {
    // Modules are named by their real paths, so the project folder must be too: a folder given by a short or linked name
    // (Windows 8.3 names, a symlinked parent) would otherwise leave every path absolute.
    let real_cwd = std::fs::canonicalize(cwd).map(|c| crate::resolve::without_verbatim_prefix(&c)).unwrap_or_else(|_| cwd.to_path_buf());
    let cwd = real_cwd.as_path();
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
                    let mut fields = vec![("path", Json::String(name.clone())), ("kind", Json::String("import-statement".into()))];
                    if external {
                        fields.push(("external", Json::Bool(true)));
                    }
                    imports.push(object(fields));
                }
                for name in &info.dynamic_imports {
                    imports.push(object(vec![("path", Json::String(name.clone())), ("kind", Json::String("dynamic-import".into()))]));
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
        outputs.push((file.name.clone(), object(fields)));
    }
    let mut text = Json::Object(vec![("inputs".to_string(), Json::Object(inputs)), ("outputs".to_string(), Json::Object(outputs))]).to_string();
    text.push('\n');
    text
}
