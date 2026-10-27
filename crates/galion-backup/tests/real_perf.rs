// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life performance test (GALION_REAL=1): bash `galion-backup.sh` vs
//! Rust `galion-backup run` on a fake tree SIZED LIKE THE VPS (read-only
//! probe 2026-10-21 03:35 +02:00, see docs/real-tests/backup-perf.md):
//!
//! * hermes legacy `state.db` 40.8 MB (text rows, FTS-like), memory
//!   `instances/galion/vectors.db` 4.0 MB + a live 4.2 MB `-wal` (f32
//!   embedding blobs), `rs-state.db` 0.9 MB, root `vectors.db` 0.13 MB;
//! * `/var/lib/memory-tencentdb`: 49 dirs (one is the excluded `.backup`),
//!   1773 non-DB files (md / jsonl / mmd / json / mdu) with the VPS size
//!   quantiles (p50 737 B, p90 7.7 KB, p99 35 KB, sum 6.28 MB);
//! * the other FILES entries + 7 dns-backup files, so files.tar has the
//!   VPS's 1845 members.
//!
//! Both implementations run in the same `unshare -rm` + chroot jail as
//! tests/real_parity.rs, alternating, after one warm-up each. Per run:
//! wall time, user+sys CPU and peak RSS of the largest process (wait4
//! rusage), and peak RSS summed over the live process tree (sampled from
//! /proc every 5 ms; bash runs tar|gzip concurrently). files.tar and
//! SHA256SUMS are compared between the two archives at full scale.
//!
//! `GALION_REAL=1 cargo test --release -p galion-backup --test real_perf -- --nocapture`
//! Optional: `GALION_PERF_RUNS` (default 5), `GALION_BACKUP_BIN`, `TMPDIR`.

mod common;

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-backup");
const ORACLE_REV: &str = "e43d7ab:deploy/foundation/galion-backup.sh";
const ORACLE_SHA256: &str = "0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4";
const MARK: &str = "GALION_BACKUP_PERF_ROOT";
const VPS_FILES_TAR_MEMBERS: usize = 1845;

fn say(m: &str) {
    println!("[{}] {m}", galion_backup::backup::now_iso());
}

fn sha(b: &[u8]) -> String {
    let mut h = galion_backup::sha256::Sha256::new();
    h.update(b);
    galion_backup::sha256::hex(&h.finish())
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

// Fake tree: common::vps_tree (moved there for real_restore_parity, slot 279).
use common::vps_tree::build_vps_sized_tree;

// ------------------------------------------------------------ outer

#[test]
fn real_bash_vs_rust_perf() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let Some(script) = oracle() else {
        eprintln!("SKIP: oracle {ORACLE_REV} not in this repo (set GALION_BACKUP_ORACLE)");
        return;
    };
    assert_eq!(sha(&script), ORACLE_SHA256, "oracle is the VPS script");
    let f = common::tmp("perf");
    for d in [
        "usr",
        "dev",
        "proc",
        "tmp",
        "var/backups",
        "opt/galion/rust/bin",
        "etc/alternatives",
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
    let bin = std::env::var("GALION_BACKUP_BIN").unwrap_or_else(|_| BIN.to_string());
    say(&format!("rust binary under test: {bin}"));
    fs::copy(&bin, bin_dir.join("galion-backup")).unwrap();
    fs::write(bin_dir.join("galion-backup.sh"), &script).unwrap();
    fs::set_permissions(
        bin_dir.join("galion-backup.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let t0 = Instant::now();
    let held = build_vps_sized_tree(&f);
    say(&format!(
        "fake VPS-sized tree built in {:.1} s under {}",
        t0.elapsed().as_secs_f64(),
        f.display()
    ));
    let me = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .args(["-rm", "--propagation", "private"])
        .arg(&me)
        .args([
            "--exact",
            "real_perf_inner",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MARK, &f)
        .status()
        .expect("unshare");
    drop(held);
    if st.success() {
        let _ = fs::remove_dir_all(&f);
    }
    assert!(
        st.success(),
        "inner perf run failed (tree kept at {})",
        f.display()
    );
}

// ------------------------------------------------------------ inner

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Impl {
    Bash,
    Rust,
}

type Metric = (&'static str, fn(&Sample) -> f64);

#[derive(Debug, Clone, Copy)]
struct Sample {
    wall: f64,
    user: f64,
    sys: f64,
    maxrss_kb: i64,
    tree_peak_kb: u64,
    procs: usize,
}

/// pid -> (ppid, rss KiB) for every process we can read.
fn proc_table() -> HashMap<i32, (i32, u64)> {
    let mut m = HashMap::new();
    let Ok(rd) = fs::read_dir("/proc") else {
        return m;
    };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // comm may contain spaces: fields after the last ')'.
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 2..]) else {
            continue;
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        let (Some(ppid), Some(rss)) = (f.get(1), f.get(21)) else {
            continue;
        };
        let pages: u64 = rss.parse().unwrap_or(0);
        m.insert(pid, (ppid.parse().unwrap_or(0), pages * 4));
    }
    m
}

fn tree_rss(root: i32, seen: &mut std::collections::HashSet<i32>) -> u64 {
    let t = proc_table();
    let mut sum = 0;
    for (&pid, &(_, rss)) in &t {
        let mut p = pid;
        let mut hops = 0;
        while p > 1 && hops < 64 {
            if p == root {
                sum += rss;
                seen.insert(pid);
                break;
            }
            p = t.get(&p).map(|x| x.0).unwrap_or(0);
            hops += 1;
        }
    }
    sum
}

fn jail_cmd(root: &Path, which: Impl, dest: &str, extra: &[(&str, &str)]) -> Command {
    let argv: &[&str] = match which {
        Impl::Bash => &["/bin/bash", "/opt/galion/rust/bin/galion-backup.sh"],
        Impl::Rust => &["/opt/galion/rust/bin/galion-backup", "run"],
    };
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..])
        .env_clear()
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("LANG", "C")
        .env("GALION_BACKUP_DEST", dest)
        .env("GALION_BACKUP_MINFREE_KB", "1024")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (k, v) in extra {
        c.env(k, v);
    }
    let r = std::ffi::CString::new(root.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: only async-signal-safe syscalls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            if libc::chroot(r.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::umask(0o022);
            Ok(())
        });
    }
    c
}

fn measure(root: &Path, which: Impl, dest: &str) -> (Sample, String) {
    measure_env(root, which, dest, &[])
}

fn measure_env(root: &Path, which: Impl, dest: &str, extra: &[(&str, &str)]) -> (Sample, String) {
    let mut c = jail_cmd(root, which, dest, extra);
    let t0 = Instant::now();
    // Reaped below with wait4(2) (std's wait() cannot return the rusage).
    #[allow(clippy::zombie_processes)]
    let mut child = c.spawn().unwrap();
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut s).ok();
        s
    });
    let mut peak = 0u64;
    let mut seen = std::collections::HashSet::new();
    let (status, ru) = loop {
        let mut st = 0i32;
        // SAFETY: zeroed rusage is a valid out-parameter for wait4.
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: wait4 on our own child with valid pointers.
        let rc = unsafe { libc::wait4(pid, &mut st, libc::WNOHANG, &mut ru) };
        if rc == pid {
            break (st, ru);
        }
        assert!(rc == 0, "wait4: {}", std::io::Error::last_os_error());
        peak = peak.max(tree_rss(pid, &mut seen));
        std::thread::sleep(Duration::from_millis(5));
    };
    let wall = t0.elapsed().as_secs_f64();
    let out = reader.join().unwrap();
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "{which:?} failed: status {status} out {out}"
    );
    let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    (
        Sample {
            wall,
            user: tv(ru.ru_utime),
            sys: tv(ru.ru_stime),
            maxrss_kb: ru.ru_maxrss,
            tree_peak_kb: peak,
            procs: seen.len(),
        },
        out,
    )
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn newest(dir: &Path) -> PathBuf {
    galion_backup::backup::archives(dir)
        .into_iter()
        .next()
        .expect("an archive")
}

fn member(a: &Path, name: &str) -> Vec<u8> {
    let o = Command::new("tar")
        .args(["-xzOf"])
        .arg(a)
        .arg(name)
        .output()
        .unwrap();
    assert!(o.status.success(), "tar -xzOf {} {name}", a.display());
    o.stdout
}

fn tar_count(bytes: &[u8]) -> usize {
    let mut c = Command::new("tar")
        .args(["-tf", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut i = c.stdin.take().unwrap();
    let b = bytes.to_vec();
    let wr = std::thread::spawn(move || std::io::Write::write_all(&mut i, &b));
    let o = c.wait_with_output().unwrap();
    wr.join().unwrap().unwrap();
    String::from_utf8_lossy(&o.stdout).lines().count()
}

#[test]
fn real_perf_inner() {
    let Ok(root) = std::env::var(MARK) else {
        return; // only meaningful when re-executed by real_bash_vs_rust_perf
    };
    let root = PathBuf::from(root);
    for (s, d) in [
        ("/usr", "usr"),
        ("/dev", "dev"),
        ("/proc", "proc"),
        ("/etc/alternatives", "etc/alternatives"),
    ] {
        bind(s, &root.join(d));
    }
    let runs: usize = std::env::var("GALION_PERF_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let du = |rel: &str| -> u64 {
        fn walk(p: &Path) -> u64 {
            let md = fs::symlink_metadata(p).unwrap();
            if md.is_dir() {
                fs::read_dir(p)
                    .unwrap()
                    .flatten()
                    .map(|e| walk(&e.path()))
                    .sum()
            } else {
                md.len()
            }
        }
        walk(&root.join(rel))
    };
    say(&format!(
        "tree: memory-tencentdb {} B, hermes state.db {} B, rs-state.db {} B",
        du("var/lib/memory-tencentdb"),
        du("opt/galion/hermes/state.db"),
        du("var/lib/hermes-rs/rs-state.db")
    ));
    for p in [
        "var/lib/memory-tencentdb/instances/galion/vectors.db",
        "var/lib/memory-tencentdb/instances/galion/vectors.db-wal",
        "var/lib/memory-tencentdb/vectors.db",
    ] {
        say(&format!("  {p} {} B", du(p)));
    }

    say("== warm-up (page cache), one run each, not counted");
    measure(&root, Impl::Bash, "/var/backups/warm-sh");
    measure(&root, Impl::Rust, "/var/backups/warm-rs");

    let mut res: Vec<(Impl, Sample)> = Vec::new();
    for i in 0..runs {
        let order = if i % 2 == 0 {
            [Impl::Bash, Impl::Rust]
        } else {
            [Impl::Rust, Impl::Bash]
        };
        for which in order {
            let dest = match which {
                Impl::Bash => "/var/backups/sh",
                Impl::Rust => "/var/backups/rs",
            };
            let (s, out) = measure(&root, which, dest);
            say(&format!(
                "run {i} {which:?}: wall {:.3} s user {:.3} s sys {:.3} s maxrss {} KiB tree-peak {} KiB procs {} | {}",
                s.wall,
                s.user,
                s.sys,
                s.maxrss_kb,
                s.tree_peak_kb,
                s.procs,
                out.trim().rsplit(' ').take(3).collect::<Vec<_>>().join(" ")
            ));
            res.push((which, s));
            // Keep disk use flat: drop all but the newest archive of this side.
            for old in galion_backup::backup::archives(&root.join(&dest[1..]))
                .into_iter()
                .skip(1)
            {
                fs::remove_file(old).unwrap();
            }
        }
    }
    let pick = |w: Impl, f: fn(&Sample) -> f64| -> Vec<f64> {
        res.iter().filter(|r| r.0 == w).map(|r| f(&r.1)).collect()
    };
    let stats = |w: Impl, f: fn(&Sample) -> f64| -> (f64, f64, f64) {
        let v = pick(w, f);
        let mn = v.iter().cloned().fold(f64::MAX, f64::min);
        let mx = v.iter().cloned().fold(f64::MIN, f64::max);
        (median(v), mn, mx)
    };
    let metrics: [Metric; 6] = [
        ("wall s", |s| s.wall),
        ("user s", |s| s.user),
        ("sys s", |s| s.sys),
        ("cpu s", |s| s.user + s.sys),
        ("maxrss MiB", |s| s.maxrss_kb as f64 / 1024.0),
        ("tree-peak MiB", |s| s.tree_peak_kb as f64 / 1024.0),
    ];
    say(&format!("== SUMMARY ({runs} runs each; median [min-max])"));
    for (name, f) in metrics {
        let (bm, bl, bh) = stats(Impl::Bash, f);
        let (rm, rl, rh) = stats(Impl::Rust, f);
        say(&format!(
            "   {name:<14} bash {bm:>8.3} [{bl:.3}-{bh:.3}]   rust {rm:>8.3} [{rl:.3}-{rh:.3}]   rust/bash {:.2}",
            rm / bm
        ));
    }

    // ---- full-scale content check between the last bash and rust archives
    let (a_sh, a_rs) = (
        newest(&root.join("var/backups/sh")),
        newest(&root.join("var/backups/rs")),
    );
    let st = |a: &Path| {
        a.file_name()
            .unwrap()
            .to_string_lossy()
            .trim_end_matches(".tar.gz")
            .to_string()
    };
    let (tsh, trs) = (
        member(&a_sh, &format!("{}/files/files.tar", st(&a_sh))),
        member(&a_rs, &format!("{}/files/files.tar", st(&a_rs))),
    );
    let (ssh, srs) = (
        member(&a_sh, &format!("{}/SHA256SUMS", st(&a_sh))),
        member(&a_rs, &format!("{}/SHA256SUMS", st(&a_rs))),
    );
    let n = tar_count(&trs);
    say(&format!(
        "   gz size bash {} B rust {} B; files.tar {} B, {n} members (VPS: {VPS_FILES_TAR_MEMBERS})",
        fs::metadata(&a_sh).unwrap().len(),
        fs::metadata(&a_rs).unwrap().len(),
        trs.len()
    ));
    let mut fail = 0;
    let mut check = |what: &str, ok: bool| {
        say(&format!("{} {what}", if ok { "PASS" } else { "FAIL" }));
        if !ok {
            fail += 1;
        }
    };
    check("files.tar byte-identical at VPS scale", tsh == trs);
    check("SHA256SUMS byte-identical at VPS scale", ssh == srs);
    check(
        "files.tar member count = VPS (1845)",
        n == VPS_FILES_TAR_MEMBERS,
    );
    let rust_rss = pick(Impl::Rust, |s| s.maxrss_kb as f64)
        .into_iter()
        .fold(0.0, f64::max);
    check("rust peak RSS < 64 MiB", rust_rss < 64.0 * 1024.0);

    // ---- gzip level trade-off (Rust only; GALION_BACKUP_GZIP_LEVEL)
    say("== gzip level sweep (rust, 3 runs each; files.tar must stay identical to bash's)");
    let lruns = runs.clamp(1, 3);
    for lvl in ["1", "3", "5", "6"] {
        let dest = format!("/var/backups/lvl-{lvl}");
        let mut walls = Vec::new();
        let mut cpus = Vec::new();
        for _ in 0..lruns {
            let (s, _) = measure_env(
                &root,
                Impl::Rust,
                &dest,
                &[("GALION_BACKUP_GZIP_LEVEL", lvl)],
            );
            walls.push(s.wall);
            cpus.push(s.user + s.sys);
        }
        let a = newest(&root.join(&dest[1..]));
        let ft = member(&a, &format!("{}/files/files.tar", st(&a)));
        say(&format!(
            "   level {lvl}: wall {:.3} s cpu {:.3} s (median) gz {} B",
            median(walls),
            median(cpus),
            fs::metadata(&a).unwrap().len()
        ));
        check(
            &format!("level {lvl}: files.tar identical to bash's"),
            ft == tsh,
        );
        let _ = fs::remove_dir_all(root.join(&dest[1..]));
    }
    let o = jail_cmd(
        &root,
        Impl::Rust,
        "/var/backups/lvl-bad",
        &[("GALION_BACKUP_GZIP_LEVEL", "10")],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .unwrap();
    say(&format!(
        "   level 10: exit {:?} stderr {:?}",
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).trim()
    ));
    check(
        "GALION_BACKUP_GZIP_LEVEL=10 refused (exit 1, DEST not created)",
        o.status.code() == Some(1) && !root.join("var/backups/lvl-bad").exists(),
    );
    say(&format!("RESULT: {} failed", fail));
    assert_eq!(fail, 0);
}
