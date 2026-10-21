// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Box-side real test of the galion-admin binary (GALION_REAL=1). Uses examples/fake_box as the
//! launcher: NO sandbox / namespace is ever created on the dev box (sandbox_vps_only). The real
//! galion-box lifecycle is proven by tests/real_vps.rs on the VPS. Ports: 20350-20359 (slot 165).

mod common;
use common::*;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn on() -> bool {
    let ok = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !ok {
        eprintln!("skipped: set GALION_REAL=1");
    }
    ok
}

fn setup(name: &str) -> (Admin, PathBuf) {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_galion-admin"));
    let fake = bin.parent().unwrap().join("examples/fake_box");
    assert!(fake.is_file(), "build examples first: {}", fake.display());
    let base = fresh_dir(
        &PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        &format!("real-cli-{name}"),
    );
    let rootfs = base.join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    (
        Admin {
            bin,
            box_bin: fake,
            state: base.join("state"),
        },
        rootfs,
    )
}

fn fake_pid(rootfs: &std::path::Path) -> u32 {
    let p = rootfs.join("fake-box.pid");
    assert!(wait_until(Duration::from_secs(3), || p.exists()));
    std::fs::read_to_string(p).unwrap().trim().parse().unwrap()
}

#[test]
fn r01_version_help_usage() {
    if !on() {
        return;
    }
    let (a, _) = setup("r01");
    let (c, out, _) = a.run(&["version"]);
    assert_eq!(c, 0);
    assert!(out.starts_with("galion-admin 0.1.0 ("), "{out}");
    let (c, out, _) = a.run(&["help"]);
    assert_eq!(c, 0);
    assert!(out.contains("never uses su/root"));
    let (c, _, err) = a.run(&["sandbox", "start", "x", "--rootfs", "/", "--", "sh"]);
    assert_eq!(c, 2, "{err}");
    let (c, _, _) = a.run(&["nope"]);
    assert_eq!(c, 2);
    eprintln!("[{}] r01 ok", stamp());
}

#[test]
fn r02_start_status_stop_audit() {
    if !on() {
        return;
    }
    let (a, r) = setup("r02");
    let rs = r.to_str().unwrap();
    let t = Instant::now();
    let (c, out, err) = a.run(&[
        "sandbox",
        "start",
        "w1",
        "--rootfs",
        rs,
        "--ro-root",
        "--",
        "sleep",
        "60",
    ]);
    assert_eq!(c, 0, "{out}{err}");
    let start_ms = t.elapsed().as_millis();
    assert!(out.contains("sandbox w1 started: box pid"), "{out}");
    let pid = fake_pid(&r);
    assert!(pid_alive(pid));
    assert_eq!(a.field("w1", "box_pid"), pid.to_string());
    let args = std::fs::read_to_string(r.join("fake-box.args")).unwrap();
    assert_eq!(
        args.replace('\n', " "),
        format!("run --rootfs {rs} --net none --ro-root -- sleep 60")
    );
    let (c, out, _) = a.run(&["sandbox", "status", "w1"]);
    assert_eq!(c, 0);
    assert!(out.contains("running") && out.contains("+02:00"), "{out}");
    let (c, out, _) = a.run(&[
        "sandbox", "start", "w1", "--rootfs", rs, "--", "sleep", "60",
    ]);
    assert_eq!(c, 1, "second start must be refused: {out}");
    let t = Instant::now();
    let (c, out, err) = a.run(&["sandbox", "stop", "w1"]);
    assert_eq!(c, 0, "{out}{err}");
    let stop_ms = t.elapsed().as_millis();
    assert!(
        out.contains("SIGTERM") && out.contains("signal:15"),
        "{out}"
    );
    assert!(!pid_alive(pid), "fake box still alive");
    let (c, out, _) = a.run(&["sandbox", "status", "w1"]);
    assert_eq!(c, 4);
    assert!(out.contains("exited   signal:15"), "{out}");
    let (c, out, _) = a.run(&["sandbox", "stop", "w1"]);
    assert_eq!(c, 4, "{out}");
    let au = a.audit();
    let actions: Vec<&str> = au
        .lines()
        .map(|l| {
            l.split("\"action\":\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(
        actions,
        ["sandbox.start", "sandbox.exit", "sandbox.stop"],
        "{au}"
    );
    let (c, out, _) = a.run(&["audit", "verify"]);
    assert_eq!(c, 0);
    assert!(out.starts_with("audit ok: 3 records, head "), "{out}");
    eprintln!(
        "[{}] r02 ok start {start_ms} ms (settle 500) stop {stop_ms} ms; audit: {actions:?}",
        stamp()
    );
}

#[test]
fn r03_early_failure_and_quick_success() {
    if !on() {
        return;
    }
    let (a, r) = setup("r03");
    let rs = r.to_str().unwrap();
    let (c, _, err) = a.run(&["sandbox", "start", "f1", "--rootfs", rs, "--", "exit", "3"]);
    assert_eq!(c, 1);
    assert!(err.contains("failed to start: box exit:3"), "{err}");
    let (c, out, _) = a.run(&["sandbox", "status", "f1"]);
    assert_eq!(c, 4);
    assert!(out.contains("exited   exit:3"), "{out}");
    let (c, out, err) = a.run(&["sandbox", "start", "q1", "--rootfs", rs, "--", "exit", "0"]);
    assert_eq!(c, 0, "{err}");
    assert!(out.contains("finished"), "{out}");
    // missing launcher binary
    let b = Admin {
        bin: a.bin.clone(),
        box_bin: "/nonexistent/galion-box".into(),
        state: a.state.clone(),
    };
    let (c, _, err) = b.run(&["sandbox", "start", "m1", "--rootfs", rs, "--", "sleep", "1"]);
    assert_eq!(c, 1);
    assert!(err.contains("spawn-error"), "{err}");
    let au = a.audit();
    assert!(
        au.contains("\"target\":\"m1\",\"result\":\"failed\""),
        "{au}"
    );
    assert_eq!(a.run(&["audit", "verify"]).0, 0);
    eprintln!("[{}] r03 ok", stamp());
}

#[test]
fn r04_sigkill_after_grace() {
    if !on() {
        return;
    }
    let (a, r) = setup("r04");
    let rs = r.to_str().unwrap();
    let (c, _, err) = a.run(&[
        "sandbox",
        "start",
        "stubborn",
        "--rootfs",
        rs,
        "--",
        "ignore-term",
        "60",
    ]);
    assert_eq!(c, 0, "{err}");
    let pid = fake_pid(&r);
    let t = Instant::now();
    let (c, out, err) = a.run(&["sandbox", "stop", "stubborn", "--grace", "1"]);
    assert_eq!(c, 0, "{err}");
    assert!(
        out.contains("SIGKILL after grace") && out.contains("signal:9"),
        "{out}"
    );
    let ms = t.elapsed().as_millis();
    assert!((1000..4000).contains(&ms), "{ms}");
    assert!(!pid_alive(pid));
    eprintln!(
        "[{}] r04 ok SIGTERM ignored -> SIGKILL, stop took {ms} ms",
        stamp()
    );
}

#[test]
fn r05_supervisor_death_kills_box() {
    if !on() {
        return;
    }
    let (a, r) = setup("r05");
    let rs = r.to_str().unwrap();
    assert_eq!(
        a.run(&["sandbox", "start", "orphan", "--rootfs", rs, "--", "sleep", "60"])
            .0,
        0
    );
    let pid = fake_pid(&r);
    let sup: u32 = a.field("orphan", "supervisor_pid").parse().unwrap();
    // SAFETY: SIGKILL to our own supervisor process.
    unsafe { libc::kill(sup as i32, libc::SIGKILL) };
    assert!(
        wait_until(Duration::from_secs(3), || !pid_alive(pid)),
        "box survived its supervisor"
    );
    let (c, out, _) = a.run(&["sandbox", "status", "orphan"]);
    assert_eq!(c, 4);
    assert!(
        out.contains("lost") && out.contains("supervisor died"),
        "{out}"
    );
    assert!(a.audit().contains(
        "\"action\":\"sandbox.lost\",\"target\":\"orphan\",\"result\":\"supervisor-gone\""
    ));
    let n_lost = a.audit().matches("sandbox.lost").count();
    let _ = a.run(&["sandbox", "status"]);
    assert_eq!(
        a.audit().matches("sandbox.lost").count(),
        n_lost,
        "lost recorded once"
    );
    // a new start under the same name works (lock released by the kernel)
    assert_eq!(
        a.run(&["sandbox", "start", "orphan", "--rootfs", rs, "--", "sleep", "60"])
            .0,
        0
    );
    assert_eq!(a.run(&["sandbox", "stop", "orphan"]).0, 0);
    eprintln!(
        "[{}] r05 ok PDEATHSIG: box died with its supervisor",
        stamp()
    );
}

#[test]
fn r06_secrets_never_logged_and_private_files() {
    if !on() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let (a, r) = setup("r06");
    let rs = r.to_str().unwrap();
    let (c, _, _) = a.run(&[
        "sandbox",
        "start",
        "s1",
        "--rootfs",
        rs,
        "--",
        "exit",
        "0",
        "--token=hunter2-SECRET",
    ]);
    assert_eq!(c, 0);
    let au = a.audit();
    assert!(
        !au.contains("SECRET") && au.contains("cmd=exit argc=3"),
        "{au}"
    );
    assert!(!a.state_file("s1").contains("SECRET"));
    for p in ["", "sandboxes"] {
        let m = std::fs::metadata(a.state.join(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o700, "{p}");
    }
    for p in [
        "audit.jsonl",
        "sandboxes/s1.state",
        "sandboxes/s1.log",
        "sandboxes/s1.lock",
    ] {
        let m = std::fs::metadata(a.state.join(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o600, "{p}");
    }
    // a world-writable state dir is refused
    std::fs::set_permissions(&a.state, std::fs::Permissions::from_mode(0o777)).unwrap();
    let (c, _, err) = a.run(&["sandbox", "start", "s2", "--rootfs", rs, "--", "exit", "0"]);
    assert_eq!(c, 1);
    assert!(err.contains("world-writable"), "{err}");
    std::fs::set_permissions(&a.state, std::fs::Permissions::from_mode(0o700)).unwrap();
    eprintln!("[{}] r06 ok", stamp());
}

#[test]
fn r07_tunnel_status() {
    if !on() {
        return;
    }
    let (a, _) = setup("r07");
    let l = TcpListener::bind("127.0.0.1:20350").expect("slot port 20350");
    std::fs::create_dir_all(&a.state).unwrap();
    let cfg = a.state.join("tunnel.conf");
    std::fs::write(
        &cfg,
        "# test\nadb = 127.0.0.1:20350\nssh_rev = 127.0.0.1:20351\n",
    )
    .unwrap();
    let (c, out, _) = a.run(&["tunnel", "status"]);
    assert_eq!(c, 4, "{out}");
    assert!(
        out.contains("adb") && out.contains(" up ") && out.contains("down  refused"),
        "{out}"
    );
    assert!(out.contains("2 tunnel(s), 1 up, 1 down"));
    // the probe connected exactly once and sent nothing
    l.set_nonblocking(false).unwrap();
    let (mut s, _) = l.accept().unwrap();
    s.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut buf = [0u8; 16];
    use std::io::Read;
    assert_eq!(s.read(&mut buf).unwrap_or(0), 0, "probe must send no bytes");
    std::fs::write(&cfg, "adb = 127.0.0.1:20350\n").unwrap();
    let (c, out, _) = a.run(&["tunnel", "status"]);
    assert_eq!(c, 0, "{out}");
    std::fs::write(&cfg, "leak = 1.1.1.1:443\n").unwrap();
    let (c, _, err) = a.run(&["tunnel", "status"]);
    assert_eq!(c, 1);
    assert!(err.contains("not loopback"), "{err}");
    std::fs::write(&cfg, "dns = localhost:22\n").unwrap();
    assert_eq!(a.run(&["tunnel", "status"]).0, 1);
    std::fs::remove_file(&cfg).unwrap();
    let (c, out, _) = a.run(&["tunnel", "status"]);
    assert_eq!(c, 0);
    assert!(out.contains("no tunnels configured"));
    assert!(a
        .audit()
        .contains("\"action\":\"tunnel.status\",\"target\":\"\",\"result\":\"down\""));
    eprintln!("[{}] r07 ok", stamp());
}

#[test]
fn r08_audit_tail_follow_tamper() {
    if !on() {
        return;
    }
    let (a, r) = setup("r08");
    let rs = r.to_str().unwrap();
    for i in 0..3 {
        assert_eq!(
            a.run(&[
                "sandbox",
                "start",
                &format!("t{i}"),
                "--rootfs",
                rs,
                "--",
                "exit",
                "0"
            ])
            .0,
            0
        );
    }
    let (c, out, _) = a.run(&["audit", "tail", "-n", "2"]);
    assert_eq!(c, 0);
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(out.lines().all(|l| l.contains("+02:00 #")), "{out}");
    let (_, js, _) = a.run(&["audit", "tail", "-n", "1", "--json"]);
    assert!(js.starts_with("{\"v\":1,\"seq\":6,"), "{js}");
    // follow picks up a record written by another process
    let f = a
        .cmd(&["audit", "tail", "-n", "0", "--follow-secs", "3"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(
        a.run(&["sandbox", "start", "late", "--rootfs", rs, "--", "exit", "0"])
            .0,
        0
    );
    let o = f.wait_with_output_compat();
    assert!(
        o.contains("sandbox.start late launched") && o.contains("sandbox.exit late exit:0"),
        "{o}"
    );
    // tamper: edit one record in a copy of the state
    let au = a.state.join("audit.jsonl");
    let orig = std::fs::read_to_string(&au).unwrap();
    std::fs::write(
        &au,
        orig.replacen("\"target\":\"t1\"", "\"target\":\"tX\"", 1),
    )
    .unwrap();
    let (c, out, _) = a.run(&["audit", "verify"]);
    assert_eq!(c, 4);
    assert!(out.contains("BROKEN at line 3"), "{out}");
    let (c, _, err) = a.run(&[
        "sandbox", "start", "after", "--rootfs", rs, "--", "exit", "0",
    ]);
    assert_eq!(c, 0, "{err}"); // the start itself works; only the tail record is checked on append
    std::fs::write(&au, &orig).unwrap();
    let mut fh = std::fs::OpenOptions::new().append(true).open(&au).unwrap();
    fh.write_all(b"garbage\n").unwrap();
    let (c, _, err) = a.run(&[
        "sandbox", "start", "refused", "--rootfs", rs, "--", "exit", "0",
    ]);
    assert_eq!(c, 1, "start must fail closed on a damaged audit tail");
    assert!(err.contains("audit log tail is damaged"), "{err}");
    assert!(!a.state.join("sandboxes/refused.state").exists());
    eprintln!("[{}] r08 ok", stamp());
}

trait WaitCompat {
    fn wait_with_output_compat(self) -> String;
}
impl WaitCompat for std::process::Child {
    fn wait_with_output_compat(self) -> String {
        let o = self.wait_with_output().unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    }
}

#[test]
fn r09_parallel_sandboxes_keep_chain() {
    if !on() {
        return;
    }
    let (a, r) = setup("r09");
    let rs = r.to_str().unwrap().to_string();
    let a = std::sync::Arc::new(a);
    let hs: Vec<_> = (0..4)
        .map(|i| {
            let a = a.clone();
            let rs = rs.clone();
            std::thread::spawn(move || {
                let n = format!("p{i}");
                assert_eq!(
                    a.run(&[
                        "sandbox",
                        "start",
                        &n,
                        "--rootfs",
                        &rs,
                        "--settle-ms",
                        "100",
                        "--",
                        "sleep",
                        "30"
                    ])
                    .0,
                    0
                );
                std::thread::sleep(Duration::from_millis(300));
                assert_eq!(a.run(&["sandbox", "stop", &n]).0, 0);
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let (c, out, _) = a.run(&["audit", "verify"]);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("12 records"), "{out}");
    let (_, st, _) = a.run(&["sandbox", "status"]);
    assert_eq!(
        st.lines()
            .filter(|l| l.contains("exited   signal:15"))
            .count(),
        4,
        "{st}"
    );
    assert!(procs_with("fake_box").is_empty() || procs_with(&rs).is_empty());
    eprintln!(
        "[{}] r09 ok 4 parallel sandboxes, 12 chained records",
        stamp()
    );
}
