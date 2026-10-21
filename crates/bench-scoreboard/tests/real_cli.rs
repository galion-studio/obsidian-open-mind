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

/// Real CLI on the committed score JSONs of the three merged adversarial / hard branches.
#[test]
fn real_cli_folds_committed_adv_sets() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_bench-scoreboard");
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/bench");
    let p = |f: &str| docs.join(f).display().to_string();
    let dir = std::env::temp_dir().join(format!("bench-scoreboard-adv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("SCOREBOARD.md");
    let specs = [
        format!("B1={}@b1", p("banking.score.json")),
        format!("B2={}@b2", p("wallstreet.score.json")),
        format!("B3={}@b3", p("vending.score.json")),
        format!(
            "B1-ADV={}@44e5c61",
            p("hermes-banking-adv.mock-llm.score.json")
        ),
        format!("B2-ADV={}@2ecfc19", p("wallstreet-adversarial.score.json")),
        format!("B3-HARD={}@1b0a30c", p("vending-hard.score.json")),
        format!(
            "B3-HARD-H={}@1b0a30c",
            p("hermes-vending-hard.mock-llm.score.json")
        ),
    ];
    let mut cmd = Command::new(bin);
    for s in &specs {
        cmd.args(["--score", s]);
    }
    let o = cmd.args(["--out", out.to_str().unwrap()]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = std::fs::read_to_string(&out).unwrap();
    assert_eq!(md.matches("| PENDING |").count(), 0, "{md}");
    assert!(md.contains("## Adversarial / hard sets"));
    for (key, sha) in [
        ("bench-banking-adv", "44e5c61"),
        ("bench-ws-adv", "2ecfc19"),
        ("bench-vending-adv", "1b0a30c"),
    ] {
        assert!(
            md.contains(&format!("| 100% | swarm/{key} @ {sha} |")),
            "{key}"
        );
    }
    // B2-ADV families became axes (e.g. croatian, obfuscation) and none fail.
    assert!(md.contains("| croatian | 100 |") && md.contains("| obfuscation | 100 |"));
    assert!(md.contains("| pricing | 100 |"));
    eprintln!(
        "adv board: {} lines, {} axis rows",
        md.lines().count(),
        md.lines().filter(|l| l.ends_with(" | 100 |")).count()
    );

    // Tampered copy: a family claims 100 % with 25/26 passed -> exit 1.
    let ws = std::fs::read_to_string(p("wallstreet-adversarial.score.json")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&ws).unwrap();
    v["families"][0]["passed"] = serde_json::json!(v["families"][0]["total"].as_u64().unwrap() - 1);
    let bad = dir.join("ws-tampered.json");
    std::fs::write(&bad, v.to_string()).unwrap();
    let o = Command::new(bin)
        .args(["--score", &format!("B2-ADV={}@x", bad.display())])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "tampered family must be rejected");
    let o = Command::new(bin)
        .args(["--score", &format!("B9-ADV={}@x", bad.display())])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}
