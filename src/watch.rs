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

/// Ends the wait of a `Notifier` that is in `wait` (or the next one), from a signal handler or another thread: the wait
/// returns `Wake::Timeout` at once, so a caller that waits long still sees its own flag. Safe to call from a signal handler.
pub fn interrupt() {
    imp::interrupt();
}

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

    unsafe extern "C" {
        fn pipe2(fds: *mut i32, flags: i32) -> i32;
        fn write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
    }

    /// The write end of a pipe the wait also listens to (`interrupt`), or -1.
    static WAKE_WRITE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
    static WAKE_READ: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

    pub fn interrupt() {
        let fd = WAKE_WRITE.load(std::sync::atomic::Ordering::Relaxed);
        if fd >= 0 {
            // A full pipe already holds a wake-up.
            unsafe { write(fd, b"x".as_ptr().cast(), 1) };
        }
    }

    /// Makes the pipe the first time it is asked for.
    fn wake_pipe() -> i32 {
        use std::sync::atomic::Ordering::SeqCst;
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let mut fds = [-1i32; 2];
            if unsafe { pipe2(fds.as_mut_ptr(), IN_NONBLOCK | IN_CLOEXEC) } == 0 {
                WAKE_READ.store(fds[0], SeqCst);
                WAKE_WRITE.store(fds[1], SeqCst);
            }
        });
        WAKE_READ.load(SeqCst)
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
    const IN_ISDIR: u32 = 0x4000_0000;
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
        /// Files that were written to and not yet closed (or renamed, or removed): an editor that is still saving.
        open: std::collections::HashSet<PathBuf>,
    }

    impl Notifier {
        pub fn new() -> Option<Notifier> {
            let fd = unsafe { inotify_init1(IN_NONBLOCK | IN_CLOEXEC) };
            (fd >= 0).then(|| Notifier { fd, dirs: HashMap::new(), by_dir: HashMap::new(), buf: vec![0; 64 * 1024], open: std::collections::HashSet::new() })
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

        /// Whether every file seen written to has been closed since: the writers are done, so there is nothing to wait for.
        pub fn settled(&self) -> bool {
            self.open.is_empty()
        }

        /// As `settled`, for the files `watched` says matter: a log or a swap file that something keeps open in the same folder
        /// does not hold anything up.
        pub fn settled_among(&self, watched: impl Fn(&Path) -> bool) -> bool {
            !self.open.iter().any(|p| watched(p))
        }

        /// Waits up to `timeout_ms` for something to happen.
        pub fn wait(&mut self, timeout_ms: i32) -> Wake {
            let wake_fd = wake_pipe();
            let mut fds = [PollFd { fd: self.fd, events: POLLIN, revents: 0 }, PollFd { fd: wake_fd, events: POLLIN, revents: 0 }];
            let ready = unsafe { poll(fds.as_mut_ptr(), if wake_fd >= 0 { 2 } else { 1 }, timeout_ms) };
            if wake_fd >= 0 && fds[1].revents != 0 {
                let mut sink = [0u8; 64];
                while unsafe { read(wake_fd, sink.as_mut_ptr().cast(), sink.len()) } > 0 {}
            }
            if ready <= 0 || fds[0].revents == 0 {
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
                        self.open.clear();
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
                        let path = dir.join(std::ffi::OsString::from_vec(raw[..end].to_vec()));
                        if mask & IN_ISDIR == 0 {
                            if mask & IN_MODIFY != 0 {
                                self.open.insert(path.clone());
                            } else if mask & (IN_CLOSE_WRITE | IN_MOVED_FROM | IN_MOVED_TO | IN_DELETE) != 0 {
                                self.open.remove(&path);
                            }
                        }
                        paths.push(path);
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

    pub fn interrupt() {}

    impl Notifier {
        pub fn settled(&self) -> bool {
            true
        }
        pub fn settled_among(&self, _watched: impl Fn(&std::path::Path) -> bool) -> bool {
            true
        }
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

    #[test]
    fn a_file_is_settled_when_its_writer_has_closed_it_and_not_before() {
        let dir = temp("settled");
        let a = dir.join("a.js");
        std::fs::write(&a, "1").unwrap();
        let mut n = Notifier::new().expect("inotify");
        n.watch([a.clone()].iter()).unwrap();
        let _ = collect(&mut n);
        assert!(n.settled());
        let mut file = std::fs::File::create(&a).unwrap();
        std::io::Write::write_all(&mut file, b"22").unwrap();
        let _ = n.wait(500);
        assert!(!n.settled(), "still open for writing");
        assert!(!n.settled_among(|p| p == a));
        assert!(n.settled_among(|p| p != a), "a file nobody asks about does not matter");
        drop(file);
        let _ = n.wait(500);
        assert!(n.settled());
        assert!(n.settled_among(|_| true));
        // A folder made is not a file that gets closed.
        std::fs::create_dir(dir.join("sub")).unwrap();
        let _ = collect(&mut n);
        assert!(n.settled());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_signal_handler_can_end_a_long_wait() {
        let mut n = Notifier::new().expect("inotify");
        let t = std::time::Instant::now();
        // The wake-up pipe is the process's, so another test's wait may take one byte: it is written until the wait is over.
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let writer = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(20));
                interrupt();
            }
        });
        assert_eq!(n.wait(5000), Wake::Timeout);
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        writer.join().unwrap();
        assert!(t.elapsed().as_millis() < 1000, "took {:?}", t.elapsed());
    }
}
