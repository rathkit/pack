//! ESTree output: the syntax tree as JSON in the shape every JavaScript tool reads.
//!
//! Plugins from other bundlers call `this.parse(code)` and expect an ESTree back, with `start`
//! and `end` offsets counted in UTF-16 code units. This writes that form straight from the
//! arena, so no other parser needs to be installed. Plain JavaScript only: TypeScript and JSX
//! have no ESTree form here.
//!
//! Some positions are not kept in the arena (the label of a `break`, the names inside an
//! import specifier, the braces of a class body). Those come from the recorded tokens.

use crate::ast::{Ast, Atom, List, MethodKind, NodeId, NodeKind, PropKind, UpdateOp, flags};
use crate::lexer::{Kind, Kw, Punct, Token};
use crate::parser::{ParseOptions, Parsed, parse};

#[derive(Debug)]
pub struct EstreeError {
    pub message: String,
    pub pos: u32,
}

/// Parses `src` and writes the ESTree. Source type: a module unless it only reads as a script
/// (or `script_first`, for callers that allow a top-level `return`).
pub fn to_json(src: &str, script_first: bool) -> Result<String, EstreeError> {
    let attempt = |module: bool| parse(src, ParseOptions::new(module).with_recorded_tokens());
    let (parsed, module) = if script_first {
        match attempt(false) {
            Ok(p) => (p, false),
            Err(_) => match attempt(true) {
                Ok(p) => (p, true),
                Err(e) => return Err(EstreeError { message: e.message.clone(), pos: e.pos }),
            },
        }
    } else {
        match attempt(true) {
            Ok(p) => (p, true),
            Err(module_error) => match attempt(false) {
                Ok(p) => (p, false),
                // Report the error that got further: it is the better guess at what the file is.
                Err(script_error) => {
                    let e = if script_error.pos > module_error.pos { script_error } else { module_error };
                    return Err(EstreeError { message: e.message.clone(), pos: e.pos });
                }
            },
        }
    };
    let mut writer = Writer::new(src, &parsed);
    writer.program(module)?;
    Ok(writer.out)
}

struct Writer<'a> {
    src: &'a str,
    ast: &'a Ast,
    tokens: &'a [Token],
    /// Byte offset to UTF-16 offset, only when the text is not all ASCII.
    utf16: Option<Vec<u32>>,
    out: String,
    error: Option<EstreeError>,
}

impl<'a> Writer<'a> {
    fn new(src: &'a str, parsed: &'a Parsed) -> Writer<'a> {
        let utf16 = if src.is_ascii() {
            None
        } else {
            let mut map = Vec::with_capacity(src.len() + 1);
            let mut unit = 0u32;
            for ch in src.chars() {
                for _ in 0..ch.len_utf8() {
                    map.push(unit);
                }
                unit += ch.len_utf16() as u32;
            }
            map.push(unit);
            Some(map)
        };
        Writer { src, ast: &parsed.ast, tokens: &parsed.tokens, utf16, out: String::with_capacity(src.len() * 8), error: None }
    }

    fn pos(&self, byte: u32) -> u32 {
        match &self.utf16 {
            Some(map) => map[byte as usize],
            None => byte,
        }
    }

    fn fail(&mut self, message: &str, at: u32) {
        if self.error.is_none() {
            self.error = Some(EstreeError { message: message.to_string(), pos: at });
        }
    }

    // ---- output helpers ------------------------------------------------------------------

    /// Starts an object: `{"type":..,"start":..,"end":..`.
    fn open(&mut self, ty: &str, start: u32, end: u32) {
        let (s, e) = (self.pos(start), self.pos(end));
        self.out.push_str("{\"type\":\"");
        self.out.push_str(ty);
        self.out.push_str("\",\"start\":");
        self.out.push_str(&s.to_string());
        self.out.push_str(",\"end\":");
        self.out.push_str(&e.to_string());
    }

    fn close(&mut self) {
        self.out.push('}');
    }

    fn key(&mut self, name: &str) {
        self.out.push_str(",\"");
        self.out.push_str(name);
        self.out.push_str("\":");
    }

    fn string(&mut self, text: &str) {
        self.out.push('"');
        for ch in text.chars() {
            match ch {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                c if (c as u32) < 0x20 => self.out.push_str(&format!("\\u{:04x}", c as u32)),
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }

    fn boolean(&mut self, name: &str, value: bool) {
        self.key(name);
        self.out.push_str(if value { "true" } else { "false" });
    }

    fn text(&mut self, name: &str, value: &str) {
        self.key(name);
        self.string(value);
    }

    fn atom_text(&mut self, name: &str, atom: Atom) {
        let value = self.ast.atom(atom).to_string();
        self.text(name, &value);
    }

    fn child(&mut self, name: &str, id: NodeId) {
        self.key(name);
        self.node(id);
    }

    fn child_opt(&mut self, name: &str, id: Option<NodeId>) {
        self.key(name);
        match id {
            Some(id) => self.node(id),
            None => self.out.push_str("null"),
        }
    }

    /// A list of nodes, a hole written as `null`.
    fn children(&mut self, name: &str, list: List) {
        self.key(name);
        self.node_list(list);
    }

    fn node_list(&mut self, list: List) {
        self.out.push('[');
        for (i, &id) in self.ast.list(list).iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.node(id);
        }
        self.out.push(']');
    }

    fn span(&self, id: NodeId) -> (u32, u32) {
        let s = self.ast.node(id).span;
        (s.start, s.end)
    }

    fn slice(&self, start: u32, end: u32) -> &'a str {
        &self.src[start as usize..end as usize]
    }

    // ---- token lookups -------------------------------------------------------------------

    /// Index of the first token that starts at or after `pos`.
    fn token_at(&self, pos: u32) -> usize {
        self.tokens.partition_point(|t| t.start < pos)
    }

    /// The tokens inside `start..end`.
    fn tokens_in(&self, start: u32, end: u32) -> &'a [Token] {
        let first = self.token_at(start);
        let last = self.tokens.partition_point(|t| t.start < end);
        &self.tokens[first..last.max(first)]
    }

    /// An identifier or string written as a module export name, as a node.
    fn module_name(&mut self, token: Token) {
        if token.kind == Kind::String {
            let raw = self.slice(token.start, token.end);
            let value = crate::literal::cook_string(&raw[1..raw.len() - 1], false).unwrap_or_default();
            self.open("Literal", token.start, token.end);
            self.text("value", &value);
            self.text("raw", raw);
            self.close();
        } else {
            self.identifier(token.start, token.end, self.slice(token.start, token.end));
        }
    }

    fn identifier(&mut self, start: u32, end: u32, name: &str) {
        self.open("Identifier", start, end);
        self.text("name", name);
        self.close();
    }

    /// The `from` source of a declaration: the first string token after `from` (or after the
    /// keyword for `import "m"`), within `start..end`.
    fn source_token(&self, start: u32, end: u32) -> Option<Token> {
        let tokens = self.tokens_in(start, end);
        for (i, t) in tokens.iter().enumerate() {
            if t.kind != Kind::String {
                continue;
            }
            let before = if i > 0 { Some(tokens[i - 1]) } else { None };
            let ok = match before {
                Some(b) => b.kind == Kind::Keyword(Kw::Import) || (b.kind == Kind::Ident && self.slice(b.start, b.end) == "from"),
                None => false,
            };
            if ok {
                return Some(*t);
            }
        }
        None
    }

    fn source_literal(&mut self, start: u32, end: u32) {
        match self.source_token(start, end) {
            Some(t) => self.module_name(t),
            None => {
                self.fail("missing module source", start);
                self.out.push_str("null");
            }
        }
    }

    /// `with { type: "json" }`, as `ImportAttribute` nodes.
    fn attributes(&mut self, attrs: List) {
        self.key("attributes");
        self.out.push('[');
        for (i, &id) in self.ast.list(attrs).iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            let (start, end) = self.span(id);
            let tokens = self.tokens_in(start, end);
            self.open("ImportAttribute", start, end);
            if let (Some(&k), Some(&v)) = (tokens.first(), tokens.last()) {
                self.child_token("key", k);
                self.child_token("value", v);
            }
            self.close();
        }
        self.out.push(']');
    }

    fn child_token(&mut self, name: &str, token: Token) {
        self.key(name);
        self.module_name(token);
    }

    // ---- program -------------------------------------------------------------------------

    fn program(&mut self, module: bool) -> Result<(), EstreeError> {
        let NodeKind::Program(body) = self.ast.node(self.ast.root).kind else { unreachable!() };
        self.open("Program", 0, self.src.len() as u32);
        self.children("body", body);
        self.text("sourceType", if module { "module" } else { "script" });
        self.close();
        match self.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    // ---- nodes ---------------------------------------------------------------------------

    fn node(&mut self, id: NodeId) {
        use NodeKind::*;
        let ast = self.ast;
        let node = ast.node(id);
        let (start, end) = (node.span.start, node.span.end);
        match node.kind {
            Ident(a) | BindingIdent(a) | PropName(a) => {
                let name = ast.atom(a).to_string();
                self.identifier(start, end, &name);
            }
            PrivateName(a) => {
                self.open("PrivateIdentifier", start, end);
                let name = ast.atom(a).trim_start_matches('#').to_string();
                self.text("name", &name);
                self.close();
            }
            This => {
                self.open("ThisExpression", start, end);
                self.close();
            }
            Super => {
                self.open("Super", start, end);
                self.close();
            }
            Null => {
                self.open("Literal", start, end);
                self.key("value");
                self.out.push_str("null");
                self.text("raw", "null");
                self.close();
            }
            Bool(b) => {
                self.open("Literal", start, end);
                self.boolean("value", b);
                self.text("raw", if b { "true" } else { "false" });
                self.close();
            }
            Num(n) => {
                self.open("Literal", start, end);
                self.key("value");
                let v = n.get();
                if v.is_finite() {
                    self.out.push_str(&format!("{v}"));
                } else {
                    self.out.push_str("null");
                }
                self.text("raw", self.slice(start, end));
                self.close();
            }
            BigInt(a) => {
                self.open("Literal", start, end);
                self.key("value");
                self.out.push_str("null");
                self.text("raw", self.slice(start, end));
                let digits = decimal_digits(ast.atom(a));
                self.text("bigint", &digits);
                self.close();
            }
            Str(a) => {
                self.open("Literal", start, end);
                self.atom_text("value", a);
                self.text("raw", self.slice(start, end));
                self.close();
            }
            Regex => {
                let raw = self.slice(start, end);
                let split = raw.rfind('/').unwrap_or(0);
                self.open("Literal", start, end);
                self.key("value");
                self.out.push_str("null");
                self.text("raw", raw);
                self.key("regex");
                self.out.push_str("{\"pattern\":");
                self.string(&raw[1..split.max(1)]);
                self.out.push_str(",\"flags\":");
                self.string(&raw[split + 1..]);
                self.out.push('}');
                self.close();
            }
            Template { quasis, exprs } => {
                self.open("TemplateLiteral", start, end);
                self.key("quasis");
                self.out.push('[');
                let qs = ast.list(quasis);
                for (i, &q) in qs.iter().enumerate() {
                    if i > 0 {
                        self.out.push(',');
                    }
                    self.template_element(q, i + 1 == qs.len());
                }
                self.out.push(']');
                self.children("expressions", exprs);
                self.close();
            }
            TemplateElement { .. } => self.template_element(id, false),
            TaggedTemplate { tag, quasi } => {
                self.open("TaggedTemplateExpression", start, end);
                self.child("tag", tag);
                self.child("quasi", quasi);
                self.close();
            }
            Array(items) => {
                self.open("ArrayExpression", start, end);
                self.children("elements", items);
                self.close();
            }
            Hole => self.out.push_str("null"),
            Object(items) => {
                self.open("ObjectExpression", start, end);
                self.children("properties", items);
                self.close();
            }
            Property { key, value, kind } => {
                self.open("Property", start, end);
                self.child("key", key);
                self.child("value", value);
                let kind_name = match kind {
                    PropKind::Get => "get",
                    PropKind::Set => "set",
                    _ => "init",
                };
                self.text("kind", kind_name);
                self.boolean("method", kind == PropKind::Method);
                self.boolean("shorthand", node.has(flags::SHORTHAND));
                self.boolean("computed", node.has(flags::COMPUTED));
                self.close();
            }
            PatternProp { key, value } => {
                self.open("Property", start, end);
                self.child("key", key);
                self.child("value", value);
                self.text("kind", "init");
                self.boolean("method", false);
                self.boolean("shorthand", node.has(flags::SHORTHAND));
                self.boolean("computed", node.has(flags::COMPUTED));
                self.close();
            }
            Spread(arg) => {
                self.open("SpreadElement", start, end);
                self.child("argument", arg);
                self.close();
            }
            Rest(arg) => {
                self.open("RestElement", start, end);
                self.child("argument", arg);
                self.close();
            }
            Function { name, params, body } => {
                let ty = if node.has(flags::ARROW) {
                    "ArrowFunctionExpression"
                } else if node.has(flags::DECL) {
                    "FunctionDeclaration"
                } else {
                    "FunctionExpression"
                };
                self.open(ty, start, end);
                self.child_opt("id", name);
                self.boolean("expression", node.has(flags::EXPR_BODY));
                self.boolean("generator", node.has(flags::GENERATOR));
                self.boolean("async", node.has(flags::ASYNC));
                self.children("params", params);
                self.child("body", body);
                self.close();
            }
            Class { name, superclass, members } => {
                let ty = if node.has(flags::DECL) { "ClassDeclaration" } else { "ClassExpression" };
                self.open(ty, start, end);
                self.child_opt("id", name);
                self.child_opt("superClass", superclass);
                // The body starts at the first `{` after the heading.
                let head_end = superclass.or(name).map(|n| self.span(n).1).unwrap_or(start + 5);
                let open = self.tokens[self.token_at(head_end)..]
                    .iter()
                    .find(|t| t.kind == Kind::Punct(Punct::LBrace))
                    .map_or(start, |t| t.start);
                self.key("body");
                self.open("ClassBody", open, end);
                self.children("body", members);
                self.close();
                self.close();
            }
            ClassMethod { key, value, kind } => {
                self.open("MethodDefinition", start, end);
                self.child("key", key);
                self.child("value", value);
                let kind_name = match kind {
                    MethodKind::Method => "method",
                    MethodKind::Get => "get",
                    MethodKind::Set => "set",
                    MethodKind::Constructor => "constructor",
                };
                self.text("kind", kind_name);
                self.boolean("computed", node.has(flags::COMPUTED));
                self.boolean("static", node.has(flags::STATIC));
                self.close();
            }
            ClassField { key, value } => {
                let ty = if node.has(flags::ACCESSOR) { "AccessorProperty" } else { "PropertyDefinition" };
                self.open(ty, start, end);
                self.child("key", key);
                self.child_opt("value", value);
                self.boolean("computed", node.has(flags::COMPUTED));
                self.boolean("static", node.has(flags::STATIC));
                self.close();
            }
            StaticBlock(body) => {
                self.open("StaticBlock", start, end);
                self.children("body", body);
                self.close();
            }
            Unary { op, arg } => {
                self.open("UnaryExpression", start, end);
                self.text("operator", op.as_str());
                self.boolean("prefix", true);
                self.child("argument", arg);
                self.close();
            }
            Update { op, arg } => {
                self.open("UpdateExpression", start, end);
                self.text("operator", if op == UpdateOp::Inc { "++" } else { "--" });
                self.boolean("prefix", node.has(flags::PREFIX));
                self.child("argument", arg);
                self.close();
            }
            Binary { op, left, right } => {
                use crate::ast::BinOp::*;
                let logical = matches!(op, LogAnd | LogOr | Nullish);
                self.open(if logical { "LogicalExpression" } else { "BinaryExpression" }, start, end);
                self.child("left", left);
                self.text("operator", op.as_str());
                self.child("right", right);
                self.close();
            }
            Assign { op, target, value } => {
                self.open("AssignmentExpression", start, end);
                self.text("operator", &op.as_str());
                self.child("left", target);
                self.child("right", value);
                self.close();
            }
            AssignPattern { target, default } => {
                self.open("AssignmentPattern", start, end);
                self.child("left", target);
                self.child("right", default);
                self.close();
            }
            Conditional { test, cons, alt } => {
                self.open("ConditionalExpression", start, end);
                self.child("test", test);
                self.child("consequent", cons);
                self.child("alternate", alt);
                self.close();
            }
            Call { callee, args } => {
                self.open("CallExpression", start, end);
                self.child("callee", callee);
                self.children("arguments", args);
                self.boolean("optional", node.has(flags::OPTIONAL));
                self.close();
            }
            New { callee, args } => {
                self.open("NewExpression", start, end);
                self.child("callee", callee);
                self.children("arguments", args);
                self.close();
            }
            Member { object, prop } => {
                self.open("MemberExpression", start, end);
                self.child("object", object);
                self.child("property", prop);
                self.boolean("computed", node.has(flags::COMPUTED));
                self.boolean("optional", node.has(flags::OPTIONAL));
                self.close();
            }
            Chain(inner) => {
                self.open("ChainExpression", start, end);
                self.child("expression", inner);
                self.close();
            }
            Sequence(items) => {
                self.open("SequenceExpression", start, end);
                self.children("expressions", items);
                self.close();
            }
            Yield(arg) => {
                self.open("YieldExpression", start, end);
                self.boolean("delegate", node.has(flags::DELEGATE));
                self.child_opt("argument", arg);
                self.close();
            }
            Await(arg) => {
                self.open("AwaitExpression", start, end);
                self.child("argument", arg);
                self.close();
            }
            ImportCall { source, options } => {
                self.open("ImportExpression", start, end);
                self.child("source", source);
                self.child_opt("options", options);
                self.close();
            }
            ImportMeta | NewTarget => {
                let tokens = self.tokens_in(start, end);
                let (first, last) = (tokens.first().copied(), tokens.last().copied());
                self.open("MetaProperty", start, end);
                if let (Some(f), Some(l)) = (first, last) {
                    self.key("meta");
                    self.identifier(f.start, f.end, self.slice(f.start, f.end));
                    self.key("property");
                    self.identifier(l.start, l.end, self.slice(l.start, l.end));
                }
                self.close();
            }
            ObjectPattern(items) => {
                self.open("ObjectPattern", start, end);
                self.children("properties", items);
                self.close();
            }
            ArrayPattern(items) => {
                self.open("ArrayPattern", start, end);
                self.children("elements", items);
                self.close();
            }
            VarDecl { kind, decls } => {
                self.open("VariableDeclaration", start, end);
                self.children("declarations", decls);
                self.text("kind", kind.as_str());
                self.close();
            }
            VarDeclarator { target, init } => {
                self.open("VariableDeclarator", start, end);
                self.child("id", target);
                self.child_opt("init", init);
                self.close();
            }
            ExprStmt(expr) => {
                self.open("ExpressionStatement", start, end);
                self.child("expression", expr);
                if node.has(flags::DIRECTIVE) {
                    let (s, e) = self.span(expr);
                    self.text("directive", self.slice(s + 1, e - 1));
                }
                self.close();
            }
            Block(body) => {
                self.open("BlockStatement", start, end);
                self.children("body", body);
                self.close();
            }
            Empty => {
                self.open("EmptyStatement", start, end);
                self.close();
            }
            If { test, cons, alt } => {
                self.open("IfStatement", start, end);
                self.child("test", test);
                self.child("consequent", cons);
                self.child_opt("alternate", alt);
                self.close();
            }
            For { init, test, update, body } => {
                self.open("ForStatement", start, end);
                self.child_opt("init", init);
                self.child_opt("test", test);
                self.child_opt("update", update);
                self.child("body", body);
                self.close();
            }
            ForIn { left, right, body } => {
                self.open("ForInStatement", start, end);
                self.child("left", left);
                self.child("right", right);
                self.child("body", body);
                self.close();
            }
            ForOf { left, right, body } => {
                self.open("ForOfStatement", start, end);
                self.boolean("await", node.has(flags::AWAIT));
                self.child("left", left);
                self.child("right", right);
                self.child("body", body);
                self.close();
            }
            While { test, body } => {
                self.open("WhileStatement", start, end);
                self.child("test", test);
                self.child("body", body);
                self.close();
            }
            DoWhile { body, test } => {
                self.open("DoWhileStatement", start, end);
                self.child("body", body);
                self.child("test", test);
                self.close();
            }
            Return(arg) => {
                self.open("ReturnStatement", start, end);
                self.child_opt("argument", arg);
                self.close();
            }
            Break(label) | Continue(label) => {
                let ty = if matches!(node.kind, Break(_)) { "BreakStatement" } else { "ContinueStatement" };
                self.open(ty, start, end);
                self.key("label");
                match label {
                    Some(a) => {
                        let name = ast.atom(a).to_string();
                        let tokens = self.tokens_in(start, end);
                        match tokens.iter().skip(1).find(|t| t.kind == Kind::Ident || matches!(t.kind, Kind::Keyword(_))) {
                            Some(t) => self.identifier(t.start, t.end, &name),
                            None => self.out.push_str("null"),
                        }
                    }
                    None => self.out.push_str("null"),
                }
                self.close();
            }
            Throw(arg) => {
                self.open("ThrowStatement", start, end);
                self.child("argument", arg);
                self.close();
            }
            Try { block, handler, finalizer } => {
                self.open("TryStatement", start, end);
                self.child("block", block);
                self.child_opt("handler", handler);
                self.child_opt("finalizer", finalizer);
                self.close();
            }
            Catch { param, body } => {
                self.open("CatchClause", start, end);
                self.child_opt("param", param);
                self.child("body", body);
                self.close();
            }
            Switch { disc, cases } => {
                self.open("SwitchStatement", start, end);
                self.child("discriminant", disc);
                self.children("cases", cases);
                self.close();
            }
            Case { test, body } => {
                self.open("SwitchCase", start, end);
                self.child_opt("test", test);
                self.children("consequent", body);
                self.close();
            }
            Labeled { label, body } => {
                self.open("LabeledStatement", start, end);
                let name = ast.atom(label).to_string();
                self.key("label");
                match self.tokens.get(self.token_at(start)) {
                    Some(t) => self.identifier(t.start, t.end, &name),
                    None => self.out.push_str("null"),
                }
                self.child("body", body);
                self.close();
            }
            Debugger => {
                self.open("DebuggerStatement", start, end);
                self.close();
            }
            With { object, body } => {
                self.open("WithStatement", start, end);
                self.child("object", object);
                self.child("body", body);
                self.close();
            }
            Program(body) => {
                self.open("Program", start, end);
                self.children("body", body);
                self.close();
            }
            Import(data) => {
                let data = *ast.import(data);
                self.open("ImportDeclaration", start, end);
                self.children("specifiers", data.specifiers);
                self.key("source");
                self.source_literal(start, end);
                self.attributes(data.attrs);
                self.close();
            }
            ImportDefault(local) => {
                self.open("ImportDefaultSpecifier", start, end);
                self.child("local", local);
                self.close();
            }
            ImportNamespace(local) => {
                self.open("ImportNamespaceSpecifier", start, end);
                self.child("local", local);
                self.close();
            }
            ImportSpecifier { local, .. } => {
                self.open("ImportSpecifier", start, end);
                let tokens = self.tokens_in(start, end);
                if let Some(&first) = tokens.first() {
                    self.child_token("imported", first);
                }
                self.child("local", local);
                self.close();
            }
            ExportNamed(data) => {
                let data = *ast.export(data);
                self.open("ExportNamedDeclaration", start, end);
                self.child_opt("declaration", data.decl);
                self.children("specifiers", data.specifiers);
                self.key("source");
                if data.source.is_some() {
                    self.source_literal(start, end);
                } else {
                    self.out.push_str("null");
                }
                self.attributes(data.attrs);
                self.close();
            }
            ExportSpecifier { .. } => {
                let tokens = self.tokens_in(start, end);
                self.open("ExportSpecifier", start, end);
                if let (Some(&local), Some(&exported)) = (tokens.first(), tokens.last()) {
                    self.child_token("local", local);
                    self.child_token("exported", exported);
                }
                self.close();
            }
            ExportDefault(decl) => {
                self.open("ExportDefaultDeclaration", start, end);
                self.child("declaration", decl);
                self.close();
            }
            ExportAll { exported, attrs, .. } => {
                self.open("ExportAllDeclaration", start, end);
                self.key("exported");
                match exported {
                    Some(_) => {
                        // `export * as name from`: the token after `as`.
                        let tokens = self.tokens_in(start, end);
                        let name = tokens.iter().position(|t| t.kind == Kind::Ident && self.slice(t.start, t.end) == "as").and_then(|i| tokens.get(i + 1));
                        match name {
                            Some(&t) => self.module_name(t),
                            None => self.out.push_str("null"),
                        }
                    }
                    None => self.out.push_str("null"),
                }
                self.key("source");
                self.source_literal(start, end);
                self.attributes(attrs);
                self.close();
            }
            ImportAttr { .. } => {
                self.open("ImportAttribute", start, end);
                self.close();
            }
            _ => self.fail("this syntax has no ESTree form here (TypeScript and JSX are not supported)", start),
        }
    }

    fn template_element(&mut self, id: NodeId, tail: bool) {
        let ast = self.ast;
        let node = ast.node(id);
        let NodeKind::TemplateElement { cooked, raw } = node.kind else { return };
        // The arena keeps the delimiters (`` ` ``, `}`, `${`); ESTree leaves them out.
        let text = self.slice(node.span.start, node.span.end);
        let from = node.span.start + 1;
        let to = node.span.end - if text.ends_with("${") { 2 } else { 1 };
        self.open("TemplateElement", from, to.max(from));
        self.key("value");
        self.out.push_str("{\"raw\":");
        let raw_text = ast.atom(raw).to_string();
        self.string(&raw_text);
        self.out.push_str(",\"cooked\":");
        match cooked {
            Some(c) => {
                let text = ast.atom(c).to_string();
                self.string(&text);
            }
            None => self.out.push_str("null"),
        }
        self.out.push('}');
        self.boolean("tail", tail);
        self.close();
    }
}

/// The names a program uses without declaring them (free variables), for tools that add imports for them.
pub fn free_variables(src: &str) -> Result<Vec<String>, EstreeError> {
    let parsed = match parse(src, ParseOptions::new(true)) {
        Ok(p) => p,
        Err(module_error) => match parse(src, ParseOptions::new(false)) {
            Ok(p) => p,
            Err(script_error) => {
                let e = if script_error.pos > module_error.pos { script_error } else { module_error };
                return Err(EstreeError { message: e.message.clone(), pos: e.pos });
            }
        },
    };
    let mut names: Vec<String> = Vec::new();
    for &id in parsed.semantic.unresolved() {
        if let NodeKind::Ident(a) = parsed.ast.node(id).kind {
            let name = parsed.ast.atom(a);
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

/// A BigInt literal's digits (with `0x`, `0o` or `0b` kept) as decimal digits, which is what ESTree's `bigint` holds.
fn decimal_digits(text: &str) -> String {
    let (radix, body) = match text.get(..2) {
        Some("0x" | "0X") => (16u32, &text[2..]),
        Some("0o" | "0O") => (8, &text[2..]),
        Some("0b" | "0B") => (2, &text[2..]),
        _ => return text.to_string(),
    };
    // Little-endian base 10^9 limbs: multiply by the radix and add each digit.
    let mut limbs: Vec<u32> = vec![0];
    for ch in body.chars() {
        let Some(digit) = ch.to_digit(radix) else { continue };
        let mut carry = digit as u64;
        for limb in limbs.iter_mut() {
            let value = *limb as u64 * radix as u64 + carry;
            *limb = (value % 1_000_000_000) as u32;
            carry = value / 1_000_000_000;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
    }
    let mut out = limbs.last().map_or("0".to_string(), |l| l.to_string());
    for limb in limbs.iter().rev().skip(1) {
        out.push_str(&format!("{limb:09}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(src: &str) -> String {
        to_json(src, false).unwrap_or_else(|e| panic!("{src:?}: {}", e.message))
    }

    #[test]
    fn positions_count_utf16_units() {
        // The emoji is two units but four bytes, so `b` is at unit 14 although it is at byte 16.
        let text = json("let a = \"\u{1F600}\"; b;");
        assert!(text.contains("\"name\":\"b\"") && text.contains("\"start\":14,\"end\":15"), "{text}");
    }

    #[test]
    fn modules_and_scripts() {
        assert!(json("import a, { b as c } from 'm'; export default a;").contains("\"sourceType\":\"module\""));
        assert!(json("with (a) b").contains("\"sourceType\":\"script\""));
        assert!(to_json("return 1", true).is_ok());
        assert!(to_json("let = ;", false).is_err());
    }

    #[test]
    fn imports_carry_their_names() {
        let text = json("import d, * as n from 'x'; import { a as b } from \"y\" with { type: 'json' }; export * as z from 'w';");
        for needle in ["ImportDefaultSpecifier", "ImportNamespaceSpecifier", "ImportSpecifier", "\"imported\":{\"type\":\"Identifier\"", "ImportAttribute", "ExportAllDeclaration", "\"value\":\"w\""] {
            assert!(text.contains(needle), "{needle} in {text}");
        }
    }

    #[test]
    fn big_integers_are_decimal() {
        assert_eq!(decimal_digits("0xff"), "255");
        assert_eq!(decimal_digits("0xffffffffffffffffffffffffff"), "20282409603651670423947251286015");
        assert_eq!(decimal_digits("0b101"), "5");
        assert_eq!(decimal_digits("123"), "123");
    }

    #[test]
    fn free_variables_are_the_undeclared_names() {
        let names = free_variables("const a = 1; function f(b) { return a + b + c + $; } d.e;").unwrap();
        assert_eq!(names, vec!["c".to_string(), "$".to_string(), "d".to_string()]);
    }
}
