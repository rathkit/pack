//! Declarations and statements of es2015 written as es5: patterns in declarations, assignments, `catch` and `for of`; spread in arrays, calls and
//! `new`; `for of` itself.
use super::*;
use crate::ast::{AssignOp, VarKind};
use crate::target::Feature;

fn text(s: &str) -> Piece {
    Piece::Text(s.to_string())
}

impl Scanner<'_, '_> {
    pub(super) fn lower_es5_stmt(&mut self, node: NodeId) {
        let ast = self.ast;
        match ast[node].kind {
            NodeKind::ExprStmt(e) if matches!(ast[e].kind, NodeKind::Assign { .. }) => {
                self.es5.unused.insert(e);
            }
            NodeKind::VarDeclarator { target, init: Some(init) } if self.lowers(Feature::Destructuring) && matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) => {
                let source = self.local("d");
                let mut items = vec![[vec![Piece::Text(format!("{source} = "))], self.rng(init)].concat()];
                items.extend(self.destructure(target, &source, true));
                self.push(node, super::lower_es5::join(items));
            }
            NodeKind::Assign { op: AssignOp::Assign, target, value } if self.lowers(Feature::Destructuring) && matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) => {
                let source = self.temp();
                let unused = self.es5.unused.contains(&node);
                let mut items = vec![[vec![Piece::Text(format!("{source} = "))], self.rng(value)].concat()];
                items.extend(self.destructure(target, &source, false));
                if !unused {
                    items.push(vec![Piece::Text(source)]);
                }
                let mut pieces = vec![text("(")];
                pieces.extend(super::lower_es5::join(items));
                pieces.push(text(")"));
                self.push(node, pieces);
            }
            NodeKind::Catch { param: Some(param), body } if self.lowers(Feature::Destructuring) && matches!(ast[param].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) => {
                let name = self.local("e");
                let items = self.destructure(param, &name, true);
                self.push(param, vec![Piece::Text(name)]);
                let mut pieces = vec![text(" var ")];
                pieces.extend(super::lower_es5::join(items));
                pieces.push(text(";"));
                let at = ast[body].span.start + 1;
                self.insert(at, pieces);
            }
            NodeKind::Array(list) if self.lowers(Feature::Spread) && ast.list(list).iter().any(|&e| matches!(ast[e].kind, NodeKind::Spread(_))) => {
                let pieces = self.spread_array_pieces(ast.list(list));
                self.push(node, pieces);
            }
            NodeKind::Call { callee, args } if self.lowers(Feature::Spread) && ast.list(args).iter().any(|&e| matches!(ast[e].kind, NodeKind::Spread(_))) && !ast[node].has(crate::ast::flags::OPTIONAL) => {
                self.lower_spread_call(node, callee, args);
            }
            NodeKind::New { callee, args } if self.lowers(Feature::Spread) && ast.list(args).iter().any(|&e| matches!(ast[e].kind, NodeKind::Spread(_))) => {
                let mut pieces = vec![text("new (Function.prototype.bind.apply(")];
                pieces.extend(self.callee_range(callee));
                pieces.push(text(", [void 0].concat("));
                pieces.extend(self.spread_segments(ast.list(args)));
                pieces.push(text(")))()"));
                self.push(node, pieces);
            }
            NodeKind::ForOf { left, right, body } if self.lowers(Feature::ForOf) => self.lower_for_of(node, left, right, body),
            NodeKind::ForOf { left, body, .. } | NodeKind::ForIn { left, body, .. } if self.lowers(Feature::Destructuring) && !ast[node].has(crate::ast::flags::AWAIT) => self.lower_pattern_head(left, body),
            _ => {}
        }
    }

    /// A callee that is applied or constructed: in parentheses unless it is a name or a member access.
    fn callee_range(&self, callee: NodeId) -> Vec<Piece> {
        match self.ast[callee].kind {
            NodeKind::Ident(_) | NodeKind::Member { .. } | NodeKind::This => self.rng(callee),
            _ => self.rng_wrapped(callee),
        }
    }

    /// `[a, b].concat(__arr(c), [d])`: an array literal with spread.
    pub(super) fn spread_array_pieces(&mut self, items: &[NodeId]) -> Vec<Piece> {
        let ast = self.ast;
        let starts_plain = !matches!(ast[items[0]].kind, NodeKind::Spread(_));
        if starts_plain {
            // `[a, b].concat(rest...)`: the leading elements are the array, the rest of the segments are its arguments.
            let first_spread = items.iter().position(|&e| matches!(ast[e].kind, NodeKind::Spread(_))).unwrap();
            let (a, _) = self.span(items[0]);
            let (_, b) = self.span(items[first_spread - 1]);
            let mut pieces = vec![text("["), Piece::Range(a, b), text("]")];
            let rest = self.spread_segments(&items[first_spread..]);
            pieces.push(text(".concat("));
            pieces.extend(rest);
            pieces.push(text(")"));
            pieces
        } else {
            let mut pieces = vec![text("[].concat(")];
            pieces.extend(self.spread_segments(items));
            pieces.push(text(")"));
            pieces
        }
    }

    /// The arguments of a `concat` that put these elements one after the other: `__arr(x)` for a spread, `[a, b]` for the others.
    fn spread_segments(&mut self, items: &[NodeId]) -> Vec<Piece> {
        let ast = self.ast;
        self.bundler.helpers.es5_array = true;
        let mut out: Vec<Piece> = Vec::new();
        let mut i = 0;
        while i < items.len() {
            if i > 0 {
                out.push(text(", "));
            }
            if let NodeKind::Spread(arg) = ast[items[i]].kind {
                out.push(text("__arr("));
                out.extend(self.rng(arg));
                out.push(text(")"));
                i += 1;
            } else {
                let mut j = i;
                while j < items.len() && !matches!(ast[items[j]].kind, NodeKind::Spread(_)) {
                    j += 1;
                }
                let (a, _) = self.span(items[i]);
                let (_, b) = self.span(items[j - 1]);
                out.extend([text("["), Piece::Range(a, b), text("]")]);
                i = j;
            }
        }
        out
    }

    fn lower_spread_call(&mut self, node: NodeId, callee: NodeId, args: List) {
        let ast = self.ast;
        let items = ast.list(args).to_vec();
        if matches!(ast[callee].kind, NodeKind::Super) || matches!(ast[callee].kind, NodeKind::Member { object, .. } if matches!(ast[object].kind, NodeKind::Super)) {
            // A call of the parent, rewritten with the class.
            return;
        }
        let mut pieces: Vec<Piece>;
        let segments = self.spread_segments(&items);
        let argument_list: Vec<Piece> = if matches!(ast[items[0]].kind, NodeKind::Spread(_)) {
            [vec![text("[].concat(")], segments, vec![text(")")]].concat()
        } else {
            let first_spread = items.iter().position(|&e| matches!(ast[e].kind, NodeKind::Spread(_))).unwrap();
            let (a, _) = self.span(items[0]);
            let (_, b) = self.span(items[first_spread - 1]);
            let mut p = vec![text("["), Piece::Range(a, b), text("].concat(")];
            p.extend(self.spread_segments(&items[first_spread..]));
            p.push(text(")"));
            p
        };
        match ast[callee].kind {
            NodeKind::Member { object, prop } if ast[callee].flags & crate::ast::flags::OPTIONAL == 0 && !matches!(ast[object].kind, NodeKind::Super) => {
                let (ps, pe) = self.span(prop);
                let computed = ast[callee].has(crate::ast::flags::COMPUTED);
                let (first, again) = if self.is_plain(object) {
                    (self.rng(object), self.rng(object))
                } else {
                    let t = self.temp();
                    ([vec![Piece::Text(format!("({t} = "))], self.rng(object), vec![text(")")]].concat(), vec![Piece::Text(t)])
                };
                pieces = first;
                if computed {
                    pieces.extend([text("["), Piece::Range(ps, pe), text("]")]);
                } else {
                    pieces.extend([text("."), Piece::Range(ps, pe)]);
                }
                pieces.push(text(".apply("));
                pieces.extend(again);
                pieces.push(text(", "));
            }
            _ => {
                pieces = self.callee_range(callee);
                pieces.push(text(".apply(void 0, "));
            }
        }
        pieces.extend(argument_list);
        pieces.push(text(")"));
        self.push(node, pieces);
    }

    /// A `for of` or `for in` that stays, whose variable is a pattern: it is a name, and the body takes the pattern apart.
    fn lower_pattern_head(&mut self, left: NodeId, body: NodeId) {
        let ast = self.ast;
        let (pattern, declared) = match ast[left].kind {
            NodeKind::VarDecl { decls, .. } => match ast.list(decls).first().map(|&d| ast[d].kind) {
                Some(NodeKind::VarDeclarator { target, .. }) => (target, true),
                _ => return,
            },
            _ => (left, false),
        };
        if !matches!(ast[pattern].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) {
            return;
        }
        let name = self.local("v");
        let mut items = vec![vec![Piece::Text(format!("{name} = "))]];
        let source = self.local("d");
        items[0] = vec![Piece::Text(format!("{source} = {name}"))];
        items.extend(self.destructure(pattern, &source, declared));
        let mut take = vec![text(if declared { " var " } else { " " })];
        take.extend(super::lower_es5::join(items));
        take.push(text(";"));
        self.push(left, vec![Piece::Text(if declared { format!("var {name}") } else { name })]);
        let (ba, bb) = self.span(body);
        let pieces = if matches!(ast[body].kind, NodeKind::Block(_)) {
            self.insert(ba + 1, take);
            return;
        } else {
            let mut p = vec![text("{")];
            p.extend(take);
            p.push(Piece::Range(ba, bb));
            p.push(text(" }"));
            p
        };
        self.push(body, pieces);
    }

    fn lower_for_of(&mut self, node: NodeId, left: NodeId, right: NodeId, body: NodeId) {
        let ast = self.ast;
        if ast[node].has(crate::ast::flags::AWAIT) {
            self.refuse(Feature::ForOf, node, "and a for await loop is written with a generator, which Rathkit Pack cannot rewrite for it yet");
            return;
        }
        self.bundler.helpers.es5_iter = true;
        let (it, step) = (self.local("i"), self.local("r"));
        let value = format!("{step}.value");
        // The statement that takes the value of the turn into the loop's variable.
        let mut take: Vec<Piece> = Vec::new();
        match ast[left].kind {
            NodeKind::VarDecl { kind, decls } => {
                let _ = kind;
                let Some(&declarator) = ast.list(decls).first() else { return };
                let NodeKind::VarDeclarator { target, .. } = ast[declarator].kind else { return };
                if matches!(ast[target].kind, NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_)) && self.lowers(Feature::Destructuring) {
                    let source = self.local("d");
                    let mut items = vec![vec![Piece::Text(format!("{source} = {value}"))]];
                    items.extend(self.destructure(target, &source, true));
                    take.push(text(" var "));
                    take.extend(super::lower_es5::join(items));
                    take.push(text(";"));
                } else {
                    take.push(text(" var "));
                    take.extend(self.rng(target));
                    take.push(Piece::Text(format!(" = {value};")));
                }
            }
            NodeKind::ObjectPattern(_) | NodeKind::ArrayPattern(_) if self.lowers(Feature::Destructuring) => {
                let source = self.temp();
                let mut items = vec![vec![Piece::Text(format!("{source} = {value}"))]];
                items.extend(self.destructure(left, &source, false));
                take.push(text(" "));
                take.extend(super::lower_es5::join(items));
                take.push(text(";"));
            }
            _ => {
                take.push(text(" "));
                take.extend(self.rng(left));
                take.push(Piece::Text(format!(" = {value};")));
            }
        }
        let mut pieces = vec![Piece::Text(format!("for (var {it} = __iter("))];
        pieces.extend(self.rng(right));
        pieces.push(Piece::Text(format!("), {step}; !({step} = {it}.next()).done;) {{")));
        pieces.extend(take);
        let (ba, bb) = self.span(body);
        if let Some(wrapped) = self.loop_wrapper(node, body) {
            // The body is a function called in each turn; it holds the statements of the block too.
            pieces.extend(wrapped);
            pieces.push(text(" }"));
        } else if matches!(ast[body].kind, NodeKind::Block(_)) {
            pieces.push(Piece::Range(ba + 1, bb));
        } else {
            pieces.push(text(" "));
            pieces.push(Piece::Range(ba, bb));
            pieces.push(text(" }"));
        }
        self.push(node, pieces);
    }
}

#[allow(dead_code)]
fn _unused(_: VarKind) {}
