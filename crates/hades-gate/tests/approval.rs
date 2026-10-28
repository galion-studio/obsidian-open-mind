// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real approvals: forged, tampered, expired, replayed, wrong-scope and
//! admin-safety refusals, the disable switch and the ledger. Throwaway keys
//! only (fixed test seeds generated here, never a real secret).

use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use hades_gate::adapter::{check_gaia, gaia_plan_bytes};
use hades_gate::ledger::{Ledger, LedgerError};
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Decision, Refusal, Request, Verdict};

const NOW: u64 = 1_792_000_000;
const PLAN: &[u8] = b"restart galion-tuner\nverify /health";

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
        "hades-approval-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Miri cannot fsync a directory fd (an interpreter limit, not UB), and
    // Ledger::open only syncs the directory when it creates the file. Under
    // Miri the empty ledger is created first so the rest of the ledger code
    // (scan, flock, append, file fsync) still runs. Normal builds unchanged.
    #[cfg(miri)]
    std::fs::write(dir.join("ledger.jsonl"), b"").unwrap();
    let admin = SigningKey::from_bytes(&[0x42; 32]);
    let gate = Approver::new(
        admin.verifying_key(),
        dir.join("ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .unwrap();
    Env { dir, admin, gate }
}

fn claims(nonce: u8) -> Claims {
    Claims {
        action: "heal-apply".into(),
        target: "galion-tuner".into(),
        plan_hash: plan_hash(PLAN),
        nonce: [nonce; 16],
        issued_at: NOW,
        expires_at: NOW + 300,
    }
}

fn req(token: &str) -> Request<'_> {
    Request {
        action: "heal-apply",
        target: "galion-tuner",
        plan: PLAN,
        token,
    }
}

fn code(v: &Verdict) -> &'static str {
    match v {
        Verdict::Refused(r) => r.code(),
        Verdict::DryRun => "dry-run",
        Verdict::Approved(_) => "approved",
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn no_token_is_dry_run() {
    let e = env("dry");
    let v = e.gate.approve(req(""), NOW);
    assert_eq!(v, Verdict::DryRun);
    assert_eq!(v.decision(), Decision::DryRun);
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn valid_token_approves_once_then_replay_is_refused() {
    let e = env("replay");
    let t = claims(1).sign(&e.admin);
    let v = e.gate.approve(req(&t), NOW + 1);
    assert!(matches!(v, Verdict::Approved(_)), "{v:?}");
    assert_eq!(v.decision(), Decision::WouldApply);
    let again = e.gate.approve(req(&t), NOW + 2);
    assert_eq!(code(&again), "replayed");
    // A second gate on the same ledger (e.g. after a restart) also refuses.
    let other = Approver::new(
        e.admin.verifying_key(),
        e.dir.join("ledger.jsonl"),
        e.dir.join("DISABLED"),
        900,
    )
    .unwrap();
    assert_eq!(code(&other.approve(req(&t), NOW + 3)), "replayed");
    // The ledger never stores the token string.
    let text = std::fs::read_to_string(e.dir.join("ledger.jsonl")).unwrap();
    assert!(!text.contains(&t));
    assert!(!text.contains(t.split('.').nth(2).unwrap()));
    assert!(e.gate.ledger().is_consumed(&[1; 16]).unwrap());
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn forged_token_is_refused() {
    let e = env("forged");
    let mallory = SigningKey::from_bytes(&[0x13; 32]);
    let t = claims(2).sign(&mallory);
    assert_eq!(code(&e.gate.approve(req(&t), NOW)), "forged");
    for junk in ["approved-test", "hades1.00.00", "x"] {
        assert_eq!(code(&e.gate.approve(req(junk), NOW)), "malformed", "{junk}");
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn tampered_token_is_refused() {
    let e = env("tamper");
    let t = claims(3).sign(&e.admin);
    let mut c = claims(3);
    c.target = "galion-one".into();
    let sig = t.rsplit('.').next().unwrap();
    let forged = format!(
        "hades1.{}.{sig}",
        hades_gate::token::to_hex(c.payload().as_bytes())
    );
    let r = Request {
        target: "galion-one",
        ..req(&forged)
    };
    assert_eq!(code(&e.gate.approve(r, NOW)), "forged");
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn expired_token_is_refused() {
    let e = env("expired");
    let t = claims(4).sign(&e.admin);
    assert_eq!(code(&e.gate.approve(req(&t), NOW + 300)), "expired");
    assert_eq!(code(&e.gate.approve(req(&t), NOW + 10_000)), "expired");
    // Not consumed by the failed attempts.
    assert!(!e.gate.ledger().is_consumed(&[4; 16]).unwrap());
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn wrong_scope_is_refused() {
    let e = env("scope");
    let t = claims(5).sign(&e.admin);
    let wrong_action = Request {
        action: "rollback",
        ..req(&t)
    };
    let wrong_target = Request {
        target: "galion-one",
        ..req(&t)
    };
    let wrong_plan = Request {
        plan: b"restart galion-tuner\nverify /health\n",
        ..req(&t)
    };
    for r in [wrong_action, wrong_target, wrong_plan] {
        assert_eq!(code(&e.gate.approve(r, NOW)), "wrong-scope", "{r:?}");
    }
    // Still usable for its exact scope.
    assert!(matches!(e.gate.approve(req(&t), NOW), Verdict::Approved(_)));
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn kill_and_rotate_refused_even_with_a_valid_token() {
    let e = env("kill");
    for action in ["kill", "rotate", "teardown", "unknown"] {
        let mut c = claims(6);
        c.action = action.into();
        let t = c.sign(&e.admin);
        let r = Request { action, ..req(&t) };
        assert_eq!(code(&e.gate.approve(r, NOW)), "action", "{action}");
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn admin_safety_beats_a_valid_token() {
    let e = env("safety");
    let cases: [(&str, &[u8]); 6] = [
        ("galion-tuner", b"rm -rf /var/lib/galion"),
        ("galion-tuner", b"cat /root/galion-secrets/x"),
        ("galion-tuner", b"usermod -L slavik"),
        ("galion-tuner", b"systemctl stop sshd"),
        ("hades-gate", b"restart hades-gate"),
        ("sshd", b"restart"),
    ];
    for (i, (target, plan)) in cases.into_iter().enumerate() {
        let c = Claims {
            target: target.into(),
            plan_hash: plan_hash(plan),
            nonce: [100 + u8::try_from(i).unwrap(); 16],
            ..claims(0)
        };
        let t = c.sign(&e.admin);
        let r = Request {
            action: "heal-apply",
            target,
            plan,
            token: &t,
        };
        let v = e.gate.approve(r, NOW);
        assert_eq!(code(&v), "admin-safety", "{target} {:?}", v);
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn admin_can_always_disable() {
    let e = env("disable");
    let t = claims(7).sign(&e.admin);
    std::fs::write(e.dir.join("DISABLED"), b"x").unwrap();
    assert!(e.gate.is_disabled());
    assert_eq!(code(&e.gate.approve(req(&t), NOW)), "disabled");
    assert_eq!(code(&e.gate.approve(req(""), NOW)), "disabled");
    std::fs::remove_file(e.dir.join("DISABLED")).unwrap();
    assert!(matches!(e.gate.approve(req(&t), NOW), Verdict::Approved(_)));
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn corrupt_ledger_fails_closed() {
    let e = env("corrupt");
    std::fs::write(e.dir.join("ledger.jsonl"), b"{\"event\":\"consumed\"").unwrap();
    let t = claims(8).sign(&e.admin);
    let v = e.gate.approve(req(&t), NOW);
    assert!(
        matches!(
            v,
            Verdict::Refused(Refusal::Ledger(LedgerError::Corrupt(_)))
        ),
        "{v:?}"
    );
}

#[test]
#[cfg_attr(
    miri,
    ignore = "threads contend on the ledger flock; Miri does not support a blocking flock (interpreter limit, not UB), so it is skipped in the FS pass too (--skip); covered by the native test run"
)]
fn concurrent_consumers_approve_exactly_once() {
    let e = env("race");
    let t = claims(9).sign(&e.admin);
    let path = e.dir.join("ledger.jsonl");
    let wins: usize = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let c = claims(9);
                s.spawn(move || Ledger::open(path).unwrap().consume(&c, NOW).is_ok())
            })
            .collect();
        handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum()
    });
    assert_eq!(wins, 1);
    assert_eq!(code(&e.gate.approve(req(&t), NOW)), "replayed");
}

#[test]
#[cfg_attr(
    miri,
    ignore = "temp-dir ledger (FS + clock), refused under Miri isolation; run in the FS pass"
)]
fn gaia_adapter_maps_systems() {
    let e = env("gaia");
    let steps = vec![
        "read the current and the previous release".to_owned(),
        "operator runs deploy/rollback.sh".to_owned(),
    ];
    let plan = gaia_plan_bytes(&steps);
    let c = Claims {
        action: "rollback".into(),
        target: "galion-one".into(),
        plan_hash: plan_hash(&plan),
        ..claims(10)
    };
    let t = c.sign(&e.admin);
    let v = check_gaia(&e.gate, "rollback", "galion-one", &steps, &t, NOW);
    assert_eq!(v.decision(), Decision::WouldApply, "{v:?}");
    for sys in ["kill-switch", "teardown", "secret-rotation-plan"] {
        let v = check_gaia(&e.gate, sys, "galion-one", &steps, &t, NOW);
        assert_eq!(v.decision(), Decision::Refused, "{sys}");
    }
    assert_eq!(
        check_gaia(&e.gate, "heal-apply", "galion-one", &steps, "", NOW),
        Verdict::DryRun
    );
}
