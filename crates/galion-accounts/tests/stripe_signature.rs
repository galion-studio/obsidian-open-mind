// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Stripe-Signature verification against FIXED vectors. The expected v1
//! constants are re-derived in this file by a second, independent pure-Rust
//! SHA-256 + HMAC (written from FIPS 180-4 and RFC 2104, sharing no code
//! with the crate or galion_common), which is itself checked against the
//! published FIPS 180-2 and RFC 4231 test vectors. No external tool.

use galion_accounts::pay::stripe::{sign, verify_signature, SigError};

/// Independent reference SHA-256 (FIPS 180-4), test-only.
fn ref_sha256(msg: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut m = msg.to_vec();
    let bits = (msg.len() as u64).wrapping_mul(8);
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_be_bytes());
    for block in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

/// Independent reference HMAC-SHA256 (RFC 2104), lower-case hex.
fn ref_hmac_hex(key: &[u8], msg: &[u8]) -> String {
    let mut k = if key.len() > 64 {
        ref_sha256(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(64, 0);
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let ih = ref_sha256(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    ref_sha256(&outer)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn stripe_ref(secret: &str, t: i64, body: &[u8]) -> String {
    let mut m = format!("{t}.").into_bytes();
    m.extend_from_slice(body);
    ref_hmac_hex(secret.as_bytes(), &m)
}

#[test]
fn reference_implementation_matches_published_vectors() {
    let hex = |d: [u8; 32]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
    // FIPS 180-2 appendix B.1 / B.2 and the empty message.
    assert_eq!(
        hex(ref_sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(ref_sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        hex(ref_sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    // RFC 4231 test cases 1, 2 and 6 (key longer than the block).
    assert_eq!(
        ref_hmac_hex(&[0x0b; 20], b"Hi There"),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        ref_hmac_hex(b"Jefe", b"what do ya want for nothing?"),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        ref_hmac_hex(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        ),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn fixed_vectors_rederived_in_rust() {
    assert_eq!(stripe_ref(SECRET, T, BODY.as_bytes()), V1);
    assert_eq!(
        stripe_ref(SECRET, T, format!("{BODY} ").as_bytes()),
        V1_SPACE
    );
    assert_eq!(stripe_ref("whsec_other", 1_700_000_000, b"{}"), V1_OTHER);
    // And the crate agrees with the independent implementation on many
    // lengths around the 64-byte block edges.
    for n in [0usize, 1, 55, 56, 63, 64, 65, 119, 120, 128, 1000] {
        let body = vec![b'x'; n];
        assert_eq!(
            sign(SECRET, T, &body),
            stripe_ref(SECRET, T, &body),
            "len {n}"
        );
    }
}

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
