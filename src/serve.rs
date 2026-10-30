//! The development server of `rkp serve`: the build kept in memory, served over HTTP, with the page told
//! to reload (or to swap its style sheets) when a rebuild finishes.
//!
//! No dependencies: a `TcpListener`, a thread per connection, and server-sent events for the page. Plugins take part
//! through the host (Vite's `configureServer`, `transformIndexHtml`, `handleHotUpdate`); this side does the HTTP.
//!
//! Without `--hmr` a page cannot have module-by-module hot replacement: the output is one scope-hoisted program, so a script
//! change reloads the page. (With it the build is a table of modules; see `bundle/registry.rs`.) Style sheets are swapped without a reload, and `import.meta.hot` exists so code written
//! for it runs (its `accept` callbacks are not used; `dispose` callbacks run before the reload).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::plugins::PluginHost;

const CLIENT: &str = include_str!("serve/client.js");

pub struct ServeState {
    pub cwd: PathBuf,
    pub host: Option<Arc<PluginHost>>,
    files: Mutex<HashMap<String, Vec<u8>>>,
    /// Entry file (as written on the command line, relative to the project) and the output file that holds it.
    entries: Mutex<Vec<(PathBuf, String)>>,
    clients: Mutex<Vec<TcpStream>>,
    middlewares: Mutex<bool>,
    last_error: Mutex<Option<String>>,
    /// Listening beyond this machine: only `public/` is served from disk, and host names must be addresses.
    exposed: Mutex<bool>,
    bound_host: Mutex<String>,
    /// Origins other than this server that may read the build (module federation: a page of another
    /// dev server loads this one's files). `local` stands for any page on this machine.
    cors: Mutex<Vec<String>>,
    /// With `--hmr`: the text of every module of the last build, to find the ones a rebuild changed.
    modules: Mutex<Option<HashMap<String, String>>>,
    /// The update for the pages, made when the modules of a build were published.
    update: Mutex<Option<String>>,
    /// The modules of the last two builds, so that a file that was one (and may be gone) is known to be a module.
    known_ids: Mutex<std::collections::HashSet<String>>,
    /// With React Fast Refresh: the runtime of the project's `react-refresh`, as a script for the page.
    refresh_runtime: Mutex<Option<String>>,
}

impl ServeState {
    pub fn new(cwd: PathBuf, host: Option<Arc<PluginHost>>) -> Arc<ServeState> {
        Arc::new(ServeState { cwd, host, files: Mutex::new(HashMap::new()), entries: Mutex::new(Vec::new()), clients: Mutex::new(Vec::new()), middlewares: Mutex::new(false), last_error: Mutex::new(None), exposed: Mutex::new(false), bound_host: Mutex::new(String::new()), cors: Mutex::new(Vec::new()), modules: Mutex::new(None), update: Mutex::new(None), known_ids: Mutex::new(std::collections::HashSet::new()), refresh_runtime: Mutex::new(None) })
    }

    /// The finished build: files by name, and which file holds which entry.
    pub fn publish(&self, files: Vec<(String, Vec<u8>)>, entries: Vec<(PathBuf, String)>) {
        *self.files.lock().unwrap() = files.into_iter().collect();
        *self.entries.lock().unwrap() = entries;
        *self.last_error.lock().unwrap() = None;
    }

    /// The page gets the runtime of React Fast Refresh, from this file of the `react-refresh` package. It is read now and made into a
    /// script that runs before the page's own, so React finds the hook it injects into.
    pub fn use_react_refresh(&self, runtime_file: &Path) -> std::io::Result<()> {
        let source = std::fs::read_to_string(runtime_file)?;
        let script = format!(
            "(function () {{\nvar process = {{ env: {{ NODE_ENV: 'development' }} }};\nvar module = {{ exports: {{}} }};\nvar exports = module.exports;\n{source}\nvar runtime = module.exports;\nruntime.injectIntoGlobalHook(window);\nwindow.__rathkit_refresh = runtime;\nwindow.$RefreshReg$ = function () {{}};\nwindow.$RefreshSig$ = function () {{ return function (type) {{ return type; }}; }};\n}})();\n"
        );
        *self.refresh_runtime.lock().unwrap() = Some(script);
        Ok(())
    }

    /// With `--hmr`: the modules of the build just published. What changed since the last one becomes the update for the pages.
    pub fn publish_modules(&self, modules: &[crate::bundle::HmrModule]) {
        let quote = crate::loader::quote;
        let mut known = self.modules.lock().unwrap();
        let next: HashMap<String, String> = modules.iter().map(|m| (m.id.clone(), m.code.clone())).collect();
        let mut update = None;
        if let Some(before) = known.as_ref() {
            let mut entries = Vec::new();
            for m in modules.iter().filter(|m| before.get(&m.id) != Some(&m.code)) {
                let deps = m.deps.iter().map(|d| quote(d)).collect::<Vec<_>>().join(",");
                let specs = m.specs.iter().map(|(s, t)| format!("{}:{}", quote(s), quote(t))).collect::<Vec<_>>().join(",");
                entries.push(format!("{}:{{\"kind\":{},\"code\":{},\"deps\":[{deps}],\"specs\":{{{specs}}},\"refresh\":{}}}", quote(&m.id), quote(m.kind), quote(&m.code), m.refresh));
            }
            let removed: Vec<String> = before.keys().filter(|k| !next.contains_key(*k)).map(|k| quote(k)).collect();
            if !entries.is_empty() || !removed.is_empty() {
                update = Some(format!("{{\"type\":\"hmr\",\"modules\":{{{}}},\"removed\":[{}]}}", entries.join(","), removed.join(",")));
            }
        }
        let mut ids: std::collections::HashSet<String> = next.keys().cloned().collect();
        if let Some(before) = known.as_ref() {
            ids.extend(before.keys().cloned());
        }
        *self.known_ids.lock().unwrap() = ids;
        *known = Some(next);
        *self.update.lock().unwrap() = update;
    }

    /// Whether the build is a table of modules that pages can update.
    pub fn hmr_active(&self) -> bool {
        self.modules.lock().unwrap().is_some()
    }

    /// The update since the last build (if any change), with the modules, when the build is a table of modules that pages can update, and if so the update since the last build (if any change) and
    /// whether a file is one of its modules.
    pub fn hmr_state(&self) -> Option<(Option<String>, std::collections::HashSet<String>)> {
        self.modules.lock().unwrap().as_ref()?;
        Some((self.update.lock().unwrap().take(), self.known_ids.lock().unwrap().clone()))
    }

    /// Adds files to the published build (they are replaced when the next build is published).
    pub fn add_files(&self, files: Vec<(String, Vec<u8>)>) {
        self.files.lock().unwrap().extend(files);
    }

    /// Lets pages from these origins read the built files and listen for reloads.
    pub fn allow_origins(&self, origins: Vec<String>) {
        *self.cors.lock().unwrap() = origins;
    }

    /// The origin to answer with, when the request comes from one that may read the build.
    fn cors_origin(&self, request: &Request) -> Option<String> {
        let origin = request.headers.iter().find(|(k, _)| k == "origin").map(|(_, v)| v.clone())?;
        let allowed = self.cors.lock().unwrap();
        let host = origin.split("://").nth(1)?.rsplit_once(':').map_or_else(|| origin.split("://").nth(1).unwrap_or(""), |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { origin.split("://").nth(1).unwrap_or("") });
        let local = host == "localhost" || host.ends_with(".localhost") || host == "127.0.0.1" || host == "[::1]";
        (allowed.iter().any(|o| o == "*" || *o == origin) || (local && allowed.iter().any(|o| o == "local"))).then_some(origin)
    }

    pub fn style_sheets(&self) -> Vec<String> {
        let mut names: Vec<String> = self.files.lock().unwrap().keys().filter(|n| n.ends_with(".css")).cloned().collect();
        names.sort();
        names
    }

    pub fn set_error(&self, message: &str) {
        *self.last_error.lock().unwrap() = Some(message.to_string());
    }

    /// Sends a JSON message to every page that is listening.
    pub fn broadcast(&self, json: &str) {
        let mut clients = self.clients.lock().unwrap();
        let line = format!("data: {json}\n\n");
        clients.retain_mut(|c| c.write_all(line.as_bytes()).and_then(|()| c.flush()).is_ok());
    }

    pub fn broadcast_error(&self, message: &str) {
        self.set_error(message);
        self.broadcast(&format!("{{\"type\":\"error\",\"message\":{}}}", crate::loader::quote(message)));
    }
}

/// Binds the address and serves in the background. Returns the port.
pub fn start(state: Arc<ServeState>, host: &str, port: u16) -> std::io::Result<u16> {
    let port = bind_and_serve(state.clone(), host, host, port)?;
    configure(&state, port, host);
    Ok(port)
}

/// Serves on the loopback, on a port of its own, for the HTTPS proxy to pass requests to; `public_host` is the address people reach the
/// proxy at, which decides what is served (beyond this machine, only `public/`) and which host names are accepted.
pub fn start_behind_proxy(state: Arc<ServeState>, public_host: &str) -> std::io::Result<u16> {
    bind_and_serve(state, "127.0.0.1", public_host, 0)
}

/// Lets plugins set up the server (`configureServer`) and send messages to pages.
pub fn configure(state: &Arc<ServeState>, port: u16, host: &str) {
    if let Some(plugins) = &state.host {
        let sink_state = state.clone();
        plugins.set_page_sink(Some(Arc::new(move |payload: &str| sink_state.broadcast(payload))));
        if let Ok(has) = plugins.configure_server(port, host) {
            *state.middlewares.lock().unwrap() = has;
        }
    }
}

fn is_loopback_host(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    host == "localhost" || (host, 0).to_socket_addrs().is_ok_and(|mut a| a.all(|addr| addr.ip().is_loopback()))
}

/// Binds `host:port`. `localhost` is both loopback addresses (127.0.0.1 and ::1) on one port, since a client that names either one
/// must get an answer: the first address the system resolves `localhost` to is not the same everywhere, and `http://127.0.0.1:PORT`
/// would be refused where it is `::1`. A machine without IPv6 simply has the one.
fn bind_listeners(host: &str, port: u16) -> std::io::Result<Vec<TcpListener>> {
    if host != "localhost" {
        return Ok(vec![TcpListener::bind((host, port))?]);
    }
    let first = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        // No IPv4 loopback: the other one alone, or the original error.
        Err(e) => return TcpListener::bind(("::1", port)).map(|l| vec![l]).map_err(|_| e),
    };
    let mut listeners = vec![first];
    if let Ok(second) = TcpListener::bind(("::1", listeners[0].local_addr()?.port())) {
        listeners.push(second);
    }
    Ok(listeners)
}

fn bind_and_serve(state: Arc<ServeState>, bind_host: &str, public_host: &str, port: u16) -> std::io::Result<u16> {
    let listeners = bind_listeners(bind_host, port)?;
    let local = listeners[0].local_addr()?;
    let port = local.port();
    *state.exposed.lock().unwrap() = if bind_host == public_host { !local.ip().is_loopback() } else { !is_loopback_host(public_host) };
    *state.bound_host.lock().unwrap() = public_host.to_string();
    for listener in listeners {
        let state = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, &state);
                });
            }
        });
    }
    Ok(port)
}

const HTTPS: &str = include_str!("serve/https.mjs");

/// The process that speaks TLS for the dev server (see `serve/https.mjs`). Dropping it closes its standard input, which ends it.
pub struct HttpsProxy {
    _child: std::process::Child,
    _stdin: std::process::ChildStdin,
}

/// Starts the proxy on `host:port` (port 0 for any), passing requests to the dev server on the loopback at `upstream`. Returns it and
/// the port it listens on. `cert` and `key` are PEM files; with neither, a self-signed certificate for localhost is made and kept.
pub fn start_https(host: &str, port: u16, upstream: u16, cert: Option<&str>, key: Option<&str>, cwd: &Path) -> Result<(HttpsProxy, u16), String> {
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("rathkit-https-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos())));
    let file = dir.join("https.mjs");
    {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&file).and_then(|mut f| f.write_all(HTTPS.as_bytes())).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    let quote = crate::loader::quote;
    let names = if matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0" | "::" | "[::]" | "[::1]") { String::new() } else { quote(host) };
    let request = format!(
        "{{\"host\":{},\"port\":{port},\"upstream\":{upstream},\"cert\":{},\"key\":{},\"names\":[{names}]}}",
        quote(host),
        cert.map_or("null".to_string(), |c| quote(&cwd.join(c).to_string_lossy())),
        key.map_or("null".to_string(), |k| quote(&cwd.join(k).to_string_lossy())),
    );
    let spawned = Command::new(std::env::var_os("RATHKIT_NODE").unwrap_or_else(|| "node".into()))
        .arg("--no-warnings")
        .arg(&file)
        .arg(&request)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("Node.js could not be started ({e}); --https needs it"));
        }
    };
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let mut line = String::new();
    let read = child.stdout.take().map(|out| BufReader::new(out).read_line(&mut line));
    let _ = std::fs::remove_dir_all(&dir);
    if !matches!(read, Some(Ok(n)) if n > 0) {
        let _ = child.kill();
        return Err("the HTTPS proxy did not start (is the certificate readable?)".to_string());
    }
    match line.trim().strip_prefix("listening ").and_then(|p| p.parse::<u16>().ok()) {
        Some(public) => Ok((HttpsProxy { _child: child, _stdin: stdin }, public)),
        None => {
            let _ = child.kill();
            Err(format!("the HTTPS proxy said: {}", line.trim()))
        }
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Ok(None) };
    let (method, target) = (method.to_string(), target.to_string());
    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let length = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0).min(16 << 20);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(Request { method, target, headers, body }))
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (k, v) in headers {
        if !k.eq_ignore_ascii_case("content-length") && !k.eq_ignore_ascii_case("transfer-encoding") && !k.eq_ignore_ascii_case("connection") {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    head.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// A path below `base`, or none if it climbs out or names a hidden file.
fn safe_join(base: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path);
    let mut out = base.to_path_buf();
    for part in decoded.split('/').filter(|p| !p.is_empty()) {
        if part == ".." || part.starts_with('.') || part.contains('\\') || part.contains('\0') {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Keeps other web sites from using the server through the visitor's browser: the `Host` must be an address or a
/// local name (DNS rebinding comes with a foreign name), and a request that names an `Origin` must come from this server.
fn check_origin(request: &Request, state: &ServeState) -> Result<(), &'static str> {
    let header = |name: &str| request.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let host_of = |value: &str| -> String {
        let value = value.trim();
        match value.strip_prefix('[') {
            Some(rest) => format!("[{}", rest.split(']').next().unwrap_or("")) + "]",
            None => value.rsplit_once(':').map_or(value, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { value }).to_string(),
        }
    };
    if let Some(host) = header("host") {
        let name = host_of(host).to_ascii_lowercase();
        let bound = state.bound_host.lock().unwrap().to_ascii_lowercase();
        let is_address = name.parse::<std::net::IpAddr>().is_ok() || (name.starts_with('[') && name.ends_with(']'));
        let local = name == "localhost" || name.ends_with(".localhost");
        if !(is_address || local || name == bound) {
            return Err("Blocked: unexpected Host header");
        }
        if let Some(origin) = header("origin") {
            // A page of another dev server may read what this one built.
            if matches!(request.method.as_str(), "GET" | "HEAD" | "OPTIONS") && state.cors_origin(request).is_some() {
                return Ok(());
            }
            let origin_host = origin.split("://").nth(1).unwrap_or("");
            if origin != "null" && !origin_host.eq_ignore_ascii_case(host.trim()) {
                return Err("Blocked: cross-origin request");
            }
            if origin == "null" {
                return Err("Blocked: cross-origin request");
            }
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, state: &Arc<ServeState>) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream)? else { return Ok(()) };
    if let Err(reason) = check_origin(&request, state) {
        return respond(&mut stream, 403, &[], reason.as_bytes());
    }
    let (path, _query) = request.target.split_once('?').map_or((request.target.as_str(), ""), |(p, q)| (p, q));
    let cors = state.cors_origin(&request);
    let with_cors = |mut headers: Vec<(String, String)>| {
        if let Some(origin) = &cors {
            headers.push(("access-control-allow-origin".into(), origin.clone()));
            headers.push(("vary".into(), "origin".into()));
        }
        headers
    };
    if request.method == "OPTIONS" && cors.is_some() {
        return respond(&mut stream, 204, &with_cors(vec![("access-control-allow-methods".into(), "GET, HEAD, OPTIONS".into()), ("access-control-allow-headers".into(), "*".into()), ("access-control-max-age".into(), "600".into())]), b"");
    }
    match path {
        "/__rathkit/react-refresh.js" if state.refresh_runtime.lock().unwrap().is_some() => {
            let script = state.refresh_runtime.lock().unwrap().clone().unwrap_or_default();
            return respond(&mut stream, 200, &[("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), "no-store".into())], script.as_bytes());
        }
        "/__rathkit/client.js" => return respond(&mut stream, 200, &[("content-type".into(), "text/javascript; charset=utf-8".into()), ("cache-control".into(), "no-store".into())], CLIENT.as_bytes()),
        "/__rathkit/events" => {
            // A page that stops reading must not hold up every broadcast.
            stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
            let allow = match &cors {
                Some(origin) => format!("access-control-allow-origin: {origin}\r\nvary: origin\r\n"),
                None => String::new(),
            };
            stream.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n{allow}connection: keep-alive\r\n\r\n").as_bytes())?;
            let first = match state.last_error.lock().unwrap().clone() {
                Some(message) => format!("data: {{\"type\":\"connected\"}}\n\ndata: {{\"type\":\"error\",\"message\":{}}}\n\n", crate::loader::quote(&message)),
                None => "data: {\"type\":\"connected\"}\n\n".to_string(),
            };
            stream.write_all(first.as_bytes())?;
            stream.flush()?;
            state.clients.lock().unwrap().push(stream);
            return Ok(());
        }
        "/__rathkit/send" if request.method == "POST" => {
            if let Some(host) = &state.host {
                let _ = host.page_message(&String::from_utf8_lossy(&request.body));
            }
            return respond(&mut stream, 204, &[], b"");
        }
        _ => {}
    }
    // Plugins' middlewares come first, as in Vite.
    if *state.middlewares.lock().unwrap() {
        if let Some(host) = &state.host {
            if let Ok(Some((status, headers, body))) = host.serve_request(&request.method, &request.target, &request.headers, &request.body) {
                return respond(&mut stream, status, &headers, &body);
            }
        }
    }
    if request.method != "GET" && request.method != "HEAD" {
        return respond(&mut stream, 404, &[], b"Not found");
    }
    let name = path.trim_start_matches('/');
    // An output file.
    if let Some(bytes) = state.files.lock().unwrap().get(name).cloned() {
        return respond(&mut stream, 200, &with_cors(vec![("content-type".into(), content_type(name).into()), ("cache-control".into(), "no-store".into())]), &bytes);
    }
    // A page: the project's own, or one made for the entries.
    let is_page = name.is_empty() || name.ends_with('/') || name.ends_with(".html");
    if is_page {
        let file = if name.is_empty() || name.ends_with('/') { format!("{name}index.html") } else { name.to_string() };
        let html = match safe_join(&state.cwd, &file).and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => text,
            None if name.is_empty() => default_page(state),
            None => return respond(&mut stream, 404, &[], b"Not found"),
        };
        let html = match &state.host {
            Some(host) => host.transform_index_html(&html, path).unwrap_or(html),
            None => html,
        };
        let page = prepare_page(&html, state);
        return respond(&mut stream, 200, &[("content-type".into(), "text/html; charset=utf-8".into()), ("cache-control".into(), "no-store".into())], page.as_bytes());
    }
    // Files from `public/`, then from the project.
    let exposed = *state.exposed.lock().unwrap();
    let bases: Vec<PathBuf> = if exposed { vec![state.cwd.join("public")] } else { vec![state.cwd.join("public"), state.cwd.clone()] };
    for base in bases {
        if let Some(file) = safe_join(&base, path) {
            // Dependencies and the project's own configuration are not for pages.
            // Parts of the path, whatever the separator (a backslash on Windows).
            let relative = file.strip_prefix(&state.cwd).unwrap_or(&file).to_string_lossy().replace('\\', "/");
            let private = relative.split('/').any(|part| part == "node_modules") || relative.starts_with("rathkit.config.") || relative == "package.json" || relative == "Cargo.toml";
            if file.is_file() && !private {
                if let Ok(bytes) = std::fs::read(&file) {
                    return respond(&mut stream, 200, &[("content-type".into(), content_type(name).into()), ("cache-control".into(), "no-store".into())], &bytes);
                }
            }
        }
    }
    respond(&mut stream, 404, &[], b"Not found")
}

/// A page for entries that come with none: their script, their style sheets and the reload client.
fn default_page(state: &ServeState) -> String {
    let entries = state.entries.lock().unwrap().clone();
    let mut head = String::from("<meta charset=\"utf-8\">\n");
    for sheet in state.style_sheets() {
        head.push_str(&format!("<link rel=\"stylesheet\" href=\"/{sheet}\">\n"));
    }
    let mut body = String::from("<div id=\"app\"></div>\n<div id=\"root\"></div>\n");
    for (_, file) in &entries {
        if file.ends_with(".js") || file.ends_with(".mjs") {
            body.push_str(&format!("<script type=\"module\" src=\"/{file}\"></script>\n"));
        }
    }
    format!("<!doctype html>\n<html>\n<head>\n{head}</head>\n<body>\n{body}</body>\n</html>\n")
}

/// Points an html file's scripts at the built files, and adds the style sheets and the reload client.
fn prepare_page(html: &str, state: &ServeState) -> String {
    let entries = state.entries.lock().unwrap().clone();
    let mut out = html.to_string();
    // `<script type="module" src="/src/main.ts">` names an entry; the built file replaces it.
    for (entry, file) in &entries {
        // The project's folder and the entry may be written differently (Windows short names, a symlinked folder): compare the real ones.
        let real = |p: &std::path::Path| std::fs::canonicalize(p).map(|c| crate::resolve::without_verbatim_prefix(&c)).unwrap_or_else(|_| p.to_path_buf());
        let relative = entry.strip_prefix(&state.cwd).map(|r| r.to_path_buf()).or_else(|_| real(entry).strip_prefix(real(&state.cwd)).map(|r| r.to_path_buf())).unwrap_or_else(|_| entry.clone()).to_string_lossy().replace('\\', "/");
        let relative = relative.trim_start_matches("./").to_string();
        for needle in [format!("/{relative}"), format!("./{relative}"), relative.clone()] {
            for quote in ['"', '\''] {
                out = out.replace(&format!("src={quote}{needle}{quote}"), &format!("src={quote}/{file}{quote}"));
            }
        }
    }
    let mut extra = String::new();
    for sheet in state.style_sheets() {
        if !out.contains(&format!("/{sheet}")) {
            extra.push_str(&format!("<link rel=\"stylesheet\" href=\"/{sheet}\">\n"));
        }
    }
    if state.refresh_runtime.lock().unwrap().is_some() {
        extra.push_str("<script src=\"/__rathkit/react-refresh.js\"></script>\n");
    }
    extra.push_str("<script type=\"module\" src=\"/__rathkit/client.js\"></script>\n");
    match out.to_ascii_lowercase().find("</head>") {
        Some(at) => out.insert_str(at, &extra),
        None => out = format!("{extra}{out}"),
    }
    out
}
