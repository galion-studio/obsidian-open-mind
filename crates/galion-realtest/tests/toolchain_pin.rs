// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 196 (ci-toolchain-pin, integration S4-T6, RELEASE-CHECKLIST 1.8):
//! one Rust version for the whole tree.
//!
//! * `rust-toolchain.toml` pins `channel` to an exact `X.Y.Z` release.
//! * `[workspace.package] rust-version` is `X.Y` of that channel and every
//!   crate inherits it; `edge/openmind-node` (own workspace) has the same.
//! * Every workflow job that runs cargo (directly or through a script that
//!   calls cargo) installs exactly that channel with `dtolnay/rust-toolchain`;
//!   nothing in the workflows, `scripts/` or `crates/xtask` overrides it
//!   (`+stable`, `RUSTUP_TOOLCHAIN`, `toolchain: stable`, ...).
//!
//! `toolchain_pin_consistent` always runs (reads files). The `rejects_*`
//! tests mutate a scratch copy and prove the checker fails. With
//! `GALION_REAL=1`, `real_toolchain_pin` asks rustup which compiler this tree
//! resolves to (env cleaned), proves a missing pinned toolchain is a hard
//! error rather than a silent fallback, builds tuner for x86_64 and aarch64
//! musl and armv7 musleabihf with the pinned compiler, reads the rustc version
//! rustc stamps into each binary, and runs the three binaries (aarch64 and
//! armv7 under qemu-user) on 127.0.0.1:20660-20662 with real HTTP GETs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::http;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// `key = "value"` of a flat TOML section (comments stripped).
fn toml_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim().to_string();
        if line.starts_with('[') && line.ends_with(']') {
            cur = line[1..line.len() - 1].trim().to_string();
        } else if cur == section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Drop a `#` comment that is not inside a double-quoted string.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(v: &str) -> &str {
    v.trim().trim_matches('"').trim_matches('\'')
}

/// `X.Y.Z` -> (X, Y, Z); None for channel names or partial versions.
fn exact_version(v: &str) -> Option<(u32, u32, u32)> {
    let p: Vec<&str> = v.split('.').collect();
    if p.len() != 3 {
        return None;
    }
    Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn files_in(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_in(&p, ext));
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// Text that switches a cargo/rustc call away from the pinned toolchain.
const OVERRIDES: &[&str] = &[
    "+stable",
    "+beta",
    "+nightly",
    "RUSTUP_TOOLCHAIN",
    "rustup default",
    "rustup override",
];

/// Workflow jobs: (name, lines) under the top-level `jobs:` key.
fn jobs(yaml: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut in_jobs = false;
    for raw in yaml.lines() {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        if indent == 0 {
            in_jobs = raw.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if indent == 2 && raw.trim_end().ends_with(':') {
            out.push((raw.trim().trim_end_matches(':').to_string(), Vec::new()));
        } else if let Some(j) = out.last_mut() {
            j.1.push(raw.to_string());
        }
    }
    out
}

/// Does this job compile Rust? A run line names cargo/rustc/rustup, or runs a
/// repo script whose text calls cargo.
fn job_runs_cargo(root: &Path, lines: &[String]) -> bool {
    lines.iter().any(|l| {
        let l = strip_comment(l);
        if l.trim_start().starts_with("name:") {
            return false;
        }
        let words: Vec<&str> = l
            .split(|c: char| c.is_whitespace() || c == '"' || c == '(' || c == '$')
            .collect();
        if words
            .iter()
            .any(|w| *w == "cargo" || *w == "rustc" || *w == "rustup")
        {
            return true;
        }
        words.iter().any(|w| {
            w.starts_with("scripts/") && {
                let s = read(&root.join(w));
                s.lines()
                    .map(strip_comment)
                    .any(|x| x.contains("cargo ") || x.contains("\"$CARGO\""))
            }
        })
    })
}

/// Every problem found, as human-readable lines. Empty = consistent.
fn check(root: &Path) -> Vec<String> {
    let mut errs = Vec::new();
    let tc = read(&root.join("rust-toolchain.toml"));
    if tc.is_empty() {
        errs.push("rust-toolchain.toml missing".into());
        return errs;
    }
    let channel = toml_get(&tc, "toolchain", "channel")
        .map(|v| unquote(&v).to_string())
        .unwrap_or_default();
    let Some((maj, min, _)) = exact_version(&channel) else {
        errs.push(format!(
            "rust-toolchain.toml channel {channel:?} is not an exact X.Y.Z release"
        ));
        return errs;
    };
    let comps = toml_get(&tc, "toolchain", "components").unwrap_or_default();
    for c in ["rustfmt", "clippy"] {
        if !comps.contains(c) {
            errs.push(format!("rust-toolchain.toml components lacks {c}"));
        }
    }
    let want_rv = format!("{maj}.{min}");

    let ws = read(&root.join("Cargo.toml"));
    match toml_get(&ws, "workspace.package", "rust-version") {
        Some(v) if unquote(&v) == want_rv || unquote(&v) == channel => {}
        other => errs.push(format!(
            "Cargo.toml [workspace.package] rust-version {other:?}, want \"{want_rv}\""
        )),
    }
    let mut manifests: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path().join("Cargo.toml"))
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    manifests.sort();
    if manifests.is_empty() {
        errs.push("no crates/*/Cargo.toml found".into());
    }
    for m in &manifests {
        let t = read(m);
        let ok = match toml_get(&t, "package", "rust-version.workspace") {
            Some(v) => v == "true",
            None => toml_get(&t, "package", "rust-version")
                .is_some_and(|v| unquote(&v) == want_rv || unquote(&v) == channel),
        };
        if !ok {
            errs.push(format!(
                "{}: no rust-version.workspace = true",
                m.strip_prefix(root).unwrap_or(m).display()
            ));
        }
    }
    let node = root.join("edge/openmind-node/Cargo.toml");
    if node.is_file() {
        let v = toml_get(&read(&node), "package", "rust-version");
        if !v
            .as_deref()
            .is_some_and(|v| unquote(v) == want_rv || unquote(v) == channel)
        {
            errs.push(format!(
                "edge/openmind-node/Cargo.toml rust-version {v:?}, want \"{want_rv}\""
            ));
        }
    }

    let wf = files_in(&root.join(".github/workflows"), "yml");
    if wf.is_empty() {
        errs.push("no .github/workflows/*.yml".into());
    }
    for f in &wf {
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        let y = read(f);
        for (n, l) in y.lines().enumerate() {
            let code = strip_comment(l);
            if let Some((_, v)) = code.split_once("toolchain:") {
                if unquote(v) != channel {
                    errs.push(format!(
                        "{rel}:{}: toolchain {:?} != {channel}",
                        n + 1,
                        unquote(v)
                    ));
                }
            }
            for o in OVERRIDES {
                if code.contains(o) {
                    errs.push(format!("{rel}:{}: toolchain override {o:?}", n + 1));
                }
            }
        }
        for (job, lines) in jobs(&y) {
            if !job_runs_cargo(root, &lines) {
                continue;
            }
            let action = lines
                .iter()
                .any(|l| strip_comment(l).contains("dtolnay/rust-toolchain@"));
            let pinned = lines.iter().any(|l| {
                strip_comment(l)
                    .split_once("toolchain:")
                    .is_some_and(|(_, v)| unquote(v) == channel)
            });
            if !(action && pinned) {
                errs.push(format!(
                    "{rel} job {job}: runs cargo but does not install toolchain {channel}"
                ));
            }
        }
    }

    // Local runners: scripts/ and (when present) the Rust CI task runner.
    let mut runners = files_in(&root.join("scripts"), "sh");
    runners.extend(files_in(&root.join("crates/xtask"), "rs"));
    for f in runners {
        let rel = f.strip_prefix(root).unwrap_or(&f).display().to_string();
        for (n, l) in read(&f).lines().enumerate() {
            let code = if rel.ends_with(".sh") {
                strip_comment(l)
            } else {
                l.split("//").next().unwrap_or("")
            };
            for o in OVERRIDES {
                if code.contains(o) {
                    errs.push(format!("{rel}:{}: toolchain override {o:?}", n + 1));
                }
            }
        }
    }
    errs
}

#[test]
fn toolchain_pin_consistent() {
    let r = root();
    let errs = check(&r);
    assert!(
        errs.is_empty(),
        "toolchain pin broken:\n{}",
        errs.join("\n")
    );
    // The crate under test was itself compiled with the workspace value.
    let tc = read(&r.join("rust-toolchain.toml"));
    let ch = unquote(&toml_get(&tc, "toolchain", "channel").unwrap()).to_string();
    let (a, b, _) = exact_version(&ch).unwrap();
    assert_eq!(env!("CARGO_PKG_RUST_VERSION"), format!("{a}.{b}"));
    // The cargo running this test (and `cargo xtask`, which spawns $CARGO)
    // is the pinned one when it comes from a rustup toolchain directory.
    if let Ok(c) = std::env::var("CARGO") {
        if c.contains("/toolchains/") {
            assert!(
                c.contains(&format!("/toolchains/{ch}-")),
                "CARGO={c} is not {ch}"
            );
            eprintln!("CARGO is the pinned {ch} cargo");
        }
    }
    // Kept from slot 145 (toolchain-aarch64 4d0c036): cross linkers.
    let cfg = read(&r.join(".cargo/config.toml"));
    for t in [
        "aarch64-unknown-linux-musl",
        "armv7-unknown-linux-musleabihf",
    ] {
        assert_eq!(
            toml_get(&cfg, &format!("target.{t}"), "linker").as_deref(),
            Some("\"rust-lld\""),
            "{t} linker"
        );
    }
    let jobs_n: usize = files_in(&r.join(".github/workflows"), "yml")
        .iter()
        .map(|f| {
            jobs(&read(f))
                .iter()
                .filter(|(_, l)| job_runs_cargo(&r, l))
                .count()
        })
        .sum();
    eprintln!("channel {ch}; {jobs_n} cargo jobs all pinned");
    assert!(jobs_n >= 10, "expected the CI cargo jobs, found {jobs_n}");
}

/// Scratch copy of the files `check` reads.
fn scratch(tag: &str) -> PathBuf {
    let r = root();
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("toolchain-pin-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let copy = |rel: &Path| {
        let d = base.join(rel);
        std::fs::create_dir_all(d.parent().unwrap()).unwrap();
        std::fs::copy(r.join(rel), d).unwrap();
    };
    for f in [
        "rust-toolchain.toml",
        "Cargo.toml",
        "edge/openmind-node/Cargo.toml",
    ] {
        copy(Path::new(f));
    }
    for dir in ["crates", ".github/workflows", "scripts"] {
        let mut v = files_in(&r.join(dir), "toml");
        v.extend(files_in(&r.join(dir), "yml"));
        v.extend(files_in(&r.join(dir), "sh"));
        if dir == "crates" {
            v.retain(|p| p.ends_with("Cargo.toml") || p.starts_with(r.join("crates/xtask")));
        }
        for p in v {
            copy(p.strip_prefix(&r).unwrap());
        }
    }
    assert!(check(&base).is_empty(), "scratch copy must start clean");
    base
}

fn edit(p: &Path, from: &str, to: &str) {
    let t = read(p);
    assert!(t.contains(from), "{} lacks {from:?}", p.display());
    std::fs::write(p, t.replacen(from, to, 1)).unwrap();
}

fn expect_err(base: &Path, needle: &str) {
    let errs = check(base);
    let _ = std::fs::remove_dir_all(base);
    assert!(
        errs.iter().any(|e| e.contains(needle)),
        "wanted an error containing {needle:?}, got {errs:?}"
    );
}

#[test]
fn rejects_floating_channel() {
    let b = scratch("channel");
    edit(
        &b.join("rust-toolchain.toml"),
        "channel = \"",
        "channel = \"stable\" # ",
    );
    expect_err(&b, "not an exact X.Y.Z");
}

#[test]
fn rejects_workflow_stable() {
    let b = scratch("wf");
    let f = b.join(".github/workflows/self-eval.yml");
    let t = read(&f);
    let i = t.find("toolchain: \"").expect("pinned toolchain line");
    let end = i + t[i..].find('\n').unwrap();
    std::fs::write(&f, format!("{}toolchain: stable{}", &t[..i], &t[end..])).unwrap();
    expect_err(&b, "toolchain \"stable\"");
}

#[test]
fn rejects_job_without_toolchain_step() {
    let b = scratch("job");
    let f = b.join(".github/workflows/realtest.yml");
    let t = read(&f);
    let kept: Vec<&str> = t
        .lines()
        .filter(|l| !l.contains("dtolnay/rust-toolchain@") && !l.contains("toolchain: \""))
        .collect();
    std::fs::write(&f, kept.join("\n")).unwrap();
    expect_err(&b, "does not install toolchain");
}

#[test]
fn rejects_crate_without_rust_version() {
    let b = scratch("crate");
    edit(
        &b.join("crates/tuner/Cargo.toml"),
        "rust-version.workspace = true\n",
        "",
    );
    expect_err(&b, "crates/tuner/Cargo.toml: no rust-version");
}

#[test]
fn rejects_mismatched_rust_version() {
    let b = scratch("rv");
    edit(
        &b.join("Cargo.toml"),
        "rust-version = \"",
        "rust-version = \"1.70\" # ",
    );
    expect_err(&b, "rust-version");
}

#[test]
fn rejects_override_in_runner() {
    let b = scratch("runner");
    let f = b.join("scripts/ucpu-gate.sh");
    let t = read(&f);
    std::fs::write(&f, format!("{t}\ncargo +nightly build\n")).unwrap();
    expect_err(&b, "scripts/ucpu-gate.sh");
    let b = scratch("xtask");
    let x = b.join("crates/xtask/src/main.rs");
    std::fs::create_dir_all(x.parent().unwrap()).unwrap();
    std::fs::write(
        &x,
        "fn main() { std::env::set_var(\"RUSTUP_TOOLCHAIN\", \"stable\"); }\n",
    )
    .unwrap();
    expect_err(&b, "crates/xtask/src/main.rs");
}

// ------------------------------------------------------------ GALION_REAL=1

/// rustup/cargo with every toolchain-selecting variable removed, so only the
/// rust-toolchain.toml in `dir` (or the default) decides.
fn clean(cmd: &mut Command) -> &mut Command {
    for k in [
        "RUSTUP_TOOLCHAIN",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTDOC",
        "CARGO",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTC",
    ] {
        cmd.env_remove(k);
    }
    for (k, _) in std::env::vars() {
        if k.starts_with("CARGO_TARGET_") && k.ends_with("_LINKER") {
            cmd.env_remove(k);
        }
    }
    cmd
}

fn out(cmd: &mut Command) -> (bool, String) {
    let o = cmd.output().expect("spawn");
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.success(), s)
}

struct Kill(Vec<Child>);
impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|f| f.is_file())
    })
}

/// All `rustc version ...` stamps (the `.comment` section) in a binary.
fn rustc_stamps(p: &Path) -> Vec<String> {
    let b = std::fs::read(p).expect("read binary");
    let pat = b"rustc version ";
    let mut v = Vec::new();
    let mut i = 0;
    while let Some(off) = b[i..].windows(pat.len()).position(|w| w == pat) {
        let s = i + off;
        let e = b[s..]
            .iter()
            .position(|&c| c == 0)
            .map_or(b.len(), |n| s + n);
        let st = String::from_utf8_lossy(&b[s..e]).into_owned();
        if !v.contains(&st) {
            v.push(st);
        }
        i = e;
    }
    v
}

#[test]
fn real_toolchain_pin() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!(
            "skipped: set GALION_REAL=1 (rustup resolve, 3 cross builds, runs on 20660-20662)"
        );
        return;
    }
    let r = root();
    let tc = read(&r.join("rust-toolchain.toml"));
    let ch = unquote(&toml_get(&tc, "toolchain", "channel").unwrap()).to_string();

    // 1. Which compiler does this tree resolve to, with no env override?
    let (ok, s) =
        out(clean(Command::new("rustup").args(["show", "active-toolchain"])).current_dir(&r));
    eprintln!("repo: rustup show active-toolchain -> {}", s.trim());
    assert!(
        ok && s.starts_with(&format!("{ch}-")),
        "active toolchain {s}"
    );
    assert!(
        s.contains("rust-toolchain.toml"),
        "not chosen by the file: {s}"
    );
    let (ok, s) = out(clean(Command::new("rustc").arg("-vV")).current_dir(&r));
    eprintln!(
        "repo: rustc -vV release line: {:?}",
        s.lines().find(|l| l.starts_with("release:"))
    );
    assert!(
        ok && s.lines().any(|l| l == format!("release: {ch}")),
        "rustc {s}"
    );
    let (ok, s) = out(clean(Command::new("cargo").arg("-V")).current_dir(&r));
    eprintln!("repo: cargo -V -> {}", s.trim());
    assert!(ok);

    // 2. Negative control: a channel that is not installed must fail loudly
    //    (no silent fallback to the default toolchain), auto-install off.
    let tmp = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("toolchain-pin-neg-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.0.0\"\n",
    )
    .unwrap();
    let (ok, s) = out(clean(&mut Command::new("rustc"))
        .arg("-V")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .current_dir(&tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "neg: channel 1.0.0, RUSTUP_AUTO_INSTALL=0 -> ok={ok} {:?}",
        s.lines().next().unwrap_or("")
    );
    assert!(!ok, "unpinned fallback: {s}");

    // 3. Build tuner (pure Rust) for the three musl targets with the pinned
    //    compiler resolved by the file, then read the rustc stamp.
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| r.join("target"));
    let td = base.join("toolchain-pin-real");
    let targets = [
        ("x86_64-unknown-linux-musl", None, 20660u16),
        (
            "aarch64-unknown-linux-musl",
            Some("qemu-aarch64-static"),
            20661,
        ),
        (
            "armv7-unknown-linux-musleabihf",
            Some("qemu-arm-static"),
            20662,
        ),
    ];
    let mut cmd = Command::new("nice");
    clean(&mut cmd)
        .args([
            "-n",
            "10",
            "cargo",
            "build",
            "--release",
            "--locked",
            "-j",
            "2",
            "-p",
            "tuner",
        ])
        .current_dir(&r)
        .env("CARGO_TARGET_DIR", &td);
    for (t, _, _) in &targets {
        cmd.args(["--target", t]);
    }
    let t0 = Instant::now();
    assert!(cmd.status().expect("cargo").success(), "tuner cross build");
    eprintln!("build tuner x3 targets: {:.1}s", t0.elapsed().as_secs_f64());

    let www = td.join("www");
    std::fs::create_dir_all(&www).unwrap();
    std::fs::write(
        www.join("index.html"),
        "<!doctype html><title>slot196 pin</title>\n",
    )
    .unwrap();
    let mut kids = Kill(Vec::new());
    for (t, runner, port) in &targets {
        let bin = td.join(t).join("release/tuner");
        let stamps = rustc_stamps(&bin);
        eprintln!("{t}: {stamps:?}");
        assert!(!stamps.is_empty(), "{t}: no rustc stamp");
        assert!(
            stamps
                .iter()
                .all(|s| s.starts_with(&format!("rustc version {ch} "))),
            "{t}: built by another rustc: {stamps:?}"
        );
        let mut c = match runner {
            None => Command::new(&bin),
            Some(q) => {
                let Some(q) = which(q) else {
                    eprintln!("{t}: {q} missing, stamp check only");
                    continue;
                };
                let mut c = Command::new(q);
                c.arg(&bin);
                c
            }
        };
        let addr = format!("127.0.0.1:{port}");
        assert!(!http::port_open(&addr), "{addr} already in use");
        c.env("TUNER_ADDR", &addr)
            .env("TUNER_ROOT", &www)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        kids.0.push(c.spawn().expect("spawn tuner"));
        let end = Instant::now() + Duration::from_secs(60);
        while !http::port_open(&addr) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(250));
        }
        for (path, want) in [("/health", "\"ok\":true"), ("/", "slot196 pin")] {
            let resp = http::get(&addr, path).unwrap_or_else(|e| panic!("GET {addr}{path}: {e}"));
            let body = resp.text();
            eprintln!(
                "{t}: GET {addr}{path} -> {} {}",
                resp.status,
                body.chars().take(80).collect::<String>().replace('\n', " ")
            );
            assert_eq!(resp.status, 200);
            assert!(body.contains(want), "{addr}{path}: wanted {want}");
        }
    }
    drop(kids);
    for (_, _, port) in &targets {
        assert!(
            !http::port_open(&format!("127.0.0.1:{port}")),
            "{port} still open"
        );
    }
    let _ = std::fs::remove_dir_all(&www);
}
