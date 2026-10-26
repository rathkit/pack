//! `tsconfig.json`: the parts of it a bundler acts on.
//!
//! `paths` and `baseUrl` change how imports resolve, `jsx` and its siblings say how JSX is
//! compiled, and `extends` pulls in a parent configuration. Type-checking options are ignored.

use std::path::{Path, PathBuf};

use crate::graph::GraphOptions;
use crate::json::Json;
use crate::resolve::{Alias, FileSystem, normalize};

#[derive(Clone, Debug, Default)]
pub struct TsConfig {
    /// The directory of the file that was asked for.
    pub dir: PathBuf,
    pub base_url: Option<PathBuf>,
    /// `paths`, with the directory they are relative to when there is no `baseUrl`.
    pub paths: Vec<(String, Vec<String>)>,
    paths_dir: PathBuf,
    pub jsx: Option<String>,
    pub jsx_import_source: Option<String>,
    pub jsx_factory: Option<String>,
    pub jsx_fragment_factory: Option<String>,
    pub experimental_decorators: bool,
    pub emit_decorator_metadata: bool,
    pub strict_null_checks: Option<bool>,
    pub strict: bool,
}

impl TsConfig {
    /// The `tsconfig.json` nearest to `start` (in it or a parent directory), if any.
    pub fn find(fs: &dyn FileSystem, start: &Path) -> Option<TsConfig> {
        let mut dir = Some(start);
        while let Some(d) = dir {
            let file = d.join("tsconfig.json");
            if fs.is_file(&file) {
                return TsConfig::load(fs, &file);
            }
            dir = d.parent();
        }
        None
    }

    /// Reads a configuration file and the ones it extends.
    pub fn load(fs: &dyn FileSystem, file: &Path) -> Option<TsConfig> {
        let mut seen = Vec::new();
        let mut config = TsConfig::load_chain(fs, file, &mut seen)?;
        config.dir = file.parent().unwrap_or(Path::new("/")).to_path_buf();
        Some(config)
    }

    fn load_chain(fs: &dyn FileSystem, file: &Path, seen: &mut Vec<PathBuf>) -> Option<TsConfig> {
        if seen.iter().any(|f| f == file) || seen.len() > 16 {
            return None; // a cycle in `extends`
        }
        seen.push(file.to_path_buf());
        let text = fs.read_to_string(file).ok()?;
        let json = Json::parse_jsonc(&text).ok()?;
        let dir = file.parent().unwrap_or(Path::new("/"));
        Some(TsConfig::from_json(fs, &json, dir, seen))
    }

    /// A configuration given as text (`--tsconfig-raw`), as if it were the file `tsconfig.json` in `dir`.
    pub fn raw(fs: &dyn FileSystem, text: &str, dir: &Path) -> Result<TsConfig, String> {
        let json = Json::parse_jsonc(text).map_err(|e| format!("not JSON: {e}"))?;
        let mut seen = Vec::new();
        let mut config = TsConfig::from_json(fs, &json, dir, &mut seen);
        config.dir = dir.to_path_buf();
        Ok(config)
    }

    fn from_json(fs: &dyn FileSystem, json: &Json, dir: &Path, seen: &mut Vec<PathBuf>) -> TsConfig {
        // Parents first, so that the file's own settings override them.
        let mut config = TsConfig::default();
        let parents: Vec<&str> = match json.get("extends") {
            Some(Json::String(s)) => vec![s.as_str()],
            Some(Json::Array(items)) => items.iter().filter_map(Json::as_str).collect(),
            _ => Vec::new(),
        };
        for parent in parents {
            if let Some(path) = locate_parent(fs, dir, parent) {
                if let Some(base) = TsConfig::load_chain(fs, &path, seen) {
                    config.merge(base);
                }
            }
        }
        if let Some(options) = json.get("compilerOptions") {
            let text_of = |key: &str| options.get(key).and_then(Json::as_str).map(String::from);
            if let Some(base) = text_of("baseUrl") {
                config.base_url = Some(normalize(&dir.join(base)));
            }
            if let Some(Json::Object(entries)) = options.get("paths") {
                config.paths = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_array().unwrap_or(&[]).iter().filter_map(Json::as_str).map(String::from).collect()))
                    .collect();
                config.paths_dir = dir.to_path_buf();
            }
            config.jsx = text_of("jsx").or(config.jsx);
            config.jsx_import_source = text_of("jsxImportSource").or(config.jsx_import_source);
            config.jsx_factory = text_of("jsxFactory").or(config.jsx_factory);
            config.jsx_fragment_factory = text_of("jsxFragmentFactory").or(config.jsx_fragment_factory);
            if let Some(b) = options.get("experimentalDecorators").and_then(Json::as_bool) {
                config.experimental_decorators = b;
            }
            if let Some(b) = options.get("emitDecoratorMetadata").and_then(Json::as_bool) {
                config.emit_decorator_metadata = b;
            }
            if let Some(b) = options.get("strictNullChecks").and_then(Json::as_bool) {
                config.strict_null_checks = Some(b);
            }
            if let Some(b) = options.get("strict").and_then(Json::as_bool) {
                config.strict = b;
            }
        }
        config
    }

    /// Takes a parent's settings; the ones set here afterwards override them.
    fn merge(&mut self, parent: TsConfig) {
        *self = TsConfig {
            dir: self.dir.clone(),
            base_url: parent.base_url.or(self.base_url.take()),
            paths: if parent.paths.is_empty() { std::mem::take(&mut self.paths) } else { parent.paths },
            paths_dir: if self.paths_dir.as_os_str().is_empty() { parent.paths_dir } else { self.paths_dir.clone() },
            jsx: parent.jsx.or(self.jsx.take()),
            jsx_import_source: parent.jsx_import_source.or(self.jsx_import_source.take()),
            jsx_factory: parent.jsx_factory.or(self.jsx_factory.take()),
            jsx_fragment_factory: parent.jsx_fragment_factory.or(self.jsx_fragment_factory.take()),
            experimental_decorators: parent.experimental_decorators || self.experimental_decorators,
            emit_decorator_metadata: parent.emit_decorator_metadata || self.emit_decorator_metadata,
            strict_null_checks: self.strict_null_checks.or(parent.strict_null_checks),
            strict: parent.strict || self.strict,
        };
    }

    /// Applies the settings to the options a build uses.
    pub fn apply(&self, options: &mut GraphOptions) {
        if self.base_url.is_some() {
            options.resolve.base_url = self.base_url.clone();
        }
        // `paths` are relative to `baseUrl`, or to the file that declares them without one.
        let base = self.base_url.clone().unwrap_or_else(|| self.paths_dir.clone());
        for (pattern, targets) in &self.paths {
            let replacements = targets.iter().map(|t| normalize(&base.join(t)).to_string_lossy().into_owned()).collect();
            options.resolve.alias.push(Alias { pattern: pattern.clone(), replacements, exact_only: true });
        }
        options.decorator_metadata |= self.emit_decorator_metadata;
        options.experimental_decorators |= self.experimental_decorators;
        options.strict_null_checks |= self.strict_null_checks.unwrap_or(self.strict);
        match self.jsx.as_deref() {
            Some("react") => options.jsx.automatic = false,
            Some("react-jsx") => options.jsx.automatic = true,
            Some("react-jsxdev") => {
                options.jsx.automatic = true;
                options.jsx.dev = true;
            }
            _ => {}
        }
        if let Some(source) = &self.jsx_import_source {
            options.jsx.import_source = source.clone();
        }
        if let Some(factory) = &self.jsx_factory {
            options.jsx.factory = factory.clone();
        }
        if let Some(fragment) = &self.jsx_fragment_factory {
            options.jsx.fragment = fragment.clone();
        }
    }
}

/// The file an `extends` entry names: a relative path, or a path inside a package.
fn locate_parent(fs: &dyn FileSystem, dir: &Path, spec: &str) -> Option<PathBuf> {
    let candidates = |base: PathBuf| -> Option<PathBuf> {
        [base.clone(), PathBuf::from(format!("{}.json", base.display())), base.join("tsconfig.json")].into_iter().find(|p| fs.is_file(p))
    };
    if spec.starts_with('.') || Path::new(spec).is_absolute() {
        return candidates(normalize(&dir.join(spec)));
    }
    let mut current = Some(dir);
    while let Some(d) = current {
        if let Some(found) = candidates(d.join("node_modules").join(spec)) {
            return Some(found);
        }
        current = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::MemFs;

    #[test]
    #[cfg(unix)] // in-memory files at /p/...: not absolute paths on Windows
    fn reads_options_with_comments_and_extends() {
        let fs = MemFs::new()
            .with("/p/tsconfig.base.json", r#"{ "compilerOptions": { "jsx": "react-jsx", "baseUrl": "./src", "paths": { "@a/*": ["a/*"] } } }"#)
            .with("/p/tsconfig.json", "{ // app\n \"extends\": \"./tsconfig.base\", \"compilerOptions\": { \"jsxImportSource\": \"preact\", \"experimentalDecorators\": true, }, }");
        let config = TsConfig::find(&fs, Path::new("/p/src/deep")).unwrap();
        assert_eq!(config.base_url.as_deref(), Some(Path::new("/p/src")));
        assert_eq!(config.paths, [("@a/*".to_string(), vec!["a/*".to_string()])]);
        assert_eq!((config.jsx.as_deref(), config.jsx_import_source.as_deref(), config.experimental_decorators), (Some("react-jsx"), Some("preact"), true));
        let mut options = GraphOptions::default();
        config.apply(&mut options);
        assert_eq!(options.jsx.import_source, "preact");
        assert_eq!(options.resolve.alias[0].replacements, ["/p/src/a/*"]);
    }

    #[test]
    #[cfg(unix)] // in-memory files at /p/...: not absolute paths on Windows
    fn paths_without_base_url_are_relative_to_the_file() {
        let fs = MemFs::new().with("/p/sub/tsconfig.json", r#"{ "compilerOptions": { "paths": { "~/*": ["./lib/*"] } } }"#);
        let config = TsConfig::find(&fs, Path::new("/p/sub")).unwrap();
        let mut options = GraphOptions::default();
        config.apply(&mut options);
        assert_eq!(options.resolve.alias[0].replacements, ["/p/sub/lib/*"]);
    }

    #[test]
    fn extends_from_a_package_and_cycles() {
        let fs = MemFs::new()
            .with("/p/node_modules/@cfg/base/tsconfig.json", r#"{ "compilerOptions": { "jsx": "react" } }"#)
            .with("/p/tsconfig.json", r#"{ "extends": "@cfg/base" }"#)
            .with("/q/tsconfig.json", r#"{ "extends": "./tsconfig.json" }"#);
        assert_eq!(TsConfig::find(&fs, Path::new("/p")).unwrap().jsx.as_deref(), Some("react"));
        assert!(TsConfig::find(&fs, Path::new("/q")).is_some(), "a cycle must not hang");
    }
}
