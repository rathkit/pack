use super::*;
use crate::ast::{Atom, ExportData, ImportData, VarKind};
use crate::semantic::ScopeKind;

impl<'a> Parser<'a> {
    // ---- module items -------------------------------------------------------------------

    /// A top-level item: any statement, plus `import` and `export` declarations.
    pub(super) fn parse_module_item(&mut self) -> PResult<NodeId> {
        if self.ts && self.at(Punct::At) {
            // Decorators that come before `export`: they belong to the class after it.
            self.pending_decorators = self.parse_decorators()?;
        }
        if self.at_kw(Kw::Import) {
            let next = self.peek()?;
            if !matches!(next.kind, Kind::Punct(Punct::LParen | Punct::Dot)) {
                return self.parse_import_decl();
            }
        }
        if self.at_kw(Kw::Export) {
            return self.parse_export_decl();
        }
        self.parse_statement_list_item()
    }

    fn require_module(&self, what: &str) -> PResult<()> {
        if self.module {
            Ok(())
        } else {
            Err(self.error(format!("Cannot use `{what}` outside a module")))
        }
    }

    fn parse_module_source(&mut self) -> PResult<Atom> {
        if self.tok.kind != Kind::String {
            return Err(self.unexpected());
        }
        let source = self.string_atom(self.tok)?;
        self.next()?;
        Ok(source)
    }

    /// An imported or exported name: an identifier name or a string literal.
    fn parse_module_export_name(&mut self) -> PResult<Atom> {
        let tok = self.tok;
        match tok.kind {
            Kind::Ident | Kind::Keyword(_) => {
                let name = self.name_of(tok);
                self.next()?;
                Ok(name)
            }
            Kind::String => {
                let name = self.string_atom(tok)?;
                self.next()?;
                Ok(name)
            }
            _ => Err(self.unexpected()),
        }
    }

    fn parse_import_attributes(&mut self) -> PResult<List> {
        if !self.at_kw(Kw::With) {
            return Ok(List::default());
        }
        self.next()?;
        self.expect(Punct::LBrace)?;
        let mark = self.mark();
        let mut keys: Vec<Atom> = Vec::new();
        while !self.at(Punct::RBrace) {
            let start = self.tok.start;
            let key = self.parse_module_export_name()?;
            if keys.contains(&key) {
                return Err(self.error_at(format!("Duplicate import attribute `{}`", self.ast.atom(key)), start));
            }
            keys.push(key);
            self.expect(Punct::Colon)?;
            let value = self.parse_module_source()?;
            let attr = self.finish(NodeKind::ImportAttr { key, value }, start);
            self.push(attr);
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        self.expect(Punct::RBrace)?;
        Ok(self.take_list(mark))
    }

    fn parse_import_decl(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.require_module("import")?;
        self.next()?;
        if self.ts && self.ts_import_equals_ahead()? {
            return self.parse_ts_import_equals(start);
        }
        let mark = self.mark();
        // `import type ...` exists only for the type checker: it binds nothing.
        let type_only = self.ts && self.ts_import_type_keyword()?;
        // Every binding in an import declaration is an import binding.
        self.binding_ctx = if type_only { BindingCtx::None } else { BindingCtx::Import };
        if self.tok.kind != Kind::String {
            if self.tok.kind == Kind::Ident {
                let spec_start = self.tok.start;
                let local = self.parse_binding_ident()?;
                let spec = self.finish(NodeKind::ImportDefault(local), spec_start);
                self.push(spec);
                if !self.eat(Punct::Comma)? {
                    self.expect_word("from")?;
                    return self.finish_import_ts(start, mark, type_only);
                }
            }
            if self.at(Punct::Star) {
                let spec_start = self.tok.start;
                self.next()?;
                self.expect_word("as")?;
                let local = self.parse_binding_ident()?;
                let spec = self.finish(NodeKind::ImportNamespace(local), spec_start);
                self.push(spec);
            } else if self.at(Punct::LBrace) {
                self.next()?;
                while !self.at(Punct::RBrace) {
                    let spec_start = self.tok.start;
                    if self.ts && !type_only && self.at_word("type") && self.ts_type_specifier()? {
                        // `{ type A }`: left out, with its comma.
                        self.next()?;
                        let saved = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
                        let result = self.parse_module_export_name().and_then(|_| {
                            if self.at_word("as") {
                                self.next()?;
                                self.parse_binding_ident()?;
                            }
                            Ok(())
                        });
                        self.binding_ctx = saved;
                        result?;
                        let more = self.eat(Punct::Comma)?;
                        self.ts_erase_from(spec_start);
                        if !more {
                            break;
                        }
                        continue;
                    }
                    let name_tok = self.tok;
                    let imported = self.parse_module_export_name()?;
                    let local = if self.at_word("as") {
                        self.next()?;
                        self.parse_binding_ident()?
                    } else {
                        // `import { a }` binds `a`, so it must be a plain identifier.
                        if name_tok.kind != Kind::Ident {
                            return Err(self.error_at("Expected `as` after this import name", name_tok.start));
                        }
                        self.check_binding_name(imported, name_tok.start)?;
                        let local =
                            self.ast.alloc(NodeKind::BindingIdent(imported), Span { start: name_tok.start, end: name_tok.end }, 0);
                        self.declare_binding(imported, local)?;
                        local
                    };
                    let spec = self.finish(NodeKind::ImportSpecifier { imported, local }, spec_start);
                    self.push(spec);
                    if !self.eat(Punct::Comma)? {
                        break;
                    }
                }
                self.expect(Punct::RBrace)?;
            } else {
                return Err(self.unexpected());
            }
            self.expect_word("from")?;
        }
        self.finish_import_ts(start, mark, type_only)
    }

    fn finish_import_ts(&mut self, start: u32, mark: usize, type_only: bool) -> PResult<NodeId> {
        if !type_only {
            return self.finish_import(start, mark);
        }
        self.binding_ctx = BindingCtx::None;
        self.parse_module_source()?;
        self.parse_import_attributes()?;
        self.semicolon()?;
        self.scratch.truncate(mark);
        Ok(self.ts_empty_statement(start))
    }

    /// Cursor is at the module specifier, just past `from` if there was one.
    fn finish_import(&mut self, start: u32, mark: usize) -> PResult<NodeId> {
        self.binding_ctx = BindingCtx::None;
        let source = self.parse_module_source()?;
        let attrs = self.parse_import_attributes()?;
        self.semicolon()?;
        let specifiers = self.take_list(mark);
        let data = self.ast.alloc_import(ImportData { specifiers, source, attrs });
        Ok(self.finish(NodeKind::Import(data), start))
    }

    fn parse_export_decl(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.require_module("export")?;
        self.next()?;
        if self.ts && self.tok.kind == Kind::Ident && !self.tok.escaped {
            let next = self.peek()?;
            match self.text(self.tok) {
                // `export type { A }` and `export type * from "m"` re-export types only.
                "type" if matches!(next.kind, Kind::Punct(Punct::LBrace | Punct::Star)) => {
                    self.next()?;
                    if self.at(Punct::LBrace) {
                        self.ts_balanced()?;
                    } else {
                        self.next()?;
                        if self.at_word("as") {
                            self.next()?;
                            self.parse_module_export_name()?;
                        }
                    }
                    if self.at_word("from") {
                        self.next()?;
                        self.parse_module_source()?;
                        self.parse_import_attributes()?;
                    }
                    self.semicolon()?;
                    return Ok(self.ts_empty_statement(start));
                }
                "type" | "interface" | "declare" if !next.newline_before => {
                    if let Some(statement) = self.ts_statement()? {
                        self.ts_erase_from(start);
                        return Ok(statement);
                    }
                }
                _ => {}
            }
        }
        if self.ts {
            if self.at(Punct::Eq) {
                // `export = value;` is `module.exports = value;`.
                let eq = self.tok.end;
                self.next()?;
                let value = self.parse_assign()?;
                self.semicolon()?;
                self.ast.ts_commonjs = true;
                self.ast.rewrite(start, eq, "module.exports =");
                return Ok(self.finish(NodeKind::ExprStmt(value), start));
            }
            let as_namespace = self.at_word("as") && {
                let next = self.peek()?;
                next.kind == Kind::Ident && self.text(next) == "namespace"
            };
            if as_namespace {
                // `export as namespace Name;` names a global for scripts that load the bundle.
                self.next()?;
                self.next()?;
                self.next()?;
                self.semicolon()?;
                return Ok(self.ts_empty_statement(start));
            }
            if self.at_kw(Kw::Import) {
                // `export import a = b.c;`
                let import_start = self.tok.start;
                self.next()?;
                let decl = self.parse_ts_import_equals(import_start)?;
                let none = List::default();
                let data = self.ast.alloc_export(ExportData { specifiers: none, source: None, decl: Some(decl), attrs: none });
                if let NodeKind::VarDecl { decls, .. } = self.ast[decl].kind {
                    if let NodeKind::VarDeclarator { target, .. } = self.ast[self.ast.list(decls)[0]].kind {
                        let mut names = Vec::new();
                        self.bound_names(target, &mut names);
                        for name in names {
                            self.add_export(name, import_start)?;
                        }
                    }
                }
                self.sb.note_export_decl(decl);
                return Ok(self.finish(NodeKind::ExportNamed(data), start));
            }
        }
        match self.tok.kind {
            Kind::Punct(Punct::Star) => {
                self.next()?;
                let exported = if self.at_word("as") {
                    let name_pos = self.tok.start;
                    self.next()?;
                    let name = self.parse_module_export_name()?;
                    self.add_export(name, name_pos)?;
                    Some(name)
                } else {
                    None
                };
                self.expect_word("from")?;
                let source = self.parse_module_source()?;
                let attrs = self.parse_import_attributes()?;
                self.semicolon()?;
                Ok(self.finish(NodeKind::ExportAll { exported, source, attrs }, start))
            }
            Kind::Punct(Punct::LBrace) => {
                self.next()?;
                let mark = self.mark();
                // Without `from`, each local name must be a real binding reference, so a
                // reserved word or a string is not allowed there.
                let mut bad_local: Option<(Kind, u32)> = None;
                while !self.at(Punct::RBrace) {
                    let spec_start = self.tok.start;
                    if self.ts && self.at_word("type") && self.ts_type_specifier()? {
                        // `{ type A }`: left out, with its comma.
                        self.next()?;
                        self.parse_module_export_name()?;
                        if self.at_word("as") {
                            self.next()?;
                            self.parse_module_export_name()?;
                        }
                        let more = self.eat(Punct::Comma)?;
                        self.ts_erase_from(spec_start);
                        if !more {
                            break;
                        }
                        continue;
                    }
                    if matches!(self.tok.kind, Kind::Keyword(_) | Kind::String) && bad_local.is_none() {
                        bad_local = Some((self.tok.kind, self.tok.start));
                    }
                    let local = self.parse_module_export_name()?;
                    let exported = if self.at_word("as") {
                        self.next()?;
                        self.parse_module_export_name()?
                    } else {
                        local
                    };
                    self.add_export(exported, spec_start)?;
                    let spec = self.finish(NodeKind::ExportSpecifier { local, exported }, spec_start);
                    self.push(spec);
                    if !self.eat(Punct::Comma)? {
                        break;
                    }
                }
                self.expect(Punct::RBrace)?;
                let specifiers = self.take_list(mark);
                let (source, attrs) = if self.at_word("from") {
                    self.next()?;
                    let source = self.parse_module_source()?;
                    (Some(source), self.parse_import_attributes()?)
                } else {
                    if let Some((kind, pos)) = bad_local {
                        let message = if kind == Kind::String {
                            "String literal export names must be followed by a `from` clause"
                        } else {
                            "A reserved word cannot be exported without `from`"
                        };
                        return Err(self.error_at(message, pos));
                    }
                    (None, List::default())
                };
                self.semicolon()?;
                let local_names = source.is_none();
                let data = self.ast.alloc_export(ExportData { specifiers, source, decl: None, attrs });
                let node = self.finish(NodeKind::ExportNamed(data), start);
                if local_names {
                    self.sb.note_export_list(node);
                }
                Ok(node)
            }
            Kind::Keyword(Kw::Default) => {
                self.next()?;
                if self.ts && self.at_word("interface") && self.peek()?.kind == Kind::Ident {
                    self.ts_statement()?;
                    return Ok(self.ts_empty_statement(start));
                }
                if self.ts && self.at_word("abstract") && self.peek()?.kind == Kind::Keyword(Kw::Class) {
                    let abstract_start = self.tok.start;
                    self.next()?;
                    self.ts_erase_from(abstract_start);
                }
                let decl_start = self.tok.start;
                let default_name = self.atom("default");
                self.add_export(default_name, decl_start)?;
                let value = if self.at_kw(Kw::Function) {
                    self.parse_function(decl_start, false, true, false)?
                } else if self.at_kw(Kw::Class) {
                    self.parse_class(decl_start, true, false)?
                } else if self.at_word("async") && self.async_function_follows()? {
                    self.next()?;
                    self.parse_function(decl_start, true, true, false)?
                } else {
                    let expr = self.parse_assign()?;
                    self.semicolon()?;
                    expr
                };
                // `export default function f() {}` exports the declared `f`; an expression or
                // an anonymous declaration binds nothing.
                if self.ast[value].has(flags::DECL) {
                    self.sb.note_export_decl(value);
                }
                Ok(self.finish(NodeKind::ExportDefault(value), start))
            }
            _ => {
                let decl = self.parse_statement_list_item()?;
                if self.ts && matches!(self.ast[decl].kind, NodeKind::Empty) {
                    // An overload signature, or a declaration of types only.
                    return Ok(self.ts_empty_statement(start));
                }
                let declares = match self.ast[decl].kind {
                    NodeKind::VarDecl { .. } | NodeKind::Function { .. } | NodeKind::Class { .. } | NodeKind::TsEnum { .. } | NodeKind::TsNamespace { .. } => true,
                    _ => false,
                };
                if !declares {
                    return Err(self.error_at("Expected a declaration after `export`", self.ast[decl].span.start));
                }
                // Everything the declaration binds is now exported.
                let mut names = Vec::new();
                match self.ast[decl].kind {
                    NodeKind::VarDecl { decls, .. } => {
                        for &d in self.ast.list(decls) {
                            if let NodeKind::VarDeclarator { target, .. } = self.ast[d].kind {
                                self.bound_names(target, &mut names);
                            }
                        }
                    }
                    NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } | NodeKind::TsEnum { name: n, .. } | NodeKind::TsNamespace { name: n, .. } => {
                        self.bound_names(n, &mut names)
                    }
                    _ => {}
                }
                for name in names {
                    // Merged enums and namespaces export their name once.
                    if self.ts && matches!(self.ast[decl].kind, NodeKind::TsEnum { .. } | NodeKind::TsNamespace { .. }) && self.exported.contains(&name) {
                        continue;
                    }
                    // A namespace that holds only types shares its name with a value, in either
                    // order (`export namespace fn {}` and `export const fn = ...`).
                    if self.ts && self.exported_namespaces.contains(&name) && self.exported.contains(&name) {
                        continue;
                    }
                    if self.ts && matches!(self.ast[decl].kind, NodeKind::TsNamespace { .. }) {
                        self.exported_namespaces.insert(name);
                    }
                    self.add_export(name, self.ast[decl].span.start)?;
                }
                let none = List::default();
                let data = self.ast.alloc_export(ExportData { specifiers: none, source: None, decl: Some(decl), attrs: none });
                self.sb.note_export_decl(decl);
                Ok(self.finish(NodeKind::ExportNamed(data), start))
            }
        }
    }

    fn add_export(&mut self, name: Atom, pos: u32) -> PResult<()> {
        if !self.exported.insert(name) {
            return Err(self.error_at(format!("Duplicate export of `{}`", self.ast.atom(name)), pos));
        }
        Ok(())
    }

    fn async_function_follows(&mut self) -> PResult<bool> {
        let next = self.peek()?;
        Ok(next.kind == Kind::Keyword(Kw::Function) && !next.newline_before && !next.escaped)
    }

    // ---- statements ---------------------------------------------------------------------

    /// A statement or a declaration.
    pub(super) fn parse_statement_list_item(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        // These look ahead, which a match guard cannot do, so decide before matching.
        let is_ident = self.tok.kind == Kind::Ident;
        let is_let_decl = is_ident && self.at_word("let") && self.let_starts_declaration()?;
        let is_async_fn = is_ident && self.at_word("async") && self.async_function_follows()?;
        if self.ts && self.at(Punct::At) {
            self.pending_decorators = self.parse_decorators()?;
        }
        let is_ident = self.tok.kind == Kind::Ident;
        if self.ts && is_ident {
            if let Some(statement) = self.ts_statement()? {
                return Ok(statement);
            }
        }
        if self.ts && self.at_kw(Kw::Enum) {
            return self.parse_ts_enum(start, false);
        }
        if self.ts && self.at_kw(Kw::Const) && self.peek()?.kind == Kind::Keyword(Kw::Enum) {
            self.next()?;
            return self.parse_ts_enum(start, true);
        }
        match self.tok.kind {
            Kind::Keyword(Kw::Function) if !self.tok.escaped => self.parse_function(start, false, true, true),
            Kind::Keyword(Kw::Class) if !self.tok.escaped => self.parse_class(start, true, true),
            Kind::Keyword(Kw::Const) if !self.tok.escaped => self.parse_var_statement(VarKind::Const),
            _ if is_let_decl => self.parse_var_statement(VarKind::Let),
            _ if is_async_fn => {
                self.next()?;
                self.parse_function(start, true, true, true)
            }
            _ => self.parse_statement(),
        }
    }

    /// `let` begins a declaration when a binding follows; otherwise (sloppy mode only) it
    /// is an ordinary identifier.
    fn let_starts_declaration(&mut self) -> PResult<bool> {
        let next = self.peek()?;
        Ok(matches!(next.kind, Kind::Ident | Kind::Punct(Punct::LBracket | Punct::LBrace)))
    }

    pub(super) fn parse_statement(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        match self.tok.kind {
            Kind::Punct(Punct::LBrace) => self.parse_block(),
            Kind::Punct(Punct::Semi) => {
                self.next()?;
                Ok(self.finish(NodeKind::Empty, start))
            }
            Kind::Keyword(kw) if !self.tok.escaped => match kw {
                Kw::Var => self.parse_var_statement(VarKind::Var),
                Kw::If => self.parse_if(),
                Kw::For => self.parse_for(),
                Kw::While => self.parse_while(),
                Kw::Do => self.parse_do_while(),
                Kw::Continue | Kw::Break => self.parse_jump(kw),
                Kw::Return => self.parse_return(),
                Kw::With => self.parse_with(),
                Kw::Switch => self.parse_switch(),
                Kw::Throw => self.parse_throw(),
                Kw::Try => self.parse_try(),
                Kw::Debugger => {
                    self.next()?;
                    self.semicolon()?;
                    Ok(self.finish(NodeKind::Debugger, start))
                }
                Kw::Function | Kw::Class | Kw::Const => Err(self.error("Declarations are not allowed in this position")),
                Kw::Import => {
                    let next = self.peek()?;
                    if matches!(next.kind, Kind::Punct(Punct::LParen | Punct::Dot)) {
                        self.parse_expression_statement()
                    } else {
                        Err(self.error("`import` declarations may only appear at the top level of a module"))
                    }
                }
                Kw::Export => Err(self.error("`export` declarations may only appear at the top level of a module")),
                _ => self.parse_expression_statement(),
            },
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_expression_statement(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let expr = self.parse_expression()?;
        let node = self.ast[expr];
        if let NodeKind::Ident(label) = node.kind {
            if !node.parenthesized() && self.at(Punct::Colon) {
                // A label names no variable, so it is not a reference after all.
                self.sb.forget_ref(expr);
                return self.parse_labeled(start, label);
            }
        }
        self.semicolon()?;
        Ok(self.finish(NodeKind::ExprStmt(expr), start))
    }

    fn parse_labeled(&mut self, start: u32, label: Atom) -> PResult<NodeId> {
        if self.cx.labels.contains(&label) {
            return Err(self.error_at(format!("Label `{}` is already declared", self.ast.atom(label)), start));
        }
        self.next()?; // `:`
        self.cx.labels.push(label);
        let body = if self.at_kw(Kw::Function) && !self.strict {
            self.parse_statement_list_item()
        } else {
            self.parse_statement()
        };
        self.cx.labels.pop();
        let body = body?;
        Ok(self.finish(NodeKind::Labeled { label, body }, start))
    }

    pub(super) fn parse_block(&mut self) -> PResult<NodeId> {
        self.parse_block_scoped(true)
    }

    /// A `{ ... }` block. A `catch` body passes `scoped: false` because it shares the scope of
    /// the catch clause (where its parameter lives).
    fn parse_block_scoped(&mut self, scoped: bool) -> PResult<NodeId> {
        let start = self.tok.start;
        self.expect(Punct::LBrace)?;
        if scoped {
            self.push_scope(ScopeKind::Block);
        }
        let mark = self.mark();
        while !self.at(Punct::RBrace) {
            if self.tok.kind == Kind::Eof {
                return Err(self.unexpected_expecting("}"));
            }
            let stmt = self.parse_statement_list_item()?;
            self.push(stmt);
        }
        self.expect(Punct::RBrace)?;
        let list = self.take_list(mark);
        let node = self.finish(NodeKind::Block(list), start);
        if scoped {
            self.sb.pop(node);
        }
        Ok(node)
    }

    // ---- declarations -------------------------------------------------------------------

    fn parse_var_statement(&mut self, kind: VarKind) -> PResult<NodeId> {
        let start = self.tok.start;
        let decl = self.parse_var_decl(kind, start)?;
        self.check_initializers(decl)?;
        self.semicolon()?;
        self.ast.node_mut(decl).span.end = self.prev_end;
        Ok(decl)
    }

    /// Cursor on `var`, `let` or `const`. Does not consume the terminator, so a `for` head
    /// can use it too.
    fn parse_var_decl(&mut self, kind: VarKind, start: u32) -> PResult<NodeId> {
        self.next()?;
        let mark = self.mark();
        let ctx = match kind {
            VarKind::Var => BindingCtx::Var,
            VarKind::Let => BindingCtx::Let,
            VarKind::Const => BindingCtx::Const,
        };
        loop {
            let decl_start = self.tok.start;
            let saved_lexical = std::mem::replace(&mut self.cx.lexical, kind != VarKind::Var);
            let saved_ctx = std::mem::replace(&mut self.binding_ctx, ctx);
            let target = self.parse_binding_target();
            // The initializer is an ordinary expression: nothing in it declares here.
            self.binding_ctx = saved_ctx;
            self.cx.lexical = saved_lexical;
            let target = target?;
            if self.ts {
                self.ts_definite_marker()?;
                if self.at(Punct::Colon) {
                    self.ts_annotation()?;
                }
            }
            let init = if self.eat(Punct::Eq)? { Some(self.parse_assign()?) } else { None };
            if init.is_some() {
                self.count_writes(target);
            }
            let declarator = self.finish(NodeKind::VarDeclarator { target, init }, decl_start);
            self.push(declarator);
            if !self.eat(Punct::Comma)? {
                break;
            }
        }
        let decls = self.take_list(mark);
        Ok(self.finish(NodeKind::VarDecl { kind, decls }, start))
    }

    /// `const x;` and `let [a];` need an initializer unless a `for-in`/`for-of` supplies it.
    fn check_initializers(&self, decl: NodeId) -> PResult<()> {
        let NodeKind::VarDecl { kind, decls } = self.ast[decl].kind else { return Ok(()) };
        for &d in self.ast.list(decls) {
            let NodeKind::VarDeclarator { target, init: None } = self.ast[d].kind else { continue };
            let is_pattern = !matches!(self.ast[target].kind, NodeKind::BindingIdent(_));
            if kind == VarKind::Const || is_pattern {
                return Err(self.error_at("Missing initializer in declaration", self.ast[d].span.start));
            }
        }
        Ok(())
    }

    // ---- control flow -------------------------------------------------------------------

    fn parse_paren_expression(&mut self) -> PResult<NodeId> {
        self.expect(Punct::LParen)?;
        let expr = self.allow_in(|p| p.parse_expression())?;
        self.expect(Punct::RParen)?;
        Ok(expr)
    }

    fn parse_if(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let test = self.parse_paren_expression()?;
        let cons = self.parse_if_body()?;
        let alt = if self.at_kw(Kw::Else) {
            self.next()?;
            Some(self.parse_if_body()?)
        } else {
            None
        };
        Ok(self.finish(NodeKind::If { test, cons, alt }, start))
    }

    /// The branch of an `if`. Sloppy code may use a bare function declaration there (Annex B);
    /// it is treated as if it sat in its own block, so that is how it is represented.
    fn parse_if_body(&mut self) -> PResult<NodeId> {
        if self.strict || !self.at_kw(Kw::Function) {
            return self.parse_statement();
        }
        let start = self.tok.start;
        self.push_scope(ScopeKind::Block);
        let function = self.parse_function(start, false, true, true)?;
        if self.ast[function].has(flags::GENERATOR) {
            return Err(self.error_at("Generators cannot be declared in this position", start));
        }
        let list = self.ast.alloc_list(&[function]);
        let block = self.finish(NodeKind::Block(list), start);
        self.sb.pop(block);
        Ok(block)
    }

    fn parse_loop_body(&mut self) -> PResult<NodeId> {
        let saved = std::mem::replace(&mut self.cx.in_loop, true);
        let body = self.parse_statement();
        self.cx.in_loop = saved;
        body
    }

    fn parse_while(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let test = self.parse_paren_expression()?;
        let body = self.parse_loop_body()?;
        Ok(self.finish(NodeKind::While { test, body }, start))
    }

    fn parse_do_while(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let body = self.parse_loop_body()?;
        self.expect_kw(Kw::While)?;
        let test = self.parse_paren_expression()?;
        // The semicolon after `do ... while (...)` is optional even on the same line.
        self.eat(Punct::Semi)?;
        Ok(self.finish(NodeKind::DoWhile { body, test }, start))
    }

    fn parse_for(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let mut is_await = false;
        if self.at_word("await") {
            if !self.cx.is_async && !(self.module && !self.cx.in_function) {
                return Err(self.error("`for await` is only valid in async functions and at the top level of modules"));
            }
            self.next()?;
            is_await = true;
        }
        self.expect(Punct::LParen)?;

        // A `let` or `const` in the head is scoped to the loop; open that scope first.
        let lexical_head = self.for_head_declares_lexically()?;
        if lexical_head {
            self.push_scope(ScopeKind::For);
        }
        let saved_no_in = std::mem::replace(&mut self.no_in, true);
        let head = self.parse_for_head();
        self.no_in = saved_no_in;
        let (init, left_is_decl) = head?;

        if let Some(left) = init {
            if self.at_word("of") || self.at_kw(Kw::In) {
                let is_of = self.at_word("of");
                if is_await && !is_of {
                    return Err(self.unexpected());
                }
                if left_is_decl {
                    self.check_for_in_of_decl(left, is_of)?;
                    // Each iteration assigns the declared names.
                    if let NodeKind::VarDecl { decls, .. } = self.ast[left].kind {
                        let declarators: Vec<NodeId> = self.ast.list(decls).to_vec();
                        for d in declarators {
                            if let NodeKind::VarDeclarator { target, .. } = self.ast[d].kind {
                                self.count_writes(target);
                            }
                        }
                    }
                } else {
                    self.to_assign_target(left)?;
                    self.cover_init = None;
                }
                self.next()?;
                let right = if is_of { self.parse_assign()? } else { self.parse_expression()? };
                self.expect(Punct::RParen)?;
                let body = self.parse_loop_body()?;
                let (kind, flag) = if is_of {
                    (NodeKind::ForOf { left, right, body }, if is_await { flags::AWAIT } else { 0 })
                } else {
                    (NodeKind::ForIn { left, right, body }, 0)
                };
                let node = self.finish_with(kind, start, flag);
                if lexical_head {
                    self.sb.pop(node);
                }
                return Ok(node);
            }
        }
        if is_await {
            return Err(self.unexpected_expecting("of"));
        }
        if let Some((pos, message)) = self.cover_init.take() {
            return Err(self.error_at(message, pos));
        }
        if let Some(decl) = init.filter(|_| left_is_decl) {
            self.check_initializers(decl)?;
        }
        self.expect(Punct::Semi)?;
        let test = if self.at(Punct::Semi) { None } else { Some(self.allow_in(|p| p.parse_expression())?) };
        self.expect(Punct::Semi)?;
        let update = if self.at(Punct::RParen) { None } else { Some(self.allow_in(|p| p.parse_expression())?) };
        self.expect(Punct::RParen)?;
        let body = self.parse_loop_body()?;
        let node = self.finish(NodeKind::For { init, test, update, body }, start);
        if lexical_head {
            self.sb.pop(node);
        }
        Ok(node)
    }

    /// Whether the `for` head about to be parsed starts with `let` or `const`.
    fn for_head_declares_lexically(&mut self) -> PResult<bool> {
        if self.at_kw(Kw::Const) {
            return Ok(true);
        }
        Ok(self.tok.kind == Kind::Ident && self.at_word("let") && self.let_starts_declaration()?)
    }

    /// The part of a `for` head before the first `;`, `in` or `of`. Returns the node and
    /// whether it is a declaration.
    fn parse_for_head(&mut self) -> PResult<(Option<NodeId>, bool)> {
        if self.at(Punct::Semi) {
            return Ok((None, false));
        }
        let start = self.tok.start;
        let is_let_decl = self.tok.kind == Kind::Ident && self.at_word("let") && self.let_starts_declaration()?;
        let kind = match self.tok.kind {
            Kind::Keyword(Kw::Var) if !self.tok.escaped => Some(VarKind::Var),
            Kind::Keyword(Kw::Const) if !self.tok.escaped => Some(VarKind::Const),
            _ if is_let_decl => Some(VarKind::Let),
            _ => None,
        };
        if let Some(kind) = kind {
            return Ok((Some(self.parse_var_decl(kind, start)?), true));
        }
        let outer = self.cover_init.take();
        let first = self.parse_assign_cover()?;
        let init = if self.at(Punct::Comma) {
            let mark = self.mark();
            self.push(first);
            while self.eat(Punct::Comma)? {
                let item = self.parse_assign()?;
                self.push(item);
            }
            let list = self.take_list(mark);
            self.finish(NodeKind::Sequence(list), start)
        } else {
            first
        };
        // A pending `{ a = 1 }` stays in `cover_init` for the caller: it is fine if this
        // turns out to be a `for-in`/`for-of` target, and an error otherwise.
        self.cover_init = self.cover_init.or(outer);
        Ok((Some(init), false))
    }

    fn check_for_in_of_decl(&self, decl: NodeId, is_of: bool) -> PResult<()> {
        let NodeKind::VarDecl { kind, decls } = self.ast[decl].kind else { return Ok(()) };
        let decls = self.ast.list(decls);
        if decls.len() != 1 {
            return Err(self.error_at("Only one variable may be declared in a `for-in` or `for-of` head", self.ast[decl].span.start));
        }
        if let NodeKind::VarDeclarator { target, init: Some(_) } = self.ast[decls[0]].kind {
            // Annex B: sloppy code may write `for (var x = 1 in obj)`.
            let legacy = !is_of
                && !self.strict
                && kind == VarKind::Var
                && matches!(self.ast[target].kind, NodeKind::BindingIdent(_));
            if !legacy {
                return Err(self.error_at("A `for-in` or `for-of` variable cannot have an initializer", self.ast[decls[0]].span.start));
            }
        }
        Ok(())
    }

    fn parse_jump(&mut self, kw: Kw) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let mut label = None;
        if self.tok.kind == Kind::Ident && !self.tok.newline_before {
            let atom = self.name_of(self.tok);
            if !self.cx.labels.contains(&atom) {
                return Err(self.error(format!("Undefined label `{}`", self.ast.atom(atom))));
            }
            label = Some(atom);
            self.next()?;
        } else if kw == Kw::Break && !(self.cx.in_loop || self.cx.in_switch) {
            return Err(self.error_at("Illegal `break` statement", start));
        }
        if kw == Kw::Continue && !self.cx.in_loop {
            return Err(self.error_at("Illegal `continue` statement", start));
        }
        self.semicolon()?;
        let kind = if kw == Kw::Break { NodeKind::Break(label) } else { NodeKind::Continue(label) };
        Ok(self.finish(kind, start))
    }

    fn parse_return(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        if (self.module && !self.cx.in_function && !self.fragment) || (self.cx.static_block && !self.cx.arrow) {
            return Err(self.error_at("Illegal `return` statement", start));
        }
        self.next()?;
        let ends = self.at(Punct::Semi) || self.at(Punct::RBrace) || self.tok.kind == Kind::Eof || self.tok.newline_before;
        let arg = if ends { None } else { Some(self.parse_expression()?) };
        self.semicolon()?;
        Ok(self.finish(NodeKind::Return(arg), start))
    }

    fn parse_throw(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        if self.tok.newline_before {
            return Err(self.error("Illegal newline after `throw`"));
        }
        let arg = self.parse_expression()?;
        self.semicolon()?;
        Ok(self.finish(NodeKind::Throw(arg), start))
    }

    fn parse_with(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        if self.strict {
            return Err(self.error("`with` is not allowed in strict mode"));
        }
        self.next()?;
        let object = self.parse_paren_expression()?;
        self.push_scope(ScopeKind::With);
        let body = self.parse_statement()?;
        let node = self.finish(NodeKind::With { object, body }, start);
        self.sb.pop(node);
        Ok(node)
    }

    fn parse_try(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let block = self.parse_block()?;
        let mut handler = None;
        if self.at_kw(Kw::Catch) {
            let catch_start = self.tok.start;
            self.next()?;
            // The catch clause's parameter and its body's declarations share one scope.
            self.push_scope(ScopeKind::Catch);
            let param = if self.eat(Punct::LParen)? {
                let ctx = if self.tok.kind == Kind::Ident { BindingCtx::CatchParam } else { BindingCtx::CatchPattern };
                let saved_ctx = std::mem::replace(&mut self.binding_ctx, ctx);
                let target = self.parse_binding_target();
                self.binding_ctx = saved_ctx;
                let target = target?;
                if self.ts && self.at(Punct::Colon) {
                    self.ts_annotation()?;
                }
                self.expect(Punct::RParen)?;
                Some(target)
            } else {
                None
            };
            let body = self.parse_block_scoped(false)?;
            let catch = self.finish(NodeKind::Catch { param, body }, catch_start);
            self.sb.pop(catch);
            handler = Some(catch);
        }
        let finalizer = if self.at_kw(Kw::Finally) {
            self.next()?;
            Some(self.parse_block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.error("Missing `catch` or `finally` after `try`"));
        }
        Ok(self.finish(NodeKind::Try { block, handler, finalizer }, start))
    }

    fn parse_switch(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let disc = self.parse_paren_expression()?;
        self.expect(Punct::LBrace)?;
        self.push_scope(ScopeKind::Switch);
        let saved = std::mem::replace(&mut self.cx.in_switch, true);
        let result = self.parse_switch_cases();
        self.cx.in_switch = saved;
        let cases = result?;
        self.expect(Punct::RBrace)?;
        let node = self.finish(NodeKind::Switch { disc, cases }, start);
        self.sb.pop(node);
        Ok(node)
    }

    fn parse_switch_cases(&mut self) -> PResult<List> {
        let mark = self.mark();
        let mut seen_default = false;
        while !self.at(Punct::RBrace) {
            let case_start = self.tok.start;
            let test = if self.at_kw(Kw::Case) {
                self.next()?;
                Some(self.allow_in(|p| p.parse_expression())?)
            } else if self.at_kw(Kw::Default) {
                if seen_default {
                    return Err(self.error("More than one `default` clause in `switch`"));
                }
                seen_default = true;
                self.next()?;
                None
            } else {
                return Err(self.unexpected());
            };
            self.expect(Punct::Colon)?;
            let body_mark = self.mark();
            while !(self.at_kw(Kw::Case) || self.at_kw(Kw::Default) || self.at(Punct::RBrace)) {
                if self.tok.kind == Kind::Eof {
                    return Err(self.unexpected_expecting("}"));
                }
                let stmt = self.parse_statement_list_item()?;
                self.push(stmt);
            }
            let body = self.take_list(body_mark);
            let case = self.finish(NodeKind::Case { test, body }, case_start);
            self.push(case);
        }
        Ok(self.take_list(mark))
    }
}
