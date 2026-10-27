//! React Fast Refresh for `--hmr`: what the Babel plugin of `react-refresh` does to a module, so that React keeps the state of a component
//! whose module is replaced.
//!
//! Each component (a capitalized function, or one wrapped by `memo`, `forwardRef` and the like) is registered by name with the
//! runtime at the end of the module (`$RefreshReg$(_c, "Name")`), and each function that calls hooks reports the list of them
//! (`_s(Name, "useState{[a, b](0)}")`) so the runtime can tell when the hooks changed, which resets the state. The runtime (the project's
//! own `react-refresh` package) and what a replaced module does with all this are in `registry.js`.

use super::*;

/// The text a module gets: declarations before its statements, registrations after.
pub(super) struct RefreshText {
    pub prefix: String,
    pub suffix: String,
}

const BUILTIN_HOOKS: &[&str] = &[
    "useState", "useReducer", "useEffect", "useLayoutEffect", "useMemo", "useCallback", "useRef", "useContext", "useImperativeHandle", "useDebugValue", "useId",
    "useDeferredValue", "useTransition", "useInsertionEffect", "useSyncExternalStore", "useFormStatus", "useFormState", "useActionState", "useOptimistic",
];

struct HookCall {
    name: String,
    key: String,
    callee: NodeId,
}

#[derive(Default)]
struct State {
    /// `_s`, `_s2`, ... with the text that calls them.
    signatures: usize,
    /// `_c`, `_c2`, ... with the name each is registered under.
    registrations: Vec<(String, String)>,
}

fn is_component_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(|b| b.is_ascii_uppercase())
}

fn is_hook_name(name: &str) -> bool {
    name.strip_prefix("use").is_some_and(|rest| rest.as_bytes().first().is_some_and(|b| b.is_ascii_uppercase()))
}

fn counted(prefix: &str, n: usize) -> String {
    if n == 1 { prefix.to_string() } else { format!("{prefix}{n}") }
}

impl Scanner<'_, '_> {
    /// Whether the module is one Fast Refresh looks at: one of the project's, which uses React.
    fn refreshable(&self) -> bool {
        let module = self.module;
        if module.path.components().any(|c| c.as_os_str() == "node_modules") {
            return false;
        }
        module.record.jsx_runtime.is_some() || module.record.requests.iter().any(|r| r.specifier == "react" || r.specifier.starts_with("react/") || r.specifier == "react-dom")
    }

    /// Registers the components of the module and reports the hooks of its functions. None when there is nothing to say.
    pub(super) fn react_refresh(&mut self, code: &mut EsmCode) -> Option<RefreshText> {
        if !self.refreshable() {
            return None;
        }
        let ast = self.ast;
        let NodeKind::Program(body) = ast[ast.root].kind else { return None };
        let mut state = State::default();
        let force_reset = self.module.source.contains("@refresh reset");
        for &stmt in ast.list(body) {
            let decl = match ast[stmt].kind {
                NodeKind::ExportNamed(export) => ast.export(export).decl.unwrap_or(stmt),
                NodeKind::ExportDefault(value) => value,
                _ => stmt,
            };
            let (start, end) = self.span(decl);
            let Some(index) = code.stmts.iter().position(|s| s.start == start && s.end == end) else { continue };
            match ast[decl].kind {
                NodeKind::Function { name: Some(name), body: fbody, .. } => {
                    let NodeKind::BindingIdent(atom) = ast[name].kind else { continue };
                    let text = ast.atom(atom).to_string();
                    let signature = self.signature_for_declaration(decl, fbody, &text, &mut state, force_reset, &mut code.stmts[index]);
                    let stmt = &mut code.stmts[index];
                    if is_component_name(&text) {
                        let handle = self.register(&mut state, &text);
                        stmt.trailer.push(Piece::Text(format!("\n{handle} = {text};")));
                    }
                    if let Some(call) = signature {
                        stmt.trailer.insert(0, Piece::Text(format!("\n{call}")));
                    }
                }
                NodeKind::VarDecl { decls, .. } => {
                    for &declarator in ast.list(decls) {
                        let NodeKind::VarDeclarator { target, init: Some(init) } = ast[declarator].kind else { continue };
                        let NodeKind::BindingIdent(atom) = ast[target].kind else { continue };
                        let text = ast.atom(atom).to_string();
                        self.declarator(&text, init, &mut state, force_reset, &mut code.stmts[index]);
                    }
                }
                _ => {}
            }
        }
        if state.signatures == 0 && state.registrations.is_empty() {
            return None;
        }
        let mut prefix = String::new();
        if state.signatures > 0 {
            let list: Vec<String> = (1..=state.signatures).map(|n| format!("{} = $RefreshSig$()", counted("_s", n))).collect();
            prefix.push_str(&format!("var {};\n", list.join(", ")));
        }
        if !state.registrations.is_empty() {
            prefix.push_str(&format!("var {};\n", state.registrations.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>().join(", ")));
        }
        let suffix: String = state.registrations.iter().map(|(handle, name)| format!("\n$RefreshReg$({handle}, {});", json_string(name))).collect();
        Some(RefreshText { prefix, suffix })
    }

    fn register(&self, state: &mut State, name: &str) -> String {
        let handle = counted("_c", state.registrations.len() + 1);
        state.registrations.push((handle.clone(), name.to_string()));
        handle
    }

    /// `const Name = <init>`: a function, or calls (`memo`, `forwardRef`) around one, registered under the name and its layers' names.
    fn declarator(&mut self, name: &str, init: NodeId, state: &mut State, force_reset: bool, stmt: &mut Stmt) {
        let ast = self.ast;
        let component = is_component_name(name);
        match ast[init].kind {
            NodeKind::Function { .. } => {
                self.wrap_function(init, state, force_reset, stmt, component.then_some(name), name);
            }
            NodeKind::Call { .. } if component => {
                let _ = self.higher_order(name, init, state, force_reset, stmt);
            }
            _ => {}
        }
    }

    /// A call whose first argument is a component, or another such call: each layer is registered. False when it is not.
    fn higher_order(&mut self, inferred: &str, node: NodeId, state: &mut State, force_reset: bool, stmt: &mut Stmt) -> bool {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::Function { .. } => {
                if ast[node].has(flags::ARROW) && !self.has_block_body(node) {
                    return false;
                }
                self.wrap_function(node, state, force_reset, stmt, Some(inferred), inferred);
                true
            }
            NodeKind::Call { callee, args } => {
                let Some(&first) = ast.list(args).first() else { return false };
                if !matches!(ast[callee].kind, NodeKind::Ident(_) | NodeKind::Member { .. }) {
                    return false;
                }
                let (cs, ce) = self.span(callee);
                let callee_text = self.source_between(cs, ce).to_string();
                if !self.higher_order(&format!("{inferred}${callee_text}"), first, state, force_reset, stmt) {
                    return false;
                }
                let handle = self.register(state, inferred);
                let (start, _) = self.span(node);
                stmt.edits.push(Edit { start, end: start, pieces: vec![Piece::Text(format!("{handle} = "))], shorthand: None, transparent: false });
                true
            }
            NodeKind::Ident(atom) => is_component_name(ast.atom(atom)),
            _ => false,
        }
    }

    fn has_block_body(&self, function: NodeId) -> bool {
        matches!(self.ast[function].kind, NodeKind::Function { body, .. } if matches!(self.ast[body].kind, NodeKind::Block(_)))
    }

    /// A function expression: registered if it is a component, and wrapped in its signature call if it uses hooks.
    fn wrap_function(&mut self, function: NodeId, state: &mut State, force_reset: bool, stmt: &mut Stmt, register_as: Option<&str>, _inferred: &str) {
        let (start, end) = self.span(function);
        if let Some(name) = register_as {
            let handle = self.register(state, name);
            stmt.edits.push(Edit { start, end: start, pieces: vec![Piece::Text(format!("{handle} = "))], shorthand: None, transparent: false });
        }
        let NodeKind::Function { body, .. } = self.ast[function].kind else { return };
        if !matches!(self.ast[body].kind, NodeKind::Block(_)) {
            return;
        }
        let Some((key, customs, reset)) = self.hook_signature(function, body, force_reset) else { return };
        state.signatures += 1;
        let handle = counted("_s", state.signatures);
        let (bs, _) = self.span(body);
        stmt.edits.push(Edit { start, end: start, pieces: vec![Piece::Text(format!("{handle}("))], shorthand: None, transparent: false });
        stmt.edits.push(Edit { start: bs + 1, end: bs + 1, pieces: vec![Piece::Text(format!(" {handle}();"))], shorthand: None, transparent: false });
        stmt.edits.push(Edit { start: end, end, pieces: vec![Piece::Text(signature_arguments(&key, &customs, reset))], shorthand: None, transparent: false });
        // The wrapped function ends with the argument list and a closing parenthesis.
        stmt.edits.push(Edit { start: end, end, pieces: vec![Piece::Text(")".to_string())], shorthand: None, transparent: false });
    }

    /// A function declaration that uses hooks: `_s();` at the start of its body, and the call that reports them after it.
    fn signature_for_declaration(&mut self, function: NodeId, body: NodeId, name: &str, state: &mut State, force_reset: bool, stmt: &mut Stmt) -> Option<String> {
        if !matches!(self.ast[body].kind, NodeKind::Block(_)) {
            return None;
        }
        let (key, customs, reset) = self.hook_signature(function, body, force_reset)?;
        state.signatures += 1;
        let handle = counted("_s", state.signatures);
        let (bs, _) = self.span(body);
        stmt.edits.push(Edit { start: bs + 1, end: bs + 1, pieces: vec![Piece::Text(format!(" {handle}();"))], shorthand: None, transparent: false });
        Some(format!("{handle}({name}{});", signature_arguments(&key, &customs, reset)))
    }

    /// The hooks a function calls, as the runtime wants them: the key that changes when they do, the custom ones to ask about, and whether
    /// the state is to be reset whenever the function is replaced.
    fn hook_signature(&self, function: NodeId, body: NodeId, force_reset: bool) -> Option<(String, Vec<String>, bool)> {
        let mut calls = Vec::new();
        self.collect_hooks(body, None, &mut calls);
        let _ = function;
        if calls.is_empty() {
            return None;
        }
        let key = calls.iter().map(|c| format!("{}{{{}}}", c.name, c.key)).collect::<Vec<_>>().join("\n");
        let mut reset = force_reset;
        let mut customs = Vec::new();
        for call in calls.iter().filter(|c| !BUILTIN_HOOKS.contains(&c.name.as_str())) {
            let (s, e) = self.span(call.callee);
            let text = self.source_between(s, e).to_string();
            match self.ast[call.callee].kind {
                NodeKind::Ident(_) => customs.push(text),
                NodeKind::Member { object, .. } if matches!(self.ast[object].kind, NodeKind::Ident(_)) => customs.push(text),
                _ => reset = true,
            }
        }
        customs.dedup();
        Some((key, customs, reset))
    }

    fn collect_hooks(&self, node: NodeId, parent: Option<NodeId>, out: &mut Vec<HookCall>) {
        let ast = self.ast;
        if matches!(ast[node].kind, NodeKind::Function { .. }) {
            // A function of its own reports its own hooks.
            return;
        }
        if let NodeKind::Call { callee, args } = ast[node].kind {
            let name = match ast[callee].kind {
                NodeKind::Ident(a) => Some(ast.atom(a).to_string()),
                NodeKind::Member { prop, .. } => match ast[prop].kind {
                    NodeKind::PropName(a) => Some(ast.atom(a).to_string()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(name) = name.filter(|n| is_hook_name(n)) {
                let mut key = String::new();
                if let Some(NodeKind::VarDeclarator { target, .. }) = parent.map(|p| ast[p].kind) {
                    let (s, e) = self.span(target);
                    key = self.source_between(s, e).to_string();
                }
                let list = ast.list(args);
                let argument = |i: usize| -> Option<String> {
                    let (s, e) = self.span(*list.get(i)?);
                    Some(self.source_between(s, e).to_string())
                };
                if name == "useState" {
                    if let Some(a) = argument(0) {
                        key.push_str(&format!("({a})"));
                    }
                } else if name == "useReducer" && list.len() > 1 {
                    if let Some(a) = argument(1) {
                        key.push_str(&format!("({a})"));
                    }
                }
                out.push(HookCall { name, key, callee });
            }
        }
        let mut children = Vec::new();
        ast.for_each_child(node, |c| children.push(c));
        for child in children {
            self.collect_hooks(child, Some(node), out);
        }
    }
}

/// `, "key"` and, when needed, `, forceReset` and `, () => [customHook, ...]`.
fn signature_arguments(key: &str, customs: &[String], reset: bool) -> String {
    let mut out = format!(", {}", json_string(key));
    if reset || !customs.is_empty() {
        let _ = write!(out, ", {reset}");
    }
    if !customs.is_empty() {
        let _ = write!(out, ", () => [{}]", customs.join(", "));
    }
    out
}
