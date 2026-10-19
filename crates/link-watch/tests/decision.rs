// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! A watch decision is a report only.
//!
//! [`watch_decision`] is the library function under test. This file calls it
//! and checks that the call does not spawn a process, open a socket, or name
//! a service manager. The binary probe loop is not this function.

use link_watch::{watch_decision, WatchFacts, WatchSignature};
use serde_json::Value;
use std::path::PathBuf;

const GOLDEN: [&str; 10] = [
    "at",
    "at_local",
    "laptop_port",
    "model",
    "model_ms",
    "hermes",
    "hermes_ms",
    "france",
    "laptop",
    "events",
];

fn sample(laptop_port: bool, hermes: bool, france: Value, model: &str) -> WatchFacts {
    WatchFacts {
        at: "2026-10-19T16:00:00Z".to_owned(),
        at_local: "2026-10-19T18:00:00+02:00".to_owned(),
        stamp: "16:00:00".to_owned(),
        laptop_port,
        model: model.to_owned(),
        model_ms: 12,
        hermes,
        hermes_ms: 4,
        france,
        laptop: serde_json::json!({ "name": "desk" }),
    }
}

fn socket_fds() -> Vec<String> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir("/proc/self/fd").expect("fd table");
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let text = target.to_string_lossy();
        if text.starts_with("socket:") {
            found.push(text.into_owned());
        }
    }
    found.sort();
    found
}

fn child_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let tasks = std::fs::read_dir("/proc/self/task").expect("tasks");
    for task in tasks.flatten() {
        let path = task.path().join("children");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for word in text.split_whitespace() {
            if let Ok(pid) = word.parse() {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

fn assert_report(value: &Value) {
    let keys: Vec<&str> = value
        .as_object()
        .expect("report object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, GOLDEN);
    for action in ["systemctl", "restart", "kill", "spawn", "exec"] {
        assert!(
            value.get(action).is_none(),
            "{action} is not a report field"
        );
    }
}

#[test]
fn watch_decision_is_report_only() {
    let sockets_before = socket_fds();
    let children_before = child_pids();

    let up = sample(true, true, Value::String("unconfigured".into()), "galion");
    let first = watch_decision(&up, None, &[]);
    assert!(!first.events_changed);
    assert!(first.events.is_empty());
    assert_eq!(first.report["laptop_port"], Value::Bool(true));
    assert_eq!(first.report["model"], Value::String("galion".into()));
    assert_eq!(first.report["model_ms"], serde_json::json!(12));
    assert_eq!(first.report["hermes"], Value::Bool(true));
    assert_eq!(first.report["hermes_ms"], serde_json::json!(4));
    assert_eq!(first.report["france"], Value::String("unconfigured".into()));
    assert_eq!(first.report["events"], Value::Array(Vec::new()));
    assert_eq!(first.report["laptop"]["name"], Value::String("desk".into()));
    assert_report(&first.report);

    let again = watch_decision(&up, Some(&first.signature), &first.events);
    assert!(!again.events_changed);
    assert!(again.events.is_empty());
    assert_eq!(again.signature, first.signature);

    let down = sample(false, false, Value::Bool(false), "");
    let changed = watch_decision(&down, Some(&first.signature), &first.events);
    assert!(changed.events_changed);
    assert_eq!(
        changed.events,
        vec![Value::String(
            "16:00:00  laptop down  model none  hermes down".into()
        )]
    );
    assert_eq!(
        changed.report["events"],
        Value::Array(changed.events.clone())
    );
    assert_eq!(changed.report["laptop_port"], Value::Bool(false));
    assert_eq!(changed.report["hermes"], Value::Bool(false));
    assert_eq!(changed.report["france"], Value::Bool(false));
    assert_eq!(changed.report["model"], Value::String(String::new()));
    assert_report(&changed.report);
    let rendered = serde_json::to_string(&changed.report).expect("report json");
    assert!(
        !rendered.to_ascii_lowercase().contains("systemctl"),
        "{rendered}"
    );

    let mut events = changed.events.clone();
    let mut previous: WatchSignature = changed.signature.clone();
    let mut laptop_port = true;
    for _ in 0..20 {
        let facts = sample(laptop_port, true, Value::Bool(true), "galion");
        let next = watch_decision(&facts, Some(&previous), &events);
        assert!(next.events_changed);
        assert!(next.events.len() <= 12);
        events = next.events;
        previous = next.signature;
        laptop_port = !laptop_port;
    }
    assert_eq!(events.len(), 12);

    assert_eq!(
        socket_fds(),
        sockets_before,
        "watch_decision opened a socket"
    );
    assert_eq!(
        child_pids(),
        children_before,
        "watch_decision spawned a process"
    );

    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("lib source");
    assert!(
        !src.to_ascii_lowercase().contains("systemctl"),
        "library names a service manager"
    );
    for token in [
        "std::process",
        "process::Command",
        "Command::new",
        "Command::spawn",
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "UnixStream",
        "UnixListener",
    ] {
        assert!(!src.contains(token), "{token} in the library function");
    }
}
