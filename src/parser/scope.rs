//! The parser's side of scope analysis: declaring each binding as it is parsed.
//!
//! A binding identifier does not say what it declares (`var`, a parameter, a catch
//! parameter...), so the code that knows sets `Parser::binding_ctx` before parsing a
//! pattern, and [`Parser::parse_binding_ident`] declares into the current scope according
//! to it. See `crate::semantic` for the scope data and the declaration rules.

use super::*;
use crate::ast::Atom;
use crate::semantic::{Redeclared, ScopeId, ScopeKind, SymbolKind, scope_flags};

/// What the binding identifiers parsed next declare.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BindingCtx {
    /// Nothing: a function or class name, which the caller declares itself.
    None,
    Var,
    Let,
    Const,
    Param,
    CatchParam,
    CatchPattern,
    Import,
}

impl<'a> Parser<'a> {
    pub(super) fn redeclared(&self, r: Redeclared) -> ParseError {
        let message = format!("Identifier '{}' has already been declared", self.ast.atom(r.name));
        self.error_at(message, self.ast[r.node].span.start)
    }

    /// Declares the binding identifier `node` according to the current binding context.
    pub(super) fn declare_binding(&mut self, name: Atom, node: NodeId) -> PResult<()> {
        let strict = self.strict;
        let result = match self.binding_ctx {
            BindingCtx::None => Ok(()),
            BindingCtx::Var => self.sb.declare_var(name, SymbolKind::Var, node).map(|_| ()),
            BindingCtx::Let => self.sb.declare_lexical(name, SymbolKind::Let, node, strict).map(|_| ()),
            BindingCtx::Const => self.sb.declare_lexical(name, SymbolKind::Const, node, strict).map(|_| ()),
            BindingCtx::Import => self.sb.declare_lexical(name, SymbolKind::Import, node, strict).map(|_| ()),
            BindingCtx::Param => {
                self.sb.declare_param(name, node);
                Ok(())
            }
            BindingCtx::CatchParam => self.sb.declare_catch_param(name, SymbolKind::CatchParam, node),
            BindingCtx::CatchPattern => self.sb.declare_catch_param(name, SymbolKind::CatchPatternParam, node),
        };
        result.map_err(|r| self.redeclared(r))
    }

    pub(super) fn push_scope(&mut self, kind: ScopeKind) -> ScopeId {
        self.sb.push(kind, 0)
    }

    /// Opens an arrow function's scope. Its parameters were parsed before `=>` revealed what
    /// they were, so whatever they created since `items.scope_mark` moves inside it.
    pub(super) fn push_arrow_scope(&mut self, scope_mark: usize, ref_mark: usize) -> ScopeId {
        self.sb.push_adopting(ScopeKind::Function, scope_flags::ARROW, scope_mark, ref_mark)
    }

    /// Records `node`, a freshly made `Ident`, as a reference in the current scope.
    pub(super) fn record_ref(&mut self, node: NodeId) {
        self.sb.record_ref(node);
    }

    /// An `Ident` that is assigned to. `also_read` for `+=`, `++` and the like.
    pub(super) fn mark_assigned(&mut self, node: NodeId, also_read: bool) {
        let extra = if also_read { flags::READ_ALSO } else { 0 };
        self.set_flag(node, flags::ASSIGNED | extra);
    }

    /// Counts a write for every name a declaration pattern binds (it was given a value).
    pub(super) fn count_writes(&mut self, target: NodeId) {
        if matches!(self.ast[target].kind, NodeKind::BindingIdent(_)) {
            self.sb.add_write(target);
            return;
        }
        let mut idents = Vec::new();
        crate::semantic::binding_idents(&self.ast, target, &mut idents);
        for ident in idents {
            self.sb.add_write(ident);
        }
    }

    /// A function declaration's name, bound where the declaration sits.
    pub(super) fn declare_function_decl_name(&mut self, name_node: NodeId, is_async: bool, is_generator: bool) -> PResult<()> {
        let NodeKind::BindingIdent(name) = self.ast[name_node].kind else { return Ok(()) };
        let strict = self.strict;
        self.sb
            .declare_function_name(name, name_node, !is_async && !is_generator, strict)
            .map_err(|r| self.redeclared(r))
    }
}
