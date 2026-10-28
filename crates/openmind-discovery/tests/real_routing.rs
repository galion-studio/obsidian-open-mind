// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life routing test: runs `scripts/real/lan-routing.sh` (release
//! binaries, mock OpenAI servers on 127.0.0.1:19311-19316, test announcer on
//! mDNS port 19310). Runs only with `GALION_REAL=1`. Needs curl and jq.

use std::path::Path;
use std::process::Command;

#[test]
fn approve_route_kill_fallback_move_deny_end_to_end() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/real/lan-routing.sh");
    let out = Command::new("bash")
        .arg(&script)
        .output()
        .expect("run script");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("PASS"), "{stdout}");
}
