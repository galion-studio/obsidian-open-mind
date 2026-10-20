// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life enrollment test (GALION_REAL=1): runs scripts/real/hermes-enroll.sh,
//! which builds the static release binaries and drives the real hermes-gateway
//! (--mock) and two real omnode nodes on loopback ports 19320-19323.

#[test]
fn real_enroll_script_passes() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skipped: set GALION_REAL=1 to run scripts/real/hermes-enroll.sh");
        return;
    }
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/real/hermes-enroll.sh"
    );
    let out = std::process::Command::new("bash")
        .arg(script)
        .output()
        .expect("run scripts/real/hermes-enroll.sh");
    let text = String::from_utf8_lossy(&out.stdout);
    println!("{text}");
    assert!(out.status.success(), "script failed:\n{text}");
    assert!(text.contains(" fail=0"), "{text}");
}
