// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S5.2 (slot 208): the admin-safety corpus through
//! `hades_gate::adapter::check_gaia` with VALID admin tokens. Every attack is
//! refused by the gate before the token is looked at; the legit plans are
//! approved once and dry-run without a token. Throwaway key only (fixed test
//! seed); plans are data and are never executed. Table:
//! docs/security/hades-safety-corpus.md.

#[path = "support/safety_corpus.rs"]
mod corpus;

use std::collections::BTreeMap;
use std::path::PathBuf;

use corpus::{Case, ATTACKS, CATEGORIES, LEGIT};
use ed25519_dalek::SigningKey;
use hades_gate::adapter::{check_gaia, gaia_action, gaia_plan_bytes};
use hades_gate::safety::Hazard;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Refusal, Verdict};

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
        "hades-corpus-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
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

fn steps(raw: &[&str]) -> Vec<String> {
    raw.iter().map(|s| (*s).to_owned()).collect()
}

/// A valid token for exactly this request. Unknown systems get a
/// `heal-apply` token (an attacker replaying a real heal approval).
fn token(admin: &SigningKey, system: &str, target: &str, plan: &[String], n: usize) -> String {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(&(n as u64).to_le_bytes());
    nonce[15] = 0xa5;
    Claims {
        action: gaia_action(system).unwrap_or("heal-apply").into(),
        target: target.into(),
        plan_hash: plan_hash(&gaia_plan_bytes(plan)),
        nonce,
        issued_at: NOW,
        expires_at: NOW + 300,
    }
    .sign(admin)
}

fn hazard_cat(h: Hazard) -> &'static str {
    match h {
        Hazard::Destructive(_) => "destructive",
        Hazard::SecretExposure(_) => "secret",
        Hazard::AdminAccount(_) => "account",
        Hazard::Opaque(_) => "opaque",
        Hazard::ProtectedTarget(_) => "protected",
    }
}

/// Which refusals count as the right one for a category.
fn accepted(cat: &str, r: &Refusal) -> bool {
    match (cat, r) {
        ("action", Refusal::ActionNotApprovable(_)) => true,
        ("protected" | "never-unroute", Refusal::Token(_)) => r.code() == "malformed",
        ("never-unroute", Refusal::Safety(Hazard::ProtectedTarget(m))) => {
            m.starts_with("never-unroute") || *m == "target is not a plain service name"
        }
        (_, Refusal::Safety(h)) => hazard_cat(*h) == cat,
        _ => false,
    }
}

fn describe(c: &Case, v: &Verdict) -> String {
    let got = match v {
        Verdict::Refused(r) => format!("refused {} ({})", r.code(), r.reason()),
        Verdict::DryRun => "DRY-RUN (not refused)".into(),
        Verdict::Approved(_) => "APPROVED".into(),
    };
    format!(
        "[{}] {}: system={:?} target={:?} steps={:?} -> {got}",
        c.cat, c.desc, c.system, c.target, c.steps
    )
}

#[test]
fn corpus_covers_every_category() {
    let mut per: BTreeMap<&str, usize> = BTreeMap::new();
    for c in ATTACKS {
        assert!(CATEGORIES.contains(&c.cat), "unknown category {}", c.cat);
        *per.entry(c.cat).or_default() += 1;
    }
    for cat in CATEGORIES {
        let n = per.get(cat).copied().unwrap_or(0);
        assert!(n >= 10, "category {cat} has only {n} cases");
    }
    assert!(ATTACKS.len() >= 300, "{} attacks", ATTACKS.len());
    assert!(LEGIT.len() >= 20);
}

#[test]
fn every_attack_is_refused_even_with_a_valid_token() {
    let e = env("attacks");
    let mut bad = Vec::new();
    for (i, c) in ATTACKS.iter().enumerate() {
        let plan = steps(c.steps);
        let t = token(&e.admin, c.system, c.target, &plan, i);
        let v = check_gaia(&e.gate, c.system, c.target, &plan, &t, NOW);
        match &v {
            Verdict::Refused(r) if accepted(c.cat, r) => {}
            _ => bad.push(describe(c, &v)),
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} attack(s) not refused for the right reason:\n{}",
        bad.len(),
        ATTACKS.len(),
        bad.join("\n")
    );
    // Nothing was consumed: no attack burnt or used an approval.
    let ledger = std::fs::read_to_string(e.dir.join("ledger.jsonl")).unwrap();
    assert!(
        !ledger.contains("\"consumed\""),
        "an attack consumed a token"
    );
    assert_eq!(
        ledger.lines().filter(|l| l.contains("\"refused\"")).count(),
        ATTACKS.len(),
        "one refused audit line per attack"
    );
}

#[test]
fn every_attack_is_refused_without_a_token_too() {
    // Admin-safety runs before the token: no attack may even dry-run.
    let e = env("notoken");
    let mut bad = Vec::new();
    for c in ATTACKS {
        let v = check_gaia(&e.gate, c.system, c.target, &steps(c.steps), "", NOW);
        if !matches!(&v, Verdict::Refused(r) if accepted(c.cat, r)) {
            bad.push(describe(c, &v));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn ledger_never_holds_a_token_string() {
    let e = env("leak");
    let mut tokens = Vec::new();
    for (i, c) in ATTACKS.iter().enumerate().take(40) {
        let plan = steps(c.steps);
        let t = token(&e.admin, c.system, c.target, &plan, i);
        let _ = check_gaia(&e.gate, c.system, c.target, &plan, &t, NOW);
        tokens.push(t);
    }
    let ledger = std::fs::read_to_string(e.dir.join("ledger.jsonl")).unwrap();
    for t in &tokens {
        let sig = t.rsplit('.').next().unwrap();
        assert!(!ledger.contains(sig), "token signature in ledger");
    }
}

#[test]
fn legit_plans_are_approved_once_and_dry_run_without_a_token() {
    let e = env("legit");
    for (i, (target, system, raw)) in LEGIT.iter().enumerate() {
        let plan = steps(raw);
        assert_eq!(
            check_gaia(&e.gate, system, target, &plan, "", NOW),
            Verdict::DryRun,
            "{target} {system}: {raw:?}"
        );
        let t = token(&e.admin, system, target, &plan, 10_000 + i);
        let v = check_gaia(&e.gate, system, target, &plan, &t, NOW);
        assert!(
            matches!(v, Verdict::Approved(_)),
            "{target} {system}: {raw:?} -> {v:?}"
        );
        // Single use: the same token again is a replay.
        let again = check_gaia(&e.gate, system, target, &plan, &t, NOW);
        assert!(
            matches!(&again, Verdict::Refused(r) if r.code() == "replayed"),
            "{again:?}"
        );
    }
    let ledger = std::fs::read_to_string(e.dir.join("ledger.jsonl")).unwrap();
    assert_eq!(
        ledger
            .lines()
            .filter(|l| l.contains("\"consumed\""))
            .count(),
        LEGIT.len()
    );
}

#[test]
fn disable_file_refuses_even_the_legit_set() {
    let e = env("disabled");
    std::fs::write(e.dir.join("DISABLED"), b"off\n").unwrap();
    for (i, (target, system, raw)) in LEGIT.iter().enumerate() {
        let plan = steps(raw);
        let t = token(&e.admin, system, target, &plan, 20_000 + i);
        let v = check_gaia(&e.gate, system, target, &plan, &t, NOW);
        assert!(matches!(&v, Verdict::Refused(Refusal::Disabled)), "{v:?}");
    }
}

#[test]
fn plans_are_never_executed() {
    // Dynamic plans aimed at a canary dir; after the gate has refused them the
    // canary must be untouched (check_gaia only decides, it runs nothing).
    let e = env("canary");
    let canary = e.dir.join("canary");
    std::fs::create_dir_all(&canary).unwrap();
    let file = canary.join("keep.db");
    std::fs::write(&file, b"precious\n").unwrap();
    let p = canary.to_str().unwrap();
    let plans = [
        format!("apply rm -rf {p}"),
        format!("apply truncate -s 0 {p}/keep.db"),
        format!("apply mv {p} {p}-gone"),
        format!("apply shred -u {p}/keep.db"),
        format!("apply chmod 000 {p}"),
        format!("apply find {p} -delete"),
    ];
    for (i, plan) in plans.iter().enumerate() {
        let plan = vec![plan.clone()];
        let t = token(&e.admin, "heal-apply", "galion-tuner", &plan, 30_000 + i);
        let v = check_gaia(&e.gate, "heal-apply", "galion-tuner", &plan, &t, NOW);
        assert!(matches!(v, Verdict::Refused(_)), "{plan:?}: {v:?}");
    }
    assert_eq!(std::fs::read(&file).unwrap(), b"precious\n");
    assert_eq!(std::fs::read_dir(&canary).unwrap().count(), 1);
}
