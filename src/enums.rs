//! TypeScript enums whose members have values known at build time.
//!
//! A reference such as `Color.Red` is replaced by the member's value, and the shaker leaves it
//! out of the names a statement uses, so an enum nobody reads as a whole disappears from the
//! bundle. Both sides ask [`EnumTable::member`], which keeps them in agreement.

use std::collections::HashMap;

use crate::ast::{Ast, Atom, NodeId, NodeKind};
use crate::graph::{Graph, ModuleId, ModuleKind};
use crate::link::{Binding, Linked};
use crate::semantic::{Semantic, SymbolId};

/// A value an enum member is known to have.
#[derive(Clone, Debug, PartialEq)]
pub enum EnumValue {
    Num(f64),
    Str(String),
}

pub fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else if n == 0.0 && n.is_sign_negative() {
        "-0".to_string()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    (n.trunc() % 4294967296.0) as i64 as u32 as i32
}

/// The value of a constant enum initializer, if it is one. Members named in it must have
/// known values of their own.
pub fn eval(ast: &Ast, semantic: &Semantic, id: NodeId, enum_node: NodeId, values: &HashMap<Atom, EnumValue>) -> Option<EnumValue> {
    use crate::ast::{BinOp, UnaryOp};
    match ast[id].kind {
        NodeKind::Num(n) => Some(EnumValue::Num(n.get())),
        NodeKind::Str(a) => Some(EnumValue::Str(ast.atom(a).to_string())),
        NodeKind::Template { quasis, exprs } if ast.list(exprs).is_empty() => match ast[*ast.list(quasis).first()?].kind {
            NodeKind::TemplateElement { cooked: Some(a), .. } => Some(EnumValue::Str(ast.atom(a).to_string())),
            _ => None,
        },
        NodeKind::Unary { op, arg } => {
            let EnumValue::Num(n) = eval(ast, semantic, arg, enum_node, values)? else { return None };
            match op {
                UnaryOp::Neg => Some(EnumValue::Num(-n)),
                UnaryOp::Pos => Some(EnumValue::Num(n)),
                UnaryOp::BitNot => Some(EnumValue::Num(!to_int32(n) as f64)),
                _ => None,
            }
        }
        NodeKind::Binary { op, left, right } => {
            let (l, r) = (eval(ast, semantic, left, enum_node, values)?, eval(ast, semantic, right, enum_node, values)?);
            match (l, r) {
                (EnumValue::Num(a), EnumValue::Num(b)) => {
                    let (ia, ib) = (to_int32(a), to_int32(b));
                    Some(EnumValue::Num(match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Div => a / b,
                        BinOp::Rem => a % b,
                        BinOp::Exp => a.powf(b),
                        BinOp::Shl => (ia << (ib & 31)) as f64,
                        BinOp::Shr => (ia >> (ib & 31)) as f64,
                        BinOp::UShr => ((ia as u32) >> (ib & 31)) as f64,
                        BinOp::BitAnd => (ia & ib) as f64,
                        BinOp::BitOr => (ia | ib) as f64,
                        BinOp::BitXor => (ia ^ ib) as f64,
                        _ => return None,
                    }))
                }
                (EnumValue::Str(a), EnumValue::Str(b)) if op == BinOp::Add => Some(EnumValue::Str(a + &b)),
                (EnumValue::Str(a), EnumValue::Num(b)) if op == BinOp::Add => Some(EnumValue::Str(a + &number_text(b))),
                (EnumValue::Num(a), EnumValue::Str(b)) if op == BinOp::Add => Some(EnumValue::Str(number_text(a) + &b)),
                _ => None,
            }
        }
        // A member of this enum, by its bare name.
        NodeKind::Ident(_) => {
            let sym = semantic.symbol(semantic.symbol_of(id)?);
            if semantic.scope(sym.scope).node == Some(enum_node) {
                values.get(&sym.name).cloned()
            } else {
                None
            }
        }
        // `Enum.Member` and `Enum["Member"]` inside its own body.
        NodeKind::Member { object, prop } => {
            let NodeKind::TsEnum { name, .. } = ast[enum_node].kind else { return None };
            if semantic.symbol_of(object)? != semantic.symbol_of(name)? {
                return None;
            }
            let member = match ast[prop].kind {
                NodeKind::PropName(a) | NodeKind::Str(a) => a,
                _ => return None,
            };
            values.get(&member).cloned()
        }
        _ => None,
    }
}

/// The members of each module's top-level enums that are safe to replace by their values.
#[derive(Debug, Default)]
pub struct EnumTable {
    modules: Vec<HashMap<SymbolId, HashMap<String, EnumValue>>>,
}

impl EnumTable {
    /// No enum is inlined.
    pub fn empty() -> EnumTable {
        EnumTable::default()
    }

    pub fn build(graph: &Graph) -> EnumTable {
        let modules = graph.modules.iter().map(|m| module_enums(graph, m.id)).collect();
        EnumTable { modules }
    }

    /// The value of `name` in the enum `symbol` of `module`: a declaration there, or an import
    /// that leads to one.
    pub fn member(&self, linked: &Linked, module: ModuleId, symbol: SymbolId, name: &str) -> Option<EnumValue> {
        let (module, symbol) = match linked.binding(module, symbol) {
            None => (module, symbol),
            Some(Binding::Symbol { module, symbol }) => (*module, *symbol),
            Some(_) => return None,
        };
        self.modules.get(module.index())?.get(&symbol)?.get(name).cloned()
    }
}

fn module_enums(graph: &Graph, id: ModuleId) -> HashMap<SymbolId, HashMap<String, EnumValue>> {
    let module = graph.module(id);
    let mut found = HashMap::new();
    let (ModuleKind::Esm, Some(parsed)) = (module.kind, module.parsed.as_ref()) else { return found };
    let (ast, semantic) = (&parsed.ast, &parsed.semantic);
    let NodeKind::Program(body) = ast[ast.root].kind else { return found };
    let declaration = |stmt: NodeId| match ast[stmt].kind {
        NodeKind::ExportNamed(export) => ast.export(export).decl.unwrap_or(stmt),
        _ => stmt,
    };
    // An enum that shares its name with another enum or a namespace is merged with it: the
    // members seen here are not all there are.
    let mut declared: HashMap<SymbolId, usize> = HashMap::new();
    for &stmt in ast.list(body) {
        if let NodeKind::TsEnum { name, .. } | NodeKind::TsNamespace { name, .. } = ast[declaration(stmt)].kind {
            if let Some(symbol) = semantic.symbol_of(name) {
                *declared.entry(symbol).or_default() += 1;
            }
        }
    }
    for &stmt in ast.list(body) {
        let node = declaration(stmt);
        let NodeKind::TsEnum { name, .. } = ast[node].kind else { continue };
        let Some(symbol) = semantic.symbol_of(name) else { continue };
        if declared[&symbol] != 1 || semantic.symbol(symbol).writes > 1 {
            continue;
        }
        found.insert(symbol, enum_values(ast, semantic, node));
    }
    found
}

/// Every member whose value is a constant, by name.
fn enum_values(ast: &Ast, semantic: &Semantic, node: NodeId) -> HashMap<String, EnumValue> {
    let NodeKind::TsEnum { members, .. } = ast[node].kind else { return HashMap::new() };
    let mut values: HashMap<Atom, EnumValue> = HashMap::new();
    let mut by_name = HashMap::new();
    let mut auto: Option<f64> = Some(0.0);
    for &member in ast.list(members) {
        let NodeKind::TsEnumMember { name, init } = ast[member].kind else { continue };
        let (text, atom) = match ast[name].kind {
            NodeKind::BindingIdent(a) => (ast.atom(a).to_string(), Some(a)),
            NodeKind::Str(a) => (ast.atom(a).to_string(), None),
            _ => continue,
        };
        let known = match init {
            Some(e) => eval(ast, semantic, e, node, &values),
            None => auto.map(EnumValue::Num),
        };
        auto = match &known {
            Some(EnumValue::Num(n)) => Some(n + 1.0),
            _ => None,
        };
        if let Some(value) = known {
            if let Some(a) = atom {
                values.insert(a, value.clone());
            }
            by_name.insert(text, value);
        }
    }
    by_name
}
