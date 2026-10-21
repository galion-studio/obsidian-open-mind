// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life sandbox tests for galion-box. sandbox_vps_only: these RUN ONLY ON THE VPS as an
//! ordinary non-root user inside /var/tmp/sandbox-test-<ts>. They are skipped unless both
//! GALION_REAL=1 and GALION_BOX_ON_VPS=1 are set. Never set them on the dev box.
//!
//! Env:
//!   GALION_BOX_BIN    launcher binary (static musl, x86_64 or aarch64)
//!   GALION_BOX_QEMU   optional qemu-aarch64-static: run the aarch64 launcher AND payload under it
//!   GALION_BOX_WORK   work dir (default: current dir)
//!
//! Run single-threaded with --nocapture: ports 20141-20143 (slot 144 range 20140-20149).

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn enabled() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1")
        && std::env::var("GALION_BOX_ON_VPS").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: needs GALION_REAL=1 GALION_BOX_ON_VPS=1 (VPS only, sandbox_vps_only)");
    }
    on
}

/// Tests that need a working namespace sandbox. qemu-user cannot give its guest one
/// (clone with CLONE_NEW* flags -> EINVAL from qemu; unshare(CLONE_NEWUSER) -> EINVAL because
/// qemu's own RCU helper thread makes the process multi-threaded), so under GALION_BOX_QEMU
/// these are skipped and real_10 asserts the launcher fails closed instead.
fn enabled_ns() -> bool {
    if !enabled() {
        return false;
    }
    if std::env::var_os("GALION_BOX_QEMU").is_some() {
        eprintln!("skipped under qemu-user: no namespaces for the guest (see real_10_fail_closed_under_qemu)");
        return false;
    }
    true
}

struct Env {
    bin: PathBuf,
    qemu: Option<PathBuf>,
    work: PathBuf,
}

fn env() -> Env {
    let bin = PathBuf::from(std::env::var("GALION_BOX_BIN").expect("GALION_BOX_BIN"));
    let qemu = std::env::var("GALION_BOX_QEMU").ok().map(PathBuf::from);
    let work = std::env::var("GALION_BOX_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    Env { bin, qemu, work }
}

impl Env {
    /// Fresh rootfs containing only /bin/galion-box (+ /qemu under emulation).
    fn rootfs(&self, name: &str) -> PathBuf {
        let r = self.work.join(format!("rootfs-{name}"));
        let _ = std::fs::remove_dir_all(&r);
        std::fs::create_dir_all(r.join("bin")).unwrap();
        std::fs::copy(&self.bin, r.join("bin/galion-box")).unwrap();
        if let Some(q) = &self.qemu {
            std::fs::copy(q, r.join("qemu")).unwrap();
        }
        r
    }

    fn payload(&self, args: &[&str]) -> Vec<String> {
        let mut v = Vec::new();
        if self.qemu.is_some() {
            v.push("/qemu".to_string());
        }
        v.push("/bin/galion-box".to_string());
        v.extend(args.iter().map(|s| s.to_string()));
        v
    }

    fn launcher(&self) -> Command {
        match &self.qemu {
            Some(q) => {
                let mut c = Command::new(q);
                c.arg(&self.bin);
                c
            }
            None => Command::new(&self.bin),
        }
    }

    /// galion-box run OPTS -- payload; returns (exit code, stdout, stderr, wall ms).
    fn run(&self, rootfs: &Path, opts: &[&str], payload: &[&str]) -> (i32, String, String, u128) {
        let t = Instant::now();
        let out = self
            .launcher()
            .arg("run")
            .arg("--rootfs")
            .arg(rootfs)
            .args(opts)
            .arg("--")
            .args(self.payload(payload))
            .output()
            .expect("spawn launcher");
        let ms = t.elapsed().as_millis();
        let so = String::from_utf8_lossy(&out.stdout).into_owned();
        let se = String::from_utf8_lossy(&out.stderr).into_owned();
        eprintln!(
            "--- run {opts:?} -- {payload:?} => exit {:?} in {ms} ms\n{so}{se}",
            out.status.code()
        );
        (out.status.code().unwrap_or(-1), so, se, ms)
    }
}

fn kv(s: &str) -> BTreeMap<String, String> {
    s.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Host listener counting accepted connections (the sandbox must never reach it).
fn listener(port: u16) -> Arc<AtomicUsize> {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind test port");
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            h.fetch_add(1, Ordering::SeqCst);
            let mut s = s;
            let mut b = [0u8; 64];
            let _ = s.read(&mut b);
        }
    });
    hits
}

/// Processes of this uid whose cmdline mentions galion-box (leftovers after a run).
fn leftovers() -> Vec<String> {
    // SAFETY: trivial getter.
    let me = unsafe { libc::getuid() };
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(st) = std::fs::read_to_string(e.path().join("status")) else {
            continue;
        };
        let uid_ok = st
            .lines()
            .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some(&me.to_string()));
        let cmd = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        let cmd = String::from_utf8_lossy(&cmd).replace('\0', " ");
        if uid_ok && cmd.contains("galion-box") && !cmd.contains("real_vps") {
            v.push(format!("{name}: {cmd}"));
        }
    }
    v
}

#[test]
fn real_01_probe_reports_userns() {
    if !enabled() {
        return;
    }
    let e = env();
    let out = e.launcher().arg("probe").output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    eprintln!("--- probe\n{s}");
    if e.qemu.is_some() {
        assert!(s.contains("verdict=emulated"), "{s}");
    } else {
        assert!(s.contains("verdict=userns"), "{s}");
    }
}

#[test]
fn real_02_default_isolation() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let hits = listener(20141);
    let r = e.rootfs("default");
    let (code, out, _, _) = e.run(
        &r,
        &["--timeout", "30"],
        &[
            "inside-check",
            "--connect",
            "1.1.1.1:443",
            "--connect",
            "127.0.0.1:20141",
            "--connect",
            "1.0.0.1:80",
            "--path",
            "/opt/galion",
            "--path",
            "/opt/galion/galion-light/publish/obsidian/link.env",
            "--path",
            "/etc/passwd",
            "--path",
            "/root",
            "--path",
            "/home",
            "--path",
            "/var/tmp",
            "--path",
            "/usr",
        ],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["uid"], "1000");
    assert_eq!(m["euid"], "1000");
    assert_eq!(m["gid"], "1000");
    assert_eq!(m["pid"], "2", "payload is pid 2 under its own init");
    assert_eq!(m["ppid"], "1");
    assert_eq!(m["hostname"], "galion-box");
    assert_eq!(m["cap_eff"], "0000000000000000");
    assert_eq!(m["no_new_privs"], "1");
    assert_eq!(m["env_count"], "4", "host env must not leak");
    match m["proc_state"].as_str() {
        "fresh" => assert!(
            m["proc_pids"].parse::<u32>().unwrap() <= 3,
            "sees host pids: {}",
            m["proc_pids"]
        ),
        "hidden" => assert_eq!(m["proc_pids"], "0", "hidden /proc must be empty"),
        o => panic!("proc_state {o}"),
    }
    assert_eq!(m["interfaces"], "lo", "only the namespace's own lo");
    assert!(m["setuid0"].starts_with("denied"));
    assert!(m["mount_tmpfs"].starts_with("denied"));
    assert!(m["chroot"].starts_with("denied"));
    assert_eq!(m["write_tmp"], "ALLOWED");
    for k in [
        "connect:1.1.1.1:443",
        "connect:127.0.0.1:20141",
        "connect:1.0.0.1:80",
    ] {
        assert!(m[k].starts_with("blocked"), "{k}={}", m[k]);
    }
    for k in [
        "/opt/galion",
        "/opt/galion/galion-light/publish/obsidian/link.env",
        "/etc/passwd",
        "/root",
        "/home",
        "/var/tmp",
        "/usr",
    ] {
        assert!(m[&format!("path:{k}")].starts_with("hidden"), "{k} visible");
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(hits.load(Ordering::SeqCst), 0, "host listener was reached");
    assert!(leftovers().is_empty(), "{:?}", leftovers());
}

#[test]
fn real_03_ro_root_and_ro_bind() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let shared = e.work.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("marker"), "hello").unwrap();
    let r = e.rootfs("ro");
    let bind = format!("{}:/data", shared.display());
    let (code, out, _, _) = e.run(
        &r,
        &["--ro-root", "--bind-ro", &bind],
        &[
            "inside-check",
            "--path",
            "/data/marker",
            "--write",
            "/data/new",
            "--write",
            "/bin/evil",
        ],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["path:/data/marker"], "VISIBLE");
    assert!(m["write_root"].contains("EROFS"), "{}", m["write_root"]);
    assert!(
        m["write:/data/new"].contains("EROFS"),
        "{}",
        m["write:/data/new"]
    );
    assert!(
        m["write:/bin/evil"].contains("EROFS"),
        "{}",
        m["write:/bin/evil"]
    );
    assert_eq!(m["write_tmp"], "ALLOWED", "/tmp stays a private tmpfs");
    assert!(!shared.join("new").exists());
    assert!(!r.join("galion-box-write-test").exists());
}

#[test]
fn real_04_net_lo_is_private() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let hits = listener(20142);
    let r = e.rootfs("lo");
    let (code, out, _, _) = e.run(
        &r,
        &["--net", "lo"],
        &[
            "inside-check",
            "--connect",
            "127.0.0.1:20142",
            "--connect",
            "1.1.1.1:443",
        ],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["interfaces"], "lo");
    assert_eq!(
        m["connect:127.0.0.1:20142"], "blocked:ECONNREFUSED",
        "sandbox lo is its own"
    );
    assert!(m["connect:1.1.1.1:443"].starts_with("blocked"));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn real_05_timeout_kills_whole_tree() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("timeout");
    let (code, _, _, ms) = e.run(&r, &["--timeout", "1.5"], &["sleep", "3600"]);
    assert_eq!(code, 124);
    assert!(ms < 6000, "timeout took {ms} ms");
    std::thread::sleep(Duration::from_millis(500));
    let left = leftovers();
    assert!(left.is_empty(), "leftover processes: {left:?}");
}

#[test]
fn real_06_nproc_limit_contains_forkbomb() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("nproc");
    let (code, out, _, _) = e.run(&r, &["--nproc", "16", "--timeout", "30"], &["spawn", "200"]);
    assert_eq!(code, 0);
    let m = kv(&out);
    let ok: u32 = m["spawn_ok"].parse().unwrap();
    assert!(ok < 16, "forked {ok} with nproc 16");
    assert_eq!(m["spawn_first_err"], "EAGAIN");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(leftovers().is_empty());
}

#[test]
fn real_07_memory_limit() {
    if !enabled() {
        return;
    }
    let e = env();
    if e.qemu.is_some() {
        eprintln!("skipped under qemu: RLIMIT_AS also caps the emulator's own guest reservation");
        return;
    }
    let r = e.rootfs("as");
    let (_, out, _, _) = e.run(&r, &["--as-mb", "256"], &["alloc", "512"]);
    assert_eq!(kv(&out)["alloc"], "denied:ENOMEM");
    let (_, out, _, _) = e.run(&r, &["--as-mb", "256"], &["alloc", "32"]);
    assert_eq!(kv(&out)["alloc"], "ALLOWED");
}

#[test]
fn real_08_exit_codes_and_refusals() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("codes");
    assert_eq!(e.run(&r, &[], &["exit", "7"]).0, 7);
    // Missing payload binary: exec fails inside, launcher reports 127 (qemu reports its own error code).
    let (code, _, _, _) = {
        let t = Instant::now();
        let o = e
            .launcher()
            .args(["run", "--rootfs"])
            .arg(&r)
            .args(["--", "/bin/nope"])
            .output()
            .unwrap();
        (o.status.code().unwrap_or(-1), (), (), t.elapsed())
    };
    assert_eq!(code, 127);
}

#[test]
fn real_08b_policy_refusals() {
    if !enabled() {
        return;
    }
    let e = env();
    let r = e.rootfs("refuse");
    let o = e
        .launcher()
        .args(["run", "--rootfs", "/", "--", "/bin/sh"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "rootfs / must be refused");
    let o = e
        .launcher()
        .args(["run", "--rootfs"])
        .arg(&r)
        .args(["--uid", "0", "--", "/bin/galion-box"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "uid 0 must be refused");
}

#[test]
fn real_09_startup_overhead() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("bench");
    let mut v: Vec<u128> = (0..20)
        .map(|_| {
            let (code, _, _, ms) = e.run(&r, &[], &["exit", "0"]);
            assert_eq!(code, 0);
            ms
        })
        .collect();
    v.sort();
    eprintln!(
        "STARTUP_MS n=20 min={} median={} p90={} max={}",
        v[0], v[10], v[18], v[19]
    );
    assert!(v[10] < 2000, "median start {} ms", v[10]);
}

#[test]
fn real_10_fail_closed_under_qemu() {
    if !enabled() {
        return;
    }
    let e = env();
    let Some(q) = e.qemu.clone() else {
        eprintln!("skipped: only meaningful under GALION_BOX_QEMU");
        return;
    };
    // If the launcher ever fell back to running the payload WITHOUT a sandbox, this host-path
    // payload would run and create the marker. It must not.
    let marker = e.work.join("UNSANDBOXED-RUN-MARKER");
    let _ = std::fs::remove_file(&marker);
    let r = e.rootfs("failclosed");
    let o = e
        .launcher()
        .args(["run", "--rootfs"])
        .arg(&r)
        .arg("--")
        .arg(&q)
        .arg(&e.bin)
        .args(["inside-check", "--write"])
        .arg(&marker)
        .output()
        .unwrap();
    let se = String::from_utf8_lossy(&o.stderr);
    eprintln!("--- fail-closed: exit {:?} stderr: {se}", o.status.code());
    assert_eq!(o.status.code(), Some(125));
    assert!(se.contains("clone(namespaces)"), "{se}");
    assert!(!marker.exists(), "payload ran without a sandbox");
    assert!(o.stdout.is_empty(), "payload printed output");
}
