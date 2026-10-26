//! Class fields, private members and static blocks, for targets older than es2022 (`--target`).
//!
//! ```js
//! class C extends B { x = 1; #p = 2; static s = 3; #m() {} static { init(); } get() { return this.#p; } }
//! ```
//! becomes
//! ```js
//! class C extends B { constructor() { super(...arguments); __privateAdd(this, _m); __publicField(this, "x", 1); __privateAdd(this, _p, 2); } get() { return (__privateGet(this, _p)); } }
//! ;_p = new WeakMap(); _m = new WeakSet(); _m_fn = function () {};   // brands and private methods
//! ;__publicField(C, "s", 3); (function () { init(); }).call(C);        // statics, in order
//! ```
//! (the names are temporaries, `__rkN`, declared at the top of the file). A class that is an expression is wrapped: `(T = class {...}, ..., T)`.
//! Private names are looked up in the class that declares them, and in the classes around it, so a nested class can use or hide the name of the
//! outer one. Every use is rewritten when the class that declares the name is met: a read is `__privateGet`, an assignment `__privateSet`, an
//! update, compound assignment or destructuring target `__privateWrapper(...)._`, a call keeps `this` with `.call`, and `#x in o` is `__privateIn`.

use super::*;
use crate::ast::{AssignOp, BinOp, MethodKind};
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PrivateKind {
    Field,
    Method,
    Accessor,
}

/// A private name of a class: where its objects are recorded, and the functions of a method or accessor.
#[derive(Clone, Debug)]
pub(super) struct Private {
    brand: String,
    kind: PrivateKind,
    method: Option<String>,
    getter: Option<String>,
    setter: Option<String>,
}

/// The private names a class declares (`None`: a nested class declares the name, so it is not the outer one here).
#[derive(Default)]
pub(super) struct PrivateScope {
    names: std::collections::HashMap<String, Option<Private>>,
}

impl Scanner<'_, '_> {
    fn private_text(&self, key: NodeId) -> String {
        let (s, e) = self.span(key);
        self.source_between(s, e).to_string()
    }

    fn is_private_key(&self, key: NodeId) -> bool {
        matches!(self.ast[key].kind, NodeKind::PrivateName(_))
    }

    fn resolve_private(&self, name: &str) -> Option<Private> {
        for scope in self.private_scopes.iter().rev() {
            if let Some(found) = scope.names.get(name) {
                return found.clone();
            }
        }
        None
    }

    /// The private member a `Member` node names, when one of the classes being rewritten declares it. What is found is kept for the
    /// rewrites that come after the class has been dealt with (an assignment operator, an optional chain).
    pub(super) fn private_of_member(&mut self, member: NodeId) -> Option<Private> {
        if let Some(known) = self.private_refs.get(&member) {
            return Some(known.clone());
        }
        match self.ast[member].kind {
            NodeKind::Member { prop, .. } if self.is_private_key(prop) => {
                let found = self.resolve_private(&self.private_text(prop))?;
                self.private_refs.insert(member, found.clone());
                Some(found)
            }
            _ => None,
        }
    }

    /// Whether a chain has a link that names a private member that is being rewritten.
    pub(super) fn chain_has_private(&self, inner: NodeId) -> bool {
        let mut at = inner;
        loop {
            match self.ast[at].kind {
                NodeKind::Member { object, .. } => {
                    if self.private_refs.contains_key(&at) {
                        return true;
                    }
                    at = object;
                }
                NodeKind::Call { callee, .. } => at = callee,
                _ => return false,
            }
            if matches!(self.ast[at].kind, NodeKind::Chain(_)) {
                return false;
            }
        }
    }

    pub(super) fn private_ref(&self, member: NodeId) -> Option<&Private> {
        self.private_refs.get(&member)
    }

    /// The pieces that read a private member: `(__privateGet(obj, brand))`, or the method itself.
    pub(super) fn private_read(&self, info: &Private, object: Vec<Piece>) -> Vec<Piece> {
        let mut pieces = vec![text("(")];
        match info.kind {
            PrivateKind::Method => {
                pieces.push(text("__privateMethod("));
                pieces.extend(object);
                pieces.push(text(&format!(", {}, {})", info.brand, info.method.clone().unwrap_or_default())));
            }
            _ => {
                pieces.push(text("__privateGet("));
                pieces.extend(object);
                match &info.getter {
                    Some(g) => pieces.push(text(&format!(", {}, {})", info.brand, g))),
                    None => pieces.push(text(&format!(", {})", info.brand))),
                }
            }
        }
        pieces.push(text(")"));
        pieces
    }

    /// `__privateWrapper(obj, brand)`: an object whose `_` can be assigned to, updated and destructured into.
    pub(super) fn private_wrapper_call(&self, info: &Private, object: Vec<Piece>) -> Vec<Piece> {
        let mut pieces = vec![text("__privateWrapper(")];
        pieces.extend(object);
        match (&info.setter, &info.getter) {
            (None, None) => pieces.push(text(&format!(", {})", info.brand))),
            (set, get) => pieces.push(text(&format!(", {}, {}, {})", info.brand, set.clone().unwrap_or_else(|| "void 0".into()), get.clone().unwrap_or_else(|| "void 0".into())))),
        }
        pieces
    }

    /// `__privateWrapper(obj, brand)._`: something that can be assigned to, updated and destructured into.
    pub(super) fn private_wrapper(&self, info: &Private, object: Vec<Piece>) -> Vec<Piece> {
        let mut pieces = self.private_wrapper_call(info, object);
        pieces.push(text("._"));
        pieces
    }

    /// An object written for its first use and again for the next: a name or `this`, or a temporary that holds it.
    pub(super) fn object_forms(&mut self, object: NodeId) -> (Vec<Piece>, Vec<Piece>) {
        let (os, oe) = self.span(object);
        if self.is_plain(object) {
            (vec![Piece::Range(os, oe)], vec![Piece::Range(os, oe)])
        } else {
            let t = self.temp();
            (vec![text(&format!("({t} = (")), Piece::Range(os, oe), text("))")], vec![text(&t)])
        }
    }

    fn helpers_for_private(&mut self) {
        self.bundler.helpers.class_private = true;
    }

    // ---- the class ----

    /// Whether the class has a field, a private member or a static block.
    pub(super) fn class_needs_lowering(&self, class: NodeId) -> bool {
        let ast = self.ast;
        let NodeKind::Class { members, .. } = ast[class].kind else { return false };
        ast.list(members).iter().any(|&m| match ast[m].kind {
            NodeKind::ClassField { .. } | NodeKind::StaticBlock(_) => true,
            NodeKind::ClassMethod { key, .. } => self.is_private_key(key),
            _ => false,
        })
    }

    pub(super) fn lower_class(&mut self, class: NodeId) {
        let ast = self.ast;
        let NodeKind::Class { name, superclass, members } = ast[class].kind else { return };
        let member_ids: Vec<NodeId> = ast.list(members).to_vec();
        let target = self.bundler.target.name();
        if !ast.decorators_of(class).is_empty() || member_ids.iter().any(|&m| !ast.decorators_of(m).is_empty()) {
            self.lower_error_message(class, format!("a class with decorators and a class field, private member or static block cannot be rewritten for the target {target} yet"));
            return;
        }
        if member_ids.iter().any(|&m| matches!(ast[m].kind, NodeKind::ClassField { .. }) && ast[m].has(flags::ACCESSOR)) {
            self.lower_error_message(class, format!("an auto-accessor (accessor x) cannot be rewritten for the target {target} yet"));
            return;
        }
        self.helpers_for_private();

        // The private names this class declares, and the variables behind them.
        let mut scope = PrivateScope::default();
        for &m in &member_ids {
            match ast[m].kind {
                NodeKind::ClassField { key, .. } if self.is_private_key(key) => {
                    let brand = self.temp();
                    scope.names.insert(self.private_text(key), Some(Private { brand, kind: PrivateKind::Field, method: None, getter: None, setter: None }));
                }
                NodeKind::ClassMethod { key, kind, .. } if self.is_private_key(key) => {
                    let key_text = self.private_text(key);
                    if !scope.names.contains_key(&key_text) {
                        let brand = self.temp();
                        let kind_of = if kind == MethodKind::Method { PrivateKind::Method } else { PrivateKind::Accessor };
                        scope.names.insert(key_text.clone(), Some(Private { brand, kind: kind_of, method: None, getter: None, setter: None }));
                    }
                    let function = self.temp();
                    if let Some(Some(info)) = scope.names.get_mut(&key_text) {
                        match kind {
                            MethodKind::Get => info.getter = Some(function),
                            MethodKind::Set => info.setter = Some(function),
                            _ => info.method = Some(function),
                        }
                    }
                }
                _ => {}
            }
        }
        let declared: Vec<(String, Private)> = scope.names.iter().filter_map(|(k, v)| v.clone().map(|p| (k.clone(), p))).collect();
        self.private_scopes.push(scope);
        self.rewrite_privates(class, true);

        // What the class looks like from outside: its name, or a temporary that holds it.
        let declares = name.is_some() && ast[class].has(flags::DECL);
        let (class_ref, holder): (Vec<Piece>, Option<String>) = match name {
            Some(n) if declares => {
                let (s, e) = self.span(n);
                (vec![Piece::Range(s, e)], None)
            }
            _ => {
                let t = self.temp();
                (vec![text(&t)], Some(t))
            }
        };
        let inferred = match self.name_hint.take() {
            Some((node, name)) if node == class => Some(name),
            other => {
                self.name_hint = other;
                None
            }
        };

        // A field's value, as an argument. A function or class with no name of its own is named by the field, which an argument does not do,
        // so it is evaluated as the value of a property of that name instead: `({ [key]: value })[key]`.
        let value_pieces = |this: &mut Self, value: Option<NodeId>, key: Option<&[Piece]>| -> Vec<Piece> {
            match value {
                None => vec![text("void 0")],
                Some(v) => {
                    let (s, e) = this.span(v);
                    let plain = if ast[v].flags & crate::ast::flags::PARENTHESIZED != 0 || matches!(ast[v].kind, NodeKind::Sequence(_)) {
                        vec![text("("), Piece::Range(s, e), text(")")]
                    } else {
                        vec![Piece::Range(s, e)]
                    };
                    let anonymous = matches!(ast[v].kind, NodeKind::Function { name: None, .. } | NodeKind::Class { name: None, .. });
                    match (anonymous, key) {
                        (true, Some(key)) => {
                            let mut pieces = vec![text("({ [")];
                            pieces.extend(key.to_vec());
                            pieces.push(text("]: "));
                            pieces.extend(plain);
                            pieces.push(text(" })["));
                            pieces.extend(key.to_vec());
                            pieces.push(text("]"));
                            pieces
                        }
                        _ => plain,
                    }
                }
            }
        };

        let mut brand_adds: Vec<Vec<Piece>> = Vec::new();
        let mut instance_inits: Vec<Vec<Piece>> = Vec::new();
        let mut group1: Vec<Vec<Piece>> = Vec::new();
        let mut group2: Vec<Vec<Piece>> = Vec::new();
        let mut deletions: Vec<(u32, u32)> = Vec::new();
        let mut added_brands: Vec<String> = Vec::new();
        // The brands of all the private names, made first.
        for (_, info) in &declared {
            let maker = if info.kind == PrivateKind::Field { "new WeakMap()" } else { "new WeakSet()" };
            group1.push(vec![text(&format!("{} = {maker}", info.brand))]);
        }
        let mut static_name_defined = false;
        for &m in &member_ids {
            let is_static = ast[m].has(flags::STATIC);
            match ast[m].kind {
                NodeKind::ClassField { key, value } => {
                    let private = if self.is_private_key(key) { self.resolve_private(&self.private_text(key)) } else { None };
                    let computed = ast[m].has(flags::COMPUTED);
                    let key_pieces = |this: &mut Self| -> (Vec<Piece>, Vec<Piece>, Option<Vec<Piece>>) {
                        // The key as the argument that names it, the same key as an expression that can be written again, and for a
                        // computed key the statement that evaluates it once (after the class, for an instance field).
                        let (ks, ke) = this.span(key);
                        match ast[key].kind {
                            NodeKind::PropName(a) | NodeKind::Str(a) if !computed => (vec![Piece::Text(json_string(ast.atom(a)))], vec![Piece::Text(json_string(ast.atom(a)))], None),
                            NodeKind::Num(_) if !computed => (vec![Piece::Range(ks, ke)], vec![Piece::Range(ks, ke)], None),
                            _ if is_static => {
                                let t = this.temp();
                                (vec![text(&format!("({t} = ")), Piece::Range(ks, ke), text(")")], vec![text(&t)], None)
                            }
                            _ => {
                                let t = this.temp();
                                (vec![text(&t)], vec![text(&t)], Some(vec![text(&format!("{t} = (")), Piece::Range(ks, ke), text(")")]))
                            }
                        }
                    };
                    if is_static {
                        let wrapped = self.static_value(value, &class_ref, &mut group2, name, holder.as_deref());
                        let mut item = Vec::new();
                        let repeat: Vec<Piece>;
                        match &private {
                            Some(info) => {
                                item.push(text("__privateAdd("));
                                item.extend(class_ref.clone());
                                item.push(text(&format!(", {}, ", info.brand)));
                                repeat = vec![Piece::Text(json_string(&self.private_text(key)))];
                            }
                            None => {
                                let (key_arg, key_repeat, _) = key_pieces(self);
                                if matches!(ast[key].kind, NodeKind::PropName(a) | NodeKind::Str(a) if ast.atom(a) == "name") {
                                    static_name_defined = true;
                                }
                                item.push(text("__publicField("));
                                item.extend(class_ref.clone());
                                item.push(text(", "));
                                item.extend(key_arg);
                                item.push(text(", "));
                                repeat = key_repeat;
                            }
                        }
                        item.extend(wrapped.unwrap_or_else(|| value_pieces(self, value, Some(&repeat))));
                        item.push(text(")"));
                        group2.push(item);
                    } else {
                        let mut item = Vec::new();
                        let repeat: Vec<Piece>;
                        match &private {
                            Some(info) => {
                                item.push(text(&format!("__privateAdd(this, {}, ", info.brand)));
                                repeat = vec![Piece::Text(json_string(&self.private_text(key)))];
                            }
                            None => {
                                let (key_arg, key_repeat, once) = key_pieces(self);
                                if let Some(once) = once {
                                    group1.push(once);
                                }
                                item.push(text("__publicField(this, "));
                                item.extend(key_arg);
                                item.push(text(", "));
                                repeat = key_repeat;
                            }
                        }
                        item.extend(value_pieces(self, value, Some(&repeat)));
                        item.push(text(")"));
                        instance_inits.push(item);
                    }
                    deletions.push(self.span(m));
                }
                NodeKind::ClassMethod { key, value, kind } if self.is_private_key(key) => {
                    let Some(info) = self.resolve_private(&self.private_text(key)) else { continue };
                    let function = match kind {
                        MethodKind::Get => info.getter.clone(),
                        MethodKind::Set => info.setter.clone(),
                        _ => info.method.clone(),
                    }
                    .unwrap_or_default();
                    let value_flags = ast[value].flags;
                    let (vs, ve) = self.span(value);
                    if self.uses_super(value) {
                        self.lower_error_message(m, format!("a private method that uses super cannot be rewritten for the target {target} yet"));
                    }
                    let mut item = vec![text(&format!("{function} = {}function{}", if value_flags & crate::ast::flags::ASYNC != 0 { "async " } else { "" }, if value_flags & crate::ast::flags::GENERATOR != 0 { "*" } else { "" })), Piece::Range(vs, ve)];
                    item.insert(0, text(""));
                    group1.push(item);
                    if !added_brands.contains(&info.brand) {
                        added_brands.push(info.brand.clone());
                        if is_static {
                            let mut add = vec![text("__privateAdd(")];
                            add.extend(class_ref.clone());
                            add.push(text(&format!(", {})", info.brand)));
                            group1.push(add);
                        } else {
                            brand_adds.push(vec![text(&format!("__privateAdd(this, {})", info.brand))]);
                        }
                    }
                    deletions.push(self.span(m));
                }
                NodeKind::StaticBlock(_) => {
                    let (s, e) = self.span(m);
                    let open = self.source_between(s, e).find('{').map_or(s, |p| s + p as u32);
                    let mut item = vec![text("(function () { "), Piece::Range(open + 1, e - 1), text(" }).call(")];
                    item.extend(class_ref.clone());
                    item.push(text(")"));
                    group2.push(item);
                    deletions.push((s, e));
                }
                _ => {}
            }
        }
        let _ = static_name_defined;
        for (s, e) in deletions {
            // The `;` after a field is part of it; one after a method is an empty member and harmless.
            self.edits.push(Edit { start: s, end: e, pieces: Vec::new(), shorthand: None, transparent: false });
        }

        // The constructor runs the instance initializers.
        let mut inits: Vec<Vec<Piece>> = brand_adds;
        inits.extend(instance_inits);
        if !inits.is_empty() {
            self.inject_constructor(class, name, superclass, &member_ids, &inits);
        }

        // After the class: brands, private methods, then the statics.
        let mut all = group1;
        if let Some(inferred_name) = inferred {
            if let (Some(t), false) = (&holder, static_name_defined) {
                all.insert(0, vec![text(&format!("Object.defineProperty({t}, \"name\", {{ value: {}, configurable: true }})", json_string(&inferred_name)))]);
            }
        }
        all.extend(group2);
        let (cs, ce) = self.span(class);
        match holder {
            Some(t) => {
                // A class expression: `(T = class {...}, brands, statics, T)`.
                self.edits.push(Edit { start: cs, end: cs, pieces: vec![text(&format!("({t} = "))], shorthand: None, transparent: false });
                let mut tail = Vec::new();
                for item in all {
                    tail.push(text(", "));
                    tail.extend(item);
                }
                tail.push(text(&format!(", {t})")));
                self.edits.push(Edit { start: ce, end: ce, pieces: tail, shorthand: None, transparent: false });
            }
            None => {
                let mut tail = Vec::new();
                for item in all {
                    tail.push(text(";\n"));
                    tail.extend(item);
                }
                tail.push(text(";"));
                self.edits.push(Edit { start: ce, end: ce, pieces: tail, shorthand: None, transparent: false });
            }
        }
        let _ = declared;
        self.private_scopes.pop();
    }

    /// The value of a static field as the text that runs after the class: a plain expression, or, when it uses `this`, a function called on the class.
    /// `None` when the value needs no wrapper.
    fn static_value(&mut self, value: Option<NodeId>, class_ref: &[Piece], _group: &mut Vec<Vec<Piece>>, name: Option<NodeId>, holder: Option<&str>) -> Option<Vec<Piece>> {
        let value = value?;
        let (s, e) = self.span(value);
        // A named class expression's own name is the temporary out here.
        if let (Some(n), Some(t)) = (name, holder) {
            self.rename_class_name(value, n, t);
        }
        if self.uses_super(value) {
            let target = self.bundler.target.name();
            self.lower_error_message(value, format!("a static field that uses super cannot be rewritten for the target {target} yet"));
            return None;
        }
        if !self.uses_this(value) {
            return None;
        }
        let mut pieces = vec![text("(function () { return ("), Piece::Range(s, e), text(") }).call(")];
        pieces.extend(class_ref.to_vec());
        pieces.push(text(")"));
        Some(pieces)
    }

    /// In the value of a static field of a class expression with a name, that name is the temporary that holds the class.
    fn rename_class_name(&mut self, root: NodeId, class_name: NodeId, temp: &str) {
        let Some(symbol) = self.semantic.symbol_of(class_name) else { return };
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if matches!(self.ast[node].kind, NodeKind::Ident(_)) && self.semantic.symbol_of(node) == Some(symbol) {
                let (s, e) = self.span(node);
                self.edits.push(Edit { start: s, end: e, pieces: vec![text(temp)], shorthand: None, transparent: false });
            }
            self.ast.for_each_child(node, |c| stack.push(c));
        }
    }

    /// Whether `this` (or `arguments`-free `new.target`) is read in an expression outside any function that has a `this` of its own.
    fn uses_this(&self, root: NodeId) -> bool {
        self.find_in_scope(root, |kind| matches!(kind, NodeKind::This | NodeKind::NewTarget))
    }

    fn uses_super(&self, root: NodeId) -> bool {
        // `super` of a method body is the method's own; only an arrow function or the initializer itself reaches the class's.
        self.find_in_scope(root, |kind| matches!(kind, NodeKind::Super))
    }

    /// Whether a node matches in an expression, not looking into functions that have their own `this` (or classes).
    fn find_in_scope(&self, root: NodeId, test: impl Fn(&NodeKind) -> bool) -> bool {
        let ast = self.ast;
        let mut stack = vec![(root, true)];
        while let Some((node, first)) = stack.pop() {
            let kind = ast[node].kind;
            if test(&kind) {
                return true;
            }
            // The root of a method is its own function: its body is looked at; a function inside is not (unless an arrow).
            let is_function = matches!(kind, NodeKind::Function { .. });
            let arrow = ast[node].flags & crate::ast::flags::ARROW != 0;
            if is_function && !arrow && !first {
                continue;
            }
            if matches!(kind, NodeKind::Class { .. }) && !first {
                continue;
            }
            ast.for_each_child(node, |c| stack.push((c, false)));
        }
        false
    }

    /// The `{` that opens a class body.
    fn class_body_open(&self, class: NodeId, name: Option<NodeId>, superclass: Option<NodeId>) -> Option<u32> {
        let ast = self.ast;
        let from = [name.map(|n| ast[n].span.end), superclass.map(|s| ast[s].span.end), Some(ast[class].span.start + 5)].into_iter().flatten().max()?;
        let end = ast[class].span.end;
        let bytes = self.module.source.as_bytes();
        let mut at = from;
        while at < end {
            // Text that TypeScript erases (`implements X`, type arguments) is not looked at.
            if let Some(span) = ast.erased_within(at, at + 1).first().copied().or_else(|| ast.erased_within(0, end).iter().copied().find(|s| s.start <= at && at < s.end)) {
                at = span.end.max(at + 1);
                continue;
            }
            if bytes[at as usize] == b'{' {
                return Some(at);
            }
            at += 1;
        }
        None
    }

    /// Makes the constructor run `inits` once the object exists: after `super()` in a derived class, at the start otherwise (and writes a
    /// constructor when the class has none).
    fn inject_constructor(&mut self, class: NodeId, name: Option<NodeId>, superclass: Option<NodeId>, members: &[NodeId], inits: &[Vec<Piece>]) {
        let ast = self.ast;
        let target = self.bundler.target.name();
        let constructor = members.iter().copied().find_map(|m| match ast[m].kind {
            NodeKind::ClassMethod { value, kind: MethodKind::Constructor, .. } => Some(value),
            _ => None,
        });
        let list = |sep: &str| -> Vec<Piece> {
            let mut out = Vec::new();
            for (i, init) in inits.iter().enumerate() {
                if i > 0 {
                    out.push(text(sep));
                }
                out.extend(init.clone());
            }
            out
        };
        match constructor {
            Some(value) => {
                let NodeKind::Function { params, body, .. } = ast[value].kind else { return };
                self.check_constructor_names(params, inits, members);
                if superclass.is_some() {
                    let calls = self.super_calls(body);
                    if calls.is_empty() {
                        self.lower_error_message(class, format!("a constructor that does not call super() cannot get class fields for the target {target} yet"));
                        return;
                    }
                    for call in calls {
                        let (s, e) = self.span(call);
                        let mut pieces = vec![text("("), Piece::Range(s, e), text(", ")];
                        pieces.extend(list(", "));
                        pieces.push(text(", this)"));
                        self.edits.push(Edit { start: s, end: e, pieces, shorthand: None, transparent: false });
                    }
                } else {
                    let NodeKind::Block(statements) = ast[body].kind else { return };
                    let mut at = self.span(body).0 + 1;
                    for &statement in ast.list(statements) {
                        match ast[statement].kind {
                            NodeKind::ExprStmt(_) if ast[statement].flags & crate::ast::flags::DIRECTIVE != 0 => at = ast[statement].span.end,
                            _ => break,
                        }
                    }
                    let mut pieces = vec![text(" ")];
                    pieces.extend(list("; "));
                    pieces.push(text("; "));
                    self.edits.push(Edit { start: at, end: at, pieces, shorthand: None, transparent: false });
                }
            }
            None => {
                let Some(open) = self.class_body_open(class, name, superclass) else {
                    self.lower_error_message(class, "the body of a class could not be found".to_string());
                    return;
                };
                let mut pieces = vec![text(if superclass.is_some() { " constructor() { super(...arguments); " } else { " constructor() { " })];
                pieces.extend(list("; "));
                pieces.push(text("; } "));
                self.edits.push(Edit { start: open + 1, end: open + 1, pieces, shorthand: None, transparent: false });
            }
        }
    }

    /// The `super(...)` calls of a constructor body (not those of functions with a `this` of their own).
    fn super_calls(&self, body: NodeId) -> Vec<NodeId> {
        let ast = self.ast;
        let mut found = Vec::new();
        let mut stack = vec![(body, true)];
        while let Some((node, first)) = stack.pop() {
            if let NodeKind::Call { callee, .. } = ast[node].kind {
                if matches!(ast[callee].kind, NodeKind::Super) {
                    found.push(node);
                }
            }
            let is_function = matches!(ast[node].kind, NodeKind::Function { .. });
            if (is_function && ast[node].flags & crate::ast::flags::ARROW == 0 && !first) || (matches!(ast[node].kind, NodeKind::Class { .. }) && !first) {
                continue;
            }
            ast.for_each_child(node, |c| stack.push((c, false)));
        }
        found.sort_by_key(|&n| ast[n].span.start);
        found
    }

    /// A field's initializer runs inside the constructor now, where the constructor's own parameters could hide a name it uses.
    fn check_constructor_names(&mut self, params: List, _inits: &[Vec<Piece>], members: &[NodeId]) {
        let ast = self.ast;
        let mut names: Vec<String> = Vec::new();
        let mut stack: Vec<NodeId> = ast.list(params).to_vec();
        while let Some(node) = stack.pop() {
            if let NodeKind::BindingIdent(a) = ast[node].kind {
                names.push(ast.atom(a).to_string());
            }
            ast.for_each_child(node, |c| stack.push(c));
        }
        if names.is_empty() {
            return;
        }
        for &m in members {
            let NodeKind::ClassField { value: Some(value), .. } = ast[m].kind else { continue };
            if ast[m].has(flags::STATIC) {
                continue;
            }
            let mut stack = vec![value];
            while let Some(node) = stack.pop() {
                if let NodeKind::Ident(a) = ast[node].kind {
                    if self.semantic.symbol_of(node).is_none_or(|s| self.semantic.symbol(s).scope == self.semantic.root_scope()) && names.iter().any(|n| n == ast.atom(a)) {
                        let target = self.bundler.target.name();
                        let word = ast.atom(a).to_string();
                        self.lower_error_message(m, format!("a field initializer uses the name {word}, which a parameter of the constructor would hide once the field is rewritten for the target {target}"));
                        return;
                    }
                }
                ast.for_each_child(node, |c| stack.push(c));
            }
        }
    }

    // ---- uses of private names ----

    /// Rewrites every use of a private name that an enclosing class declares, in the subtree of `node`.
    pub(super) fn rewrite_privates(&mut self, node: NodeId, is_class_root: bool) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Class { members, .. } => {
                // A class inside: the names it declares hide the outer ones for what is in it.
                if !is_class_root {
                    let mut scope = PrivateScope::default();
                    for &m in ast.list(members) {
                        let key = match ast[m].kind {
                            NodeKind::ClassField { key, .. } | NodeKind::ClassMethod { key, .. } => key,
                            _ => continue,
                        };
                        if self.is_private_key(key) {
                            scope.names.insert(self.private_text(key), None);
                        }
                    }
                    self.private_scopes.push(scope);
                    self.visit_children_for_privates(node);
                    self.private_scopes.pop();
                    return;
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Chain(inner) => {
                // The private links of a chain are noted for the chain's own rewrite.
                let mut at = inner;
                loop {
                    match ast[at].kind {
                        NodeKind::Member { object, .. } => {
                            if let Some(info) = self.private_of_member(at) {
                                self.private_refs.insert(at, info);
                                self.chain_members.insert(at);
                            }
                            at = object;
                        }
                        NodeKind::Call { callee, .. } => at = callee,
                        _ => break,
                    }
                    if matches!(ast[at].kind, NodeKind::Chain(_)) {
                        break;
                    }
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Member { object, prop } if self.is_private_key(prop) && !self.chain_members.contains(&node) => {
                if let Some(info) = self.private_of_member(node) {
                    let (os, oe) = self.span(object);
                    let pieces = self.private_read(&info, vec![Piece::Range(os, oe)]);
                    self.push(node, pieces);
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Binary { op: BinOp::In, left, right } if self.is_private_key(left) => {
                if let Some(info) = self.resolve_private(&self.private_text(left)) {
                    let (start, end) = self.span(node);
                    let after_left = self.span(left).1;
                    let at = self.source_between(after_left, self.span(right).0.max(after_left)).find("in").map_or(after_left, |p| after_left + p as u32 + 2);
                    let _ = start;
                    self.push(node, vec![text(&format!("__privateIn({}, ", info.brand)), Piece::Range(at, end), text(")")]);
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Assign { op, target, value } => {
                let private_target = matches!(ast[target].kind, NodeKind::Member { .. }) && self.private_of_member(target).is_some() && !self.chain_members.contains(&target);
                if private_target {
                    let info = self.private_of_member(target).unwrap();
                    let NodeKind::Member { object, .. } = ast[target].kind else { return };
                    let (ts, te) = self.span(target);
                    let _ = (ts, te);
                    if op == AssignOp::Assign {
                        let (os, oe) = self.span(object);
                        let (vs, ve) = self.span(value);
                        let mut pieces = vec![text("__privateSet(")];
                        pieces.push(Piece::Range(os, oe));
                        let value_text = if ast[value].flags & crate::ast::flags::PARENTHESIZED != 0 || matches!(ast[value].kind, NodeKind::Sequence(_)) { vec![text("("), Piece::Range(vs, ve), text(")")] } else { vec![Piece::Range(vs, ve)] };
                        pieces.push(text(&format!(", {}, ", info.brand)));
                        pieces.extend(value_text);
                        if let Some(setter) = &info.setter {
                            pieces.push(text(&format!(", {setter}")));
                        }
                        pieces.push(text(")"));
                        self.push(node, pieces);
                        self.chain_members.insert(target);
                    } else {
                        // `+=`, `||=` and the others: the operator is the lowering's when the target has none of it, else a wrapper.
                        let lowered = match op {
                            AssignOp::Op(BinOp::LogAnd | BinOp::LogOr | BinOp::Nullish) => self.lowers(Feature::LogicalAssignment),
                            AssignOp::Op(BinOp::Exp) => self.lowers(Feature::Exponent),
                            _ => false,
                        };
                        if !lowered {
                            let (os, oe) = self.span(object);
                            let pieces = self.private_wrapper(&info, vec![Piece::Range(os, oe)]);
                            self.push(target, pieces);
                        }
                        self.chain_members.insert(target);
                    }
                } else if matches!(ast[target].kind, NodeKind::ArrayPattern(_) | NodeKind::ObjectPattern(_)) {
                    self.pattern_targets(target);
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Update { arg, .. } => {
                if matches!(ast[arg].kind, NodeKind::Member { .. }) && self.private_of_member(arg).is_some() && !self.chain_members.contains(&arg) {
                    let info = self.private_of_member(arg).unwrap();
                    let NodeKind::Member { object, .. } = ast[arg].kind else { return };
                    let (os, oe) = self.span(object);
                    let pieces = self.private_wrapper(&info, vec![Piece::Range(os, oe)]);
                    self.push(arg, pieces);
                    self.chain_members.insert(arg);
                }
                self.visit_children_for_privates(node);
            }
            NodeKind::Call { callee, args } if matches!(ast[callee].kind, NodeKind::Member { .. }) && self.private_of_member(callee).is_some() && !self.chain_members.contains(&callee) && ast[node].flags & crate::ast::flags::OPTIONAL == 0 => {
                // A call keeps `this`: `o.#m(a)` is `__privateMethod(o, brand, fn).call(o, a)`.
                let info = self.private_of_member(callee).unwrap();
                let NodeKind::Member { object, .. } = ast[callee].kind else { return };
                let (first, again) = self.object_forms(object);
                let mut pieces = self.private_read(&info, first);
                pieces.push(text(".call("));
                pieces.extend(again);
                let (_, end) = self.span(node);
                let after_callee = self.span(callee).1;
                let open = self.source_between(after_callee, end).find('(').map_or(after_callee, |p| after_callee + p as u32 + 1);
                if !ast.list(args).is_empty() {
                    pieces.push(text(", "));
                    pieces.push(Piece::Range(open, end.saturating_sub(1)));
                }
                pieces.push(text(")"));
                self.push(node, pieces);
                self.chain_members.insert(callee);
                self.visit_children_for_privates(node);
            }
            NodeKind::TaggedTemplate { tag, .. } if matches!(ast[tag].kind, NodeKind::Member { .. }) && self.private_of_member(tag).is_some() => {
                let target = self.bundler.target.name();
                self.lower_error_message(node, format!("a tagged template of a private member cannot be rewritten for the target {target} yet"));
            }
            NodeKind::ForOf { left, .. } | NodeKind::ForIn { left, .. } if !matches!(ast[left].kind, NodeKind::VarDecl { .. }) => {
                self.pattern_targets(left);
                self.visit_children_for_privates(node);
            }
            _ => self.visit_children_for_privates(node),
        }
    }

    fn visit_children_for_privates(&mut self, node: NodeId) {
        let mut children = Vec::new();
        self.ast.for_each_child(node, |c| children.push(c));
        for child in children {
            self.rewrite_privates(child, false);
        }
    }

    /// The private members that a destructuring assigns to are wrappers (`[this.#a, { b: this.#b }] = ...`).
    fn pattern_targets(&mut self, node: NodeId) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::ArrayPattern(list) => {
                for &element in ast.list(list) {
                    self.pattern_targets(element);
                }
            }
            NodeKind::ObjectPattern(list) => {
                for &p in ast.list(list) {
                    match ast[p].kind {
                        NodeKind::PatternProp { value, .. } => self.pattern_targets(value),
                        NodeKind::Rest(target) => self.pattern_targets(target),
                        _ => {}
                    }
                }
            }
            NodeKind::AssignPattern { target, .. } | NodeKind::Rest(target) => self.pattern_targets(target),
            NodeKind::Member { object, .. } if self.private_of_member(node).is_some() && !self.chain_members.contains(&node) => {
                let info = self.private_of_member(node).unwrap();
                let (os, oe) = self.span(object);
                let pieces = self.private_wrapper(&info, vec![Piece::Range(os, oe)]);
                self.push(node, pieces);
                self.chain_members.insert(node);
            }
            _ => {}
        }
    }
}
