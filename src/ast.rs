//! Arena-allocated syntax tree.
//!
//! Every node lives in one `Vec` and refers to its children by `NodeId`, a `u32` index.
//! Child lists live in a second `Vec<NodeId>` and are referred to by `List` ranges. The
//! tree is built bottom-up (children before parents), so no node is ever moved or freed
//! while parsing, and later passes can attach side tables indexed by `NodeId`.
//!
//! Names and string values are interned as `Atom`s so renaming and comparison are integer
//! operations. Source positions are byte offsets in `Span`.

use std::hash::{BuildHasherDefault, Hasher};
use std::num::NonZeroU32;
use std::ops::Index;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0.get() as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Atom(NonZeroU32);

impl Atom {
    pub fn index(self) -> usize {
        self.0.get() as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// A run of child ids stored in the shared list arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct List {
    start: u32,
    len: u32,
}

impl List {
    pub fn len(self) -> usize {
        self.len as usize
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

/// An `f64` stored as two words so `NodeKind` keeps 4-byte alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Num {
    hi: u32,
    lo: u32,
}

impl Num {
    pub fn new(value: f64) -> Num {
        let bits = value.to_bits();
        Num { hi: (bits >> 32) as u32, lo: bits as u32 }
    }

    pub fn get(self) -> f64 {
        f64::from_bits(((self.hi as u64) << 32) | self.lo as u64)
    }
}

/// Bits for `Node::flags`. Bit 0 means the same thing everywhere; the rest are specific to
/// the node kind named in the comment.
pub mod flags {
    /// Any expression that was written inside parentheses.
    pub const PARENTHESIZED: u8 = 1 << 0;
    /// `Member`, `Property`, `ClassMethod`, `ClassField`, `PatternProp`: key is `[computed]`.
    pub const COMPUTED: u8 = 1 << 1;
    /// `Member`, `Call`: this link was written with `?.`.
    pub const OPTIONAL: u8 = 1 << 2;
    /// `Property`, `PatternProp`: `{ a }` or `{ a = 1 }`.
    pub const SHORTHAND: u8 = 1 << 3;
    /// `ClassMethod`, `ClassField`: declared `static`.
    pub const STATIC: u8 = 1 << 4;
    /// `Function`.
    pub const ASYNC: u8 = 1 << 1;
    pub const GENERATOR: u8 = 1 << 2;
    pub const ARROW: u8 = 1 << 3;
    /// `Function` with `ARROW`: the body is an expression, not a block.
    pub const EXPR_BODY: u8 = 1 << 4;
    /// `Function`, `Class`: a declaration (statement or export default), not an expression.
    pub const DECL: u8 = 1 << 5;
    /// `ClassField`: an auto-accessor, `accessor x = 1`.
    pub const ACCESSOR: u8 = 1 << 5;
    /// `Update`: `++x` rather than `x++`.
    pub const PREFIX: u8 = 1 << 1;
    /// `Yield`: `yield*`.
    pub const DELEGATE: u8 = 1 << 1;
    /// `ForOf`: `for await`.
    pub const AWAIT: u8 = 1 << 1;
    /// `ExprStmt`: a directive such as `"use strict"`.
    pub const DIRECTIVE: u8 = 1 << 1;
    /// `Ident`: the identifier is assigned to (`x = 1`, `[x] = a`, `for (x of y)`).
    pub const ASSIGNED: u8 = 1 << 1;
    /// `Ident` with `ASSIGNED`: the old value is read too (`x += 1`, `x++`).
    pub const READ_ALSO: u8 = 1 << 2;
    /// `TsEnum`: declared `const enum`.
    pub const TS_CONST: u8 = 1 << 1;
    /// `Array`, `Object`: a `...spread` element is followed by a comma, which stops the
    /// literal from being reinterpreted as a destructuring pattern.
    pub const REST_COMMA: u8 = 1 << 1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Exp,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
    In,
    InstanceOf,
    LogAnd,
    LogOr,
    Nullish,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            Exp => "**",
            Eq => "==",
            Ne => "!=",
            StrictEq => "===",
            StrictNe => "!==",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            Shl => "<<",
            Shr => ">>",
            UShr => ">>>",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            In => "in",
            InstanceOf => "instanceof",
            LogAnd => "&&",
            LogOr => "||",
            Nullish => "??",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Op(BinOp),
}

impl AssignOp {
    pub fn as_str(self) -> String {
        match self {
            AssignOp::Assign => "=".into(),
            AssignOp::Op(op) => format!("{}=", op.as_str()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    Not,
    BitNot,
    Typeof,
    Void,
    Delete,
}

impl UnaryOp {
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Pos => "+",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::Typeof => "typeof",
            UnaryOp::Void => "void",
            UnaryOp::Delete => "delete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOp {
    Inc,
    Dec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Var,
    Let,
    Const,
}

impl VarKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VarKind::Var => "var",
            VarKind::Let => "let",
            VarKind::Const => "const",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    Init,
    Get,
    Set,
    Method,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Method,
    Get,
    Set,
    Constructor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    // ---- expressions ----
    /// A reference to a binding.
    Ident(Atom),
    /// A non-computed property name: the `b` in `a.b` or `{ b: 1 }`.
    PropName(Atom),
    PrivateName(Atom),
    This,
    Super,
    Null,
    Bool(bool),
    Num(Num),
    /// Digits only, separators and the `n` suffix removed, radix prefix kept.
    BigInt(Atom),
    Str(Atom),
    /// Pattern and flags are the source text under the node's span.
    Regex,
    Template { quasis: List, exprs: List },
    /// `cooked` is `None` for an invalid escape in a tagged template.
    TemplateElement { cooked: Option<Atom>, raw: Atom },
    TaggedTemplate { tag: NodeId, quasi: NodeId },
    Array(List),
    /// An elision: the empty slot in `[a, , b]`.
    Hole,
    Object(List),
    Property { key: NodeId, value: NodeId, kind: PropKind },
    Spread(NodeId),
    Function { name: Option<NodeId>, params: List, body: NodeId },
    Class { name: Option<NodeId>, superclass: Option<NodeId>, members: List },
    ClassMethod { key: NodeId, value: NodeId, kind: MethodKind },
    ClassField { key: NodeId, value: Option<NodeId> },
    StaticBlock(List),
    Unary { op: UnaryOp, arg: NodeId },
    Update { op: UpdateOp, arg: NodeId },
    Binary { op: BinOp, left: NodeId, right: NodeId },
    Assign { op: AssignOp, target: NodeId, value: NodeId },
    Conditional { test: NodeId, cons: NodeId, alt: NodeId },
    Call { callee: NodeId, args: List },
    New { callee: NodeId, args: List },
    Member { object: NodeId, prop: NodeId },
    /// Wraps a whole optional chain so short-circuiting ends at the right place.
    Chain(NodeId),
    Sequence(List),
    Yield(Option<NodeId>),
    Await(NodeId),
    ImportCall { source: NodeId, options: Option<NodeId> },
    ImportMeta,
    NewTarget,

    // ---- patterns ----
    BindingIdent(Atom),
    ObjectPattern(List),
    ArrayPattern(List),
    AssignPattern { target: NodeId, default: NodeId },
    Rest(NodeId),
    PatternProp { key: NodeId, value: NodeId },

    // ---- statements ----
    Program(List),
    VarDecl { kind: VarKind, decls: List },
    VarDeclarator { target: NodeId, init: Option<NodeId> },
    ExprStmt(NodeId),
    Block(List),
    Empty,
    If { test: NodeId, cons: NodeId, alt: Option<NodeId> },
    For { init: Option<NodeId>, test: Option<NodeId>, update: Option<NodeId>, body: NodeId },
    ForIn { left: NodeId, right: NodeId, body: NodeId },
    ForOf { left: NodeId, right: NodeId, body: NodeId },
    While { test: NodeId, body: NodeId },
    DoWhile { body: NodeId, test: NodeId },
    Return(Option<NodeId>),
    Break(Option<Atom>),
    Continue(Option<Atom>),
    Throw(NodeId),
    Try { block: NodeId, handler: Option<NodeId>, finalizer: Option<NodeId> },
    Catch { param: Option<NodeId>, body: NodeId },
    Switch { disc: NodeId, cases: List },
    Case { test: Option<NodeId>, body: List },
    Labeled { label: Atom, body: NodeId },
    Debugger,
    With { object: NodeId, body: NodeId },

    // ---- modules ----
    // `import` and `export {...}` declarations carry the most data of any node and are rare,
    // so they live in side tables. That keeps every node at 32 bytes instead of 44.
    Import(ImportId),
    ImportSpecifier { imported: Atom, local: NodeId },
    ImportDefault(NodeId),
    ImportNamespace(NodeId),
    ExportNamed(ExportId),
    ExportSpecifier { local: Atom, exported: Atom },
    ExportDefault(NodeId),
    ExportAll { exported: Option<Atom>, source: Atom, attrs: List },
    ImportAttr { key: Atom, value: Atom },

    // ---- TypeScript constructs that become JavaScript ----
    /// `enum E { A = 1, B }`; flagged `flags::TS_CONST` for `const enum`.
    TsEnum { name: NodeId, members: List },
    /// A member of an enum: a `BindingIdent` or `Str` name, and an optional initializer.
    TsEnumMember { name: NodeId, init: Option<NodeId> },
    /// `namespace N { ... }`. A dotted name is nested namespaces, each in the body of the last.
    TsNamespace { name: NodeId, body: List },
    /// A declaration marked `export` inside a namespace body.
    TsExport(NodeId),

    // ---- JSX ----
    /// `<tag attrs>children</tag>`. The tag is a `Str` for an intrinsic element (`div`) and an
    /// `Ident`, `Member` or `This` expression for a component (`Foo`, `a.B`).
    JsxElement(JsxId),
    /// `<>children</>`
    JsxFragment { children: List },
    /// `name="text"`, `name={expr}`, `name=<el/>` or a bare `name`.
    JsxAttr { name: Atom, value: Option<NodeId> },
    /// `{...expr}` among the attributes.
    JsxSpreadAttr(NodeId),
    /// Text between tags, already cleaned the way React reads it (never empty).
    JsxText(Atom),
}

/// Index of an [`ImportData`] in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportId(u32);

/// Index of an [`ExportData`] in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportId(u32);

/// An `import` declaration: `import specifiers from "source" with { attrs }`. A side-effect
/// import (`import "source"`) has no specifiers.
#[derive(Clone, Copy, Debug)]
pub struct ImportData {
    pub specifiers: List,
    pub source: Atom,
    pub attrs: List,
}

/// Index of a [`JsxData`] in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsxId(u32);

/// An element's tag, attributes and children, out of line to keep nodes small.
#[derive(Clone, Copy, Debug)]
pub struct JsxData {
    pub tag: NodeId,
    pub attrs: List,
    pub children: List,
}

/// An `export` of local names or a declaration: `export { a as b }`, `export { a } from "m"`,
/// or `export const x = 1` (then `decl` is set and `specifiers` is empty).
#[derive(Clone, Copy, Debug)]
pub struct ExportData {
    pub specifiers: List,
    pub source: Option<Atom>,
    pub decl: Option<NodeId>,
    pub attrs: List,
}

/// What a type annotation becomes at run time for `emitDecoratorMetadata`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeMeta {
    Object,
    Function,
    Array,
    String,
    Number,
    Boolean,
    Symbol,
    BigInt,
    /// `void`, `null`, `undefined`: serialized as `void 0`.
    Void,
    /// A reference to a class or other value, an `Ident` or `Member` node.
    Ref(NodeId),
}

/// A decorator, `@expression`, on a class, a class member or a parameter.
#[derive(Clone, Copy, Debug)]
pub struct Decorator {
    /// The whole `@expression`.
    pub span: Span,
    pub expr: NodeId,
}

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
    pub flags: u8,
}

impl Node {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn parenthesized(&self) -> bool {
        self.has(flags::PARENTHESIZED)
    }
}

pub struct Ast {
    nodes: Vec<Node>,
    lists: Vec<NodeId>,
    imports: Vec<ImportData>,
    exports: Vec<ExportData>,
    jsx_elements: Vec<JsxData>,
    /// Source text that exists only for TypeScript (types, modifiers, `as` casts): spans to
    /// leave out of the output, sorted and not overlapping.
    erased: Vec<Span>,
    /// TypeScript text replaced by other text: `import x = require()` starts with `const`.
    rewrites: Vec<(Span, &'static str)>,
    /// Parameter properties (`constructor(private x)`): the constructor and the parameters'
    /// names, in the order the constructors finish parsing.
    param_props: Vec<(NodeId, Vec<NodeId>)>,
    /// `export =` or `import x = require()`: the file is CommonJS written in TypeScript.
    pub ts_commonjs: bool,
    /// Decorators by the node they decorate, in the order those nodes finished parsing.
    decorators: Vec<(NodeId, Vec<Decorator>)>,
    /// Declared types of parameters, properties and accessors, and return types of methods,
    /// recorded only when decorator metadata is on. Keyed by the parameter's binding, the
    /// member, or the method's function node.
    type_meta: std::collections::HashMap<NodeId, TypeMeta>,
    /// The `{` and `}` of a class body, by class, recorded in TypeScript files.
    class_bodies: Vec<(NodeId, Span)>,
    /// The file contains JSX.
    pub uses_jsx: bool,
    /// The file has an `import()` of a template literal or of a `+` (`import(`./a/${x}.js`)`): a path with a variable in it.
    pub variable_import: bool,
    atoms: Interner,
    pub root: NodeId,
}

impl Ast {
    pub(crate) fn new(source_len: usize) -> Ast {
        // Real programs average roughly one node per 6 to 8 bytes of source.
        let mut nodes = Vec::with_capacity(source_len / 6 + 8);
        // Index 0 is a placeholder so `NodeId` can be non-zero and `Option<NodeId>` stays 4 bytes.
        nodes.push(Node { kind: NodeKind::Empty, span: Span::default(), flags: 0 });
        let root = NodeId(NonZeroU32::new(1).unwrap());
        Ast { nodes, lists: Vec::with_capacity(source_len / 16), imports: Vec::new(), exports: Vec::new(), jsx_elements: Vec::new(), erased: Vec::new(), rewrites: Vec::new(), param_props: Vec::new(), ts_commonjs: false, decorators: Vec::new(), type_meta: std::collections::HashMap::new(), class_bodies: Vec::new(), uses_jsx: false, variable_import: false, atoms: Interner::with_source_len(source_len), root }
    }

    pub(crate) fn alloc(&mut self, kind: NodeKind, span: Span, flags: u8) -> NodeId {
        let index = self.nodes.len();
        assert!(index < u32::MAX as usize, "too many syntax nodes");
        self.nodes.push(Node { kind, span, flags });
        NodeId(NonZeroU32::new(index as u32).unwrap())
    }

    pub(crate) fn alloc_list(&mut self, items: &[NodeId]) -> List {
        let start = self.lists.len();
        assert!(start + items.len() < u32::MAX as usize, "too many syntax list entries");
        self.lists.extend_from_slice(items);
        List { start: start as u32, len: items.len() as u32 }
    }

    /// Gives back the capacity reserved for growth. Parsing reserves generously so it rarely
    /// has to reallocate, but a finished tree can stay in memory for the whole build.
    pub(crate) fn shrink_to_fit(&mut self) {
        // Spans are recorded as they are met, which is not always left to right (a type after
        // an arrow's parameters is met before the parameters' defaults are rewritten).
        self.erased.sort_by_key(|s| (s.start, s.end));
        // A member erased as a whole also had its modifiers and type erased on the way; keep
        // only the outermost, so the list really is non-overlapping.
        let mut merged: Vec<Span> = Vec::with_capacity(self.erased.len());
        for span in self.erased.drain(..) {
            match merged.last_mut() {
                Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
                _ => merged.push(span),
            }
        }
        self.erased = merged;
        self.nodes.shrink_to_fit();
        self.lists.shrink_to_fit();
        self.atoms.shrink_to_fit();
    }

    /// Marks `start..end` of the source as TypeScript-only text.
    pub(crate) fn erase(&mut self, start: u32, end: u32) {
        if end > start {
            self.erased.push(Span { start, end });
        }
    }

    pub(crate) fn rewrite(&mut self, start: u32, end: u32, text: &'static str) {
        self.rewrites.push((Span { start, end }, text));
    }

    /// The rewrites inside `start..end`.
    pub fn rewrites_within(&self, start: u32, end: u32) -> impl Iterator<Item = &(Span, &'static str)> {
        self.rewrites.iter().filter(move |(span, _)| span.start >= start && span.end <= end)
    }

    pub(crate) fn add_decorators(&mut self, target: NodeId, decorators: Vec<Decorator>) {
        if !decorators.is_empty() {
            self.decorators.push((target, decorators));
        }
    }

    /// The decorators on `target`, in source order.
    pub fn decorators_of(&self, target: NodeId) -> &[Decorator] {
        if self.decorators.is_empty() {
            return &[];
        }
        match self.decorators.binary_search_by_key(&target.index(), |(t, _)| t.index()) {
            Ok(i) => &self.decorators[i].1,
            Err(_) => &[],
        }
    }

    pub(crate) fn set_class_body(&mut self, class: NodeId, open: u32, close: u32) {
        self.class_bodies.push((class, Span { start: open, end: close }));
    }

    /// Where a class body's braces are: `start` is the `{`, `end` the offset of the `}`.
    pub fn class_body(&self, class: NodeId) -> Option<Span> {
        self.class_bodies.iter().find(|(c, _)| *c == class).map(|(_, s)| *s)
    }

    pub(crate) fn set_type_meta(&mut self, node: NodeId, meta: TypeMeta) {
        self.type_meta.insert(node, meta);
    }

    pub fn type_meta_of(&self, node: NodeId) -> Option<TypeMeta> {
        self.type_meta.get(&node).copied()
    }

    pub(crate) fn add_param_props(&mut self, function: NodeId, props: Vec<NodeId>) {
        self.param_props.push((function, props));
    }

    /// The parameters of constructor `function` that declare properties.
    pub fn param_props_of(&self, function: NodeId) -> &[NodeId] {
        match self.param_props.binary_search_by_key(&function.index(), |(f, _)| f.index()) {
            Ok(i) => &self.param_props[i].1,
            Err(_) => &[],
        }
    }

    /// Whether a namespace has anything that exists at run time. One that holds only types
    /// is gone from the output and binds no name.
    pub fn ts_namespace_has_values(&self, node: NodeId) -> bool {
        let NodeKind::TsNamespace { body, .. } = self[node].kind else { return false };
        self.list(body).iter().any(|&item| {
            let inner = match self[item].kind {
                NodeKind::TsExport(d) => d,
                _ => item,
            };
            match self[inner].kind {
                NodeKind::TsNamespace { .. } => self.ts_namespace_has_values(inner),
                NodeKind::Empty => false,
                _ => true,
            }
        })
    }

    pub(crate) fn erased_len(&self) -> usize {
        self.erased.len()
    }

    pub(crate) fn truncate_erased(&mut self, len: usize) {
        self.erased.truncate(len);
    }

    /// The erased spans that lie inside `start..end`.
    pub fn erased_within(&self, start: u32, end: u32) -> &[Span] {
        let first = self.erased.partition_point(|s| s.start < start);
        let last = self.erased.partition_point(|s| s.start < end);
        &self.erased[first..last.max(first)]
    }

    pub(crate) fn alloc_import(&mut self, data: ImportData) -> ImportId {
        self.imports.push(data);
        ImportId(self.imports.len() as u32 - 1)
    }

    pub(crate) fn alloc_export(&mut self, data: ExportData) -> ExportId {
        self.exports.push(data);
        ExportId(self.exports.len() as u32 - 1)
    }

    pub(crate) fn alloc_jsx(&mut self, data: JsxData) -> JsxId {
        self.jsx_elements.push(data);
        JsxId(self.jsx_elements.len() as u32 - 1)
    }

    pub fn jsx(&self, id: JsxId) -> &JsxData {
        &self.jsx_elements[id.0 as usize]
    }

    pub fn import(&self, id: ImportId) -> &ImportData {
        &self.imports[id.0 as usize]
    }

    pub fn export(&self, id: ExportId) -> &ExportData {
        &self.exports[id.0 as usize]
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub(crate) fn intern(&mut self, text: &str) -> Atom {
        self.atoms.intern(text)
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn list(&self, list: List) -> &[NodeId] {
        &self.lists[list.start as usize..(list.start + list.len) as usize]
    }

    pub fn atom(&self, atom: Atom) -> &str {
        self.atoms.get(atom)
    }

    /// The atom for `text` if the file uses that text anywhere, without interning it.
    pub fn find_atom(&self, text: &str) -> Option<Atom> {
        self.atoms.find(text)
    }

    /// The node at position `index` (1 to `node_count()`), for scans that need no structure.
    pub fn node_id(&self, index: usize) -> NodeId {
        assert!(index >= 1 && index < self.nodes.len());
        NodeId(NonZeroU32::new(index as u32).unwrap())
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len() - 1
    }

    /// Checks the structural promises other passes rely on: every child lies inside its
    /// parent's span, and siblings appear in source order without overlapping. Returns a
    /// description of the first violation. Intended for tests and debugging; it walks the
    /// whole tree.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let node = self.node(id);
            let mut cursor = node.span.start;
            let mut problem = None;
            self.for_each_child(id, |child| {
                let span = self.node(child).span;
                // `{ a }` and `{ a = 1 }` use one name as both key and value.
                let shares_name = node.has(flags::SHORTHAND) && matches!(node.kind, NodeKind::Property { .. } | NodeKind::PatternProp { .. });
                if problem.is_none() {
                    if span.start < node.span.start || span.end > node.span.end {
                        problem = Some(format!("{:?} at {:?} has a child outside it: {:?} at {:?}", node.kind, node.span, self.node(child).kind, span));
                    } else if span.start < cursor && !shares_name {
                        problem = Some(format!("{:?} at {:?} has overlapping or reordered children near {:?} at {:?}", node.kind, node.span, self.node(child).kind, span));
                    }
                }
                cursor = cursor.max(span.end);
                stack.push(child);
            });
            if let Some(problem) = problem {
                return Err(problem);
            }
        }
        Ok(())
    }

    /// Calls `f` for each direct child in source order.
    pub fn for_each_child(&self, id: NodeId, mut f: impl FnMut(NodeId)) {
        use NodeKind::*;
        // Decorators come before what they decorate.
        for decorator in self.decorators_of(id) {
            f(decorator.expr);
        }
        // So does a type that is serialized by name for decorator metadata.
        if !self.type_meta.is_empty() {
            if let Some(TypeMeta::Ref(reference)) = self.type_meta.get(&id) {
                f(*reference);
            }
        }
        let list = |l: List, f: &mut dyn FnMut(NodeId)| {
            for &child in self.list(l) {
                f(child);
            }
        };
        match self.node(id).kind {
            Ident(_) | PropName(_) | PrivateName(_) | This | Super | Null | Bool(_) | Num(_) | BigInt(_) | Str(_)
            | Regex | TemplateElement { .. } | Hole | ImportMeta | NewTarget | BindingIdent(_) | Empty | Debugger
            | Break(_) | Continue(_) | ExportSpecifier { .. } | ImportAttr { .. } => {}
            TsEnum { members, .. } => list(members, &mut f),
            TsEnumMember { init, .. } => {
                init.map(&mut f);
            }
            TsNamespace { body, .. } => list(body, &mut f),
            TsExport(decl) => f(decl),
            JsxElement(data) => {
                let data = self.jsx(data);
                f(data.tag);
                list(data.attrs, &mut f);
                list(data.children, &mut f);
            }
            JsxFragment { children } => list(children, &mut f),
            JsxAttr { value, .. } => {
                value.map(&mut f);
            }
            JsxSpreadAttr(expr) => f(expr),
            JsxText(_) => {}
            Template { quasis, exprs } => {
                // Interleaved: quasi, expr, quasi, expr, ..., quasi.
                let (quasis, exprs) = (self.list(quasis), self.list(exprs));
                for (i, &q) in quasis.iter().enumerate() {
                    f(q);
                    if let Some(&e) = exprs.get(i) {
                        f(e);
                    }
                }
            }
            TaggedTemplate { tag, quasi } => {
                f(tag);
                f(quasi);
            }
            Array(l) | Object(l) | StaticBlock(l) | Sequence(l) | ObjectPattern(l) | ArrayPattern(l) | Program(l)
            | Block(l) => list(l, &mut f),
            Property { key, value, .. } | ClassMethod { key, value, .. } | PatternProp { key, value } => {
                f(key);
                f(value);
            }
            Spread(a) | Chain(a) | Await(a) | Rest(a) | ExprStmt(a) | Throw(a) | ExportDefault(a) | ImportDefault(a)
            | ImportNamespace(a) => f(a),
            Unary { arg, .. } | Update { arg, .. } => f(arg),
            Function { name, params, body } => {
                name.map(&mut f);
                list(params, &mut f);
                f(body);
            }
            Class { name, superclass, members } => {
                name.map(&mut f);
                superclass.map(&mut f);
                list(members, &mut f);
            }
            ClassField { key, value } => {
                f(key);
                value.map(&mut f);
            }
            Binary { left, right, .. } => {
                f(left);
                f(right);
            }
            Assign { target, value, .. } => {
                f(target);
                f(value);
            }
            Conditional { test, cons, alt } => {
                f(test);
                f(cons);
                f(alt);
            }
            Call { callee, args } | New { callee, args } => {
                f(callee);
                list(args, &mut f);
            }
            Member { object, prop } => {
                f(object);
                f(prop);
            }
            Yield(arg) | Return(arg) => {
                arg.map(&mut f);
            }
            ImportCall { source, options } => {
                f(source);
                options.map(&mut f);
            }
            AssignPattern { target, default } => {
                f(target);
                f(default);
            }
            VarDecl { decls, .. } => list(decls, &mut f),
            VarDeclarator { target, init } => {
                f(target);
                init.map(&mut f);
            }
            If { test, cons, alt } => {
                f(test);
                f(cons);
                alt.map(&mut f);
            }
            For { init, test, update, body } => {
                init.map(&mut f);
                test.map(&mut f);
                update.map(&mut f);
                f(body);
            }
            ForIn { left, right, body } | ForOf { left, right, body } => {
                f(left);
                f(right);
                f(body);
            }
            While { test, body } => {
                f(test);
                f(body);
            }
            DoWhile { body, test } => {
                f(body);
                f(test);
            }
            Try { block, handler, finalizer } => {
                f(block);
                handler.map(&mut f);
                finalizer.map(&mut f);
            }
            Catch { param, body } => {
                param.map(&mut f);
                f(body);
            }
            Switch { disc, cases } => {
                f(disc);
                list(cases, &mut f);
            }
            Case { test, body } => {
                test.map(&mut f);
                list(body, &mut f);
            }
            Labeled { body, .. } => f(body),
            With { object, body } => {
                f(object);
                f(body);
            }
            Import(import) => {
                let data = self.import(import);
                list(data.specifiers, &mut f);
                list(data.attrs, &mut f);
            }
            ImportSpecifier { local, .. } => f(local),
            ExportNamed(export) => {
                let data = self.export(export);
                list(data.specifiers, &mut f);
                data.decl.map(&mut f);
                list(data.attrs, &mut f);
            }
            ExportAll { attrs, .. } => list(attrs, &mut f),
        }
    }
}

impl Index<NodeId> for Ast {
    type Output = Node;

    fn index(&self, id: NodeId) -> &Node {
        self.node(id)
    }
}

// ---- interner ---------------------------------------------------------------------------

/// Multiply-rotate hash (the same idea as rustc's FxHash). The standard SipHash is built to
/// resist hostile keys, which costs a lot and buys nothing for identifier names.
#[derive(Default)]
pub(crate) struct FxHasher(u64);

pub(crate) type FxBuildHasher = BuildHasherDefault<FxHasher>;

const FX_SEED: u64 = 0x517c_c1b7_2722_0a95;

impl FxHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(FX_SEED);
    }
}

impl Hasher for FxHasher {
    // Atom and node ids are hashed constantly; skip the generic byte path for them.
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Names the parser and analyzer compare against constantly. They are interned first, in
/// this order, so each has a fixed `Atom` and a check like "is this `arguments`?" is an
/// integer comparison instead of a string match.
const WELL_KNOWN: [&str; 17] = [
    // Words reserved only in some contexts come first: `RESERVED_CANDIDATES` counts them, so
    // `atom.index() <= RESERVED_CANDIDATES` is a cheap first test before any string match.
    "yield", "await", "let", "static", "implements", "interface", "package", "private", "protected", "public",
    "arguments", "eval", "async", "default", "constructor", "prototype", "__proto__",
];

/// How many of the `WELL_KNOWN` atoms (counting from the first) may be reserved words.
pub const RESERVED_CANDIDATES: usize = 12;

/// Atoms with fixed identities; see `WELL_KNOWN`.
pub mod known {
    use super::Atom;
    use std::num::NonZeroU32;

    const fn at(index: u32) -> Atom {
        Atom(NonZeroU32::new(index).unwrap())
    }

    pub const YIELD: Atom = at(1);
    pub const AWAIT: Atom = at(2);
    pub const LET: Atom = at(3);
    pub const STATIC: Atom = at(4);
    pub const IMPLEMENTS: Atom = at(5);
    pub const INTERFACE: Atom = at(6);
    pub const PACKAGE: Atom = at(7);
    pub const PRIVATE: Atom = at(8);
    pub const PROTECTED: Atom = at(9);
    pub const PUBLIC: Atom = at(10);
    pub const ARGUMENTS: Atom = at(11);
    pub const EVAL: Atom = at(12);
    pub const ASYNC: Atom = at(13);
    pub const DEFAULT: Atom = at(14);
    pub const CONSTRUCTOR: Atom = at(15);
    pub const PROTOTYPE: Atom = at(16);
    pub const PROTO: Atom = at(17);
}

/// Deduplicates names and string values. All text lives back to back in one buffer, so
/// interning a new name allocates nothing in the common case, and the lookup table holds
/// only `u32` ids (with a stored hash to skip most string comparisons).
struct Interner {
    text: String,
    /// `ends[i]` is where atom `i` stops in `text`; it starts where atom `i - 1` stops.
    /// Index 0 is a placeholder so `Atom` can be non-zero.
    ends: Vec<u32>,
    hashes: Vec<u32>,
    /// Open addressing with linear probing; 0 means empty. Length is a power of two.
    table: Vec<u32>,
}

/// Multiply-rotate hash over 8-byte words (the same idea as rustc's FxHash).
fn hash_text(text: &[u8]) -> u32 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h = (text.len() as u64).wrapping_mul(K);
    let mut chunks = text.chunks_exact(8);
    for chunk in &mut chunks {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(chunk.try_into().unwrap())).wrapping_mul(K);
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut buf = [0u8; 8];
        buf[..rest.len()].copy_from_slice(rest);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(buf)).wrapping_mul(K);
    }
    // The low bits of a product depend only on the low bits of its inputs; fold the high half in.
    (h ^ (h >> 32)) as u32
}

impl Interner {
    /// Sizes the buffers for a source file of `source_len` bytes, so that interning rarely
    /// has to grow them. Real code has roughly one distinct name per 40 bytes.
    fn with_source_len(source_len: usize) -> Interner {
        let names = source_len / 40 + 32;
        let mut ends = Vec::with_capacity(names);
        ends.push(0);
        let mut hashes = Vec::with_capacity(names);
        hashes.push(0);
        let mut interner = Interner {
            text: String::with_capacity(source_len / 10 + 64),
            ends,
            hashes,
            table: vec![0; (names * 2).next_power_of_two()],
        };
        for word in WELL_KNOWN {
            interner.intern(word);
        }
        interner
    }

    fn intern(&mut self, text: &str) -> Atom {
        let hash = hash_text(text.as_bytes());
        let mask = self.table.len() - 1;
        let mut slot = hash as usize & mask;
        loop {
            let id = self.table[slot];
            if id == 0 {
                break;
            }
            if self.hashes[id as usize] == hash && self.slice(id as usize) == text {
                return Atom(NonZeroU32::new(id).unwrap());
            }
            slot = (slot + 1) & mask;
        }
        let id = self.ends.len() as u32;
        assert!(self.text.len() + text.len() < u32::MAX as usize, "atom text overflow");
        self.text.push_str(text);
        self.ends.push(self.text.len() as u32);
        self.hashes.push(hash);
        self.table[slot] = id;
        // Keep the table at most half full.
        if self.ends.len() * 2 > self.table.len() {
            self.grow();
        }
        Atom(NonZeroU32::new(id).unwrap())
    }

    fn find(&self, text: &str) -> Option<Atom> {
        let hash = hash_text(text.as_bytes());
        let mask = self.table.len() - 1;
        let mut slot = hash as usize & mask;
        loop {
            let id = self.table[slot];
            if id == 0 {
                return None;
            }
            if self.hashes[id as usize] == hash && self.slice(id as usize) == text {
                return Some(Atom(NonZeroU32::new(id).unwrap()));
            }
            slot = (slot + 1) & mask;
        }
    }

    fn grow(&mut self) {
        let mut table = vec![0u32; self.table.len() * 2];
        let mask = table.len() - 1;
        for id in 1..self.ends.len() {
            let mut slot = self.hashes[id] as usize & mask;
            while table[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            table[slot] = id as u32;
        }
        self.table = table;
    }

    fn shrink_to_fit(&mut self) {
        self.text.shrink_to_fit();
        self.ends.shrink_to_fit();
        self.hashes.shrink_to_fit();
        // The table is not needed to read atoms back, only to intern new ones, and a finished
        // tree rarely interns more (the bundler's renamer does). Keep it; it is small.
    }

    fn slice(&self, id: usize) -> &str {
        &self.text[self.ends[id - 1] as usize..self.ends[id] as usize]
    }

    fn get(&self, atom: Atom) -> &str {
        self.slice(atom.index())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_stay_small() {
        // Four 4-byte fields plus the tag. Anything bigger belongs in a side table.
        assert!(std::mem::size_of::<NodeKind>() <= 20, "NodeKind is {} bytes", std::mem::size_of::<NodeKind>());
        assert!(std::mem::size_of::<Node>() <= 32, "Node is {} bytes", std::mem::size_of::<Node>());
        assert_eq!(std::mem::size_of::<Option<NodeId>>(), 4);
        assert_eq!(std::mem::size_of::<Option<Atom>>(), 4);
    }

    #[test]
    fn well_known_atoms_have_their_fixed_ids() {
        let ast = Ast::new(0);
        for (atom, text) in [
            (known::YIELD, "yield"),
            (known::AWAIT, "await"),
            (known::LET, "let"),
            (known::STATIC, "static"),
            (known::IMPLEMENTS, "implements"),
            (known::INTERFACE, "interface"),
            (known::PACKAGE, "package"),
            (known::PRIVATE, "private"),
            (known::PROTECTED, "protected"),
            (known::PUBLIC, "public"),
            (known::ARGUMENTS, "arguments"),
            (known::EVAL, "eval"),
            (known::ASYNC, "async"),
            (known::DEFAULT, "default"),
            (known::CONSTRUCTOR, "constructor"),
            (known::PROTOTYPE, "prototype"),
            (known::PROTO, "__proto__"),
        ] {
            assert_eq!(ast.atom(atom), text);
        }
        assert_eq!(known::PUBLIC.index(), RESERVED_CANDIDATES - 2);
        assert_eq!(known::EVAL.index(), RESERVED_CANDIDATES);
        let mut ast = Ast::new(0);
        assert_eq!(ast.intern("arguments"), known::ARGUMENTS);
    }

    #[test]
    fn interner_survives_growth_and_collisions() {
        let mut ast = Ast::new(0);
        let names: Vec<String> = (0..5000).map(|i| format!("name{i}_{}", i * 7919 % 1000)).collect();
        let atoms: Vec<Atom> = names.iter().map(|n| ast.intern(n)).collect();
        for (name, &atom) in names.iter().zip(&atoms) {
            assert_eq!(ast.atom(atom), name);
            assert_eq!(ast.intern(name), atom, "re-interning must find the same atom");
        }
        let unique: std::collections::HashSet<_> = atoms.iter().collect();
        assert_eq!(unique.len(), names.len());
        // Empty string and long strings work too.
        let empty = ast.intern("");
        assert_eq!(ast.atom(empty), "");
        let long = "x".repeat(10_000);
        let l = ast.intern(&long);
        assert_eq!(ast.atom(l), long);
    }

    #[test]
    fn interning_dedupes() {
        let mut ast = Ast::new(0);
        let a = ast.intern("foo");
        let b = ast.intern("bar");
        assert_ne!(a, b);
        assert_eq!(ast.intern("foo"), a);
        assert_eq!(ast.atom(a), "foo");
        assert_eq!(ast.atom(b), "bar");
    }

    #[test]
    fn num_round_trips() {
        for v in [0.0, -0.0, 1.5, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY] {
            assert_eq!(Num::new(v).get().to_bits(), v.to_bits());
        }
        assert!(Num::new(f64::NAN).get().is_nan());
    }
}
