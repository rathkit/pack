//! Scopes, symbols, and which symbol every identifier refers to.
//!
//! The parser builds this as it goes. It opens and closes scopes, declares each binding
//! the moment it parses it (hoisting `var` and functions as the language requires), and
//! records every identifier reference together with the scope it appeared in. Once the
//! whole file is parsed, [`ScopeBuilder::finish`] resolves the recorded references by
//! walking up each one's scope chain. No pass over the tree is needed, which is why
//! scope analysis costs a fraction of what a separate traversal did.
//!
//! Approximations, all in sloppy-mode scripts:
//! - Annex B function-in-block hoisting links the block function to a `var` in the
//!   enclosing function. It is decided after all declarations are known, so a lexical
//!   declaration anywhere in between cancels it, as the specification requires.
//! - A direct call to `eval` marks every scope enclosing the call (`scope_flags::HAS_EVAL`);
//!   symbols in those scopes must not be renamed. If the call sits in an arrow function's
//!   parameter defaults, the arrow's own scope is not marked. Nothing else about `eval` is
//!   modeled.
//! - A reference that resolves through a `with` scope is flagged `symbol_flags::DYNAMIC`.

use std::collections::HashMap;
use std::fmt;
use std::num::NonZeroU32;

use crate::ast::{Ast, Atom, FxBuildHasher, NodeId, NodeKind, flags, known};

type Map<K, V> = HashMap<K, V, FxBuildHasher>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScopeId(u32);

impl ScopeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolId(NonZeroU32);

impl SymbolId {
    pub fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// Top level of an ES module.
    Module,
    /// Top level of a script, treated as a CommonJS function body.
    Script,
    /// A function's parameters (and, with simple parameters, its body). Arrow functions are
    /// marked with `scope_flags::ARROW`.
    Function,
    /// A function body kept apart from its parameters, because the parameters have
    /// defaults or patterns whose expressions must not see the body's declarations.
    Body,
    Block,
    /// A `catch` clause: the parameter and the body's declarations.
    Catch,
    /// The head and body of a `for` loop that declares `let` or `const`.
    For,
    Switch,
    /// A class body, holding the class's inner name binding.
    Class,
    StaticBlock,
    With,
}

impl ScopeKind {
    /// Where `var` declarations land.
    pub fn is_var_scope(self) -> bool {
        matches!(self, ScopeKind::Module | ScopeKind::Script | ScopeKind::Function | ScopeKind::Body | ScopeKind::StaticBlock)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Var,
    Let,
    Const,
    Class,
    /// A function declaration that behaves like `var`: at the top of a function or script.
    Function,
    /// A function declaration that is lexical: at the top of a module, or an async or
    /// generator function in a block.
    LexicalFunction,
    /// A plain function declaration in a block. Sloppy code may declare it twice.
    BlockFunction,
    Param,
    /// A `catch (e)` parameter.
    CatchParam,
    /// A destructured `catch ({e})` parameter.
    CatchPatternParam,
    Import,
    /// The own name of a function expression, visible only inside it.
    FunctionName,
    /// The name of a class as seen from inside its body.
    ClassInner,
    /// The implicit `arguments` object of a non-arrow function.
    Arguments,
}

impl SymbolKind {
    fn is_lexical(self) -> bool {
        matches!(
            self,
            SymbolKind::Let | SymbolKind::Const | SymbolKind::Class | SymbolKind::Import | SymbolKind::LexicalFunction | SymbolKind::BlockFunction
        )
    }
}

pub mod symbol_flags {
    /// Named in an `export` declaration or specifier.
    pub const EXPORTED: u8 = 1 << 0;
    /// Some reference reached this symbol through a `with` statement, so the name may in
    /// fact have been captured by the `with` object at run time.
    pub const DYNAMIC: u8 = 1 << 1;
}

pub mod scope_flags {
    /// The scope, or a scope nested in it, contains a direct `eval(...)` call.
    pub const HAS_EVAL: u8 = 1 << 0;
    /// A function scope belonging to an arrow function (which has no `arguments`).
    pub const ARROW: u8 = 1 << 1;
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: Atom,
    pub kind: SymbolKind,
    pub scope: ScopeId,
    /// The binding identifier that first declared it. `None` for implicit symbols.
    pub decl: Option<NodeId>,
    pub reads: u32,
    pub writes: u32,
    pub flags: u8,
    /// For a function declared in a block in sloppy code: the `var` it is also hoisted to.
    pub alias: Option<SymbolId>,
}

#[derive(Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    /// The node that created the scope (the function, block, class, ...).
    pub node: Option<NodeId>,
    pub flags: u8,
    /// Symbols in declaration order, so output is deterministic. Includes bindings that a
    /// later declaration shadowed. Most scopes hold a handful of names, so finding one is a
    /// scan of this list (newest first, so a later declaration shadows an earlier one).
    declared: Vec<SymbolId>,
    /// A name index, built only once a scope grows large (a module with hundreds of names).
    index: Option<Box<Map<Atom, SymbolId>>>,
    /// Names of `var`s hoisted through this scope, which a later `let` here may not reuse.
    hoisted: Vec<Atom>,
}

/// Past this many symbols a scope gets a hash index instead of a linear scan.
const INDEX_THRESHOLD: usize = 16;

impl Scope {
    fn new(kind: ScopeKind, parent: Option<ScopeId>, flags: u8) -> Scope {
        Scope { kind, parent, node: None, flags, declared: Vec::new(), index: None, hoisted: Vec::new() }
    }

    /// Symbols declared in this scope, in source order.
    pub fn symbols(&self) -> &[SymbolId] {
        &self.declared
    }
}

#[derive(Debug)]
pub struct Semantic {
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    /// Indexed by node: the symbol a binding or reference identifier denotes.
    ident_symbols: Vec<Option<SymbolId>>,
    unresolved: Vec<NodeId>,
}

impl Semantic {
    pub fn root_scope(&self) -> ScopeId {
        ScopeId(0)
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.index()]
    }

    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.index()]
    }

    pub fn scopes(&self) -> impl Iterator<Item = (ScopeId, &Scope)> {
        self.scopes.iter().enumerate().map(|(i, s)| (ScopeId(i as u32), s))
    }

    pub fn symbols(&self) -> impl Iterator<Item = (SymbolId, &Symbol)> {
        self.symbols.iter().enumerate().map(|(i, s)| (SymbolId(NonZeroU32::new(i as u32 + 1).unwrap()), s))
    }

    /// The symbol a binding identifier declares or a reference identifier resolves to.
    /// `None` for names that resolve to no declaration (globals).
    pub fn symbol_of(&self, node: NodeId) -> Option<SymbolId> {
        self.ident_symbols.get(node.index()).copied().flatten()
    }

    /// Reference identifiers that resolved to no declaration, in source order.
    pub fn unresolved(&self) -> &[NodeId] {
        &self.unresolved
    }

    /// Finds `name` among the symbols declared directly in `scope`.
    pub fn lookup_local(&self, scope: ScopeId, name: Atom) -> Option<SymbolId> {
        find_symbol(&self.scopes[scope.index()], &self.symbols, name)
    }

    /// Finds `name` starting at `scope` and walking outwards.
    pub fn lookup(&self, mut scope: ScopeId, name: Atom) -> Option<SymbolId> {
        loop {
            if let Some(sym) = self.lookup_local(scope, name) {
                return Some(sym);
            }
            scope = self.scope(scope).parent?;
        }
    }
}

/// A scope problem found after parsing (an export of a name that does not exist).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticError {
    pub message: String,
    /// Byte offset of the offending declaration or reference.
    pub pos: u32,
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.pos)
    }
}

impl std::error::Error for SemanticError {}

fn find_symbol(scope: &Scope, symbols: &[Symbol], name: Atom) -> Option<SymbolId> {
    if let Some(index) = &scope.index {
        return index.get(&name).copied();
    }
    scope.declared.iter().rev().copied().find(|id| symbols[id.index()].name == name)
}

/// A declaration that collides with an earlier one.
pub(crate) struct Redeclared {
    pub name: Atom,
    pub node: NodeId,
}

/// Collects scopes, symbols and references while the parser runs.
pub(crate) struct ScopeBuilder {
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    /// Binding identifiers and the symbols they declare. Turned into a table indexed by node
    /// once the tree is complete, because the node count is not known until then.
    bindings: Vec<(NodeId, SymbolId)>,
    /// Binding identifiers that were given an initial value, each counted as one write.
    initial_writes: Vec<NodeId>,
    /// Every identifier reference with the scope it appeared in, in source order.
    refs: Vec<(NodeId, ScopeId)>,
    current: ScopeId,
    /// Sloppy block functions awaiting an Annex B hoist: (block scope, symbol, node).
    pending_hoists: Vec<(ScopeId, SymbolId, NodeId)>,
    /// Declarations that follow `export`, to flag their symbols once all are known.
    export_decls: Vec<NodeId>,
    /// `export { a, b }` lists without a `from`, whose names must exist.
    export_lists: Vec<NodeId>,
    /// A function may be declared again in the same scope (TypeScript overloads).
    merge_functions: bool,
    /// `export { a }` need not find `a` (the text is part of a larger file).
    unbound_exports: bool,
}

impl ScopeBuilder {
    pub fn new(module: bool, source_len: usize) -> ScopeBuilder {
        let kind = if module { ScopeKind::Module } else { ScopeKind::Script };
        // Real code has roughly one scope per 150 bytes.
        let mut scopes = Vec::with_capacity(source_len / 150 + 8);
        scopes.push(Scope::new(kind, None, 0));
        ScopeBuilder {
            scopes,
            symbols: Vec::with_capacity(source_len / 60),
            bindings: Vec::with_capacity(source_len / 60),
            initial_writes: Vec::new(),
            refs: Vec::with_capacity(source_len / 20),
            current: ScopeId(0),
            pending_hoists: Vec::new(),
            export_decls: Vec::new(),
            export_lists: Vec::new(),
            merge_functions: false,
            unbound_exports: false,
        }
    }

    pub fn allow_unbound_exports(&mut self) {
        self.unbound_exports = true;
    }

    pub fn allow_function_merging(&mut self) {
        self.merge_functions = true;
    }

    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    pub fn ref_count(&self) -> usize {
        self.refs.len()
    }

    pub fn scope_kind(&self) -> ScopeKind {
        self.scopes[self.current.index()].kind
    }

    pub fn push(&mut self, kind: ScopeKind, flags: u8) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope::new(kind, Some(self.current), flags));
        self.current = id;
        id
    }

    /// Like [`ScopeBuilder::push`], for a function discovered to be an arrow only after its
    /// parameters were parsed as ordinary expressions. Scopes and references created since
    /// the marks, which the enclosing scope owns, now belong inside the arrow's scope.
    pub fn push_adopting(&mut self, kind: ScopeKind, flags: u8, scope_mark: usize, ref_mark: usize) -> ScopeId {
        let outer = self.current;
        let id = ScopeId(self.scopes.len() as u32);
        for scope in &mut self.scopes[scope_mark..] {
            if scope.parent == Some(outer) {
                scope.parent = Some(id);
            }
        }
        for r in &mut self.refs[ref_mark..] {
            if r.1 == outer {
                r.1 = id;
            }
        }
        self.push(kind, flags)
    }

    pub fn pop(&mut self, node: NodeId) {
        let scope = &mut self.scopes[self.current.index()];
        scope.node = Some(node);
        self.current = scope.parent.expect("popped the root scope");
    }

    pub fn set_root_node(&mut self, node: NodeId) {
        self.scopes[0].node = Some(node);
    }

    pub fn record_ref(&mut self, node: NodeId) {
        self.refs.push((node, self.current));
    }

    /// Records a reference that belongs earlier in source order than the ones already
    /// recorded (rare: the `async` of an `async(...)` call, known only after its arguments).
    pub fn insert_ref(&mut self, index: usize, node: NodeId) {
        self.refs.insert(index, (node, self.current));
    }

    /// The identifier at the end of the reference list was not a reference after all (a
    /// label).
    pub fn forget_ref(&mut self, node: NodeId) {
        if self.refs.last().is_some_and(|r| r.0 == node) {
            self.refs.pop();
        }
    }

    pub fn mark_direct_eval(&mut self) {
        let mut scope_id = Some(self.current);
        while let Some(id) = scope_id {
            let scope = &mut self.scopes[id.index()];
            scope.flags |= scope_flags::HAS_EVAL;
            scope_id = scope.parent;
        }
    }

    pub fn note_export_decl(&mut self, decl: NodeId) {
        self.export_decls.push(decl);
    }

    pub fn note_export_list(&mut self, node: NodeId) {
        self.export_lists.push(node);
    }

    /// Counts one write to the symbol a binding identifier declared (an initializer).
    pub fn add_write(&mut self, binding: NodeId) {
        self.initial_writes.push(binding);
    }

    fn find(&self, scope: ScopeId, name: Atom) -> Option<SymbolId> {
        find_symbol(&self.scopes[scope.index()], &self.symbols, name)
    }

    // ---- declarations -----------------------------------------------------------------

    fn add_symbol(&mut self, scope: ScopeId, name: Atom, kind: SymbolKind, node: Option<NodeId>) -> SymbolId {
        let id = SymbolId(NonZeroU32::new(self.symbols.len() as u32 + 1).expect("symbol table overflow"));
        self.symbols.push(Symbol { name, kind, scope, decl: node, reads: 0, writes: 0, flags: 0, alias: None });
        let s = &mut self.scopes[scope.index()];
        s.declared.push(id);
        if let Some(index) = &mut s.index {
            index.insert(name, id);
        } else if s.declared.len() > INDEX_THRESHOLD {
            // Later symbols win, as in the linear scan.
            let symbols = &self.symbols;
            s.index = Some(Box::new(s.declared.iter().map(|&d| (symbols[d.index()].name, d)).collect()));
        }
        if let Some(n) = node {
            self.map_ident(n, id);
        }
        id
    }

    fn map_ident(&mut self, node: NodeId, sym: SymbolId) {
        self.bindings.push((node, sym));
    }

    /// `let`, `const`, `class`, imports and block-level functions: bound in the current
    /// scope, and it may not collide with anything already there or hoisted through.
    pub fn declare_lexical(&mut self, name: Atom, kind: SymbolKind, node: NodeId, strict: bool) -> Result<SymbolId, Redeclared> {
        let scope_id = self.current;
        let scope = &self.scopes[scope_id.index()];
        if let Some(existing) = self.find(scope_id, name) {
            match self.symbols[existing.index()].kind {
                // TypeScript overloads declare one function several times.
                SymbolKind::LexicalFunction if kind == SymbolKind::LexicalFunction && self.merge_functions => {
                    self.map_ident(node, existing);
                    return Ok(existing);
                }
                // An expression's own name is visible from an outer, implicit scope, so any
                // declaration inside may shadow it.
                SymbolKind::FunctionName => {}
                // Sloppy code may declare the same plain function twice in a block.
                SymbolKind::BlockFunction if kind == SymbolKind::BlockFunction && !strict => {
                    self.map_ident(node, existing);
                    return Ok(existing);
                }
                _ => return Err(Redeclared { name, node }),
            }
        }
        if scope.hoisted.contains(&name) {
            return Err(Redeclared { name, node });
        }
        // With separate parameter and body scopes, the body still may not redeclare a
        // parameter lexically.
        if scope.kind == ScopeKind::Body {
            let parent = scope.parent.expect("a body scope has a function scope above it");
            if let Some(p) = self.find(parent, name) {
                if self.symbols[p.index()].kind == SymbolKind::Param {
                    return Err(Redeclared { name, node });
                }
            }
        }
        Ok(self.add_symbol(scope_id, name, kind, Some(node)))
    }

    /// `var` and var-like functions: bound in the nearest var scope, passing through (and
    /// leaving a mark in) every scope between here and there.
    pub fn declare_var(&mut self, name: Atom, kind: SymbolKind, node: NodeId) -> Result<SymbolId, Redeclared> {
        let mut scope_id = self.current;
        loop {
            let scope = &self.scopes[scope_id.index()];
            let is_var_scope = scope.kind.is_var_scope();
            if let Some(existing) = self.find(scope_id, name) {
                match self.symbols[existing.index()].kind {
                    // A simple `catch (e)` parameter does not stop `var e` (Annex B).
                    SymbolKind::CatchParam => {}
                    SymbolKind::CatchPatternParam => return Err(Redeclared { name, node }),
                    SymbolKind::FunctionName if is_var_scope => {
                        // Shadows the function's own name.
                        return Ok(self.add_symbol(scope_id, name, kind, Some(node)));
                    }
                    k if k.is_lexical() => return Err(Redeclared { name, node }),
                    _ if is_var_scope => {
                        self.map_ident(node, existing);
                        return Ok(existing);
                    }
                    _ => {}
                }
            }
            if is_var_scope {
                return Ok(self.add_symbol(scope_id, name, kind, Some(node)));
            }
            let scope = &mut self.scopes[scope_id.index()];
            if !scope.hoisted.contains(&name) {
                scope.hoisted.push(name);
            }
            scope_id = scope.parent.expect("every scope chain ends in a var scope");
        }
    }

    pub fn declare_param(&mut self, name: Atom, node: NodeId) {
        match self.find(self.current, name).map(|s| (s, self.symbols[s.index()].kind)) {
            // Sloppy code may repeat a parameter name: they share one binding.
            Some((existing, SymbolKind::Param)) => self.map_ident(node, existing),
            _ => {
                self.add_symbol(self.current, name, SymbolKind::Param, Some(node));
            }
        }
    }

    /// A catch parameter; repeating a name within one pattern is an error.
    pub fn declare_catch_param(&mut self, name: Atom, kind: SymbolKind, node: NodeId) -> Result<(), Redeclared> {
        if self.find(self.current, name).is_some() {
            return Err(Redeclared { name, node });
        }
        self.add_symbol(self.current, name, kind, Some(node));
        Ok(())
    }

    /// A function expression's own name, visible inside only.
    pub fn declare_function_expr_name(&mut self, name: Atom, node: NodeId) {
        self.add_symbol(self.current, name, SymbolKind::FunctionName, Some(node));
    }

    /// A class's name as seen from inside its body.
    pub fn declare_class_inner(&mut self, name: Atom, node: NodeId, is_expression: bool) {
        let inner = self.add_symbol(self.current, name, SymbolKind::ClassInner, None);
        self.symbols[inner.index()].decl = Some(node);
        // A class expression's name node denotes the inner binding; a declaration's already
        // denotes the outer one.
        if is_expression {
            self.map_ident(node, inner);
        }
    }

    /// `namespace N`: a variable, unless `N` already names a class, function, enum or earlier
    /// namespace in this scope, which it then merges with.
    pub fn declare_namespace(&mut self, name: Atom, node: NodeId) -> Result<(), Redeclared> {
        if let Some(existing) = self.find(self.current, name) {
            match self.symbols[existing.index()].kind {
                SymbolKind::Import => return Err(Redeclared { name, node }),
                _ => {
                    self.map_ident(node, existing);
                    return Ok(());
                }
            }
        }
        self.declare_var(name, SymbolKind::Var, node).map(|_| ())
    }

    /// A function declaration's name, bound according to where the declaration sits.
    pub fn declare_function_name(&mut self, name: Atom, node: NodeId, plain: bool, strict: bool) -> Result<(), Redeclared> {
        match self.scope_kind() {
            ScopeKind::Module => {
                self.declare_lexical(name, SymbolKind::LexicalFunction, node, strict)?;
            }
            ScopeKind::Script | ScopeKind::Function | ScopeKind::Body | ScopeKind::StaticBlock => {
                self.declare_var(name, SymbolKind::Function, node)?;
            }
            _ => {
                let kind = if plain { SymbolKind::BlockFunction } else { SymbolKind::LexicalFunction };
                let sym = self.declare_lexical(name, kind, node, strict)?;
                if plain && !strict {
                    self.pending_hoists.push((self.current, sym, node));
                }
            }
        }
        Ok(())
    }

    /// Annex B.3.3: in sloppy code a function declared in a block is also visible as a `var`
    /// in the enclosing function, unless that would collide with a lexical declaration or a
    /// parameter. Runs once every declaration is known, so the order they appear in does not
    /// matter.
    fn hoist_block_functions(&mut self) {
        for (block, symbol, node) in std::mem::take(&mut self.pending_hoists) {
            self.hoist_block_function(block, symbol, node);
        }
    }

    fn hoist_block_function(&mut self, block: ScopeId, block_symbol: SymbolId, node: NodeId) {
        let name = self.symbols[block_symbol.index()].name;
        let mut scope_id = self.scopes[block.index()].parent;
        while let Some(id) = scope_id {
            let scope = &self.scopes[id.index()];
            if let Some(existing) = self.find(id, name) {
                match self.symbols[existing.index()].kind {
                    SymbolKind::CatchParam | SymbolKind::FunctionName => {}
                    SymbolKind::Var | SymbolKind::Function | SymbolKind::Arguments if scope.kind.is_var_scope() => {
                        self.symbols[block_symbol.index()].alias = Some(existing);
                        return;
                    }
                    // A parameter or lexical binding blocks the hoist; nothing is declared.
                    _ => return,
                }
            }
            if scope.kind.is_var_scope() {
                let var = self.add_symbol(id, name, SymbolKind::Var, None);
                self.symbols[var.index()].decl = Some(node);
                self.symbols[block_symbol.index()].alias = Some(var);
                return;
            }
            scope_id = scope.parent;
        }
    }

    // ---- resolution -------------------------------------------------------------------

    /// Resolves every recorded reference and checks the export lists. Consumes the builder.
    pub fn finish(mut self, ast: &Ast) -> Result<Semantic, SemanticError> {
        self.hoist_block_functions();
        // Now the node count is known: turn the declaration pairs into a table by node.
        let mut ident_symbols = vec![None; ast.node_count() + 1];
        for &(node, sym) in &self.bindings {
            ident_symbols[node.index()] = Some(sym);
        }
        let mut unresolved = Vec::new();
        let refs = std::mem::take(&mut self.refs);
        for (node, scope) in refs {
            // An identifier later rewritten as a binding (an arrow parameter) is not a reference.
            let n = &ast[node];
            let NodeKind::Ident(name) = n.kind else { continue };
            let assigned = n.has(flags::ASSIGNED);
            let (reads, writes) = (!assigned || n.has(flags::READ_ALSO), assigned);
            match self.resolve(scope, name) {
                Some((sym, through_with)) => {
                    let s = &mut self.symbols[sym.index()];
                    s.reads += reads as u32;
                    s.writes += writes as u32;
                    if through_with {
                        s.flags |= symbol_flags::DYNAMIC;
                    }
                    ident_symbols[node.index()] = Some(sym);
                }
                None => unresolved.push(node),
            }
        }
        for node in std::mem::take(&mut self.initial_writes) {
            if let Some(sym) = ident_symbols[node.index()] {
                self.symbols[sym.index()].writes += 1;
            }
        }
        self.mark_exported_declarations(ast, &ident_symbols);
        self.check_export_lists(ast)?;
        Ok(Semantic { scopes: self.scopes, symbols: self.symbols, ident_symbols, unresolved })
    }

    /// Finds the symbol `name` denotes from `scope`, and whether a `with` scope lay between.
    fn resolve(&mut self, scope: ScopeId, name: Atom) -> Option<(SymbolId, bool)> {
        let mut scope_id = Some(scope);
        let mut through_with = false;
        while let Some(id) = scope_id {
            let s = &self.scopes[id.index()];
            if let Some(sym) = find_symbol(s, &self.symbols, name) {
                return Some((sym, through_with));
            }
            if s.kind == ScopeKind::With {
                through_with = true;
            }
            // Every non-arrow function has an implicit `arguments`, created on first use.
            if s.kind == ScopeKind::Function && s.flags & scope_flags::ARROW == 0 && name == known::ARGUMENTS {
                return Some((self.add_symbol(id, name, SymbolKind::Arguments, None), through_with));
            }
            scope_id = s.parent;
        }
        None
    }

    fn mark_exported_declarations(&mut self, ast: &Ast, ident_symbols: &[Option<SymbolId>]) {
        let mut idents = Vec::new();
        for &decl in &self.export_decls {
            match ast[decl].kind {
                NodeKind::VarDecl { decls, .. } => {
                    for &d in ast.list(decls) {
                        if let NodeKind::VarDeclarator { target, .. } = ast[d].kind {
                            binding_idents(ast, target, &mut idents);
                        }
                    }
                }
                NodeKind::Function { name: Some(n), .. } | NodeKind::Class { name: Some(n), .. } | NodeKind::TsEnum { name: n, .. } | NodeKind::TsNamespace { name: n, .. } => idents.push(n),
                _ => {}
            }
        }
        for ident in idents {
            if let Some(sym) = ident_symbols[ident.index()] {
                self.symbols[sym.index()].flags |= symbol_flags::EXPORTED;
            }
        }
    }

    fn check_export_lists(&mut self, ast: &Ast) -> Result<(), SemanticError> {
        for &node in &self.export_lists {
            let NodeKind::ExportNamed(export) = ast[node].kind else { continue };
            for &spec in ast.list(ast.export(export).specifiers) {
                let NodeKind::ExportSpecifier { local, .. } = ast[spec].kind else { continue };
                match self.find(ScopeId(0), local) {
                    Some(sym) => self.symbols[sym.index()].flags |= symbol_flags::EXPORTED,
                    // TypeScript exports types by name, and a type leaves no binding behind.
                    None if self.merge_functions || self.unbound_exports => {}
                    None => {
                        return Err(SemanticError {
                            message: format!("Export '{}' is not defined in module", ast.atom(local)),
                            pos: ast[spec].span.start,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// Every binding identifier a declaration pattern introduces, in source order.
pub fn binding_idents(ast: &Ast, id: NodeId, out: &mut Vec<NodeId>) {
    match ast[id].kind {
        NodeKind::BindingIdent(_) => out.push(id),
        NodeKind::AssignPattern { target, .. } => binding_idents(ast, target, out),
        NodeKind::Rest(arg) => binding_idents(ast, arg, out),
        NodeKind::PatternProp { value, .. } => binding_idents(ast, value, out),
        NodeKind::ArrayPattern(list) | NodeKind::ObjectPattern(list) => {
            for &item in ast.list(list) {
                binding_idents(ast, item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
