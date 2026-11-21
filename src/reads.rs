//! `--depfile=FILE`: every file a build looked at, including the ones that were not there. A tool that decides whether a build can be
//! skipped needs both: a file that was read changing is a new build, and so is a file that was looked for and has appeared since (a
//! `.env.local`, an `a.ts` beside the `a.js` an import found, a `package.json` nearer than the one used).
//!
//! The list is kept only when asked for. The places that read project files go through [`read_to_string`], [`is_file`], [`is_dir`] and
//! [`note`], which cost one atomic load when nothing is recording. What a Node process does for plugins (the files a config or a plugin
//! imports, a PostCSS or Sass run) is not seen; the config file itself is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// Each path with whether it was looked at as a folder (a path compares equal with and without a trailing slash, so the slash is not
/// enough to keep `a` the file and `a/` the folder apart), and whether it was there.
static SEEN: Mutex<BTreeMap<(PathBuf, bool), bool>> = Mutex::new(BTreeMap::new());

/// Starts keeping the list.
pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

/// Starts a new list (for the next build of a watching run).
pub fn reset() {
    SEEN.lock().unwrap().clear();
}

/// Notes that `path` was looked at, and whether it was there. A path seen both ways (a file made while the build ran) counts as read.
pub fn note(path: &Path, found: bool) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let folder = path.to_string_lossy().ends_with('/');
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    *SEEN.lock().unwrap().entry((path, folder)).or_insert(false) |= found;
}

pub fn read_to_string(path: &Path) -> std::io::Result<String> {
    let result = std::fs::read_to_string(path);
    note(path, result.is_ok());
    result
}

pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    let result = std::fs::read(path);
    note(path, result.is_ok());
    result
}

pub fn is_file(path: &Path) -> bool {
    let found = path.is_file();
    note(path, found);
    found
}

pub fn is_dir(path: &Path) -> bool {
    let found = path.is_dir();
    if ON.load(Ordering::Relaxed) {
        // A folder is told from a file by the trailing slash.
        let mut folder = path.as_os_str().to_os_string();
        folder.push("/");
        note(Path::new(&folder), found);
    }
    found
}

/// What was looked at: the files that were there, and the ones that were not (each as an absolute path, in order).
pub fn all() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let seen = SEEN.lock().unwrap();
    // A folder comes back with its trailing slash.
    let named = |(path, folder): &(PathBuf, bool)| {
        if *folder {
            let mut text = path.as_os_str().to_os_string();
            text.push("/");
            PathBuf::from(text)
        } else {
            path.clone()
        }
    };
    let read = seen.iter().filter(|(_, f)| **f).map(|(k, _)| named(k)).collect();
    let missing = seen.iter().filter(|(_, f)| !**f).map(|(k, _)| named(k)).collect();
    (read, missing)
}
