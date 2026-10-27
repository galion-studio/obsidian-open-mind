// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B prototype (typed, signed plan steps): the same 82 red-team
//! attempts, each signed with a **valid** admin token for its exact bytes,
//! must all be refused by `Approver::approve_typed`, both as raw prose and
//! wrapped in the `hades-plan/v1` header. The legitimate prose plans, written
//! as typed plans, must be approved. Throwaway keys only.

#[path = "support/redteam_corpus.rs"]
mod corpus;
#[path = "support/typed_legit.rs"]
mod typed_legit;
use typed_legit::legit_typed;

use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use hades_gate::safety::review;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::typed::{Plan, Service, Step, MAGIC};
use hades_gate::{Approver, Request, Verdict};

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
    let dir =
        std::env::temp_dir().join(format!("hades-typed-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let admin = SigningKey::from_bytes(&[0x24; 32]);
    let gate = Approver::new(
        admin.verifying_key(),
        dir.join("ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .unwrap();
    Env { dir, admin, gate }
}

/// A valid token for exactly (heal-apply, target, plan) with a unique nonce.
fn token(e: &Env, target: &str, plan: &[u8], n: u32) -> String {
    let mut nonce = [0u8; 16];
    nonce[..4].copy_from_slice(&n.to_be_bytes());
    Claims {
        action: "heal-apply".into(),
        target: target.into(),
        plan_hash: plan_hash(plan),
        nonce,
        issued_at: NOW,
        expires_at: NOW + 300,
    }
    .sign(&e.admin)
}

fn decide(e: &Env, target: &str, plan: &[u8], n: u32) -> Verdict {
    // Tokens are signed for a valid target name; for a malformed target the
    // gate must refuse before the token matters.
    let tok_target = if hades_gate::token::valid_target(target) {
        target
    } else {
        "galion-tuner"
    };
    let tok = token(e, tok_target, plan, n);
    e.gate.approve_typed(
        Request {
            action: "heal-apply",
            target,
            plan,
            token: &tok,
        },
        NOW + 1,
    )
}

#[test]
fn prose_path_still_refuses_all_82() {
    // Option A (the live path) is unchanged: 82 attempts, 0 bypasses.
    assert_eq!(corpus::ATTACKS.len(), 82);
    let n = corpus::ATTACKS
        .iter()
        .filter(|c| review(c.target.unwrap_or("galion-tuner"), c.payload.as_bytes()).is_ok())
        .count();
    assert_eq!(n, 0);
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn typed_path_refuses_all_82_raw_and_header_wrapped() {
    let e = env("corpus");
    let mut bypass = Vec::new();
    let mut n = 0u32;
    for c in corpus::ATTACKS {
        let target = c.target.unwrap_or("galion-tuner");
        let wrapped = format!("{MAGIC}\n{}", c.payload);
        for plan in [c.payload.as_bytes(), wrapped.as_bytes()] {
            n += 1;
            let v = decide(&e, target, plan, n);
            if !matches!(v, Verdict::Refused(_)) {
                bypass.push(format!("[{}] {}: {:?}", c.family, c.desc, plan));
            }
        }
    }
    assert_eq!(n, 164);
    assert!(
        bypass.is_empty(),
        "{} bypass(es):\n{}",
        bypass.len(),
        bypass.join("\n")
    );
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn legit_typed_plans_are_approved_once() {
    let e = env("legit");
    let all = legit_typed();
    assert_eq!(all.len(), corpus::LEGIT.len());
    for (i, (target, plan)) in all.iter().enumerate() {
        let bytes = plan.canonical_bytes();
        let tok = token(&e, target, &bytes, 10_000 + i as u32);
        let r = Request {
            action: "heal-apply",
            target,
            plan: &bytes,
            token: &tok,
        };
        assert!(
            matches!(e.gate.approve_typed(r, NOW + 1), Verdict::Approved(_)),
            "{target}: {}",
            plan.canonical()
        );
        // Single use is unchanged on the typed path.
        assert!(matches!(
            e.gate.approve_typed(r, NOW + 2),
            Verdict::Refused(_)
        ));
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn typed_specific_attacks_are_refused() {
    let e = env("typed-attacks");
    let attacks: &[(&str, &str)] = &[
        ("galion-tuner", "hades-plan/v1\nrestart sshd"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-control"),
        ("galion-tuner", "hades-plan/v1\nrestart hades-gate"),
        ("galion-tuner", "hades-plan/v1\nrestart root"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tuner;rm"),
        ("galion-tuner", "hades-plan/v1\nrestart $(id)"),
        (
            "galion-tuner",
            "hades-plan/v1\nswitch galion-one release=../../etc",
        ),
        (
            "galion-tuner",
            "hades-plan/v1\nswitch galion-one release=a b",
        ),
        ("galion-tuner", "hades-plan/v1\nverify ../../root/.ssh/"),
        ("galion-tuner", "hades-plan/v1\nverify /a|b"),
        (
            "galion-tuner",
            "hades-plan/v1\nverify evil.example:0 /health",
        ),
        ("galion-tuner", "hades-plan/v1\noperator deploy/rollback.sh"),
        ("galion-tuner", "hades-plan/v1\noperator wipe-disk"),
        ("galion-tuner", "hades-plan/v1\ntake-findings internet"),
        ("galion-tuner", "hades-plan/v1\napply-fixes max=0"),
        ("galion-tuner", "hades-plan/v1\napply-fixes max=101"),
        ("galion-tuner", "hades-plan/v1\napply-fixes max=-1"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tuner\r"),
        ("galion-tuner", "hades-plan/v1\r\nrestart galion-tuner"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tuner\n"),
        ("galion-tuner", "hades-plan/v1\nrestart\tgalion-tuner"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tuner\u{0}"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tun\u{e9}r"),
        ("galion-tuner", "HADES-PLAN/V1\nrestart galion-tuner"),
        ("galion-tuner", "hades-plan/v1\nRESTART galion-tuner"),
        ("galion-tuner", "hades-plan/v1\nrestart galion-tuner extra"),
        ("galion-tuner", "hades-plan/v1\ndelete galion-tuner"),
        ("galion-tuner", "hades-plan/v1\nexec rm -rf /"),
        ("sshd", "hades-plan/v1\nrestart galion-tuner"),
        ("galion-control", "hades-plan/v1\nrestart galion-tuner"),
    ];
    for (i, (target, plan)) in attacks.iter().enumerate() {
        let v = decide(&e, target, plan.as_bytes(), 20_000 + i as u32);
        assert!(matches!(v, Verdict::Refused(_)), "{target}: {plan:?}");
    }
    // Too many steps and too many bytes.
    let many = format!("{MAGIC}{}", "\nrecord-lineage".repeat(65));
    assert!(matches!(
        decide(&e, "galion-tuner", many.as_bytes(), 30_000),
        Verdict::Refused(_)
    ));
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn token_for_prose_does_not_approve_typed_and_vice_versa() {
    // The signature binds the exact bytes: a token signed for the prose
    // rendering cannot approve the typed bytes, and the reverse.
    let e = env("cross");
    let plan = Plan::new(vec![Step::Restart(Service::new("galion-tuner").unwrap())]).unwrap();
    let typed = plan.canonical_bytes();
    let prose = plan.render_prose().into_bytes();
    let tok = token(&e, "galion-tuner", &prose, 40_000);
    let v = e.gate.approve_typed(
        Request {
            action: "heal-apply",
            target: "galion-tuner",
            plan: &typed,
            token: &tok,
        },
        NOW + 1,
    );
    assert!(matches!(v, Verdict::Refused(_)));
    let tok = token(&e, "galion-tuner", &typed, 40_001);
    let v = e.gate.approve(
        Request {
            action: "heal-apply",
            target: "galion-tuner",
            plan: &prose,
            token: &tok,
        },
        NOW + 1,
    );
    assert!(matches!(v, Verdict::Refused(_)));
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn typed_dry_run_and_disable() {
    let e = env("dry");
    let bytes = Plan::new(vec![Step::RecordLineage])
        .unwrap()
        .canonical_bytes();
    let r = Request {
        action: "heal-apply",
        target: "galion-tuner",
        plan: &bytes,
        token: "",
    };
    assert!(matches!(e.gate.approve_typed(r, NOW), Verdict::DryRun));
    std::fs::write(e.gate.disable_file(), b"").unwrap();
    assert!(matches!(e.gate.approve_typed(r, NOW), Verdict::Refused(_)));
}
