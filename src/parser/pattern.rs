use super::*;
use crate::ast::{AssignOp, Atom, PropKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternMode {
    /// `[a, b.c] = value`: targets are references and member accesses.
    Assign,
    /// `let [a, b] = value` and parameters: targets are new bindings.
    Binding,
}

impl<'a> Parser<'a> {
    // ---- name checks --------------------------------------------------------------------

    /// `eval` and `arguments` cannot be assigned in strict code.
    pub(super) fn check_assign_name(&self, name: Atom, pos: u32) -> PResult<()> {
        if self.strict && (name == known::EVAL || name == known::ARGUMENTS) {
            return Err(self.error_at("`eval` and `arguments` cannot be assigned in strict mode", pos));
        }
        Ok(())
    }

    /// A name being declared: reserved words are out, and in strict code so are
    /// `eval` and `arguments`.
    pub(super) fn check_binding_name(&self, name: Atom, pos: u32) -> PResult<()> {
        if let Some(message) = self.reserved_reason(name) {
            return Err(self.error_at(message, pos));
        }
        if self.cx.lexical && name == known::LET {
            return Err(self.error_at("`let` is disallowed as a lexically bound name", pos));
        }
        self.check_assign_name(name, pos)
    }

    /// Every name a binding pattern declares, in source order.
    pub(super) fn bound_names(&self, id: NodeId, out: &mut Vec<Atom>) {
        match self.ast[id].kind {
            NodeKind::BindingIdent(name) => out.push(name),
            NodeKind::AssignPattern { target, .. } => self.bound_names(target, out),
            NodeKind::Rest(arg) => self.bound_names(arg, out),
            NodeKind::PatternProp { value, .. } => self.bound_names(value, out),
            NodeKind::ArrayPattern(list) | NodeKind::ObjectPattern(list) => {
                for &item in self.ast.list(list) {
                    self.bound_names(item, out);
                }
            }
            _ => {}
        }
    }

    // ---- binding patterns in declarations and parameters --------------------------------

    /// A binding identifier, declared according to `binding_ctx`.
    pub(super) fn parse_binding_ident(&mut self) -> PResult<NodeId> {
        let node = self.parse_binding_ident_plain()?;
        if self.binding_ctx != BindingCtx::None {
            if let NodeKind::BindingIdent(name) = self.ast[node].kind {
                self.declare_binding(name, node)?;
            }
        }
        Ok(node)
    }

    /// A binding identifier that the caller declares itself (function and class names).
    pub(super) fn parse_binding_ident_plain(&mut self) -> PResult<NodeId> {
        let tok = self.tok;
        if tok.kind != Kind::Ident {
            return Err(self.unexpected());
        }
        let name = self.name_of(tok);
        self.check_binding_name(name, tok.start)?;
        self.next()?;
        Ok(self.finish(NodeKind::BindingIdent(name), tok.start))
    }

    pub(super) fn parse_binding_target(&mut self) -> PResult<NodeId> {
        match self.tok.kind {
            Kind::Ident => self.parse_binding_ident(),
            Kind::Punct(Punct::LBracket) => self.parse_array_binding(),
            Kind::Punct(Punct::LBrace) => self.parse_object_binding(),
            _ => Err(self.unexpected()),
        }
    }

    /// A binding target with an optional `= default`.
    pub(super) fn parse_binding_element(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        let target = self.parse_binding_target()?;
        if !self.eat(Punct::Eq)? {
            return Ok(target);
        }
        // The default is an ordinary expression: nothing in it declares into this pattern.
        let ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
        let default = self.allow_in(|p| p.parse_assign());
        self.binding_ctx = ctx;
        let default = default?;
        Ok(self.finish(NodeKind::AssignPattern { target, default }, start))
    }

    fn parse_array_binding(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let mark = self.mark();
        while !self.at(Punct::RBracket) {
            if self.at(Punct::Comma) {
                let hole = self.ast.alloc(NodeKind::Hole, Span { start: self.tok.start, end: self.tok.start }, 0);
                self.push(hole);
                self.next()?;
                continue;
            }
            let item_start = self.tok.start;
            if self.eat(Punct::Ellipsis)? {
                let target = self.parse_binding_target()?;
                let rest = self.finish(NodeKind::Rest(target), item_start);
                self.push(rest);
                if !self.at(Punct::RBracket) {
                    return Err(self.error_at("A rest element must be last", item_start));
                }
                break;
            }
            let element = self.parse_binding_element()?;
            self.push(element);
            if !self.at(Punct::RBracket) {
                self.expect(Punct::Comma)?;
            }
        }
        self.expect(Punct::RBracket)?;
        let list = self.take_list(mark);
        Ok(self.finish(NodeKind::ArrayPattern(list), start))
    }

    fn parse_object_binding(&mut self) -> PResult<NodeId> {
        let start = self.tok.start;
        self.next()?;
        let mark = self.mark();
        while !self.at(Punct::RBrace) {
            let item_start = self.tok.start;
            if self.eat(Punct::Ellipsis)? {
                let target = self.parse_binding_ident()?;
                let rest = self.finish(NodeKind::Rest(target), item_start);
                self.push(rest);
                if !self.at(Punct::RBrace) {
                    return Err(self.error_at("A rest element must be last", item_start));
                }
                break;
            }
            let key_tok = self.tok;
            // A computed key is an ordinary expression: nothing in it declares here.
            let ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
            let key_result = self.parse_property_key(false);
            self.binding_ctx = ctx;
            let (key, computed) = key_result?;
            let mut prop_flags = if computed { flags::COMPUTED } else { 0 };
            let value = if self.eat(Punct::Colon)? {
                self.parse_binding_element()?
            } else {
                // Shorthand: `{ a }` or `{ a = 1 }`.
                if key_tok.kind != Kind::Ident || computed {
                    return Err(self.unexpected_expecting(":"));
                }
                prop_flags |= flags::SHORTHAND;
                let name = self.name_of(key_tok);
                self.check_binding_name(name, key_tok.start)?;
                let target =
                    self.ast.alloc(NodeKind::BindingIdent(name), Span { start: key_tok.start, end: key_tok.end }, 0);
                self.declare_binding(name, target)?;
                if self.eat(Punct::Eq)? {
                    let ctx = std::mem::replace(&mut self.binding_ctx, BindingCtx::None);
                    let default = self.allow_in(|p| p.parse_assign());
                    self.binding_ctx = ctx;
                    let default = default?;
                    self.finish(NodeKind::AssignPattern { target, default }, key_tok.start)
                } else {
                    target
                }
            };
            let prop = self.finish_with(NodeKind::PatternProp { key, value }, item_start, prop_flags);
            self.push(prop);
            if !self.at(Punct::RBrace) {
                self.expect(Punct::Comma)?;
            }
        }
        self.expect(Punct::RBrace)?;
        let list = self.take_list(mark);
        Ok(self.finish(NodeKind::ObjectPattern(list), start))
    }

    // ---- cover grammar: rewriting expressions into patterns -----------------------------

    /// Rewrites an expression that was parsed as a value into a pattern, in place. Also
    /// accepts nodes that are already patterns (an `Assign` target converted earlier) and
    /// re-validates them for the new mode.
    pub(super) fn to_pattern(&mut self, id: NodeId, mode: PatternMode) -> PResult<()> {
        let node = self.ast[id];
        let pos = node.span.start;
        let invalid = || self.error_at("Invalid destructuring assignment target", pos);
        match node.kind {
            NodeKind::Ident(name) => {
                if mode == PatternMode::Binding {
                    if node.parenthesized() {
                        return Err(invalid());
                    }
                    self.check_binding_name(name, pos)?;
                    // The only binding-mode conversion is arrow parameters, whose scope the
                    // caller has just opened.
                    self.ast.node_mut(id).kind = NodeKind::BindingIdent(name);
                    self.sb.declare_param(name, id);
                } else {
                    self.check_assign_name(name, pos)?;
                    self.mark_assigned(id, false);
                }
                Ok(())
            }
            NodeKind::BindingIdent(name) => self.check_binding_name(name, pos),
            NodeKind::Member { .. } if mode == PatternMode::Assign => Ok(()),
            NodeKind::Array(_) | NodeKind::Object(_) if node.has(flags::REST_COMMA) => {
                Err(self.error_at("A rest element must be last", pos))
            }
            NodeKind::Array(list) | NodeKind::ArrayPattern(list) if !node.parenthesized() => {
                let items: Vec<NodeId> = self.ast.list(list).to_vec();
                for (i, &item) in items.iter().enumerate() {
                    match self.ast[item].kind {
                        NodeKind::Hole => {}
                        NodeKind::Spread(arg) | NodeKind::Rest(arg) => {
                            if i != items.len() - 1 {
                                return Err(self.error_at("A rest element must be last", self.ast[item].span.start));
                            }
                            if matches!(self.ast[arg].kind, NodeKind::Assign { .. } | NodeKind::AssignPattern { .. }) {
                                return Err(self.error_at("A rest element cannot have a default", self.ast[arg].span.start));
                            }
                            self.to_pattern(arg, mode)?;
                            self.ast.node_mut(item).kind = NodeKind::Rest(arg);
                        }
                        _ => self.to_pattern_element(item, mode)?,
                    }
                }
                self.ast.node_mut(id).kind = NodeKind::ArrayPattern(list);
                Ok(())
            }
            NodeKind::Object(list) | NodeKind::ObjectPattern(list) if !node.parenthesized() => {
                let items: Vec<NodeId> = self.ast.list(list).to_vec();
                for (i, &item) in items.iter().enumerate() {
                    let item_node = self.ast[item];
                    match item_node.kind {
                        NodeKind::Property { key, value, kind: PropKind::Init } => {
                            self.to_pattern_element(value, mode)?;
                            self.ast.node_mut(item).kind = NodeKind::PatternProp { key, value };
                        }
                        NodeKind::PatternProp { value, .. } => self.to_pattern_element(value, mode)?,
                        NodeKind::Spread(arg) | NodeKind::Rest(arg) => {
                            if i != items.len() - 1 {
                                return Err(self.error_at("A rest element must be last", item_node.span.start));
                            }
                            // `{ ...rest }` takes a single target, never a nested pattern.
                            if !matches!(
                                self.ast[arg].kind,
                                NodeKind::Ident(_) | NodeKind::BindingIdent(_) | NodeKind::Member { .. }
                            ) {
                                return Err(self.error_at("Invalid rest element", self.ast[arg].span.start));
                            }
                            self.to_pattern(arg, mode)?;
                            self.ast.node_mut(item).kind = NodeKind::Rest(arg);
                        }
                        _ => return Err(self.error_at("Invalid destructuring assignment target", item_node.span.start)),
                    }
                }
                self.ast.node_mut(id).kind = NodeKind::ObjectPattern(list);
                Ok(())
            }
            _ => Err(invalid()),
        }
    }

    /// The target of a plain `=` or a `for-in`/`for-of` head: a pattern, or a call (see
    /// `check_simple_target`).
    pub(super) fn to_assign_target(&mut self, id: NodeId) -> PResult<()> {
        if matches!(self.ast[id].kind, NodeKind::Call { .. }) {
            return Ok(());
        }
        self.to_pattern(id, PatternMode::Assign)
    }

    /// A pattern element: a target with an optional default.
    pub(super) fn to_pattern_element(&mut self, id: NodeId, mode: PatternMode) -> PResult<()> {
        let node = self.ast[id];
        match node.kind {
            NodeKind::Assign { op: AssignOp::Assign, target, value } if !node.parenthesized() => {
                self.to_pattern(target, mode)?;
                self.ast.node_mut(id).kind = NodeKind::AssignPattern { target, default: value };
                Ok(())
            }
            NodeKind::AssignPattern { target, .. } => self.to_pattern(target, mode),
            _ => self.to_pattern(id, mode),
        }
    }
}
