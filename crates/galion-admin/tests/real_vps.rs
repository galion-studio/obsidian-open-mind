// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! VPS-only real test: galion-admin driving the REAL galion-box launcher (sandbox_vps_only).
//! Runs only with GALION_REAL=1 GALION_ADMIN_ON_VPS=1, as an ordinary non-root user inside a
//! throwaway /var/tmp/sandbox-test-<ts> folder. Never set these on the dev box.
//!
//! Env: GALION_ADMIN_BIN (static galion-admin), GALION_BOX_BIN (static galion-box from
//! swarm/galion-os-sandbox-proto), GALION_WORK (the throwaway folder),
//! GALION_NERVE_PORT (optional loopback port of a live service, TCP connect only).

mod common;
use common::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Vps {
    a: Admin,
    work: PathBuf,
}

fn env() -> Option<Vps> {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1")
        && std::env::var("GALION_ADMIN_ON_VPS").as_deref() == Ok("1");
    if !on {
        eprintln!(
            "skipped: needs GALION_REAL=1 GALION_ADMIN_ON_VPS=1 (VPS only, sandbox_vps_only)"
        );
        return None;
    }
    // SAFETY: trivial getter.
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "must run as a non-root temp user"
    );
    let work = PathBuf::from(std::env::var("GALION_WORK").expect("GALION_WORK"));
    assert!(work.starts_with("/var/tmp") && work.to_str().unwrap().contains("sandbox-test-"));
    Some(Vps {
        a: Admin {
            bin: std::env::var("GALION_ADMIN_BIN")
                .expect("GALION_ADMIN_BIN")
                .into(),
            box_bin: std::env::var("GALION_BOX_BIN")
                .expect("GALION_BOX_BIN")
                .into(),
            state: work.join("admin-state"),
        },
        work,
    })
}

impl Vps {
    fn rootfs(&self, name: &str) -> PathBuf {
        let r = fresh_dir(&self.work, &format!("rootfs-{name}"));
        std::fs::create_dir_all(r.join("bin")).unwrap();
        std::fs::copy(&self.a.box_bin, r.join("bin/galion-box")).unwrap();
        r
    }
    fn leftovers(&self, rootfs: &std::path::Path) -> Vec<u32> {
        procs_with(rootfs.to_str().unwrap())
            .into_iter()
            .chain(procs_with("/bin/galion-box sleep"))
            .collect()
    }
}

#[test]
fn v01_probe_through_admin() {
    let Some(v) = env() else { return };
    let (c, out, err) = v.a.run(&["sandbox", "probe"]);
    eprintln!("[{}] v01 probe exit {c}:\n{out}{err}", stamp());
    assert_eq!(c, 0);
    assert!(out.contains("verdict=userns"), "{out}");
}

#[test]
fn v02_start_status_stop_real_box() {
    let Some(v) = env() else { return };
    let r = v.rootfs("v02");
    let rs = r.to_str().unwrap();
    let t = Instant::now();
    let (c, out, err) = v.a.run(&[
        "sandbox",
        "start",
        "v02",
        "--rootfs",
        rs,
        "--ro-root",
        "--",
        "/bin/galion-box",
        "sleep",
        "3600",
    ]);
    assert_eq!(c, 0, "{out}{err}");
    let start_ms = t.elapsed().as_millis();
    let pid: u32 = v.a.field("v02", "box_pid").parse().unwrap();
    assert!(pid_alive(pid));
    // supervisor + launcher + pid-1 init + payload are all ours and alive
    let procs = procs_with(rs);
    let (_, st, _) = v.a.run(&["sandbox", "status", "v02"]);
    assert!(st.contains("running"), "{st}");
    let t = Instant::now();
    let (c, out, err) = v.a.run(&["sandbox", "stop", "v02"]);
    let stop_ms = t.elapsed().as_millis();
    assert_eq!(c, 0, "{out}{err}");
    assert!(
        wait_until(Duration::from_secs(3), || v.leftovers(&r).is_empty()),
        "leftovers {:?}",
        v.leftovers(&r)
    );
    let (_, st2, _) = v.a.run(&["sandbox", "status", "v02"]);
    eprintln!(
        "[{}] v02 start {start_ms} ms, procs while running {} {:?}; status: {}; stop: {} ({stop_ms} ms); after: {}; leftovers 0",
        stamp(), procs.len(), procs, st.trim(), out.trim(), st2.trim()
    );
    assert!(
        procs.len() >= 3,
        "expected supervisor + launcher + sandbox procs, got {procs:?}"
    );
}

#[test]
fn v03_timeout_recorded() {
    let Some(v) = env() else { return };
    let r = v.rootfs("v03");
    let rs = r.to_str().unwrap();
    let (c, out, err) = v.a.run(&[
        "sandbox",
        "start",
        "v03",
        "--rootfs",
        rs,
        "--timeout",
        "1",
        "--",
        "/bin/galion-box",
        "sleep",
        "3600",
    ]);
    assert_eq!(c, 0, "{out}{err}");
    assert!(v.a.wait_exited("v03", Duration::from_secs(5)));
    let exit = v.a.field("v03", "exit");
    eprintln!(
        "[{}] v03 timeout 1 s -> {exit}; leftovers {:?}",
        stamp(),
        v.leftovers(&r)
    );
    assert_eq!(exit, "exit:124");
    assert!(v.leftovers(&r).is_empty());
}

#[test]
fn v04_isolation_report_via_admin() {
    let Some(v) = env() else { return };
    let r = v.rootfs("v04");
    let rs = r.to_str().unwrap();
    let (c, out, err) = v.a.run(&[
        "sandbox",
        "start",
        "v04",
        "--rootfs",
        rs,
        "--settle-ms",
        "3000",
        "--",
        "/bin/galion-box",
        "inside-check",
    ]);
    let log = std::fs::read_to_string(v.a.state.join("sandboxes/v04.log")).unwrap_or_default();
    eprintln!(
        "[{}] v04 start exit {c}: {}{}\nlog:\n{log}",
        stamp(),
        out.trim(),
        err.trim()
    );
    assert_eq!(c, 0);
    assert!(out.contains("finished"));
    for want in [
        "uid=1000",
        "pid=2",
        "cap_eff=0000000000000000",
        "no_new_privs=1",
        "interfaces=lo",
    ] {
        assert!(log.contains(want), "missing {want}");
    }
}

#[test]
fn v05_fail_closed_and_supervisor_death() {
    let Some(v) = env() else { return };
    // missing rootfs: galion-box refuses, admin reports it, nothing runs
    let (c, _, err) = v.a.run(&[
        "sandbox",
        "start",
        "v05a",
        "--rootfs",
        "/var/tmp/does-not-exist-165",
        "--",
        "/bin/galion-box",
        "sleep",
        "60",
    ]);
    eprintln!(
        "[{}] v05a missing rootfs: exit {c}: {}",
        stamp(),
        err.trim()
    );
    assert_eq!(c, 1);
    assert!(err.contains("failed to start"));
    // kill -9 the supervisor: PDEATHSIG -> launcher -> its pid 1 -> whole namespace
    let r = v.rootfs("v05");
    let rs = r.to_str().unwrap();
    assert_eq!(
        v.a.run(&[
            "sandbox",
            "start",
            "v05",
            "--rootfs",
            rs,
            "--",
            "/bin/galion-box",
            "sleep",
            "3600"
        ])
        .0,
        0
    );
    let before = procs_with(rs).len();
    let sup: u32 = v.a.field("v05", "supervisor_pid").parse().unwrap();
    // SAFETY: SIGKILL to our own supervisor process.
    unsafe { libc::kill(sup as i32, libc::SIGKILL) };
    let t = Instant::now();
    let gone = wait_until(Duration::from_secs(5), || v.leftovers(&r).is_empty());
    let (_, st, _) = v.a.run(&["sandbox", "status", "v05"]);
    eprintln!(
        "[{}] v05b supervisor SIGKILL: {before} procs -> 0 in {} ms; status: {}",
        stamp(),
        t.elapsed().as_millis(),
        st.trim()
    );
    assert!(gone, "leftovers {:?}", v.leftovers(&r));
    assert!(st.contains("lost"), "{st}");
}

#[test]
fn v06_tunnel_status_loopback() {
    let Some(v) = env() else { return };
    std::fs::create_dir_all(&v.a.state).unwrap();
    let port = std::env::var("GALION_NERVE_PORT").unwrap_or_else(|_| "8670".into());
    let cfg = v.work.join("tunnel.conf");
    std::fs::write(
        &cfg,
        format!("nerve = 127.0.0.1:{port}\nclosed = 127.0.0.1:20359\n"),
    )
    .unwrap();
    let (c, out, _) =
        v.a.run(&["tunnel", "status", "--config", cfg.to_str().unwrap()]);
    eprintln!("[{}] v06 tunnel status exit {c}:\n{out}", stamp());
    assert_eq!(c, 4);
    assert!(out.contains("nerve") && out.contains(" up "), "{out}");
    assert!(out.contains("closed") && out.contains("down  refused"));
}

#[test]
fn v99_audit_chain_after_everything() {
    let Some(v) = env() else { return };
    std::thread::sleep(Duration::from_millis(200));
    let (c, out, _) = v.a.run(&["audit", "verify"]);
    let (_, tail, _) = v.a.run(&["audit", "tail", "-n", "40"]);
    eprintln!("[{}] v99 {}\n{tail}", stamp(), out.trim());
    assert_eq!(c, 0);
}
