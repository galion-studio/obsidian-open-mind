// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Owner key formats, signatures and the root install step, with software
//! owner devices (a fake ES256 passkey and an Ed25519 key file).

use galion_owner_key::testkit::{envelope, SoftKeyFile, SoftPasskey};
use galion_owner_key::*;
use serde_json::{json, Value};

const ORIGIN: &str = "https://one.galion.app";
const NOW: i64 = 1_793_000_000;

fn body(nonce: &str, issued: i64, expires: i64) -> String {
    approval_body(
        "sm-20261031090000-abcdef",
        &"a".repeat(64),
        &"b".repeat(40),
        &"c".repeat(64),
        &"d".repeat(64),
        nonce,
        issued,
        expires,
    )
}

fn passkey() -> (SoftPasskey, OwnerKey) {
    let p = SoftPasskey::new(3, "galion.app", ORIGIN);
    let k = p.owner_key(&["https://galion.app", ORIGIN]);
    (p, k)
}

#[test]
fn owner_pub_round_trip_and_fingerprint() {
    let (p, k) = passkey();
    let text = k.file_text();
    assert_eq!(OwnerKey::parse(&text).unwrap(), k);
    assert!(text.starts_with(
        r#"{"version":"galion-sm/owner-key/v1","scheme":"webauthn-es256","rp_id":"galion.app""#
    ));
    assert_eq!(k.fingerprint(), sha256_hex(&p.sec1()));
    assert_eq!(k.key_id(), k.fingerprint()[..16]);
    // SPKI form accepted, same key.
    let spki = json!({"version": OWNER_KEY_V, "scheme": "webauthn-es256", "rp_id": "galion.app",
        "origins": [ORIGIN], "public_key_spki_hex": hex(&p.spki())})
    .to_string();
    assert_eq!(
        OwnerKey::parse(&spki).unwrap().fingerprint(),
        k.fingerprint()
    );
    let f = SoftKeyFile::new(9);
    let e = f.owner_key();
    assert_eq!(OwnerKey::parse(&e.file_text()).unwrap(), e);
    // Legacy: 64 hex = ed25519.
    assert_eq!(OwnerKey::parse(&hex(e.raw())).unwrap(), e);
    assert_eq!(e.key_id(), hex(e.raw())[..16]);
    assert_eq!(group4("abcdef0123"), "abcd ef01 23");
    assert!(OwnerKey::parse("{}").is_err());
    assert!(
        OwnerKey::parse(&"A".repeat(64)).is_err(),
        "uppercase hex refused"
    );
}

#[test]
fn passkey_approval_verifies_and_every_tamper_fails() {
    let (mut p, k) = passkey();
    let b = body(&"1".repeat(32), NOW, NOW + 1800);
    let env = envelope(p.assert(APPROVAL_V, &b, &k.key_id()), "approval_json", &b);
    let got = verify_owner_approval(&k, &env, NOW + 60).expect("verifies");
    assert_eq!(got["nonce"], "1".repeat(32));

    // Body changed by one character (different artifact).
    let mut t = env.clone();
    t["approval_json"] = json!(b.replace(&"a".repeat(64), &"e".repeat(64)));
    assert!(verify_owner_approval(&k, &t, NOW)
        .unwrap_err()
        .contains("challenge"));
    // Signature bit flipped.
    let mut t = env.clone();
    let mut sig = unb64url(t["signature"].as_str().unwrap()).unwrap();
    let n = sig.len() - 2;
    sig[n] ^= 1;
    t["signature"] = json!(b64url(&sig));
    assert!(verify_owner_approval(&k, &t, NOW).is_err());
    // Expired.
    assert_eq!(
        verify_owner_approval(&k, &env, NOW + 1801).unwrap_err(),
        "expired"
    );
    // Wrong site / origin.
    let mut evil = SoftPasskey::new(3, "galion.app", "https://evil.example");
    let e = envelope(
        evil.assert(APPROVAL_V, &b, &k.key_id()),
        "approval_json",
        &b,
    );
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("origin"));
    let mut other_rp = SoftPasskey::new(3, "evil.example", ORIGIN);
    let e = envelope(
        other_rp.assert(APPROVAL_V, &b, &k.key_id()),
        "approval_json",
        &b,
    );
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("rpIdHash"));
    // No user verification (UP only).
    let e = envelope(
        p.assert_raw(&webauthn_challenge(APPROVAL_V, &b), 0x01, &k.key_id()),
        "approval_json",
        &b,
    );
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("UV"));
    // Another passkey.
    let mut other = SoftPasskey::new(4, "galion.app", ORIGIN);
    let mut e = envelope(
        other.assert(APPROVAL_V, &b, &k.key_id()),
        "approval_json",
        &b,
    );
    e["credential_id"] = json!(p.credential_id.clone());
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("bad passkey signature"));
    // Ed25519 envelope against a passkey owner key.
    let f = SoftKeyFile::new(1);
    let e = envelope(f.sign(APPROVAL_V, &b), "approval_json", &b);
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("scheme"));
}

#[test]
fn passkey_high_s_signature_is_accepted() {
    // WebAuthn authenticators do not normalise S; FORMATS says accept high-S.
    let (mut p, k) = passkey();
    let b = body(&"2".repeat(32), NOW, NOW + 600);
    let mut env = envelope(p.assert(APPROVAL_V, &b, &k.key_id()), "approval_json", &b);
    let der = unb64url(env["signature"].as_str().unwrap()).unwrap();
    let sig = p256::ecdsa::Signature::from_der(&der).unwrap();
    let (r, s) = sig.split_scalars();
    let high = p256::ecdsa::Signature::from_scalars(r, -*s).unwrap();
    let flipped = if sig.normalize_s().is_some() {
        sig
    } else {
        high
    };
    env["signature"] = json!(b64url(flipped.to_der().as_bytes()));
    assert!(
        flipped.normalize_s().is_some(),
        "test uses a high-S signature"
    );
    verify_owner_approval(&k, &env, NOW).expect("high-S accepted");
}

#[test]
fn keyfile_approval_and_window_rules() {
    let f = SoftKeyFile::new(5);
    let k = f.owner_key();
    let b = body(&"3".repeat(32), NOW, NOW + 1800);
    let env = envelope(f.sign(APPROVAL_V, &b), "approval_json", &b);
    verify_owner_approval(&k, &env, NOW).expect("verifies");
    // Message is domain + "\n" + body, so another domain must not verify.
    let e = envelope(f.sign("galion-sm/test-record/v1", &b), "approval_json", &b);
    assert!(verify_owner_approval(&k, &e, NOW).is_err());
    for (issued, expires, why) in [
        (NOW, NOW + 1801, "30 minutes"),
        (NOW, NOW, "30 minutes"),
        (NOW + 1000, NOW + 1500, "future"),
    ] {
        let b = body(&"3".repeat(32), issued, expires);
        let e = envelope(f.sign(APPROVAL_V, &b), "approval_json", &b);
        assert!(
            verify_owner_approval(&k, &e, NOW)
                .unwrap_err()
                .contains(why),
            "{why}"
        );
    }
    let b = body("short", NOW, NOW + 60);
    let e = envelope(f.sign(APPROVAL_V, &b), "approval_json", &b);
    assert!(verify_owner_approval(&k, &e, NOW)
        .unwrap_err()
        .contains("nonce"));
}

fn activation_for(text: &str, fp: &str, signer: &mut dyn FnMut(&str, &str) -> Value) -> Value {
    let b = activate_body(text, fp, &"4".repeat(32), NOW, NOW + 1800);
    envelope(signer(ACTIVATE_V, &b), "body_json", &b)
}

#[test]
fn install_first_key_then_replace_needs_current_key() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("gok-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join("owner.pub");
    let opts = InstallOpts {
        chown_root: false,
        dry_run: false,
    };

    // 1. First key: a passkey.
    let (mut p, k) = passkey();
    let text = k.file_text();
    let kid = k.key_id();
    let act = activation_for(&text, &k.fingerprint(), &mut |d, b| p.assert(d, b, &kid));
    let req = install_request(&text, &act, None, NOW);
    let dry = install(
        &req,
        &dest,
        NOW + 10,
        &InstallOpts {
            chown_root: false,
            dry_run: true,
        },
    )
    .unwrap();
    assert!(matches!(
        dry,
        InstallOutcome::WouldInstall { replaces: None, .. }
    ));
    assert!(!dest.exists(), "dry run writes nothing");
    let out = install(&req, &dest, NOW + 10, &opts).unwrap();
    assert!(matches!(
        out,
        InstallOutcome::Installed { backup: None, .. }
    ));
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), text);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
        0o444
    );
    // Same request again: no-op.
    assert!(matches!(
        install(&req, &dest, NOW + 20, &opts).unwrap(),
        InstallOutcome::AlreadyInstalled { .. }
    ));

    // 2. Replacement without the current key's signature: refused.
    let f = SoftKeyFile::new(8);
    let nk = f.owner_key();
    let ntext = nk.file_text();
    let act2 = activation_for(&ntext, &nk.fingerprint(), &mut |d, b| f.sign(d, b));
    let req2 = install_request(&ntext, &act2, None, NOW);
    assert!(install(&req2, &dest, NOW + 30, &opts)
        .unwrap_err()
        .contains("current key"));
    // Signed by the NEW key instead of the current one: refused.
    let cb = change_body(
        &k.fingerprint(),
        &nk.fingerprint(),
        &ntext,
        &"5".repeat(32),
        NOW,
        NOW + 1800,
    );
    let fake = envelope(f.sign(CHANGE_V, &cb), "body_json", &cb);
    let req3 = install_request(&ntext, &act2, Some(&fake), NOW);
    assert!(install(&req3, &dest, NOW + 30, &opts).is_err());
    // Signed by the current passkey: installed, old key kept.
    let ch = envelope(p.assert(CHANGE_V, &cb, &kid), "body_json", &cb);
    let req4 = install_request(&ntext, &act2, Some(&ch), NOW);
    match install(&req4, &dest, NOW + 40, &opts).unwrap() {
        InstallOutcome::Installed {
            backup: Some(b), ..
        } => {
            assert_eq!(std::fs::read_to_string(b).unwrap(), text, "old key kept");
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), ntext);
    // Replaying the first request now (current key changed): refused.
    assert!(install(&req, &dest, NOW + 50, &opts).is_err());
    // Too late for the root helper (activation older than 24 h).
    // A request older than the 24 h install window is refused outright.
    assert_eq!(
        install(&req4, &dest, NOW + INSTALL_WINDOW + 1, &opts).unwrap_err(),
        "expired"
    );
}

#[test]
fn install_refusals() {
    let f = SoftKeyFile::new(2);
    let k = f.owner_key();
    let text = k.file_text();
    let act = activation_for(&text, &k.fingerprint(), &mut |d, b| f.sign(d, b));
    let req = install_request(&text, &act, None, NOW);
    let opts = InstallOpts {
        chown_root: false,
        dry_run: false,
    };
    // The old gate key path is never written.
    let e = install(
        &req,
        std::path::Path::new("/var/lib/galion-gate/owner.pub"),
        NOW,
        &opts,
    )
    .unwrap_err();
    assert!(e.contains("old gate key"), "{e}");
    // Activation older than the install window.
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("gok-refuse-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    assert_eq!(
        install(
            &req,
            &dir.join("owner.pub"),
            NOW + INSTALL_WINDOW + 1,
            &opts
        )
        .unwrap_err(),
        "expired"
    );
    // Symlink at the destination.
    let link = dir.join("link.pub");
    let _ = std::os::unix::fs::symlink(dir.join("elsewhere"), &link);
    assert!(install(&req, &link, NOW, &opts)
        .unwrap_err()
        .contains("symlink"));
    // Not confirmed by Slavik: activation body says confirmed_match false.
    let b = activate_body(&text, &k.fingerprint(), &"6".repeat(32), NOW, NOW + 60)
        .replace("\"confirmed_match\":true", "\"confirmed_match\":false");
    let a = envelope(f.sign(ACTIVATE_V, &b), "body_json", &b);
    let r = install_request(&text, &a, None, NOW);
    assert!(install(&r, &dir.join("x.pub"), NOW, &opts)
        .unwrap_err()
        .contains("not confirmed"));
    // Activation signed by a different key than the one being installed.
    let g = SoftKeyFile::new(3);
    let a = activation_for(&text, &k.fingerprint(), &mut |d, b| g.sign(d, b));
    let r = install_request(&text, &a, None, NOW);
    assert!(install(&r, &dir.join("y.pub"), NOW, &opts).is_err());
}

#[test]
fn registration_client_data() {
    let p = SoftPasskey::new(1, "galion.app", ORIGIN);
    let ch = webauthn_challenge(REGISTER_V, "x");
    let cd = p.client_data("webauthn.create", &ch);
    let origins = vec![ORIGIN.to_string()];
    check_client_data(&cd, "webauthn.create", &ch, &origins).unwrap();
    assert!(check_client_data(&cd, "webauthn.get", &ch, &origins).is_err());
    assert!(check_client_data(&cd, "webauthn.create", &[0u8; 32], &origins).is_err());
    assert_eq!(sec1_from_spki(&p.spki()).unwrap(), p.sec1());
    assert!(sec1_from_spki(&p.sec1()).is_err());
    let f = SoftKeyFile::new(1);
    let raw = f.owner_key().raw().to_vec();
    let mut spki = unhex(SPKI_ED25519_PREFIX).unwrap();
    spki.extend(&raw);
    assert_eq!(ed25519_from_spki_or_raw(&spki).unwrap().to_vec(), raw);
    assert_eq!(ed25519_from_spki_or_raw(&raw).unwrap().to_vec(), raw);
}
