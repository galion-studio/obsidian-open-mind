// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Security plan S3.1: adversarial review of the admin-safety rule.
//!
//! Each attack in `tests/attacks/mod.rs` gets a **validly signed**, in-scope,
//! unexpired, first-use token from a throwaway admin key, so the only thing
//! between the attack and `WouldApply` is the safety rule. Every attack must
//! be refused; every known-good GAIA/operator plan must still be approved.

mod attacks;

use std::path::PathBuf;

use attacks::{ATTACKS, GOOD};
use ed25519_dalek::SigningKey;
use hades_gate::safety::{self, Hazard};
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Decision, Request, Verdict};

const NOW: u64 = 1_792_000_000;

struct Env {
    dir: PathBuf,
    admin: SigningKey,
    gate: Approver,
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
    let dir = std::env::temp_dir().join(format!(
        "hades-adversarial-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Throwaway test seed, never a real key.
    let admin = SigningKey::from_bytes(&[0x5a; 32]);
    let gate = Approver::new(
        admin.verifying_key(),
        dir.join("ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .unwrap();
    Env { dir, admin, gate }
}

/// Mint a valid token for exactly this request and run it through the gate.
fn signed_verdict(e: &Env, i: usize, target: &str, plan: &[u8]) -> Verdict {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(&(i as u64).to_le_bytes());
    let claims = Claims {
        action: "heal-apply".into(),
        target: target.into(),
        plan_hash: plan_hash(plan),
        nonce,
        issued_at: NOW,
        expires_at: NOW + 300,
    };
    let token = claims.sign(&e.admin);
    e.gate.approve(
        Request {
            action: "heal-apply",
            target,
            plan,
            token: &token,
        },
        NOW,
    )
}

#[test]
fn every_attack_is_refused_even_with_a_valid_token() {
    let e = env("attacks");
    let bypasses: Vec<String> = ATTACKS
        .iter()
        .enumerate()
        .filter(|(i, at)| {
            signed_verdict(&e, *i, at.target, at.plan.as_bytes()).decision() == Decision::WouldApply
        })
        .map(|(i, at)| {
            format!(
                "#{i} [{}] {:?} / {:?}",
                at.family.label(),
                at.target,
                at.plan
            )
        })
        .collect();
    assert!(
        bypasses.is_empty(),
        "{} bypass(es) reached WouldApply:\n{}",
        bypasses.len(),
        bypasses.join("\n")
    );
}

#[test]
fn good_plans_still_approve() {
    let e = env("good");
    for (i, (target, plan)) in GOOD.iter().enumerate() {
        let v = signed_verdict(&e, 10_000 + i, target, plan.as_bytes());
        assert_eq!(
            v.decision(),
            Decision::WouldApply,
            "{target} {plan:?}: {v:?}"
        );
    }
}

#[test]
fn corpus_is_large_and_covers_every_family() {
    use attacks::Family::{Access, FileDestruction, Shell, Traversal, Unicode};
    assert!(ATTACKS.len() >= 150, "{}", ATTACKS.len());
    for f in [Shell, FileDestruction, Access, Unicode, Traversal] {
        assert!(
            ATTACKS.iter().filter(|a| a.family == f).count() >= 9,
            "{f:?}"
        );
    }
}

#[test]
fn every_plan_line_must_start_with_an_allow_listed_verb() {
    for plan in [
        "nuke galion-tuner",
        "restart galion-tuner\nformat the disk",
        "please rm the db",
        "operator runs deploy/rollback.sh\nthen wipe /var/lib/galion",
    ] {
        assert!(
            matches!(
                safety::review_plan(plan.as_bytes()),
                Err(Hazard::NotAllowListed(_))
            ),
            "{plan}: {:?}",
            safety::review_plan(plan.as_bytes())
        );
    }
}

#[test]
fn operator_scripts_are_an_exact_allow_list() {
    for s in [
        "deploy/rollback.sh",
        "deploy/rollback-console.sh",
        "deploy/switch.sh",
        "deploy/switch-console.sh",
    ] {
        let plan = format!("operator runs {s}");
        assert_eq!(safety::review_plan(plan.as_bytes()), Ok(()), "{s}");
    }
    for s in [
        "deploy/staged/x.sh",
        "deploy/rollback.sh.bak",
        "./deploy/rollback.sh",
        "deploy//rollback.sh",
        "/tmp/p.sh",
    ] {
        let plan = format!("operator runs {s}");
        assert!(safety::review_plan(plan.as_bytes()).is_err(), "{s}");
    }
}

#[test]
fn safety_runs_before_the_token_so_no_token_is_needed_to_refuse() {
    let e = env("notoken");
    for at in ATTACKS {
        let v = e.gate.approve(
            Request {
                action: "heal-apply",
                target: at.target,
                plan: at.plan.as_bytes(),
                token: "",
            },
            NOW,
        );
        assert_eq!(v.decision(), Decision::Refused, "{:?}", at.plan);
    }
}

#[test]
fn plan_may_only_name_our_unprotected_services() {
    assert_eq!(
        safety::review("galion-tuner", b"restart galion-tuner"),
        Ok(())
    );
    // The signature binds plan and target; naming another of OUR services is
    // allowed (approval.rs wrong_scope_is_refused relies on it).
    assert_eq!(
        safety::review("galion-tuner", b"restart galion-one"),
        Ok(())
    );
    for plan in [
        "restart the consumers",
        "restart hades-gate",
        "restart galion-control",
        "restart nginx",
        "restart galion-",
        "restart galion--x",
        "restart galion-tuner galion-one",
    ] {
        assert!(
            safety::review("galion-tuner", plan.as_bytes()).is_err(),
            "{plan}"
        );
    }
}

#[test]
fn heal_plan_v1_start_is_bound_to_the_named_service() {
    let good = "gaia heal-apply plan v1 (box test service)\n\
        service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)\n\
        probe GET /health must answer status ok\n\
        start [\"/x/gaia-dummy\",\"--port\",\"19231\"] detached in its own process group, stdin closed, no shell\n\
        at most 3 restarts of gaia-dummy per rolling hour\n\
        record the result on the nerve";
    assert_eq!(safety::review("gaia-dummy", good.as_bytes()), Ok(()));
    let swap = |from: &str, to: &str| good.replace(from, to);
    for evil in [
        swap("/x/gaia-dummy", "/bin/sh"),
        swap("/x/gaia-dummy", "/usr/bin/gaia-dummy"),
        swap("/x/gaia-dummy", "/x/gaia-other"),
        swap("/x/gaia-dummy", "gaia-dummy"),
        swap("/x/gaia-dummy", "/x/../bin/gaia-dummy"),
        swap("\"19231\"]", "\"19231\",\"--data\",\"/etc\"]"),
        swap("\"19231\"]", "\"19231\",\"--data=/root/x\"]"),
        swap("\"19231\"]", "\"19231\",\"a b\"]"),
        swap("\"19231\"]", "\"19231\",\"x\\\"\"]"),
        swap(", no shell", ", via sh"),
        swap("127.0.0.1:19231", "0.0.0.0:19231"),
        swap("127.0.0.1:19231", "127.0.0.1:22"),
        swap("19000-19999", "1-65535"),
        swap("at most 3", "at most 99"),
        swap("/health", "/../health"),
    ] {
        assert!(
            safety::review("gaia-dummy", evil.as_bytes()).is_err(),
            "{evil}"
        );
    }
}
