// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The journal returns one object and stores nothing. A test may write a temp file.

use hades_gate::{decide, Decision, ALLOW_LIST, ALWAYS_REFUSED, APPROVED_TEST_TOKEN};
use hades_journal::record;

fn decision_name(decision: Decision) -> &'static str {
    match decision {
        Decision::DryRun => "DryRun",
        Decision::WouldApply => "WouldApply",
        Decision::Refused => "Refused",
    }
}

fn assert_zagreb_ts(ts: Option<&str>) {
    let ts = ts.expect("ts string");
    assert_eq!(ts.len(), 25, "seconds precision: {ts}");
    assert!(ts.contains('T'), "no T separator: {ts}");
    assert!(!ts.ends_with('Z'), "must not end in Z: {ts}");
    assert!(!ts.contains("+00:00"), "must not be UTC: {ts}");
    assert!(
        ts.ends_with("+02:00") || ts.ends_with("+01:00"),
        "Zagreb offset: {ts}"
    );
}

fn assert_record(action: &str, token: &str, expect: Decision) {
    let row = record(action, token);
    let obj = row.as_object().expect("one JSON object");
    assert_eq!(obj.len(), 3);
    assert!(obj.contains_key("action"));
    assert!(obj.contains_key("decision"));
    assert!(obj.contains_key("ts"));
    assert_eq!(row["action"].as_str(), Some(action));
    assert_eq!(row["decision"].as_str(), Some(decision_name(expect)));
    assert_eq!(decide(action, token), expect);
    assert_zagreb_ts(row["ts"].as_str());
}

#[test]
fn default_is_dry_run() {
    assert_record("heal-apply", "", Decision::DryRun);
    assert_record("switch", "no", Decision::DryRun);
    assert_record("rollback", "approved-test ", Decision::DryRun);
    assert_record("rollback", "Approved-test", Decision::DryRun);
    assert_record("heal_apply", APPROVED_TEST_TOKEN, Decision::DryRun);
    assert_record("", APPROVED_TEST_TOKEN, Decision::DryRun);
    assert_record("unknown", APPROVED_TEST_TOKEN, Decision::DryRun);
    for action in ALLOW_LIST {
        assert_record(action, "", Decision::DryRun);
    }
}

#[test]
fn would_apply_only_for_the_test_token_on_the_allow_list() {
    for action in ALLOW_LIST {
        assert_record(action, APPROVED_TEST_TOKEN, Decision::WouldApply);
    }
}

#[test]
fn kill_and_rotate_are_refused() {
    for action in ALWAYS_REFUSED {
        assert_record(action, "", Decision::Refused);
        assert_record(action, "no", Decision::Refused);
        assert_record(action, APPROVED_TEST_TOKEN, Decision::Refused);
    }
}

#[test]
fn a_test_may_write_the_object_the_crate_does_not() {
    let row = record("switch", APPROVED_TEST_TOKEN);
    assert_eq!(row["decision"].as_str(), Some("WouldApply"));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("hades-journal-{nanos}.json"));
    let text = serde_json::to_string(&row).expect("object serializes");
    std::fs::write(&path, &text).expect("test may write a temp file");
    let back = std::fs::read_to_string(&path).expect("temp file reads");
    let parsed: serde_json::Value = serde_json::from_str(&back).expect("temp file is JSON");
    assert_eq!(parsed, row);
    std::fs::remove_file(&path).expect("temp file removed");
}

/// `record` returns JSON and writes no file. `kill` and `rotate` stay refused.
#[test]
fn record_returns_json_writes_no_file_and_kill_and_rotate_stay_refused() {
    assert_eq!(ALWAYS_REFUSED, ["kill", "rotate"]);

    let probe = probe_dir();
    std::fs::create_dir_all(&probe).expect("probe dir");
    assert!(dir_names(&probe).is_empty(), "probe dir starts empty");
    let cwd = CwdGuard::enter(&probe);

    for action in ["kill", "rotate"] {
        for token in ["", "no", APPROVED_TEST_TOKEN] {
            let row = record(action, token);
            let text = serde_json::to_string(&row).expect("record returns JSON");
            let parsed: serde_json::Value = serde_json::from_str(&text).expect("JSON text");
            assert_eq!(parsed, row);
            assert!(
                text.starts_with('{'),
                "record returns a JSON object: {text}"
            );
            assert_eq!(parsed["action"].as_str(), Some(action));
            assert_eq!(parsed["decision"].as_str(), Some("Refused"));
            assert_ne!(parsed["decision"].as_str(), Some("WouldApply"));
            assert_record(action, token, Decision::Refused);
        }
    }

    assert!(
        dir_names(&probe).is_empty(),
        "record wrote a file into the probe dir"
    );
    drop(cwd);
    std::fs::remove_dir(&probe).expect("probe dir removed");

    let src = include_str!("../src/lib.rs");
    for banned in [
        "std::process",
        "process::Command",
        "Command::",
        "std::fs",
        "OpenOptions",
        "File::create",
        "File::open",
        ".spawn(",
        "systemctl",
    ] {
        assert!(
            !src.contains(banned),
            "record writes no file; journal source contains {banned}"
        );
    }
}

struct CwdGuard {
    previous: std::path::PathBuf,
}

impl CwdGuard {
    fn enter(dir: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir).expect("enter probe");
        Self { previous }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

fn probe_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "hades-journal-probe-{}-{nanos}",
        std::process::id()
    ))
}

fn dir_names(path: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();
    let entries = std::fs::read_dir(path).expect("read dir");
    for entry in entries.flatten() {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}
