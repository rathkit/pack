//! Compact s-expression rendering of a syntax tree, for tests and debugging.
//!
//! Leaves print as their value (`a`, `$a` for a binding, `:a` for a property name, `1`,
//! `"str"`), other nodes as `(Kind child...)`. A trailing `^` marks a parenthesized node.

use crate::ast::{Ast, NodeId, NodeKind, flags};

pub fn dump(ast: &Ast, src: &str) -> String {
    let mut out = String::new();
    write_node(ast, src, ast.root, &mut out);
    out
}

pub fn dump_node(ast: &Ast, src: &str, id: NodeId) -> String {
    let mut out = String::new();
    write_node(ast, src, id, &mut out);
    out
}

fn write_node(ast: &Ast, src: &str, id: NodeId, out: &mut String) {
    use NodeKind::*;
    let node = ast.node(id);
    match node.kind {
        Ident(a) => out.push_str(ast.atom(a)),
        BindingIdent(a) => {
            out.push('$');
            out.push_str(ast.atom(a));
        }
        PropName(a) => {
            out.push(':');
            out.push_str(ast.atom(a));
        }
        PrivateName(a) => {
            out.push('#');
            out.push_str(ast.atom(a));
        }
        This => out.push_str("this"),
        Super => out.push_str("super"),
        Null => out.push_str("null"),
        Bool(b) => out.push_str(if b { "true" } else { "false" }),
        Num(n) => out.push_str(&format!("{}", n.get())),
        BigInt(a) => {
            out.push_str(ast.atom(a));
            out.push('n');
        }
        Str(a) => out.push_str(&format!("{:?}", ast.atom(a))),
        Regex => out.push_str(&src[node.span.start as usize..node.span.end as usize]),
        Hole => out.push_str("<hole>"),
        ImportMeta => out.push_str("import.meta"),
        NewTarget => out.push_str("new.target"),
        Empty => out.push_str("(Empty)"),
        Debugger => out.push_str("(Debugger)"),
        TemplateElement { cooked, .. } => match cooked {
            Some(a) => out.push_str(&format!("{:?}", ast.atom(a))),
            None => out.push_str("<invalid>"),
        },
        Function { name, params, body } => {
            out.push_str("(Function");
            for (flag, tag) in [
                (flags::ASYNC, "async"),
                (flags::GENERATOR, "gen"),
                (flags::ARROW, "arrow"),
                (flags::EXPR_BODY, "expr"),
                (flags::DECL, "decl"),
            ] {
                if node.has(flag) {
                    out.push_str(&format!("[{tag}]"));
                }
            }
            if let Some(name) = name {
                out.push(' ');
                write_node(ast, src, name, out);
            }
            out.push_str(" (");
            for (i, &p) in ast.list(params).iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_node(ast, src, p, out);
            }
            out.push_str(") ");
            write_node(ast, src, body, out);
            out.push(')');
        }
        _ => {
            out.push('(');
            out.push_str(&head(ast, id));
            ast.for_each_child(id, |child| {
                out.push(' ');
                write_node(ast, src, child, out);
            });
            out.push(')');
        }
    }
    if node.parenthesized() {
        out.push('^');
    }
}

/// The name and any inline attributes of a non-leaf node.
fn head(ast: &Ast, id: NodeId) -> String {
    use NodeKind::*;
    let node = ast.node(id);
    let mut tags = String::new();
    let mut tag = |flag: u8, text: &str| {
        if node.has(flag) {
            tags.push_str(&format!("[{text}]"));
        }
    };
    let name = match node.kind {
        Binary { op, .. } => op.as_str().to_string(),
        Assign { op, .. } => op.as_str(),
        Unary { op, .. } => format!("u{}", op.as_str()),
        Update { op, .. } => {
            tag(flags::PREFIX, "prefix");
            format!("Update{:?}", op)
        }
        Member { .. } => {
            tag(flags::COMPUTED, "computed");
            tag(flags::OPTIONAL, "?");
            "Member".into()
        }
        Call { .. } => {
            tag(flags::OPTIONAL, "?");
            "Call".into()
        }
        Property { kind, .. } => {
            tag(flags::COMPUTED, "computed");
            tag(flags::SHORTHAND, "short");
            format!("Prop{:?}", kind)
        }
        PatternProp { .. } => {
            tag(flags::COMPUTED, "computed");
            tag(flags::SHORTHAND, "short");
            "PatternProp".into()
        }
        ClassMethod { kind, .. } => {
            tag(flags::STATIC, "static");
            tag(flags::COMPUTED, "computed");
            format!("Method{:?}", kind)
        }
        ClassField { .. } => {
            tag(flags::STATIC, "static");
            tag(flags::COMPUTED, "computed");
            "Field".into()
        }
        StaticBlock(_) => "StaticBlock".into(),
        Class { .. } => {
            tag(flags::DECL, "decl");
            "Class".into()
        }
        Yield(_) => {
            tag(flags::DELEGATE, "*");
            "Yield".into()
        }
        ForOf { .. } => {
            tag(flags::AWAIT, "await");
            "ForOf".into()
        }
        ExprStmt(_) => {
            tag(flags::DIRECTIVE, "directive");
            "ExprStmt".into()
        }
        VarDecl { kind, .. } => kind.as_str().to_string(),
        Labeled { label, .. } => format!("Labeled {}", ast.atom(label)),
        Break(label) => format!("Break{}", label.map_or(String::new(), |l| format!(" {}", ast.atom(l)))),
        Continue(label) => format!("Continue{}", label.map_or(String::new(), |l| format!(" {}", ast.atom(l)))),
        Import(import) => format!("Import {:?}", ast.atom(ast.import(import).source)),
        ImportSpecifier { imported, .. } => format!("ImportSpecifier {}", ast.atom(imported)),
        ExportSpecifier { local, exported } => format!("ExportSpecifier {} as {}", ast.atom(local), ast.atom(exported)),
        ExportNamed(export) => {
            let source = ast.export(export).source;
            format!("ExportNamed{}", source.map_or(String::new(), |s| format!(" from {:?}", ast.atom(s))))
        }
        ExportAll { exported, source, .. } => format!(
            "ExportAll{} from {:?}",
            exported.map_or(String::new(), |e| format!(" as {}", ast.atom(e))),
            ast.atom(source)
        ),
        ImportAttr { key, value } => format!("ImportAttr {}={:?}", ast.atom(key), ast.atom(value)),
        other => {
            let debug = format!("{other:?}");
            debug.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string()
        }
    };
    format!("{name}{tags}")
}
