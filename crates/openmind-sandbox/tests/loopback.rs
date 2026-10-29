// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! `loopback_tcp_ports`: a live-style service boot inside the sandbox may use
//! listed loopback ports only, in a private netns; nothing else.
use openmind_sandbox::*;
use std::path::PathBuf;
use std::time::Duration;

fn wd(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/sandbox-loopback")
        .join(name);
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

const PROBE: &str = r#"
import socket, sys
ok, other = int(sys.argv[1]), int(sys.argv[2])
srv = socket.socket(); srv.bind(("127.0.0.1", ok)); srv.listen(1)
c = socket.create_connection(("127.0.0.1", ok), timeout=2)
a, _ = srv.accept(); c.sendall(b"ping"); print("listed:", a.recv(4).decode())
try:
    socket.socket().bind(("127.0.0.1", other)); print("other-bind: allowed")
except OSError as e:
    print("other-bind: denied", e.errno)
try:
    socket.create_connection((sys.argv[3], 443), timeout=2); print("outside: allowed")
except OSError as e:
    print("outside: denied", e.errno)
"#;

fn python() -> Option<&'static str> {
    ["/usr/bin/python3", "/bin/python3"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
}

#[test]
fn loopback_ports_allow_listed_port_only() {
    let Some(py) = python() else {
        eprintln!("skip: no python3");
        return;
    };
    if !features().userns {
        eprintln!("skip: no user namespaces");
        return;
    }
    let mut p = SandboxPolicy::new(wd("listed"));
    p.require_net_isolation = true;
    p.loopback_tcp_ports = vec![18671];
    p.wall_timeout = Duration::from_secs(20);
    p.cpu_secs = 20;
    let outside = [1u8, 1, 1, 1].map(|b| b.to_string()).join(".");
    let o = run(
        &[py, "-c", PROBE, "18671", "18672", outside.as_str()].map(str::to_string),
        &p,
    );
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(
        matches!(o.status, Status::Exited(0)),
        "{:?} {out} {err}",
        o.status
    );
    assert!(out.contains("listed: ping"), "{out}");
    assert!(out.contains("outside: denied"), "{out}");
    if landlock_abi().is_some_and(|a| a >= 4) {
        assert!(out.contains("other-bind: denied"), "{out}");
    }
}

#[test]
fn no_ports_means_no_loopback_listener() {
    let Some(py) = python() else {
        return;
    };
    if landlock_abi().is_none_or(|a| a < 4) {
        return;
    }
    let mut p = SandboxPolicy::new(wd("none"));
    p.require_net_isolation = true;
    p.wall_timeout = Duration::from_secs(20);
    let o = run(
        &[
            py,
            "-c",
            "import socket; socket.socket().bind(('127.0.0.1', 18673))",
        ]
        .map(str::to_string),
        &p,
    );
    assert!(!matches!(o.status, Status::Exited(0)), "{:?}", o.status);
}
