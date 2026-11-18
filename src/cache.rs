//! The persistent cache: what the minifier made of a chunk, kept in a folder between runs.
//!
//! The minifier is a function of the chunk's text and layout, its settings and the program that runs it, so a chunk it has seen
//! before is not minified again. An entry holds everything the answer depends on (the program, the settings, the layout, the
//! chunk's text) next to the answer, and a hit needs all of it to match byte for byte: the name of the file is only where to
//! look, so no collision can give a wrong answer. A file that is missing, cut short, damaged or made by another program is
//! ignored and written again; nothing here can fail a build.
//!
//! Where: `RATHKIT_CACHE_DIR`, else `$XDG_CACHE_HOME/rathkit/minify`, else `~/.cache/rathkit/minify` (`%LOCALAPPDATA%\rathkit\minify`
//! on Windows). It is off unless asked, because it writes outside the project: `RATHKIT_CACHE=on` (or `1`, `true`, `yes`) turns it on.
//! The folder is kept under a size limit, oldest entries first.

use crate::minify::{Anchor, Layout, Options};
use std::path::PathBuf;

const MAGIC: &[u8; 8] = b"RKPCACH1";
/// The folder is trimmed to this many bytes when an entry is written.
const LIMIT_BYTES: u64 = 512 << 20;
/// Chunks smaller than this are quicker to minify than to look up.
const MIN_SOURCE: usize = 16 * 1024;

fn enabled() -> bool {
    matches!(std::env::var("RATHKIT_CACHE").as_deref(), Ok("on" | "1" | "true" | "yes"))
}

/// The cache folder, if there is one to use.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("RATHKIT_CACHE_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let base = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| {
        if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("HOME").filter(|d| !d.is_empty()).map(|h| PathBuf::from(h).join(".cache"))
        }
    })?;
    Some(base.join("rathkit").join("minify"))
}

/// What identifies the program that made an entry: its version, and its size and time of change (a rebuilt program is another one).
fn program() -> String {
    let stamp = std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| {
        let nanos = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        format!("{}:{nanos}", m.len())
    });
    format!("{} {}", env!("CARGO_PKG_VERSION"), stamp.unwrap_or_default())
}

fn fnv(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^ (h >> 33)
}

/// Everything but the text that the answer depends on.
fn context(layout: &Layout, options: &Options, with_anchors: bool) -> String {
    let mode = std::env::var("RATHKIT_FRAGMENTS").unwrap_or_default();
    format!("{} {options:?} {layout:?} anchors={with_anchors} fragments={mode}", program())
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn number(&mut self) -> Option<usize> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?) as usize)
    }
}

fn decode(file: &[u8], context: &str, source: &str, with_anchors: bool) -> Option<(String, Vec<Anchor>)> {
    let (body, sum) = file.split_at(file.len().checked_sub(8)?);
    if u64::from_le_bytes(sum.try_into().ok()?) != fnv(body) {
        return None;
    }
    let mut r = Reader(body);
    if r.take(8)? != MAGIC {
        return None;
    }
    let n = r.number()?;
    if r.take(n)? != context.as_bytes() {
        return None;
    }
    let n = r.number()?;
    if r.take(n)? != source.as_bytes() {
        return None;
    }
    let n = r.number()?;
    let out = String::from_utf8(r.take(n)?.to_vec()).ok()?;
    let count = r.number()?;
    if !with_anchors && count != 0 {
        return None;
    }
    let bytes = r.take(count.checked_mul(16)?)?;
    if !r.0.is_empty() {
        return None;
    }
    let word = |c: &[u8]| u32::from_le_bytes(c.try_into().expect("four bytes"));
    let anchors = bytes.chunks_exact(16).map(|c| Anchor { out: word(&c[0..4]), out_len: word(&c[4..8]), inp: word(&c[8..12]), in_len: word(&c[12..16]) }).collect();
    Some((out, anchors))
}

fn encode(context: &str, source: &str, out: &str, anchors: &[Anchor]) -> Vec<u8> {
    let mut b = Vec::with_capacity(source.len() + out.len() + context.len() + anchors.len() * 16 + 64);
    b.extend_from_slice(MAGIC);
    for part in [context.as_bytes(), source.as_bytes(), out.as_bytes()] {
        b.extend_from_slice(&(part.len() as u64).to_le_bytes());
        b.extend_from_slice(part);
    }
    b.extend_from_slice(&(anchors.len() as u64).to_le_bytes());
    for a in anchors {
        for v in [a.out, a.out_len, a.inp, a.in_len] {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    let sum = fnv(&b);
    b.extend_from_slice(&sum.to_le_bytes());
    b
}

/// Where the entry for this chunk is, in `dir`.
fn entry_path(dir: &std::path::Path, context: &str, source: &str) -> PathBuf {
    let key = format!("{:016x}{:016x}", fnv(context.as_bytes()), fnv(source.as_bytes()) ^ (source.len() as u64).rotate_left(32));
    dir.join(format!("{key}.min"))
}

/// Writes the entry whole (a file under another name, then renamed, so a reader sees all of it or none), then trims the folder.
fn store(dir: &std::path::Path, path: &std::path::Path, bytes: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let temp = dir.join(format!(".{}.{}.tmp", std::process::id(), fnv(path.as_os_str().as_encoded_bytes())));
    if std::fs::write(&temp, bytes).is_err() || std::fs::rename(&temp, path).is_err() {
        let _ = std::fs::remove_file(&temp);
        return;
    }
    trim(dir, LIMIT_BYTES);
}

/// Removes the oldest entries until the folder is under `limit` bytes.
fn trim(dir: &std::path::Path, limit: u64) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = read
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "min"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    if total <= limit {
        return;
    }
    entries.sort();
    for (_, size, path) in entries {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// `minify::minify_chunk`, with the answer kept between runs. The text is the same as without the cache.
pub fn minify_chunk(source: &str, layout: &Layout, options: &Options, anchors: Option<&mut Vec<Anchor>>) -> Result<String, String> {
    let dir = if enabled() && options.any() && source.len() >= MIN_SOURCE { dir() } else { None };
    let Some(dir) = dir else {
        return crate::minify::minify_chunk(source, layout, options, anchors);
    };
    minify_chunk_in(&dir, source, layout, options, anchors)
}

fn minify_chunk_in(dir: &std::path::Path, source: &str, layout: &Layout, options: &Options, anchors: Option<&mut Vec<Anchor>>) -> Result<String, String> {
    let context = context(layout, options, anchors.is_some());
    let path = entry_path(dir, &context, source);
    if let Some((out, found)) = std::fs::read(&path).ok().and_then(|file| decode(&file, &context, source, anchors.is_some())) {
        if let Some(list) = anchors {
            *list = found;
        }
        return Ok(out);
    }
    match anchors {
        Some(list) => {
            let out = crate::minify::minify_chunk(source, layout, options, Some(list))?;
            store(dir, &path, &encode(&context, source, &out, list));
            Ok(out)
        }
        None => {
            let out = crate::minify::minify_chunk(source, layout, options, None)?;
            store(dir, &path, &encode(&context, source, &out, &[]));
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rkp-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn program_text() -> String {
        let mut s = String::new();
        for i in 0..800 {
            s.push_str(&format!("function helper{i}(argument) {{ const doubled = argument * 2; return doubled + {i}; }}\nglobalThis.out{i} = helper{i}({i});\n"));
        }
        s
    }

    const ALL: Options = Options { whitespace: true, identifiers: true, syntax: true, keep_top_level: false, keep_names: None, legacy: false };

    #[test]
    fn a_hit_is_what_a_cold_run_makes() {
        let dir = temp("hit");
        let text = program_text();
        assert!(text.len() >= MIN_SOURCE);
        let layout = Layout::default();
        let cold = crate::minify::minify_chunk(&text, &layout, &ALL, None).unwrap();
        let first = minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap();
        assert_eq!(first, cold);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let second = minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap();
        assert_eq!(second, cold);
        // With anchors, the same answers come back, from an entry of their own.
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let miss = minify_chunk_in(&dir, &text, &layout, &ALL, Some(&mut a)).unwrap();
        let hit = minify_chunk_in(&dir, &text, &layout, &ALL, Some(&mut b)).unwrap();
        assert_eq!((miss, hit.clone()), (cold.clone(), cold));
        assert!(!a.is_empty() && a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (x.out, x.out_len, x.inp, x.in_len) == (y.out, y.out_len, y.inp, y.in_len)));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_or_stale_entry_is_ignored_and_replaced() {
        let dir = temp("bad");
        let text = program_text();
        let layout = Layout::default();
        let cold = crate::minify::minify_chunk(&text, &layout, &ALL, None).unwrap();
        minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap();
        let entry = std::fs::read_dir(&dir).unwrap().flatten().next().unwrap().path();
        let good = std::fs::read(&entry).unwrap();
        let mut flipped = good.clone();
        let mid = flipped.len() / 2;
        flipped[mid] ^= 0x55;
        let wrong_text = encode(&context(&layout, &ALL, false), "other text", "x", &[]);
        let other_program = encode("0.0.0 another program", &text, "x", &[]);
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("garbage", b"not a cache entry".to_vec()),
            ("cut short", good[..good.len() / 2].to_vec()),
            ("bit flip", flipped),
            ("another text", wrong_text),
            ("another program", other_program),
        ];
        for (what, bytes) in cases {
            std::fs::write(&entry, bytes).unwrap();
            let out = minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap();
            assert_eq!(out, cold, "{what}");
            assert_eq!(std::fs::read(&entry).unwrap(), good, "{what}: the entry is written again");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_text_choose_the_entry() {
        let dir = temp("keys");
        let text = program_text();
        let layout = Layout::default();
        let plain = Options { whitespace: true, ..Options::default() };
        let a = minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap();
        let b = minify_chunk_in(&dir, &text, &layout, &plain, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(minify_chunk_in(&dir, &text, &layout, &ALL, None).unwrap(), a);
        assert_eq!(minify_chunk_in(&dir, &text, &layout, &plain, None).unwrap(), b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_is_trimmed_to_its_limit() {
        let dir = temp("trim");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("{i}.min")), vec![0u8; 100]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        trim(&dir, 250);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["3.min", "4.min"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
