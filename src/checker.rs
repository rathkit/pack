//! `rkp check`: finds the optional checker, `rkp-check`, and runs it, the way `cargo` finds `cargo-*` subcommands. The checker is a
//! separate program (the `check` crate of this workspace); none of its code is in `rkp`. This file is only the lookup and the message
//! for when it is not installed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The name of the checker's executable, without a platform suffix.
pub const BINARY: &str = "rkp-check";

fn file_name() -> String {
    if cfg!(windows) { format!("{BINARY}.exe") } else { BINARY.to_string() }
}

/// Where the checker is looked for, in order: the `node_modules/.bin` of the project (the directory the command runs in and each
/// parent), so a project pins its own checker version; the directory of `rkp` itself; then `PATH`.
pub fn search_dirs(exe_dir: Option<&Path>, cwd: Option<&Path>, path_var: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(cwd) = cwd {
        dirs.extend(cwd.ancestors().map(|d| d.join("node_modules").join(".bin")));
    }
    dirs.extend(exe_dir.map(Path::to_path_buf));
    if let Some(path) = path_var {
        dirs.extend(std::env::split_paths(path));
    }
    dirs
}

/// The first directory of [`search_dirs`] that holds the checker.
pub fn find_in(dirs: &[PathBuf]) -> Option<PathBuf> {
    let name = file_name();
    dirs.iter().map(|d| d.join(&name)).find(|p| p.is_file())
}

/// The checker for this process: its location, and where it was looked for.
pub fn find() -> (Option<PathBuf>, Vec<PathBuf>) {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();
    let path: Option<OsString> = std::env::var_os("PATH");
    let dirs = search_dirs(exe.as_deref().and_then(Path::parent), cwd.as_deref(), path.as_deref());
    (find_in(&dirs), dirs)
}

/// What `rkp check` prints when the checker is not installed.
pub fn not_installed_message() -> String {
    format!(
        "rkp check runs the optional checker, `{BINARY}`, and it is not installed: it was not found in a node_modules/.bin, next to rkp, or on PATH.\n\
         \n\
         Install it from source (it is not published to npm or crates.io yet):\n\
         \x20   cargo install --git https://github.com/rathkit/pack rathkit-check\n\
         and make sure cargo's bin directory is on PATH, or put `{BINARY}` next to the rkp executable.\n\
         \n\
         The checker is at stage 1: it reports a few scanner errors with TypeScript's codes and positions. It is not a type checker yet.\n\
         See docs/typechecker.md for what each stage reports and does not report."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rkp-checker-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn project_bin_comes_before_the_rkp_directory_and_path() {
        let root = temp("order");
        let project = root.join("app").join("src");
        let local = root.join("node_modules").join(".bin");
        let beside = root.join("beside");
        let on_path = root.join("onpath");
        for d in [&project, &local, &beside, &on_path] {
            std::fs::create_dir_all(d).unwrap();
        }
        let path_var = std::env::join_paths([&on_path]).unwrap();
        let dirs = search_dirs(Some(&beside), Some(&project), Some(&path_var));
        assert_eq!(find_in(&dirs), None);
        std::fs::write(on_path.join(file_name()), "").unwrap();
        assert_eq!(find_in(&dirs), Some(on_path.join(file_name())));
        std::fs::write(beside.join(file_name()), "").unwrap();
        assert_eq!(find_in(&dirs), Some(beside.join(file_name())));
        std::fs::write(local.join(file_name()), "").unwrap();
        assert_eq!(find_in(&dirs), Some(local.join(file_name())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_directory_named_like_the_checker_is_not_the_checker() {
        let root = temp("dir");
        std::fs::create_dir_all(root.join(file_name())).unwrap();
        assert_eq!(find_in(&[root.clone()]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_message_says_how_to_install_and_that_this_is_not_a_type_checker() {
        let m = not_installed_message();
        assert!(m.contains("cargo install --git https://github.com/rathkit/pack rathkit-check"));
        assert!(m.contains("not a type checker yet"));
        assert!(!m.contains("npm install"));
    }
}
