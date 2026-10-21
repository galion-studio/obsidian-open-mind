// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 152: `gaia-hades` typed plans against the real `hades-gate` approver
//! (in-process, throwaway keys in a temp dir, nothing on the network).
//!
//! * every typed plan GAIA HADES emits is approved once by `approve_typed`
//!   with a valid token for its exact canonical bytes, under the gate action
//!   `typed_action` names, and a replay is refused;
//! * the prose `plan_for` bytes are refused on the typed path (no free text);
//! * the typed plans equal the shared typed corpus entries for the same
//!   intent, which now name the real VPS unit `tuner`;
//! * the 82 red-team attacks stay 0 on the prose path, and 0/164 on the typed
//!   path (raw + `hades-plan/v1`-wrapped, each with a valid token).

#[path = "../../hades-gate/tests/support/redteam_corpus.rs"]
mod corpus;
#[path = "../../hades-gate/tests/support/typed_legit.rs"]
mod typed_legit;

use std::path::PathBuf;

use gaia_core::{FunctionId, System};
use gaia_hades::typed::{typed_action, typed_plan_bytes, typed_plan_for, TypedRefusal};
use gaia_hades::Hades;
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::safety::review;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::typed::MAGIC;
use hades_gate::{Approver, Refusal, Request, Verdict};

const NOW: u64 = 1_792_000_000;

struct Env {
    dir: PathBuf,
    key: ed25519_dalek::SigningKey,
    gate: Approver,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Env {
    // Unique per test: name + pid + a process-local counter (no wall clock).
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "gaia-hades-typed-{name}-{}-{seq}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let public =
        hades_gate::keys::generate(&dir.join("admin.seed"), &dir.join("admin.pub")).unwrap();
    let key = hades_gate::keys::load_seed(&dir.join("admin.seed")).unwrap();
    let gate = Approver::new(public, dir.join("ledger.jsonl"), dir.join("DISABLED"), 900).unwrap();
    Env { dir, key, gate }
}

fn token(e: &Env, action: &str, target: &str, plan: &[u8], n: u32) -> String {
    let mut nonce = [0u8; 16];
    nonce[..4].copy_from_slice(&n.to_be_bytes());
    Claims {
        action: action.into(),
        target: target.into(),
        plan_hash: plan_hash(plan),
        nonce,
        issued_at: NOW,
        expires_at: NOW + 300,
    }
    .sign(&e.key)
}

fn hades_systems() -> Vec<System> {
    System::ALL
        .iter()
        .copied()
        .filter(|s| s.function() == FunctionId::Hades)
        .collect()
}

#[test]
fn typed_plans_are_approved_once_and_replay_is_refused() {
    let e = env("approve");
    let mut approved = 0;
    let mut n = 0u32;
    for s in hades_systems() {
        let Some(action) = typed_action(s) else {
            continue;
        };
        for intensity in [0u8, 1, 50, 100, 255] {
            for target in ["galion-one", "tuner"] {
                n += 1;
                let bytes = typed_plan_bytes(s, intensity).unwrap();
                let tok = token(&e, action, target, &bytes, n);
                let req = Request {
                    action,
                    target,
                    plan: &bytes,
                    token: &tok,
                };
                let v = e.gate.approve_typed(req, NOW + 1);
                assert!(
                    matches!(v, Verdict::Approved(_)),
                    "{s} {intensity} {target}: {v:?}"
                );
                approved += 1;
                let again = e.gate.approve_typed(req, NOW + 2);
                assert!(
                    matches!(again, Verdict::Refused(_)),
                    "replay {s}: {again:?}"
                );
                // No token at all is a dry-run, never an approval.
                let dry = e.gate.approve_typed(Request { token: "", ..req }, NOW + 3);
                assert!(matches!(dry, Verdict::DryRun), "{dry:?}");
            }
        }
    }
    // rollback + heal-apply, 5 intensities, 2 targets.
    assert_eq!(approved, 20);
}

#[test]
fn prose_plan_bytes_are_refused_on_the_typed_path() {
    let e = env("prose");
    let mut n = 100u32;
    for s in hades_systems() {
        let action = typed_action(s).unwrap_or("heal-apply");
        let prose = gaia_plan_bytes(&Hades::plan_for(s, 100));
        n += 1;
        let tok = token(&e, action, "galion-one", &prose, n);
        let v = e.gate.approve_typed(
            Request {
                action,
                target: "galion-one",
                plan: &prose,
                token: &tok,
            },
            NOW + 1,
        );
        assert!(
            matches!(v, Verdict::Refused(Refusal::TypedPlan(_))),
            "{s}: {v:?}"
        );
    }
}

#[test]
fn a_token_for_one_intensity_does_not_approve_another() {
    let e = env("scope");
    let low = typed_plan_bytes(System::HealApply, 10).unwrap();
    let high = typed_plan_bytes(System::HealApply, 100).unwrap();
    assert_ne!(low, high);
    let tok = token(&e, "heal-apply", "tuner", &low, 1);
    let v = e.gate.approve_typed(
        Request {
            action: "heal-apply",
            target: "tuner",
            plan: &high,
            token: &tok,
        },
        NOW + 1,
    );
    assert!(matches!(v, Verdict::Refused(_)), "{v:?}");
    // And not under another action either.
    let v = e.gate.approve_typed(
        Request {
            action: "rollback",
            target: "tuner",
            plan: &low,
            token: &tok,
        },
        NOW + 1,
    );
    assert!(matches!(v, Verdict::Refused(_)), "{v:?}");
}

#[test]
fn typed_plans_match_the_typed_corpus_which_names_tuner() {
    let legit = typed_legit::legit_typed();
    let rollback = typed_plan_for(System::Rollback, 100).unwrap();
    let heal = typed_plan_for(System::HealApply, 100).unwrap();
    assert!(
        legit.iter().any(|(_, p)| *p == rollback),
        "rollback not in corpus"
    );
    assert!(
        legit.iter().any(|(_, p)| *p == heal),
        "heal-apply not in corpus"
    );
    for (target, p) in &legit {
        let text = p.canonical();
        assert!(!text.contains("galion-tuner"), "{target}: {text}");
        assert_ne!(*target, "galion-tuner");
    }
    let tuner = legit
        .iter()
        .filter(|(t, p)| *t == "tuner" || p.canonical().contains(" tuner"))
        .count();
    assert_eq!(tuner, 6, "corpus plans naming tuner");
}

#[test]
fn redteam_82_stays_zero_on_both_paths() {
    assert_eq!(corpus::ATTACKS.len(), 82);
    let prose = corpus::ATTACKS
        .iter()
        .filter(|c| review(c.target.unwrap_or("galion-tuner"), c.payload.as_bytes()).is_ok())
        .count();
    assert_eq!(prose, 0, "prose path bypasses");

    let e = env("redteam");
    let mut sent = 0;
    let mut bypass = Vec::new();
    for (i, c) in corpus::ATTACKS.iter().enumerate() {
        let target = c.target.unwrap_or("tuner");
        let tok_target = if hades_gate::token::valid_target(target) {
            target
        } else {
            "tuner"
        };
        let wrapped = format!("{MAGIC}\n{}", c.payload);
        for (j, plan) in [c.payload.as_bytes(), wrapped.as_bytes()]
            .iter()
            .enumerate()
        {
            sent += 1;
            let tok = token(
                &e,
                "heal-apply",
                tok_target,
                plan,
                (i * 2 + j) as u32 + 1000,
            );
            let v = e.gate.approve_typed(
                Request {
                    action: "heal-apply",
                    target,
                    plan,
                    token: &tok,
                },
                NOW + 1,
            );
            if !matches!(v, Verdict::Refused(_)) {
                bypass.push(format!("[{}] {}", c.family, c.desc));
            }
        }
    }
    assert_eq!(sent, 164);
    assert!(bypass.is_empty(), "{bypass:#?}");
}

#[test]
fn not_expressible_systems_name_why() {
    for s in hades_systems() {
        if typed_action(s).is_none() {
            let e = typed_plan_for(s, 100).unwrap_err();
            assert_eq!(e, TypedRefusal::NotExpressible(s));
            assert!(e.reason().contains("no stop/revoke/delete step"), "{e}");
        }
    }
}
