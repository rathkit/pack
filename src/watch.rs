//! File-system events for watch mode.
//!
//! On Linux the kernel's inotify is used, called through the C library every program already links (the same way
//! `signal` is in `main.rs`; no crate is needed). It watches the folders that hold the files a build read, so an
//! editor that saves by writing a new file and renaming it over the old one is seen too. Everywhere else, and
//! whenever inotify cannot be used (no instance left, no watch left), `Notifier::new` and `watch` say so and the
//! caller keeps polling modification times, which is what watch mode did before.
//!
//! macOS (FSEvents, kqueue) and Windows (`ReadDirectoryChangesW`) are not used: each needs a runtime of its own
//! (a run loop and callbacks, or overlapped I/O) that cannot be had without a dependency or a lot of unsafe code
//! that cannot be tested here, so they poll.

use std::path::PathBuf;

/// What a wait ended with.
#[derive(Debug, PartialEq, Eq)]
pub enum Wake {
    /// Nothing happened before the time ran out.
    Timeout,
    /// Paths that something happened to (a file written, created, renamed, removed; a folder's contents changed).
    Paths(Vec<PathBuf>),
    /// The kernel dropped events: every file has to be looked at.
    Overflow,
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Wake;
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn inotify_init1(flags: i32) -> i32;
        fn inotify_add_watch(fd: i32, path: *const std::ffi::c_char, mask: u32) -> i32;
        fn read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
        fn poll(fds: *mut PollFd, nfds: std::ffi::c_ulong, timeout: i32) -> i32;
        fn close(fd: i32) -> i32;
    }

    const IN_NONBLOCK: i32 = 0o4000;
    const IN_CLOEXEC: i32 = 0o2000000;
    const IN_MODIFY: u32 = 0x2;
    const IN_ATTRIB: u32 = 0x4;
    const IN_CLOSE_WRITE: u32 = 0x8;
    const IN_MOVED_FROM: u32 = 0x40;
    const IN_MOVED_TO: u32 = 0x80;
    const IN_CREATE: u32 = 0x100;
    const IN_DELETE: u32 = 0x200;
    const IN_DELETE_SELF: u32 = 0x400;
    const IN_MOVE_SELF: u32 = 0x800;
    const IN_Q_OVERFLOW: u32 = 0x4000;
    const IN_IGNORED: u32 = 0x8000;
    const IN_ONLYDIR: u32 = 0x0100_0000;
    const MASK: u32 = IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE | IN_MOVED_FROM | IN_MOVED_TO | IN_CREATE | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF | IN_ONLYDIR;
    const POLLIN: i16 = 1;

    pub struct Notifier {
        fd: i32,
        /// The folder each watch descriptor stands for.
        dirs: HashMap<i32, PathBuf>,
        by_dir: HashMap<PathBuf, i32>,
        buf: Vec<u8>,
    }

    impl Notifier {
        pub fn new() -> Option<Notifier> {
            let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
            (fd >= 0).then(|| Notifier { fd, dirs: HashMap::new(), by_dir: HashMap::new(), buf: vec![0; 64 * 1024] })
        }

        /// Watches the folder of each path (and the path itself when it is a folder). Returns how many folders are new, or
        /// `None` when a watch could not be made (the limit on watches, say): then events cannot be trusted to cover everything.
        pub fn watch<'a>(&mut self, paths: impl Iterator<Item = &'a PathBuf>) -> Option<usize> {
            let mut added = 0;
            for path in paths {
                let mut folders: Vec<&Path> = Vec::with_capacity(2);
                if path.is_dir() {
                    folders.push(path);
                }
                if let Some(parent) = path.parent() {
                    folders.push(parent);
                }
                for folder in folders {
                    if self.by_dir.contains_key(folder) {
                        continue;
                    }
                    let c = CString::new(folder.as_os_str().as_bytes()).ok()?;
                    let wd = unsafe { inotify_add_watch(self.fd, c.as_ptr(), MASK) };
                    if wd < 0 {
                        // A folder that is gone was not going to be a source of events; anything else (the limit) is not safe.
                        if folder.exists() {
                            return None;
                        }
                        continue;
                    }
                    // A folder that was moved and made again may get the number back; the newest name wins.
                    if let Some(old) = self.dirs.insert(wd, folder.to_path_buf()) {
                        self.by_dir.remove(&old);
                    }
                    self.by_dir.insert(folder.to_path_buf(), wd);
                    added += 1;
                }
            }
            Some(added)
        }

        /// Waits up to `timeout_ms` for something to happen.
        pub fn wait(&mut self, timeout_ms: i32) -> Wake {
            let mut fds = PollFd { fd: self.fd, events: POLLIN, revents: 0 };
            let ready = unsafe { poll(&mut fds, 1, timeout_ms) };
            if ready <= 0 {
                // Timed out, or a signal (Ctrl-C) cut the wait short: the caller looks at its own flag.
                return Wake::Timeout;
            }
            let mut paths = Vec::new();
            loop {
                let n = unsafe { read(self.fd, self.buf.as_mut_ptr().cast(), self.buf.len()) };
                if n <= 0 {
                    break;
                }
                let mut at = 0usize;
                let n = n as usize;
                while at + 16 <= n {
                    let wd = i32::from_ne_bytes(self.buf[at..at + 4].try_into().unwrap());
                    let mask = u32::from_ne_bytes(self.buf[at + 4..at + 8].try_into().unwrap());
                    let len = u32::from_ne_bytes(self.buf[at + 12..at + 16].try_into().unwrap()) as usize;
                    let name_end = (at + 16 + len).min(n);
                    let raw = &self.buf[at + 16..name_end];
                    at = name_end;
                    if mask & IN_Q_OVERFLOW != 0 {
                        return Wake::Overflow;
                    }
                    let Some(dir) = self.dirs.get(&wd) else { continue };
                    if mask & IN_IGNORED != 0 {
                        // The folder is gone and so is the watch.
                        let dir = dir.clone();
                        self.dirs.remove(&wd);
                        self.by_dir.remove(&dir);
                        paths.push(dir);
                        continue;
                    }
                    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
                    if end == 0 {
                        // The folder itself (moved or removed).
                        paths.push(dir.clone());
                    } else {
                        paths.push(dir.join(std::ffi::OsString::from_vec(raw[..end].to_vec())));
                    }
                }
            }
            if paths.is_empty() { Wake::Timeout } else { Wake::Paths(paths) }
        }
    }

    impl Drop for Notifier {
        fn drop(&mut self) {
            unsafe { close(self.fd) };
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::Wake;
    use std::path::PathBuf;

    pub struct Notifier;

    impl Notifier {
        pub fn new() -> Option<Notifier> {
            None
        }
        pub fn watch<'a>(&mut self, _paths: impl Iterator<Item = &'a PathBuf>) -> Option<usize> {
            None
        }
        pub fn wait(&mut self, _timeout_ms: i32) -> Wake {
            Wake::Timeout
        }
    }
}

pub use imp::Notifier;

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rkp-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn collect(n: &mut Notifier) -> Vec<PathBuf> {
        let mut all = Vec::new();
        while let Wake::Paths(p) = n.wait(300) {
            all.extend(p);
        }
        all
    }

    #[test]
    fn a_write_a_rename_and_a_delete_are_seen_without_waiting_for_a_tick() {
        let dir = temp("events");
        let a = dir.join("a.js");
        std::fs::write(&a, "1").unwrap();
        let mut n = Notifier::new().expect("inotify");
        assert_eq!(n.watch([a.clone()].iter()), Some(1));
        assert_eq!(n.wait(20), Wake::Timeout);

        let t = std::time::Instant::now();
        std::fs::write(&a, "22").unwrap();
        let Wake::Paths(p) = n.wait(2000) else { panic!("no event") };
        assert!(t.elapsed().as_millis() < 100, "took {:?}", t.elapsed());
        assert!(p.contains(&a));
        let _ = collect(&mut n);

        // An editor's save: a new file renamed over the old one.
        let tmp = dir.join("a.js.tmp");
        std::fs::write(&tmp, "333").unwrap();
        let _ = collect(&mut n);
        std::fs::rename(&tmp, &a).unwrap();
        assert!(collect(&mut n).contains(&a));

        std::fs::remove_file(&a).unwrap();
        assert!(collect(&mut n).contains(&a));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_is_watched_reports_new_names() {
        let dir = temp("glob");
        let mut n = Notifier::new().expect("inotify");
        n.watch([dir.clone()].iter()).unwrap();
        let _ = collect(&mut n);
        std::fs::write(dir.join("new.js"), "x").unwrap();
        assert!(collect(&mut n).contains(&dir.join("new.js")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watching_the_same_folder_twice_adds_it_once() {
        let dir = temp("twice");
        let (a, b) = (dir.join("a.js"), dir.join("b.js"));
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let mut n = Notifier::new().expect("inotify");
        assert_eq!(n.watch([a.clone(), b.clone()].iter()), Some(1));
        assert_eq!(n.watch([a, b].iter()), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
