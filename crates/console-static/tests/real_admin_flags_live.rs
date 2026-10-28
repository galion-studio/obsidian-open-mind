// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 313): READ-ONLY drift check of the live VPS
//! console units against the as-built mirrors the staged drop-ins sit on.
//! Over ssh it only runs `systemctl cat`, `systemctl show`, `test -e` and
//! loopback HTTP GETs; it never writes on the VPS.
//!
//! Checks: live body == mirror body (byte for byte, both units), units
//! active with no drop-ins yet (staged only), the live binary is still
//! 2d846fb8, and today's gap (remote `/api/v1/admin` + board routes 404 on
//! :8080 and :18080) that the drop-in closes. Nerve `/state` read.
//!
//! Gated: `GALION_REAL=1 CONSOLE_UNIT_LIVE=1 cargo test -p console-static --test real_admin_flags_live -- --nocapture`
//! Env: `CONSOLE_UNIT_SSH` (default `root@51.38.137.193`), key
//! `~/.ssh/id_ed25519_vps`.

mod units;

use std::process::Command;

const LIVE_8080: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const LIVE_18080: &str =
    include_str!("../../../deploy/console-rust-serve/live/console-rust-primary.service");

fn ssh(cmd: &str) -> String {
    let host = std::env::var("CONSOLE_UNIT_SSH").unwrap_or_else(|_| "root@51.38.137.193".into());
    let key = format!("{}/.ssh/id_ed25519_vps", std::env::var("HOME").unwrap());
    let out = Command::new("ssh")
        .args([
            "-i",
            &key,
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=10",
        ])
        .args(["-o", "BatchMode=yes", &host, cmd])
        .output()
        .expect("ssh");
    assert!(
        out.status.success(),
        "ssh {cmd}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn real_admin_flags_live() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1")
        || std::env::var("CONSOLE_UNIT_LIVE").ok().as_deref() != Some("1")
    {
        eprintln!("skip: set GALION_REAL=1 CONSOLE_UNIT_LIVE=1");
        return;
    }
    for (u, mirror) in [("console", LIVE_8080), ("console-rust-primary", LIVE_18080)] {
        let body = ssh(&format!("systemctl cat {u}.service | tail -n +2"));
        assert_eq!(
            body,
            units::live_body(mirror),
            "{u}: live drifted from mirror"
        );
        let argv = units::exec_argv(&[&body]);
        assert!(
            argv[0].ends_with("galion-console-serve-2d846fb8"),
            "{u}: {}",
            argv[0]
        );
        let show = ssh(&format!(
            "systemctl show {u}.service -p ActiveState,SubState,DropInPaths,NRestarts; \
             test -e /etc/systemd/system/{u}.service.d && echo dropin-dir=yes || echo dropin-dir=no"
        ));
        assert!(show.contains("ActiveState=active"), "{u}: {show}");
        assert!(
            show.contains("DropInPaths=\n"),
            "{u}: staged only, no drop-in: {show}"
        );
        assert!(show.contains("dropin-dir=no"), "{u}: {show}");
        eprintln!(
            "{u}: mirror == live ({} B), {}",
            body.len(),
            show.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    // The gap the drop-in closes (codes only, no bodies kept).
    let probe = ssh(
        "for p in 8080 18080; do for u in /api/v1/admin /api/v1/services /api/v1/catalog /api/v1/health; do \
         printf '%s %s lo=%s remote=%s\\n' $p $u \
         $(curl -s -o /dev/null -m 5 -w '%{http_code}' -H 'Host: 127.0.0.1' http://127.0.0.1:$p$u) \
         $(curl -s -o /dev/null -m 5 -w '%{http_code}' -H 'Host: console.galion.app' http://127.0.0.1:$p$u); done; \
         printf '%s /console remote=%s\\n' $p $(curl -s -o /dev/null -m 5 -w '%{http_code}' -H 'Host: console.galion.app' http://127.0.0.1:$p/console); done; \
         printf 'nerve /state %s\\n' $(curl -s -o /dev/null -m 5 -w '%{http_code}' http://127.0.0.1:8670/state); \
         sha256sum /opt/galion/rust/bin/galion-console-serve-2d846fb8 | cut -c1-16; \
         test -e /opt/galion/rust/bin/galion-console-serve-9c0f9d1a && echo new-bin=present || echo new-bin=absent",
    );
    eprint!("{probe}");
    assert!(probe.contains("8080 /api/v1/health lo=200"), "{probe}");
    assert!(probe.contains("nerve /state 200"), "{probe}");
}
