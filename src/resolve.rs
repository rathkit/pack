//! Module resolution: turning `"./util"` or `"react/jsx-runtime"` into a file.
//!
//! This follows Node's algorithm (relative and absolute paths, `node_modules` lookup up the
//! directory tree, `package.json` `main`/`module`, `exports` and `imports` with conditions
//! and subpath patterns), since every mainstream bundler resolves the same way at its core.
//! Bundler-specific extras (aliases, extension aliases, `browser` field maps) sit on top of
//! these rules and are configured through [`ResolveOptions`].
//!
//! The file system is a trait so tests run against an in-memory tree and so the plugin layer
//! can later serve virtual files.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::json::Json;

/// The file system operations resolution needs.
pub trait FileSystem: Send + Sync {
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    /// The file's bytes, for files that are not text.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read_to_string(path).map(String::into_bytes)
    }
    /// Follows symlinks to the real path, so two links to one file are one module.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    /// Whether `path` is not known to be anything but a link (or does not exist): the answer `true` is always safe. When it is
    /// `false`, the real path of `path` is the real path of its directory with its own name added.
    fn may_be_link(&self, _path: &Path) -> bool {
        true
    }
    /// The names in a directory, each with whether it is a directory (for `import.meta.glob`).
    fn read_dir(&self, _path: &Path) -> io::Result<Vec<(String, bool)>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "no directory listing"))
    }
}

/// On Windows `canonicalize` answers `\\?\C:\dir\file`, a form nothing else expects (a `?` is a query in module names, and
/// Node and the plugins write `C:\dir\file`). The plain form is the same file whenever the path has a drive letter.
pub fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() >= 3 && rest.as_bytes()[0].is_ascii_alphabetic() && rest.as_bytes()[1] == b':' && rest.as_bytes()[2] == b'\\' => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The real file system.
pub struct RealFs;

impl FileSystem for RealFs {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        // Source text that is not valid UTF-8 is decoded leniently, as Node does, rather than
        // refusing the file.
        let bytes = std::fs::read(path)?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        })
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            // A link is followed, as the pattern would see the file it points to.
            let is_dir = std::fs::metadata(entry.path()).map(|m| m.is_dir()).unwrap_or(false);
            out.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
        }
        Ok(out)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path).map(|p| without_verbatim_prefix(&p))
    }

    // Only where names are matched exactly: elsewhere the real path can spell a name in another case than the one asked for.
    #[cfg(target_os = "linux")]
    fn may_be_link(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).map_or(true, |m| m.file_type().is_symlink())
    }
}

/// An in-memory file tree, for tests and for virtual files. Paths are matched exactly as
/// given, so use normalized absolute paths.
#[derive(Default)]
pub struct MemFs {
    files: HashMap<PathBuf, String>,
}

impl MemFs {
    pub fn new() -> MemFs {
        MemFs::default()
    }

    pub fn with(mut self, path: &str, contents: &str) -> MemFs {
        self.files.insert(PathBuf::from(path), contents.to_string());
        self
    }
}

impl FileSystem for MemFs {
    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.files.keys().any(|f| f.starts_with(path) && f != path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files.get(path).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// Which kind of request is being resolved. It picks the `import` or `require` condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveKind {
    Import,
    Require,
}

/// Rewrites some specifiers before they are looked up. A pattern with one `*` (as in
/// `tsconfig.json` `paths`) captures the middle and puts it where each replacement has a `*`;
/// a plain name matches itself and, unless `exact_only`, anything below it (`@/x` for `@`).
#[derive(Clone, Debug)]
pub struct Alias {
    pub pattern: String,
    /// Absolute paths, or specifiers to resolve in turn. Tried in order.
    pub replacements: Vec<String>,
    pub exact_only: bool,
}

impl Alias {
    /// Whether the alias applies to the specifier.
    pub fn matches(&self, specifier: &str) -> bool {
        self.capture(specifier).is_some()
    }

    /// The first replacement that is an absolute path, with what the pattern captured filled in: `@/views/*.vue` for `@` -> `/src` is
    /// `/src/views/*.vue`. For patterns that are not module names (globs), which are not looked up as modules.
    pub fn substitute(&self, specifier: &str) -> Option<String> {
        let captured = self.capture(specifier)?;
        self.replacements.iter().filter(|r| Path::new(r.as_str()).is_absolute()).map(|replacement| {
            if replacement.contains('*') { replacement.replace('*', captured) } else { format!("{replacement}{}", if self.pattern.contains('*') { "" } else { captured }) }
        }).next()
    }

    /// What the specifier leaves over after the pattern, if the pattern matches it.
    fn capture<'s>(&self, specifier: &'s str) -> Option<&'s str> {
        match self.pattern.split_once('*') {
            Some((prefix, suffix)) => {
                let middle = specifier.strip_prefix(prefix)?.strip_suffix(suffix)?;
                Some(middle)
            }
            None if specifier == self.pattern => Some(""),
            None if !self.exact_only => specifier.strip_prefix(self.pattern.as_str()).filter(|rest| rest.starts_with('/')),
            None => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolveOptions {
    pub alias: Vec<Alias>,
    /// Bare specifiers are also tried relative to this directory (`baseUrl`).
    pub base_url: Option<PathBuf>,
    /// Extensions tried, in order, for a path without one (with the dot).
    pub extensions: Vec<String>,
    /// `./a.js` may name `a.ts`: TypeScript writes the extension the output will have. Each
    /// entry maps an extension to the source extensions tried when no such file exists.
    pub extension_alias: Vec<(String, Vec<String>)>,
    /// `package.json` fields consulted for a package's entry file, in order.
    pub main_fields: Vec<String>,
    /// File names tried inside a directory, without extension.
    pub main_files: Vec<String>,
    /// Conditions matched in `exports`/`imports`, besides `import`/`require` (picked by the
    /// request) and `default` (always matched): typically `browser` or `node`, and `module`.
    pub conditions: Vec<String>,
    /// Keep symlinked paths as they are instead of resolving them to the real file.
    pub preserve_symlinks: bool,
    /// Honor the object form of a package's `browser` field: a file (or a package) it names is replaced by another file, or by an empty
    /// module when the value is `false`. For a program that a page loads.
    pub browser_map: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        ResolveOptions {
            alias: Vec::new(),
            base_url: None,
            extensions: [".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".mts", ".cts", ".json"].map(String::from).to_vec(),
            extension_alias: vec![
                (".js".to_string(), vec![".ts".to_string(), ".tsx".to_string()]),
                (".jsx".to_string(), vec![".tsx".to_string()]),
                (".mjs".to_string(), vec![".mts".to_string()]),
                (".cjs".to_string(), vec![".cts".to_string()]),
            ],
            main_fields: ["module", "main"].map(String::from).to_vec(),
            main_files: vec!["index".to_string()],
            conditions: ["module"].map(String::from).to_vec(),
            preserve_symlinks: false,
            browser_map: false,
        }
    }
}

/// The `sideEffects` field of a `package.json`: whether importing a file only for its
/// effects (no used bindings) may be dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideEffects {
    /// Absent or `true`: every file may have side effects.
    All,
    /// `false`: no file has side effects.
    None,
    /// An array of globs: only the matching files do.
    Files(Vec<String>),
}

/// The parts of a `package.json` the bundler uses.
#[derive(Debug)]
pub struct Package {
    /// The directory holding `package.json`.
    pub dir: PathBuf,
    pub name: Option<String>,
    pub version: Option<String>,
    /// `"type": "module"`.
    pub is_module: bool,
    pub side_effects: SideEffects,
    json: Json,
}

impl Package {
    /// Whether a file of this package may have side effects. `path` must lie under `dir`.
    pub fn file_has_side_effects(&self, path: &Path) -> bool {
        match &self.side_effects {
            SideEffects::All => true,
            SideEffects::None => false,
            SideEffects::Files(globs) => {
                let Ok(relative) = path.strip_prefix(&self.dir) else { return true };
                let relative = relative.to_string_lossy().replace('\\', "/");
                globs.iter().any(|g| glob_matches(g.strip_prefix("./").unwrap_or(g), &relative))
            }
        }
    }

    pub fn field(&self, name: &str) -> Option<&Json> {
        self.json.get(name)
    }
}

/// The result of resolving a specifier.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub path: PathBuf,
    /// The package the file belongs to, if any (the nearest `package.json` above it).
    pub package: Option<Arc<Package>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    NotFound { specifier: String, from: PathBuf },
    /// `exports` exists but does not list this subpath.
    NotExported { package: String, subpath: String },
    /// `imports` has no entry for a `#specifier`.
    ImportNotDefined { specifier: String },
    /// An `exports`/`imports` target is malformed (it escapes the package, for instance).
    InvalidTarget { target: String },
    InvalidSpecifier { specifier: String },
    /// A `package.json` that does not parse.
    BadPackageJson { path: PathBuf, message: String },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NotFound { specifier, from } => write!(f, "Cannot find '{specifier}' from {}", from.display()),
            ResolveError::NotExported { package, subpath } => {
                write!(f, "Package '{package}' does not export '{subpath}'")
            }
            ResolveError::ImportNotDefined { specifier } => write!(f, "Package import '{specifier}' is not defined"),
            ResolveError::InvalidTarget { target } => write!(f, "Invalid package target '{target}'"),
            ResolveError::InvalidSpecifier { specifier } => write!(f, "Invalid module specifier '{specifier}'"),
            ResolveError::BadPackageJson { path, message } => write!(f, "Cannot parse {}: {message}", path.display()),
        }
    }
}

impl std::error::Error for ResolveError {}

pub struct Resolver {
    fs: Box<dyn FileSystem>,
    options: ResolveOptions,
    /// `package.json` by directory; `Ok(None)` records a directory without one, and a file
    /// that does not parse is remembered too so it is read and reported once.
    packages: Mutex<HashMap<PathBuf, Result<Option<Arc<Package>>, ResolveError>>>,
    /// The real path of each directory a file was found in: a build asks for hundreds of files in a few directories.
    real_dirs: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

/// What an `exports` target turned out to be. `null` blocks a path, while an unmatched
/// condition only falls through to the next one.
enum Target {
    Found(PathBuf),
    Undefined,
    Null,
}

impl Resolver {
    pub fn new(options: ResolveOptions) -> Resolver {
        Resolver::with_fs(Box::new(RealFs), options)
    }

    pub fn with_fs(fs: Box<dyn FileSystem>, options: ResolveOptions) -> Resolver {
        Resolver { fs, options, packages: Mutex::new(HashMap::new()), real_dirs: Mutex::new(HashMap::new()) }
    }

    /// Resolves `specifier` as written in a file that lives in directory `from_dir`.
    pub fn resolve(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Resolved, ResolveError> {
        let not_found = || ResolveError::NotFound { specifier: specifier.to_string(), from: from_dir.to_path_buf() };
        if specifier.is_empty() {
            return Err(ResolveError::InvalidSpecifier { specifier: String::new() });
        }
        if self.options.browser_map {
            if let Some(found) = self.browser_before(from_dir, specifier, kind)? {
                return Ok(found);
            }
        }
        // A name that starts with `/` is a path from the root. On Windows it has no drive, so `Path` does not call it absolute and
        // joining it to a folder would put the drive of that folder in front of it; the virtual modules of a build (`/@rathkit/...`) are named so.
        let rooted = specifier.starts_with('/') && !Path::new(specifier).is_absolute();
        let path = if is_relative(specifier) || rooted || Path::new(specifier).is_absolute() {
            let joined = normalize(&if rooted { PathBuf::from(specifier) } else { from_dir.join(specifier) });
            // A trailing slash says "directory"; a bare `.` or `..` does too.
            let directory_only = specifier.ends_with('/') || specifier == "." || specifier == "..";
            self.load_path(&joined, directory_only)?
        } else if specifier.starts_with('#') {
            self.resolve_package_import(from_dir, specifier, kind)?
        } else {
            self.resolve_bare(from_dir, specifier, kind)?
        };
        let path = path.ok_or_else(not_found)?;
        let path = if self.options.preserve_symlinks { path } else { self.real_path(path) };
        // A `package.json` that does not parse only costs the file its package metadata: Node
        // too reads it only when it needs something from it. `package_for` reports the error.
        let package = self.package_for(&path).unwrap_or(None);
        if self.options.browser_map {
            if let Some(found) = self.browser_after(&path, package.as_deref(), kind)? {
                return Ok(found);
            }
        }
        Ok(Resolved { path, package })
    }

    /// What a package's `browser` map says about `specifier` as written in a file of `from_dir`.
    fn browser_before(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Option<Resolved>, ResolveError> {
        let Some(package) = self.package_for(&from_dir.join("_")).ok().flatten() else { return Ok(None) };
        let Some(map) = package.field("browser").and_then(Json::as_object) else { return Ok(None) };
        if is_relative(specifier) {
            let joined = normalize(&from_dir.join(specifier));
            let found = self.load_path(&joined, false).ok().flatten();
            for (key, value) in map {
                if !is_relative(key) {
                    continue;
                }
                let at = normalize(&package.dir.join(key));
                if at == joined || (found.is_some() && (found.as_ref() == Some(&at) || found == self.load_file(&at))) {
                    return self.browser_target(&package, key, value, specifier, kind);
                }
            }
        } else if !Path::new(specifier).is_absolute() {
            for (key, value) in map {
                if key == specifier {
                    return self.browser_target(&package, key, value, specifier, kind);
                }
            }
        }
        Ok(None)
    }

    /// What the `browser` map of the package that holds `path` says about that file.
    fn browser_after(&self, path: &Path, package: Option<&Package>, kind: ResolveKind) -> Result<Option<Resolved>, ResolveError> {
        let Some(package) = package else { return Ok(None) };
        let Some(map) = package.field("browser").and_then(Json::as_object) else { return Ok(None) };
        for (key, value) in map {
            if !is_relative(key) {
                continue;
            }
            let at = normalize(&package.dir.join(key));
            if at == path || self.load_file(&at).as_deref() == Some(path) {
                return self.browser_target(package, key, value, key, kind);
            }
        }
        Ok(None)
    }

    fn browser_target(&self, package: &Package, key: &str, value: &Json, written: &str, kind: ResolveKind) -> Result<Option<Resolved>, ResolveError> {
        match value {
            Json::Bool(false) => Ok(Some(Resolved { path: PathBuf::from(format!("\0browser-external:{key}")), package: None })),
            // A file or a package in its place; a map that points a name at itself is no replacement.
            Json::String(target) if !target.is_empty() && target != written && target != key => self.resolve(&package.dir, target, kind).map(Some),
            _ => Ok(None),
        }
    }

    pub fn is_file(&self, path: &Path) -> bool {
        self.fs.is_file(path)
    }

    /// The bytes of a file, read through the same file system.
    /// The file system the resolver reads (for `import.meta.glob`).
    pub fn file_system(&self) -> &dyn FileSystem {
        &*self.fs
    }

    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.fs.read(path)
    }

    /// Reads a file through the same file system resolution uses.
    pub fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.fs.read_to_string(path)
    }

    /// `path` with its symlinks followed. A file that is not a link has the real path of its directory, which is asked once
    /// for all the files in it, in place of walking every folder of its path for each one.
    fn real_path(&self, path: PathBuf) -> PathBuf {
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            if !self.fs.may_be_link(&path) {
                let cached = self.real_dirs.lock().unwrap().get(dir).cloned();
                let real = match cached {
                    Some(real) => real,
                    None => {
                        let real = self.fs.canonicalize(dir).ok();
                        self.real_dirs.lock().unwrap().insert(dir.to_path_buf(), real.clone());
                        real
                    }
                };
                if let Some(real) = real {
                    return real.join(name);
                }
            }
        }
        self.fs.canonicalize(&path).unwrap_or(path)
    }

    /// The nearest `package.json` at or above `path`'s directory.
    pub fn package_for(&self, path: &Path) -> Result<Option<Arc<Package>>, ResolveError> {
        let mut dir = if self.fs.is_dir(path) { Some(path) } else { path.parent() };
        while let Some(d) = dir {
            // A `node_modules` directory itself is never a package root.
            if d.file_name().is_some_and(|n| n == "node_modules") {
                return Ok(None);
            }
            if let Some(package) = self.read_package(d)? {
                return Ok(Some(package));
            }
            dir = d.parent();
        }
        Ok(None)
    }

    fn read_package(&self, dir: &Path) -> Result<Option<Arc<Package>>, ResolveError> {
        if let Some(cached) = self.packages.lock().unwrap().get(dir) {
            return cached.clone();
        }
        let file = dir.join("package.json");
        let package = if self.fs.is_file(&file) {
            let bad = |message: String| ResolveError::BadPackageJson { path: file.clone(), message };
            self.fs
                .read_to_string(&file)
                .map_err(|e| bad(e.to_string()))
                .and_then(|text| Json::parse(&text).map_err(|e| bad(e.to_string())))
                .map(|json| Some(Arc::new(Package::from_json(dir, json))))
        } else {
            Ok(None)
        };
        self.packages.lock().unwrap().insert(dir.to_path_buf(), package.clone());
        package
    }

    // ---- files and directories -----------------------------------------------------------

    /// A path that must exist as a file (with an extension added if needed) or a directory.
    fn load_path(&self, path: &Path, directory_only: bool) -> Result<Option<PathBuf>, ResolveError> {
        if !directory_only {
            if let Some(file) = self.load_file(path) {
                return Ok(Some(file));
            }
        }
        self.load_directory(path)
    }

    fn load_file(&self, path: &Path) -> Option<PathBuf> {
        if self.fs.is_file(path) {
            return Some(path.to_path_buf());
        }
        // `a.b` with extension `.js` is `a.b.js`: the extension is appended, never replaced.
        for ext in &self.options.extensions {
            let mut name = path.as_os_str().to_os_string();
            name.push(ext);
            let candidate = PathBuf::from(name);
            if self.fs.is_file(&candidate) {
                return Some(candidate);
            }
        }
        // `a.js` that does not exist may be `a.ts`.
        let text = path.to_string_lossy();
        for (from, tos) in &self.options.extension_alias {
            if let Some(stem) = text.strip_suffix(from.as_str()) {
                for to in tos {
                    let candidate = PathBuf::from(format!("{stem}{to}"));
                    if self.fs.is_file(&candidate) {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    fn load_directory(&self, dir: &Path) -> Result<Option<PathBuf>, ResolveError> {
        if !self.fs.is_dir(dir) {
            return Ok(None);
        }
        if let Some(package) = self.read_package(dir)? {
            for field in &self.options.main_fields {
                let Some(main) = package.field(field).and_then(Json::as_str) else { continue };
                if main.is_empty() {
                    continue;
                }
                let target = normalize(&dir.join(main));
                if let Some(found) = self.load_file(&target).or(self.load_index(&target)) {
                    return Ok(Some(found));
                }
            }
        }
        Ok(self.load_index(dir))
    }

    fn load_index(&self, dir: &Path) -> Option<PathBuf> {
        if !self.fs.is_dir(dir) {
            return None;
        }
        self.options.main_files.iter().find_map(|name| self.load_file(&dir.join(name)))
    }

    // ---- bare specifiers ------------------------------------------------------------------

    fn resolve_bare(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Option<PathBuf>, ResolveError> {
        if let Some(found) = self.resolve_alias(from_dir, specifier, kind)? {
            return Ok(Some(found));
        }
        if let Some(base) = &self.options.base_url {
            if let Some(found) = self.load_path(&normalize(&base.join(specifier)), specifier.ends_with('/'))? {
                return Ok(Some(found));
            }
        }
        self.resolve_package(from_dir, specifier, kind)
    }

    fn resolve_alias(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Option<PathBuf>, ResolveError> {
        for alias in &self.options.alias {
            let Some(captured) = alias.capture(specifier) else { continue };
            for replacement in &alias.replacements {
                // `*` stands for what the pattern captured; a plain alias keeps what follows it.
                let text = if replacement.contains('*') { replacement.replace('*', captured) } else { format!("{replacement}{}", if alias.pattern.contains('*') { "" } else { captured }) };
                let found = if Path::new(&text).is_absolute() {
                    self.load_path(&normalize(Path::new(&text)), text.ends_with('/'))?
                } else {
                    // Another package: look it up by name, without aliases again.
                    self.resolve_package(from_dir, &text, kind)?
                };
                if found.is_some() {
                    return Ok(found);
                }
            }
        }
        Ok(None)
    }

    fn resolve_package(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Option<PathBuf>, ResolveError> {
        let (name, subpath) = split_package_name(specifier).ok_or_else(|| ResolveError::InvalidSpecifier { specifier: specifier.to_string() })?;
        // A package may import itself by name when it has `exports`.
        if let Some(package) = self.package_for(from_dir).unwrap_or(None) {
            if package.name.as_deref() == Some(name) {
                if let Some(exports) = package.field("exports") {
                    return self.resolve_exports(&package, name, &subpath, exports, kind);
                }
            }
        }
        let mut dir = Some(from_dir);
        while let Some(d) = dir {
            if d.file_name().is_none_or(|n| n != "node_modules") {
                let package_dir = d.join("node_modules").join(name);
                if self.fs.is_dir(&package_dir) {
                    if let Some(package) = self.read_package(&package_dir)? {
                        if let Some(exports) = package.field("exports") {
                            return self.resolve_exports(&package, name, &subpath, exports, kind);
                        }
                    }
                    let target = if subpath == "." { package_dir } else { normalize(&package_dir.join(&subpath)) };
                    let found = if subpath == "." {
                        self.load_directory(&target)?
                    } else {
                        self.load_path(&target, subpath.ends_with('/'))?
                    };
                    if found.is_some() {
                        return Ok(found);
                    }
                }
            }
            dir = d.parent();
        }
        Ok(None)
    }

    // ---- exports and imports ---------------------------------------------------------------

    fn is_condition(&self, condition: &str, kind: ResolveKind) -> bool {
        condition == "default"
            || condition == if kind == ResolveKind::Import { "import" } else { "require" }
            || self.options.conditions.iter().any(|c| c == condition)
    }

    fn resolve_exports(&self, package: &Package, name: &str, subpath: &str, exports: &Json, kind: ResolveKind) -> Result<Option<PathBuf>, ResolveError> {
        let not_exported = || ResolveError::NotExported { package: name.to_string(), subpath: subpath.to_string() };
        // `"exports": "./x.js"` and `"exports": { "import": ... }` mean the root subpath.
        let sugar = match exports {
            Json::String(_) | Json::Array(_) => true,
            Json::Object(entries) => !entries.iter().any(|(k, _)| k.starts_with('.')),
            _ => false,
        };
        let key = if subpath == "." { ".".to_string() } else { format!("./{}", subpath.trim_start_matches("./")) };
        let target = if sugar {
            if key != "." {
                return Err(not_exported());
            }
            self.resolve_target(package, exports, None, false, kind)?
        } else {
            let map = exports.as_object().unwrap();
            self.resolve_in_map(package, map, &key, false, kind)?.ok_or_else(not_exported)?
        };
        match target {
            Target::Found(path) => Ok(Some(path)),
            Target::Undefined | Target::Null => Err(not_exported()),
        }
    }

    fn resolve_package_import(&self, from_dir: &Path, specifier: &str, kind: ResolveKind) -> Result<Option<PathBuf>, ResolveError> {
        let undefined = || ResolveError::ImportNotDefined { specifier: specifier.to_string() };
        if specifier == "#" || specifier.starts_with("#/") {
            return Err(ResolveError::InvalidSpecifier { specifier: specifier.to_string() });
        }
        let package = self.package_for(from_dir)?.ok_or_else(undefined)?;
        let imports = package.field("imports").and_then(Json::as_object).ok_or_else(undefined)?;
        match self.resolve_in_map(&package, imports, specifier, true, kind)?.ok_or_else(undefined)? {
            Target::Found(path) => Ok(Some(path)),
            Target::Undefined | Target::Null => Err(undefined()),
        }
    }

    /// Looks `key` up in an `exports`/`imports` map: an exact key first, then the best
    /// `prefix*suffix` pattern (longest prefix wins).
    fn resolve_in_map(&self, package: &Package, map: &[(String, Json)], key: &str, is_imports: bool, kind: ResolveKind) -> Result<Option<Target>, ResolveError> {
        if !key.contains('*') {
            if let Some((_, value)) = map.iter().find(|(k, _)| k == key) {
                return Ok(Some(self.resolve_target(package, value, None, is_imports, kind)?));
            }
        }
        let mut best: Option<(&str, &Json, &str)> = None;
        for (pattern, value) in map {
            let Some(star) = pattern.find('*') else { continue };
            let (prefix, suffix) = (&pattern[..star], &pattern[star + 1..]);
            if suffix.contains('*') || key.len() < pattern.len() - 1 {
                continue;
            }
            if key.starts_with(prefix) && key.ends_with(suffix) && key != prefix {
                let better = best.is_none_or(|(p, _, _)| pattern.len() > p.len() || (pattern.len() == p.len() && prefix.len() > p.find('*').unwrap()));
                if better {
                    best = Some((pattern, value, suffix));
                }
            }
        }
        let Some((pattern, value, suffix)) = best else { return Ok(None) };
        let star = pattern.find('*').unwrap();
        let matched = &key[star..key.len() - suffix.len()];
        Ok(Some(self.resolve_target(package, value, Some(matched), is_imports, kind)?))
    }

    fn resolve_target(&self, package: &Package, target: &Json, pattern_match: Option<&str>, is_imports: bool, kind: ResolveKind) -> Result<Target, ResolveError> {
        let invalid = |t: &str| ResolveError::InvalidTarget { target: t.to_string() };
        match target {
            Json::String(text) => {
                if !text.starts_with("./") {
                    // In `imports` a bare target names another package; in `exports` it is invalid.
                    if is_imports && !text.starts_with("../") && !text.starts_with('/') {
                        let replaced = match pattern_match {
                            Some(m) => text.replace('*', m),
                            None => text.clone(),
                        };
                        return match self.resolve_bare(&package.dir, &replaced, kind)? {
                            Some(path) => Ok(Target::Found(path)),
                            None => Ok(Target::Undefined),
                        };
                    }
                    return Err(invalid(text));
                }
                let relative = &text[2..];
                if has_invalid_segment(relative) {
                    return Err(invalid(text));
                }
                let relative = match pattern_match {
                    Some(m) => {
                        if has_invalid_segment(m) {
                            return Err(invalid(text));
                        }
                        relative.replace('*', m)
                    }
                    None => relative.to_string(),
                };
                let path = normalize(&package.dir.join(&relative));
                if !path.starts_with(&package.dir) {
                    return Err(invalid(text));
                }
                Ok(Target::Found(path))
            }
            Json::Array(items) => {
                let mut last_error = None;
                for item in items {
                    match self.resolve_target(package, item, pattern_match, is_imports, kind) {
                        Ok(Target::Found(path)) => return Ok(Target::Found(path)),
                        Ok(Target::Undefined) => {}
                        Ok(Target::Null) => return Ok(Target::Null),
                        Err(e) => last_error = Some(e),
                    }
                }
                match last_error {
                    Some(e) => Err(e),
                    None => Ok(Target::Undefined),
                }
            }
            Json::Object(entries) => {
                for (condition, value) in entries {
                    if self.is_condition(condition, kind) {
                        match self.resolve_target(package, value, pattern_match, is_imports, kind)? {
                            Target::Undefined => continue,
                            other => return Ok(other),
                        }
                    }
                }
                Ok(Target::Undefined)
            }
            Json::Null => Ok(Target::Null),
            _ => Err(invalid("<non-string target>")),
        }
    }
}

impl Package {
    fn from_json(dir: &Path, json: Json) -> Package {
        let side_effects = match json.get("sideEffects") {
            Some(Json::Bool(false)) => SideEffects::None,
            Some(Json::Array(items)) => SideEffects::Files(items.iter().filter_map(|i| i.as_str().map(String::from)).collect()),
            _ => SideEffects::All,
        };
        Package {
            dir: dir.to_path_buf(),
            name: json.get("name").and_then(Json::as_str).map(String::from),
            version: json.get("version").and_then(Json::as_str).map(String::from),
            is_module: json.get("type").and_then(Json::as_str) == Some("module"),
            side_effects,
            json,
        }
    }
}

fn is_relative(specifier: &str) -> bool {
    specifier == "." || specifier == ".." || specifier.starts_with("./") || specifier.starts_with("../")
}

/// `@scope/name/sub/path` into (`@scope/name`, `./sub/path`), and `name` into (`name`, `.`).
fn split_package_name(specifier: &str) -> Option<(&str, String)> {
    let name_end = if specifier.starts_with('@') {
        let slash = specifier.find('/')?;
        match specifier[slash + 1..].find('/') {
            Some(i) => slash + 1 + i,
            None => specifier.len(),
        }
    } else {
        specifier.find('/').unwrap_or(specifier.len())
    };
    let name = &specifier[..name_end];
    let valid = !name.is_empty() && !name.starts_with('.') && !name.contains('\\') && !name.contains('%') && name != "node_modules";
    if !valid || (name.starts_with('@') && !name.contains('/')) {
        return None;
    }
    let rest = &specifier[name_end..];
    Some((name, if rest.is_empty() { ".".to_string() } else { format!(".{rest}") }))
}

/// A target segment that would climb out of the package or reach into its dependencies.
fn has_invalid_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|s| s == ".." || s == "." || s.eq_ignore_ascii_case("node_modules"))
}

/// Resolves `.` and `..` lexically. Symlinks are handled separately by `canonicalize`.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                // `..` at the root stays at the root; at the start of a relative path it is kept.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Matches a `sideEffects` glob against a package-relative path: `*` any run inside one
/// segment, `**` any number of segments, `?` one character, and `{a,b}` alternatives. A
/// pattern with no `/` matches the file name at any depth, as bundlers treat it.
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    for alternative in expand_braces(pattern) {
        let anywhere = !alternative.contains('/');
        let target = if anywhere { path.rsplit('/').next().unwrap_or(path) } else { path };
        if match_glob(alternative.as_bytes(), target.as_bytes()) {
            return true;
        }
    }
    false
}

fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else { return vec![pattern.to_string()] };
    let Some(close) = pattern[open..].find('}').map(|i| open + i) else { return vec![pattern.to_string()] };
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close].split(',').flat_map(|alt| expand_braces(&format!("{head}{alt}{tail}"))).collect()
}

fn match_glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            if let Some(after) = rest.strip_prefix(b"*") {
                // `**/` also matches zero directories.
                let after = after.strip_prefix(b"/").unwrap_or(after);
                return (0..=text.len()).any(|i| match_glob(after, &text[i..]));
            }
            (0..=text.len()).take_while(|&i| i == 0 || text[i - 1] != b'/').any(|i| match_glob(rest, &text[i..]))
        }
        Some((b'?', rest)) => text.first().is_some_and(|&c| c != b'/') && match_glob(rest, &text[1..]),
        Some((&c, rest)) => text.first() == Some(&c) && match_glob(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests;
