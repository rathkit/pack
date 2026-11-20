//! The signals that tell a tool that `rkp serve` and `rkp bundle --watch` are up: the line the server prints, `--ready-file`, and `--port 0`.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rathkit_pack::json::Json;

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rkp-ready-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("index.html"), "<!doctype html><html><body><script type=\"module\" src=\"/src/main.js\"></script></body></html>\n").unwrap();
    std::fs::write(dir.join("src/main.js"), "console.log('one');\n").unwrap();
    dir
}

fn start(dir: &std::path::Path, args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_rkp")).current_dir(dir).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
}

/// Waits for `file` to exist and returns what it holds; the file is written whole (renamed into place), so what is read is complete.
fn wait_for(file: &std::path::Path, child: &mut Child) -> Json {
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(file) {
            return Json::parse(&text).unwrap_or_else(|e| panic!("{text:?}: {}", e.message));
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("rkp ended ({status}) before it was ready");
        }
        assert!(started.elapsed() < Duration::from_secs(60), "not ready in 60 seconds");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn get(port: u64, path: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port as u16)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost:{port}\r\n\r\n").unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}

#[test]
fn the_server_says_where_it_listens_when_the_port_is_zero_and_when_it_is_ready() {
    let dir = project("serve");
    for port_args in [vec!["--port=0"], vec!["--port", "0"]] {
        let _ = std::fs::remove_file(dir.join("ready.json"));
        let mut args = vec!["serve", "index.html", "--ready-file=ready.json"];
        args.extend(port_args.clone());
        let mut child = start(&dir, &args);
        let ready = wait_for(&dir.join("ready.json"), &mut child);
        assert_eq!(ready.get("event").and_then(Json::as_str), Some("ready"));
        assert_eq!(ready.get("ok").and_then(Json::as_bool), Some(true));
        let port = ready.get("port").and_then(Json::as_f64).unwrap() as u64;
        assert!(port > 0, "{port_args:?}");
        let url = ready.get("url").and_then(Json::as_str).unwrap();
        assert_eq!(url, format!("http://localhost:{port}/"));
        // The line on the standard output says the same, and the server answers on that port with the first build in memory.
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), format!("rkp serving at {url}"));
        let page = get(port, "/");
        assert!(page.starts_with("HTTP/1.0 200") || page.starts_with("HTTP/1.1 200"), "{page}");
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_watching_build_is_ready_after_its_first_build_and_builds_again_on_a_change() {
    let dir = project("watch");
    let mut child = start(&dir, &["bundle", "src/main.js", "--outdir", "out", "--watch", "--ready-file=ready.json"]);
    let ready = wait_for(&dir.join("ready.json"), &mut child);
    assert_eq!(ready.get("event").and_then(Json::as_str), Some("ready"));
    assert!(ready.get("url").is_none() && ready.get("watching").and_then(Json::as_f64).is_some_and(|n| n >= 1.0));
    // The first build is done when the signal comes.
    assert!(std::fs::read_to_string(dir.join("out/bundle.js")).unwrap().contains("one"));
    std::fs::write(dir.join("src/main.js"), "console.log('two');\n").unwrap();
    let started = Instant::now();
    while !std::fs::read_to_string(dir.join("out/bundle.js")).is_ok_and(|t| t.contains("two")) {
        assert!(started.elapsed() < Duration::from_secs(30), "no second build");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ready_file_left_by_an_earlier_run_is_replaced_and_a_failed_first_build_is_told() {
    let dir = project("stale");
    std::fs::write(dir.join("ready.json"), "{\"event\":\"ready\",\"stale\":true}\n").unwrap();
    std::fs::write(dir.join("src/main.js"), "this is not valid {{{\n").unwrap();
    let mut child = start(&dir, &["bundle", "src/main.js", "--outdir", "out", "--watch", "--ready-file=ready.json"]);
    // The old file goes at the start and the new one comes when the first build is over: what is read in between is neither.
    let started = Instant::now();
    let ready = loop {
        let ready = wait_for(&dir.join("ready.json"), &mut child);
        if ready.get("stale").is_none() {
            break ready;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "the old file was never replaced");
        std::thread::sleep(Duration::from_millis(20));
    };
    // A first build that failed still has a watcher up, waiting for the fix, and says so.
    assert_eq!(ready.get("ok").and_then(Json::as_bool), Some(false));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
