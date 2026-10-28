// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life parity test (GALION_REAL=1): the original bash
//! galion-backup.sh (from git, ws/foundation e43d7ab, byte-identical to the
//! VPS copy, sha256 0897efc0...) and the release-built Rust
//! `galion-backup run` are run against the SAME fake VPS tree at the SAME
//! absolute paths, inside an unprivileged user + mount namespace
//! (`unshare -rm`) with a chroot whose /usr, /dev and /proc are bind mounts
//! of the box's. Every scenario's exit code, log lines, modes, archive
//! listing and archive contents are compared. Nothing touches the host's
//! /var, /opt, /etc or /root, nor the VPS.
//!
//! The outer test only re-executes this test binary under `unshare`; the
//! work happens in `real_parity_inner` (a no-op unless the env marker is set).
//!
//! `GALION_REAL=1 cargo test --release -p galion-backup --test real_parity -- --nocapture`
//! (optional `GALION_BACKUP_BIN=<path>` to test e.g. the static musl build).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-backup");
const ORACLE_REV: &str = "e43d7ab:deploy/foundation/galion-backup.sh";
const ORACLE_SHA256: &str = "0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4";
const MARK: &str = "GALION_BACKUP_PARITY_ROOT";

fn say(m: &str) {
    println!("[{}] {m}", zagreb_now());
}

fn zagreb_now() -> String {
    galion_backup::backup::now_iso()
}

fn oracle() -> Option<Vec<u8>> {
    if let Ok(p) = std::env::var("GALION_BACKUP_ORACLE") {
        return fs::read(p).ok();
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["show", ORACLE_REV])
        .output()
        .ok()?;
    o.status.success().then_some(o.stdout)
}

fn sha(b: &[u8]) -> String {
    let mut h = galion_backup::sha256::Sha256::new();
    h.update(b);
    galion_backup::sha256::hex(&h.finish())
}

#[test]
fn real_bash_vs_rust_parity() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let Some(script) = oracle() else {
        eprintln!("SKIP: oracle {ORACLE_REV} not in this repo (fetch ws/foundation or set GALION_BACKUP_ORACLE)");
        return;
    };
    assert_eq!(sha(&script), ORACLE_SHA256, "oracle is the VPS script");
    let f = common::tmp("parity");
    // ---- chroot skeleton
    for d in [
        "usr",
        "dev",
        "proc",
        "tmp",
        "var/backups",
        "opt/galion/rust/bin",
        "etc",
    ] {
        fs::create_dir_all(f.join(d)).unwrap();
    }
    fs::set_permissions(f.join("tmp"), fs::Permissions::from_mode(0o1777)).unwrap();
    for (l, t) in [
        ("bin", "usr/bin"),
        ("sbin", "usr/sbin"),
        ("lib", "usr/lib"),
        ("lib64", "usr/lib64"),
    ] {
        symlink(t, f.join(l)).unwrap();
    }
    for e in ["passwd", "group", "nsswitch.conf"] {
        let _ = fs::copy(Path::new("/etc").join(e), f.join("etc").join(e));
    }
    let bin_dir = f.join("opt/galion/rust/bin");
    // GALION_BACKUP_BIN: test a specific build (e.g. the static musl release).
    let bin = std::env::var("GALION_BACKUP_BIN").unwrap_or_else(|_| BIN.to_string());
    say(&format!("rust binary under test: {bin}"));
    fs::copy(&bin, bin_dir.join("galion-backup")).unwrap();
    fs::write(bin_dir.join("galion-backup.sh"), &script).unwrap();
    fs::set_permissions(
        bin_dir.join("galion-backup.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    common::build_tree(&f);

    let me = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .args(["-rm", "--propagation", "private"])
        .arg(&me)
        .args([
            "--exact",
            "real_parity_inner",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MARK, &f)
        .status()
        .expect("unshare");
    if st.success() {
        let _ = fs::remove_dir_all(&f);
    }
    assert!(
        st.success(),
        "inner parity run failed (tree kept at {})",
        f.display()
    );
}

// ------------------------------------------------------------------ inner

fn bind(src: &str, dst: &Path) {
    let s = std::ffi::CString::new(src).unwrap();
    let d = std::ffi::CString::new(dst.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: valid C strings; plain recursive bind mount in our own mount ns.
    let rc = unsafe {
        libc::mount(
            s.as_ptr(),
            d.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "bind {src}: {}", std::io::Error::last_os_error());
}

struct Jail {
    root: PathBuf,
}

impl Jail {
    fn host(&self, p: &str) -> PathBuf {
        self.root.join(p.trim_start_matches('/'))
    }

    fn cmd(&self, argv: &[&str], env: &[(&str, &str)]) -> Command {
        let mut c = Command::new(argv[0]);
        c.args(&argv[1..])
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .env("LANG", "C")
            .stdin(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        let root = std::ffi::CString::new(self.root.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: only async-signal-safe syscalls between fork and exec.
        unsafe {
            c.pre_exec(move || {
                if libc::chroot(root.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::umask(0o022);
                Ok(())
            });
        }
        c
    }

    fn run(&self, which: Impl, dest: &str, extra: &[(&str, &str)]) -> Output {
        let mut env = vec![("GALION_BACKUP_DEST", dest)];
        env.extend_from_slice(extra);
        let argv: &[&str] = match which {
            Impl::Bash => &["/bin/bash", "/opt/galion/rust/bin/galion-backup.sh"],
            Impl::Rust => &["/opt/galion/rust/bin/galion-backup", "run"],
        };
        self.cmd(argv, &env).output().unwrap()
    }
}

#[derive(Clone, Copy, Debug)]
enum Impl {
    Bash,
    Rust,
}

/// Log lines with the volatile parts blanked: ts, stamp, size, free KB.
fn norm_log(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out)
        .lines()
        .map(|l| {
            let l = match l.find(" svc=") {
                Some(i) if l.starts_with("ts=") => &l[i + 1..],
                _ => l,
            };
            let mut s = String::new();
            let mut digits = false;
            for ch in l.chars() {
                if ch.is_ascii_digit() {
                    if !digits {
                        s.push('N');
                    }
                    digits = true;
                } else {
                    digits = false;
                    s.push(ch);
                }
            }
            s
        })
        .collect()
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

fn archives(dir: &Path) -> Vec<PathBuf> {
    galion_backup::backup::archives(dir)
}

fn tar_tv(args: &[&str], p: &Path) -> String {
    let o = Command::new("tar").args(args).arg(p).output().unwrap();
    assert!(
        o.status.success(),
        "tar {args:?} {}: {}",
        p.display(),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `tar -tv` minus the date/time columns, stamp blanked.
fn listing(s: &str, stamp: &str) -> Vec<String> {
    s.lines()
        .map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            let mut v = vec![c[0], c[1], c[2]];
            v.extend_from_slice(&c[5..]);
            v.join(" ").replace(stamp, "STAMP")
        })
        .collect()
}

fn stamp_of(a: &Path) -> String {
    a.file_name()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches("galion-")
        .trim_end_matches(".tar.gz")
        .to_string()
}

/// Every file under `dir` (relative path -> bytes; symlinks as "-> target").
fn tree(dir: &Path) -> BTreeMap<String, (u32, Vec<u8>)> {
    let mut m = BTreeMap::new();
    fn walk(base: &Path, d: &Path, m: &mut BTreeMap<String, (u32, Vec<u8>)>) {
        for e in fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let md = fs::symlink_metadata(&p).unwrap();
            let mode = md.permissions().mode() & 0o7777;
            if md.is_dir() {
                m.insert(rel.clone() + "/", (mode, vec![]));
                walk(base, &p, m);
            } else if md.file_type().is_symlink() {
                let t = fs::read_link(&p).unwrap();
                m.insert(rel, (0, format!("-> {}", t.display()).into_bytes()));
            } else if md.is_file() {
                m.insert(rel, (mode, fs::read(&p).unwrap()));
            } else {
                m.insert(rel, (mode, b"<special>".to_vec()));
            }
        }
    }
    walk(dir, dir, &mut m);
    m
}

fn extract(a: &Path, into: &Path) -> PathBuf {
    fs::create_dir_all(into).unwrap();
    let st = Command::new("tar")
        .args(["-xpzf"])
        .arg(a)
        .arg("-C")
        .arg(into)
        .status()
        .unwrap();
    assert!(st.success());
    let top = into.join(format!("galion-{}", stamp_of(a)));
    let ft = top.join("files/files.tar");
    let fx = into.join("files-x");
    fs::create_dir_all(&fx).unwrap();
    let st = Command::new("tar")
        .args(["-xpf"])
        .arg(&ft)
        .arg("-C")
        .arg(&fx)
        .status()
        .unwrap();
    assert!(st.success());
    top
}

struct Tally {
    pass: u32,
    fail: u32,
}

impl Tally {
    fn check(&mut self, what: &str, ok: bool, detail: impl FnOnce() -> String) {
        if ok {
            self.pass += 1;
            say(&format!("PASS {what}"));
        } else {
            self.fail += 1;
            say(&format!("FAIL {what}\n{}", detail()));
        }
    }
}

fn first_diff(
    a: &BTreeMap<String, (u32, Vec<u8>)>,
    b: &BTreeMap<String, (u32, Vec<u8>)>,
) -> String {
    for (k, v) in a {
        match b.get(k) {
            None => return format!("only in bash: {k}"),
            Some(w) if w != v => return format!("differs: {k} mode {:o} vs {:o}", v.0, w.0),
            _ => {}
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            return format!("only in rust: {k}");
        }
    }
    "none".into()
}

#[test]
fn real_parity_inner() {
    let Ok(root) = std::env::var(MARK) else {
        return; // only meaningful when re-executed by real_bash_vs_rust_parity
    };
    let j = Jail {
        root: PathBuf::from(&root),
    };
    // /etc/alternatives: awk, etc. are Debian alternatives symlinks.
    fs::create_dir_all(j.root.join("etc/alternatives")).unwrap();
    for (s, d) in [
        ("/usr", "usr"),
        ("/dev", "dev"),
        ("/proc", "proc"),
        ("/etc/alternatives", "etc/alternatives"),
    ] {
        bind(s, &j.root.join(d));
    }
    let mut t = Tally { pass: 0, fail: 0 };
    let both = |dest: &str, extra: &[(&str, &str)]| {
        let b = j.run(Impl::Bash, &format!("/var/backups/sh-{dest}"), extra);
        let r = j.run(Impl::Rust, &format!("/var/backups/rs-{dest}"), extra);
        (b, r)
    };
    let show = |o: &Output| {
        format!(
            "  exit={:?} stdout={:?} stderr={:?}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    };

    // ---------------------------------------------------- S1 full backup
    say("== S1 full backup (all 4 DBs, files, dns-backup glob)");
    let (b, r) = both("full", &[]);
    say(&format!(
        "   bash: {}",
        String::from_utf8_lossy(&b.stdout).trim()
    ));
    say(&format!(
        "   rust: {}",
        String::from_utf8_lossy(&r.stdout).trim()
    ));
    t.check(
        "exit codes equal (0)",
        b.status.code() == Some(0) && r.status.code() == Some(0),
        || show(&b) + &show(&r),
    );
    t.check(
        "log lines equal (ts/stamp/size blanked)",
        norm_log(&b.stdout) == norm_log(&r.stdout),
        || show(&b) + &show(&r),
    );
    let (sd, rd) = (
        j.host("/var/backups/sh-full"),
        j.host("/var/backups/rs-full"),
    );
    t.check(
        "DEST mode 0700 both",
        mode(&sd) == 0o700 && mode(&rd) == 0o700,
        || format!("{:o} {:o}", mode(&sd), mode(&rd)),
    );
    t.check(
        ".lock mode equal (0600)",
        mode(&sd.join(".lock")) == 0o600 && mode(&rd.join(".lock")) == 0o600,
        String::new,
    );
    let (sa, ra) = (archives(&sd), archives(&rd));
    t.check("one archive each", sa.len() == 1 && ra.len() == 1, || {
        format!("{sa:?} {ra:?}")
    });
    let (sa, ra) = (&sa[0], &ra[0]);
    t.check(
        "archive mode 0600 both",
        mode(sa) == 0o600 && mode(ra) == 0o600,
        String::new,
    );
    let names = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v.into_iter()
            .map(|n| {
                if n.starts_with("galion-") {
                    "galion-STAMP.tar.gz".into()
                } else {
                    n
                }
            })
            .collect()
    };
    t.check(
        "DEST contents equal (no .work-*, no .part)",
        names(&sd) == names(&rd),
        || format!("{:?} vs {:?}", names(&sd), names(&rd)),
    );
    let (ss, rs) = (stamp_of(sa), stamp_of(ra));
    let (lb, lr) = (
        listing(&tar_tv(&["-tvzf"], sa), &ss),
        listing(&tar_tv(&["-tvzf"], ra), &rs),
    );
    t.check(
        "outer listing equal (order, modes, owners, sizes)",
        lb == lr,
        || format!("bash:\n{}\nrust:\n{}", lb.join("\n"), lr.join("\n")),
    );
    let (xb, xr) = (
        extract(sa, &j.root.join("tmp/xb")),
        extract(ra, &j.root.join("tmp/xr")),
    );
    let (fb, fr) = (
        fs::read(xb.join("files/files.tar")).unwrap(),
        fs::read(xr.join("files/files.tar")).unwrap(),
    );
    t.check("files.tar byte-identical", fb == fr, || {
        let i = fb
            .iter()
            .zip(&fr)
            .position(|(a, b)| a != b)
            .unwrap_or(fb.len().min(fr.len()));
        format!(
            "len {} vs {}, first diff at byte {i} (block {} offset {})",
            fb.len(),
            fr.len(),
            i / 512,
            i % 512
        )
    });
    let (ib, ir) = (
        tar_tv(&["-tvf"], &xb.join("files/files.tar")),
        tar_tv(&["-tvf"], &xr.join("files/files.tar")),
    );
    t.check("files.tar listing equal", ib == ir, || {
        format!("bash:\n{ib}\nrust:\n{ir}")
    });
    say(&format!(
        "   files.tar: {} members, {} bytes",
        ib.lines().count(),
        fb.len()
    ));
    for n in ["db/MAP", "SHA256SUMS"] {
        let (a, b2) = (fs::read(xb.join(n)).unwrap(), fs::read(xr.join(n)).unwrap());
        t.check(&format!("{n} byte-identical"), a == b2, || {
            format!(
                "bash:\n{}\nrust:\n{}",
                String::from_utf8_lossy(&a),
                String::from_utf8_lossy(&b2)
            )
        });
    }
    let (tb, tr) = (tree(&xb), tree(&xr));
    t.check(
        "extracted snapshot dirs identical (bytes + modes)",
        tb == tr,
        || first_diff(&tb, &tr),
    );
    let (eb, er) = (
        tree(&j.root.join("tmp/xb/files-x")),
        tree(&j.root.join("tmp/xr/files-x")),
    );
    t.check("extracted files.tar trees identical", eb == er, || {
        first_diff(&eb, &er)
    });
    say(&format!(
        "   gz size bash {} B, rust {} B",
        fs::metadata(sa).unwrap().len(),
        fs::metadata(ra).unwrap().len()
    ));

    // ---------------------------------------------------- S2 rotation
    say("== S2 rotation KEEP=3 with 14 older archives + a non-matching file");
    for side in ["sh", "rs"] {
        let d = j.host(&format!("/var/backups/{side}-rot"));
        fs::create_dir_all(&d).unwrap();
        for i in 0..14 {
            let p = d.join(format!("galion-2026010{}-{:06}+0100.tar.gz", i % 9, i));
            fs::write(&p, b"old").unwrap();
            let ts = 1_700_000_000 + (i % 7) * 3600; // pairs share an mtime: tie order
            assert!(Command::new("touch")
                .arg("-d")
                .arg(format!("@{ts}"))
                .arg(&p)
                .status()
                .unwrap()
                .success());
        }
        fs::write(d.join("galion-notes.txt"), b"x").unwrap();
        fs::write(d.join(".galion-hidden.tar.gz"), b"x").unwrap();
    }
    let (b, r) = both("rot", &[("GALION_BACKUP_KEEP", "3")]);
    t.check(
        "rotation exit 0 both",
        b.status.code() == Some(0) && r.status.code() == Some(0),
        || show(&b) + &show(&r),
    );
    t.check(
        "rotation log equal (kept=3)",
        norm_log(&b.stdout) == norm_log(&r.stdout)
            && String::from_utf8_lossy(&r.stdout).contains("kept=3"),
        || show(&b) + &show(&r),
    );
    let (nb, nr) = (
        names(&j.host("/var/backups/sh-rot")),
        names(&j.host("/var/backups/rs-rot")),
    );
    // The new archive is normalised away; compare the survivors by name.
    let surv = |d: &str| -> Vec<String> {
        let mut v: Vec<String> = archives(&j.host(d))
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("galion-2026010"))
            .collect();
        v.sort();
        v
    };
    say(&format!(
        "   survivors bash {:?}",
        surv("/var/backups/sh-rot")
    ));
    t.check(
        "same survivors (mtime order, name tie-break)",
        surv("/var/backups/sh-rot") == surv("/var/backups/rs-rot") && nb == nr,
        || {
            format!(
                "{:?} vs {:?}",
                surv("/var/backups/sh-rot"),
                surv("/var/backups/rs-rot")
            )
        },
    );

    // ---------------------------------------------------- S3 lock held
    say("== S3 another run holds DEST/.lock");
    let mut held = Vec::new();
    for side in ["sh", "rs"] {
        let d = j.host(&format!("/var/backups/{side}-lock"));
        fs::create_dir_all(&d).unwrap();
        let f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(d.join(".lock"))
            .unwrap();
        f.lock().unwrap();
        held.push(f);
    }
    let (b, r) = both("lock", &[]);
    t.check(
        "locked: exit 0 both, 'already running', no archive",
        b.status.code() == Some(0)
            && r.status.code() == Some(0)
            && norm_log(&b.stdout) == norm_log(&r.stdout)
            && String::from_utf8_lossy(&r.stdout).contains("already running")
            && archives(&j.host("/var/backups/sh-lock")).is_empty()
            && archives(&j.host("/var/backups/rs-lock")).is_empty(),
        || show(&b) + &show(&r),
    );
    drop(held);

    // ---------------------------------------------------- S4 low disk
    say("== S4 low disk (MINFREE_KB above free space) and non-integer MINFREE_KB");
    let (b, r) = both("low", &[("GALION_BACKUP_MINFREE_KB", "999999999999")]);
    say(&format!(
        "   rust: {}",
        String::from_utf8_lossy(&r.stdout).trim()
    ));
    t.check(
        "low disk: exit 1 both, same log",
        b.status.code() == Some(1)
            && r.status.code() == Some(1)
            && norm_log(&b.stdout) == norm_log(&r.stdout),
        || show(&b) + &show(&r),
    );
    t.check(
        "low disk: DEST created 0700, nothing else",
        mode(&j.host("/var/backups/sh-low")) == 0o700
            && mode(&j.host("/var/backups/rs-low")) == 0o700
            && names(&j.host("/var/backups/sh-low")).is_empty()
            && names(&j.host("/var/backups/rs-low")).is_empty(),
        || {
            format!(
                "{:?} {:?}",
                names(&j.host("/var/backups/sh-low")),
                names(&j.host("/var/backups/rs-low"))
            )
        },
    );
    let (b, r) = both("lowx", &[("GALION_BACKUP_MINFREE_KB", "abc")]);
    t.check(
        "MINFREE_KB=abc: exit 1 both, same log",
        b.status.code() == Some(1)
            && r.status.code() == Some(1)
            && norm_log(&b.stdout) == norm_log(&r.stdout),
        || show(&b) + &show(&r),
    );

    // ---------------------------------------------------- S5 missing DB
    say("== S5 one DB missing (warn, continue)");
    let legacy = j.host("/opt/galion/hermes/state.db");
    fs::rename(&legacy, j.host("/tmp/state.db.away")).unwrap();
    let (b, r) = both("miss", &[]);
    say(&format!(
        "   rust: {}",
        String::from_utf8_lossy(&r.stdout)
            .trim()
            .replace('\n', " | ")
    ));
    t.check(
        "missing db: exit 0 both, same warn + ok lines",
        b.status.code() == Some(0)
            && r.status.code() == Some(0)
            && norm_log(&b.stdout) == norm_log(&r.stdout),
        || show(&b) + &show(&r),
    );
    let (ma, mb) = (
        tar_tv(&["-tzf"], &archives(&j.host("/var/backups/sh-miss"))[0]),
        tar_tv(&["-tzf"], &archives(&j.host("/var/backups/rs-miss"))[0]),
    );
    t.check(
        "missing db: neither archive has it",
        !ma.contains("hermes-legacy") && !mb.contains("hermes-legacy"),
        String::new,
    );
    fs::rename(j.host("/tmp/state.db.away"), &legacy).unwrap();

    // ---------------------------------------------------- S6 corrupt DB
    say("== S6 corrupt DB (snapshot fails: abort, exit 1, cleanup)");
    let rsdb = j.host("/var/lib/hermes-rs/rs-state.db");
    fs::rename(&rsdb, j.host("/tmp/rs.db.away")).unwrap();
    for x in ["-wal", "-shm"] {
        let p = PathBuf::from(format!("{}{x}", rsdb.display()));
        if p.exists() {
            fs::rename(&p, j.host(&format!("/tmp/rs.db{x}.away"))).unwrap();
        }
    }
    fs::write(&rsdb, b"definitely not an sqlite database file, just text").unwrap();
    let (b, r) = both("bad", &[]);
    say(&format!(
        "   rust stdout: {}",
        String::from_utf8_lossy(&r.stdout)
            .trim()
            .replace('\n', " | ")
    ));
    say(&format!(
        "   rust stderr: {}",
        String::from_utf8_lossy(&r.stderr).trim()
    ));
    t.check(
        "corrupt db: exit 1 both, same stdout and stderr",
        b.status.code() == Some(1)
            && r.status.code() == Some(1)
            && norm_log(&b.stdout) == norm_log(&r.stdout)
            && b.stderr == r.stderr,
        || show(&b) + &show(&r),
    );
    t.check(
        "corrupt db: no archive, no .work-* left",
        names(&j.host("/var/backups/sh-bad")) == names(&j.host("/var/backups/rs-bad"))
            && names(&j.host("/var/backups/rs-bad")) == [".lock"],
        || {
            format!(
                "{:?} {:?}",
                names(&j.host("/var/backups/sh-bad")),
                names(&j.host("/var/backups/rs-bad"))
            )
        },
    );
    fs::remove_file(&rsdb).unwrap();
    fs::rename(j.host("/tmp/rs.db.away"), &rsdb).unwrap();
    for x in ["-wal", "-shm"] {
        let a = j.host(&format!("/tmp/rs.db{x}.away"));
        if a.exists() {
            fs::rename(a, format!("{}{x}", rsdb.display())).unwrap();
        }
    }

    // ---------------------------------------------------- S7 SIGTERM
    say("== S7 SIGTERM mid-run (rust): work dir removed, exit 143");
    let big = j.host("/var/lib/hermes-rs/big.bin");
    {
        let f = fs::File::create(&big).unwrap();
        f.set_len(400 * 1024 * 1024).unwrap(); // sparse: reads as zeros
    }
    let dest = "/var/backups/rs-term";
    let child = j
        .cmd(
            &["/opt/galion/rust/bin/galion-backup", "run"],
            &[("GALION_BACKUP_DEST", dest)],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let dh = j.host(dest);
    let t0 = Instant::now();
    let mut saw_work = false;
    while t0.elapsed() < Duration::from_secs(20) {
        if fs::read_dir(&dh)
            .map(|r| {
                r.flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with(".work-"))
            })
            .unwrap_or(false)
            && fs::read_dir(&dh).is_ok()
        {
            saw_work = true;
            std::thread::sleep(Duration::from_millis(300));
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: plain kill(2) of our own child.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let out = child.wait_with_output().unwrap();
    let left = names(&dh);
    say(&format!(
        "   exit={:?} signal={:?} left={left:?}",
        out.status.code(),
        out.status.signal()
    ));
    t.check(
        "SIGTERM: exit 143, only .lock left (no .work-*, no .part, no archive)",
        saw_work && out.status.code() == Some(143) && left == [".lock"],
        || format!("saw_work={saw_work} {}", show(&out)),
    );
    fs::remove_file(&big).unwrap();

    say(&format!("RESULT: {} passed, {} failed", t.pass, t.fail));
    assert_eq!(t.fail, 0);
}
