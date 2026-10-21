// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Real-life test: runs the built `tunnel-lint` binary against a config whose
//! origins are this slot's ports (19355-19357), with real TCP listeners on some
//! of them. Gated: runs only with GALION_REAL=1 (scripts/real/tunnel-audit.sh).

use std::net::TcpListener;
use std::process::Command;

fn bin() -> String {
    std::env::var("TUNNEL_LINT_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_tunnel-lint").into())
}

#[test]
fn real_binary_flags_closed_slot_ports() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("tunnel-lint-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config.yml");
    std::fs::write(
        &cfg,
        concat!(
            "tunnel: 00000000-0000-0000-0000-000000000000\n",
            "ingress:\n",
            "  - hostname: up.example\n",
            "    service: http://localhost:19355\n",
            "  - hostname: up4.example\n",
            "    service: http://127.0.0.1:19356\n",
            "  - hostname: down.example\n",
            "    service: http://localhost:19357\n",
            "  - service: http_status:404\n",
        ),
    )
    .unwrap();
    let a = TcpListener::bind("127.0.0.1:19355").expect("port 19355 free");
    let b = TcpListener::bind("127.0.0.1:19356").expect("port 19356 free");

    let out = Command::new(bin()).arg("--tsv").arg(&cfg).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("up.example\t\thttp://localhost:19355\t19355\tOPEN"),
        "{text}"
    );
    assert!(
        text.contains("up4.example\t\thttp://127.0.0.1:19356\t19356\tOPEN"),
        "{text}"
    );
    assert!(
        text.contains("down.example\t\thttp://localhost:19357\t19357\tCLOSED"),
        "{text}"
    );

    // Close one listener: the route must flip to CLOSED on the next run.
    drop(b);
    let out = Command::new(bin()).arg("--tsv").arg(&cfg).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("19356\tCLOSED"), "{text}");

    // Stage, then lint the staged copy: only the live route is left, exit 0.
    let staged = dir.join("staged.yml");
    let out = Command::new(bin())
        .arg("--stage")
        .arg(&staged)
        .arg(&cfg)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = Command::new(bin()).arg(&staged).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("summary: 2 rules, 0 closed routes"), "{text}");
    drop(a);

    // Credentials guard.
    let out = Command::new(bin())
        .arg(dir.join("x.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).ok();
}
