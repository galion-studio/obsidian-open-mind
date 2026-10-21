// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S5.2 (slot 208): the plans GAIA really generates still pass the hardened
//! admin-safety rule, through `hades_gate::adapter::check_gaia`, for every
//! intensity and every approvable VPS service name; the never-approvable
//! systems stay refused; and the LEGIT rows of the hades-gate corpus are the
//! real generator output (not hand-copied drift). Throwaway key; nothing is
//! executed.

#[path = "../../hades-gate/tests/support/safety_corpus.rs"]
mod corpus;

use gaia::live_heal::heal_plan;
use gaia_core::System;
use gaia_hades::Hades;
use gaia_live::heal::TestTarget;
use hades_gate::adapter::{check_gaia, gaia_action, gaia_plan_bytes};
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Verdict};
use std::path::PathBuf;

const NOW: u64 = 1_792_000_000;

/// Live VPS units Gaia may heal or roll back (read-only `systemctl
/// list-units` on the VPS, 2026-10-21 20:20 +02:00), minus the protected and
/// never-unroute ones.
const APPROVABLE: &[&str] = &[
    "galion-one",
    "hermes-rs",
    "openmind-memory",
    "openmind-queue",
    "galion-nerve",
    "galion-nerve-shadow",
    "gaia-cortex",
    "tuner",
    "tuner-quick",
    "memory-gateway",
    "galion-desk",
    "galion-health",
    "link-watch-rs",
    "tanga-proto",
    "console",
];

struct Env {
    dir: PathBuf,
    gate: Approver,
    seed: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Env {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("gaia-corpus-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let seed = dir.join("admin.seed");
    let public = hades_gate::keys::generate(&seed, &dir.join("admin.pub")).unwrap();
    let gate = Approver::new(public, dir.join("ledger.jsonl"), dir.join("DISABLED"), 900).unwrap();
    Env { dir, gate, seed }
}

fn token(e: &Env, system: &str, target: &str, plan: &[String], n: u64) -> String {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(&n.to_le_bytes());
    Claims {
        action: gaia_action(system).unwrap_or("heal-apply").into(),
        target: target.into(),
        plan_hash: plan_hash(&gaia_plan_bytes(plan)),
        nonce,
        issued_at: NOW,
        expires_at: NOW + 300,
    }
    .sign(&hades_gate::keys::load_seed(&e.seed).unwrap())
}

#[test]
fn generated_heal_and_rollback_plans_pass_for_every_intensity() {
    let e = env("gen");
    let mut n = 0u64;
    for system in [System::HealApply, System::Rollback] {
        for intensity in 0..=u8::MAX {
            let plan = Hades::plan_for(system, intensity);
            for target in APPROVABLE {
                assert_eq!(
                    check_gaia(&e.gate, system.name(), target, &plan, "", NOW),
                    Verdict::DryRun,
                    "{system} {intensity} {target}: {plan:?}"
                );
            }
            // One signed approval per (system, intensity) on a rotating target.
            let target = APPROVABLE[usize::from(intensity) % APPROVABLE.len()];
            n += 1;
            let t = token(&e, system.name(), target, &plan, n);
            let v = check_gaia(&e.gate, system.name(), target, &plan, &t, NOW);
            assert!(
                matches!(v, Verdict::Approved(_)),
                "{system} {target}: {v:?}"
            );
        }
    }
}

#[test]
fn never_approvable_systems_stay_refused_with_their_real_plans() {
    let e = env("never");
    for (i, system) in [System::KillSwitch, System::Teardown, System::SecretRotation]
        .into_iter()
        .enumerate()
    {
        let plan = Hades::plan_for(system, 100);
        let t = token(&e, system.name(), "galion-one", &plan, 900 + i as u64);
        let v = check_gaia(&e.gate, system.name(), "galion-one", &plan, &t, NOW);
        assert!(
            matches!(&v, Verdict::Refused(r) if r.code() == "action"),
            "{system}: {v:?}"
        );
    }
    let ledger = std::fs::read_to_string(e.dir.join("ledger.jsonl")).unwrap();
    assert_eq!(
        ledger.lines().filter(|l| l.contains("\"refused\"")).count(),
        3
    );
    assert!(!ledger.contains("\"consumed\""));
}

#[test]
fn live_heal_plans_pass_with_real_paths() {
    let dummy = env!("CARGO_BIN_EXE_gaia-dummy");
    for (name, port) in [
        ("gaia-dummy", 19780u16),
        ("gaia-dummy-a", 19781),
        ("canary", 19999),
    ] {
        let t = TestTarget {
            name: name.into(),
            port,
            health_path: "/health".into(),
            argv: vec![dummy.into(), "--port".into(), port.to_string()],
        };
        let plan = heal_plan(&t);
        assert_eq!(
            hades_gate::safety::review(&t.name, &gaia_plan_bytes(&plan)),
            Ok(()),
            "{plan:?}"
        );
    }
}

#[test]
fn corpus_legit_rows_match_the_real_generators() {
    let rows = |target: &str, system: &str| -> Vec<Vec<String>> {
        corpus::LEGIT
            .iter()
            .filter(|(t, s, _)| *t == target && *s == system)
            .map(|(_, _, steps)| steps.iter().map(|s| (*s).to_owned()).collect())
            .collect()
    };
    assert!(rows("galion-one", "rollback").contains(&Hades::plan_for(System::Rollback, 100)));
    assert!(rows("galion-one", "heal-apply").contains(&Hades::plan_for(System::HealApply, 100)));
    assert!(rows("hermes-rs", "heal-apply").contains(&Hades::plan_for(System::HealApply, 0)));
    assert!(rows("openmind-memory", "heal-apply").contains(&Hades::plan_for(System::HealApply, 50)));
    let t = TestTarget {
        name: "gaia-dummy-a".into(),
        port: 20780,
        health_path: "/health".into(),
        argv: vec![
            "/workspace/swarm/target/hades-safety-corpus/release/gaia-dummy".into(),
            "--port".into(),
            "20780".into(),
        ],
    };
    assert!(rows("gaia-dummy-a", "heal-apply").contains(&heal_plan(&t)));
}
