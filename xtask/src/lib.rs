// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `cargo xtask`: host-only build helper (slot 156 sqlite-aarch64-cc,
//! 2026-10-21 CEST +02:00). Std only, never shipped.
//!
//! The one job today is `cargo xtask cross <triple> [cargo args]`: build the
//! workspace for a musl/android cross target from a clean shell. Five crates
//! compile the bundled SQLite C amalgamation (`libsqlite3-sys`), and cc-rs
//! needs a C cross compiler for that. Before this, every agent had to write a
//! per-shell `zig cc` wrapper script and export `CC_<triple>`/`AR_<triple>`
//! (docs/toolchain-aarch64.md). Now this binary is its own wrapper:
//!
//! * invoked as `xtask`, it finds zig, hard-links itself as `galion-cc` and
//!   `galion-ar` next to its own executable, sets `CC_<triple>`,
//!   `AR_<triple>` and `GALION_ZIG` for a child `cargo`, and runs it;
//! * invoked as `galion-cc` (by cc-rs inside the SQLite build script), it
//!   execs `zig cc -target <zig triple> <rewritten args>`;
//! * invoked as `galion-ar`, it execs `zig ar <args>`.
//!
//! The cc-mode argument rewriting and the triple table come from
//! `galion-ccwrap` (swarm/realscript-port-3 03967fc, same project, MIT).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// zig version pinned by scripts/install-zig.sh and used by CI.
pub const ZIG_PINNED: &str = "0.15.2";

/// The cross targets `cargo xtask cross` knows.
pub const TARGETS: &[&str] = &[
    "aarch64-unknown-linux-musl",
    "armv7-unknown-linux-musleabihf",
    "x86_64-unknown-linux-musl",
    "aarch64-linux-android",
];

/// Rust target triple -> zig target triple (from galion-ccwrap, 03967fc).
pub fn zig_triple(rust: &str) -> Option<&'static str> {
    Some(match rust {
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        // cc-rs passes the clang spelling `arm-unknown-linux-musleabihf`.
        "armv7-unknown-linux-musleabihf" | "arm-unknown-linux-musleabihf" => "arm-linux-musleabihf",
        // zig ships no bionic headers: compile against musl (check only).
        "aarch64-linux-android" | "aarch64-unknown-linux-android" => "aarch64-linux-musl",
        _ => return None,
    })
}

/// What this executable was asked to be, from the file name of argv[0].
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Cc,
    Ar,
    Task,
}

pub const CC_NAME: &str = "galion-cc";
pub const AR_NAME: &str = "galion-ar";

pub fn mode_of(argv0: &str) -> Mode {
    match Path::new(argv0).file_name().and_then(|n| n.to_str()) {
        Some(CC_NAME) => Mode::Cc,
        Some(AR_NAME) => Mode::Ar,
        _ => Mode::Task,
    }
}

/// The rust triple a cc-mode call is for: cargo's `$TARGET`, which the build
/// script (and so this child) inherits; else cc-rs's `--target=<triple>`.
/// The `--target=` value is a clang triple, not always the rust one (cc-rs
/// 1.8 passes `arm-unknown-linux-musleabihf` for armv7 musl), so `$TARGET`
/// wins and [`zig_triple`] also knows the clang spellings.
pub fn cc_rust_target(args: &[String], target_env: Option<&str>) -> Option<String> {
    target_env.map(str::to_string).or_else(|| {
        args.iter()
            .find_map(|a| a.strip_prefix("--target=").map(str::to_string))
    })
}

/// Arguments for zig in cc mode: `cc -target <zig> <args>` with what zig
/// rejects rewritten. cc-rs's `--target=<rust triple>` is dropped (zig wants
/// its own triple) and `-march=armv7-a` becomes `-mcpu=generic+v7a`. (ucpu-gate: allow, the armv7 target baseline, not CPU pinning)
pub fn zig_cc_args(zig_target: &str, args: &[String]) -> Vec<String> {
    let mut out = vec![
        "cc".to_string(),
        "-target".to_string(),
        zig_target.to_string(),
    ];
    for a in args {
        if a.starts_with("--target=") {
            continue;
        }
        if a == "-march=armv7-a" {
            out.push("-mcpu=generic+v7a".to_string()); // ucpu-gate: allow (generic ARMv7-A = target baseline)
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// `aarch64-unknown-linux-musl` -> `aarch64_unknown_linux_musl` (cc-rs env key form).
pub fn env_key(triple: &str) -> String {
    triple.replace('-', "_")
}

/// Where to look for zig, in order: `$GALION_ZIG`, `$ZIG`, `zig` on `$PATH`,
/// then fixed install dirs (`<repo>/.tools/zig/zig` is where
/// `scripts/install-zig.sh .tools/zig` puts it; the last one is the swarm box).
pub fn zig_candidates(
    galion_zig: Option<OsString>,
    zig: Option<OsString>,
    path: Option<OsString>,
    home: Option<OsString>,
    repo: &Path,
) -> Vec<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    c.extend(galion_zig.map(PathBuf::from));
    c.extend(zig.map(PathBuf::from));
    if let Some(p) = path {
        c.extend(std::env::split_paths(&p).map(|d| d.join("zig")));
    }
    c.push(repo.join(".tools/zig/zig"));
    if let Some(h) = home {
        c.push(PathBuf::from(h).join(".local/zig/zig"));
    }
    c.push(PathBuf::from("/opt/zig/zig"));
    c.push(PathBuf::from(format!(
        "/workspace/tools/zig-x86_64-linux-{ZIG_PINNED}/zig"
    )));
    c
}

/// First candidate that is an executable file.
pub fn first_executable(cands: &[PathBuf]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    cands
        .iter()
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
        .cloned()
}

/// The child cargo command line for `cargo xtask cross <target> <rest>`.
///
/// * no rest -> `build --release --workspace`;
/// * `--target <target>` is appended unless the caller gave one;
/// * with `--workspace`, `--exclude xtask` is appended (xtask is a host tool;
///   the product binary count stays the same as before it existed).
pub fn cross_cargo_args(target: &str, rest: &[String]) -> Vec<String> {
    let mut a: Vec<String> = if rest.is_empty() {
        ["build", "--release", "--workspace"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        rest.to_vec()
    };
    // Everything after a bare `--` goes to rustc/the test binary, not cargo.
    let split = a.iter().position(|x| x == "--").unwrap_or(a.len());
    let tail = a.split_off(split);
    let has = |a: &[String], flag: &str| {
        a.iter()
            .any(|x| x == flag || x.starts_with(&format!("{flag}=")))
    };
    if !has(&a, "--target") {
        a.push("--target".into());
        a.push(target.into());
    }
    let excludes_xtask = a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "xtask")
        || a.iter().any(|x| x == "--exclude=xtask");
    if (has(&a, "--workspace") || has(&a, "--all")) && !excludes_xtask {
        a.push("--exclude".into());
        a.push("xtask".into());
    }
    a.extend(tail);
    a
}

/// Environment the child cargo gets. Variables the caller already set win
/// (same rule as cargo's own `[env]`), so a hand-made wrapper still works.
pub fn cross_env(
    target: &str,
    cc: &Path,
    ar: &Path,
    zig: &Path,
    zig_cache: &Path,
    existing: &dyn Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let v = env_key(target);
    let mut out = Vec::new();
    let mut put = |k: String, val: String| {
        if !existing(&k) {
            out.push((k, val));
        }
    };
    put(format!("CC_{v}"), cc.display().to_string());
    put(format!("AR_{v}"), ar.display().to_string());
    put("GALION_ZIG".into(), zig.display().to_string());
    put(
        "ZIG_GLOBAL_CACHE_DIR".into(),
        zig_cache.display().to_string(),
    );
    put(
        "ZIG_LOCAL_CACHE_DIR".into(),
        zig_cache.display().to_string(),
    );
    out
}

/// Hard-link (or copy, across filesystems) `exe` to `dir/name`, atomically
/// via a temp name + rename so parallel runs never see a half-written file.
pub fn install_alias(exe: &Path, dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let dst = dir.join(name);
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if std::fs::hard_link(exe, &tmp).is_err() {
        std::fs::copy(exe, &tmp)?;
    }
    std::fs::rename(&tmp, &dst)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn triples() {
        for t in TARGETS {
            assert!(zig_triple(t).is_some(), "{t}");
        }
        assert_eq!(
            zig_triple("armv7-unknown-linux-musleabihf"),
            Some("arm-linux-musleabihf")
        );
        assert_eq!(zig_triple("x86_64-unknown-linux-gnu"), None);
        assert_eq!(zig_triple(""), None);
    }

    #[test]
    fn modes() {
        assert_eq!(mode_of("/t/debug/galion-cross/galion-cc"), Mode::Cc);
        assert_eq!(mode_of("galion-ar"), Mode::Ar);
        assert_eq!(mode_of("/t/debug/xtask"), Mode::Task);
        assert_eq!(mode_of("galion-cc.exe"), Mode::Task);
        assert_eq!(mode_of(""), Mode::Task);
    }

    #[test]
    fn cc_args_rewritten() {
        let a = s(&[
            "--target=armv7-unknown-linux-musleabihf",
            "-O2",
            "-march=armv7-a",
            "-c",
            "sqlite3.c",
        ]);
        assert_eq!(
            cc_rust_target(&a, None).as_deref(),
            Some("armv7-unknown-linux-musleabihf")
        );
        assert_eq!(
            zig_cc_args("arm-linux-musleabihf", &a),
            s(&[
                "cc",
                "-target",
                "arm-linux-musleabihf",
                "-O2",
                "-mcpu=generic+v7a", // ucpu-gate: allow
                "-c",
                "sqlite3.c"
            ])
        );
        assert_eq!(
            cc_rust_target(&s(&["-E", "x.c"]), Some("aarch64-unknown-linux-musl")).as_deref(),
            Some("aarch64-unknown-linux-musl")
        );
        assert_eq!(cc_rust_target(&s(&["-E"]), None), None);
        // Real cc-rs 1.8 armv7 call (seen in the 07:04 real run): the clang
        // triple in --target= must not decide; $TARGET does.
        let real = s(&[
            "-Oz",
            "--target=arm-unknown-linux-musleabihf",
            "-c",
            "sqlite3.c",
        ]);
        assert_eq!(
            cc_rust_target(&real, Some("armv7-unknown-linux-musleabihf")).as_deref(),
            Some("armv7-unknown-linux-musleabihf")
        );
        assert_eq!(
            zig_triple("arm-unknown-linux-musleabihf"),
            Some("arm-linux-musleabihf")
        );
    }

    #[test]
    fn cargo_args_default_and_custom() {
        assert_eq!(
            cross_cargo_args("aarch64-unknown-linux-musl", &[]),
            s(&[
                "build",
                "--release",
                "--workspace",
                "--target",
                "aarch64-unknown-linux-musl",
                "--exclude",
                "xtask"
            ])
        );
        assert_eq!(
            cross_cargo_args("t", &s(&["build", "-p", "hermes-rs"])),
            s(&["build", "-p", "hermes-rs", "--target", "t"])
        );
        // caller's --target and --exclude are kept, not doubled
        assert_eq!(
            cross_cargo_args(
                "t",
                &s(&["check", "--all", "--target=u", "--exclude", "xtask"])
            ),
            s(&["check", "--all", "--target=u", "--exclude", "xtask"])
        );
        // flags after `--` are not cargo flags
        assert_eq!(
            cross_cargo_args("t", &s(&["test", "--", "--workspace"])),
            s(&["test", "--target", "t", "--", "--workspace"])
        );
    }

    #[test]
    fn env_respects_existing() {
        let p = Path::new;
        let all = cross_env(
            "aarch64-unknown-linux-musl",
            p("/x/galion-cc"),
            p("/x/galion-ar"),
            p("/z/zig"),
            p("/c"),
            &|_| false,
        );
        let keys: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "CC_aarch64_unknown_linux_musl",
                "AR_aarch64_unknown_linux_musl",
                "GALION_ZIG",
                "ZIG_GLOBAL_CACHE_DIR",
                "ZIG_LOCAL_CACHE_DIR"
            ]
        );
        assert_eq!(all[0].1, "/x/galion-cc");
        let some = cross_env(
            "aarch64-unknown-linux-musl",
            p("/x/galion-cc"),
            p("/x/galion-ar"),
            p("/z/zig"),
            p("/c"),
            &|k| k.starts_with("CC_"),
        );
        assert!(some.iter().all(|(k, _)| !k.starts_with("CC_")));
        assert_eq!(some.len(), 4);
    }

    #[test]
    fn zig_search_order() {
        let repo = Path::new("/r");
        let c = zig_candidates(
            Some("/a/zig".into()),
            None,
            Some("/p1:/p2".into()),
            Some("/h".into()),
            repo,
        );
        assert_eq!(c[0], PathBuf::from("/a/zig"));
        assert_eq!(c[1], PathBuf::from("/p1/zig"));
        assert_eq!(c[2], PathBuf::from("/p2/zig"));
        assert_eq!(c[3], PathBuf::from("/r/.tools/zig/zig"));
        assert_eq!(c[4], PathBuf::from("/h/.local/zig/zig"));
        assert!(c.last().unwrap().ends_with("zig-x86_64-linux-0.15.2/zig"));
        // nothing executable among bogus paths
        assert_eq!(first_executable(&[PathBuf::from("/nonexistent/zig")]), None);
    }

    #[test]
    fn alias_is_atomic_link() {
        let d = std::env::temp_dir().join(format!("xtask-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("src");
        std::fs::write(&src, b"bin").unwrap();
        let out = install_alias(&src, &d.join("al"), CC_NAME).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"bin");
        // second run replaces it in place
        std::fs::write(d.join("src2"), b"new").unwrap();
        install_alias(&d.join("src2"), &d.join("al"), CC_NAME).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"new");
        let left: Vec<_> = std::fs::read_dir(d.join("al")).unwrap().collect();
        assert_eq!(left.len(), 1, "no temp files left");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
