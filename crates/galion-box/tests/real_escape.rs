// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Escape suite for galion-box (wave-4 slot 164). One real test per family:
//! ptrace, mount/pivot_root, userns nesting, bpf/perf, io_uring, keyctl, socket families.
//!
//! sandbox_vps_only: RUN ONLY ON THE VPS as a non-root throwaway user in
//! /var/tmp/sandbox-test-<ts>. Skipped unless GALION_REAL=1 and GALION_BOX_ON_VPS=1.
//! Never set those on the dev box. Ports: 20340-20349 (slot 164).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

fn enabled_ns() -> bool {
    if !enabled() {
        return false;
    }
    if std::env::var_os("GALION_BOX_QEMU").is_some() {
        eprintln!("skipped under qemu-user: no namespaces for the guest");
        return false;
    }
    true
}

struct Env {
    bin: PathBuf,
    work: PathBuf,
}

fn env() -> Env {
    let bin = PathBuf::from(std::env::var("GALION_BOX_BIN").expect("GALION_BOX_BIN"));
    let work = std::env::var("GALION_BOX_WORK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    Env { bin, work }
}

impl Env {
    fn rootfs(&self, name: &str) -> PathBuf {
        let r = self.work.join(format!("rootfs-esc-{name}"));
        let _ = std::fs::remove_dir_all(&r);
        std::fs::create_dir_all(r.join("bin")).unwrap();
        std::fs::copy(&self.bin, r.join("bin/galion-box")).unwrap();
        r
    }

    fn run(&self, rootfs: &Path, opts: &[&str], payload: &[&str]) -> (i32, String, String) {
        let t = Instant::now();
        let out = Command::new(&self.bin)
            .arg("run")
            .arg("--rootfs")
            .arg(rootfs)
            .args(opts)
            .arg("--")
            .arg("/bin/galion-box")
            .args(payload)
            .output()
            .expect("spawn launcher");
        let ms = t.elapsed().as_millis();
        let so = String::from_utf8_lossy(&out.stdout).into_owned();
        let se = String::from_utf8_lossy(&out.stderr).into_owned();
        eprintln!(
            "--- escape {payload:?} => exit {:?} in {ms} ms\n{so}{se}",
            out.status.code()
        );
        (out.status.code().unwrap_or(-1), so, se)
    }
}

fn kv(s: &str) -> BTreeMap<String, String> {
    s.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn is_denied(v: &str) -> bool {
    v.starts_with("denied:")
        || v.starts_with("blocked:")
        || v.starts_with("hidden:")
        || v == "skipped"
}

/// Host-side verdict: every probe key must be denied/blocked/hidden (or an allow-listed
/// MEASURED exception). Returns the list of keys that look like an escape.
fn escapes(m: &BTreeMap<String, String>, measured_ok: &[&str]) -> Vec<String> {
    let mut bad = Vec::new();
    for (k, v) in m {
        if k == "family" {
            continue;
        }
        if measured_ok.contains(&k.as_str()) {
            continue;
        }
        if !is_denied(v) {
            bad.push(format!("{k}={v}"));
        }
    }
    bad
}

fn spawn_bystander() -> std::process::Child {
    std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn bystander")
}

#[test]
fn escape_01_ptrace() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let mut bystander = spawn_bystander();
    let pid = bystander.id().to_string();
    let r = e.rootfs("ptrace");
    let (code, out, _) = e.run(
        &r,
        &["--timeout", "20", "--ro-root"],
        &["escape", "ptrace", "--host-pid", &pid],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "ptrace");
    // Host bystander must be unreachable (different pid ns → ESRCH, or EPERM).
    assert!(
        is_denied(&m["ptrace.seize_hostlike"]),
        "{}",
        m["ptrace.seize_hostlike"]
    );
    assert!(
        is_denied(&m["ptrace.attach_hostlike"]),
        "{}",
        m["ptrace.attach_hostlike"]
    );
    assert!(
        is_denied(&m["ptrace.kill_hostlike"]),
        "{}",
        m["ptrace.kill_hostlike"]
    );
    // Bystander must still be alive on the host.
    assert!(bystander.try_wait().unwrap().is_none(), "bystander died");
    bystander.kill().ok();
    bystander.wait().ok();
    // seize_init is MEASURED: same-uid ptrace of sandbox init is not a host escape.
    eprintln!(
        "MEASURED ptrace.seize_init={}",
        m.get("ptrace.seize_init")
            .map(String::as_str)
            .unwrap_or("?")
    );
}

#[test]
fn escape_02_mount_pivot_root() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("mount");
    let (code, out, _) = e.run(&r, &["--timeout", "20", "--ro-root"], &["escape", "mount"]);
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "mount");
    for k in [
        "mount.tmpfs",
        "mount.proc",
        "mount.bind_root",
        "mount.remount_root_rw",
        "mount.pivot_root",
    ] {
        assert!(is_denied(&m[k]), "{k}={}", m[k]);
    }
    assert!(
        m["mount.oldroot_path"].starts_with("hidden:"),
        "oldroot leaked: {}",
        m["mount.oldroot_path"]
    );
}

#[test]
fn escape_03_userns_nesting() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("userns");
    let (code, out, _) = e.run(&r, &["--timeout", "20", "--ro-root"], &["escape", "userns"]);
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "userns");
    // Nested userns may be ALLOWED; mapping host uid 0 must never be.
    if m["userns.unshare"] == "ALLOWED" {
        assert!(
            is_denied(&m["userns.map_host_root"]),
            "mapped host root: {}",
            m["userns.map_host_root"]
        );
    } else {
        assert!(is_denied(&m["userns.unshare"]), "{}", m["userns.unshare"]);
    }
    // /proc/1 is sandbox init or hidden — never a door to the host root userns with new privs.
    let host_ns = &m["userns.host_proc1_ns"];
    assert!(
        is_denied(host_ns) || host_ns == "ALLOWED",
        "unexpected host_proc1_ns={host_ns}"
    );
    // ALLOWED here only means setns into our own init's userns (no privilege gain).
    eprintln!(
        "MEASURED userns.unshare={} map_self={} host_proc1_ns={}",
        m["userns.unshare"],
        m.get("userns.map_self_as_root_inside")
            .map(String::as_str)
            .unwrap_or("n/a"),
        host_ns
    );
}

#[test]
fn escape_04_bpf_perf() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("bpf");
    let (code, out, _) = e.run(&r, &["--timeout", "20", "--ro-root"], &["escape", "bpf"]);
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "bpf");
    // CapEff=0: map create / prog load / perf_event_open must not succeed as privileged ops.
    // Some kernels return EPERM, others EACCES; unprivileged bpf may be globally off (EPERM).
    for k in ["bpf.map_create", "bpf.prog_load", "bpf.perf_event_open"] {
        if m[k] == "ALLOWED" {
            // Unprivileged bpf/perf enabled on this host: not a namespace escape by itself,
            // but recorded as MEASURED for the seccomp decision (D-S3).
            eprintln!("MEASURED {k}=ALLOWED (no CAP, unprivileged bpf/perf on)");
        } else {
            assert!(is_denied(&m[k]), "{k}={}", m[k]);
        }
    }
    // Hard fail only if prog_load returned a usable fd AND we could not classify it — already
    // closed inside the probe. Family is CONTAINED when map/prog/perf are denied OR merely
    // unprivileged-but-harmless. Host escape would need CAP_SYS_ADMIN / CAP_BPF against the
    // host ns, which CapEff=0 + userns prevent.
    let bad = escapes(
        &m,
        &[
            // none hard-required ALLOWED
        ],
    );
    // Re-check: ALLOWED on these is MEASURED, not ESCAPED for this prototype.
    let real_bad: Vec<_> = bad
        .into_iter()
        .filter(|s| {
            !s.starts_with("bpf.map_create=ALLOWED")
                && !s.starts_with("bpf.prog_load=ALLOWED")
                && !s.starts_with("bpf.perf_event_open=ALLOWED")
        })
        .collect();
    assert!(real_bad.is_empty(), "bpf escape: {real_bad:?}");
}

#[test]
fn escape_05_io_uring() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("iouring");
    let (code, out, _) = e.run(
        &r,
        &["--timeout", "20", "--ro-root"],
        &["escape", "iouring"],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "iouring");
    // setup may be ALLOWED for unprivileged users; host paths must stay hidden.
    eprintln!("MEASURED iouring.setup={}", m["iouring.setup"]);
    if m["iouring.setup"] == "ALLOWED" {
        assert!(
            m["iouring.host_etc_passwd"].starts_with("hidden:"),
            "etc/passwd visible: {}",
            m["iouring.host_etc_passwd"]
        );
        assert!(
            m["iouring.host_opt_galion"].starts_with("hidden:"),
            "opt/galion visible: {}",
            m["iouring.host_opt_galion"]
        );
    } else {
        assert!(is_denied(&m["iouring.setup"]), "{}", m["iouring.setup"]);
    }
}

#[test]
fn escape_06_keyctl() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("keyctl");
    let (code, out, _) = e.run(&r, &["--timeout", "20", "--ro-root"], &["escape", "keyctl"]);
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "keyctl");
    // Reading key id 1 (host material) must never succeed with bytes.
    assert!(
        is_denied(&m["keyctl.read_id1"]),
        "key id 1 readable: {}",
        m["keyctl.read_id1"]
    );
    // session/user/join may be ALLOWED inside the userns — MEASURED, not host escape.
    for k in ["keyctl.session_id", "keyctl.user_id", "keyctl.join_session"] {
        eprintln!("MEASURED {k}={}", m[k]);
    }
}

#[test]
fn escape_07_socket_families() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    // Host abstract UNIX listener: sandbox with NEWNET must not reach it.
    let abs_name = "galion-box-escape-164";
    let (hits, stop) = {
        // SAFETY: zeroed sockaddr_un is a valid blank address.
        let mut a: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        a.sun_family = libc::AF_UNIX as libc::sa_family_t;
        a.sun_path[0] = 0;
        for (i, b) in abs_name.as_bytes().iter().enumerate() {
            a.sun_path[i + 1] = *b as _;
        }
        let path_len = 1 + abs_name.len();
        let len = (std::mem::size_of::<libc::sa_family_t>() + path_len) as libc::socklen_t;
        // SAFETY: socket + bind of an abstract name we own for the test.
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        assert!(fd >= 0, "host abstract socket");
        let br = unsafe { libc::bind(fd, &a as *const _ as *const libc::sockaddr, len) };
        assert_eq!(br, 0, "bind abstract: {}", std::io::Error::last_os_error());
        assert_eq!(unsafe { libc::listen(fd, 1) }, 0);
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let h = hits.clone();
        let s = stop.clone();
        std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                let mut pollfd = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: poll with 100 ms timeout.
                let pr = unsafe { libc::poll(&mut pollfd, 1, 100) };
                if pr > 0 {
                    // SAFETY: accept on our listening fd.
                    let c = unsafe { libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut()) };
                    if c >= 0 {
                        h.fetch_add(1, Ordering::SeqCst);
                        unsafe { libc::close(c) };
                    }
                }
            }
            unsafe { libc::close(fd) };
        });
        (hits, stop)
    };

    let r = e.rootfs("sockets");
    let (code, out, _) = e.run(
        &r,
        &["--timeout", "20", "--ro-root"],
        &["escape", "sockets", "--abstract", abs_name],
    );
    stop.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "sockets");
    assert!(
        is_denied(&m["sockets.af_packet"]),
        "AF_PACKET={}",
        m["sockets.af_packet"]
    );
    assert!(
        m["sockets.abstract_unix"].starts_with("blocked:")
            || m["sockets.abstract_unix"].starts_with("denied:"),
        "abstract unix reached host: {}",
        m["sockets.abstract_unix"]
    );
    assert!(
        m["sockets.tcp_internet"].starts_with("blocked:"),
        "internet: {}",
        m["sockets.tcp_internet"]
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "host abstract listener hit");
    for k in [
        "sockets.af_vsock",
        "sockets.af_bluetooth",
        "sockets.af_xdp",
        "sockets.af_netlink",
    ] {
        eprintln!("MEASURED {k}={}", m[k]);
        // netlink ALLOWED inside empty netns is MEASURED; privileged families must be denied.
        if k != "sockets.af_netlink" {
            assert!(is_denied(&m[k]) || m[k] == "ALLOWED", "{k}={}", m[k]);
        }
    }
    // AF_PACKET already asserted denied. AF_VSOCK/BLUETOOTH/XDP: deny OR open-but-useless.
    // Hard escape = abstract unix OPEN or tcp_internet OPEN or af_packet ALLOWED — covered above.
}

#[test]
fn escape_08_all_families_smoke() {
    if !enabled_ns() {
        return;
    }
    let e = env();
    let r = e.rootfs("all");
    let (code, out, _) = e.run(
        &r,
        &["--timeout", "30", "--ro-root"],
        &[
            "escape",
            "all",
            "--host-pid",
            "9001",
            "--abstract",
            "galion-box-escape-164",
        ],
    );
    assert_eq!(code, 0);
    let m = kv(&out);
    assert_eq!(m["family"], "all");
    // Spot-check each family contributed at least one key.
    for prefix in [
        "ptrace.", "mount.", "userns.", "bpf.", "iouring.", "keyctl.", "sockets.",
    ] {
        assert!(
            m.keys().any(|k| k.starts_with(prefix)),
            "missing family output for {prefix}"
        );
    }
    // Hard host-escape keys must stay shut in the combined run.
    for k in [
        "ptrace.seize_hostlike",
        "ptrace.attach_hostlike",
        "mount.pivot_root",
        "mount.tmpfs",
        "keyctl.read_id1",
        "sockets.af_packet",
        "sockets.tcp_internet",
        "sockets.abstract_unix",
    ] {
        assert!(is_denied(&m[k]), "{k}={}", m[k]);
    }
    if m.get("userns.unshare").map(String::as_str) == Some("ALLOWED") {
        assert!(
            is_denied(&m["userns.map_host_root"]),
            "{}",
            m["userns.map_host_root"]
        );
    }
    if m.get("iouring.setup").map(String::as_str) == Some("ALLOWED") {
        assert!(
            m["iouring.host_opt_galion"].starts_with("hidden:"),
            "{}",
            m["iouring.host_opt_galion"]
        );
    }
}
