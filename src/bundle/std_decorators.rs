//! Standard (TC39) decorators and `accessor` members, lowered to plain JavaScript.
//!
//! The shape follows what the TypeScript compiler writes for the same input, so the two
//! behave the same at run time:
//!
//! ```js
//! (() => {
//!     let _classDecorators = [dec];             // lists, hoisted out of the class
//!     let _m_decorators;
//!     var C = class {
//!         static { _classThis = this; }
//!         static { /* decorator lists, __esDecorate for each member, then the class */ }
//!         field = (__runInitializers(this, _prev), __runInitializers(this, _field_initializers, 1));
//!         static { __runInitializers(_classThis, _classExtraInitializers); }
//!     };
//!     return C = _classThis;
//! })()
//! ```
//!
//! Decorator expressions are read back from the source, so renames and TypeScript erasure
//! inside them still apply. An undecorated `accessor` member is lowered on its own.

use super::*;
use crate::ast::{MethodKind, Span};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Method,
    Getter,
    Setter,
    Accessor,
    Field,
}

struct Member {
    node: NodeId,
    kind: Kind,
    is_static: bool,
    is_private: bool,
    /// `name`, or `#name`: what the context reports. Empty for a computed key.
    name: String,
    /// For a computed key, the variable that holds the key once it is evaluated.
    temp: Option<String>,
    decorators: Vec<crate::ast::Decorator>,
    /// The base of this member's variable names: `_static_sfield`.
    var: String,
    /// The private field behind an `accessor` member.
    storage: String,
}

impl Member {
    fn decorated(&self) -> bool {
        !self.decorators.is_empty()
    }

    fn holds_value(&self) -> bool {
        matches!(self.kind, Kind::Field | Kind::Accessor)
    }
}

/// The name a lowered class is bound to inside its wrapper.
pub(super) enum Binding {
    /// A top-level class of an ES module, named globally in the bundle.
    Key(Key),
    /// A class nested in other code, or in a CommonJS module, where names are not renamed.
    /// `declares` when the lowered text replaces a declaration and so has to bind the name itself.
    Local { name: String, declares: bool },
}

impl Binding {
    fn piece(&self) -> Piece {
        match self {
            Binding::Key(key) => Piece::Name(key.clone()),
            Binding::Local { name, .. } => Piece::Text(name.clone()),
        }
    }

    fn declares(&self) -> bool {
        matches!(self, Binding::Local { declares: true, .. })
    }
}

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

fn insertion(at: u32, pieces: Vec<Piece>) -> Edit {
    Edit { start: at, end: at, pieces, shorthand: None, transparent: false }
}

fn replacement(span: Span, pieces: Vec<Piece>) -> Edit {
    Edit { start: span.start, end: span.end, pieces, shorthand: None, transparent: false }
}

impl Scanner<'_, '_> {
    /// Whether the class has anything this module lowers.
    pub(super) fn needs_standard_lowering(&self, class: NodeId) -> bool {
        let ast = self.ast;
        if !ast.decorators_of(class).is_empty() {
            return true;
        }
        let NodeKind::Class { members, .. } = ast[class].kind else { return false };
        ast.list(members).iter().any(|&m| !ast.decorators_of(m).is_empty() || (matches!(ast[m].kind, NodeKind::ClassField { .. }) && ast[m].has(flags::ACCESSOR)))
    }

    /// Records the name an anonymous class expression is given by where it stands: the
    /// variable, assignment target, property or field it initializes.
    pub(super) fn note_name_hint(&mut self, node: NodeId) {
        let ast = self.ast;
        let key_name = |this: &Self, key: NodeId, computed: bool| -> Option<String> {
            match ast[key].kind {
                NodeKind::PropName(a) | NodeKind::Str(a) if !computed => Some(ast.atom(a).to_string()),
                NodeKind::PrivateName(_) => {
                    let s = ast[key].span;
                    Some(this.module.source[s.start as usize..s.end as usize].to_string())
                }
                _ => None,
            }
        };
        let (name, value) = match ast[node].kind {
            NodeKind::VarDeclarator { target, init: Some(init) } | NodeKind::AssignPattern { target, default: init } => match ast[target].kind {
                NodeKind::BindingIdent(a) => (ast.atom(a).to_string(), init),
                _ => return,
            },
            NodeKind::Assign { op: crate::ast::AssignOp::Assign, target, value } => match ast[target].kind {
                NodeKind::Ident(a) => (ast.atom(a).to_string(), value),
                _ => return,
            },
            NodeKind::Property { key, value, kind: crate::ast::PropKind::Init } => match key_name(self, key, ast[node].has(flags::COMPUTED)) {
                Some(n) => (n, value),
                None => return,
            },
            NodeKind::ClassField { key, value: Some(value) } => match key_name(self, key, ast[node].has(flags::COMPUTED)) {
                Some(n) => (n, value),
                None => return,
            },
            _ => return,
        };
        if matches!(ast[value].kind, NodeKind::Class { name: None, .. }) {
            self.name_hint = Some((value, name));
        }
    }

    /// A decorated class met while walking, rather than as a top-level statement of an ES
    /// module: nested in a function or block, an expression, or in a CommonJS module.
    pub(super) fn lower_nested_class(&mut self, class: NodeId) {
        let ast = self.ast;
        let NodeKind::Class { name, .. } = ast[class].kind else { return };
        let declares = name.is_some() && ast[class].has(flags::DECL);
        let name = match name {
            Some(n) => {
                let s = ast[n].span;
                self.module.source[s.start as usize..s.end as usize].to_string()
            }
            None => {
                // Nothing names it, so the wrapper needs a name that no code here uses.
                let mut candidate = "_class".to_string();
                let mut n = 2;
                while self.module.source.contains(&candidate) {
                    candidate = format!("_class{n}");
                    n += 1;
                }
                candidate
            }
        };
        let inferred = match self.name_hint.take() {
            Some((node, name)) if node == class => Some(name),
            _ => None,
        };
        self.standard_lowering(class, &Binding::Local { name, declares }, inferred);
    }

    fn std_error(&mut self, message: &str, node: NodeId) {
        self.bundler.errors.push(Diagnostic {
            severity: crate::graph::Severity::Error,
            message: message.to_string(),
            file: Some(self.module.path.clone()),
            pos: Some(self.ast[node].span.start),
        });
    }

    /// The decorated members and the accessors, in source order, or `None` after an error.
    fn collect_members(&mut self, class: NodeId) -> Option<Vec<Member>> {
        let ast = self.ast;
        let NodeKind::Class { members, .. } = ast[class].kind else { return None };
        let mut out: Vec<Member> = Vec::new();
        let mut used: HashSet<String> = HashSet::new();
        let mut temps = 0;
        for &m in ast.list(members) {
            let (key, kind) = match ast[m].kind {
                NodeKind::ClassMethod { key, kind: MethodKind::Method, .. } => (key, Kind::Method),
                NodeKind::ClassMethod { key, kind: MethodKind::Get, .. } => (key, Kind::Getter),
                NodeKind::ClassMethod { key, kind: MethodKind::Set, .. } => (key, Kind::Setter),
                NodeKind::ClassField { key, .. } => (key, if ast[m].has(flags::ACCESSOR) { Kind::Accessor } else { Kind::Field }),
                _ => continue,
            };
            let decorators = ast.decorators_of(m).to_vec();
            if decorators.is_empty() && kind != Kind::Accessor {
                continue;
            }
            let computed = ast[m].has(flags::COMPUTED);
            let (name, is_private) = match ast[key].kind {
                _ if computed => (String::new(), false),
                NodeKind::PropName(a) | NodeKind::Str(a) => (ast.atom(a).to_string(), false),
                NodeKind::Num(n) => (number_text(n.get()), false),
                NodeKind::PrivateName(_) => {
                    let s = ast[key].span;
                    (self.module.source[s.start as usize..s.end as usize].to_string(), true)
                }
                _ => {
                    self.std_error("This kind of member name is not supported with decorators yet", m);
                    return None;
                }
            };
            let temp = if computed {
                // A name that no code in the file uses, since the temporaries live next to it.
                temps += 1;
                let mut candidate = format!("_key{temps}");
                while self.module.source.contains(&candidate) {
                    temps += 1;
                    candidate = format!("_key{temps}");
                }
                Some(candidate)
            } else {
                None
            };
            let is_static = ast[m].has(flags::STATIC);
            let ident: String = if computed { "member".to_string() } else { name.trim_start_matches('#').chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '$' { c } else { '_' }).collect() };
            let prefix = format!(
                "_{}{}{}",
                if is_static { "static_" } else { "" },
                match kind {
                    Kind::Getter => "get_",
                    Kind::Setter => "set_",
                    _ => "",
                },
                if is_private { "private_" } else { "" }
            );
            let mut var = format!("{prefix}{ident}");
            let mut n = 2;
            while !used.insert(var.clone()) {
                var = format!("{prefix}{ident}{n}");
                n += 1;
            }
            let storage = if computed { format!("#{}_accessor_storage", var) } else { format!("#{}_accessor_storage", name.trim_start_matches('#')) };
            out.push(Member { node: m, kind, is_static, is_private, name, temp, decorators, var, storage });
        }
        Some(out)
    }

    /// What the context reports as the member's name.
    fn name_text(member: &Member) -> String {
        match &member.temp {
            Some(t) => t.clone(),
            None => json_string(&member.name),
        }
    }

    /// `{ has: obj => "m" in obj, get: obj => obj.m }`
    fn access_text(member: &Member) -> String {
        let name = &member.name;
        let read = |obj: &str| match &member.temp {
            Some(t) => format!("{obj}[{t}]"),
            None if member.is_private || is_identifier_name(name) => format!("{obj}.{name}"),
            None => format!("{obj}[{}]", json_string(name)),
        };
        let has = if member.is_private { format!("has: obj => {name} in obj") } else { format!("has: obj => {} in obj", Self::name_text(member)) };
        let get = format!("get: obj => {}", read("obj"));
        let set = format!("set: (obj, value) => {{ {} = value; }}", read("obj"));
        match member.kind {
            Kind::Method | Kind::Getter => format!("{{ {has}, {get} }}"),
            Kind::Setter => format!("{{ {has}, {set} }}"),
            Kind::Accessor | Kind::Field => format!("{{ {has}, {get}, {set} }}"),
        }
    }

    fn context_text(member: &Member) -> String {
        let kind = match member.kind {
            Kind::Method => "method",
            Kind::Getter => "getter",
            Kind::Setter => "setter",
            Kind::Accessor => "accessor",
            Kind::Field => "field",
        };
        format!(
            "{{ kind: \"{kind}\", name: {}, static: {}, private: {}, access: {}, metadata: _metadata }}",
            Self::name_text(member),
            member.is_static,
            member.is_private,
            Self::access_text(member)
        )
    }

    /// The offset just past the `]` that closes a computed key which ends at `from`.
    fn after_close_bracket(&self, from: u32) -> u32 {
        let bytes = self.module.source.as_bytes();
        let mut i = from as usize;
        while i < bytes.len() {
            match bytes[i] {
                b']' => return i as u32 + 1,
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        i += 1;
                    }
                    i += 2;
                }
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
        from
    }

    /// Rewrites the class in place and wraps it in the function that decorates it. `binding`
    /// is the name the statement gives the class, which the wrapper declares locally so that
    /// references to it inside the class see the decorated result.
    pub(super) fn standard_lowering(&mut self, class: NodeId, binding: &Binding, inferred: Option<String>) {
        let ast = self.ast;
        self.skip.insert(class);
        let NodeKind::Class { name: name_node, superclass, members } = ast[class].kind else { return };
        let Some(body) = ast.class_body(class) else {
            self.std_error("Decorators need TypeScript syntax", class);
            return;
        };
        let Some(list) = self.collect_members(class) else { return };
        let class_decorators = ast.decorators_of(class).to_vec();
        let has_class_decorators = !class_decorators.is_empty();
        let any_decorated = has_class_decorators || list.iter().any(Member::decorated);
        if any_decorated {
            self.bundler.helpers.std_decorators = true;
        }
        // The decorators are cut from the class text and read back inside the lowering.
        let mut removals: Vec<Span> = class_decorators.iter().map(|d| d.span).collect();
        for m in &list {
            removals.extend(m.decorators.iter().map(|d| d.span));
        }
        for span in removals {
            self.edits.push(Edit { start: span.start, end: span.end, pieces: Vec::new(), shorthand: None, transparent: true });
        }
        let static_this = if has_class_decorators { "_classThis" } else { "this" };
        let has_instance_extras = list.iter().any(|m| m.decorated() && !m.is_static && !m.holds_value());
        let has_static_extras = list.iter().any(|m| m.decorated() && m.is_static && !m.holds_value());
        // What runs before the next field's initializer: extra initializers of what came before.
        let mut pending_instance: Vec<String> = if has_instance_extras { vec!["_instanceExtraInitializers".to_string()] } else { Vec::new() };
        let mut pending_static: Vec<String> = if has_static_extras { vec!["_staticExtraInitializers".to_string()] } else { Vec::new() };
        // Variables that hold the descriptor of a private member while it is decorated.
        let mut private_methods: Vec<String> = Vec::new();
        // Decorator lists are assigned in the first computed key that follows them, so that
        // decorators and keys are evaluated in the order they are written; any left over are
        // assigned at the start of the decoration block.
        let mut pending_lists: Vec<Vec<Piece>> = Vec::new();

        for &node in ast.list(members) {
            let span = ast[node].span;
            let info = list.iter().find(|m| m.node == node);
            let key = match ast[node].kind {
                NodeKind::ClassMethod { key, .. } | NodeKind::ClassField { key, .. } => key,
                _ => continue,
            };
            let computed = ast[node].has(flags::COMPUTED);
            let decorated = info.is_some_and(Member::decorated);
            let mut own: Option<Vec<Piece>> = None;
            if let Some(m) = info.filter(|m| m.decorated()) {
                let mut pieces = vec![text(&format!("{}_decorators = [", m.var))];
                for (i, d) in m.decorators.iter().enumerate() {
                    if i > 0 {
                        pieces.push(text(", "));
                    }
                    let s = ast[d.expr].span;
                    pieces.push(Piece::Range(s.start, s.end));
                }
                pieces.push(text("]"));
                own = Some(pieces);
            }
            // The key of a computed member is evaluated once, after the decorators before it.
            let mut key_value: Option<Vec<Piece>> = None;
            if computed && (info.is_some() || !pending_lists.is_empty()) {
                let k = ast[key].span;
                let mut parts: Vec<Vec<Piece>> = std::mem::take(&mut pending_lists);
                parts.extend(own.take());
                let evaluated = match info.and_then(|m| m.temp.as_ref().map(|t| (m, t))) {
                    Some((m, t)) if m.decorated() => vec![text(&format!("{t} = __propKey(")), Piece::Range(k.start, k.end), text(")")],
                    Some((_, t)) => vec![text(&format!("{t} = ")), Piece::Range(k.start, k.end)],
                    None => vec![Piece::Range(k.start, k.end)],
                };
                if parts.is_empty() {
                    key_value = Some(evaluated);
                } else {
                    let mut pieces = vec![text("(")];
                    for part in parts {
                        pieces.extend(part);
                        pieces.push(text(", "));
                    }
                    pieces.extend(evaluated);
                    pieces.push(text(")"));
                    key_value = Some(pieces);
                }
            } else if let Some(o) = own.take() {
                pending_lists.push(o);
            }
            let is_accessor = ast[node].has(flags::ACCESSOR);
            match ast[node].kind {
                NodeKind::ClassMethod { value, kind, .. } => {
                    if let Some(pieces) = &key_value {
                        let k = ast[key].span;
                        self.edits.push(replacement(k, pieces.clone()));
                    }
                    let Some(m) = info else { continue };
                    if m.is_private && m.decorated() {
                        // A private member becomes a getter or setter over a descriptor kept outside.
                        let _ = value;
                        private_methods.push(format!("{}_descriptor", m.var));
                        let k = ast[key].span;
                        let st = if m.is_static { "static " } else { "" };
                        let d = format!("{}_descriptor", m.var);
                        let pieces = match kind {
                            MethodKind::Set => vec![text(&format!("{st}set ")), Piece::Range(k.start, k.end), text(&format!("(value) {{ return {d}.set.call(this, value); }}"))],
                            MethodKind::Get => vec![text(&format!("{st}get ")), Piece::Range(k.start, k.end), text(&format!("() {{ return {d}.get.call(this); }}"))],
                            _ => vec![text(&format!("{st}get ")), Piece::Range(k.start, k.end), text(&format!("() {{ return {d}.value; }}"))],
                        };
                        self.edits.push(replacement(span, pieces));
                    }
                }
                NodeKind::ClassField { value, .. } => {
                    let is_static = ast[node].has(flags::STATIC);
                    let (pending, this_arg) = if is_static { (&mut pending_static, static_this) } else { (&mut pending_instance, "this") };
                    let before: Vec<String> = pending.drain(..).map(|p| format!("__runInitializers({this_arg}, {p})")).collect();
                    if !decorated && !is_accessor && before.is_empty() {
                        if let Some(pieces) = &key_value {
                            self.edits.push(replacement(ast[key].span, pieces.clone()));
                        }
                        continue;
                    }
                    let mut core: Vec<Piece> = Vec::new();
                    match (info, value) {
                        (Some(m), _) if decorated => {
                            core.push(text(&format!("__runInitializers({this_arg}, {}_initializers", m.var)));
                            core.push(text(", "));
                            match value {
                                Some(v) => core.push(Piece::Range(ast[v].span.start, ast[v].span.end)),
                                // The initializers still receive a value, which is undefined.
                                None => core.push(text("void 0")),
                            }
                            core.push(text(")"));
                            pending.push(format!("{}_extraInitializers", m.var));
                        }
                        (_, Some(v)) => {
                            let s = ast[v].span;
                            core.push(Piece::Range(s.start, s.end));
                        }
                        (_, None) => core.push(text("void 0")),
                    }
                    let mut init: Vec<Piece> = Vec::new();
                    if !before.is_empty() {
                        init.push(text("("));
                        for call in &before {
                            init.push(text(call));
                            init.push(text(", "));
                        }
                    }
                    init.extend(core);
                    if !before.is_empty() {
                        init.push(text(")"));
                    }
                    if is_accessor {
                        // `accessor x = v` is private storage with a getter and a setter.
                        let m = info.expect("accessors are collected");
                        let k = ast[key].span;
                        let storage = &m.storage;
                        let st = if m.is_static { "static " } else { "" };
                        // A computed key is evaluated in the getter's brackets and reused by the setter.
                        let (get_key, set_key): (Vec<Piece>, Vec<Piece>) = match (&m.temp, key_value) {
                            (Some(t), Some(value)) => {
                                let mut get_key = vec![text("[")];
                                get_key.extend(value);
                                get_key.push(text("]"));
                                (get_key, vec![text(&format!("[{t}]"))])
                            }
                            _ => (vec![Piece::Range(k.start, k.end)], vec![Piece::Range(k.start, k.end)]),
                        };
                        let mut pieces = vec![text(&format!("{st}{storage} = "))];
                        pieces.extend(init);
                        let (read, write) = if m.is_private && m.decorated() {
                            let d = format!("{}_descriptor", m.var);
                            private_methods.push(d.clone());
                            (format!("return {d}.get.call(this);"), format!("return {d}.set.call(this, value);"))
                        } else {
                            (format!("return this.{storage};"), format!("this.{storage} = value;"))
                        };
                        pieces.push(text(&format!(";\n{st}get ")));
                        pieces.extend(get_key);
                        pieces.push(text(&format!("() {{ {read} }}\n{st}set ")));
                        pieces.extend(set_key);
                        pieces.push(text(&format!("(value) {{ {write} }}")));
                        self.edits.push(replacement(span, pieces));
                    } else {
                        if let Some(pieces) = key_value {
                            self.edits.push(replacement(ast[key].span, pieces));
                        }
                        if let Some(v) = value {
                            self.edits.push(replacement(ast[v].span, init));
                        } else {
                            let k = ast[key].span;
                            let at = if computed { self.after_close_bracket(k.end) } else { k.end };
                            let mut pieces = vec![text(" = ")];
                            pieces.extend(init);
                            self.edits.push(insertion(at, pieces));
                        }
                    }
                }
                _ => {}
            }
        }

        // The constructor runs whatever instance initializers no field was left to run.
        if !pending_instance.is_empty() {
            self.add_constructor_runs(class, &pending_instance, superclass.is_some(), body);
        }
        // After the last member: static extras that no static field consumed, then the class's own.
        let mut tail_block = String::new();
        for p in &pending_static {
            tail_block.push_str(&format!("__runInitializers({static_this}, {p}); "));
        }
        if has_class_decorators {
            tail_block.push_str("__runInitializers(_classThis, _classExtraInitializers); ");
        }
        if !tail_block.is_empty() {
            self.edits.push(insertion(body.end, vec![text(&format!("static {{ {tail_block}}}\n"))]));
        }

        // The decoration itself, first in the body.
        let mut block: Vec<Piece> = Vec::new();
        // The name the class is known by: the one written, or what its surroundings give an
        // anonymous one. A rename of the binding must not change it.
        let written: Option<String> = match name_node {
            Some(n) => {
                let s = ast[n].span;
                self.module.source.get(s.start as usize..s.end as usize).map(str::to_string)
            }
            None if matches!(binding, Binding::Key(_)) => Some("default".to_string()),
            None => inferred,
        };
        if has_class_decorators {
            // The class expression is anonymous, since its own name could not be assigned to.
            if let Some(n) = name_node {
                self.edits.push(replacement(ast[n].span, Vec::new()));
            }
            self.bundler.helpers.std_decorators = true;
            block.push(text(&format!("static {{ _classThis = this; __setFunctionName(this, {}); }}\n", json_string(written.as_deref().unwrap_or("")))));
        } else if name_node.is_none() {
            if let Some(n) = &written {
                self.bundler.helpers.std_decorators = true;
                block.push(text(&format!("static {{ __setFunctionName(this, {}); }}\n", json_string(n))));
            }
        }
        if any_decorated {
            block.push(text("static {\nconst _metadata = typeof Symbol === \"function\" && Symbol.metadata ? Object.create(null) : void 0;\n"));
            for assignment in pending_lists {
                block.extend(assignment);
                block.push(text(";\n"));
            }
            // Statics that are not fields, then instance ones, then static fields, then instance fields.
            let groups: [fn(&Member) -> bool; 4] = [
                |m| m.is_static && m.kind != Kind::Field,
                |m| !m.is_static && m.kind != Kind::Field,
                |m| m.is_static && m.kind == Kind::Field,
                |m| !m.is_static && m.kind == Kind::Field,
            ];
            for group in groups {
                for m in list.iter().filter(|m| m.decorated() && group(m)) {
                    let ctx = Self::context_text(m);
                    let extras = if m.holds_value() {
                        format!("{}_extraInitializers", m.var)
                    } else if m.is_static {
                        "_staticExtraInitializers".to_string()
                    } else {
                        "_instanceExtraInitializers".to_string()
                    };
                    let inits = if m.holds_value() { format!("{}_initializers", m.var) } else { "null".to_string() };
                    if m.is_private && m.kind != Kind::Field {
                        let d = format!("{}_descriptor", m.var);
                        let name = json_string(&m.name);
                        block.push(text(&format!("__esDecorate(this, {d} = {{ ")));
                        match m.kind {
                            Kind::Accessor => {
                                block.push(text(&format!(
                                    "get: __setFunctionName(function () {{ return this.{0}; }}, {name}, \"get\"), set: __setFunctionName(function (value) {{ this.{0} = value; }}, {name}, \"set\")",
                                    m.storage
                                )));
                            }
                            kind => {
                                let NodeKind::ClassMethod { value, .. } = ast[m.node].kind else { continue };
                                let s = ast[value].span;
                                let word = match (ast[value].has(flags::ASYNC), ast[value].has(flags::GENERATOR)) {
                                    (true, true) => "async function* ",
                                    (true, false) => "async function ",
                                    (false, true) => "function* ",
                                    (false, false) => "function ",
                                };
                                let (slot, suffix) = match kind {
                                    Kind::Getter => ("get", format!(", \"get\"")),
                                    Kind::Setter => ("set", format!(", \"set\"")),
                                    _ => ("value", String::new()),
                                };
                                block.push(text(&format!("{slot}: __setFunctionName({word}")));
                                block.push(Piece::Range(s.start, s.end));
                                block.push(text(&format!(", {name}{suffix})")));
                            }
                        }
                        block.push(text(&format!(" }}, {}_decorators, {ctx}, {inits}, {extras});\n", m.var)));
                        continue;
                    }
                    let ctor = if m.kind == Kind::Field { "null" } else { "this" };
                    block.push(text(&format!("__esDecorate({ctor}, null, {}_decorators, {ctx}, {inits}, {extras});\n", m.var)));
                }
            }
            if has_class_decorators {
                block.push(text("__esDecorate(null, _classDescriptor = { value: _classThis }, _classDecorators, { kind: \"class\", name: _classThis.name, metadata: _metadata }, null, _classExtraInitializers);\n"));
                block.push(binding.piece());
                block.push(text(" = _classThis = _classDescriptor.value;\n"));
            }
            block.push(text(&format!("if (_metadata) Object.defineProperty({static_this}, Symbol.metadata, {{ enumerable: true, configurable: true, writable: true, value: _metadata }});\n}}\n")));
        }
        if !block.is_empty() {
            self.edits.push(insertion(body.start + 1, block));
        }

        // What wraps the class.
        let mut head: Vec<Piece> = Vec::new();
        let temps: Vec<&str> = list.iter().filter_map(|m| m.temp.as_deref()).collect();
        if binding.declares() {
            // A class declaration nested in a function or block: the statement binds the name.
            head.push(text("let "));
            head.push(binding.piece());
            head.push(text(" = "));
        }
        head.push(text("(() => {\n"));
        if !temps.is_empty() {
            head.push(text(&format!("var {};\n", temps.join(", "))));
        }
        if has_class_decorators {
            head.push(text("let _classDecorators = ["));
            for (i, d) in class_decorators.iter().enumerate() {
                if i > 0 {
                    head.push(text(", "));
                }
                let s = ast[d.expr].span;
                head.push(Piece::Range(s.start, s.end));
            }
            head.push(text("];\nlet _classDescriptor;\nlet _classExtraInitializers = [];\nlet _classThis;\n"));
        }
        if has_static_extras {
            head.push(text("let _staticExtraInitializers = [];\n"));
        }
        if has_instance_extras {
            head.push(text("let _instanceExtraInitializers = [];\n"));
        }
        for m in list.iter().filter(|m| m.decorated()) {
            head.push(text(&format!("let {}_decorators;\n", m.var)));
            if m.holds_value() {
                head.push(text(&format!("let {0}_initializers = [];\nlet {0}_extraInitializers = [];\n", m.var)));
            }
        }
        for d in &private_methods {
            head.push(text(&format!("let {d};\n")));
        }
        // A named class is held in a variable that its own references resolve to, and a decorated
        // one is rebound to what the decorators returned. An anonymous one needs no variable,
        // which would also give it a name.
        let mut tail: Vec<Piece> = Vec::new();
        if name_node.is_none() && !has_class_decorators {
            head.push(text("return "));
            tail.push(text(";\n})()"));
        } else {
            head.push(text("var "));
            head.push(binding.piece());
            head.push(text(" = "));
            tail.push(text(";\nreturn "));
            tail.push(binding.piece());
            if has_class_decorators {
                tail.push(text(" = _classThis"));
            }
            tail.push(text(if binding.declares() { ";\n})();" } else { ";\n})()" }));
        }
        let outer = ast[class].span;
        self.edits.push(insertion(outer.start, head));
        self.edits.push(insertion(outer.end, tail));
    }

    /// Inserts `__runInitializers(this, list)` for each name at the start of the constructor,
    /// after `super(...)` in a derived class, adding a constructor if there is none.
    fn add_constructor_runs(&mut self, class: NodeId, lists: &[String], derived: bool, body: Span) {
        let ast = self.ast;
        let NodeKind::Class { members, .. } = ast[class].kind else { return };
        let runs: String = lists.iter().map(|l| format!("\n__runInitializers(this, {l});")).collect();
        let constructor = ast.list(members).iter().find_map(|&m| match ast[m].kind {
            NodeKind::ClassMethod { value, kind: MethodKind::Constructor, .. } => Some(value),
            _ => None,
        });
        match constructor {
            Some(function) => {
                let NodeKind::Function { body: fb, .. } = ast[function].kind else { return };
                let NodeKind::Block(statements) = ast[fb].kind else { return };
                let super_call = ast.list(statements).iter().find(|&&s| match ast[s].kind {
                    NodeKind::ExprStmt(e) => matches!(ast[e].kind, NodeKind::Call { callee, .. } if matches!(ast[callee].kind, NodeKind::Super)),
                    _ => false,
                });
                let at = match super_call {
                    Some(&s) => ast[s].span.end,
                    None => ast[fb].span.start + 1,
                };
                self.edits.push(insertion(at, vec![text(&runs)]));
            }
            None => {
                let ctor = if derived { format!("constructor(...args) {{\nsuper(...args);{runs}\n}}\n") } else { format!("constructor() {{{runs}\n}}\n") };
                self.edits.push(insertion(body.start + 1, vec![text(&ctor)]));
            }
        }
    }
}
