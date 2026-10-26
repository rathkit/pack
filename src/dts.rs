//! `--dts`: declaration files for the entry points, written by the project's own `typescript` package (run through Node).
//!
//! The files are what `tsc --declaration --emitDeclarationOnly` writes: one for each source file the entries reach, not merged into
//! one, laid out from the folder the sources share. Point `types` in package.json at the one beside the entry's output.

use std::path::{Path, PathBuf};

use crate::federation::{DTS, quote, run_node};
use crate::json::Json;

pub struct Emitted {
    /// The declaration files, relative to the folder they were written to.
    pub files: Vec<String>,
    /// What TypeScript reported about the sources (the files are written all the same).
    pub messages: Vec<String>,
}

/// Writes declarations for `entries` into `out_dir`. `ts_config` is a `tsconfig.json` to use instead of the nearest one.
pub fn emit(entries: &[PathBuf], out_dir: &Path, ts_config: Option<&str>, cwd: &Path) -> Result<Emitted, String> {
    let list = entries.iter().map(|e| quote(&e.to_string_lossy())).collect::<Vec<_>>().join(",");
    let input = format!(
        "{{\"command\":\"emit\",\"cwd\":{},\"entries\":[{list}],\"outDir\":{},\"tsConfigPath\":{}}}",
        quote(&cwd.to_string_lossy()),
        quote(&out_dir.to_string_lossy()),
        ts_config.map_or("null".to_string(), quote),
    );
    let answer = run_node(DTS, &input, cwd)?;
    let messages = match answer.get("messages") {
        Some(Json::Array(notes)) => notes.iter().filter_map(|n| n.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    };
    if let Some(error) = answer.get("error").and_then(Json::as_str) {
        return Err(error.lines().next().unwrap_or("").to_string());
    }
    let files = match answer.get("written") {
        Some(Json::Array(items)) => items.iter().filter_map(|n| n.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    };
    Ok(Emitted { files, messages })
}
