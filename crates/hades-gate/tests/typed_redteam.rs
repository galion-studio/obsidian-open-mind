// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B (typed plans): the same S3.1 red-team corpus, proven against the
//! typed path. Every one of the 82 attack payloads fails to parse as a
//! canonical typed plan, so none can reach a token. The legitimate intents,
//! re-expressed as typed plans, parse and round-trip byte-for-byte.
//!
//! The prose path and its corpus (`tests/redteam.rs`) are untouched, so the
//! 82 attacks stay at 0 there too.

#[path = "support/redteam_corpus.rs"]
mod corpus;

use hades_gate::typed::{Plan, Probe, Release, Runbook, Service, Source, Step};

#[test]
fn every_redteam_attempt_fails_typed_parse() {
    // The typed parser accepts only a canonical typed plan. Each attack payload
    // is prose (and most carry shell/Unicode tricks), so none parse. Target-only
    // attacks carry a benign prose plan that still lacks the typed header.
    let bypasses: Vec<String> = corpus::ATTACKS
        .iter()
        .filter(|c| Plan::parse(c.payload.as_bytes()).is_ok())
        .map(|c| format!("[{}] {}: {:?}", c.family, c.desc, c.payload))
        .collect();
    assert!(
        bypasses.is_empty(),
        "{} attack(s) parsed as a typed plan:\n{}",
        bypasses.len(),
        bypasses.join("\n")
    );
}

/// The legitimate intents from the corpus, re-expressed as typed plans. These
/// are what Option B replaces the prose with.
fn legit_typed_plans() -> Vec<Plan> {
    let tuner = || Service::new("tuner").unwrap();
    let one = || Service::new("galion-one").unwrap();
    vec![
        // rollback runbook with verify/keep/record
        Plan::new(vec![
            Step::ReadReleases,
            Step::Operator(Runbook::DeployRollback),
            Step::Verify(Probe::new(None, "/health").unwrap()),
            Step::KeepFailed,
            Step::RecordLineage,
        ])
        .unwrap(),
        // heal-apply
        Plan::new(vec![
            Step::TakeFindings(Source::Demeter),
            Step::ApplyFixes { max: 12 },
            Step::AppendActions,
            Step::RecordLineage,
        ])
        .unwrap(),
        Plan::new(vec![
            Step::ReadReleases,
            Step::Operator(Runbook::DeployRollback),
        ])
        .unwrap(),
        Plan::new(vec![Step::Switch {
            service: one(),
            release: Release::new("2026-10-20").unwrap(),
        }])
        .unwrap(),
        Plan::new(vec![
            Step::Restart(tuner()),
            Step::Verify(Probe::new(Some("127.0.0.1:8420"), "/health").unwrap()),
        ])
        .unwrap(),
        Plan::new(vec![
            Step::Operator(Runbook::DeployRollback),
            Step::KeepFailed,
        ])
        .unwrap(),
        Plan::new(vec![Step::Restart(tuner())]).unwrap(),
    ]
}

#[test]
fn legitimate_typed_plans_parse_and_round_trip() {
    for p in legit_typed_plans() {
        let bytes = p.canonical_bytes();
        let back = Plan::parse(&bytes).expect("legit typed plan must parse");
        assert_eq!(back, p, "{:?}", p.canonical());
        assert_eq!(back.canonical_bytes(), bytes);
    }
}
