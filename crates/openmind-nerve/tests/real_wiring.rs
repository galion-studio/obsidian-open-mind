// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life wiring test (gated: `GALION_REAL=1`; slot 30, ports
//! 19300-19309). Runs `scripts/real/nerve-wiring.sh`: the real release
//! binaries of openmind-nerve, hermes-gateway (--mock, offline, no spend)
//! and openmind-memory, traffic with 5xx and slow requests, the nerve hung
//! and killed, hermes killed and restarted under a call cap. Checks the
//! markers in its output.

use std::process::Command;

#[test]
fn nerve_wiring_end_to_end() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life wiring test");
        return;
    }
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let out = Command::new("bash")
        .arg(format!("{root}/scripts/real/nerve-wiring.sh"))
        .env("N_REQ", "200")
        .output()
        .expect("run scripts/real/nerve-wiring.sh");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "script failed:\n{text}");
    for needle in [
        r#""name":"started","msg":"listening on 127.0.0.1:19301""#,
        r#""name":"started","msg":"listening on 127.0.0.1:19302""#,
        r#""port":19305,"owner":"unknown owner""#,
        r#""process":"openmind-memory""#,
        "pushed 3 slow request(s)",
        "POST /seed -> 501",
        "health probe down",
        r#""name":"spend_cap""#,
        "spend cap hit (provider calls refused)",
        r#""gaia":["ARTEMIS"]"#,
        r#""gaia":["APOLLO"]"#,
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    // Requests keep being answered while the nerve hangs and when it is gone.
    for phase in ["hung", "down"] {
        let line = text
            .lines()
            .find(|l| l.contains(&format!("memory ({phase}):")))
            .unwrap_or_else(|| panic!("no memory ({phase}) line"));
        assert!(line.contains("200"), "{line}");
    }
}
