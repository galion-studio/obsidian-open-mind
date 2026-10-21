// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Finding 4 of cortex-s2-d: a ledger that silently stops taking writes.
//!
//! Before 2026-10-21 `Approver::approve` dropped the result of every refusal
//! and dry-run audit write (`let _ = ledger.note(..)`), so a ledger on a full
//! disk, a read-only mount or with lost write permission recorded nothing and
//! said nothing: the cortex `hades` sensor saw silence and called it healthy.
//! These tests pin that every such decision now reports [`Audit::Failed`],
//! that the verdict itself is unchanged (fail closed, nothing extra granted),
//! and that [`Approver::ledger_health`] sees the state without writing.
//! Throwaway keys only (fixed test seed).

use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use hades_gate::adapter::check_gaia_audited;
use hades_gate::ledger::LedgerError;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Audit, Refusal, Request, Verdict};

const NOW: u64 = 1_792_000_000;
const PLAN: &[u8] = b"restart galion-tuner\nverify /health";

struct Env {
    dir: PathBuf,
    admin: SigningKey,
    gate: Approver,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = set_mode(&self.dir.join("ledger.jsonl"), 0o600);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn env(name: &str) -> Env {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "hades-ledger-fail-{name}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
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

fn set_mode(p: &std::path::Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
}

/// Remove write permission; `false` when it has no effect (running as root).
fn make_read_only(e: &Env) -> bool {
    let p = e.dir.join("ledger.jsonl");
    set_mode(&p, 0o400).unwrap();
    if std::fs::OpenOptions::new().append(true).open(&p).is_ok() {
        eprintln!("skipped: chmod 0400 does not stop this user (root?)");
        return false;
    }
    true
}

fn lines(e: &Env) -> usize {
    std::fs::read_to_string(e.dir.join("ledger.jsonl"))
        .unwrap()
        .lines()
        .count()
}

#[test]
fn healthy_ledger_reports_written() {
    let e = env("ok");
    let d = e.gate.approve_audited(req(""), NOW);
    assert_eq!(d.verdict, Verdict::DryRun);
    assert_eq!(d.audit, Audit::Written);
    let forged = claims(1).sign(&SigningKey::from_bytes(&[7; 32]));
    let d = e.gate.approve_audited(req(&forged), NOW);
    assert!(matches!(d.verdict, Verdict::Refused(_)));
    assert!(d.audit.ok());
    let t = claims(2).sign(&e.admin);
    let d = e.gate.approve_audited(req(&t), NOW);
    assert!(matches!(d.verdict, Verdict::Approved(_)));
    assert!(d.audit.ok());
    // A replay is an ordinary refusal: the ledger works, it did its job.
    let d = e.gate.approve_audited(req(&t), NOW + 1);
    assert_eq!(
        d.verdict,
        Verdict::Refused(Refusal::Ledger(LedgerError::Replayed))
    );
    assert!(d.audit.ok());
    assert_eq!(lines(&e), 4);
    let h = e.gate.ledger_health().unwrap();
    assert_eq!(h.consumed, 1);
    assert!(h.bytes > 0);
    assert_eq!(lines(&e), 4, "health probe wrote nothing");
}

#[test]
fn read_only_ledger_is_loud_not_silent() {
    let e = env("ro");
    assert!(e.gate.approve_audited(req(""), NOW).audit.ok());
    if !make_read_only(&e) {
        return;
    }
    // Dry-run: verdict unchanged, audit failed with the sensor's wording.
    let d = e.gate.approve_audited(req(""), NOW);
    assert_eq!(d.verdict, Verdict::DryRun);
    let Audit::Failed(err) = &d.audit else {
        panic!("{d:?}")
    };
    assert!(err.reason().starts_with("ledger unavailable"), "{err}");
    // A refusal stays the same refusal, and is now visible as unaudited.
    let forged = claims(3).sign(&SigningKey::from_bytes(&[7; 32]));
    let d = e.gate.approve_audited(req(&forged), NOW);
    assert!(
        matches!(d.verdict, Verdict::Refused(Refusal::Token(_))),
        "{d:?}"
    );
    assert!(!d.audit.ok());
    // A valid token cannot pass: consume fails closed, audit failed.
    let t = claims(4).sign(&e.admin);
    let d = e.gate.approve_audited(req(&t), NOW);
    assert!(
        matches!(
            d.verdict,
            Verdict::Refused(Refusal::Ledger(LedgerError::Io(_)))
        ),
        "{d:?}"
    );
    assert!(!d.audit.ok());
    // The legacy API still returns the same verdicts.
    assert_eq!(e.gate.approve(req(""), NOW), Verdict::DryRun);
    // Health sees it without writing.
    let h = e.gate.ledger_health().unwrap_err();
    assert!(h.reason().starts_with("ledger unavailable"), "{h}");
    // Nothing got in while read-only; recovery is seen.
    set_mode(&e.dir.join("ledger.jsonl"), 0o600).unwrap();
    assert_eq!(lines(&e), 1);
    assert!(e.gate.ledger_health().is_ok());
    let d = e.gate.approve_audited(req(&t), NOW);
    assert!(matches!(d.verdict, Verdict::Approved(_)), "{d:?}");
}

#[test]
fn full_disk_is_seen_by_the_write() {
    // /dev/full accepts open(O_APPEND) and fails every write with ENOSPC,
    // the same as a ledger on a full file system.
    if !std::path::Path::new("/dev/full").exists() {
        eprintln!("skipped: no /dev/full");
        return;
    }
    let e = env("full");
    let p = e.dir.join("ledger.jsonl");
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink("/dev/full", &p).unwrap();
    let d = e.gate.approve_audited(req(""), NOW);
    assert_eq!(d.verdict, Verdict::DryRun);
    let Audit::Failed(LedgerError::Io(kind)) = &d.audit else {
        panic!("{d:?}")
    };
    assert!(!kind.is_empty());
    // A token cannot pass and the scan does not read /dev/full forever.
    let t = claims(5).sign(&e.admin);
    let d = e.gate.approve_audited(req(&t), NOW);
    assert_eq!(
        d.verdict,
        Verdict::Refused(Refusal::Ledger(LedgerError::Io(
            "not a regular file".into()
        )))
    );
    assert!(!d.audit.ok());
    assert!(e.gate.ledger_health().is_err());
}

#[test]
fn corrupt_ledger_marks_audit_failed_even_if_the_note_lands() {
    let e = env("corrupt");
    std::fs::write(e.dir.join("ledger.jsonl"), b"not json\n").unwrap();
    let t = claims(6).sign(&e.admin);
    let d = e.gate.approve_audited(req(&t), NOW);
    assert_eq!(
        d.verdict,
        Verdict::Refused(Refusal::Ledger(LedgerError::Corrupt(1)))
    );
    // The refusal line itself was appended (the file is writable), but the
    // gate is failing closed, so the decision is still reported as failed.
    assert_eq!(lines(&e), 2);
    assert_eq!(d.audit, Audit::Failed(LedgerError::Corrupt(1)));
    assert_eq!(e.gate.ledger_health(), Err(LedgerError::Corrupt(1)));
}

#[test]
fn gaia_adapter_reports_audit_too() {
    let e = env("gaia");
    let steps = vec!["stop everything".to_owned()];
    let d = check_gaia_audited(&e.gate, "kill-switch", "galion-one", &steps, "", NOW);
    assert!(matches!(
        d.verdict,
        Verdict::Refused(Refusal::ActionNotApprovable(_))
    ));
    assert!(d.audit.ok());
    assert_eq!(lines(&e), 1, "unapprovable system is now audited");
    if !make_read_only(&e) {
        return;
    }
    let d = check_gaia_audited(&e.gate, "kill-switch", "galion-one", &steps, "", NOW);
    assert!(!d.audit.ok());
    let d = check_gaia_audited(&e.gate, "rollback", "galion-one", &steps, "", NOW);
    assert!(!d.audit.ok());
}
