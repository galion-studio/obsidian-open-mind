// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B: the corpus LEGIT prose plans written as typed plans. Shared by
//! `tests/typed.rs` and `tests/real_typed.rs`.

use hades_gate::typed::{Plan, Probe, Release, Runbook, Service, Source, Step};

/// The LEGIT prose plans from the corpus, as typed plans (same target).
pub fn legit_typed() -> Vec<(&'static str, Plan)> {
    let sv = |n| Service::new(n).unwrap();
    let rel = |r| Release::new(r).unwrap();
    let probe = |ep, p| Probe::new(ep, p).unwrap();
    let plan = |s| Plan::new(s).unwrap();
    vec![
        (
            "galion-one",
            plan(vec![
                Step::ReadReleases,
                Step::Operator(Runbook::DeployRollback),
                Step::Operator(Runbook::DeployRollbackConsole),
                Step::Verify(probe(None, "/health")),
                Step::KeepFailed,
                Step::RecordLineage,
            ]),
        ),
        (
            "galion-one",
            plan(vec![
                Step::TakeFindings(Source::Demeter),
                Step::ApplyFixes { max: 12 },
                Step::AppendActions,
                Step::RecordLineage,
            ]),
        ),
        (
            "galion-one",
            plan(vec![
                Step::ReadReleases,
                Step::Operator(Runbook::DeployRollback),
            ]),
        ),
        (
            "galion-tuner",
            plan(vec![Step::Switch {
                service: sv("galion-tuner"),
                release: rel("2026-10-20"),
            }]),
        ),
        (
            "galion-tuner",
            plan(vec![
                Step::Restart(sv("galion-tuner")),
                Step::Verify(probe(Some("127.0.0.1:8420"), "/health")),
            ]),
        ),
        (
            "galion-tuner",
            plan(vec![
                Step::Operator(Runbook::DeployRollback),
                Step::KeepFailed,
            ]),
        ),
        (
            "galion-tuner",
            plan(vec![
                Step::Restart(sv("galion-tuner")),
                Step::Verify(probe(None, "/health")),
            ]),
        ),
        (
            "galion-tuner",
            plan(vec![Step::Restart(sv("galion-tuner"))]),
        ),
        (
            "openmind-memory",
            plan(vec![
                Step::Restart(sv("galion-tuner")),
                Step::Verify(probe(None, "/health")),
            ]),
        ),
    ]
}
