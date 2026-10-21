// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life heal test (GALION_REAL=1). Runs the real guardian-tick binary
//! against a live loopback listener (healthy, port 19045) and a closed port
//! (dead, 19046). The restart backend is a real argv command that appends
//! to a marker file, so we can count real executions. Shows: detection, the
//! dry-run (no --allow), 3 applied restarts, then the reflex cap.
//! The full start/kill/restart of real dummy processes is
//! scripts/real/survival-heal.sh.

use std::net::TcpListener;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_guardian-tick");

fn run(args: &[&str]) -> String {
    let o = Command::new(BIN).args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn real_heal_dry_run_apply_and_cap() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let d = std::env::temp_dir().join(format!("gt-real-heal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let l = TcpListener::bind("127.0.0.1:19045").unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            use std::io::{Read, Write};
            let mut s = s;
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\nok");
        }
    });
    let marker = d.join("restarts.txt");
    let probes = d.join("probes.toml");
    std::fs::write(
        &probes,
        format!(
            "[[probe]]\nkind=\"http\"\nname=\"healthy\"\nport=19045\npath=\"/\"\n\
             [[probe]]\nkind=\"http\"\nname=\"dies\"\nport=19046\npath=\"/\"\n\
             [[repair]]\nprobe=\"dies\"\nunit=\"dummy-dies\"\nafter_fails=1\nbackend=\"command\"\n\
             verify_after_ms=0\ncommand=[\"/bin/sh\",\"-c\",\"echo restart >> {}\"]\n",
            marker.display()
        ),
    )
    .unwrap();
    let st = d.join("state");
    let (p, s) = (probes.to_str().unwrap(), st.to_str().unwrap());
    let dry = run(&["heal", "--probes", p, "--state", s]);
    assert!(dry.contains("mode=dry-run"), "{dry}");
    assert!(dry.contains("would restart"), "{dry}");
    assert!(!marker.exists(), "dry-run must not run the backend");
    for _ in 0..3 {
        let o = run(&["heal", "--probes", p, "--state", s, "--allow", "dummy-dies"]);
        assert!(o.contains("mode=apply"), "{o}");
    }
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 3);
    let o = run(&["heal", "--probes", p, "--state", s, "--allow", "dummy-dies"]);
    assert!(o.contains("cap-reached"), "{o}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 3);
    let store = std::fs::read_to_string(st.join("incidents.jsonl")).unwrap();
    assert!(store.contains("\"kind\":\"cap-hit\""));
    assert!(!store.contains("Z\""), "no UTC stamps");
    let nerve = std::fs::read_to_string(st.join("nerve.jsonl")).unwrap();
    assert!(nerve.contains("heal-cap-hit"));
    let _ = std::fs::remove_dir_all(&d);
}
