// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Dry-run, would-apply, and refused. No process is spawned.
#![allow(deprecated)]

use hades_gate::{
    decide, Action, Decision, Gate, Mode, ALLOW_LIST, ALWAYS_REFUSED, APPROVED_TEST_TOKEN,
};

#[test]
fn dry_run_is_the_default() {
    assert_eq!(Mode::default(), Mode::DryRun);
    assert_eq!(Gate::default().mode(), Mode::DryRun);
    assert_eq!(Gate::new().mode(), Mode::DryRun);

    for action in ALLOW_LIST {
        assert_eq!(decide(action, ""), Decision::DryRun);
        assert_eq!(decide(action, "no"), Decision::DryRun);
        assert_eq!(decide(action, "approved-test "), Decision::DryRun);
        assert_eq!(decide(action, "Approved-test"), Decision::DryRun);
        assert_eq!(Gate::default().decide(action, ""), Decision::DryRun);
    }

    assert_eq!(decide("heal_apply", APPROVED_TEST_TOKEN), Decision::DryRun);
    assert_eq!(decide("", APPROVED_TEST_TOKEN), Decision::DryRun);
    assert_eq!(decide("unknown", APPROVED_TEST_TOKEN), Decision::DryRun);
}

#[test]
fn the_public_test_token_no_longer_approves() {
    for action in ALLOW_LIST {
        assert_eq!(decide(action, APPROVED_TEST_TOKEN), Decision::DryRun);
        assert_eq!(
            Gate::default().decide(action, APPROVED_TEST_TOKEN),
            Decision::DryRun
        );
    }
    assert_eq!(APPROVED_TEST_TOKEN, "approved-test");
}

#[test]
fn kill_and_rotate_are_refused() {
    for action in ALWAYS_REFUSED {
        assert_eq!(decide(action, ""), Decision::Refused);
        assert_eq!(decide(action, "no"), Decision::Refused);
        assert_eq!(decide(action, APPROVED_TEST_TOKEN), Decision::Refused);
        assert_eq!(
            Gate::default().decide(action, APPROVED_TEST_TOKEN),
            Decision::Refused
        );
    }
}

#[test]
fn action_names_match_the_two_lists() {
    let actions = [
        Action::HealApply,
        Action::Switch,
        Action::Rollback,
        Action::Kill,
        Action::Rotate,
    ];
    for action in actions {
        let name = action.as_str();
        assert_eq!(Action::parse(name), Some(action));
        let allowed = ALLOW_LIST.contains(&name);
        let refused = ALWAYS_REFUSED.contains(&name);
        assert_ne!(allowed, refused);
        assert_eq!(action.is_allow_listed(), allowed);
    }
    assert_eq!(ALLOW_LIST.len() + ALWAYS_REFUSED.len(), actions.len());
}

/// `kill` and `rotate` stay refused with the approved-test token.
/// `WouldApply` is a decision only: no process is spawned and no file is written.
#[test]
fn approved_token_refuses_kill_and_rotate_and_would_apply_is_not_an_apply() {
    assert_eq!(APPROVED_TEST_TOKEN, "approved-test");

    for action in ["kill", "rotate"] {
        assert_eq!(decide(action, APPROVED_TEST_TOKEN), Decision::Refused);
        assert_eq!(decide(action, "approved-test"), Decision::Refused);
        assert_eq!(
            Gate::default().decide(action, APPROVED_TEST_TOKEN),
            Decision::Refused
        );
        assert_ne!(decide(action, APPROVED_TEST_TOKEN), Decision::WouldApply);
    }

    let before_children = child_pids();
    let probe = probe_dir();
    std::fs::create_dir_all(&probe).expect("probe dir");
    assert!(dir_names(&probe).is_empty(), "probe dir starts empty");

    for action in ALLOW_LIST {
        let decision = decide(action, APPROVED_TEST_TOKEN);
        assert_eq!(decision, Decision::DryRun);
        assert_eq!(
            Gate::default().decide(action, "approved-test"),
            Decision::DryRun
        );
    }

    assert!(
        dir_names(&probe).is_empty(),
        "WouldApply wrote a file into the probe dir"
    );
    std::fs::remove_dir(&probe).expect("probe dir removed");
    assert_eq!(before_children, child_pids(), "decide spawned a process");

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
            "WouldApply is not an apply; gate source contains {banned}"
        );
    }
}

fn probe_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("hades-gate-{}-{nanos}", std::process::id()))
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

fn child_pids() -> Vec<String> {
    let mut kids = Vec::new();
    let tasks = std::fs::read_dir("/proc/self/task").expect("task");
    for entry in tasks.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("children")) else {
            continue;
        };
        kids.extend(text.split_whitespace().map(str::to_owned));
    }
    kids.sort();
    kids
}
