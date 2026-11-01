// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Stripe-Signature verification against FIXED vectors. The expected v1
//! values were computed once, independently of this crate, with
//! `printf '%s' "<t>.<body>" | openssl dgst -sha256 -hmac <secret>` (a CLI
//! used only to derive the constants; nothing links OpenSSL).

use galion_accounts::pay::stripe::{sign, verify_signature, SigError};

const SECRET: &str = "whsec_test_galion_fixed_vector_secret";
const T: i64 = 1_761_955_200;
const BODY: &str = r#"{"id":"evt_fixed_1","object":"event","type":"invoice.paid"}"#;
const V1: &str = "708b7045a45e71678ed11a0d5dbc337a6b8c7001d3b4de62d2909ac5c1fabd13";
/// Same secret and t, body with one trailing space.
const V1_SPACE: &str = "60e725c00b553791b28459f61c8bb388823feaf70ffa6adc498c1b65edc8fc2d";
/// secret "whsec_other", t=1700000000, body "{}".
const V1_OTHER: &str = "36c5ad38b2ab8df0d8bdff252bc59e124df92d1dea6bd02620dd7da6afcce2ac";

fn hdr(t: i64, v1: &str) -> String {
    format!("t={t},v1={v1}")
}

#[test]
fn sign_matches_independent_vectors() {
    assert_eq!(sign(SECRET, T, BODY.as_bytes()), V1);
    assert_eq!(sign(SECRET, T, format!("{BODY} ").as_bytes()), V1_SPACE);
    assert_eq!(sign("whsec_other", 1_700_000_000, b"{}"), V1_OTHER);
}

#[test]
fn valid_header_accepted() {
    let h = hdr(T, V1);
    assert_eq!(
        verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T),
        Ok(T)
    );
    // Edge of the tolerance window, both directions.
    assert!(verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T + 300).is_ok());
    assert!(verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T - 300).is_ok());
}

#[test]
fn tampered_body_or_wrong_secret_rejected() {
    let h = hdr(T, V1);
    let tampered = BODY.replace("invoice.paid", "invoice.paiD");
    assert_eq!(
        verify_signature(Some(&h), tampered.as_bytes(), SECRET, 300, T),
        Err(SigError::Mismatch)
    );
    assert_eq!(
        verify_signature(Some(&h), format!("{BODY} ").as_bytes(), SECRET, 300, T),
        Err(SigError::Mismatch)
    );
    assert_eq!(
        verify_signature(Some(&h), BODY.as_bytes(), "whsec_wrong", 300, T),
        Err(SigError::Mismatch)
    );
    // A valid signature moved to another timestamp no longer matches.
    assert_eq!(
        verify_signature(Some(&hdr(T + 1, V1)), BODY.as_bytes(), SECRET, 300, T),
        Err(SigError::Mismatch)
    );
}

#[test]
fn replayed_old_or_future_timestamps_rejected() {
    let h = hdr(T, V1);
    assert_eq!(
        verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T + 301),
        Err(SigError::OutsideTolerance)
    );
    assert_eq!(
        verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T - 301),
        Err(SigError::OutsideTolerance)
    );
}

#[test]
fn multiple_v1_and_v0_handled() {
    // Stripe sends several v1 during secret rolls; any match is enough.
    let h = format!("t={T},v1={},v1={V1},v0=deadbeef", "0".repeat(64));
    assert!(verify_signature(Some(&h), BODY.as_bytes(), SECRET, 300, T).is_ok());
    // Only v0 (legacy test scheme) is never accepted.
    let h0 = format!("t={T},v0={V1}");
    assert_eq!(
        verify_signature(Some(&h0), BODY.as_bytes(), SECRET, 300, T),
        Err(SigError::NoV1)
    );
}

#[test]
fn missing_or_malformed_rejected() {
    assert_eq!(
        verify_signature(None, BODY.as_bytes(), SECRET, 300, T),
        Err(SigError::Missing)
    );
    for bad in ["", "garbage", "v1=abc", "t=notanumber,v1=abc", "t=1,,v1=x"] {
        let r = verify_signature(Some(bad), BODY.as_bytes(), SECRET, 300, T);
        assert!(
            matches!(r, Err(SigError::Malformed) | Err(SigError::NoV1)),
            "{bad:?} -> {r:?}"
        );
    }
    // Upper-case hex is not what Stripe sends: refused.
    let up = hdr(T, &V1.to_ascii_uppercase());
    assert_eq!(
        verify_signature(Some(&up), BODY.as_bytes(), SECRET, 300, T),
        Err(SigError::Mismatch)
    );
}
