// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

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
