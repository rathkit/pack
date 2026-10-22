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

/// The byte ranges of the pieces: the text between `head` and `tail`, cut at the layout's cuts
/// into parts of about the same size.
fn pieces(len: usize, layout: &Layout) -> Vec<std::ops::Range<usize>> {
    let (head, tail) = (layout.head, layout.tail.min(len));
    let size = tail.saturating_sub(head);
    let threads = workers();
    // `RATHKIT_FRAGMENT_BYTES` sets both the least size worth cutting and the size of a piece
    // (the tests use a small one to cut small programs into many pieces).
    let forced = std::env::var("RATHKIT_FRAGMENT_BYTES").ok().and_then(|v| v.parse::<usize>().ok());
    if forced.is_none() && (size < MIN_BYTES || threads < 2) {
        return Vec::new();
    }
    let target = forced.unwrap_or_else(|| (size / (threads * 4)).clamp(24 * 1024, 2 * 1024 * 1024));
    let mut out = Vec::new();
    let mut start = head;
    for &cut in &layout.cuts {
        if cut > start && cut < tail && cut - start >= target {
            out.push(start..cut);
            start = cut;
        }
    }
    // A small remainder joins the piece before it.
    match out.last_mut() {
        Some(last) if tail - start < target / 2 => last.end = tail,
        _ => out.push(start..tail),
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
        Ok(Unit { source: text, start: range.start, parsed, facts, prep, plan, asi, unresolved })
    });
    let units: Vec<Unit> = analyzed.into_iter().collect::<Result<_, _>>()?;
    lap("parse and analyze (pieces)");

    // 2. Name the top level of the bundle.
    let shared = if options.identifiers { Some(top_level(&units, layout)?) } else { None };
    lap("top level names");

    // 3. Name everything inside the pieces and write them.
    let written: Vec<(String, Vec<Anchor>)> = par_map(units.len(), |i| {
        let unit = &units[i];
        let tokens = &unit.parsed.tokens;
        let mut renames = Renames::new(tokens.len());
        if let (Some(prep), Some(top)) = (&unit.prep, &shared) {
            unit.facts.plan_renames(&unit.parsed.ast, &unit.parsed.semantic, tokens, unit.source, prep, &top.avoid, Some(&top.names), &mut renames);
        }
        let mut found = anchors.is_some().then(Vec::new);
        let out = emit(unit.source, tokens, &renames, &unit.asi, &unit.facts, &options, unit.plan.as_ref(), found.as_mut(), i + 1 < units.len());
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
    for (unit, (text, found)) in units.iter().zip(written) {
        if compact {
            if let (Some(&last), Some(&first)) = (out.as_bytes().last(), text.as_bytes().first()) {
                if separator_needed(last, first, &out, false) {
                    out.push(' ');
                }
            }
        }
        if let Some(list) = anchors.as_deref_mut() {
            let (shift_out, shift_in) = (out.len() as u32, unit.start as u32);
            list.extend(found.into_iter().map(|a| Anchor { out: a.out + shift_out, inp: a.inp + shift_in, ..a }));
        }
        out.push_str(&text);
    }
    let tail = wrapper(&source[layout.tail.min(source.len())..], true);
    if compact {
        if let (Some(&last), Some(&first)) = (out.as_bytes().last(), tail.as_bytes().first()) {
            if separator_needed(last, first, &out, false) {
                out.push(' ');
            }
        }
    }
    out.push_str(&tail);
    Ok(out)
}

/// What the pieces share.
struct TopLevel<'a> {
    /// The new name of every name declared at the top level of some piece.
    names: HashMap<&'a str, String>,
    avoid: Avoid<'a>,
}

/// Chooses the names of the bundle's top level, most used first, as one pass over one scope would.
fn top_level<'a>(units: &'a [Unit], layout: &'a Layout) -> Result<TopLevel<'a>, String> {
    struct Group<'a> {
        name: &'a str,
        uses: u32,
        fixed: bool,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::default();
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
            let g = *index.entry(name).or_insert_with(|| {
                groups.push(Group { name, uses: 0, fixed: false });
                groups.len() - 1
            });
            groups[g].uses += prep.uses[c];
            groups[g].fixed |= prep.fixed[c];
        }
    }
    // `undefined` read in one piece is the global unless another declares it; the plan was made
    // without knowing, so leave it to one pass.
    if index.contains_key("undefined") {
        return Err("a piece declares `undefined`".to_string());
    }
    // A name given to an anonymous function by an assignment in a piece other than the declaring one.
    for unit in units {
        if let Some(prep) = &unit.prep {
            for &a in &prep.inferred_outer {
                if let Some(&g) = index.get(unit.parsed.ast.atom(a)) {
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
    let mut globals: HashSet<&str> = HashSet::default();
    for unit in units {
        let ast = &unit.parsed.ast;
        for &(atom, count) in &unit.unresolved {
            let name = ast.atom(atom);
            match index.get(name) {
                Some(&g) => groups[g].uses += count,
                None => {
                    globals.insert(name);
                }
            }
        }
        if let Some(prep) = &unit.prep {
            kept.extend(prep.kept.iter().map(|&a| ast.atom(a)));
        }
    }
    let mut names: HashMap<&str, String> = HashMap::default();
    let mut taken: HashSet<&str> = HashSet::default();
    let mut todo: Vec<usize> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        if g.fixed {
            names.insert(g.name, g.name.to_string());
            kept.insert(g.name);
            taken.insert(g.name);
        } else {
            todo.push(i);
        }
    }
    todo.sort_by_key(|&i| std::cmp::Reverse(groups[i].uses));
    let mut next = 0usize;
    let mut chosen: HashSet<String> = HashSet::default();
    for i in todo {
        let name = loop {
            let candidate = name_at(next);
            next += 1;
            if is_reserved(&candidate) || kept.contains(candidate.as_str()) || globals.contains(candidate.as_str()) || taken.contains(candidate.as_str()) || chosen.contains(&candidate) {
                continue;
            }
            break candidate;
        };
        chosen.insert(name.clone());
        names.insert(groups[i].name, name);
    }
    Ok(TopLevel { names, avoid: Avoid { kept, globals } })
}

/// The text with the white space that carries no meaning taken out.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
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
        out.push(ch);
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
        let layout = Layout { head: 0, tail: source.len(), cuts: cuts(source), reserved: Vec::new() };
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
        let layout = Layout { head: 0, tail: source.len(), cuts: cuts(source), reserved: Vec::new() };
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
        let layout = Layout { head: 0, tail: source.len(), cuts: c.clone(), reserved: Vec::new() };
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
        let layout = Layout { head: 0, tail: source.len(), cuts: c.clone(), reserved: Vec::new() };
        assert!(in_pieces(source, &layout, &[0..c[0], c[0]..source.len()], &options, None).is_err());
    }

    #[test]
    fn a_wrapper_is_written_around_the_pieces() {
        let options = Options { whitespace: true, identifiers: true, syntax: false, keep_top_level: true, keep_names: None };
        let source = "var Lib = (() => {\n\"use strict\";\nvar one = 1;\nvar two = one + 1;\nreturn { two };\n})();\n";
        let head = source.find("\"use").unwrap();
        let tail = source.find("})();").unwrap();
        let layout = Layout { head, tail, cuts: vec![head + 14, head + 27], reserved: vec!["Lib".to_string()] };
        let ranges = vec![head..head + 14, head + 14..head + 27, head + 27..tail];
        let out = in_pieces(source, &layout, &ranges, &options, None).unwrap();
        assert_eq!(out, "var Lib=(()=>{\"use strict\";var e=1;var t=e+1;return{two:t}})()");
    }

    #[test]
    fn a_piece_that_does_not_parse_alone_fails() {
        let options = Options { whitespace: true, identifiers: true, ..Options::default() };
        let source = "function f() {\n  return 1;\n}\n";
        let layout = Layout { head: 0, tail: source.len(), cuts: vec![15], reserved: Vec::new() };
        assert!(in_pieces(source, &layout, &[0..15, 15..source.len()], &options, None).is_err());
    }
}
