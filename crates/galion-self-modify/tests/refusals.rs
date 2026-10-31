// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Every path to a deploy that must be refused: forged, expired, replayed,
//! wrong artifact, missing signature, wrong key (all four signatures).

mod common;
use common::*;
use galion_self_modify::deploy::{ledger_consume, verify_all};
use galion_self_modify::owner::{self, OwnerKey};
use galion_self_modify::records;
use galion_self_modify::util::write_atomic;
use serde_json::{json, Value};

fn refused(f: &Fix, want: &str) {
    match verify_all(&f.entry, &f.keys(), &f.ledger(), NOW) {
        Ok(v) => panic!("expected refusal containing {want:?}, got OK {v:?}"),
        Err(e) => assert!(e.contains(want), "refusal {e:?} does not mention {want:?}"),
    }
}

fn tamper_json(p: &std::path::Path, f: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    f(&mut v);
    write_atomic(p, v.to_string().as_bytes(), 0o644).unwrap();
}

// ---------- happy paths (the baseline the refusals differ from) ----------

#[test]
fn all_four_signatures_ed25519_owner_verify() {
    let f = signed("ok-ed");
    f.approve_ed25519(NONCE);
    let v = verify_all(&f.entry, &f.keys(), &f.ledger(), NOW).unwrap();
    assert_eq!(v.nonce, NONCE);
    assert_eq!(v.base_commit, BASE);
    assert_eq!(v.owner_scheme, "ed25519");
}

#[test]
fn all_four_signatures_webauthn_owner_verify() {
    let f = signed("ok-wa");
    let pk = p256_key(9);
    let body = f.body(NONCE, NOW - 30, 1800);
    f.put_approval(&Assertion::new(&pk, &body).env());
    let mut k = f.keys();
    k.owner = OwnerKey::parse(&p256_owner_pub(&pk, b"cred-1")).unwrap();
    let v = verify_all(&f.entry, &k, &f.ledger(), NOW).unwrap();
    assert_eq!(v.owner_scheme, "webauthn-es256");
}

// ---------- missing signatures ----------

#[test]
fn missing_stack_signature_refused() {
    let f = signed("miss-stack");
    f.approve_ed25519(NONCE);
    std::fs::rename(
        f.e().stack_sig(),
        f.e().stack_sig().with_extension("json.off"),
    )
    .unwrap();
    refused(&f, "stack signature missing");
}

#[test]
fn missing_test_record_refused() {
    let f = signed("miss-test");
    f.approve_ed25519(NONCE);
    std::fs::rename(
        f.e().test_record(),
        f.e().test_record().with_extension("json.off"),
    )
    .unwrap();
    refused(&f, "test record missing");
}

#[test]
fn missing_dryrun_record_refused() {
    let f = signed("miss-dry");
    f.approve_ed25519(NONCE);
    std::fs::rename(
        f.e().dryrun_record(),
        f.e().dryrun_record().with_extension("json.off"),
    )
    .unwrap();
    refused(&f, "dry-run record missing");
}

#[test]
fn missing_owner_approval_refused() {
    let f = signed("miss-owner");
    refused(&f, "no owner approval");
}

#[test]
fn owner_envelope_without_signature_refused() {
    let f = signed("nosig-owner");
    f.approve_ed25519(NONCE);
    tamper_json(&f.e().owner_approval(), |v| {
        v.as_object_mut().unwrap().remove("sig");
    });
    refused(&f, "sig missing");
}

// ---------- forged / tampered ----------

#[test]
fn forged_stack_signature_refused_by_signer_and_deploy() {
    let f = signed("forge-stack");
    f.approve_ed25519(NONCE);
    // Re-sign the stack body with a different key (an attacker without the stack key).
    tamper_json(&f.e().stack_sig(), |v| {
        let sig = hermes_rs::self_modify::sign_env(
            records::STACK_SIG_V,
            &serde_json::from_str(v["body_json"].as_str().unwrap()).unwrap(),
            "body_json",
            &key(66),
        );
        *v = sig;
    });
    refused(&f, "stack signature");
    // And the signer will not sign a test record on top of it.
    let g = common::tmpdir("forge-stack-2");
    let entry = g.join("e");
    proposal(&entry, "sm-x");
    records::stack_sign(&entry, &key(66), NOW).unwrap();
    test_result(&entry, "sm-x", &[("build", 0)]);
    let e = records::sign_test(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap_err();
    assert!(e.contains("stack signature"), "{e}");
}

#[test]
fn proposal_changed_after_stack_signature_refused() {
    let f = signed("tamper-prop");
    f.approve_ed25519(NONCE);
    let p = f.e().proposal();
    let t = std::fs::read_to_string(&p)
        .unwrap()
        .replace("\"proposed_by\":\"hermes\"", "\"proposed_by\":\"owner\"");
    std::fs::write(&p, t).unwrap();
    refused(&f, "different proposal");
}

#[test]
fn patch_changed_after_signing_refused() {
    let f = signed("tamper-patch");
    f.approve_ed25519(NONCE);
    std::fs::write(f.e().patch(), b"--- evil\n").unwrap();
    refused(&f, "change.patch does not match");
}

#[test]
fn test_record_body_tampered_refused() {
    let f = signed("tamper-trec");
    f.approve_ed25519(NONCE);
    tamper_json(&f.e().test_record(), |v| {
        let t = v["record_json"]
            .as_str()
            .unwrap()
            .replace("\"verdict\":\"pass\"", "\"verdict\":\"pass\" ");
        v["record_json"] = json!(t);
    });
    refused(&f, "bad signature");
}

#[test]
fn owner_body_tampered_after_signing_refused() {
    let f = signed("tamper-owner");
    f.approve_ed25519(NONCE);
    tamper_json(&f.e().owner_approval(), |v| {
        let t = v["approval_json"]
            .as_str()
            .unwrap()
            .replace(NONCE, "ffffffffffffffffffffffffffffffff");
        v["approval_json"] = json!(t);
    });
    refused(&f, "bad signature");
}

#[test]
fn test_log_changed_after_signing_refused() {
    let f = signed("tamper-log");
    f.approve_ed25519(NONCE);
    std::fs::write(f.e().test_dir().join("logs/test.log"), b"rewritten\n").unwrap();
    refused(&f, "changed after signing");
}

#[test]
fn signer_refuses_symlinked_log() {
    let root = tmpdir("symlog");
    let entry = root.join("e");
    proposal(&entry, "sm-sym");
    records::stack_sign(&entry, &key(1), NOW).unwrap();
    test_result(&entry, "sm-sym", &[("build", 0)]);
    let log = entry.join("test/logs/build.log");
    std::fs::rename(&log, log.with_extension("log.off")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", &log).unwrap();
    let e = records::sign_test(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap_err();
    assert!(e.contains("leaves") || e.contains("symlink"), "{e}");
}

#[test]
fn signer_refuses_path_escape_in_result() {
    let root = tmpdir("escape");
    let entry = root.join("e");
    proposal(&entry, "sm-esc");
    records::stack_sign(&entry, &key(1), NOW).unwrap();
    test_result(&entry, "sm-esc", &[("build", 0)]);
    tamper_json(&entry.join("test/result.json"), |v| {
        v["stages"][0]["log"] = json!("../../proposal.json")
    });
    let e = records::sign_test(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap_err();
    assert!(e.contains("unsafe path"), "{e}");
}

// ---------- wrong artifact ----------

#[test]
fn artifact_swapped_after_test_refused() {
    let f = signed("swap-art");
    f.approve_ed25519(NONCE);
    std::fs::write(f.e().artifact(), b"\x7fELF a different binary").unwrap();
    refused(&f, "fingerprint mismatch");
}

#[test]
fn owner_approved_other_artifact_refused() {
    let f = signed("other-art");
    let (t, ts, _, ds) = f.records();
    let body = owner::approval_body(&owner::ApprovalFacts {
        id: t["id"].as_str().unwrap(),
        artifact_sha256: &"ab".repeat(32),
        base_commit: BASE,
        test_record_sha256: &ts,
        dryrun_record_sha256: &ds,
        nonce: NONCE,
        issued_at: NOW - 10,
        ttl_s: 600,
    })
    .unwrap();
    f.put_approval(&owner::sign_ed25519(&body, &f.owner));
    refused(&f, "different artifact");
}

#[test]
fn owner_approved_other_base_commit_refused() {
    let f = signed("other-base");
    let body = f.body(NONCE, NOW - 10, 600).replace(BASE, &"1".repeat(40));
    f.put_approval(&owner::sign_ed25519(&body, &f.owner));
    refused(&f, "different base commit");
}

#[test]
fn owner_approved_other_records_refused() {
    let f = signed("other-recs");
    let (_, ts, _, _) = f.records();
    let body = f.body(NONCE, NOW - 10, 600).replace(&ts, &"c".repeat(64));
    f.put_approval(&owner::sign_ed25519(&body, &f.owner));
    refused(&f, "different test/dry-run records");
}

#[test]
fn dryrun_of_a_different_binary_is_not_signed() {
    let root = tmpdir("dry-other");
    let entry = root.join("e");
    proposal(&entry, "sm-d");
    records::stack_sign(&entry, &key(1), NOW).unwrap();
    test_result(&entry, "sm-d", &[("build", 0)]);
    records::sign_test(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap();
    dryrun_result(&entry, "sm-d", &"de".repeat(32), 19_070, 0, true);
    let e = records::sign_dryrun(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap_err();
    assert!(e.contains("different binary"), "{e}");
}

// ---------- failing evidence ----------

#[test]
fn failing_test_stage_signs_fail_and_blocks_everything() {
    let root = tmpdir("fail-test");
    let entry = root.join("e");
    proposal(&entry, "sm-f");
    records::stack_sign(&entry, &key(1), NOW).unwrap();
    test_result(&entry, "sm-f", &[("build", 0), ("test", 101)]);
    let t = records::sign_test(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap();
    assert_eq!(t["verdict"], "fail");
    let e = records::sign_dryrun(&entry, &key(1).verifying_key(), &key(2), NOW).unwrap_err();
    assert!(e.contains("not a pass"), "{e}");
}

#[test]
fn dryrun_with_real_model_calls_or_bad_port_fails() {
    for (tag, port, calls, ok) in [
        ("calls", 19_070, 1, true),
        ("port", 8642, 0, true),
        ("check", 19_071, 0, false),
    ] {
        let f = signed(&format!("dry-{tag}"));
        // Replace the passing dry-run with a failing one (new entry state).
        let e = f.e();
        std::fs::rename(
            e.dryrun_record(),
            e.dryrun_record().with_extension("json.off"),
        )
        .unwrap();
        let (t, ..) = f.records_test_only();
        dryrun_result(
            &f.entry,
            t["id"].as_str().unwrap(),
            t["artifact"]["sha256"].as_str().unwrap(),
            port,
            calls,
            ok,
        );
        let d = records::sign_dryrun(&f.entry, &f.stack.verifying_key(), &f.sandbox, NOW).unwrap();
        assert_eq!(d["verdict"], "fail", "{tag}: {d}");
        let (_, ts) = f.records_test_only();
        let (_, ds) =
            records::verify_dryrun(&f.entry, &f.sandbox.verifying_key(), &t, &ts).unwrap();
        let body = owner::approval_body(&owner::ApprovalFacts {
            id: t["id"].as_str().unwrap(),
            artifact_sha256: t["artifact"]["sha256"].as_str().unwrap(),
            base_commit: BASE,
            test_record_sha256: &ts,
            dryrun_record_sha256: &ds,
            nonce: NONCE,
            issued_at: NOW - 10,
            ttl_s: 600,
        })
        .unwrap();
        f.put_approval(&owner::sign_ed25519(&body, &f.owner));
        refused(&f, "dry run is not a pass");
    }
}

trait TestOnly {
    fn records_test_only(&self) -> (Value, String);
}
impl TestOnly for Fix {
    fn records_test_only(&self) -> (Value, String) {
        records::verify_test(
            &self.entry,
            &self.stack.verifying_key(),
            &self.sandbox.verifying_key(),
        )
        .unwrap()
    }
}

// ---------- wrong keys ----------

#[test]
fn test_record_signed_by_old_sandbox_key_refused() {
    let f = signed("old-sandbox");
    f.approve_ed25519(NONCE);
    let mut k = f.keys();
    k.sandbox = key(77).verifying_key(); // root trusts only the NEW sandbox pub
    match verify_all(&f.entry, &k, &f.ledger(), NOW) {
        Err(e) => assert!(
            e.contains("test record") && e.contains("bad signature"),
            "{e}"
        ),
        Ok(_) => panic!("old sandbox key accepted"),
    }
}

#[test]
fn owner_signed_with_wrong_key_refused() {
    let f = signed("wrong-owner");
    let b = f.body(NONCE, NOW - 10, 600);
    f.put_approval(&owner::sign_ed25519(&b, &key(99)));
    refused(&f, "bad signature");
}

#[test]
fn sandbox_key_cannot_act_as_owner() {
    let f = signed("sandbox-owner");
    let b = f.body(NONCE, NOW - 10, 600);
    f.put_approval(&owner::sign_ed25519(&b, &f.sandbox));
    let mut k = f.keys();
    k.owner = OwnerKey::Ed25519(f.sandbox.verifying_key());
    let e = verify_all(&f.entry, &k, &f.ledger(), NOW).unwrap_err();
    assert!(e.contains("must all be different"), "{e}");
}

#[test]
fn stack_key_cannot_sign_test_record() {
    let root = tmpdir("stack-as-sandbox");
    let entry = root.join("e");
    proposal(&entry, "sm-s");
    records::stack_sign(&entry, &key(1), NOW).unwrap();
    test_result(&entry, "sm-s", &[("build", 0)]);
    let e = records::sign_test(&entry, &key(1).verifying_key(), &key(1), NOW).unwrap_err();
    assert!(e.contains("must not be the stack key"), "{e}");
}

// ---------- time / replay ----------

#[test]
fn expired_approval_refused() {
    let f = signed("expired");
    let b = f.body(NONCE, NOW - 1801, 1800);
    f.put_approval(&owner::sign_ed25519(&b, &f.owner));
    refused(&f, "expired");
}

#[test]
fn window_longer_than_30_minutes_refused() {
    let f = signed("long");
    assert!(owner::approval_body(&owner::ApprovalFacts {
        id: "x",
        artifact_sha256: "a",
        base_commit: BASE,
        test_record_sha256: "t",
        dryrun_record_sha256: "d",
        nonce: NONCE,
        issued_at: NOW,
        ttl_s: 1801,
    })
    .is_err());
    // A hand-made body with a 2 h window, validly signed, is still refused.
    let b = f.body(NONCE, NOW - 10, 1800).replace(
        &format!("\"expires\":{}", NOW - 10 + 1800),
        &format!("\"expires\":{}", NOW - 10 + 7200),
    );
    f.put_approval(&owner::sign_ed25519(&b, &f.owner));
    refused(&f, "longer than 30 minutes");
}

#[test]
fn approval_issued_in_future_refused() {
    let f = signed("future");
    let b = f.body(NONCE, NOW + 600, 600);
    f.put_approval(&owner::sign_ed25519(&b, &f.owner));
    refused(&f, "issued in the future");
}

#[test]
fn replayed_nonce_refused() {
    let f = signed("replay");
    f.approve_ed25519(NONCE);
    verify_all(&f.entry, &f.keys(), &f.ledger(), NOW).unwrap();
    ledger_consume(&f.ledger(), NONCE, "earlier", NOW - 5).unwrap();
    refused(&f, "already used");
    let e = ledger_consume(&f.ledger(), NONCE, "again", NOW).unwrap_err();
    assert!(e.contains("replay"), "{e}");
}

#[test]
fn malformed_nonce_refused() {
    let f = signed("bad-nonce");
    let b = f.body(NONCE, NOW - 10, 600).replace(NONCE, "short");
    f.put_approval(&owner::sign_ed25519(&b, &f.owner));
    refused(&f, "nonce malformed");
}

// ---------- WebAuthn specifics ----------

fn wa(tag: &str, edit: impl FnOnce(&mut Assertion), want: &str) {
    let f = signed(tag);
    let pk = p256_key(9);
    let body = f.body(NONCE, NOW - 30, 1800);
    let other = f.body("fedcba9876543210fedcba9876543210", NOW - 30, 1800);
    let mut a = Assertion::new(&pk, &body);
    let wrong = p256_key(10);
    if tag.ends_with("wrongkey") {
        a.sk = &wrong;
    }
    if tag.ends_with("otherchal") {
        a.challenge_body = Some(&other);
    }
    edit(&mut a);
    f.put_approval(&a.env());
    let mut k = f.keys();
    k.owner = OwnerKey::parse(&p256_owner_pub(&pk, b"cred-1")).unwrap();
    match verify_all(&f.entry, &k, &f.ledger(), NOW) {
        Ok(_) => panic!("{tag}: accepted"),
        Err(e) => assert!(e.contains(want), "{tag}: {e:?} lacks {want:?}"),
    }
}

#[test]
fn webauthn_wrong_origin_refused() {
    wa(
        "wa-origin",
        |a| a.origin = "https://evil.example",
        "not allowed",
    );
}
#[test]
fn webauthn_wrong_rp_refused() {
    wa("wa-rp", |a| a.rp = "evil.example", "rpIdHash");
}
#[test]
fn webauthn_without_user_verification_refused() {
    wa("wa-uv", |a| a.flags = 0x01, "user verification");
}
#[test]
fn webauthn_create_ceremony_refused() {
    wa("wa-type", |a| a.typ = "webauthn.create", "webauthn.get");
}
#[test]
fn webauthn_other_credential_refused() {
    wa("wa-cred", |a| a.cred = b"cred-2", "credential id");
}
#[test]
fn webauthn_wrongkey_refused() {
    wa("wa-wrongkey", |_| {}, "bad passkey signature");
}
#[test]
fn webauthn_otherchal_refused() {
    wa("wa-otherchal", |_| {}, "challenge does not match");
}

#[test]
fn webauthn_scheme_mismatch_refused() {
    // An Ed25519 envelope presented while owner.pub is a passkey.
    let f = signed("wa-scheme");
    f.approve_ed25519(NONCE);
    let mut k = f.keys();
    k.owner = OwnerKey::parse(&p256_owner_pub(&p256_key(9), b"cred-1")).unwrap();
    let e = verify_all(&f.entry, &k, &f.ledger(), NOW).unwrap_err();
    assert!(e.contains("does not match owner.pub"), "{e}");
}

#[test]
fn webauthn_high_s_signature_accepted_and_garbage_refused() {
    use p256::elliptic_curve::PrimeField;
    let f = signed("wa-highs");
    let pk = p256_key(9);
    let body = f.body(NONCE, NOW - 30, 1800);
    let mut env = Assertion::new(&pk, &body).env();
    let der = galion_self_modify::util::unb64url(env["signature"].as_str().unwrap()).unwrap();
    let sig = p256::ecdsa::Signature::from_der(&der).unwrap();
    let (r, s) = sig.split_scalars();
    let neg: p256::Scalar = -*s.as_ref();
    let hs = p256::ecdsa::Signature::from_scalars(r.to_repr(), neg.to_repr()).unwrap();
    // Exactly one of (s, n-s) is high; check the other form still verifies.
    env["signature"] = json!(galion_self_modify::util::b64url(hs.to_der().as_bytes()));
    f.put_approval(&env);
    let mut k = f.keys();
    k.owner = OwnerKey::parse(&p256_owner_pub(&pk, b"cred-1")).unwrap();
    verify_all(&f.entry, &k, &f.ledger(), NOW).unwrap();
    env["signature"] = json!("AAAA");
    f.put_approval(&env);
    let e = verify_all(&f.entry, &k, &f.ledger(), NOW).unwrap_err();
    assert!(e.contains("not DER"), "{e}");
}

// ---------- owner.pub parsing ----------

#[test]
fn owner_pub_formats_and_fingerprint() {
    let ed = key(3).verifying_key();
    let legacy = OwnerKey::parse(&hermes_rs::self_modify::hex(ed.as_bytes())).unwrap();
    let j = OwnerKey::parse(&owner::ed25519_owner_pub(&ed)).unwrap();
    assert_eq!(legacy.fingerprint(), j.fingerprint());
    assert_eq!(
        j.fingerprint(),
        hermes_rs::self_modify::sha256_hex(ed.as_bytes())
    );
    let pk = p256_key(9);
    let w = OwnerKey::parse(&p256_owner_pub(&pk, b"c")).unwrap();
    let sec1 = pk.verifying_key().to_encoded_point(false);
    assert_eq!(
        w.fingerprint(),
        hermes_rs::self_modify::sha256_hex(sec1.as_bytes())
    );
    // SPKI form (what the browser's getPublicKey() returns) gives the same key.
    let spki = format!(
        "{}{}",
        owner::P256_SPKI_PREFIX,
        hermes_rs::self_modify::hex(sec1.as_bytes())
    );
    let s = OwnerKey::parse(
        &json!({"version": owner::OWNER_KEY_V, "scheme": "webauthn-es256",
        "rp_id": RP, "origins": [ORIGIN], "public_key_spki_hex": spki})
        .to_string(),
    )
    .unwrap();
    assert_eq!(s.fingerprint(), w.fingerprint());
    for bad in [
        json!({"version": owner::OWNER_KEY_V, "scheme": "webauthn-es256", "rp_id": RP,
            "origins": ["http://galion.app"], "public_key_sec1_hex": hermes_rs::self_modify::hex(sec1.as_bytes())}),
        json!({"version": owner::OWNER_KEY_V, "scheme": "webauthn-es256", "rp_id": "https://galion.app",
            "origins": [ORIGIN], "public_key_sec1_hex": hermes_rs::self_modify::hex(sec1.as_bytes())}),
        json!({"version": owner::OWNER_KEY_V, "scheme": "webauthn-es256", "rp_id": RP,
            "origins": [ORIGIN], "public_key_sec1_hex": "04".to_string() + &"00".repeat(64)}),
        json!({"version": owner::OWNER_KEY_V, "scheme": "rsa"}),
        json!({"version": "other", "scheme": "ed25519", "public_key_hex": hermes_rs::self_modify::hex(ed.as_bytes())}),
    ] {
        assert!(OwnerKey::parse(&bad.to_string()).is_err(), "accepted {bad}");
    }
}
