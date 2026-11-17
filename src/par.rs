//! Running independent work on several threads, with the results in the order of the work.
//!
//! The threads are started for the call and finish with it. Starting a set of them costs about a fifth of a millisecond, so a
//! caller with little to do should not use this: see `worth_it`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Threads to use: one per core the process may run on.
pub fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Whether `bytes` of work in `items` pieces is enough to pay for starting threads.
pub fn worth_it(items: usize, bytes: usize) -> bool {
    items >= 4 && bytes >= 48 * 1024 && workers() > 1
}

/// `f(0)`, `f(1)`, ... `f(n - 1)`, each taken by the first free thread (the caller's thread is one of them), results in order.
pub fn map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = workers().min(n);
    if threads < 2 {
        return (0..n).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    let work = || {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n {
                break;
            }
            let result = f(i);
            done.lock().expect("results")[i] = Some(result);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads {
            scope.spawn(work);
        }
        work();
    });
    done.into_inner().expect("results").into_iter().map(|r| r.expect("every item ran")).collect()
}

/// For `RATHKIT_TIMING`: the resident size now and at its peak (Linux; empty elsewhere).
pub fn memory_note() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| status.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok());
    match (field("VmRSS:"), field("VmHWM:")) {
        (Some(now), Some(peak)) => format!("  [rss {} MB, peak {} MB]", now / 1024, peak / 1024),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_come_back_in_order() {
        let out = map(1000, |i| i * 3);
        assert_eq!(out, (0..1000).map(|i| i * 3).collect::<Vec<_>>());
    }

    #[test]
    fn every_item_runs_once() {
        let count = AtomicUsize::new(0);
        let out = map(257, |i| {
            count.fetch_add(1, Ordering::Relaxed);
            i
        });
        assert_eq!(count.load(Ordering::Relaxed), 257);
        assert_eq!(out.len(), 257);
    }

    #[test]
    fn nothing_and_one_item() {
        assert!(map(0, |i| i).is_empty());
        assert_eq!(map(1, |i| i + 7), vec![7]);
    }

    #[test]
    fn small_work_is_not_worth_threads() {
        assert!(!worth_it(2, 10 << 20));
        assert!(!worth_it(100, 1024));
    }
}
