// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of `omnode sandbox denylist` (swarm slot 220, S4.3).
//! Runs only with GALION_REAL=1. Uses OMNODE_BIN (e.g. an aarch64 or armv7
//! musl release binary) and an optional OMNODE_RUNNER (e.g. qemu-aarch64),
//! else the binary cargo built for this test. No seccomp filter is installed
//! and no syscall is tried: the command only reads the compiled tables, so
//! this is not a sandbox run (sandbox runs stay on the VPS).

use std::path::PathBuf;
use std::process::{Command, Output};

fn enabled() -> bool {
    if std::env::var("GALION_REAL").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("SKIP: set GALION_REAL=1 to run the real-life denylist test");
    false
}

fn run(args: &[&str]) -> Output {
    let bin = std::env::var_os("OMNODE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_omnode")));
    let mut c = match std::env::var_os("OMNODE_RUNNER") {
        Some(r) => {
            let mut c = Command::new(r);
            c.arg(&bin);
            c
        }
        None => Command::new(&bin),
    };
    c.args(args).output().expect("start omnode")
}

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/security/denylist-parity.md")
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&o.stdout)))
}

#[test]
fn real_denylist_parity() {
    if !enabled() {
        return;
    }
    let doc = doc_path();
    let doc_s = doc.to_str().unwrap();

    // 1. The binary's own arch: live filter == table, parity ok, doc current.
    let o = run(&["sandbox", "denylist", "--check", doc_s]);
    let j = json(&o);
    eprintln!(
        "arch={} audit_arch={} live={} counts={} parity_ok={} doc_current={}",
        j["arch"],
        j["audit_arch"],
        j["denied_live_count"],
        j["counts"],
        j["parity_ok"],
        j["doc_current"]
    );
    assert!(
        o.status.success(),
        "exit {:?}: {}",
        o.status,
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(j["parity_ok"], true);
    assert_eq!(j["live_matches_table"], true);
    assert_eq!(j["doc_current"], true);
    assert_eq!(j["violations"].as_array().unwrap().len(), 0);
    let arch = j["arch"].as_str().expect("supported arch");
    let want = match arch {
        "x86_64" => 40,
        "aarch64" => 38,
        "armv7" => 39,
        a => panic!("unexpected arch {a}"),
    };
    assert_eq!(j["denied_live_count"], want);
    assert_eq!(j["counts"]["x86_64"], 40);
    assert_eq!(j["counts"]["aarch64"], 38);
    assert_eq!(j["counts"]["armv7"], 39);
    let live: Vec<u64> = j["denied_live"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    // kexec_file_load per arch (the armv7 gap slot 220 fixed) and fsconfig.
    let kexec_file = match arch {
        "x86_64" => 320,
        "aarch64" => 294,
        _ => 401,
    };
    assert!(
        live.contains(&kexec_file),
        "{arch}: kexec_file_load {kexec_file} not denied"
    );
    assert!(live.contains(&431) && live.contains(&443) && live.contains(&467));

    // 2. Markdown output is byte-identical to the checked-in doc.
    let md = run(&["sandbox", "denylist", "--markdown"]);
    assert!(md.status.success());
    let on_disk = std::fs::read(&doc).unwrap();
    assert!(md.stdout == on_disk, "--markdown differs from {doc_s}");
    assert!(String::from_utf8_lossy(&md.stdout).contains("PASS: 0 violations."));

    // 3. A stale doc is refused with exit 2.
    let dir = std::env::temp_dir().join(format!("omnode-slot220-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stale = dir.join("stale.md");
    let mut s = String::from_utf8(on_disk).unwrap();
    s = s.replacen("| 401 DENY |", "| 401 allow |", 1);
    std::fs::write(&stale, s).unwrap();
    let o = run(&["sandbox", "denylist", "--check", stale.to_str().unwrap()]);
    let j = json(&o);
    assert_eq!(o.status.code(), Some(2), "stale doc must exit 2");
    assert_eq!(j["doc_current"], false);
    assert!(String::from_utf8_lossy(&o.stderr).contains("stale"));

    // 4. Missing doc file and bad flags are errors, not passes.
    let o = run(&[
        "sandbox",
        "denylist",
        "--check",
        dir.join("nope.md").to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(&["sandbox", "denylist", "--bogus"]);
    assert_eq!(o.status.code(), Some(2));
    std::fs::remove_dir_all(&dir).unwrap();
}
