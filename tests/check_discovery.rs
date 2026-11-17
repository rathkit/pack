//! `rkp check` finds the optional checker or says how to install it (src/checker.rs).
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rkp-check-discovery-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A copy of rkp in its own directory: the lookup also tries the directory of the executable, where a `cargo build` of the whole workspace
/// may have left a real checker beside the one under test.
fn lonely_rkp(dir: &std::path::Path) -> PathBuf {
    let copy = dir.join("rkp-copy").join(if cfg!(windows) { "rkp.exe" } else { "rkp" });
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_rkp"), &copy).unwrap();
    copy
}

/// Runs the command, and again after a moment while the system says the file is busy ("Text file busy"): the tests run on several threads, and
/// another thread may fork while this one still has the copy it just wrote open, which keeps the kernel from running it for a few milliseconds.
fn output(command: &mut Command) -> std::process::Output {
    for _ in 0..100 {
        match command.output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(20)),
            other => return other.unwrap(),
        }
    }
    command.output().unwrap()
}

#[test]
fn without_the_checker_it_says_how_to_install_it() {
    let empty = scratch("none");
    let out = output(Command::new(lonely_rkp(&empty)).args(["check", "a.ts"]).current_dir(&empty).env("PATH", &empty));
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not installed"), "{err}");
    assert!(err.contains("cargo install --git https://github.com/rathkit/pack rathkit-check"), "{err}");
    assert!(out.stdout.is_empty());
    let _ = std::fs::remove_dir_all(&empty);
}

#[cfg(unix)]
#[test]
fn with_a_checker_on_path_it_runs_it_with_the_arguments_and_returns_its_exit_code() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("fake");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("rkp-check");
    std::fs::write(&script, "#!/bin/sh\necho \"checker got: $*\"\nexit 3\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = output(Command::new(lonely_rkp(&dir)).args(["check", "--json", "a.ts"]).current_dir(&dir).env("PATH", &bin));
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "checker got: --json a.ts");
    let _ = std::fs::remove_dir_all(&dir);
}
