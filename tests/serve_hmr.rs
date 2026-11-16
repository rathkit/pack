//! The early module update of the development server: what a page hears, and when the files wait for the build.
use rathkit_pack::bundle::HmrModule;
use rathkit_pack::serve::ServeState;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

fn module(id: &str, code: &str) -> HmrModule {
    HmrModule { id: id.to_string(), kind: "esm", code: code.to_string(), deps: Vec::new(), specs: Vec::new(), refresh: false }
}

fn listener(state: &ServeState) -> TcpStream {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(server.local_addr().unwrap()).unwrap();
    let (accepted, _) = server.accept().unwrap();
    state.add_listener(accepted);
    client
}

fn read_some(client: &mut TcpStream) -> String {
    client.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
    let mut buf = [0u8; 4096];
    match client.read(&mut buf) {
        Ok(n) => String::from_utf8_lossy(&buf[..n]).into_owned(),
        Err(_) => String::new(),
    }
}

#[test]
fn an_update_for_modules_goes_out_before_the_build_is_done_and_files_wait_for_it() {
    let state = ServeState::new(std::env::temp_dir(), None);
    // The first build has no update to send.
    state.publish_modules(&[module("/p/a.js", "1"), module("/p/b.js", "1")]);
    let mut page = listener(&state);
    state.rebuild_for(vec![PathBuf::from("/p/a.js")], false);
    state.modules_ready(&[module("/p/a.js", "2"), module("/p/b.js", "1")]);
    let said = read_some(&mut page);
    assert!(said.contains("\"type\":\"hmr\"") && said.contains("/p/a.js") && !said.contains("/p/b.js"), "{said}");
    assert!(state.modules_published_early());
    assert!(state.files_are_gated(), "files wait until the build has published them");
    state.publish(Vec::new(), Vec::new());
    assert!(!state.files_are_gated());
    assert!(state.take_early_sent());
    // Asked once.
    assert!(!state.take_early_sent());
}

#[test]
fn a_style_sheet_a_file_that_is_no_module_or_a_suppressed_change_is_left_to_the_end_of_the_build() {
    for (changed, suppressed) in [("/p/style.css", false), ("/p/other.txt", false), ("/p/a.js", true)] {
        let state = ServeState::new(std::env::temp_dir(), None);
        state.publish_modules(&[module("/p/a.js", "1"), module("/p/style.css", "1")]);
        let mut page = listener(&state);
        state.rebuild_for(vec![PathBuf::from(changed)], suppressed);
        state.modules_ready(&[module("/p/a.js", "2"), module("/p/style.css", "2")]);
        assert_eq!(read_some(&mut page), "", "{changed}");
        assert!(!state.files_are_gated());
        assert!(!state.take_early_sent());
    }
}

#[test]
fn a_build_nothing_asked_for_is_not_sent_early() {
    let state = ServeState::new(std::env::temp_dir(), None);
    state.publish_modules(&[module("/p/a.js", "1")]);
    state.modules_ready(&[module("/p/a.js", "2")]);
    assert!(!state.modules_published_early());
}
