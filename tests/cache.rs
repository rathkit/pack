//! The persistent cache through the command line: a warm build writes what a cold one writes, a damaged or stale entry is ignored,
//! and the cache is used unless `RATHKIT_CACHE=off` (`off`, `0`, `false` and `no` leave the folder alone; no setting uses it).
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rkp-cache-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn build(dir: &Path, cache: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Vec<u8> {
    let out = dir.join("out.js");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rkp"));
    cmd.args(["bundle", dir.join("main.js").to_str().unwrap(), "--outfile", out.to_str().unwrap(), "--no-config", "--minify"]).args(extra).env("RATHKIT_CACHE_DIR", cache).env("RATHKIT_CACHE", "on");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let r = cmd.output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    std::fs::read(&out).unwrap()
}

/// A build with the cache folder named but no RATHKIT_CACHE setting at all.
fn build_unset(dir: &Path, cache: &Path) -> Vec<u8> {
    let out = dir.join("unset.js");
    let r = Command::new(env!("CARGO_BIN_EXE_rkp"))
        .args(["bundle", dir.join("main.js").to_str().unwrap(), "--outfile", out.to_str().unwrap(), "--no-config", "--minify"])
        .env("RATHKIT_CACHE_DIR", cache)
        .env_remove("RATHKIT_CACHE")
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    std::fs::read(&out).unwrap()
}

fn entries(cache: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(cache).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

#[test]
fn cold_warm_damaged_and_off() {
    let dir = scratch("all");
    let cache = dir.join("cache");
    let mut text = String::new();
    for i in 0..600 {
        text.push_str(&format!("function helper{i}(argument) {{ const doubled = argument * 2; return doubled + {i}; }}\nglobalThis.out{i} = helper{i}({i});\n"));
    }
    std::fs::write(dir.join("main.js"), &text).unwrap();

    for word in ["off", "0", "false", "no"] {
        build(&dir, &cache, &[], &[("RATHKIT_CACHE", word)]);
        assert!(entries(&cache).is_empty(), "RATHKIT_CACHE={word} writes nothing");
    }
    let off = build(&dir, &cache, &[], &[("RATHKIT_CACHE", "off")]);
    // No setting at all is on.
    let unset = build_unset(&dir, &cache);
    assert_eq!(entries(&cache).len(), 1, "with no setting the cache is used");
    assert_eq!(unset, off);
    std::fs::remove_dir_all(&cache).unwrap();
    let cold = build(&dir, &cache, &[], &[]);
    assert_eq!(cold, off);
    assert_eq!(entries(&cache).len(), 1);
    let warm = build(&dir, &cache, &[], &[]);
    assert_eq!(warm, cold, "a hit is what a cold build writes");

    // Damaged, truncated and foreign files in place of the entry: the build still succeeds with the same bytes, and the entry is whole again.
    let entry = entries(&cache).remove(0);
    let good = std::fs::read(&entry).unwrap();
    for bad in [Vec::new(), good[..good.len() / 3].to_vec(), b"RKPCACH1 and then rubbish".to_vec(), good.iter().map(|b| b ^ 0xff).collect()] {
        std::fs::write(&entry, bad).unwrap();
        assert_eq!(build(&dir, &cache, &[], &[]), cold);
        assert_eq!(std::fs::read(&entry).unwrap(), good);
    }

    // A changed source is a different entry; the old output is not served for it.
    std::fs::write(dir.join("main.js"), format!("{text}globalThis.last = 1;\n")).unwrap();
    let changed = build(&dir, &cache, &[], &[]);
    assert_ne!(changed, cold);
    assert_eq!(build(&dir, &cache, &[], &[("RATHKIT_CACHE", "off")]), changed);
    assert_eq!(entries(&cache).len(), 2);

    // A folder that cannot be written to is not an error.
    let blocked = dir.join("file-not-folder");
    std::fs::write(&blocked, "x").unwrap();
    assert_eq!(build(&dir, &blocked.join("inside"), &[], &[]), changed);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A setting that changes the output is part of what an entry answers to: the same source built another way is another entry, and
/// each way keeps getting its own answer from the cache that it gets without one.
#[test]
fn a_changed_setting_is_another_entry() {
    let dir = scratch("settings");
    let cache = dir.join("cache");
    let mut text = String::new();
    for i in 0..600 {
        text.push_str(&format!("export const value{i} = (argument) => {{ const doubled = argument ?? {i}; return doubled * 2 + {i}; }};\nglobalThis.out{i} = value{i}({i});\n"));
    }
    std::fs::write(dir.join("main.js"), &text).unwrap();
    let variants: [&[&str]; 4] = [&[], &["--target=es2015"], &["--format=cjs"], &["--sourcemap"]];
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for (n, extra) in variants.iter().enumerate() {
        let plain = build(&dir, &cache, extra, &[("RATHKIT_CACHE", "off")]);
        let cold = build(&dir, &cache, extra, &[]);
        let warm = build(&dir, &cache, extra, &[]);
        assert_eq!(cold, plain, "{extra:?}: a cold build with the cache is the build without it");
        assert_eq!(warm, plain, "{extra:?}: a hit is the build without the cache");
        assert!(entries(&cache).len() > n, "{extra:?}: its own entry");
        seen.push(plain);
    }
    // The two that differ in the text of the output differ from the first; none was answered with another's bytes.
    assert_ne!(seen[0], seen[1], "es2015 lowers `??`");
    assert_ne!(seen[0], seen[2], "cjs wraps the exports");
    let _ = std::fs::remove_dir_all(&dir);
}
