// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Dry-run, would-apply, and refused. No process is spawned.

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
fn would_apply_only_for_the_test_token_on_the_allow_list() {
    for action in ALLOW_LIST {
        assert_eq!(decide(action, APPROVED_TEST_TOKEN), Decision::WouldApply);
        assert_eq!(
            Gate::default().decide(action, APPROVED_TEST_TOKEN),
            Decision::WouldApply
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
