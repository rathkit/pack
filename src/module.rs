//! What a module says about its neighbors: the import and export tables the graph and the
//! linker work from.
//!
//! [`ModuleRecord::build`] reads a parsed file once and copies out everything that crosses a
//! module boundary as owned strings and symbol ids, so the record does not borrow the tree.
//! It is the ES module record from the specification (requested modules, import entries,
//! local/indirect/star export entries), plus the two things real code adds: dynamic
//! `import("x")` and CommonJS `require("x")` calls with a literal specifier.

use crate::ast::{Ast, NodeId, NodeKind, Span, flags};
use crate::define::Defines;
use crate::parser::Parsed;
use crate::semantic::{Semantic, SymbolId, SymbolKind};

/// How a module asks for another one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestKind {
    /// `import ... from "x"`, `export ... from "x"`, `import "x"`.
    Static,
    /// `import("x")`.
    Dynamic,
    /// `require("x")`.
    Require,
    /// `require.resolve("x")`: the path is wanted, not the module.
    Resolve,
    /// `new URL("./x.png", import.meta.url)`: a file copied to the output, whose new address replaces the string.
    AssetUrl,
}

/// One distinct `(specifier, kind)` a module asks for, in the order first seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub specifier: String,
    pub kind: RequestKind,
    /// Import attributes (`with { type: "json" }`), in source order.
    pub attrs: Vec<(String, String)>,
    /// Where the first such request appears, for error messages.
    pub span: Span,
}

/// What an `import` binds from the other module.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImportName {
    Default,
    Named(String),
    /// `import * as ns`.
    Namespace,
}

#[derive(Clone, Debug)]
pub struct ImportEntry {
    /// Index into [`ModuleRecord::requests`].
    pub request: usize,
    pub name: ImportName,
    /// The local binding the import creates.
    pub local: SymbolId,
    pub node: NodeId,
}

/// What an `export` makes visible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportSource {
    /// A binding of this module: `export const a`, `export { a }`, `export default function f`.
    Local(SymbolId),
    /// `export default <expression>` and `export default class {}`: a value with no name of
    /// its own, which the linker gives one.
    DefaultValue(NodeId),
    /// `export { a as b } from "x"`: another module's export, passed through.
    Indirect { request: usize, name: ImportName },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportEntry {
    /// The name other modules import it by.
    pub exported: String,
    pub source: ExportSource,
    pub node: NodeId,
}

/// A module's imports, exports and requests.
/// How JSX becomes JavaScript.
#[derive(Clone, Debug)]
pub struct JsxOptions {
    /// `_jsx("div", { children })` from `<import_source>/jsx-runtime`, as React 17+ expects.
    /// Otherwise `factory("div", props, ...children)`.
    pub automatic: bool,
    pub import_source: String,
    pub factory: String,
    pub fragment: String,
}

impl Default for JsxOptions {
    fn default() -> Self {
        JsxOptions { automatic: true, import_source: "react".to_string(), factory: "React.createElement".to_string(), fragment: "React.Fragment".to_string() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModuleRecord {
    pub requests: Vec<Request>,
    pub imports: Vec<ImportEntry>,
    pub exports: Vec<ExportEntry>,
    /// `export * from "x"`: indexes into `requests`.
    pub star_exports: Vec<usize>,
    /// The file has ES module syntax (`import`, `export`, or top-level `await`).
    pub is_esm: bool,
    /// The file uses `module`, `exports` or `require` freely, as CommonJS does.
    pub uses_commonjs_globals: bool,
    /// For JSX compiled to the automatic runtime: the request for `jsx-runtime`.
    pub jsx_runtime: Option<usize>,
}

impl ModuleRecord {
    /// Reads the import/export tables out of a parsed file.
    pub fn build(parsed: &Parsed) -> ModuleRecord {
        ModuleRecord::build_with(parsed, &JsxOptions::default(), &Defines::default())
    }

    pub fn build_with(parsed: &Parsed, jsx: &JsxOptions, defines: &Defines) -> ModuleRecord {
        let mut builder = Builder { ast: &parsed.ast, semantic: &parsed.semantic, is_ts: parsed.is_ts, jsx, defines, record: ModuleRecord::default() };
        builder.run();
        builder.record
    }

    pub fn request_index(&self, specifier: &str, kind: RequestKind) -> Option<usize> {
        self.requests.iter().position(|r| r.kind == kind && r.specifier == specifier)
    }

    /// Looks an export up by the name other modules use.
    pub fn export(&self, name: &str) -> Option<&ExportEntry> {
        self.exports.iter().find(|e| e.exported == name)
    }
}

struct Builder<'a> {
    ast: &'a Ast,
    semantic: &'a Semantic,
    is_ts: bool,
    jsx: &'a JsxOptions,
    defines: &'a Defines,
    record: ModuleRecord,
}

impl Builder<'_> {
    fn run(&mut self) {
        let NodeKind::Program(body) = self.ast[self.ast.root].kind else { return };
        for &item in self.ast.list(body) {
            match self.ast[item].kind {
                NodeKind::Import(import) => {
                    self.record.is_esm = true;
                    self.import_declaration(item, import);
                }
                NodeKind::ExportNamed(export) => {
                    self.record.is_esm = true;
                    self.export_named(item, export);
                }
                NodeKind::ExportAll { exported, source, attrs } => {
                    self.record.is_esm = true;
                    let request = self.request(source, RequestKind::Static, attrs, item);
                    match exported {
                        Some(name) => {
                            let exported = self.ast.atom(name).to_string();
                            self.record.exports.push(ExportEntry {
                                exported,
                                source: ExportSource::Indirect { request, name: ImportName::Namespace },
                                node: item,
                            });
                        }
                        None => self.record.star_exports.push(request),
                    }
                }
                NodeKind::ExportDefault(value) => {
                    self.record.is_esm = true;
                    self.export_default(item, value);
                }
                _ => {}
            }
        }
        self.scan_calls();
        if self.ast.uses_jsx && self.jsx.automatic {
            let specifier = format!("{}/jsx-runtime", self.jsx.import_source);
            let index = self.request_for(&specifier, RequestKind::Static, Vec::new(), Span { start: 0, end: 0 });
            self.record.jsx_runtime = Some(index);
        }
    }

    fn request(&mut self, source: crate::ast::Atom, kind: RequestKind, attrs: crate::ast::List, node: NodeId) -> usize {
        let specifier = self.ast.atom(source);
        let attrs: Vec<(String, String)> = self
            .ast
            .list(attrs)
            .iter()
            .filter_map(|&a| match self.ast[a].kind {
                NodeKind::ImportAttr { key, value } => Some((self.ast.atom(key).to_string(), self.ast.atom(value).to_string())),
                _ => None,
            })
            .collect();
        self.request_for(specifier, kind, attrs, self.ast[node].span)
    }

    fn request_for(&mut self, specifier: &str, kind: RequestKind, attrs: Vec<(String, String)>, span: Span) -> usize {
        let existing = self.record.requests.iter().position(|r| r.kind == kind && r.specifier == specifier && r.attrs == attrs);
        if let Some(index) = existing {
            return index;
        }
        self.record.requests.push(Request { specifier: specifier.to_string(), kind, attrs, span });
        self.record.requests.len() - 1
    }

    fn import_declaration(&mut self, node: NodeId, import: crate::ast::ImportId) {
        let data = *self.ast.import(import);
        // TypeScript drops an import whose names are only ever used as types. A bare
        // `import "m"` has no names and stays.
        let used = |this: &Self, local: NodeId| match this.semantic.symbol_of(local) {
            Some(sym) => {
                let s = this.semantic.symbol(sym);
                // The classic JSX factory is called by the output, not written in the source.
                let jsx_root = this.ast.uses_jsx && !this.jsx.automatic && {
                    let name = this.ast.atom(s.name);
                    [&this.jsx.factory, &this.jsx.fragment].iter().any(|path| path.split('.').next() == Some(name))
                };
                s.reads + s.writes > 0 || s.flags & crate::semantic::symbol_flags::EXPORTED != 0 || jsx_root
            }
            None => false,
        };
        if self.is_ts && !self.ast.list(data.specifiers).is_empty() {
            let any_used = self.ast.list(data.specifiers).iter().any(|&spec| match self.ast[spec].kind {
                NodeKind::ImportDefault(local) | NodeKind::ImportNamespace(local) | NodeKind::ImportSpecifier { local, .. } => used(self, local),
                _ => false,
            });
            if !any_used {
                return;
            }
        }
        let request = self.request(data.source, RequestKind::Static, data.attrs, node);
        for &spec in self.ast.list(data.specifiers) {
            let (name, local) = match self.ast[spec].kind {
                NodeKind::ImportDefault(local) => (ImportName::Default, local),
                NodeKind::ImportNamespace(local) => (ImportName::Namespace, local),
                NodeKind::ImportSpecifier { imported, local } => {
                    let imported = self.ast.atom(imported);
                    // `import { default as x }` is the default import written long-hand.
                    let name = if imported == "default" { ImportName::Default } else { ImportName::Named(imported.to_string()) };
                    (name, local)
                }
                _ => continue,
            };
            if self.is_ts && !used(self, local) {
                continue;
            }
            if let Some(sym) = self.semantic.symbol_of(local) {
                self.record.imports.push(ImportEntry { request, name, local: sym, node: spec });
            }
        }
    }

    fn export_named(&mut self, node: NodeId, export: crate::ast::ExportId) {
        let data = *self.ast.export(export);
        if let Some(decl) = data.decl {
            let mut idents = Vec::new();
            match self.ast[decl].kind {
                NodeKind::VarDecl { decls, .. } => {
                    for &d in self.ast.list(decls) {
                        if let NodeKind::VarDeclarator { target, .. } = self.ast[d].kind {
                            crate::semantic::binding_idents(self.ast, target, &mut idents);
                        }
                    }
                }
                NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } | NodeKind::TsEnum { name: n, .. } | NodeKind::TsNamespace { name: n, .. } => idents.push(n),
                _ => {}
            }
            for ident in idents {
                let NodeKind::BindingIdent(name) = self.ast[ident].kind else { continue };
                if let Some(sym) = self.semantic.symbol_of(ident) {
                    let exported = self.ast.atom(name).to_string();
                    self.record.exports.push(ExportEntry { exported, source: ExportSource::Local(sym), node });
                }
            }
            return;
        }
        let request = data.source.map(|source| self.request(source, RequestKind::Static, data.attrs, node));
        for &spec in self.ast.list(data.specifiers) {
            let NodeKind::ExportSpecifier { local, exported } = self.ast[spec].kind else { continue };
            let exported_name = self.ast.atom(exported).to_string();
            let source = match request {
                Some(request) => {
                    let imported = self.ast.atom(local);
                    let name = if imported == "default" { ImportName::Default } else { ImportName::Named(imported.to_string()) };
                    ExportSource::Indirect { request, name }
                }
                None => {
                    // `export { x }` where `x` is itself an import is a re-export in disguise,
                    // but the linker sees through that using the import table, so it stays local.
                    match self.semantic.lookup_local(self.semantic.root_scope(), local) {
                        Some(sym) => ExportSource::Local(sym),
                        None => continue,
                    }
                }
            };
            self.record.exports.push(ExportEntry { exported: exported_name, source, node: spec });
        }
    }

    fn export_default(&mut self, node: NodeId, value: NodeId) {
        // `export default function f() {}` and `export default class C {}` bind their name.
        let named = match self.ast[value].kind {
            NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } if self.ast[value].has(flags::DECL) => {
                self.semantic.symbol_of(n)
            }
            _ => None,
        };
        let source = match named {
            Some(sym) => ExportSource::Local(sym),
            None => ExportSource::DefaultValue(value),
        };
        self.record.exports.push(ExportEntry { exported: "default".to_string(), source, node });
    }

    /// Finds `import("x")` and `require("x")` anywhere in the file. Nodes sit in one flat
    /// array, so this is a linear scan with no recursion.
    fn scan_calls(&mut self) {
        let require = self.ast.find_atom("require");
        let globals = [self.ast.find_atom("module"), self.ast.find_atom("exports"), require];
        for &id in self.semantic.unresolved() {
            if let NodeKind::Ident(name) = self.ast[id].kind {
                if globals.contains(&Some(name)) {
                    self.record.uses_commonjs_globals = true;
                }
            }
        }
        // Code in a branch that never runs loads nothing.
        let mut dead: Vec<(u32, u32)> = (1..=self.ast.node_count())
            .filter(|_| !self.defines.is_empty())
            .filter_map(|index| self.defines.dead_branch(self.ast, self.semantic, self.ast.node_id(index)))
            .map(|d| (self.ast[d.node].span.start, self.ast[d.node].span.end))
            .collect();
        // Ranges inside others add nothing; what is left is apart, so one search finds the range around a node.
        dead.sort_unstable_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
        let mut outer: Vec<(u32, u32)> = Vec::new();
        for (start, end) in dead {
            if outer.last().is_none_or(|&(_, last_end)| start >= last_end) {
                outer.push((start, end));
            }
        }
        let dead = outer;
        let is_dead = |span: Span| {
            let at = dead.partition_point(|&(start, _)| start <= span.start);
            at > 0 && span.end <= dead[at - 1].1
        };
        for index in 1..=self.ast.node_count() {
            let id = self.ast.node_id(index);
            if !dead.is_empty() && is_dead(self.ast[id].span) {
                continue;
            }
            match self.ast[id].kind {
                NodeKind::ImportCall { source, .. } => {
                    if let NodeKind::Str(s) = self.ast[source].kind {
                        let specifier = self.ast.atom(s);
                        self.request_for(specifier, RequestKind::Dynamic, Vec::new(), self.ast[id].span);
                    }
                }
                NodeKind::New { callee, args } => {
                    // `new URL("./file", import.meta.url)`
                    let list = self.ast.list(args);
                    let is_url = matches!(self.ast[callee].kind, NodeKind::Ident(n) if self.ast.atom(n) == "URL") && self.semantic.symbol_of(callee).is_none();
                    if let (true, [first, second]) = (is_url, list) {
                        let is_meta_url = matches!(self.ast[*second].kind, NodeKind::Member { object, prop }
                            if matches!(self.ast[object].kind, NodeKind::ImportMeta)
                                && matches!(self.ast[prop].kind, NodeKind::PropName(p) if self.ast.atom(p) == "url")
                                && !self.ast[*second].has(flags::COMPUTED));
                        if let (true, NodeKind::Str(s)) = (is_meta_url, self.ast[*first].kind) {
                            let specifier = self.ast.atom(s);
                            if specifier.starts_with("./") || specifier.starts_with("../") {
                                self.request_for(specifier, RequestKind::AssetUrl, Vec::new(), self.ast[*first].span);
                            }
                        }
                    }
                }
                NodeKind::Call { callee, args } => {
                    // `require.resolve("x")`
                    if let NodeKind::Member { object, prop } = self.ast[callee].kind {
                        let is_resolve = matches!(self.ast[prop].kind, NodeKind::PropName(p) if self.ast.atom(p) == "resolve")
                            && matches!(self.ast[object].kind, NodeKind::Ident(n) if Some(n) == require)
                            && self.semantic.symbol_of(object).is_none()
                            && !self.ast[callee].has(flags::COMPUTED);
                        if let (true, [arg]) = (is_resolve, self.ast.list(args)) {
                            if let NodeKind::Str(s) = self.ast[*arg].kind {
                                let specifier = self.ast.atom(s);
                                self.request_for(specifier, RequestKind::Resolve, Vec::new(), self.ast[id].span);
                            }
                        }
                        continue;
                    }
                    let NodeKind::Ident(name) = self.ast[callee].kind else { continue };
                    // The real `require` is unbound. A parameter named `require` is the UMD pattern
                    // `factory(require, exports)` and counts too; any other local called `require`
                    // is some unrelated function.
                    let local = self.semantic.symbol_of(callee);
                    let is_param = local.is_some_and(|s| self.semantic.symbol(s).kind == SymbolKind::Param);
                    if Some(name) != require || (local.is_some() && !is_param) {
                        continue;
                    }
                    let list = self.ast.list(args);
                    if let [arg] = list {
                        if let NodeKind::Str(s) = self.ast[*arg].kind {
                            let specifier = self.ast.atom(s);
                            self.request_for(specifier, RequestKind::Require, Vec::new(), self.ast[id].span);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
