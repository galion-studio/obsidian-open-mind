// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! `seccomp`: the opt-in deny-list holds without Landlock (the VPS LXC has
//! none): host-control syscalls fail, raw sockets fail, normal work (fork,
//! exec, pipes, a loopback listener in the private netns) still runs.
use openmind_sandbox::*;
use std::path::PathBuf;
use std::time::Duration;

fn wd(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/sandbox-seccomp")
        .join(name);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

const PROBE: &str = r#"
import ctypes, ctypes.util, errno, os, socket, sys
libc = ctypes.CDLL(ctypes.util.find_library("c") or "libc.so.6", use_errno=True)
def tryc(name, f):
    r = f()
    e = ctypes.get_errno()
    print(name + ":", "allowed" if r == 0 else "denied " + errno.errorcode.get(e, str(e)))
print("status:", [l.split()[1] for l in open("/proc/self/status") if l.startswith("Seccomp:")])
tryc("unshare", lambda: libc.unshare(0x10000000))
tryc("ptrace", lambda: libc.ptrace(0, 0, 0, 0))
tryc("mount", lambda: libc.mount(b"none", b"/tmp", b"tmpfs", 0, None))
tryc("setns", lambda: libc.setns(0, 0))
try:
    socket.socket(socket.AF_PACKET, socket.SOCK_RAW); print("packet: allowed")
except OSError as e:
    print("packet: denied", errno.errorcode.get(e.errno))
srv = socket.socket(); srv.bind(("127.0.0.1", int(sys.argv[1]))); srv.listen(1)
c = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=2)
a, _ = srv.accept(); c.sendall(b"ping"); print("loopback:", a.recv(4).decode())
try:
    socket.create_connection((sys.argv[2], 443), timeout=2); print("outside: allowed")
except OSError as e:
    print("outside: denied", e.errno)
print("child:", os.popen("echo forked | cat").read().strip())
"#;

fn python() -> Option<&'static str> {
    ["/usr/bin/python3", "/bin/python3"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

#[test]
fn seccomp_denies_host_control_without_landlock() {
    let Some(py) = python() else {
        eprintln!("skip: no python3");
        return;
    };
    if !features().userns {
        eprintln!("skip: no user namespaces");
        return;
    }
    let mut p = SandboxPolicy::new(wd("deny"));
    p.require_net_isolation = true;
    p.landlock = false;
    p.seccomp = true;
    p.loopback_tcp_ports = vec![18681];
    p.wall_timeout = Duration::from_secs(20);
    p.cpu_secs = 20;
    p.nofile = 256;
    let outside = [1u8, 1, 1, 1].map(|b| b.to_string()).join(".");
    let o = run(
        &[py, "-c", PROBE, "18681", outside.as_str()].map(str::to_string),
        &p,
    );
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    eprintln!("{out}\n{err}\n{:?}", o.enforced);
    assert!(
        matches!(o.status, Status::Exited(0)),
        "{:?} {out} {err}",
        o.status
    );
    assert_eq!(o.layer("seccomp"), Some(&Enforcement::Enforced));
    assert!(!o.enforced("landlock"), "landlock must be off here");
    assert!(out.contains("status: ['2']"), "{out}");
    for l in [
        "unshare: denied EPERM",
        "ptrace: denied EPERM",
        "mount: denied EPERM",
        "setns: denied EPERM",
        "packet: denied EAFNOSUPPORT",
        "loopback: ping",
        "outside: denied",
        "child: forked",
    ] {
        assert!(out.contains(l), "missing {l:?} in {out}");
    }
}

#[test]
fn seccomp_off_is_reported_as_off() {
    let mut p = SandboxPolicy::new(wd("off"));
    p.landlock = false;
    let o = run(&["/bin/true".to_string()], &p);
    assert!(matches!(o.status, Status::Exited(0)), "{:?}", o.status);
    assert!(matches!(
        o.layer("seccomp"),
        Some(Enforcement::NotEnforced(w)) if w.contains("disabled")
    ));
}
