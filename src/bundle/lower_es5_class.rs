//! Classes as es5: a constructor function, methods defined on its prototype (not enumerable, as the language defines them), `__extends` for a
//! parent, fields set from the constructor (`_$init`) and static members set once the class exists, in a function that returns the class.
//! `super(...)` makes the object with `Reflect.construct` where there is one (so a parent that is a built-in or a native class works), and
//! calls the parent as a function where there is not.
use super::*;
use crate::ast::MethodKind;
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

/// What `super` and `new.target` mean inside a function that belongs to a class.
#[derive(Clone, Debug)]
pub(super) struct Home {
    /// The variable of the rewritten class's function that holds the parent.
    pub base: String,
    pub is_static: bool,
    /// The function that sets the fields, if the class has any.
    pub init: Option<String>,
    /// The name of the class, for `new.target` in its constructor.
    pub is_ctor: bool,
}

impl Scanner<'_, '_> {
    /// `super(...)` and `super.x` in a class that is rewritten. Returns whether `node` was taken over.
    pub(super) fn lower_super_use(&mut self, node: NodeId) -> bool {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Call { callee, args } if matches!(ast[callee].kind, NodeKind::Super) => {
                let Some(home) = self.home() else {
                    self.refuse(Feature::Class, node, "and this `super()` is not in a class it can rewrite");
                    return true;
                };
                // Before `super` returns, `this` is the object `new` made, which tells `__superCall` what was constructed.
                let this = if self.es5_in_arrow() { "_$this" } else { "this" };
                let items = ast.list(args).to_vec();
                let array = self.arguments_array(&items);
                let mut pieces = vec![Piece::Text(format!("(_$this = __superCall({this}, {}, ", home.base))];
                pieces.extend(array);
                pieces.push(text(")"));
                if let Some(init) = &home.init {
                    pieces.push(Piece::Text(format!(", {init}.call(_$this)")));
                }
                pieces.push(text(", _$this)"));
                self.bundler.helpers.es5_class = true;
                self.push(node, pieces);
                true
            }
            NodeKind::Call { callee, args } if matches!(ast[callee].kind, NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Super)) => {
                let NodeKind::Member { prop, .. } = ast[callee].kind else { return false };
                let Some(access) = self.super_access(callee, prop) else { return true };
                let this = self.this_for_super();
                let items = ast.list(args).to_vec();
                let has_spread = items.iter().any(|&e| matches!(ast[e].kind, NodeKind::Spread(_)));
                let mut pieces = access;
                if has_spread {
                    pieces.push(Piece::Text(format!(".apply({this}, ")));
                    pieces.extend(self.arguments_array(&items));
                } else {
                    pieces.push(Piece::Text(format!(".call({this}")));
                    for &a in &items {
                        pieces.push(text(", "));
                        pieces.extend(self.rng(a));
                    }
                }
                pieces.push(text(")"));
                self.push(node, pieces);
                true
            }
            NodeKind::Member { object, prop } if matches!(ast[object].kind, NodeKind::Super) => {
                // A read of a property of the parent; a call of one is the arm above, an assignment is not rewritten.
                if let Some(access) = self.super_access(node, prop) {
                    self.push(node, access);
                }
                true
            }
            NodeKind::Assign { target, .. } if matches!(ast[target].kind, NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Super)) => {
                self.refuse(Feature::Class, node, "and a write to a property of super cannot be rewritten");
                true
            }
            NodeKind::Return(arg) if self.in_derived_ctor() => {
                let mut pieces = vec![text("return ")];
                match arg {
                    Some(a) => pieces.extend(self.rng(a)),
                    None => pieces.push(text("_$this")),
                }
                pieces.push(text(";"));
                self.push(node, pieces);
                true
            }
            _ => false,
        }
    }

    fn in_derived_ctor(&self) -> bool {
        self.es5_top_function().is_some_and(|(_, derived)| derived)
    }

    /// `this` where `super` is used: the variable of an arrow that is rewritten, or of a derived constructor.
    fn this_for_super(&self) -> &'static str {
        if self.bundler.legacy && (self.arrows_rewritten() && self.es5_in_arrow() || self.in_derived_ctor()) { "_$this" } else { "this" }
    }

    fn arguments_array(&mut self, items: &[NodeId]) -> Vec<Piece> {
        let ast = self.ast;
        if items.is_empty() {
            return vec![text("[]")];
        }
        if items.iter().any(|&e| matches!(ast[e].kind, NodeKind::Spread(_))) {
            return self.spread_array_pieces(items);
        }
        let (a, _) = self.span(items[0]);
        let (_, b) = self.span(*items.last().unwrap());
        vec![text("["), Piece::Range(a, b), text("]")]
    }

    /// `Base.prototype.name` (or `Base.name` in a static method) for a `super.name` or `super[name]`.
    fn super_access(&mut self, member: NodeId, prop: NodeId) -> Option<Vec<Piece>> {
        let ast = self.ast;
        let Some(home) = self.home() else {
            self.refuse(Feature::Class, member, "and this `super` is not in a class it can rewrite");
            return None;
        };
        let mut pieces = vec![Piece::Text(if home.is_static { home.base.clone() } else { format!("{}.prototype", home.base) })];
        if ast[member].has(crate::ast::flags::COMPUTED) {
            pieces.push(text("["));
            pieces.extend(self.rng(prop));
            pieces.push(text("]"));
        } else {
            let (a, b) = self.span(prop);
            pieces.push(text("."));
            pieces.push(Piece::Range(a, b));
        }
        Some(pieces)
    }

    pub(super) fn lower_class_es5(&mut self, node: NodeId) {
        let ast = self.ast;
        let NodeKind::Class { name, superclass, members } = ast[node].kind else { return };
        let members = ast.list(members).to_vec();
        if !ast.decorators_of(node).is_empty() {
            self.refuse(Feature::Class, node, "and a decorated class cannot be rewritten for it yet");
            return;
        }
        for &m in &members {
            match ast[m].kind {
                NodeKind::ClassField { .. } if ast[m].has(crate::ast::flags::ACCESSOR) => {
                    self.refuse(Feature::ClassField, m, "and an accessor field cannot be rewritten for it yet");
                    return;
                }
                _ => {}
            }
            if !ast.decorators_of(m).is_empty() {
                self.refuse(Feature::Class, m, "and a decorated member cannot be rewritten for it yet");
                return;
            }
        }
        self.bundler.helpers.es5_class = true;
        self.bundler.helpers.es5_object = true;
        let has_private = members.iter().any(|&m| matches!(ast[m].kind, NodeKind::ClassMethod { key, .. } | NodeKind::ClassField { key, .. } if matches!(ast[key].kind, NodeKind::PrivateName(_))));
        let declared = if has_private {
            self.bundler.helpers.class_private = true;
            let declared = self.declare_privates(&members);
            self.rewrite_privates(node, true);
            declared
        } else {
            Vec::new()
        };
        let derived = superclass.is_some();
        let n = self.bundler.temp_count;
        self.bundler.temp_count += 1;
        let base = format!("_$B{n}");
        let class_name: Vec<Piece> = match name {
            Some(id) => {
                let key = if self.top_decl == Some(node) { self.semantic.symbol_of(id).map(|s| Key::Symbol(self.module.id, s)) } else { None };
                match key {
                    Some(key) => vec![Piece::Name(key)],
                    None => self.rng(id),
                }
            }
            None => match &self.name_hint {
                Some((hinted, hint)) if *hinted == node && is_identifier_name(hint) => vec![Piece::Text(hint.clone())],
                _ => vec![Piece::Text(format!("_$C{n}"))],
            },
        };
        let named = |pieces: &mut Vec<Piece>| pieces.extend(class_name.clone());
        // Fields set for each new object.
        let instance_fields: Vec<NodeId> = members.iter().copied().filter(|&m| matches!(ast[m].kind, NodeKind::ClassField { .. }) && !ast[m].has(crate::ast::flags::STATIC)).collect();
        // Private methods and accessors of the objects are recorded for each new object.
        let mut instance_brands: Vec<String> = Vec::new();
        for &m in &members {
            if let NodeKind::ClassMethod { key, .. } = ast[m].kind {
                if !ast[m].has(crate::ast::flags::STATIC) {
                    if let Some(info) = self.private_info(key) {
                        if !instance_brands.contains(&info.brand) {
                            instance_brands.push(info.brand);
                        }
                    }
                }
            }
        }
        let init = (!instance_fields.is_empty() || !instance_brands.is_empty()).then(|| format!("_$init{n}"));
        let mut constructor: Option<NodeId> = None;
        for &m in &members {
            if let NodeKind::ClassMethod { value, kind, .. } = ast[m].kind {
                let is_static = ast[m].has(crate::ast::flags::STATIC);
                let is_ctor = kind == MethodKind::Constructor && !is_static;
                if is_ctor {
                    constructor = Some(value);
                    if derived {
                        self.es5.derived.insert(value);
                    }
                }
                self.es5.homes.insert(value, Home { base: base.clone(), is_static, init: init.clone(), is_ctor });
            }
        }
        let mut body: Vec<Piece> = Vec::new();
        for (_, info) in &declared {
            body.push(Piece::Text(format!(" {} = new {}();", info.brand, if info.kind == super::lower_class::PrivateKind::Field { "WeakMap" } else { "WeakSet" })));
        }
        if derived {
            body.push(text(" __extends("));
            named(&mut body);
            body.push(Piece::Text(format!(", {base});")));
        }
        // The constructor.
        match constructor {
            Some(function) => {
                let NodeKind::Function { body: block, .. } = ast[function].kind else { return };
                body.push(text(" function "));
                named(&mut body);
                let (fa, fb) = self.span(function);
                body.push(Piece::Range(fa, fb));
                let (ba, bb) = self.span(block);
                let mut head = vec![text(" __classCall(this, ")];
                head.extend(class_name.clone());
                head.push(text(");"));
                if derived {
                    head.push(text(" var _$this;"));
                } else if let Some(init) = &init {
                    head.push(Piece::Text(format!(" {init}.call(this);")));
                }
                self.insert(ba + 1, head);
                if derived {
                    self.insert(bb - 1, vec![text(" return _$this; ")]);
                }
            }
            None => {
                body.push(text(" function "));
                named(&mut body);
                body.push(text("() { __classCall(this, "));
                named(&mut body);
                body.push(text(");"));
                if derived {
                    body.push(Piece::Text(format!(" var _$this = __superCall(this, {base}, arguments);")));
                    if let Some(init) = &init {
                        body.push(Piece::Text(format!(" {init}.call(_$this);")));
                    }
                    body.push(text(" return _$this; }"));
                } else {
                    if let Some(init) = &init {
                        body.push(Piece::Text(format!(" {init}.call(this);")));
                    }
                    body.push(text(" }"));
                }
            }
        }
        // Fields: one function that sets them on `this`.
        if let Some(init) = &init {
            let mut statements: Vec<Piece> = Vec::new();
            let mut uses = lower_es5::Uses::default();
            for brand in &instance_brands {
                statements.push(Piece::Text(format!(" __privateAdd(this, {brand});")));
            }
            for &f in &instance_fields {
                if let NodeKind::ClassField { key, value } = ast[f].kind {
                    match self.private_info(key) {
                        Some(info) => {
                            statements.push(Piece::Text(format!(" __privateAdd(this, {}, ", info.brand)));
                        }
                        None => {
                            let key_pieces = self.field_key(f, key);
                            statements.push(text(" __ck(this, "));
                            statements.extend(key_pieces);
                            statements.push(text(", "));
                        }
                    }
                    match value {
                        Some(v) => {
                            statements.extend(self.rng(v));
                            self.arrow_uses(v, false, &mut uses);
                        }
                        None => statements.push(text("void 0")),
                    }
                    statements.push(text(");"));
                }
            }
            let mut head = Vec::new();
            if uses.this && self.arrows_rewritten() {
                head.push(text(" var _$this = this;"));
            }
            body.push(Piece::Text(format!(" var {init} = function () {{")));
            body.extend(head);
            body.extend(statements);
            body.push(text(" };"));
        }
        // Methods and accessors, in the order they are written.
        for &m in &members {
            let NodeKind::ClassMethod { key, value, kind } = ast[m].kind else { continue };
            if kind == MethodKind::Constructor && !ast[m].has(crate::ast::flags::STATIC) {
                continue;
            }
            // A private method, getter or setter is a function in a variable of the chunk.
            if let Some(info) = self.private_info(key) {
                let function = match kind {
                    MethodKind::Get => info.getter.clone(),
                    MethodKind::Set => info.setter.clone(),
                    _ => info.method.clone(),
                }
                .unwrap_or_default();
                let (fa, fb) = self.span(value);
                body.push(Piece::Text(format!(" {function} = function ")));
                body.push(Piece::Range(fa, fb));
                body.push(text(";"));
                if ast[m].has(crate::ast::flags::STATIC) {
                    body.push(text(" __privateAdd("));
                    named(&mut body);
                    body.push(Piece::Text(format!(", {});", info.brand)));
                }
                continue;
            }
            let is_static = ast[m].has(crate::ast::flags::STATIC);
            let kind_code = match kind {
                MethodKind::Get => 1,
                MethodKind::Set => 2,
                _ => 0,
            };
            body.push(text(" __method("));
            named(&mut body);
            body.push(text(if is_static { ", " } else { ".prototype, " }));
            // `named` wrote the class; the call's first argument is the class or its prototype.
            let _ = value;
            body.extend(self.field_key(m, key));
            body.push(Piece::Text(format!(", function{} ", self.native_star(value))));
            let (fa, fb) = self.span(value);
            body.push(Piece::Range(fa, fb));
            body.push(Piece::Text(format!(", {kind_code});")));
        }
        // Static fields and blocks, in a function whose `this` is the class.
        let statics: Vec<NodeId> = members.iter().copied().filter(|&m| matches!(ast[m].kind, NodeKind::StaticBlock(_)) || matches!(ast[m].kind, NodeKind::ClassField { .. }) && ast[m].has(crate::ast::flags::STATIC)).collect();
        if !statics.is_empty() {
            let mut statements: Vec<Piece> = Vec::new();
            let mut uses = lower_es5::Uses::default();
            for &s in &statics {
                match ast[s].kind {
                    NodeKind::ClassField { key, value } => {
                        match self.private_info(key) {
                            Some(info) => statements.push(Piece::Text(format!(" __privateAdd(this, {}, ", info.brand))),
                            None => {
                                statements.push(text(" __ck(this, "));
                                statements.extend(self.field_key(s, key));
                                statements.push(text(", "));
                            }
                        }
                        match value {
                            Some(v) => {
                                statements.extend(self.rng(v));
                                self.arrow_uses(v, false, &mut uses);
                            }
                            None => statements.push(text("void 0")),
                        }
                        statements.push(text(");"));
                    }
                    NodeKind::StaticBlock(list) => {
                        let (a, b) = self.span(s);
                        if let Some(open) = self.source_between(a, b).find('{') {
                            statements.push(text(" {"));
                            statements.push(Piece::Range(a + open as u32 + 1, b));
                        }
                        for &st in ast.list(list) {
                            self.arrow_uses(st, false, &mut uses);
                        }
                    }
                    _ => {}
                }
            }
            body.push(text(" (function () {"));
            if uses.this && self.arrows_rewritten() {
                body.push(text(" var _$this = this;"));
            }
            body.extend(statements);
            body.push(text(" }).call("));
            named(&mut body);
            body.push(text(");"));
        }
        body.push(text(" return "));
        named(&mut body);
        body.push(text("; "));
        // The whole: a function that makes the class, called with the parent.
        let mut pieces: Vec<Piece> = Vec::new();
        let declaration = ast[node].has(crate::ast::flags::DECL) && name.is_some() && self.top_decl != Some(node);
        if declaration {
            pieces.push(text("var "));
            pieces.extend(class_name.clone());
            pieces.push(text(" = "));
        }
        pieces.push(Piece::Text(if derived { format!("(function ({base}) {{") } else { "(function () {".to_string() }));
        pieces.extend(body);
        pieces.push(text("})("));
        if let Some(parent) = superclass {
            pieces.extend(self.rng(parent));
        }
        pieces.push(text(")"));
        if declaration {
            pieces.push(text(";"));
        }
        if has_private {
            self.private_scopes.pop();
        }
        self.push(node, pieces);
    }

    /// A member's key: a string for a name, the expression for a computed key.
    fn field_key(&mut self, member: NodeId, key: NodeId) -> Vec<Piece> {
        if self.ast[member].has(crate::ast::flags::COMPUTED) { self.rng(key) } else { vec![Piece::Text(self.key_string(key))] }
    }
}
