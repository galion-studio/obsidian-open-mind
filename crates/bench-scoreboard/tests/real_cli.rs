// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real CLI test (GALION_REAL=1): runs the built binary on real files.

use std::process::Command;

#[test]
fn real_cli_folds_and_pends() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_bench-scoreboard");
    let dir = std::env::temp_dir().join(format!("bench-scoreboard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("b3.json");
    std::fs::write(
        &good,
        r#"{"pct":90,"axes":[{"name":"inventory truth","score":100},{"name":"pricing","score":80}]}"#,
    )
    .unwrap();
    let out = dir.join("SCOREBOARD.md");
    let o = Command::new(bin)
        .args(["--score", &format!("B3={}@deadbee", good.display())])
        .args(["--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = std::fs::read_to_string(&out).unwrap();
    assert!(md.contains("| 90% | swarm/bench-vending @ deadbee |"));
    assert!(md.contains("pricing (80)"));
    assert_eq!(md.matches("| PENDING |").count(), 2);
    assert!(md.contains("+02:00") || md.contains("+01:00"));

    let bad = dir.join("bad.json");
    std::fs::write(&bad, r#"{"pct":100,"axes":[{"name":"a","score":0}]}"#).unwrap();
    let o = Command::new(bin)
        .args(["--score", &format!("B1={}@x", bad.display())])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    let o = Command::new(bin).arg("--nope").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}
