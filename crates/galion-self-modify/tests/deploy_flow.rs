// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Root deploy with mocked systemctl / health / ntfy: one restart, backup,
//! auto-rollback, alerts both ways, nonce burned, refusals change nothing.

mod common;
use common::*;
use galion_self_modify::deploy::{deploy, http_status, ledger_has, rollback, RealSystem, System};
use hermes_rs::self_modify::sha256_hex;
use std::io::{Read, Write};

const PREV: &str =
    "[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/hermes-gateway-previous\n";

fn with_prev(f: &Fix) {
    let c = f.cfg();
    std::fs::create_dir_all(&c.dropin_dir).unwrap();
    std::fs::write(c.dropin(), PREV).unwrap();
}

#[test]
fn deploy_success_one_restart_backup_alert() {
    let f = signed("dep-ok");
    f.approve_ed25519(NONCE);
    with_prev(&f);
    let mut m = Mock::default();
    let r = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW).unwrap();
    assert_eq!(r["state"], "deployed");
    assert_eq!(
        m.calls,
        vec!["daemon-reload", "restart hermes-rs.service", "health"]
    );
    assert_eq!(
        m.calls.iter().filter(|c| c.starts_with("restart")).count(),
        1,
        "exactly ONE restart"
    );
    assert_eq!(m.alerts.len(), 1);
    assert!(m.alerts[0].0.contains("DEPLOYED"));
    // Binary installed with the signed fingerprint, drop-in points at it.
    let bin = std::path::PathBuf::from(r["binary"].as_str().unwrap());
    assert_eq!(
        sha256_hex(&std::fs::read(&bin).unwrap()),
        r["artifact_sha256"].as_str().unwrap()
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let d = std::fs::read_to_string(f.cfg().dropin()).unwrap();
    assert!(d.contains(&format!("ExecStart={}", bin.display())), "{d}");
    assert!(d.contains(NONCE));
    // Backup of the previous drop-in, nonce burned, deploy record written.
    let bk = std::path::PathBuf::from(r["backup"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(bk.join("previous-dropin")).unwrap(),
        PREV
    );
    assert!(ledger_has(&f.ledger(), NONCE).unwrap());
    assert!(f.e().deploy_record().exists());
    // A second deploy of the same entry is refused (already deployed / replay).
    let mut m2 = Mock::default();
    let e = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m2, NOW).unwrap_err();
    assert!(e.contains("already deployed"), "{e}");
    assert!(m2.calls.is_empty());
}

#[test]
fn replay_of_same_approval_on_copied_entry_refused() {
    let f = signed("dep-replay");
    f.approve_ed25519(NONCE);
    deploy(&f.entry, &f.keys(), &f.cfg(), &mut Mock::default(), NOW).unwrap();
    // Attacker moves deploy.json aside to try the same signed approval again.
    let d = f.e().deploy_record();
    std::fs::rename(&d, d.with_extension("json.off")).unwrap();
    let mut m = Mock::default();
    let e = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW + 10).unwrap_err();
    assert!(e.contains("already used"), "{e}");
    assert!(m.calls.is_empty(), "nothing restarted on refusal");
    assert!(m.alerts[0].0.contains("REFUSED"));
}

#[test]
fn health_failure_rolls_back_to_previous_dropin() {
    let f = signed("dep-rb");
    f.approve_ed25519(NONCE);
    with_prev(&f);
    let mut m = Mock {
        health: vec![Err("http 502".into()), Ok("/health 200".into())],
        ..Default::default()
    };
    let e = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW).unwrap_err();
    assert!(e.contains("rolled back"), "{e}");
    assert_eq!(
        m.calls,
        vec![
            "daemon-reload",
            "restart hermes-rs.service",
            "health",
            "daemon-reload",
            "restart hermes-rs.service",
            "health"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(f.cfg().dropin()).unwrap(),
        PREV,
        "previous drop-in back"
    );
    let rec: serde_json::Value =
        serde_json::from_slice(&std::fs::read(f.e().deploy_record()).unwrap()).unwrap();
    assert_eq!(rec["state"], "rolled_back");
    assert_eq!(rec["rollback"]["health"], "/health 200");
    assert!(m
        .alerts
        .iter()
        .any(|a| a.0.contains("ROLLED BACK") && a.1 == "urgent"));
    assert!(
        ledger_has(&f.ledger(), NONCE).unwrap(),
        "a failed deploy burns the nonce"
    );
}

#[test]
fn health_failure_without_previous_dropin_moves_ours_aside() {
    let f = signed("dep-rb2");
    f.approve_ed25519(NONCE);
    let mut m = Mock {
        health: vec![Err("timeout".into())],
        ..Default::default()
    };
    deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW).unwrap_err();
    assert!(!f.cfg().dropin().exists());
    let aside: Vec<_> = std::fs::read_dir(&f.cfg().dropin_dir)
        .unwrap()
        .filter_map(|x| x.ok())
        .filter(|x| x.file_name().to_string_lossy().contains(".rolled-back-"))
        .collect();
    assert_eq!(aside.len(), 1, "ours renamed, never deleted");
}

#[test]
fn refusal_changes_nothing_and_alerts() {
    let f = signed("dep-refuse");
    f.approve_ed25519(NONCE);
    with_prev(&f);
    std::fs::write(f.e().artifact(), b"swapped").unwrap();
    let mut m = Mock::default();
    let e = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW).unwrap_err();
    assert!(e.contains("fingerprint"), "{e}");
    assert!(m.calls.is_empty());
    assert_eq!(m.alerts.len(), 1);
    assert_eq!(m.alerts[0].1, "high");
    assert!(!f.cfg().bin_dir.exists());
    assert_eq!(std::fs::read_to_string(f.cfg().dropin()).unwrap(), PREV);
    assert!(
        !ledger_has(&f.ledger(), NONCE).unwrap(),
        "refusal does not burn the nonce"
    );
}

#[test]
fn expired_at_deploy_time_refused() {
    let f = signed("dep-exp");
    f.approve_ed25519(NONCE);
    let mut m = Mock::default();
    let e = deploy(&f.entry, &f.keys(), &f.cfg(), &mut m, NOW + 1800).unwrap_err();
    assert!(e.contains("expired"), "{e}");
    assert!(m.calls.is_empty());
}

#[test]
fn manual_rollback_restores_previous() {
    let f = signed("dep-manual");
    f.approve_ed25519(NONCE);
    with_prev(&f);
    deploy(&f.entry, &f.keys(), &f.cfg(), &mut Mock::default(), NOW).unwrap();
    let mut m = Mock::default();
    let r = rollback(&f.entry, &f.cfg(), &mut m, NOW + 60).unwrap();
    assert_eq!(r["state"], "rolled_back");
    assert_eq!(std::fs::read_to_string(f.cfg().dropin()).unwrap(), PREV);
    assert_eq!(
        m.calls.iter().filter(|c| c.starts_with("restart")).count(),
        1
    );
    assert!(
        rollback(&f.entry, &f.cfg(), &mut Mock::default(), NOW + 61).is_err(),
        "only once"
    );
}

/// RealSystem against loopback stand-ins: a health server and an ntfy
/// relay (same request shape as galion-ntfy-relay), systemctl = /bin/true.
#[test]
fn real_system_http_health_and_ntfy_relay() {
    let health = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let relay = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ha = health.local_addr().unwrap().to_string();
    let ra = relay.local_addr().unwrap().to_string();
    let h = std::thread::spawn(move || {
        let (mut c, _) = health.accept().unwrap();
        let mut b = [0u8; 1024];
        let n = c.read(&mut b).unwrap();
        let req = String::from_utf8_lossy(&b[..n]).to_string();
        c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .unwrap();
        req
    });
    let r = std::thread::spawn(move || {
        let (mut c, _) = relay.accept().unwrap();
        let mut b = vec![0u8; 8192];
        let mut got = 0;
        // read until the declared body is in
        loop {
            let n = c.read(&mut b[got..]).unwrap();
            got += n;
            let s = String::from_utf8_lossy(&b[..got]).to_string();
            if n == 0 || s.contains("\r\n\r\n") && s.ends_with("is live") {
                break;
            }
        }
        c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        String::from_utf8_lossy(&b[..got]).to_string()
    });
    let mut s = RealSystem {
        systemctl: "/bin/true".into(),
        health_addr: ha,
        health_path: "/health".into(),
        health_timeout_s: 5,
        ntfy_addr: ra,
    };
    s.systemctl(&["daemon-reload"]).unwrap();
    assert_eq!(s.health().unwrap(), "/health 200");
    s.alert(
        "Hermes self-change DEPLOYED",
        "default",
        "white_check_mark",
        "build abc is live",
    )
    .unwrap();
    assert!(h.join().unwrap().starts_with("GET /health HTTP/1.0"));
    let req = r.join().unwrap();
    assert!(req.starts_with("POST / HTTP/1.0"), "{req}");
    assert!(req.contains("Title: Hermes self-change DEPLOYED"));
    assert!(req.contains("Content-Length: 17"));
    let mut bad = RealSystem {
        systemctl: "/bin/false".into(),
        ..s
    };
    assert!(bad.systemctl(&["restart", "x"]).is_err());
    assert_eq!(http_status("HTTP/1.1 503 x"), 503);
}
