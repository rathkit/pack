use std::process::ExitCode;
use std::time::Instant;

use rathkit_pack::lexer::{Kind, Kw, Lexer, Punct};
use rathkit_pack::parser::{ParseOptions, parse};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((cmd, _)) if cmd == "--version" || cmd == "-V" => {
            println!("rkp {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some((cmd, files)) if cmd == "--lex-check" && !files.is_empty() => lex_check(files),
        Some((cmd, files)) if cmd == "--parse-check" && !files.is_empty() => parse_check(files),
        Some((cmd, files)) if cmd == "--minify-verify" && !files.is_empty() => minify_verify(files),
        // Development aid: prints each file as the minifier writes it.
        Some((cmd, files)) if cmd == "--minify-print" && !files.is_empty() => {
            let options = rathkit_pack::minify::Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None };
            for file in files {
                match std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|src| rathkit_pack::minify::minify(&src, &options)) {
                    Ok(out) => println!("{out}"),
                    Err(e) => eprintln!("{file}: {e}"),
                }
            }
            ExitCode::SUCCESS
        }
        // ESTree for `this.parse` in plugins: `--estree [--script] FILE`, `-` reads standard input.
        Some((cmd, rest)) if cmd == "--estree" && !rest.is_empty() => estree_command(rest),
        // A long-lived ESTree/free-variable service for the plugin host: requests on standard input.
        Some((cmd, _)) if cmd == "--estree-server" => estree_server(),
        // Free variables of a file as a JSON array: `--globals FILE|-`.
        Some((cmd, rest)) if cmd == "--globals" && !rest.is_empty() => globals_command(&rest[0]),
        Some((cmd, files)) if cmd == "--parse-verdicts" && !files.is_empty() => parse_verdicts(files, false),
        Some((cmd, files)) if cmd == "--script-verdicts" && !files.is_empty() => parse_verdicts(files, true),
        Some((cmd, files)) if cmd == "--scopes" && files.len() == 1 => dump_scopes(&files[0]),
        Some((cmd, files)) if cmd == "--bench" && !files.is_empty() => bench(files),
        Some((cmd, rest)) if cmd == "bundle" && !rest.is_empty() => bundle_command(rest, false),
        // The development server: the build in memory, reloads on change.
        Some((cmd, rest)) if cmd == "serve" => bundle_command(rest, true),
        Some((cmd, files)) if cmd == "--graph" && !files.is_empty() => graph(files),
        Some((cmd, files)) if cmd == "--resolve-dump" && !files.is_empty() => resolve_dump(files),
        Some((cmd, files)) if cmd == "--ts-verdicts" && !files.is_empty() => ts_verdicts(files),
        Some((cmd, files)) if cmd == "--link-dump" && !files.is_empty() => link_dump(files),
        _ => {
            eprintln!("usage: rkp bundle <entry>... | serve <entry>... | --version\n       rkp --lex-check|--parse-check|--parse-verdicts|--script-verdicts|--bench <file.js>... (development aids)\n       rkp --scopes <file.js>\n       rkp bundle <entry.js>... [--outfile FILE | --outdir DIR --splitting] [--format esm|cjs|iife] [--no-treeshake] [--define KEY=VALUE]... [--loader .ext=NAME]... [--jsx automatic|classic] [--external NAME]... [--condition NAME]...\n       rkp --graph <entry.js>... [--quiet] [--threads N] [--external NAME]... [--condition NAME]...");
            ExitCode::from(2)
        }
    }
}

/// Development aid: minifies each file as a module (those that parse as one) and checks the
/// result is the same program under other names. Files that need script mode are skipped.
fn minify_verify(files: &[String]) -> ExitCode {
    let options = rathkit_pack::minify::Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None };
    let (mut checked, mut skipped, mut failures, mut before, mut after) = (0, 0, 0, 0usize, 0usize);
    for file in files {
        let Ok(src) = std::fs::read_to_string(file) else { continue };
        if parse(&src, ParseOptions::new(true)).is_err() {
            skipped += 1;
            continue;
        }
        match rathkit_pack::minify::verify(&src, &options) {
            Ok(()) => {
                checked += 1;
                before += src.len();
                after += rathkit_pack::minify::minify(&src, &options).map_or(0, |m| m.len());
            }
            Err(problem) => {
                failures += 1;
                if failures <= 10 {
                    println!("FAIL {file}: {problem}");
                }
            }
        }
    }
    println!("{checked} files verified, {failures} failures, {skipped} skipped (not modules); {before} bytes became {after}");
    if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Development aid: lexes files the way a parser would drive the lexer, without building a
/// tree. Regex-versus-division is guessed from the previous token, which is wrong in a few
/// rare spots (`) /re/` after an `if (...)`), so a reported error here may be this guess
/// and not the lexer.
fn lex_check(files: &[String]) -> ExitCode {
    let (mut bytes, mut tokens, mut failures) = (0usize, 0usize, 0usize);
    let started = Instant::now();
    for file in files {
        let src = match std::fs::read_to_string(file) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("{file}: {e}");
                failures += 1;
                continue;
            }
        };
        bytes += src.len();
        match lex_all(&src) {
            Ok(n) => tokens += n,
            Err(e) => {
                let line = src[..(e.pos as usize).min(src.len())].matches('\n').count() + 1;
                eprintln!("{file}:{line}: {e}");
                failures += 1;
            }
        }
    }
    let secs = started.elapsed().as_secs_f64();
    println!(
        "{} files, {} MB, {} tokens, {} failed, {:.1} ms ({:.0} MB/s)",
        files.len(),
        bytes / 1_000_000,
        tokens,
        failures,
        secs * 1000.0,
        bytes as f64 / 1e6 / secs.max(1e-9)
    );
    if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn lex_all(src: &str) -> Result<usize, rathkit_pack::lexer::LexError> {
    let mut lexer = Lexer::new(src)?;
    // Open `{` count for each template substitution we are inside.
    let mut template_braces: Vec<u32> = Vec::new();
    let mut prev: Option<Kind> = None;
    let mut count = 0;
    loop {
        let mut tok = lexer.next_token()?;
        match tok.kind {
            Kind::Eof => return Ok(count),
            Kind::Punct(Punct::Slash | Punct::SlashEq) if regex_allowed(prev) => tok = lexer.rescan_regex(tok)?,
            Kind::Punct(Punct::Gt) => tok = lexer.rescan_gt(tok),
            Kind::TemplateHead => template_braces.push(0),
            Kind::Punct(Punct::LBrace) => {
                if let Some(open) = template_braces.last_mut() {
                    *open += 1;
                }
            }
            Kind::Punct(Punct::RBrace) => match template_braces.last_mut() {
                Some(0) => {
                    tok = lexer.rescan_template_continuation(tok)?;
                    if tok.kind == Kind::TemplateTail {
                        template_braces.pop();
                    }
                }
                Some(open) => *open -= 1,
                None => {}
            },
            _ => {}
        }
        prev = Some(tok.kind);
        count += 1;
    }
}

fn regex_allowed(prev: Option<Kind>) -> bool {
    match prev {
        None => true,
        Some(Kind::Punct(p)) => !matches!(p, Punct::RParen | Punct::RBracket | Punct::RBrace | Punct::PlusPlus | Punct::MinusMinus),
        Some(Kind::Keyword(k)) => !matches!(k, Kw::This | Kw::Super | Kw::Null | Kw::True | Kw::False),
        Some(Kind::TemplateHead | Kind::TemplateMiddle) => true,
        Some(_) => false,
    }
}

/// Development aid: parses each file as a module, falling back to a script (CommonJS files
/// are not valid modules), and reports files that fail both ways.
fn parse_check(files: &[String]) -> ExitCode {
    let (mut bytes, mut nodes, mut modules, mut scripts, mut failures) = (0usize, 0usize, 0, 0, 0);
    let started = Instant::now();
    for file in files {
        let src = match std::fs::read_to_string(file) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("{file}: {e}");
                failures += 1;
                continue;
            }
        };
        bytes += src.len();
        match parse(&src, ParseOptions::new(true)) {
            Ok(parsed) => {
                if let Err(problem) = parsed.ast.check_invariants() {
                    eprintln!("{file}: broken tree: {problem}");
                    failures += 1;
                }
                nodes += parsed.ast.node_count();
                modules += 1;
            }
            Err(module_error) => match parse(&src, ParseOptions::new(false)) {
                Ok(parsed) => {
                    if let Err(problem) = parsed.ast.check_invariants() {
                        eprintln!("{file}: broken tree: {problem}");
                        failures += 1;
                    }
                    nodes += parsed.ast.node_count();
                    scripts += 1;
                }
                Err(script_error) => {
                    // Report the script error: it is the more likely to be a real bug, since
                    // a module-only construct would have parsed in the module attempt.
                    let error = if src.contains("import ") || src.contains("export ") { module_error } else { script_error };
                    let line = src[..(error.pos as usize).min(src.len())].matches('\n').count() + 1;
                    eprintln!("{file}:{line}: {error}");
                    failures += 1;
                }
            },
        }
    }
    let secs = started.elapsed().as_secs_f64();
    println!(
        "{} files ({} module, {} script), {} MB, {} nodes, {} failed, {:.1} ms ({:.0} MB/s)",
        files.len(),
        modules,
        scripts,
        bytes / 1_000_000,
        nodes,
        failures,
        secs * 1000.0,
        bytes as f64 / 1e6 / secs.max(1e-9)
    );
    if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Development aid: prints `path<TAB>module|script|fail` per file, for comparing against
/// another engine (see tools/diff-v8.mjs). With `script_only`, modules are not tried, which
/// exercises sloppy-mode behavior that a valid module would otherwise hide.
fn parse_verdicts(files: &[String], script_only: bool) -> ExitCode {
    for file in files {
        let Ok(src) = std::fs::read_to_string(file) else { continue };
        let verdict = if !script_only && accepts(&src, true) {
            "module"
        } else if accepts(&src, false) {
            "script"
        } else {
            "fail"
        };
        println!("{file}\t{verdict}");
    }
    ExitCode::SUCCESS
}

/// Whether a bundler would accept `src`: it parses, and its scopes are consistent.
fn accepts(src: &str, module: bool) -> bool {
    parse(src, ParseOptions::new(module)).is_ok()
}

/// Development aid: prints the scope tree with each scope's symbols.
fn dump_scopes(file: &str) -> ExitCode {
    let src = match std::fs::read_to_string(file) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let parsed = match parse(&src, ParseOptions::new(true)) {
        Ok(parsed) => parsed,
        Err(_) => match parse(&src, ParseOptions::new(false)) {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!("{file}: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let (ast, sem) = (&parsed.ast, &parsed.semantic);
    for (id, scope) in sem.scopes() {
        let mut depth = 0;
        let mut up = scope.parent;
        while let Some(p) = up {
            depth += 1;
            up = sem.scope(p).parent;
        }
        let eval = if scope.flags & rathkit_pack::semantic::scope_flags::HAS_EVAL != 0 { " [eval]" } else { "" };
        println!("{}{:?} #{}{}", "  ".repeat(depth), scope.kind, id.index(), eval);
        for &sym in scope.symbols() {
            let s = sem.symbol(sym);
            let mut notes = String::new();
            if s.flags & rathkit_pack::semantic::symbol_flags::EXPORTED != 0 {
                notes.push_str(" exported");
            }
            if s.flags & rathkit_pack::semantic::symbol_flags::DYNAMIC != 0 {
                notes.push_str(" dynamic");
            }
            println!("{}  {} {:?} reads={} writes={}{}", "  ".repeat(depth), ast.atom(s.name), s.kind, s.reads, s.writes, notes);
        }
    }
    let globals: Vec<String> = sem
        .unresolved()
        .iter()
        .filter_map(|&n| match ast[n].kind {
            rathkit_pack::ast::NodeKind::Ident(a) => Some(ast.atom(a).to_string()),
            _ => None,
        })
        .collect();
    let mut unique = globals.clone();
    unique.sort();
    unique.dedup();
    println!("globals: {}", unique.join(", "));
    ExitCode::SUCCESS
}

/// Development aid: times lexing and parsing (which includes scope analysis) over many files,
/// read once up front.
fn bench(files: &[String]) -> ExitCode {
    let sources: Vec<String> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    let bytes: usize = sources.iter().map(|s| s.len()).sum();
    let mb = bytes as f64 / 1e6;
    let (mut lex_s, mut parse_s) = (0.0, 0.0);
    let (mut parsed, mut nodes, mut scopes, mut symbols) = (0, 0usize, 0usize, 0usize);
    let mut per_file: Vec<(f64, usize, usize)> = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        let t = Instant::now();
        let _ = lex_all(src);
        lex_s += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let Ok(result) = parse(src, ParseOptions::new(true)) else { continue };
        let took = t.elapsed().as_secs_f64();
        parse_s += took;
        if src.len() > 200_000 {
            per_file.push((took * 1e3 / (src.len() as f64 / 1e6), src.len(), i));
        }
        parsed += 1;
        nodes += result.ast.node_count();
        scopes += result.semantic.scopes().count();
        symbols += result.semantic.symbols().count();
    }
    println!("{} files, {:.1} MB; parsed {} as modules; {} nodes, {} scopes, {} symbols", sources.len(), mb, parsed, nodes, scopes, symbols);
    println!("  lex-only (heuristic regex):   {:6.1} ms  {:5.0} MB/s", lex_s * 1e3, mb / lex_s);
    println!("  parse + scope analysis:       {:6.1} ms  {:5.0} MB/s", parse_s * 1e3, mb / parse_s);
    per_file.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("  slowest large files to parse (ms per MB):");
    for (ms_per_mb, len, i) in per_file.iter().take(4) {
        println!("    {:7.1} ms/MB  {:6.1} MB  {}", ms_per_mb, *len as f64 / 1e6, files.get(*i).map_or("", |f| f.as_str()).rsplit('/').next().unwrap_or(""));
    }
    if let Some(median) = per_file.get(per_file.len() / 2) {
        println!("  median over {} large files: {:.1} ms/MB", per_file.len(), median.0);
    }
    ExitCode::SUCCESS
}

/// Development aid: builds the module graph from the given entries and prints it.
fn graph(args: &[String]) -> ExitCode {
    use rathkit_pack::graph::{Edge, GraphOptions, Severity, build};
    let mut entries = Vec::new();
    let mut options = GraphOptions::default();
    let mut quiet = false;
    let mut shake_report = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--quiet" => quiet = true,
            "--shake" => shake_report = true,
            "--threads" => options.threads = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            "--external" => options.external.extend(it.next().cloned()),
            "--condition" => options.resolve.conditions.extend(it.next().cloned()),
            _ => entries.push(arg.clone()),
        }
    }
    let cwd = std::env::current_dir().expect("working directory");
    let started = Instant::now();
    let graph = build(&cwd, &entries, &options);
    let elapsed = started.elapsed();
    let show = |path: &std::path::Path| path.strip_prefix(&cwd).unwrap_or(path).display().to_string();
    if !quiet {
        for module in &graph.modules {
            println!("{:>5} {:?} {}", module.id.index(), module.kind, show(&module.path));
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                let target = match edge {
                    Edge::Module(id) => format!("-> {}", show(&graph.module(*id).path)),
                    Edge::External(name) => format!("-> external {name}"),
                    Edge::Unresolved => "-> UNRESOLVED".to_string(),
                };
                println!("        {:?} {} {target}", request.kind, request.specifier);
            }
        }
    }
    let (mut errors, mut warnings) = (0, 0);
    for d in &graph.diagnostics {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
        }
        let label = if d.severity == Severity::Error { "error" } else { "warning" };
        let place = match (&d.file, d.pos) {
            (Some(file), Some(pos)) => {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                let before = &text.as_bytes()[..(pos as usize).min(text.len())];
                let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
                let col = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
                format!("{}:{line}:{col}: ", show(file))
            }
            (Some(file), None) => format!("{}: ", show(file)),
            _ => String::new(),
        };
        eprintln!("{place}{label}: {}", d.message);
    }
    if !quiet || errors == 0 {
        let linked = rathkit_pack::link::link(&graph);
        let started = Instant::now();
        let _ = started;
        for d in &linked.diagnostics {
            let file = d.file.as_deref().map(&show).unwrap_or_default();
            eprintln!("{file}: error: {}", d.message);
            errors += 1;
        }
        let bound: usize = linked.imports.iter().map(|m| m.len()).sum();
        println!("linked {bound} import bindings, {} link errors", linked.diagnostics.len());
    }
    if shake_report {
        let linked = rathkit_pack::link::link(&graph);
        let started = Instant::now();
        let shaken = rathkit_pack::shake::shake(&graph, &linked);
        let took = started.elapsed();
        let (live, total) = shaken.part_counts();
        let kept_bytes: usize = graph
            .modules
            .iter()
            .zip(&shaken.modules)
            .map(|(module, shake)| match (&module.parsed, module.kind) {
                (Some(parsed), rathkit_pack::graph::ModuleKind::Esm) => {
                    shake.parts.iter().zip(&shake.live).filter(|(_, l)| **l).map(|(p, _)| (parsed.ast[p.stmt].span.end - parsed.ast[p.stmt].span.start) as usize).sum::<usize>()
                }
                _ if shake.included => module.source.len(),
                _ => 0,
            })
            .sum();
        let total_bytes: usize = graph.modules.iter().map(|m| m.source.len()).sum();
        println!(
            "shaken: {}/{} modules, {live}/{total} statements, {:.1} of {:.1} MB kept ({:.0}%), {:.0} ms",
            shaken.included_modules(),
            graph.modules.len(),
            kept_bytes as f64 / 1e6,
            total_bytes as f64 / 1e6,
            100.0 * kept_bytes as f64 / total_bytes.max(1) as f64,
            took.as_secs_f64() * 1e3
        );
    }
    let bytes: usize = graph.modules.iter().map(|m| m.source.len()).sum();
    println!(
        "{} modules, {:.1} MB, {errors} errors, {warnings} warnings, {:.0} ms ({:.0} MB/s)",
        graph.modules.len(),
        bytes as f64 / 1e6,
        elapsed.as_secs_f64() * 1e3,
        bytes as f64 / 1e6 / elapsed.as_secs_f64()
    );
    if errors > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Development aid: prints `file<TAB>specifier<TAB>resolved` for every request in the given
/// files, resolved the way Node's `require` does, for comparing against Node itself.
fn resolve_dump(files: &[String]) -> ExitCode {
    use rathkit_pack::module::ModuleRecord;
    use rathkit_pack::resolve::{ResolveKind, ResolveOptions, Resolver};
    let options = ResolveOptions {
        extensions: [".js", ".json", ".node"].map(String::from).to_vec(),
        main_fields: vec!["main".to_string()],
        conditions: ["node", "module-sync"].map(String::from).to_vec(),
        ..ResolveOptions::default()
    };
    let resolver = Resolver::new(options);
    let kind = if std::env::var_os("RESOLVE_KIND").is_some_and(|k| k == "import") { ResolveKind::Import } else { ResolveKind::Require };
    for file in files {
        let Ok(src) = std::fs::read_to_string(file) else { continue };
        let parsed = parse(&src, ParseOptions::new(false)).or_else(|_| parse(&src, ParseOptions::new(true)));
        let Ok(parsed) = parsed else { continue };
        let record = ModuleRecord::build(&parsed);
        let dir = std::path::Path::new(file).parent().unwrap();
        for request in &record.requests {
            if rathkit_pack::graph::is_node_builtin(&request.specifier) {
                continue;
            }
            let result = match resolver.resolve(dir, &request.specifier, kind) {
                Ok(found) => found.path.display().to_string(),
                Err(e) => format!("ERROR {e}"),
            };
            println!("{file}\t{}\t{result}", request.specifier);
        }
    }
    ExitCode::SUCCESS
}

/// Development aid: for each entry whose whole import closure is plain ES modules, prints the
/// closure's edges and the entry's namespace names, for comparing against Node's own linker
/// (`tools/diff-link.mjs`). Entries that reach CommonJS, JSON, externals or errors are skipped.
fn link_dump(entries: &[String]) -> ExitCode {
    use rathkit_pack::graph::{Edge, GraphOptions, ModuleKind, build};
    use rathkit_pack::link::{export_names, link};
    let cwd = std::env::current_dir().expect("working directory");
    let options = GraphOptions { threads: 1, node_builtins: true, ..GraphOptions::default() };
    for entry in entries {
        let graph = build(&cwd, std::slice::from_ref(entry), &options);
        let pure = !graph.has_errors()
            && graph.diagnostics.is_empty()
            && graph.modules.iter().all(|m| m.kind == ModuleKind::Esm && m.edges.iter().all(|e| matches!(e, Edge::Module(_))));
        if !pure || graph.entries.is_empty() {
            continue;
        }
        let linked = link(&graph);
        let root = graph.entries[0];
        println!("ENTRY\t{}", graph.module(root).path.display());
        for module in &graph.modules {
            for (request, edge) in module.record.requests.iter().zip(&module.edges) {
                if let Edge::Module(id) = edge {
                    println!("EDGE\t{}\t{}\t{}", module.path.display(), request.specifier, graph.module(*id).path.display());
                }
            }
        }
        println!("NAMES\t{}", export_names(&graph, root).join("\u{1f}"));
        println!("ERRORS\t{}", linked.diagnostics.len());
    }
    ExitCode::SUCCESS
}

/// `rkp bundle ENTRY... [--outfile FILE]`: builds one ES module file from the entries.
fn bundle_command(args: &[String], serve: bool) -> ExitCode {
    loop {
        match bundle_once(args, serve) {
            Done::Exit(code) => return code,
            Done::Restart => continue,
        }
    }
}

fn bundle_once(args: &[String], serve: bool) -> Done {
    use rathkit_pack::graph::GraphOptions;
    let mut entries = Vec::new();
    let mut options = GraphOptions::default();
    let mut outfile: Option<String> = None;
    let mut treeshake = true;
    let mut hmr = false;
    let mut react_refresh: Option<bool> = None;
    let mut https = false;
    let mut https_cert: Option<String> = None;
    let mut https_key: Option<String> = None;
    let mut main_fields_set = false;
    let mut tsconfig_path: Option<String> = None;
    let mut outdir: Option<String> = None;
    let mut use_tsconfig = true;
    let mut config_path: Option<String> = None;
    let mut use_config = true;
    let mut watch = serve;
    let mut port: u16 = 5173;
    let mut host_name = String::from("localhost");
    let mut mode = String::new();
    let mut cli_defines: Vec<rathkit_pack::define::Define> = Vec::new();
    let mut federation_file: Option<String> = None;
    let mut cors_origins: Vec<String> = Vec::new();
    let mut real_federation_plugin = false;
    // `import.meta.env`: where the .env files are, which names reach the program, and whether the files are read at all.
    let mut env_dir: Option<String> = None;
    let mut env_prefixes: Vec<String> = Vec::new();
    let mut env_files = true;
    let mut public_path_given = false;
    let mut metafile: Option<String> = None;
    let mut dts = false;
    let mut dts_dir: Option<String> = None;
    let mut dts_bundle = false;
    let mut inject_files: Vec<String> = Vec::new();
    let mut tsconfig_raw: Option<String> = None;
    let mut bundle_options = rathkit_pack::bundle::Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--outfile" => outfile = it.next().cloned(),
            "--no-treeshake" => treeshake = false,
            // --hmr (serve): modules are replaced in the running page instead of reloading it.
            "--hmr" => hmr = true,
            // --react-refresh / --no-react-refresh (with --hmr): components keep their state; by default on when the project has react-refresh.
            "--react-refresh" => react_refresh = Some(true),
            "--no-react-refresh" => react_refresh = Some(false),
            // --https [--https-cert FILE --https-key FILE] (serve): TLS in front of the dev server.
            "--https" => https = true,
            "--https-cert" => {
                https = true;
                https_cert = it.next().cloned();
            }
            "--https-key" => {
                https = true;
                https_key = it.next().cloned();
            }
            "--original-paths" => bundle_options.original_paths = true,
            "--external" => options.external.extend(it.next().cloned()),
            "--condition" => options.resolve.conditions.extend(it.next().cloned()),
            "--platform-browser" => options.node_builtins = false,
            "--config" => config_path = it.next().cloned(),
            "--no-config" => use_config = false,
            "--watch" => watch = true,
            "--port" => port = it.next().and_then(|p| p.parse().ok()).unwrap_or(5173),
            "--host" => host_name = it.next().cloned().unwrap_or_else(|| "localhost".into()),
            "--mode" => mode = it.next().cloned().unwrap_or_default(),
            "--tsconfig" => tsconfig_path = it.next().cloned(),
            "--no-tsconfig" => use_tsconfig = false,
            "--splitting" => bundle_options.splitting = true,
            "--federation" => federation_file = it.next().cloned(),
            "--cors-origin" => cors_origins.extend(it.next().cloned()),
            "--real-federation-plugin" => real_federation_plugin = true,
            "--minify" => {
                bundle_options.minify.whitespace = true;
                bundle_options.minify.identifiers = true;
                bundle_options.minify.syntax = true;
            }
            "--minify-syntax" => bundle_options.minify.syntax = true,
            "--minify-whitespace" => bundle_options.minify.whitespace = true,
            "--minify-identifiers" => bundle_options.minify.identifiers = true,
            "--keep-names" => bundle_options.minify.keep_names = Some(true),
            "--no-keep-names" => bundle_options.minify.keep_names = Some(false),
            "--outdir" => outdir = it.next().cloned(),
            "--sourcemap" | "--sourcemap=linked" => bundle_options.sourcemap = rathkit_pack::bundle::SourceMapMode::Linked,
            "--sourcemap=inline" => bundle_options.sourcemap = rathkit_pack::bundle::SourceMapMode::Inline,
            "--sourcemap=external" => bundle_options.sourcemap = rathkit_pack::bundle::SourceMapMode::External,
            "--no-sources-content" => bundle_options.sources_content = false,
            "--format" => {
                bundle_options.format = match it.next().map(String::as_str) {
                    Some("cjs") => rathkit_pack::bundle::Format::Cjs,
                    Some("iife") => rathkit_pack::bundle::Format::Iife,
                    _ => rathkit_pack::bundle::Format::Esm,
                }
            }
            "--global-name" => bundle_options.global_name = it.next().cloned(),
            "--global" => {
                // SPECIFIER=GLOBAL: the external module `react` is the global `React`.
                if let Some((spec, global)) = it.next().and_then(|a| a.split_once('=')) {
                    bundle_options.globals.push((spec.to_string(), global.to_string()));
                }
            }
            // KEY=VALUE: the expression KEY (a global name or a chain like process.env.NODE_ENV)
            // is replaced by VALUE (a constant, a name, or an expression); `--define:KEY=VALUE` works too.
            arg if arg == "--define" || arg.starts_with("--define:") => {
                let spec = if arg == "--define" { it.next().cloned() } else { Some(arg["--define:".len()..].to_string()) };
                match spec.as_deref().and_then(|s| s.split_once('=')).map(|(k, v)| rathkit_pack::define::Define::parse(k, v)) {
                    Some(Ok(define)) => {
                        cli_defines.push(define.clone());
                        options.defines.add(define)
                    }
                    Some(Err(message)) => {
                        eprintln!("error: --define: {message}");
                        return Done::Exit(ExitCode::from(2));
                    }
                    None => {
                        eprintln!("error: --define needs KEY=VALUE");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            // .ext=NAME: files with this extension are loaded as NAME (text, base64, dataurl, binary, file, empty, css, local-css).
            arg if arg == "--loader" || arg.starts_with("--loader:") => {
                let spec = if arg == "--loader" { it.next().cloned() } else { Some(arg["--loader:".len()..].to_string()) };
                match spec.as_deref().and_then(|s| s.split_once('=')) {
                    Some((ext, name)) => match rathkit_pack::loader::Loader::parse(name) {
                        Some(loader) => options.loaders.push((format!(".{}", ext.trim_start_matches('.').to_ascii_lowercase()), loader)),
                        None => {
                            eprintln!("error: --loader: '{name}' is not a loader (text, base64, dataurl, binary, file, empty, css, local-css)");
                            return Done::Exit(ExitCode::from(2));
                        }
                    },
                    None => {
                        eprintln!("error: --loader needs .ext=name");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            "--asset-names" => options.asset_names = it.next().cloned().unwrap_or_default(),
            "--public-path" => {
                options.public_path = it.next().cloned().unwrap_or_default();
                public_path_given = true;
            }
            "--preserve-symlinks" => options.resolve.preserve_symlinks = true,
            // --target=es2020: the version of JavaScript the output may use; newer syntax is rewritten, or refused.
            arg if arg == "--target" || arg.starts_with("--target=") => {
                let value = if arg == "--target" { it.next().cloned().unwrap_or_default() } else { arg["--target=".len()..].to_string() };
                match rathkit_pack::target::Target::parse(&value) {
                    Ok(target) => bundle_options.target = target,
                    Err(message) => {
                        eprintln!("error: --target: {message}");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            // --tsconfig-raw='{"compilerOptions":{...}}': a tsconfig given as text, which is used instead of any tsconfig.json.
            arg if arg == "--tsconfig-raw" || arg.starts_with("--tsconfig-raw=") => {
                tsconfig_raw = if arg == "--tsconfig-raw" { it.next().cloned() } else { Some(arg["--tsconfig-raw=".len()..].to_string()) };
            }
            // --inject:FILE: what FILE exports stands in for the globals of those names, in every module that uses one.
            arg if arg == "--inject" || arg.starts_with("--inject:") => {
                let file = if arg == "--inject" { it.next().cloned().unwrap_or_default() } else { arg["--inject:".len()..].to_string() };
                if file.is_empty() {
                    eprintln!("error: --inject needs a file");
                    return Done::Exit(ExitCode::from(2));
                }
                inject_files.push(file);
            }
            // What becomes of /*! ... */, @license and @preserve comments when minifying: eof (the default), none, linked, external.
            arg if arg == "--legal-comments" || arg.starts_with("--legal-comments=") => {
                let value = if arg == "--legal-comments" { it.next().cloned().unwrap_or_default() } else { arg["--legal-comments=".len()..].to_string() };
                bundle_options.legal_comments = match value.as_str() {
                    "eof" => rathkit_pack::legal::LegalComments::Eof,
                    "none" => rathkit_pack::legal::LegalComments::None,
                    "linked" => rathkit_pack::legal::LegalComments::Linked,
                    "external" => rathkit_pack::legal::LegalComments::External,
                    other => {
                        eprintln!("error: --legal-comments is eof, none, linked or external, not '{other}'");
                        return Done::Exit(ExitCode::from(2));
                    }
                };
            }
            // Templates for the names of files: [name] and [hash] (a hash of what is in the file and what it loads).
            arg if arg == "--entry-names" || arg == "--chunk-names" || arg.starts_with("--entry-names=") || arg.starts_with("--chunk-names=") => {
                let entry = arg.starts_with("--entry-names");
                let value = match arg.split_once('=') {
                    Some((_, v)) => v.to_string(),
                    None => it.next().cloned().unwrap_or_default(),
                };
                if value.is_empty() || value.contains("[dir]") || value.contains('/') {
                    eprintln!("error: {} needs a template such as [name]-[hash] (a [dir] or a / in it is not supported: files go in one folder)", if entry { "--entry-names" } else { "--chunk-names" });
                    return Done::Exit(ExitCode::from(2));
                }
                if entry {
                    bundle_options.entry_template = Some(value);
                } else {
                    bundle_options.chunk_template = Some(value);
                }
            }
            // --out-extension:.js=.mjs
            arg if arg == "--out-extension" || arg.starts_with("--out-extension:") => {
                let spec = if arg == "--out-extension" { it.next().cloned() } else { Some(arg["--out-extension:".len()..].to_string()) };
                match spec.as_deref().and_then(|v| v.split_once('=')) {
                    Some((from @ (".js" | ".css"), to)) if to.starts_with('.') && !to.contains(['/', '\\']) => bundle_options.out_extensions.push((from.to_string(), to.to_string())),
                    _ => {
                        eprintln!("error: --out-extension is .js=.EXT or .css=.EXT");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            // --pure:NAME: calls of NAME (a name, or a chain such as styled.div) are free of effects, so an unused one is removed.
            arg if arg == "--pure" || arg.starts_with("--pure:") => {
                let name = if arg == "--pure" { it.next().cloned().unwrap_or_default() } else { arg["--pure:".len()..].to_string() };
                if name.is_empty() {
                    eprintln!("error: --pure needs a name");
                    return Done::Exit(ExitCode::from(2));
                }
                options.pure_calls.push(name);
            }
            // --dts [--dts-dir=DIR]: declaration files for the entries, written with the project's TypeScript.
            "--dts" => dts = true,
            "--dts-bundle" => {
                dts = true;
                dts_bundle = true;
            }
            arg if arg == "--dts-dir" || arg.starts_with("--dts-dir=") => {
                dts = true;
                dts_dir = if arg == "--dts-dir" { it.next().cloned() } else { Some(arg["--dts-dir=".len()..].to_string()) };
            }
            arg if arg == "--metafile" || arg.starts_with("--metafile=") => {
                metafile = if arg == "--metafile" { it.next().cloned() } else { Some(arg["--metafile=".len()..].to_string()) };
            }
            // --drop:console and --drop:debugger (also --drop console).
            arg if arg == "--drop" || arg.starts_with("--drop:") => {
                let what = if arg == "--drop" { it.next().cloned().unwrap_or_default() } else { arg["--drop:".len()..].to_string() };
                match what.as_str() {
                    "console" => bundle_options.drop_console = true,
                    "debugger" => bundle_options.drop_debugger = true,
                    other => {
                        eprintln!("error: --drop is console or debugger, not '{other}'");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            // Text before and after the code of every file: --banner:js=TEXT, --banner:css=TEXT, --banner=TEXT (script), and the same for --footer.
            arg if arg == "--banner" || arg == "--footer" || arg.starts_with("--banner:") || arg.starts_with("--footer:") || arg.starts_with("--banner=") || arg.starts_with("--footer=") => {
                let banner = arg.starts_with("--banner");
                let rest = &arg[8..];
                let (kind, value) = match rest.split_once('=') {
                    Some((kind, value)) => (kind.trim_start_matches(':'), value.to_string()),
                    None => (rest.trim_start_matches(':'), it.next().cloned().unwrap_or_default()),
                };
                let slot = match (banner, kind) {
                    (true, "" | "js") => &mut bundle_options.banner_js,
                    (true, "css") => &mut bundle_options.banner_css,
                    (false, "" | "js") => &mut bundle_options.footer_js,
                    (false, "css") => &mut bundle_options.footer_css,
                    _ => {
                        eprintln!("error: {} is for js or css files, not '{kind}'", &arg[..8]);
                        return Done::Exit(ExitCode::from(2));
                    }
                };
                *slot = Some(value);
            }
            // esbuild's spelling: --packages=external leaves every package out, --packages=bundle (the default) puts them in.
            arg if arg == "--packages" || arg.starts_with("--packages=") => {
                let value = if arg == "--packages" { it.next().cloned().unwrap_or_default() } else { arg["--packages=".len()..].to_string() };
                match value.as_str() {
                    "external" => options.packages_external = true,
                    "bundle" => options.packages_external = false,
                    other => {
                        eprintln!("error: --packages is external or bundle, not '{other}'");
                        return Done::Exit(ExitCode::from(2));
                    }
                }
            }
            arg if arg == "--resolve-extensions" || arg.starts_with("--resolve-extensions=") => {
                let value = if arg == "--resolve-extensions" { it.next().cloned().unwrap_or_default() } else { arg["--resolve-extensions=".len()..].to_string() };
                let list: Vec<String> = value.split(',').map(str::trim).filter(|e| !e.is_empty()).map(|e| if e.starts_with('.') { e.to_string() } else { format!(".{e}") }).collect();
                if list.is_empty() {
                    eprintln!("error: --resolve-extensions needs a list such as .ts,.js");
                    return Done::Exit(ExitCode::from(2));
                }
                options.resolve.extensions = list;
            }
            "--env-dir" => env_dir = it.next().cloned(),
            "--env-prefix" => env_prefixes.extend(it.next().cloned()),
            "--no-env-files" => env_files = false,
            "--decorator-metadata" => options.decorator_metadata = true,
            "--experimental-decorators" => options.experimental_decorators = true,
            "--strict-null-checks" => options.strict_null_checks = true,
            "--alias" => {
                // NAME=PATH: the name, and anything below it, resolves to the path.
                if let Some((name, target)) = it.next().and_then(|a| a.split_once('=')) {
                    let target = std::path::absolute(target).unwrap_or_else(|_| target.into());
                    options.resolve.alias.push(rathkit_pack::resolve::Alias { pattern: name.to_string(), replacements: vec![target.to_string_lossy().into_owned()], exact_only: false });
                }
            }
            "--jsx" => options.jsx.automatic = it.next().is_none_or(|mode| mode != "classic"),
            "--jsx-dev" => {
                options.jsx.automatic = true;
                options.jsx.dev = true;
            }
            "--jsx-import-source" => options.jsx.import_source = it.next().cloned().unwrap_or_default(),
            "--jsx-factory" => options.jsx.factory = it.next().cloned().unwrap_or_default(),
            "--jsx-fragment" => options.jsx.fragment = it.next().cloned().unwrap_or_default(),
            "--main-field" => {
                if !main_fields_set {
                    options.resolve.main_fields.clear();
                    main_fields_set = true;
                }
                options.resolve.main_fields.extend(it.next().cloned());
            }
            // A word that starts with a dash and is not an option is a mistake (a typo, or an option this program does not have): it is not a file.
            arg if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("error: unknown option {arg} (the options are listed in the README)");
                return Done::Exit(ExitCode::from(2));
            }
            _ => entries.push(arg.clone()),
        }
    }
    let cwd = std::env::current_dir().expect("working directory");
    let mut restart_files: Vec<std::path::PathBuf> = Vec::new();
    for file in &inject_files {
        match rathkit_pack::graph::Inject::read(&rathkit_pack::graph::absolute(&cwd, file)) {
            Ok(inject) => options.inject.push(inject),
            Err(message) => {
                eprintln!("error: --inject: {message}");
                return Done::Exit(ExitCode::from(2));
            }
        }
    }
    // A build is `production` and the dev server `development`, unless --mode says otherwise.
    if mode.is_empty() {
        mode = if serve { "development" } else { "production" }.to_string();
    }
    {
        // `import.meta.env`: the mode, the base, and the `.env` files' names that start with a prefix; a --define of the same expression wins.
        let prefixes = if env_prefixes.is_empty() { vec!["VITE_".to_string()] } else { env_prefixes.clone() };
        let dir = env_dir.as_ref().map(|d| rathkit_pack::graph::absolute(&cwd, d)).unwrap_or_else(|| cwd.clone());
        let base = if public_path_given { options.public_path.as_str() } else { "/" };
        let env = rathkit_pack::env::EnvOptions { dir: &dir, mode: &mode, serve, prefixes: &prefixes, base, files: env_files };
        match rathkit_pack::env::definitions(&env, &|name| std::env::var(name).ok(), &options.defines.env_overrides()) {
            Ok((definitions, read)) => {
                for define in definitions {
                    options.defines.add_default(define);
                }
                // A change to a .env file starts the build again (the values are in the output).
                restart_files.extend(read);
            }
            Err(message) => {
                eprintln!("error: {message}");
                return Done::Exit(ExitCode::FAILURE);
            }
        }
    }
    if let Some(text) = &tsconfig_raw {
        match rathkit_pack::tsconfig::TsConfig::raw(&rathkit_pack::resolve::RealFs, text, &cwd) {
            Ok(config) => config.apply(&mut options),
            Err(message) => {
                eprintln!("error: --tsconfig-raw: {message}");
                return Done::Exit(ExitCode::from(2));
            }
        }
    } else if use_tsconfig {
        let found = match &tsconfig_path {
            Some(path) => Some(std::path::absolute(path).unwrap_or_else(|_| path.into())),
            None => cwd.ancestors().map(|d| d.join("tsconfig.json")).find(|p| p.is_file()),
        };
        restart_files.extend(found);
        let fs = rathkit_pack::resolve::RealFs;
        let config = match &tsconfig_path {
            Some(path) => rathkit_pack::tsconfig::TsConfig::load(&fs, &std::path::absolute(path).unwrap_or_else(|_| path.into())),
            None => rathkit_pack::tsconfig::TsConfig::find(&fs, &cwd),
        };
        if let Some(config) = config {
            config.apply(&mut options);
        }
    }
    // Plugins: a config file that lists them, named or found next to the project.
    let config_file = match (&config_path, use_config) {
        (Some(path), _) => Some(path.clone()),
        (None, true) => ["rathkit.config.mjs", "rathkit.config.js", "rathkit.config.cjs", ".parcelrc"].iter().find(|f| cwd.join(f).is_file()).map(|f| f.to_string()),
        (None, false) => None,
    };
    let plugin_host = match &config_file {
        Some(file) => {
            let info = rathkit_pack::plugins::CliInfo {
                entries: entries.clone(),
                outdir: outdir.clone(),
                outfile: outfile.clone(),
                format: match bundle_options.format {
                    rathkit_pack::bundle::Format::Esm => "es",
                    rathkit_pack::bundle::Format::Cjs => "cjs",
                    rathkit_pack::bundle::Format::Iife => "iife",
                }
                .to_string(),
                minify: bundle_options.minify.whitespace || bundle_options.minify.identifiers || bundle_options.minify.syntax,
                sourcemap: bundle_options.sourcemap != rathkit_pack::bundle::SourceMapMode::None,
                splitting: bundle_options.splitting,
                external: options.external.clone(),
                loaders: options.loaders.iter().map(|(e, l)| (e.clone(), format!("{l:?}").to_ascii_lowercase())).collect(),
                platform_browser: !options.node_builtins,
                mode: mode.clone(),
                real_federation_plugin,
                serve,
                public_path: options.public_path.clone(),
            };
            match rathkit_pack::plugins::PluginHost::start(&cwd, Some(file), &info) {
                Ok(host) => Some(host),
                Err(message) => {
                    eprintln!("error: {message}");
                    return Done::Exit(ExitCode::FAILURE);
                }
            }
        }
        None => None,
    };
    if let Some(host) = &plugin_host {
        // The config's definitions come first, so `--define` on the command line wins.
        for (key, value) in &host.info.define {
            match rathkit_pack::define::Define::parse(key, value) {
                Ok(define) => options.defines.add(define),
                Err(message) => eprintln!("warning: define {key}: {message}"),
            }
        }
        for define in &cli_defines {
            options.defines.add(define.clone());
        }
        // Where plugins have the resolver look: conditions and package.json fields (a `svelte` field, a `svelte` condition).
        for condition in &host.info.conditions {
            if !options.resolve.conditions.contains(condition) {
                options.resolve.conditions.push(condition.clone());
            }
        }
        for (position, field) in host.info.main_fields.iter().enumerate() {
            if !options.resolve.main_fields.contains(field) {
                options.resolve.main_fields.insert(position.min(options.resolve.main_fields.len()), field.clone());
            }
        }
        if entries.is_empty() {
            entries = host.info.input.clone();
        }
        options.plugins = Some(host.clone());
    }
    // An html page names its entries: the module scripts it points at. They are built, and the page is written again pointing at the built files.
    let mut pages: Vec<Page> = Vec::new();
    if !serve {
        let mut expanded = Vec::new();
        for entry in &entries {
            if !entry.to_ascii_lowercase().ends_with(".html") {
                expanded.push(entry.clone());
                continue;
            }
            let file = rathkit_pack::graph::absolute(&cwd, entry);
            let html = match std::fs::read_to_string(&file) {
                Ok(html) => html,
                Err(e) => {
                    eprintln!("error: {entry}: {e}");
                    return Done::Exit(ExitCode::FAILURE);
                }
            };
            let dir = file.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| cwd.clone());
            let scripts: Vec<(rathkit_pack::html::ScriptRef, std::path::PathBuf)> = rathkit_pack::html::module_scripts(&html)
                .into_iter()
                .map(|s| {
                    let path = if let Some(rooted) = s.src.strip_prefix('/') { rathkit_pack::graph::absolute(&cwd, rooted) } else { rathkit_pack::graph::absolute(&dir, &s.src) };
                    (s, path)
                })
                .collect();
            let styles: Vec<(rathkit_pack::html::ScriptRef, std::path::PathBuf)> = rathkit_pack::html::style_sheets(&html)
                .into_iter()
                .map(|s| {
                    let path = if let Some(rooted) = s.src.strip_prefix('/') { rathkit_pack::graph::absolute(&cwd, rooted) } else { rathkit_pack::graph::absolute(&dir, &s.src) };
                    (s, path)
                })
                .filter(|(_, path)| path.is_file())
                .collect();
            if scripts.is_empty() {
                eprintln!("error: {entry} has no <script type=\"module\" src=\"...\"> to build");
                return Done::Exit(ExitCode::FAILURE);
            }
            expanded.extend(scripts.iter().chain(&styles).map(|(_, path)| path.to_string_lossy().into_owned()));
            pages.push(Page { file, html, scripts, styles });
        }
        if !pages.is_empty() && outdir.is_none() {
            eprintln!("error: a page is written next to its scripts; give --outdir");
            return Done::Exit(ExitCode::FAILURE);
        }
        entries = expanded;
        // Each script and sheet of a page is a file of its own (ES modules); the other formats make one file of them all.
        if !pages.is_empty() {
            match bundle_options.format {
                rathkit_pack::bundle::Format::Esm => bundle_options.splitting = true,
                rathkit_pack::bundle::Format::Iife => {}
                rathkit_pack::bundle::Format::Cjs => {
                    eprintln!("error: a page runs in a browser, which cannot load CommonJS; use --format esm (the default) or iife");
                    return Done::Exit(ExitCode::FAILURE);
                }
            }
        }
    }
    // Module federation: options from a file, or from a plugin in the config.
    let federation_json = match &federation_file {
        Some(file) => match std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|t| rathkit_pack::json::Json::parse(&t).map_err(|e| e.to_string())) {
            Ok(json) => Some(json),
            Err(message) => {
                eprintln!("error: {file}: {message}");
                return Done::Exit(ExitCode::FAILURE);
            }
        },
        None => plugin_host.as_ref().and_then(|h| h.info.federation.clone()),
    };
    if let Some(json) = &federation_json {
        match rathkit_pack::federation::Federation::from_json(json, &cwd) {
            Ok(federation) => {
                let federation = std::sync::Arc::new(federation);
                if bundle_options.format != rathkit_pack::bundle::Format::Esm {
                    eprintln!("error: module federation needs the esm format");
                    return Done::Exit(ExitCode::FAILURE);
                }
                if outdir.is_none() && !serve {
                    eprintln!("error: module federation writes several files; give --outdir");
                    return Done::Exit(ExitCode::FAILURE);
                }
                bundle_options.splitting = true;
                bundle_options.federation = Some(federation.clone());
                options.federation = Some(federation);
            }
            Err(message) => {
                eprintln!("error: module federation: {message}");
                return Done::Exit(ExitCode::FAILURE);
            }
        }
    }
    if watch {
        // Watching builds again and again: modules whose files did not change are kept between builds.
        options.cache = Some(std::sync::Arc::new(rathkit_pack::graph::ModuleCache::default()));
    }
    let serve_state = if serve {
        // Maps are served from memory and name the files relative to the project.
        if bundle_options.sourcemap == rathkit_pack::bundle::SourceMapMode::None {
            bundle_options.sourcemap = rathkit_pack::bundle::SourceMapMode::Linked;
        }
        bundle_options.map_base = Some(cwd.clone());
        bundle_options.hot = true;
        Some(rathkit_pack::serve::ServeState::new(cwd.clone(), plugin_host.clone()))
    } else {
        None
    };
    if https && serve_state.is_none() {
        eprintln!("error: --https is for rkp serve");
        return Done::Exit(ExitCode::from(2));
    }
    if hmr {
        if serve_state.is_none() {
            eprintln!("error: --hmr is for rkp serve");
            return Done::Exit(ExitCode::from(2));
        }
        if options.federation.is_some() {
            eprintln!("error: --hmr does not work with module federation");
            return Done::Exit(ExitCode::from(2));
        }
        bundle_options.hmr = true;
        treeshake = false;
        // React Fast Refresh needs the runtime of the project's own `react-refresh`.
        let runtime = cwd.ancestors().map(|d| d.join("node_modules").join("react-refresh").join("cjs").join("react-refresh-runtime.development.js")).find(|f| f.is_file());
        let has_react = cwd.ancestors().any(|d| d.join("node_modules").join("react").join("package.json").is_file());
        let wanted = react_refresh.unwrap_or(runtime.is_some() && has_react);
        if wanted {
            let Some(runtime) = runtime else {
                eprintln!("error: --react-refresh needs the react-refresh package (install it in the project)");
                return Done::Exit(ExitCode::from(2));
            };
            if let Some(state) = &serve_state {
                if let Err(e) = state.use_react_refresh(&runtime) {
                    eprintln!("error: cannot read {}: {e}", runtime.display());
                    return Done::Exit(ExitCode::from(2));
                }
            }
            bundle_options.react_refresh = true;
        }
    }
    if let (Some(state), Some(federation)) = (&serve_state, &options.federation) {
        // Declarations written after a rebuild are served as soon as they are there.
        let state = state.clone();
        federation.on_types(std::sync::Arc::new(move |files| state.add_files(files.iter().map(|f| (f.name.clone(), f.data.clone().unwrap_or_else(|| f.code.clone().into_bytes()))).collect())));
    }
    if let (Some(state), Some(_)) = (&serve_state, &options.federation) {
        // Pages of other dev servers load this one's files, and listen for its reloads.
        let mut origins = vec!["local".to_string()];
        origins.extend(cors_origins.iter().cloned());
        state.allow_origins(origins);
    }
    // Workers are scripts built on their own; the files they read and the files they copy are noted for the build that asked.
    let workers = std::sync::Arc::new(WorkerLog::default());
    options.worker_builder = Some(worker_builder(cwd.clone(), options.clone(), bundle_options.clone(), treeshake, workers.clone(), 0));
    let job = std::sync::Arc::new(Job { cwd, entries, options, bundle_options, treeshake, outfile, outdir, host: plugin_host, config_file, watch, serve: serve_state, bind: (host_name, port), restart_files, workers, pages, metafile, dts: dts.then(|| (dts_dir, tsconfig_path.clone(), dts_bundle)), https: https.then(|| (https_cert, https_key)) });
    if let Some(host) = &job.host {
        // Child compilers of webpack plugins build one entry as a script that sets a global.
        let for_children = job.clone();
        host.set_child_builder(Some(std::sync::Arc::new(move |entry: &str, global: &str| child_build(&for_children, entry, global))));
    }
    if job.watch {
        return watch_loop(job);
    }
    let outcome = run_build(&job);
    if let Some(host) = &job.host {
        if let Err(message) = host.close_bundle() {
            eprintln!("error: {message}");
        }
        host.shutdown();
    }
    Done::Exit(if outcome.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Everything one build needs, fixed once the command line and the config are read.
struct Job {
    cwd: std::path::PathBuf,
    entries: Vec<String>,
    options: rathkit_pack::graph::GraphOptions,
    bundle_options: rathkit_pack::bundle::Options,
    treeshake: bool,
    outfile: Option<String>,
    outdir: Option<String>,
    host: Option<std::sync::Arc<rathkit_pack::plugins::PluginHost>>,
    config_file: Option<String>,
    watch: bool,
    serve: Option<std::sync::Arc<rathkit_pack::serve::ServeState>>,
    bind: (String, u16),
    /// Files read at startup whose change means starting again (`tsconfig.json`).
    restart_files: Vec<std::path::PathBuf>,
    workers: std::sync::Arc<WorkerLog>,
    pages: Vec<Page>,
    /// Where to write what went into the build and what came out, as JSON.
    metafile: Option<String>,
    /// `--dts`: with the folder to write the declarations to, or none for the output folder.
    dts: Option<(Option<String>, Option<String>, bool)>,
    /// `--https`: the certificate and key files, or none for a self-signed certificate.
    https: Option<(Option<String>, Option<String>)>,
}

/// An html page given as an entry: its text, and the scripts in it with the files they come from.
struct Page {
    file: std::path::PathBuf,
    html: String,
    scripts: Vec<(rathkit_pack::html::ScriptRef, std::path::PathBuf)>,
    /// The style sheets it links: they are built too, and the link points at the built sheet.
    styles: Vec<(rathkit_pack::html::ScriptRef, std::path::PathBuf)>,
}

/// The pages of a build, written next to its scripts: each script points at the built file, the style sheets are linked,
/// the images and other files a page names are copied, plugins' `transformIndexHtml` runs with the built files, and
/// `public/` is copied.
fn write_pages(job: &Job, dir: &std::path::Path, output: &rathkit_pack::bundle::Output) -> Result<(), String> {
    if job.pages.is_empty() {
        return Ok(());
    }
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).map(|c| rathkit_pack::resolve::without_verbatim_prefix(&c)).unwrap_or_else(|_| p.to_path_buf());
    // Style sheet files end in `.css`, or in what --out-extension:.css= says.
    let css_extension = job.bundle_options.out_extensions.iter().find(|(from, _)| from == ".css").map_or(".css", |(_, to)| to.as_str());
    let sheets: Vec<&str> = output.files.iter().map(|f| f.name.as_str()).filter(|n| n.ends_with(css_extension)).collect();
    let public = job.cwd.join("public");
    for page in &job.pages {
        let mut html = page.html.clone();
        // Every change to the text, with the place it is made at; applied last first so the places of the others stay true.
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        // With ES modules each script and sheet is a file of its own. In the other formats everything is one file, which the
        // first script loads and the first sheet links; the other tags would load it again, so they go.
        let single = !job.bundle_options.splitting;
        let mut refs: Vec<(&rathkit_pack::html::ScriptRef, &std::path::PathBuf, bool)> = page.scripts.iter().map(|(s, p)| (s, p, false)).chain(page.styles.iter().map(|(s, p)| (s, p, true))).collect();
        refs.sort_by_key(|(r, _, _)| r.start);
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (reference, path, is_sheet) in refs {
            let built = output.chunks.iter().find(|c| c.is_entry && (single || c.facade_module.as_ref().is_some_and(|m| canon(std::path::Path::new(m)) == canon(path))));
            let Some(chunk) = built else { return Err(format!("{}: no built file for {}", page.file.display(), reference.src)) };
            let file = if is_sheet {
                // A sheet is built as an entry: its rules are in the `.css` beside the (empty) script.
                let sheet = format!("{}{css_extension}", chunk.file.rsplit_once('.').map_or(chunk.file.as_str(), |(stem, _)| stem));
                if !output.files.iter().any(|f| f.name == sheet) {
                    return Err(format!("{}: no built style sheet for {}", page.file.display(), reference.src));
                }
                if !single {
                    let _ = std::fs::remove_file(dir.join(&chunk.file));
                    let _ = std::fs::remove_file(dir.join(format!("{}.map", chunk.file)));
                }
                sheet
            } else {
                chunk.file.clone()
            };
            if used.insert(file.clone()) {
                edits.push((reference.start, reference.end, format!("{}{}", job.options.public_path, file)));
            } else {
                edits.push((reference.tag_start, reference.tag_end, String::new()));
            }
        }
        // Images, icons, media and the `url()`s of `<style>` blocks: files of the project are copied under a name with a hash.
        // A file that is in `public/` stays where it is, and one that is nowhere stays as written.
        let page_dir = page.file.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| job.cwd.clone());
        for reference in rathkit_pack::html::assets(&page.html) {
            let (wanted, suffix) = match reference.src.find(['?', '#']) {
                Some(at) => (&reference.src[..at], &reference.src[at..]),
                None => (reference.src.as_str(), ""),
            };
            let file = match wanted.strip_prefix('/') {
                Some(rooted) if public.join(rooted).is_file() => continue,
                Some(rooted) => job.cwd.join(rooted),
                None => page_dir.join(wanted),
            };
            if !file.is_file() {
                continue;
            }
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let name = rathkit_pack::loader::asset_name(&job.options.asset_names, &file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), &bytes);
            std::fs::write(dir.join(&name), &bytes).map_err(|e| format!("{name}: {e}"))?;
            edits.push((reference.start, reference.end, format!("{}{name}{suffix}", job.options.public_path)));
        }
        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        for (start, end, text) in edits {
            html.replace_range(start..end, &text);
        }
        let links: String = sheets.iter().filter(|n| !html.contains(*n)).map(|n| format!("<link rel=\"stylesheet\" href=\"{}{n}\">\n", job.options.public_path)).collect();
        if !links.is_empty() {
            match html.to_ascii_lowercase().find("</head>") {
                Some(at) => html.insert_str(at, &links),
                None => html = format!("{links}{html}"),
            }
        }
        let relative = page.file.strip_prefix(&job.cwd).unwrap_or(&page.file).to_path_buf();
        if let Some(host) = &job.host {
            html = host.transform_built_page(&html, &format!("/{}", relative.to_string_lossy().replace('\\', "/")))?;
        }
        let target = dir.join(relative.file_name().map(std::path::PathBuf::from).unwrap_or_default());
        std::fs::write(&target, html).map_err(|e| format!("{}: {e}", target.display()))?;
        println!("{}  page", target.display());
    }
    copy_public(&public, dir)
}

/// What is in `public/` goes to the output as it is.
fn copy_public(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    if !from.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            copy_public(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// What building workers read and wrote, kept for the build that asked for them.
#[derive(Default)]
struct WorkerLog {
    watched: std::sync::Mutex<Vec<std::path::PathBuf>>,
    /// Files the workers copy (images they import): name and bytes.
    files: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
}

/// Builds a script on its own as an ES module with the options of the main build: what a worker is made of.
/// A worker that starts workers is built the same way, a few levels deep.
fn worker_builder(cwd: std::path::PathBuf, options: rathkit_pack::graph::GraphOptions, bundle_options: rathkit_pack::bundle::Options, treeshake: bool, log: std::sync::Arc<WorkerLog>, depth: usize) -> std::sync::Arc<rathkit_pack::graph::WorkerBuilder> {
    std::sync::Arc::new(move |entry: &std::path::Path| {
        let mut inner = options.clone();
        inner.cache = None;
        inner.federation = None;
        inner.worker_builder = (depth < 3).then(|| worker_builder(cwd.clone(), options.clone(), bundle_options.clone(), treeshake, log.clone(), depth + 1));
        let graph = rathkit_pack::graph::build(&cwd, &[entry.to_string_lossy().into_owned()], &inner);
        log.watched.lock().unwrap().extend(graph.modules.iter().map(|m| m.path.clone()).filter(|p| p.is_file()));
        if graph.has_errors() {
            return Err(graph.diagnostics.iter().filter(|d| d.severity == rathkit_pack::graph::Severity::Error).map(|d| d.message.clone()).collect::<Vec<_>>().join("\n"));
        }
        let linked = rathkit_pack::link::link(&graph);
        let shaken = if treeshake { rathkit_pack::shake::shake(&graph, &linked) } else { rathkit_pack::shake::keep_everything(&graph) };
        let mut o = bundle_options.clone();
        o.format = rathkit_pack::bundle::Format::Esm;
        o.global_name = None;
        o.splitting = false;
        o.sourcemap = rathkit_pack::bundle::SourceMapMode::None;
        o.entry_names = Vec::new();
        o.file_name = None;
        o.globals = Vec::new();
        o.federation = None;
        o.hot = false;
        o.hmr = false;
        o.react_refresh = false;
        o.map_base = None;
        match rathkit_pack::bundle::bundle(&graph, &linked, &shaken, &o) {
            Ok(output) => {
                log.files.lock().unwrap().extend(output.files.iter().skip(1).filter_map(|f| f.data.as_ref().map(|d| (f.name.clone(), d.clone()))));
                Ok(output.code.into_bytes())
            }
            Err(diagnostics) => Err(diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n")),
        }
    })
}

struct Outcome {
    ok: bool,
    /// Files whose change should start another build.
    watched: Vec<std::path::PathBuf>,
    /// What went wrong, for the page.
    error: Option<String>,
}

/// How a run of the command ended: with an exit code, or by asking to be started again (the
/// config file changed while watching).
enum Done {
    Exit(ExitCode),
    Restart,
}

fn format_name(format: rathkit_pack::bundle::Format) -> &'static str {
    match format {
        rathkit_pack::bundle::Format::Esm => "es",
        rathkit_pack::bundle::Format::Cjs => "cjs",
        rathkit_pack::bundle::Format::Iife => "iife",
    }
}

/// One entry built alone as a CommonJS module: what a webpack child compiler returns.
fn child_build(job: &Job, entry: &str, _global: &str) -> Result<(String, Vec<(String, Vec<u8>)>), String> {
    let graph = rathkit_pack::graph::build(&job.cwd, &[entry.to_string()], &job.options);
    if graph.has_errors() {
        return Err(graph.diagnostics.iter().filter(|d| d.severity == rathkit_pack::graph::Severity::Error).map(|d| d.message.clone()).collect::<Vec<_>>().join("\n"));
    }
    let linked = rathkit_pack::link::link(&graph);
    let shaken = rathkit_pack::shake::shake(&graph, &linked);
    let mut options = job.bundle_options.clone();
    // CommonJS keeps `require` for what stays outside; the caller wraps the result in the global it wants.
    options.format = rathkit_pack::bundle::Format::Cjs;
    options.global_name = None;
    options.splitting = false;
    options.sourcemap = rathkit_pack::bundle::SourceMapMode::None;
    options.minify = Default::default();
    options.entry_names = Vec::new();
    options.file_name = None;
    options.globals = Vec::new();
    match rathkit_pack::bundle::bundle(&graph, &linked, &shaken, &options) {
        Ok(output) => {
            let assets = output.files.iter().filter_map(|f| f.data.as_ref().map(|d| (f.name.clone(), d.clone()))).collect();
            Ok((output.code, assets))
        }
        Err(diagnostics) => Err(diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n")),
    }
}

/// One build: plugins' `buildStart`, the graph, link, shake, bundle, the output hooks, and
/// writing the files.
fn run_build(job: &Job) -> Outcome {
    use rathkit_pack::graph::{Severity, build};
    let cwd = &job.cwd;
    let options = &job.options;
    let (outfile, outdir) = (&job.outfile, &job.outdir);
    let plugin_host = &job.host;
    let mut bundle_options = job.bundle_options.clone();
    let mut outcome = Outcome { ok: false, watched: Vec::new(), error: None };
    if let Some(host) = plugin_host {
        if let Err(message) = host.build_start() {
            eprintln!("error: {message}");
            return outcome;
        }
    }
    let timing = std::env::var_os("RATHKIT_TIMING").is_some();
    let started = Instant::now();
    // Plugins may ask for more entry points (`this.emitFile({ type: 'chunk' })`) while the graph is built;
    // the graph is built again with them until no new ones turn up.
    let mut entries = job.entries.clone();
    let mut entry_names: Vec<Option<String>> = vec![None; entries.len()];
    let mut seen_chunks: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(federation) = &options.federation {
        if let Err(message) = federation.read_types(job.watch) {
            eprintln!("error: {message}");
            return outcome;
        }
    }
    let graph = loop {
        let graph = match &options.federation {
            Some(federation) => match federation.build_graph(cwd, &entries, options) {
                Ok(graph) => graph,
                Err(problems) => {
                    for d in &problems {
                        eprintln!("error: {}", d.message);
                    }
                    outcome.error = Some(problems.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n"));
                    return outcome;
                }
            },
            None => build(cwd, &entries, options),
        };
        let Some(host) = plugin_host else { break graph };
        match host.emitted_chunks() {
            Ok(list) => {
                let fresh: Vec<_> = list.into_iter().filter(|c| seen_chunks.insert(c.reference.clone())).collect();
                if fresh.is_empty() {
                    break graph;
                }
                for chunk in fresh {
                    entries.push(chunk.id);
                    entry_names.push(Some(chunk.file_name));
                }
            }
            Err(message) => {
                eprintln!("error: {message}");
                return outcome;
            }
        }
    };
    outcome.watched = graph.modules.iter().map(|m| m.path.clone()).filter(|p| p.is_file()).collect();
    // What the resolver read about packages (`exports`, `sideEffects`, `type`) decides how modules are built.
    let mut package_dirs: Vec<&std::path::Path> = graph.modules.iter().filter_map(|m| m.package.as_ref().map(|p| p.dir.as_path())).collect();
    package_dirs.sort();
    package_dirs.dedup();
    outcome.watched.extend(package_dirs.into_iter().map(|d| d.join("package.json")).filter(|p| p.is_file()));
    // A file that failed to parse is not in the graph, and is the one to watch for the fix.
    for d in &graph.diagnostics {
        if let Some(file) = &d.file {
            if file.is_file() && !outcome.watched.contains(file) {
                outcome.watched.push(file.clone());
            }
        }
    }
    outcome.watched.extend(job.workers.watched.lock().unwrap().drain(..));
    outcome.watched.extend(graph.watch_dirs.iter().cloned());
    if let Some(host) = plugin_host {
        outcome.watched.extend(host.watch_files());
        let failure = graph.diagnostics.iter().find(|d| d.severity == Severity::Error).map(|d| d.message.clone());
        if let Err(message) = host.module_graph(&graph, &entries) {
            eprintln!("error: {message}");
        }
        let _ = host.build_end(failure.as_deref());
    }
    if entries.len() > job.entries.len() {
        // Every chunk a plugin asked for is a file of its own.
        if bundle_options.format != rathkit_pack::bundle::Format::Esm {
            eprintln!("error: a plugin emitted chunks, which need the esm format (--format esm)");
            return outcome;
        }
        bundle_options.splitting = true;
    }
    if options.federation.is_some() {
        // The runtime's own entry follows the application's.
        entry_names.push(None);
    }
    bundle_options.entry_names = entry_names;
    if timing {
        eprintln!("  graph: {:.1} ms ({} modules)", started.elapsed().as_secs_f64() * 1000.0, graph.modules.len());
    }
    let linked_at = Instant::now();
    let linked = rathkit_pack::link::link(&graph);
    if timing {
        eprintln!("  link: {:.1} ms", linked_at.elapsed().as_secs_f64() * 1000.0);
    }
    let mut failed = false;
    let mut error_texts: Vec<String> = Vec::new();
    for d in graph.diagnostics.iter().chain(&linked.diagnostics) {
        let label = if d.severity == Severity::Error { failed = true; "error" } else { "warning" };
        let place = match (&d.file, d.pos) {
            (Some(file), Some(pos)) => {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                let before = &text.as_bytes()[..(pos as usize).min(text.len())];
                let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
                let col = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
                format!("{}:{line}:{col}: ", file.strip_prefix(cwd).unwrap_or(file).display())
            }
            (Some(file), None) => format!("{}: ", file.strip_prefix(cwd).unwrap_or(file).display()),
            _ => String::new(),
        };
        eprintln!("{place}{label}: {}", d.message);
        if d.severity == Severity::Error {
            error_texts.push(format!("{place}{}", d.message));
        }
    }
    if failed {
        outcome.error = Some(error_texts.join("\n"));
        // The modules that did build are good for the next try.
        if let Some(cache) = &job.options.cache {
            cache.store(graph);
        }
        return outcome;
    }
    let shake_at = Instant::now();
    let entry_exports = !(bundle_options.format == rathkit_pack::bundle::Format::Iife && bundle_options.global_name.is_none());
    let shaken = if job.treeshake { rathkit_pack::shake::shake_entries(&graph, &linked, entry_exports) } else { rathkit_pack::shake::keep_everything(&graph) };
    if timing {
        eprintln!("  shake: {:.1} ms", shake_at.elapsed().as_secs_f64() * 1000.0);
    }
    let bundle_at = Instant::now();
    // Files written beside the output (style sheets) are named after it.
    if let Some(name) = outfile.as_ref().and_then(|f| std::path::Path::new(f).file_name()) {
        bundle_options.file_name = Some(name.to_string_lossy().into_owned());
    }
    if bundle_options.sourcemap != rathkit_pack::bundle::SourceMapMode::None && job.serve.is_none() {
        // The map names the original files relative to where the output will be.
        let target = match (outdir, outfile) {
            (Some(dir), _) => Some(std::path::PathBuf::from(dir)),
            (None, Some(file)) => std::path::Path::new(file).parent().map(|p| p.to_path_buf()),
            _ => None,
        };
        match target {
            Some(dir) => {
                let dir = if dir.as_os_str().is_empty() { cwd.clone() } else { dir };
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("{}: {e}", dir.display());
                    return outcome;
                }
                bundle_options.map_base = std::fs::canonicalize(&dir).ok().map(|c| rathkit_pack::resolve::without_verbatim_prefix(&c));
            }
            None if bundle_options.sourcemap != rathkit_pack::bundle::SourceMapMode::Inline => {
                eprintln!("error: --sourcemap writes a file next to the output; give --outfile or --outdir, or use --sourcemap=inline");
                return outcome;
            }
            None => {}
        }
    }
    if let Some(federation) = &options.federation {
        federation.remember(&bundle_options);
    }
    let mut result = rathkit_pack::bundle::bundle(&graph, &linked, &shaken, &bundle_options);
    if let Ok(output) = &mut result {
        // What the workers copy goes beside the rest.
        for (name, data) in job.workers.files.lock().unwrap().drain(..) {
            if !output.files.iter().any(|f| f.name == name) {
                output.files.push(rathkit_pack::bundle::OutputFile { name, code: String::new(), map: None, data: Some(data), original: None });
            }
        }
    }
    if let (Ok(output), Some(federation)) = (&mut result, &options.federation) {
        if let Err(message) = federation.finish(output, bundle_options.hot, job.watch) {
            result = Err(vec![rathkit_pack::graph::Diagnostic { severity: Severity::Error, message, file: None, pos: None }]);
        }
    }
    if timing {
        eprintln!("  bundle (all chunks): {:.1} ms", bundle_at.elapsed().as_secs_f64() * 1000.0);
    }
    // Plugins get the finished chunks (renderChunk), then the whole output (generateBundle).
    let mut written_names: Vec<String> = Vec::new();
    let format = format_name(bundle_options.format);
    let result = match (result, plugin_host) {
        (Ok(mut output), Some(host)) => {
            let dir = outdir.clone().or_else(|| outfile.as_ref().and_then(|f| std::path::Path::new(f).parent().map(|p| p.to_string_lossy().into_owned())));
            match rathkit_pack::plugins::post_process(host, &mut output.files, &output.chunks, format, dir.as_deref()) {
                Ok(()) => {
                    if let Some(first) = output.files.first() {
                        output.code = first.code.clone();
                    }
                    written_names = output.files.iter().map(|f| f.name.clone()).collect();
                    Ok(output)
                }
                Err(message) => Err(vec![rathkit_pack::graph::Diagnostic { severity: Severity::Error, message, file: None, pos: None }]),
            }
        }
        (other, _) => other,
    };
    if let (Ok(output), Some(file)) = (&result, &job.metafile) {
        let target = std::path::Path::new(file);
        let text = rathkit_pack::metafile::build(&graph, output, cwd);
        if let Err(e) = target.parent().filter(|p| !p.as_os_str().is_empty()).map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(target, text)) {
            eprintln!("error: {file}: {e}");
            return outcome;
        }
    }
    // The dev server keeps the build in memory instead of writing it.
    if let Some(state) = &job.serve {
        match result {
            Ok(output) => {
                let mut files = Vec::new();
                for f in &output.files {
                    files.push((f.name.clone(), f.data.clone().unwrap_or_else(|| f.code.clone().into_bytes())));
                    if let Some(map) = &f.map {
                        files.push((format!("{}.map", f.name), map.clone().into_bytes()));
                    }
                }
                let entries = output.chunks.iter().filter(|c| c.is_entry).filter_map(|c| c.facade_module.as_ref().map(|m| (std::path::PathBuf::from(m), c.file.clone()))).collect();
                state.publish(files, entries);
                if let Some(modules) = &output.hmr {
                    state.publish_modules(modules);
                }
                outcome.ok = true;
            }
            Err(diagnostics) => {
                let text = diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
                eprintln!("error: {text}");
                outcome.error = Some(text);
            }
        }
        if let Some(cache) = &job.options.cache {
            cache.store(graph);
        }
        return outcome;
    }
    let mut status = match result {
        Ok(output) if outdir.is_some() => {
            let dir = std::path::PathBuf::from(outdir.clone().unwrap());
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("{}: {e}", dir.display());
                return outcome;
            }
            for file in &output.files {
                if let Err(e) = write_output(&dir, file) {
                    eprintln!("{}: {e}", file.name);
                    return outcome;
                }
                if let Some(map) = &file.map {
                    if let Err(e) = std::fs::write(dir.join(format!("{}.map", file.name)), map) {
                        eprintln!("{}.map: {e}", file.name);
                        return outcome;
                    }
                }
                println!("{}  {:.1} KB", dir.join(&file.name).display(), file.data.as_ref().map_or(file.code.len(), |d| d.len()) as f64 / 1024.0);
            }
            if let Err(message) = write_pages(job, &dir, &output) {
                eprintln!("error: {message}");
                return outcome;
            }
            ExitCode::SUCCESS
        }
        Ok(output) => match outfile {
            Some(path) => match std::path::Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, &output.code))
                .and_then(|()| match output.files.first().and_then(|f| f.map.as_ref()) {
                    Some(map) => std::fs::write(format!("{path}.map"), map),
                    None => Ok(()),
                })
                .and_then(|()| {
                    // Style sheets and copied files go next to the output file.
                    let dir = std::path::Path::new(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    output.files.iter().skip(1).try_for_each(|f| {
                        write_output(&dir, f)?;
                        match &f.map {
                            Some(map) => std::fs::write(dir.join(format!("{}.map", f.name)), map),
                            None => Ok(()),
                        }
                    })
                }) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{path}: {e}");
                    ExitCode::FAILURE
                }
            },
            None => {
                print!("{}", output.code);
                ExitCode::SUCCESS
            }
        },
        Err(diagnostics) => {
            for d in diagnostics {
                eprintln!("error: {}", d.message);
            }
            ExitCode::FAILURE
        }
    };
    if status == ExitCode::SUCCESS {
        if let Some((folder, ts_config, bundle)) = &job.dts {
            let base = match (outdir, outfile) {
                (Some(dir), _) => std::path::PathBuf::from(dir),
                (None, Some(file)) => std::path::Path::new(file).parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                _ => cwd.to_path_buf(),
            };
            let target = folder.as_deref().map_or(base, std::path::PathBuf::from);
            match rathkit_pack::dts::emit(&job.entries.iter().map(|e| cwd.join(e)).collect::<Vec<_>>(), &target, ts_config.as_deref(), *bundle, cwd) {
                Ok(done) => {
                    for message in &done.messages {
                        eprintln!("types: {message}");
                    }
                    println!("{}  {} declaration file{}", target.display(), done.files.len(), if done.files.len() == 1 { "" } else { "s" });
                }
                Err(message) => {
                    eprintln!("error: --dts: {message}");
                    status = ExitCode::FAILURE;
                }
            }
        }
    }
    if status == ExitCode::SUCCESS {
        if let Some(host) = plugin_host {
            // `writeBundle` says where the files are: the output folder, which plugins read the files back from.
            let dir = match (outdir, outfile) {
                (Some(dir), _) => Some(std::path::absolute(dir).unwrap_or_else(|_| dir.into())),
                (None, Some(file)) => std::path::Path::new(file).parent().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())),
                _ => std::env::current_dir().ok(),
            };
            let dir = dir.map(|d| d.to_string_lossy().into_owned());
            if let Err(message) = host.write_bundle(&written_names, format, dir.as_deref()) {
                eprintln!("error: {message}");
            }
        }
    }
    outcome.ok = status == ExitCode::SUCCESS;
    if let Some(cache) = &job.options.cache {
        cache.store(graph);
    }
    outcome
}

// ---- watching ---------------------------------------------------------------------------------

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
fn catch_interrupt() {
    // `signal` is in the C library every program already links; no crate is needed to call it.
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    extern "C" fn on_interrupt(_: i32) {
        INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    // SIGINT and SIGTERM.
    unsafe {
        signal(2, on_interrupt as extern "C" fn(i32) as usize);
        signal(15, on_interrupt as extern "C" fn(i32) as usize);
    }
}

#[cfg(not(unix))]
fn catch_interrupt() {}

/// What a file looked like the last time: modification time and size, or nothing if it was gone.
type Stamp = Option<(std::time::SystemTime, u64)>;

fn stamp(path: &std::path::Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
    if meta.is_dir() {
        // A directory (watched because `import.meta.glob` lists it) is what names it holds: its modification time is not trusted to
        // change when a file is added (NTFS updates it late) and its size says nothing.
        let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(path).map(|d| d.filter_map(|e| e.ok().map(|e| e.file_name())).collect()).unwrap_or_default();
        names.sort();
        let mut hash = 0xcbf29ce484222325u64;
        for name in names {
            for byte in name.to_string_lossy().bytes().chain([0]) {
                hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
            }
        }
        return Some((modified, hash));
    }
    Some((modified, meta.len()))
}

/// Builds, then builds again whenever a file the build read changes, until interrupted. A change to
/// the config file starts the whole command again, since the plugins come from there.
fn watch_loop(job: std::sync::Arc<Job>) -> Done {
    use std::sync::atomic::Ordering;
    catch_interrupt();
    if let Some(host) = &job.host {
        host.set_watch_mode(true);
    }
    let config_path = job.config_file.as_ref().map(|f| job.cwd.join(f));
    // Kept for as long as the server runs: dropping it ends the HTTPS proxy.
    let mut _proxy = None;
    if let Some(state) = &job.serve {
        match &job.https {
            None => match rathkit_pack::serve::start(state.clone(), &job.bind.0, job.bind.1) {
                Ok(port) => println!("rkp serving at http://{}:{port}/", job.bind.0),
                Err(e) => {
                    eprintln!("error: cannot listen on {}:{}: {e}", job.bind.0, job.bind.1);
                    return Done::Exit(ExitCode::FAILURE);
                }
            },
            Some((cert, key)) => {
                let upstream = match rathkit_pack::serve::start_behind_proxy(state.clone(), &job.bind.0) {
                    Ok(port) => port,
                    Err(e) => {
                        eprintln!("error: cannot listen: {e}");
                        return Done::Exit(ExitCode::FAILURE);
                    }
                };
                match rathkit_pack::serve::start_https(&job.bind.0, job.bind.1, upstream, cert.as_deref(), key.as_deref(), &job.cwd) {
                    Ok((proxy, port)) => {
                        rathkit_pack::serve::configure(state, port, &job.bind.0);
                        println!("rkp serving at https://{}:{port}/", job.bind.0);
                        _proxy = Some(proxy);
                    }
                    Err(message) => {
                        eprintln!("error: {message}");
                        return Done::Exit(ExitCode::FAILURE);
                    }
                }
            }
        }
    }
    let mut began = std::time::SystemTime::now();
    let mut outcome = run_build(&job);
    if let (Some(state), Some(message)) = (&job.serve, &outcome.error) {
        state.set_error(message);
    }
    let mut stamps: std::collections::HashMap<std::path::PathBuf, Stamp> = std::collections::HashMap::new();
    // A file seen for the first time is stamped as it was when the build began; if it changed after that, the next look rebuilds.
    let remember = |outcome: &Outcome, stamps: &mut std::collections::HashMap<std::path::PathBuf, Stamp>, began: std::time::SystemTime| {
        let paths = outcome.watched.iter().chain(config_path.iter()).chain(job.restart_files.iter());
        *stamps = carry_stamps(paths, stamps, began);
        eprintln!("watching {} files (Ctrl-C to stop)", stamps.len());
    };
    remember(&outcome, &mut stamps, began);
    let mut restart = false;
    // Files under node_modules rarely change and are many: they are looked at every 25th time.
    let in_node_modules = |path: &std::path::Path| path.components().any(|c| c.as_os_str() == "node_modules");
    let mut tick: u32 = 0;
    while !INTERRUPTED.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(80));
        tick = tick.wrapping_add(1);
        let full_look = tick % 25 == 0;
        let mut changed: Vec<(std::path::PathBuf, &'static str)> = stamps
            .iter()
            .filter(|(path, _)| full_look || !in_node_modules(path))
            .filter_map(|(path, old)| {
                let now = stamp(path);
                if now == *old { None } else { Some((path.clone(), if old.is_none() { "create" } else if now.is_none() { "delete" } else { "update" })) }
            })
            .collect();
        if changed.is_empty() {
            continue;
        }
        // An editor often writes a file in several steps; let it finish.
        std::thread::sleep(std::time::Duration::from_millis(40));
        changed = stamps.iter().filter(|(path, _)| full_look || !in_node_modules(path)).filter_map(|(path, old)| { let now = stamp(path); if now == *old { None } else { Some((path.clone(), if old.is_none() { "create" } else if now.is_none() { "delete" } else { "update" })) } }).collect();
        changed.sort();
        if config_path.as_ref().is_some_and(|c| changed.iter().any(|(p, _)| p == c)) || changed.iter().any(|(p, _)| job.restart_files.contains(p)) {
            eprintln!("the config changed; starting again");
            restart = true;
            break;
        }
        for (path, _) in &changed {
            eprintln!("changed: {}", path.strip_prefix(&job.cwd).unwrap_or(path).display());
        }
        // A package.json decides how its modules are built, so modules kept from before may be out of date.
        if changed.iter().any(|(p, _)| p.file_name().is_some_and(|n| n == "package.json")) {
            if let Some(cache) = &job.options.cache {
                cache.clear();
            }
        }
        // Plugins may keep the page from reloading for a file (`handleHotUpdate` returning nothing).
        let mut suppressed = false;
        if let Some(host) = &job.host {
            let list: Vec<(String, &str)> = changed.iter().map(|(p, e)| (p.to_string_lossy().into_owned(), *e)).collect();
            if let Err(message) = host.watch_change(&list) {
                eprintln!("error: {message}");
            }
            if job.serve.is_some() {
                for (path, event) in &list {
                    suppressed |= host.hot_update(path, event).unwrap_or(false);
                }
            }
        }
        // What the files look like now is what this build will see (or something older, which the next look catches).
        for (path, _) in &changed {
            stamps.insert(path.clone(), stamp(path));
        }
        let started = Instant::now();
        began = std::time::SystemTime::now();
        outcome = run_build(&job);
        if let Some(state) = &job.serve {
            match &outcome.error {
                Some(message) => state.broadcast_error(message),
                None if outcome.ok && !suppressed && state.hmr_active() => {
                    // Modules are replaced in the page; style sheets are swapped; any other file reloads it.
                    let (update, ids) = state.hmr_state().unwrap_or_default();
                    let is_sheet = |p: &std::path::PathBuf| p.extension().is_some_and(|e| e == "css");
                    if changed.iter().all(|(p, _)| is_sheet(p) || ids.contains(&p.to_string_lossy().into_owned())) {
                        if let Some(json) = update {
                            state.broadcast(&json);
                        }
                        if changed.iter().any(|(p, _)| is_sheet(p)) {
                            let files: Vec<String> = state.style_sheets().iter().map(|f| format!("{f:?}")).collect();
                            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                            state.broadcast(&format!("{{\"type\":\"css-update\",\"files\":[{}],\"t\":{t}}}", files.join(",")));
                        }
                    } else {
                        state.broadcast("{\"type\":\"full-reload\"}");
                    }
                }
                None if outcome.ok && !suppressed => {
                    // Style sheets are swapped in place; anything else reloads the page.
                    let sheets_only = changed.iter().all(|(p, _)| p.extension().is_some_and(|e| e == "css") && !p.to_string_lossy().ends_with(".module.css"));
                    if sheets_only {
                        let files: Vec<String> = state.style_sheets().iter().map(|f| format!("{f:?}")).collect();
                        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                        state.broadcast(&format!("{{\"type\":\"css-update\",\"files\":[{}],\"t\":{t}}}", files.join(",")));
                    } else {
                        state.broadcast("{\"type\":\"full-reload\"}");
                    }
                }
                None => {}
            }
        }
        eprintln!("rebuilt in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
        remember(&outcome, &mut stamps, began);
    }
    if let Some(host) = &job.host {
        let _ = host.close_watcher();
        let _ = host.close_bundle();
        host.shutdown();
    }
    if restart { Done::Restart } else { Done::Exit(if outcome.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }) }
}

/// The stamps to keep after a build: those already known stay; a file seen for the first time gets the stamp it has now, unless
/// it was written after the build began (the build may have read it before), in which case it gets one no real file has,
/// so the next look finds it changed.
fn carry_stamps<'a>(paths: impl Iterator<Item = &'a std::path::PathBuf>, known: &std::collections::HashMap<std::path::PathBuf, Stamp>, began: std::time::SystemTime) -> std::collections::HashMap<std::path::PathBuf, Stamp> {
    let mut next = std::collections::HashMap::new();
    for path in paths {
        let kept = known.get(path).cloned().unwrap_or_else(|| match stamp(path) {
            Some((modified, _)) if modified > began => Some((std::time::UNIX_EPOCH, u64::MAX)),
            now => now,
        });
        next.insert(path.clone(), kept);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_file_written_during_the_build_is_found_changed_by_the_next_look() {
        let dir = std::env::temp_dir().join(format!("rkp-carry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (before, during) = (dir.join("before.js"), dir.join("during.js"));
        std::fs::write(&before, "1").unwrap();
        std::fs::write(&during, "1").unwrap();
        let known = std::collections::HashMap::new();
        let paths = [before.clone(), during.clone()];
        // The build began after `before` was written and before `during` was.
        let began = std::fs::metadata(&before).unwrap().modified().unwrap();
        std::fs::File::options().write(true).open(&during).unwrap().set_modified(began + Duration::from_secs(5)).unwrap();
        let next = carry_stamps(paths.iter(), &known, began);
        assert_eq!(next[&before], stamp(&before));
        assert_ne!(next[&during], stamp(&during), "a file changed during the build must look changed");
        // A build that began later sees both as they are.
        let later = carry_stamps(paths.iter(), &known, SystemTime::now() + Duration::from_secs(60));
        assert_eq!(later[&during], stamp(&during));
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Development aid: prints `OK` or `ERR <message>` for each TypeScript file, for comparison
/// with the TypeScript compiler's own parser (`tools/diff-ts.mjs`).
fn ts_verdicts(files: &[String]) -> ExitCode {
    for file in files {
        let Ok(src) = std::fs::read_to_string(file) else {
            println!("SKIP");
            continue;
        };
        let name = file.to_ascii_lowercase();
        let mut options = ParseOptions::new(true).typescript();
        if name.ends_with(".tsx") {
            options = options.with_jsx();
        }
        if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
            options = ParseOptions::new(true).declaration_file();
        }
        match parse(&src, options) {
            Ok(_) => println!("OK"),
            Err(e) => println!("ERR {} @{}", e.message, e.pos),
        }
    }
    ExitCode::SUCCESS
}

/// Writes one output file (text or bytes) under `dir`, making the folders its name asks for.
fn write_output(dir: &std::path::Path, file: &rathkit_pack::bundle::OutputFile) -> std::io::Result<()> {
    let target = dir.join(&file.name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match &file.data {
        Some(bytes) => std::fs::write(target, bytes),
        None => std::fs::write(target, &file.code),
    }
}

/// Writes the ESTree of one file (or standard input) as JSON. A syntax error is written as
/// `{"error":..,"pos":..}` with a failing exit code.
fn estree_command(args: &[String]) -> ExitCode {
    use std::io::{Read, Write};
    let script_first = args.iter().any(|a| a == "--script");
    let Some(file) = args.iter().find(|a| *a == "-" || !a.starts_with("--")) else { return ExitCode::from(2) };
    let mut src = String::new();
    let read = if file == "-" { std::io::stdin().read_to_string(&mut src).map(|_| ()) } else { std::fs::read_to_string(file).map(|s| src = s) };
    if let Err(e) = read {
        eprintln!("{file}: {e}");
        return ExitCode::from(2);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match rathkit_pack::estree::to_json(&src, script_first) {
        Ok(json) => {
            let _ = out.write_all(json.as_bytes());
            ExitCode::SUCCESS
        }
        Err(e) => {
            let message = format!("{:?}", e.message);
            let _ = write!(out, "{{\"error\":{message},\"pos\":{}}}", e.pos);
            ExitCode::from(1)
        }
    }
}

/// Writes the names a file uses without declaring them, as a JSON array.
fn globals_command(file: &str) -> ExitCode {
    use std::io::{Read, Write};
    let mut src = String::new();
    let read = if file == "-" { std::io::stdin().read_to_string(&mut src).map(|_| ()) } else { std::fs::read_to_string(file).map(|s| src = s) };
    if let Err(e) = read {
        eprintln!("{file}: {e}");
        return ExitCode::from(2);
    }
    match rathkit_pack::estree::free_variables(&src) {
        Ok(names) => {
            let items: Vec<String> = names.iter().map(|n| format!("{n:?}")).collect();
            let _ = write!(std::io::stdout(), "[{}]", items.join(","));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{file}: {}", e.message);
            ExitCode::from(1)
        }
    }
}

/// Answers parse requests from the plugin host until its pipe closes. A request is a header line
/// `KIND LENGTH` (`m` module first, `s` script first, `g` free variables) and then LENGTH bytes of source;
/// the answer is `LENGTH` on a line and then that many bytes of JSON.
fn estree_server() -> ExitCode {
    use std::io::{BufRead, Read, Write};
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut header = String::new();
    loop {
        header.clear();
        if input.read_line(&mut header).unwrap_or(0) == 0 {
            return ExitCode::SUCCESS;
        }
        let mut parts = header.split_whitespace();
        let (Some(kind), Some(length)) = (parts.next(), parts.next().and_then(|n| n.parse::<usize>().ok())) else { return ExitCode::from(2) };
        let mut bytes = vec![0u8; length];
        if input.read_exact(&mut bytes).is_err() {
            return ExitCode::from(2);
        }
        let src = String::from_utf8_lossy(&bytes).into_owned();
        let reply = match kind {
            "g" => match rathkit_pack::estree::free_variables(&src) {
                Ok(names) => format!("[{}]", names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(",")),
                Err(e) => format!("{{\"error\":{:?},\"pos\":{}}}", e.message, e.pos),
            },
            _ => match rathkit_pack::estree::to_json(&src, kind == "s") {
                Ok(json) => json,
                Err(e) => format!("{{\"error\":{:?},\"pos\":{}}}", e.message, e.pos),
            },
        };
        if write!(out, "{}\n{reply}", reply.len()).and_then(|()| out.flush()).is_err() {
            return ExitCode::SUCCESS;
        }
    }
}
