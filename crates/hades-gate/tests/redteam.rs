// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S3.1: every red-team attempt must be refused by the admin-safety rule, and
//! the legitimate plans must still pass. Table: docs/security/hades-redteam.md.

#[path = "support/redteam_corpus.rs"]
mod corpus;

use hades_gate::safety::review;

#[test]
fn every_redteam_attempt_is_refused() {
    let bypasses: Vec<String> = corpus::ATTACKS
        .iter()
        .filter(|c| review(c.target.unwrap_or("galion-tuner"), c.payload.as_bytes()).is_ok())
        .map(|c| format!("[{}] {}: {:?}", c.family, c.desc, c.payload))
        .collect();
    assert!(
        bypasses.is_empty(),
        "{} bypass(es) got WouldApply:\n{}",
        bypasses.len(),
        bypasses.join("\n")
    );
}

#[test]
fn legitimate_plans_still_pass() {
    for (target, plan) in corpus::LEGIT {
        assert_eq!(
            review(target, plan.as_bytes()),
            Ok(()),
            "{target}: {plan:?}"
        );
    }
}

#[test]
fn legitimate_targets_still_pass() {
    for t in corpus::LEGIT_TARGETS {
        assert_eq!(review(t, b"restart galion-tuner"), Ok(()), "{t}");
    }
}
