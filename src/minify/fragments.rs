//! Minifying a bundle in pieces, side by side.
//!
//! A bundle is a run of top-level statements (the modules' code), so it can be cut between
//! statements and each piece parsed, analyzed and written by its own thread. What the pieces
//! share is only the top level: a name declared in one piece may be read in another, so those
//! names are chosen once for the whole bundle, between the analysis of the pieces and the
//! naming inside them. Every other name belongs to one piece alone.
//!
//! The result is the same text a single pass over the whole bundle gives, except that
//! statements on either side of a cut are never merged into one declaration. If any piece
//! fails to parse on its own (the bundler's cuts were wrong, or a source file held text that
//! looks like one) the whole bundle is minified in one pass instead.
//!
//! The statements of a big CommonJS module can be pieces too: the module is one statement of the
//! bundle, but its body is a list of statements, and the opening and closing of its function are
//! text kept as written (the layout's `fixed` ranges). The names the pieces of such a body declare
//! belong to the function, not to the bundle, so each body is a group of its own (`bodies`): it
//! reuses the short names of the bundle except those its pieces read. What the kept text and the
//! pieces both name (`names`) is chosen with them. The text written is not what one pass writes
//! (names are chosen by group, not by frequency in one scope), so these are checked by running programs.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// How the text of a bundle is laid out.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// The text before this offset is the opening of a wrapper, written as it is.
    pub head: usize,
    /// The text from this offset on is the closing of the wrapper. Both lie outside every piece.
    pub tail: usize,
    /// Offsets between `head` and `tail` where one top-level statement has ended, in order.
    pub cuts: Vec<usize>,
    /// Names the wrapper declares outside the text: no new name may be one of them.
    pub reserved: Vec<String>,
    /// Text between `head` and `tail` that is not minified, only stripped of white space and comments: the opening and closing of a
    /// function whose body is cut into pieces (a CommonJS module's wrapper). It holds no statement a piece could take and nothing
    /// it declares may be renamed; every name it writes is kept as written wherever a piece declares it. Both ends of each range are in `cuts`.
    pub fixed: Vec<std::ops::Range<usize>>,
    /// Names written in `fixed` text that belong to the pieces: `(where, scope, declares)`. The scope is 0 for the bundle's own top level
    /// or `k + 1` for the body `k`. A name that `declares` is a binding of that scope that no piece declares (the module's own name,
    /// the parameters of its function); the others are read from a piece's scope when a piece declares them. All are renamed with the rest.
    /// In order of position, each inside one of the fixed ranges; the words of fixed text that are not here stay as written.
    pub names: Vec<(std::ops::Range<usize>, usize, bool)>,
    /// Ranges of pieces that are the body of one function: what such a body declares is not visible outside it, nor to another body.
    /// Both ends are in `cuts`, and no range holds another.
    pub bodies: Vec<std::ops::Range<usize>>,
}

/// Bundles smaller than this are not worth cutting.
const MIN_BYTES: usize = 192 * 1024;

/// As `minify`, for a bundle whose layout is known, saying where every token went when asked.
pub fn minify_chunk(source: &str, layout: &Layout, options: &Options, anchors: Option<&mut Vec<Anchor>>) -> Result<String, String> {
    // `RATHKIT_FRAGMENTS`: `off` minifies in one pass; `whole` does too but treats the layout's
    // cuts as places declarations are not merged across, which is what the pieces do (for
    // checking one against the other); `strict` fails where it would fall back to one pass.
    let mode = std::env::var("RATHKIT_FRAGMENTS").unwrap_or_default();
    let ranges = if mode == "off" { Vec::new() } else { pieces(source.len(), layout) };
    if ranges.len() < 2 || !options.any() {
        return run(source, options, anchors, &[]);
    }
    if mode == "whole" {
        let barriers: Vec<u32> = ranges[1..].iter().map(|r| r.start as u32).collect();
        return run(source, options, anchors, &barriers);
    }
    let mut local = anchors.is_some().then(Vec::new);
    match in_pieces(source, layout, &ranges, options, local.as_mut()) {
        Ok(out) => {
            if let (Some(list), Some(found)) = (anchors, local) {
                *list = found;
            }
            Ok(out)
        }
        Err(e) if mode == "strict" => Err(e),
        Err(_) => run(source, options, anchors, &[]),
    }
}

fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The byte ranges of the pieces: the text between `head` and `tail` that is not fixed, cut at the layout's cuts
/// into parts of about the same size.
fn pieces(len: usize, layout: &Layout) -> Vec<std::ops::Range<usize>> {
    let (head, tail) = (layout.head, layout.tail.min(len));
    let threads = workers();
    // The runs of text between the fixed ranges.
    let mut regions: Vec<std::ops::Range<usize>> = Vec::new();
    let mut from = head;
    for f in &layout.fixed {
        if f.start > from {
            regions.push(from..f.start);
        }
        from = f.end;
    }
    if tail > from {
        regions.push(from..tail);
    }
    let size: usize = regions.iter().map(|r| r.len()).sum();
    // `RATHKIT_FRAGMENT_BYTES` sets both the least size worth cutting and the size of a piece
    // (the tests use a small one to cut small programs into many pieces).
    let forced = std::env::var("RATHKIT_FRAGMENT_BYTES").ok().and_then(|v| v.parse::<usize>().ok());
    if forced.is_none() && (size < MIN_BYTES || threads < 2) {
        return Vec::new();
    }
    let target = forced.unwrap_or_else(|| (size / (threads * 4)).clamp(24 * 1024, 2 * 1024 * 1024));
    let mut out = Vec::new();
    for region in regions {
        let first = out.len();
        let mut start = region.start;
        for &cut in &layout.cuts {
            if cut > start && cut < region.end && cut - start >= target {
                out.push(start..cut);
                start = cut;
            }
        }
        // A small remainder joins the piece before it (in the same run of text).
        let joins = out.len() > first && region.end - start < target / 2;
        match out.last_mut() {
            Some(last) if joins => last.end = region.end,
            _ => out.push(start..region.end),
        }
    }
    // One piece that is most of the text (a huge module is a single statement) leaves nothing to
    // gain, and one pass overlaps its own steps better.
    let biggest = out.iter().map(|r| r.len()).max().unwrap_or(0);
    if forced.is_none() && biggest * 5 > size * 3 {
        return Vec::new();
    }
    out
}

/// Runs `f` for `0..n` on several threads, results in order.
fn par_map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..workers().min(n) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let result = f(i);
                    done.lock().expect("results")[i] = Some(result);
                }
            });
        }
    });
    done.into_inner().expect("results").into_iter().map(|r| r.expect("every piece ran")).collect()
}

/// One piece, parsed and analyzed.
struct Unit<'s> {
    source: &'s str,
    /// Where the piece starts in the whole text.
    start: usize,
    /// 0 for the bundle's own top level; `k` for the body of function `k - 1` of the layout's `bodies`.
    group: usize,
    parsed: parser::Parsed,
    facts: Facts,
    prep: Option<Prep>,
    plan: Option<Plan>,
    asi: Vec<bool>,
    /// Identifiers nothing in the piece declares, with how often each stands (an `export { a }`
    /// of a name declared elsewhere counts as one).
    unresolved: Vec<(Atom, u32)>,
}

fn in_pieces(source: &str, layout: &Layout, ranges: &[std::ops::Range<usize>], options: &Options, mut anchors: Option<&mut Vec<Anchor>>) -> Result<String, String> {
    let timing = std::env::var_os("RATHKIT_TIMING").is_some();
    let mut clock = std::time::Instant::now();
    let mut lap = |what: &str| {
        if timing {
            eprintln!("  minify {what}: {:.1} ms", clock.elapsed().as_secs_f64() * 1000.0);
            clock = std::time::Instant::now();
        }
    };
    // The wrapper's own names are not part of any piece, and nothing outside the wrapper is "the top level".
    let options = Options { keep_top_level: false, ..*options };
    let compact = options.whitespace || options.syntax;

    // 1. Parse and analyze every piece.
    let analyzed: Vec<Result<Unit, String>> = par_map(ranges.len(), |i| {
        let range = &ranges[i];
        let text = &source[range.clone()];
        let parsed = parser::parse(text, ParseOptions::new(true).with_recorded_tokens().as_fragment()).map_err(|e| format!("cannot minify: {e}"))?;
        let tokens = &parsed.tokens;
        let mut asi = vec![false; tokens.len() + 1];
        for &i in &parsed.asi {
            asi[i] = true;
        }
        asi[tokens.len()] = parsed.eof_asi;
        let plan = options.syntax.then(|| crate::compress::plan(text, &parsed.ast, &parsed.semantic, tokens, &[]));
        let mut facts = Facts::default();
        facts.collect(&parsed.ast, &parsed.semantic, text);
        let prep = options.identifiers.then(|| facts.prepare(&parsed.ast, &parsed.semantic, &options));
        let mut counts: HashMap<Atom, u32> = HashMap::default();
        for &(node, _) in &facts.unresolved {
            if let NodeKind::Ident(a) = parsed.ast[node].kind {
                *counts.entry(a).or_default() += 1;
            }
        }
        let root = parsed.semantic.root_scope();
        for (_, local, _) in &facts.export_specifiers {
            if parsed.semantic.lookup_local(root, *local).is_none() {
                *counts.entry(*local).or_default() += 1;
            }
        }
        let unresolved = counts.into_iter().collect();
        let group = layout.bodies.iter().position(|b| b.start <= range.start && range.end <= b.end).map_or(0, |k| k + 1);
        Ok(Unit { source: text, start: range.start, group, parsed, facts, prep, plan, asi, unresolved })
    });
    let units: Vec<Unit> = analyzed.into_iter().collect::<Result<_, _>>()?;
    lap("parse and analyze (pieces)");

    // 2. Name the top level of the bundle.
    let shared = if options.identifiers { Some(top_level(&units, layout, source)?) } else { None };
    lap("top level names");

    // What comes right after a piece: another piece, or text that is kept (which needs the statement ended, unless it closes a block).
    let follows: Vec<bool> = (0..ranges.len())
        .map(|i| match ranges.get(i + 1) {
            Some(next) if next.start == ranges[i].end => true,
            Some(next) => !source[ranges[i].end..next.start].trim_start().starts_with('}'),
            None => false,
        })
        .collect();

    // 3. Name everything inside the pieces and write them.
    let written: Vec<(String, Vec<Anchor>)> = par_map(units.len(), |i| {
        let unit = &units[i];
        let tokens = &unit.parsed.tokens;
        let mut renames = Renames::new(tokens.len());
        if let (Some(prep), Some(top)) = (&unit.prep, &shared) {
            unit.facts.plan_renames(&unit.parsed.ast, &unit.parsed.semantic, tokens, unit.source, prep, &top.avoid, Some(&top.names[unit.group]), &mut renames);
        }
        let mut found = anchors.is_some().then(Vec::new);
        let out = emit(unit.source, tokens, &renames, &unit.asi, &unit.facts, &options, unit.plan.as_ref(), found.as_mut(), follows[i]);
        (out, found.unwrap_or_default())
    });
    lap("name and write (pieces)");

    // 4. Put the wrapper and the pieces together.
    let wrapper = |text: &str, tail: bool| -> String {
        if !compact {
            return text.to_string();
        }
        let mut squeezed = squeeze(text);
        if tail && squeezed.ends_with(';') {
            squeezed.pop();
        }
        squeezed
    };
    let mut out = String::with_capacity(source.len() / 2);
    out.push_str(&wrapper(&source[..layout.head], false));
    let mut at = layout.head;
    // The text kept as written, with the names it shares with the pieces written as they were named.
    let spliced = |from: usize, to: usize| -> String {
        let (Some(top), true) = (&shared, options.identifiers) else { return source[from..to].to_string() };
        let mut text = String::with_capacity(to - from);
        let mut at = from;
        let first = layout.names.partition_point(|n| n.0.start < from);
        for (range, scope, _) in layout.names[first..].iter().take_while(|n| n.0.end <= to) {
            let old = &source[range.clone()];
            text.push_str(&source[at..range.start]);
            text.push_str(top.names[*scope].get(old).map_or(old, String::as_str));
            at = range.end;
        }
        text.push_str(&source[at..to]);
        text
    };
    // Whether the first or the last statement of a piece is a `var` declaration: next to kept text that starts or ends with one, the two
    // are one declaration, as they would be if they were one pass.
    let var_at = |unit: &Unit, first: bool| -> bool {
        let ast = &unit.parsed.ast;
        let NodeKind::Program(list) = ast[ast.root].kind else { return false };
        let statements = ast.list(list);
        let statement = if first { statements.first() } else { statements.last() };
        statement.is_some_and(|&n| matches!(ast[n].kind, NodeKind::VarDecl { kind: crate::ast::VarKind::Var, .. }))
    };
    // True after a `var` declaration (a piece's last statement, or the closing of the bundler's own `var x = __cjs(...)`) that was
    // written with its `;`: when a `var` follows it, the two are one declaration and the `;` is a `,`.
    let mut open_var = false;
    let keep_text = |out: &mut String, from: usize, to: usize, open_var: &mut bool| {
        let mut text = wrapper(&spliced(from, to), false);
        if compact {
            if *open_var && text.starts_with("var ") && out.ends_with(';') {
                out.pop();
                out.push(',');
                text.drain(..4);
            }
            if let (Some(&last), Some(&first)) = (out.as_bytes().last(), text.as_bytes().first()) {
                if separator_needed(last, first, out, false) {
                    out.push(' ');
                }
            }
        }
        // Kept text that closes a function call and ends in `;` ends the statement `var x = __cjs(function () { ... });`.
        *open_var = compact && text.starts_with('}') && text.ends_with(");");
        out.push_str(&text);
    };
    // The anchors of a big bundle are tens of megabytes: make room once, so growing the list never holds two copies of it.
    if let Some(list) = anchors.as_deref_mut() {
        list.reserve_exact(written.iter().map(|(_, found)| found.len()).sum());
    }
    for (unit, (text, found)) in units.iter().zip(written) {
        let (mut text, mut found) = (text, found);
        if unit.start > at {
            keep_text(&mut out, at, unit.start, &mut open_var);
        }
        at = unit.start + unit.source.len();
        let mut dropped = 0u32;
        if compact && open_var && text.starts_with("var ") && out.ends_with(';') && var_at(unit, true) {
            out.pop();
            out.push(',');
            text.drain(..4);
            dropped = 4;
        }
        if compact {
            if let (Some(&last), Some(&first)) = (out.as_bytes().last(), text.as_bytes().first()) {
                if separator_needed(last, first, &out, false) {
                    out.push(' ');
                }
            }
        }
        if let Some(list) = anchors.as_deref_mut() {
            let (shift_out, shift_in) = (out.len() as u32, unit.start as u32);
            // The `var ` that went is no token of the output.
            found.retain(|a| a.out >= dropped);
            list.extend(found.into_iter().map(|a| Anchor { out: a.out + shift_out - dropped, inp: a.inp + shift_in, ..a }));
        }
        out.push_str(&text);
        open_var = compact && text.ends_with(';') && var_at(unit, false);
    }
    let end = layout.tail.min(source.len());
    if at < end {
        keep_text(&mut out, at, end, &mut false);
    }
    let tail = wrapper(&source[end..], true);
    if compact {
        if let (Some(&last), Some(&first)) = (out.as_bytes().last(), tail.as_bytes().first()) {
            if separator_needed(last, first, &out, false) {
                out.push(' ');
            }
        }
    }
    out.push_str(&tail);
    // The syntax trees and tokens of every piece are done with.
    drop(shared);
    drop(units);
    // Only a big bundle has enough to give back for it to be worth the time.
    if source.len() >= 2 << 20 {
        release_free_memory();
    }
    Ok(out)
}

/// The pieces were parsed and written on threads of their own, and what those threads freed stays with them: the next stages (a source
/// map, the output) would grow the process by as much again. Gives it back, where the allocator keeps a heap for each thread.
fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: `malloc_trim` takes a number and only returns free memory to the system.
        unsafe {
            malloc_trim(0);
        }
    }
}

/// What the pieces share.
struct TopLevel<'a> {
    /// The new name of every name declared at the top level of some piece, for the pieces of each group: what a piece of the
    /// bundle's own top level (group 0) sees is its group's names, and a piece in a function body sees its body's and then the bundle's.
    names: Vec<HashMap<&'a str, String>>,
    avoid: Avoid<'a>,
}

/// Chooses the names of the bundle's top level, most used first, as one pass over one scope would.
/// The names of a function body (a group of pieces) and of the bundle are chosen together, so that none is given twice.
fn top_level<'a>(units: &'a [Unit], layout: &'a Layout, source: &'a str) -> Result<TopLevel<'a>, String> {
    struct Group<'a> {
        name: &'a str,
        scope: usize,
        uses: u32,
        fixed: bool,
    }
    let scopes = layout.bodies.len() + 1;
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<(usize, &str), usize> = HashMap::default();
    // The group a name means from a piece in `scope`: the body's own, else the bundle's.
    let lookup = |index: &HashMap<(usize, &str), usize>, scope: usize, name: &str| -> Option<usize> { index.get(&(scope, name)).or_else(|| if scope != 0 { index.get(&(0, name)) } else { None }).copied() };
    let mut eval = false;
    for unit in units {
        let (ast, sem) = (&unit.parsed.ast, &unit.parsed.semantic);
        let Some(prep) = &unit.prep else { continue };
        let root = sem.root_scope();
        eval |= sem.scope(root).flags & scope_flags::HAS_EVAL != 0;
        for &sym in sem.scope(root).symbols() {
            let c = prep.canon[sym.index()];
            if c != sym.index() {
                continue;
            }
            let name = ast.atom(sem.symbol(sym).name);
            let g = *index.entry((unit.group, name)).or_insert_with(|| {
                groups.push(Group { name, scope: unit.group, uses: 0, fixed: false });
                groups.len() - 1
            });
            groups[g].uses += prep.uses[c];
            groups[g].fixed |= prep.fixed[c];
        }
    }
    // `undefined` read in one piece is the global unless another declares it; the plan was made
    // without knowing, so leave it to one pass.
    if groups.iter().any(|g| g.name == "undefined") {
        return Err("a piece declares `undefined`".to_string());
    }
    // A name given to an anonymous function by an assignment in a piece other than the declaring one.
    for unit in units {
        if let Some(prep) = &unit.prep {
            for &a in &prep.inferred_outer {
                if let Some(g) = lookup(&index, unit.group, unit.parsed.ast.atom(a)) {
                    groups[g].fixed = true;
                }
            }
        }
    }
    // A direct `eval` anywhere can reach every top-level name.
    if eval {
        for g in &mut groups {
            g.fixed = true;
        }
    }
    let mut kept: HashSet<&str> = layout.reserved.iter().map(String::as_str).collect();
    // Names the kept text shares with the pieces take part like any other: a binding no piece declares is one more name of its scope.
    for (range, scope, declares) in &layout.names {
        let name = &source[range.clone()];
        match index.get(&(*scope, name)).copied() {
            Some(g) => groups[g].uses += 1,
            None if *declares => {
                groups.push(Group { name, scope: *scope, uses: 1, fixed: false });
                index.insert((*scope, name), groups.len() - 1);
            }
            // Not a name of any piece (a keyword, a global): it stays, and no new name is that.
            None => {
                kept.insert(name);
            }
        }
    }
    // The rest of the text that is kept as written names things as they are: whatever a piece declares under such a name stays, and no new name is that.
    for range in &layout.fixed {
        let mut at = range.start;
        let first = layout.names.partition_point(|n| n.0.start < range.start);
        for (named, _, _) in layout.names[first..].iter().take_while(|n| n.0.end <= range.end) {
            for word in words(&source[at..named.start]) {
                kept.insert(word);
            }
            at = named.end;
        }
        for word in words(&source[at..range.end]) {
            kept.insert(word);
        }
    }
    for g in &mut groups {
        if kept.contains(g.name) {
            g.fixed = true;
        }
    }
    let mut globals: HashSet<&str> = HashSet::default();
    for unit in units {
        let ast = &unit.parsed.ast;
        for &(atom, count) in &unit.unresolved {
            let name = ast.atom(atom);
            match lookup(&index, unit.group, name) {
                Some(g) => groups[g].uses += count,
                None => {
                    globals.insert(name);
                }
            }
        }
        if let Some(prep) = &unit.prep {
            kept.extend(prep.kept.iter().map(|&a| ast.atom(a)));
        }
    }
    let mut names: Vec<HashMap<&str, String>> = (0..scopes).map(|_| HashMap::default()).collect();
    let mut taken: HashSet<&str> = HashSet::default();
    let mut todo: Vec<Vec<usize>> = vec![Vec::new(); scopes];
    for (i, g) in groups.iter().enumerate() {
        if g.fixed {
            names[g.scope].insert(g.name, g.name.to_string());
            kept.insert(g.name);
            taken.insert(g.name);
        } else {
            todo[g.scope].push(i);
        }
    }
    // The bundle's own top level first, then each body: a body may reuse a name of the bundle unless one of its pieces reads that one.
    for scope in 0..scopes {
        let mut list = std::mem::take(&mut todo[scope]);
        list.sort_by_key(|&i| std::cmp::Reverse(groups[i].uses));
        let mut avoid: HashSet<String> = HashSet::default();
        if scope > 0 {
            for unit in units.iter().filter(|u| u.group == scope) {
                for &(atom, _) in &unit.unresolved {
                    let name = unit.parsed.ast.atom(atom);
                    if !index.contains_key(&(scope, name)) {
                        if let Some(outer) = names[0].get(name) {
                            avoid.insert(outer.clone());
                        }
                    }
                }
            }
        }
        let mut next = 0usize;
        let mut chosen: HashSet<String> = HashSet::default();
        for i in list {
            let name = loop {
                let candidate = name_at(next);
                next += 1;
                if is_reserved(&candidate) || kept.contains(candidate.as_str()) || globals.contains(candidate.as_str()) || taken.contains(candidate.as_str()) || chosen.contains(&candidate) || avoid.contains(&candidate) {
                    continue;
                }
                break candidate;
            };
            chosen.insert(name.clone());
            names[scope].insert(groups[i].name, name);
        }
    }
    // A piece in a body sees the bundle's names too, except those the body declares itself.
    if scopes > 1 {
        let outer = names[0].clone();
        for body in names.iter_mut().skip(1) {
            for (k, v) in &outer {
                body.entry(k).or_insert_with(|| v.clone());
            }
        }
    }
    Ok(TopLevel { names, avoid: Avoid { kept, globals } })
}

/// Whether a text that is kept as written is safe to take white space and comments out of: no template literal, and no `/` that is not
/// a comment (which could be a regular expression).
pub fn plain_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => return false,
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'/' => return false,
            _ => i += 1,
        }
    }
    true
}

/// Where the words of a text are: names, keywords and numbers' letters outside strings and comments and not after a `.`.
pub fn word_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    let mut after_dot = false;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                after_dot = false;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'.' => {
                after_dot = true;
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            b if is_word(b) => {
                let from = i;
                while i < bytes.len() && is_word(bytes[i]) {
                    i += 1;
                }
                if !after_dot && !bytes[from].is_ascii_digit() {
                    out.push(from..i);
                }
                after_dot = false;
            }
            _ => {
                after_dot = false;
                i += 1;
            }
        }
    }
    out
}

/// The words of a text kept as written: what could be a name, outside strings and comments.
fn words(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b if is_word(b) => {
                let from = i;
                while i < bytes.len() && is_word(bytes[i]) {
                    i += 1;
                }
                if !bytes[from].is_ascii_digit() {
                    out.push(&text[from..i.min(bytes.len())]);
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// The text with the white space that carries no meaning, and the comments, taken out. Strings are left as they are.
fn squeeze(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    let mut i = 0;
    while i < bytes.len() {
        let ch = text[i..].chars().next().expect("a character");
        if ch.is_whitespace() {
            gap = true;
            i += ch.len_utf8();
            continue;
        }
        // A comment is a gap.
        if ch == '/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            gap = true;
            continue;
        }
        if ch == '/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            gap = true;
            continue;
        }
        if gap {
            if let Some(&last) = out.as_bytes().last() {
                let mut first = [0u8; 4];
                ch.encode_utf8(&mut first);
                if separator_needed(last, first[0], &out, false) {
                    out.push(' ');
                }
            }
            gap = false;
        }
        if matches!(ch, '"' | '\'' | '`') {
            let quote = bytes[i];
            let from = i;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            out.push_str(&text[from..i]);
            continue;
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle-like text: each line is a top-level statement that ends there.
    const BUNDLE: &str = "\
var counter = 0;
function bump(by) { counter += by; return counter; }
var items = [];
function add(name, price) { var total = bump(price); items.push({ name, total }); return total; }
var helper = (a, b) => { const sum = a + b; return sum * sum; };
class Box { constructor(v) { this.v = v; } get() { return bump(this.v); } }
console.log(add(\"a\", 2), helper(1, 2), new Box(3).get(), items.length)
const after = typeof window === \"undefined\" ? helper : add;
console.log(after.name)
export { counter as total, bump };
";

    fn cuts(source: &str) -> Vec<usize> {
        source.match_indices('\n').map(|(i, _)| i + 1).collect()
    }

    fn compare(options: Options) {
        let source = BUNDLE;
        let layout = Layout { head: 0, tail: source.len(), cuts: cuts(source), reserved: Vec::new(), ..Layout::default() };
        // Cut after every second statement.
        let ranges: Vec<_> = layout.cuts.iter().step_by(2).copied().scan(0, |start, end| {
            let r = *start..end;
            *start = end;
            Some(r)
        }).collect();
        let mut ranges = ranges;
        let last = ranges.last().map_or(0, |r| r.end);
        if last < source.len() {
            ranges.push(last..source.len());
        }
        assert!(ranges.len() > 2);
        let barriers: Vec<u32> = ranges[1..].iter().map(|r| r.start as u32).collect();
        let whole = run(source, &options, None, &barriers).unwrap();
        let mut found = Vec::new();
        let pieces = in_pieces(source, &layout, &ranges, &options, Some(&mut found)).unwrap();
        // A last `;` is kept between pieces and dropped at the very end of one pass.
        assert_eq!(pieces.trim_end_matches(';'), whole.trim_end_matches(';'));
        assert!(found.windows(2).all(|w| w[0].out <= w[1].out && w[0].inp <= w[1].inp), "anchors in order");
        for a in &found {
            let before = &source[a.inp as usize..(a.inp + a.in_len) as usize];
            let after = &pieces[a.out as usize..(a.out + a.out_len) as usize];
            assert!(before == after || after.contains(before) || (before.chars().all(char::is_alphanumeric) && after.chars().all(char::is_alphanumeric)) || options.syntax, "{before} -> {after}");
        }
    }

    #[test]
    fn pieces_write_what_one_pass_writes() {
        compare(Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None });
        compare(Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: Some(true) });
        compare(Options { whitespace: false, identifiers: true, syntax: false, keep_top_level: false, keep_names: None });
        compare(Options { whitespace: true, identifiers: false, syntax: true, keep_top_level: false, keep_names: None });
    }

    #[test]
    fn a_name_read_in_another_piece_is_renamed_the_same_everywhere() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None };
        let source = "function helper(x) { return x + 1; }\nvar r = helper(2);\nconsole.log(r, helper(3));\n";
        let layout = Layout { head: 0, tail: source.len(), cuts: cuts(source), reserved: Vec::new(), ..Layout::default() };
        let c = cuts(source);
        let ranges = vec![0..c[0], c[0]..c[1], c[1]..source.len()];
        let out = in_pieces(source, &layout, &ranges, &options, None).unwrap();
        assert_eq!(out, "function e(t){return t+1}var t=e(2);console.log(t,e(3))");
    }

    #[test]
    fn a_function_named_by_an_assignment_in_another_piece_keeps_its_name() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: Some(true) };
        let source = "var loose, other;\nloose = function () { return 1; };\nother = () => loose();\nconsole.log(loose.name, other.name);\n";
        let c = cuts(source);
        let layout = Layout { head: 0, tail: source.len(), cuts: c.clone(), reserved: Vec::new(), ..Layout::default() };
        let ranges = vec![0..c[0], c[0]..c[1], c[1]..c[2], c[2]..source.len()];
        let out = in_pieces(source, &layout, &ranges, &options, None).unwrap();
        assert_eq!(out, run(source, &options, None, &[]).unwrap());
        assert!(out.contains("loose=function") && out.contains("other=()=>loose()"), "{out}");
    }

    #[test]
    fn a_top_level_undefined_sends_the_bundle_to_one_pass() {
        let options = Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None };
        let source = "var undefined = 5;\nconsole.log(undefined);\n";
        let c = cuts(source);
        let layout = Layout { head: 0, tail: source.len(), cuts: c.clone(), reserved: Vec::new(), ..Layout::default() };
        assert!(in_pieces(source, &layout, &[0..c[0], c[0]..source.len()], &options, None).is_err());
    }

    #[test]
    fn a_wrapper_is_written_around_the_pieces() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: true, keep_names: None };
        let source = "var Lib = (() => {\n\"use strict\";\nvar one = 1;\nvar two = one + 1;\nreturn { two };\n})();\n";
        let head = source.find("\"use").unwrap();
        let tail = source.find("})();").unwrap();
        let layout = Layout { head, tail, cuts: vec![head + 14, head + 27], reserved: vec!["Lib".to_string()], ..Layout::default() };
        let ranges = vec![head..head + 14, head + 14..head + 27, head + 27..tail];
        let out = in_pieces(source, &layout, &ranges, &options, None).unwrap();
        // The two `var` statements are cut apart and written as one declaration, as one pass would.
        assert_eq!(out, "var Lib=(()=>{\"use strict\";var e=1,t=e+1;return{two:t}})()");
    }

    #[test]
    fn a_piece_that_does_not_parse_alone_fails() {
        let options = Options { whitespace: true, identifiers: true, ..Options::default() };
        let source = "function f() {\n  return 1;\n}\n";
        let layout = Layout { head: 0, tail: source.len(), cuts: vec![15], reserved: Vec::new(), ..Layout::default() };
        assert!(in_pieces(source, &layout, &[0..15, 15..source.len()], &options, None).is_err());
    }

    /// The text of a CommonJS wrapper, laid out as the bundler does: the opening and closing are kept, the statements between are
    /// pieces of one body, and the names the kept text shares are marked.
    fn wrapped(source: &str) -> Layout {
        let open_at = source.find("var mod").unwrap();
        let body_at = source.find("var a").unwrap();
        let close_at = source.find("});").unwrap();
        let end = close_at + "});\n".len();
        let mut cuts = vec![0, open_at];
        let mut at = body_at;
        cuts.push(body_at);
        for line in source[body_at..close_at].split_inclusive('\n') {
            at += line.len();
            cuts.push(at);
        }
        cuts.push(end);
        cuts.extend(source[end..].match_indices('\n').map(|(i, _)| end + i + 1));
        let word = |text: &str, from: usize| from + source[from..].find(text).unwrap();
        let mut names = Vec::new();
        for (text, from, scope, declares) in [("mod", open_at, 0, true), ("__cjs", open_at, 0, false), ("exports", open_at, 1, true), ("module", open_at, 1, true)] {
            let start = word(text, from);
            names.push((start..start + text.len(), scope, declares));
        }
        names.sort_by_key(|n| n.0.start);
        cuts.dedup();
        Layout { head: 0, tail: source.len(), cuts, fixed: vec![open_at..body_at, close_at..end], bodies: vec![body_at..close_at], names, ..Layout::default() }
    }

    const WRAPPED: &str = "var go = 1;\nvar mod = __cjs(function (exports, module) {\nvar a = 1;\nfunction f(x) { return x + a + go; }\nexports.f = f;\n});\nconsole.log(go, mod, exports, f);\n";

    fn in_bundle_pieces(source: &str, layout: &Layout, options: &Options) -> String {
        // Pieces: the statement before the wrapper, each statement of the body, and the statements after it.
        let mut ranges = Vec::new();
        let mut start = layout.head;
        let mut points: Vec<usize> = layout.cuts.iter().copied().filter(|&c| c > 0 && c < source.len()).collect();
        points.push(source.len());
        for &cut in &points {
            if layout.fixed.iter().any(|f| f.start <= start && start < f.end) {
                start = cut.max(start);
                if layout.fixed.iter().any(|f| f.end == cut) {
                    continue;
                }
            }
            if cut > start {
                ranges.push(start..cut);
                start = cut;
            }
        }
        // A range that is inside kept text is no piece.
        ranges.retain(|r| !layout.fixed.iter().any(|f| f.start <= r.start && r.end <= f.end));
        in_pieces(source, layout, &ranges, options, None).unwrap()
    }

    #[test]
    fn a_body_is_written_between_text_that_is_kept() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None };
        let layout = wrapped(WRAPPED);
        let out = in_bundle_pieces(WRAPPED, &layout, &options);
        // The wrapper's own name and parameters are named with the rest (`exports` is `r` inside, where `module` is `i`); the body reuses
        // a name of the bundle it does not read (`mod` is `t` outside and `a` is `t` inside) but not one it reads (`go` is `e`).
        // Outside the function `exports` and `f` mean globals, and stay. The wrapper's `var` joins the declaration before it.
        assert_eq!(out, "var e=1,t=__cjs(function(r,i){var t=1;function n(r){return r+t+e}r.f=n});console.log(e,t,exports,f)");
    }

    #[test]
    fn the_var_chain_runs_through_the_wrapper_and_the_anchors_follow() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None };
        let source = "var go = 1;\nvar mod = __cjs(function (exports, module) {\nvar a = 1;\nfunction f(x) { return x + a + go; }\nexports.f = f;\n});\nvar after = mod;\nvar more = after + go;\nconsole.log(more, after);\n";
        let layout = wrapped(source);
        let mut found = Vec::new();
        let mut ranges = Vec::new();
        let mut start = 0;
        for &cut in layout.cuts.iter().chain(std::iter::once(&source.len())) {
            if cut > start && !layout.fixed.iter().any(|f| f.start <= start && start < f.end) {
                ranges.push(start..cut);
            }
            start = start.max(cut);
        }
        ranges.retain(|r| !layout.fixed.iter().any(|f| f.start <= r.start && r.end <= f.end));
        let out = in_pieces(source, &layout, &ranges, &options, Some(&mut found)).unwrap();
        // One declaration from `go` to `more`, though the wrapper and several cuts are in it.
        assert_eq!(out, "var e=1,r=__cjs(function(r,i){var t=1;function n(r){return r+t+e}r.f=n}),t=r,n=t+e;console.log(n,t)");
        assert_eq!(out.matches("var ").count(), 2, "{out}");
        // Every anchor names text that is there, in order.
        assert!(found.windows(2).all(|w| w[0].out <= w[1].out && w[0].inp <= w[1].inp), "anchors in order");
        for a in &found {
            assert!((a.out + a.out_len) as usize <= out.len() && (a.inp + a.in_len) as usize <= source.len());
        }
        let r = found.iter().find(|a| &source[a.inp as usize..(a.inp + a.in_len) as usize] == "more" && a.out_len > 0).unwrap();
        assert_eq!(&out[r.out as usize..(r.out + r.out_len) as usize], "n", "{out}");
    }

    #[test]
    fn names_in_text_that_is_kept_stay_as_written() {
        // `a` is written in the kept text (a property name there, but nothing tells that from a binding), so what a piece declares as `a` keeps it,
        // while the wrapper's own name and parameters are still named with the rest.
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: false, keep_names: None };
        let source = "var mod = __cjs(function (exports, module) {\nvar a = 1;\nexports.a = a;\n}, { a: 1 });\n";
        let open_at = 0;
        let body_at = source.find("var a").unwrap();
        let close_at = source.find("}, {").unwrap();
        let end = source.len();
        let word = |text: &str| source.find(text).unwrap();
        let mut names = vec![(word("mod")..word("mod") + 3, 0, true), (word("exports")..word("exports") + 7, 1, true), (word("module")..word("module") + 6, 1, true)];
        names.sort_by_key(|n| n.0.start);
        let layout = Layout {
            head: 0,
            tail: end,
            cuts: vec![0, body_at, body_at + "var a = 1;\n".len(), close_at, end],
            fixed: vec![open_at..body_at, close_at..end],
            bodies: vec![body_at..close_at],
            names,
            ..Layout::default()
        };
        let ranges = vec![body_at..body_at + "var a = 1;\n".len(), body_at + "var a = 1;\n".len()..close_at];
        let out = in_pieces(source, &layout, &ranges, &options, None).unwrap();
        assert_eq!(out, "var e=__cjs(function(e,t){var a=1;e.a=a},{a:1});");
    }

    #[test]
    fn white_space_and_comments_go_but_strings_stay() {
        assert_eq!(squeeze("a  =  \"x  y\" ; /* c */ b // end\n+ 'p  q'"), "a=\"x  y\";b+'p  q'");
        assert_eq!(squeeze("var   a\n=\t1"), "var a=1");
        assert_eq!(squeeze("a /* c */ b"), "a b");
        assert_eq!(squeeze("x // c\ny"), "x y");
    }

    #[test]
    fn the_words_of_kept_text_are_the_names_outside_strings_and_comments() {
        let text = "var a = b.c + \"d e\"; /* f */ g(1, h2); // i\nj";
        assert_eq!(words(text), ["var", "a", "b", "c", "g", "h2", "j"]);
        let spans: Vec<&str> = word_spans(text).into_iter().map(|r| &text[r]).collect();
        // Not after a dot: `c` is a property.
        assert_eq!(spans, ["var", "a", "b", "g", "h2", "j"]);
    }

    #[test]
    fn kept_text_must_have_nothing_that_white_space_could_change() {
        assert!(plain_text("var ts = {}; ((module) => {\n\"use strict\"; // note\n/* c */"));
        assert!(!plain_text("x = `a ${b}`"));
        assert!(!plain_text("x = /ab+c/.test(y)"));
        assert!(!plain_text("x = a / b"));
    }
}
