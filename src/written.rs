//! Every file a build writes, with what kind of file it is. The command lists them for `--list-files=FILE` (the Node API's `files`), and
//! the metafile's `outputs` has them with their kind, so a caller can compare a build's writes with what it declared.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::bundle::{Output, OutputFile};

/// What a written file is. The names are the ones the metafile's `kind` field uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The file an entry stands for (a script, or the style sheet an entry makes).
    Entry,
    /// Shared code, or code loaded with `import()`.
    Chunk,
    /// A source map.
    Map,
    /// A copied or generated file that is not code: an image, a federation manifest.
    Asset,
    /// A page.
    Html,
    /// A file copied from `public/`.
    Public,
    /// A declaration file.
    Dts,
    /// A license file (`--legal-comments=linked` or `external`).
    Legal,
    /// The file of `--metafile`.
    Metafile,
    /// The `package.json` that `--write-exports` changed.
    Exports,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Entry => "entry",
            Kind::Chunk => "chunk",
            Kind::Map => "map",
            Kind::Asset => "asset",
            Kind::Html => "html",
            Kind::Public => "public",
            Kind::Dts => "dts",
            Kind::Legal => "legal",
            Kind::Metafile => "metafile",
            Kind::Exports => "exports",
        }
    }
}

/// One file the build wrote: where (absolute), what it is, and how many bytes.
#[derive(Clone, Debug)]
pub struct Written {
    pub path: PathBuf,
    pub kind: Kind,
    pub bytes: u64,
}

static LIST: Mutex<Vec<Written>> = Mutex::new(Vec::new());

/// Starts a new list (a command, or a rebuild when watching).
pub fn reset() {
    LIST.lock().unwrap().clear();
}

/// Notes a file that was written. A file written twice is listed once, at its first place, with the last kind and size.
pub fn record(path: &Path, kind: Kind, bytes: u64) {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut list = LIST.lock().unwrap();
    match list.iter_mut().find(|w| w.path == path) {
        Some(known) => {
            known.kind = kind;
            known.bytes = bytes;
        }
        None => list.push(Written { path, kind, bytes }),
    }
}

/// What has been written since the last [`reset`].
pub fn all() -> Vec<Written> {
    LIST.lock().unwrap().clone()
}

/// The kind of one file of the bundler's output.
pub fn kind_of(output: &Output, file: &OutputFile) -> Kind {
    if file.data.is_some() {
        return Kind::Asset;
    }
    if file.name.ends_with(".LEGAL.txt") {
        return Kind::Legal;
    }
    let stem = |name: &str| name.rsplit_once('.').map_or(name.to_string(), |(s, _)| s.to_string());
    let is_css = file.name.ends_with(".css");
    let own = stem(&file.name);
    match output.chunks.iter().find(|c| if is_css { stem(&c.file) == own } else { c.file == file.name }) {
        Some(chunk) if chunk.is_entry => Kind::Entry,
        Some(_) => Kind::Chunk,
        None => Kind::Asset,
    }
}
