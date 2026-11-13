//! Development profiler: a 1 kHz sampling profiler and allocation counter, built with no
//! dependencies (it declares the few libc functions it needs itself). Linux x86-64 only.
//!
//!     RUSTFLAGS="-C force-frame-pointers=yes" CARGO_TARGET_DIR=target/prof \
//!         cargo build --profile profiling --features profiling --bin prof
//!     target/prof/profiling/prof PHASE ITERATIONS FILE...
//!
//! PHASE is `lex`, `parse` (which includes scope analysis), or `retain` (parse and keep every
//! result alive, to measure how much memory a build holds). Set PROF_OUT=samples.txt to write raw stack samples
//! for `tools/prof-report.py`. Only the measured phase is sampled; reading files and
//! building the inputs for later phases is not.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use rathkit_pack::lexer::{Kind, Kw, Lexer, Punct};
use rathkit_pack::parser::{ParseOptions, parse};

// ---- allocation counting ------------------------------------------------------------------

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Counting costs an atomic add per allocation, which would show up in a sampled profile,
/// so sampled runs turn it off. Run once without PROF_OUT for allocation numbers.
static COUNTING: AtomicBool = AtomicBool::new(true);

fn grow(by: usize) {
    let live = LIVE.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(live, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size(), Relaxed);
            grow(layout.size());
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Relaxed) {
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Relaxed) {
            REALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(new_size, Relaxed);
            LIVE.fetch_sub(layout.size(), Relaxed);
            grow(new_size);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// ---- sampling -------------------------------------------------------------------------------

const DEPTH: usize = 20;
const MAX_SAMPLES: usize = 400_000;
const SIGPROF: i32 = 27;
const ITIMER_PROF: i32 = 2;
const SA_SIGINFO: i32 = 4;
const SA_RESTART: i32 = 0x1000_0000;

#[repr(C)]
struct SigAction {
    handler: usize,
    mask: [u64; 16],
    flags: i32,
    restorer: usize,
}

#[repr(C)]
struct TimeVal {
    sec: i64,
    usec: i64,
}

#[repr(C)]
struct ITimerVal {
    interval: TimeVal,
    value: TimeVal,
}

unsafe extern "C" {
    fn sigaction(signum: i32, act: *const SigAction, old: *mut SigAction) -> i32;
    fn setitimer(which: i32, new: *const ITimerVal, old: *mut ITimerVal) -> i32;
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);
static STACK_TOP: AtomicUsize = AtomicUsize::new(0);
static BUFFER: AtomicPtr<usize> = AtomicPtr::new(std::ptr::null_mut());

/// Records the interrupted instruction pointer and walks the frame-pointer chain. It reads
/// raw stack memory, so it only follows frame pointers that lie inside the main thread's
/// stack and strictly increase.
extern "C" fn on_sample(_signal: i32, _info: *mut u8, context: *mut u8) {
    if !ENABLED.load(Relaxed) {
        return;
    }
    let n = COUNT.load(Relaxed);
    let buffer = BUFFER.load(Relaxed);
    if n >= MAX_SAMPLES || buffer.is_null() {
        DROPPED.fetch_add(1, Relaxed);
        return;
    }
    // ucontext_t on x86-64 Linux: gregs start at byte 40; REG_RBP is 10 and REG_RIP is 16.
    let (rip, mut fp) = unsafe { (*(context.add(40 + 16 * 8) as *const usize), *(context.add(40 + 10 * 8) as *const usize)) };
    let here = &rip as *const usize as usize;
    let top = STACK_TOP.load(Relaxed);
    let slot = unsafe { buffer.add(n * DEPTH) };
    let mut depth = 0;
    unsafe {
        *slot = rip;
        depth += 1;
        while depth < DEPTH && fp % 8 == 0 && fp >= here && fp + 16 <= top {
            let next = *(fp as *const usize);
            let ret = *((fp + 8) as *const usize);
            if ret == 0 {
                break;
            }
            *slot.add(depth) = ret;
            depth += 1;
            if next <= fp {
                break;
            }
            fp = next;
        }
        for i in depth..DEPTH {
            *slot.add(i) = 0;
        }
    }
    COUNT.store(n + 1, Relaxed);
}

fn start_sampling() {
    let mut buffer = vec![0usize; MAX_SAMPLES * DEPTH];
    BUFFER.store(buffer.as_mut_ptr(), Relaxed);
    std::mem::forget(buffer);
    let marker = 0usize;
    STACK_TOP.store(&marker as *const usize as usize + 4096, Relaxed);
    let action = SigAction { handler: on_sample as *const () as usize, mask: [0; 16], flags: SA_SIGINFO | SA_RESTART, restorer: 0 };
    let timer = ITimerVal { interval: TimeVal { sec: 0, usec: 1000 }, value: TimeVal { sec: 0, usec: 1000 } };
    unsafe {
        assert_eq!(sigaction(SIGPROF, &action, std::ptr::null_mut()), 0);
        assert_eq!(setitimer(ITIMER_PROF, &timer, std::ptr::null_mut()), 0);
    }
}

fn write_samples(path: &str) {
    use std::fmt::Write;
    let n = COUNT.load(Relaxed);
    let buffer = BUFFER.load(Relaxed);
    let mut out = String::new();
    // File mappings, so the report can tell the program's own code from libc's and find each
    // file's load base (the start of its offset-0 mapping).
    for line in std::fs::read_to_string("/proc/self/maps").unwrap().lines() {
        let mut parts = line.split_whitespace();
        let (range, _perms, offset) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let path = parts.nth(2).unwrap_or("");
        if path.starts_with('/') {
            writeln!(out, "# {range} {offset} {path}").unwrap();
        }
    }
    for i in 0..n {
        for d in 0..DEPTH {
            let pc = unsafe { *buffer.add(i * DEPTH + d) };
            if pc == 0 {
                break;
            }
            write!(out, "{pc:x} ").unwrap();
        }
        out.push('\n');
    }
    std::fs::write(path, out).unwrap();
}

// ---- workload ---------------------------------------------------------------------------------

fn regex_allowed(prev: Option<Kind>) -> bool {
    match prev {
        None => true,
        Some(Kind::Punct(p)) => !matches!(p, Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::PlusPlus | Punct::MinusMinus),
        Some(Kind::Keyword(k)) => !matches!(k, Kw::This | Kw::Super | Kw::Null | Kw::True | Kw::False),
        Some(Kind::TemplateHead | Kind::TemplateMiddle) => true,
        Some(_) => false,
    }
}

fn lex_all(src: &str) -> usize {
    let Ok(mut lexer) = Lexer::new(src) else { return 0 };
    let mut template_braces: Vec<u32> = Vec::new();
    let mut prev: Option<Kind> = None;
    let mut count = 0;
    loop {
        let Ok(mut tok) = lexer.next_token() else { return count };
        match tok.kind {
            Kind::Eof => return count,
            Kind::Punct(Punct::Slash | Punct::SlashEq) if regex_allowed(prev) => match lexer.rescan_regex(tok) {
                Ok(t) => tok = t,
                Err(_) => return count,
            },
            Kind::Punct(Punct::Gt) => tok = lexer.rescan_gt(tok),
            Kind::TemplateHead => template_braces.push(0),
            Kind::Punct(Punct::LBrace) => {
                if let Some(open) = template_braces.last_mut() {
                    *open += 1;
                }
            }
            Kind::Punct(Punct::RBrace) => match template_braces.last_mut() {
                Some(0) => match lexer.rescan_template_continuation(tok) {
                    Ok(t) => {
                        tok = t;
                        if tok.kind == Kind::TemplateTail {
                            template_braces.pop();
                        }
                    }
                    Err(_) => return count,
                },
                Some(open) => *open -= 1,
                None => {}
            },
            _ => {}
        }
        prev = Some(tok.kind);
        count += 1;
    }
}

fn snapshot() -> (usize, usize, usize, usize) {
    (ALLOCS.load(Relaxed), REALLOCS.load(Relaxed), BYTES.load(Relaxed), PEAK.load(Relaxed))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: prof lex|parse|retain|minify ITERATIONS FILE...");
        std::process::exit(2);
    }
    let phase = args[0].as_str();
    let iterations: usize = args[1].parse().expect("iterations");
    let sources: Vec<String> = args[2..].iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();

    // Inputs for later phases are built before sampling starts. Files that do not parse as
    // modules are skipped for every phase so the numbers are comparable.
    let usable: Vec<&String> = sources.iter().filter(|s| parse(s, ParseOptions::new(true)).is_ok()).collect();
    let usable_mb = usable.iter().map(|s| s.len()).sum::<usize>() as f64 / 1e6;
    let work_mb = usable_mb;

    if std::env::var("PROF_OUT").is_ok() {
        COUNTING.store(false, Relaxed);
        start_sampling();
    }
    let before = snapshot();
    let started = Instant::now();
    ENABLED.store(true, Relaxed);
    let mut checksum = 0usize;
    for _ in 0..iterations {
        match phase {
            "lex" => {
                for s in &usable {
                    checksum += lex_all(s);
                }
            }
            "retain" => {
                let kept: Vec<_> = usable.iter().filter_map(|s| parse(s, ParseOptions::new(true)).ok()).collect();
                checksum += kept.len();
                // Everything is still alive here, so the peak printed below is the retained size.
                std::hint::black_box(&kept);
            }
            "parse" => {
                for s in &usable {
                    if let Ok(parsed) = parse(s, ParseOptions::new(true)) {
                        checksum += parsed.ast.node_count();
                    }
                }
            }
            "minify" => {
                let options = rathkit_pack::minify::Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None, legacy: false };
                for s in &usable {
                    checksum += rathkit_pack::minify::minify(s, &options).map_or(0, |m| m.len());
                }
            }
            other => panic!("unknown phase {other}"),
        }
    }
    ENABLED.store(false, Relaxed);
    let secs = started.elapsed().as_secs_f64();
    let after = snapshot();

    let total_mb = work_mb * iterations as f64;
    println!("phase {phase}: {} of {} files usable, {:.1} MB per pass x {iterations}", usable.len(), sources.len(), work_mb);
    println!("  {:.0} ms total, {:.1} MB/s (checksum {checksum})", secs * 1e3, total_mb / secs);
    let allocs = after.0 - before.0;
    let reallocs = after.1 - before.1;
    println!("  allocations: {allocs} ({:.1} per KB of source), reallocations: {reallocs}", allocs as f64 / (total_mb * 1e3));
    println!("  bytes requested: {:.1} MB ({:.2} bytes per source byte), peak live: {:.1} MB", (after.2 - before.2) as f64 / 1e6, (after.2 - before.2) as f64 / (total_mb * 1e6), after.3 as f64 / 1e6);
    if let Ok(path) = std::env::var("PROF_OUT") {
        write_samples(&path);
        println!("  {} samples written to {path} ({} dropped)", COUNT.load(Relaxed), DROPPED.load(Relaxed));
    }
}
