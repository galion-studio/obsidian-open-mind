// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Signed, expiring, single-use approval tokens scoped to one exact action.
//!
//! Wire form: `hades1.<payload-hex>.<signature-hex>`.
//!
//! The payload is one canonical ASCII line:
//!
//! ```text
//! v=1;action=<kind>;target=<service>;plan=<sha256-hex>;nonce=<32-hex>;iat=<unix>;exp=<unix>
//! ```
//!
//! The signature is Ed25519 over [`SIGN_DOMAIN`] followed by the payload bytes,
//! checked with `verify_strict` (no malleable signatures, no weak keys). Only
//! the admin's 32-byte public key lives on the gate host; the private seed
//! stays with the admin, so the gate cannot mint its own approvals.
//!
//! This module only parses and checks. It reads no file and keeps no state:
//! single use is enforced by [`crate::ledger`].

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// Prefix of every token string.
pub const TOKEN_PREFIX: &str = "hades1";
/// Domain separation: signatures for anything else never verify here.
pub const SIGN_DOMAIN: &[u8] = b"galion-hades-gate/approval/v1\n";
/// Longest accepted token string.
pub const MAX_TOKEN_LEN: usize = 1024;
/// Longest accepted target name.
pub const MAX_TARGET_LEN: usize = 64;
/// Clock skew tolerated for `iat` in the future, in seconds.
pub const MAX_CLOCK_SKEW_SECS: u64 = 60;
/// Hard ceiling on a token's lifetime (`exp - iat`), whatever the config says.
pub const HARD_MAX_TTL_SECS: u64 = 3600;

/// What one token approves. Every field is covered by the signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    /// Action kind wire name, for example `heal-apply`.
    pub action: String,
    /// Exact target service name.
    pub target: String,
    /// SHA-256 of the exact plan bytes.
    pub plan_hash: [u8; 32],
    /// Random single-use nonce.
    pub nonce: [u8; 16],
    /// Issued at, Unix seconds.
    pub issued_at: u64,
    /// Expires at, Unix seconds (exclusive).
    pub expires_at: u64,
}

/// Why a token was rejected. None of these reveal key material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// Not `hades1.<hex>.<hex>`, bad hex, non-canonical payload or too long.
    Malformed(&'static str),
    /// The signature does not verify with the admin public key.
    BadSignature,
    /// `now >= exp`.
    Expired,
    /// `iat` is too far in the future.
    NotYetValid,
    /// `exp - iat` is longer than the allowed TTL, or `exp <= iat`.
    BadLifetime,
    /// Signed for another action, target or plan.
    WrongScope(&'static str),
}

impl TokenError {
    /// Short machine-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Malformed(why) => format!("malformed token: {why}"),
            Self::BadSignature => "forged or tampered token: signature does not verify".into(),
            Self::Expired => "expired token".into(),
            Self::NotYetValid => "token issued in the future".into(),
            Self::BadLifetime => "token lifetime exceeds the allowed TTL".into(),
            Self::WrongScope(field) => format!("token is scoped to a different {field}"),
        }
    }
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for TokenError {}

/// SHA-256 of the exact plan bytes.
#[must_use]
pub fn plan_hash(plan: &[u8]) -> [u8; 32] {
    Sha256::digest(plan).into()
}

/// Lower-case hex.
#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}

/// Strict lower-case hex decode.
#[must_use]
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    fn nib(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let raw = text.as_bytes();
    if !raw.len().is_multiple_of(2) {
        return None;
    }
    raw.chunks(2)
        .map(|pair| Some((nib(pair[0])? << 4) | nib(pair[1])?))
        .collect()
}

/// Decode exactly `N` bytes of hex.
#[must_use]
pub fn hex_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    from_hex(text)?.try_into().ok()
}

/// A target name: 1 to 64 of `A-Z a-z 0-9 . _ - @ :`.
#[must_use]
pub fn valid_target(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= MAX_TARGET_LEN
        && target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@' | b':'))
}

fn valid_action(action: &str) -> bool {
    !action.is_empty()
        && action.len() <= 32
        && action
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Claims {
    /// The canonical payload line that is signed.
    #[must_use]
    pub fn payload(&self) -> String {
        format!(
            "v=1;action={};target={};plan={};nonce={};iat={};exp={}",
            self.action,
            self.target,
            to_hex(&self.plan_hash),
            to_hex(&self.nonce),
            self.issued_at,
            self.expires_at
        )
    }

    /// Parse a payload line. It must re-encode to the same bytes.
    ///
    /// # Errors
    /// [`TokenError::Malformed`] for anything that is not canonical.
    pub fn parse_payload(text: &str) -> Result<Self, TokenError> {
        let bad = TokenError::Malformed;
        let mut fields = text.split(';');
        let mut take = |key: &str| -> Result<&str, TokenError> {
            let field = fields.next().ok_or(bad("missing field"))?;
            field
                .strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('='))
                .ok_or(bad("unexpected field"))
        };
        if take("v")? != "1" {
            return Err(bad("unknown version"));
        }
        let action = take("action")?.to_owned();
        let target = take("target")?.to_owned();
        let plan_hash = hex_array::<32>(take("plan")?).ok_or(bad("plan hash"))?;
        let nonce = hex_array::<16>(take("nonce")?).ok_or(bad("nonce"))?;
        let issued_at = parse_u64(take("iat")?).ok_or(bad("iat"))?;
        let expires_at = parse_u64(take("exp")?).ok_or(bad("exp"))?;
        if fields.next().is_some() {
            return Err(bad("trailing field"));
        }
        if !valid_action(&action) {
            return Err(bad("action name"));
        }
        if !valid_target(&target) {
            return Err(bad("target name"));
        }
        let claims = Self {
            action,
            target,
            plan_hash,
            nonce,
            issued_at,
            expires_at,
        };
        if claims.payload() != text {
            return Err(bad("non-canonical payload"));
        }
        Ok(claims)
    }

    /// Sign with the admin seed and return the token string.
    ///
    /// Used by the admin-side `mint` command and by tests with throwaway keys.
    #[must_use]
    pub fn sign(&self, key: &SigningKey) -> String {
        let payload = self.payload();
        let sig = key.sign(&signed_message(payload.as_bytes()));
        format!(
            "{TOKEN_PREFIX}.{}.{}",
            to_hex(payload.as_bytes()),
            to_hex(&sig.to_bytes())
        )
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    // No sign, no leading zeros (canonical), at most 20 digits.
    if text.is_empty() || text.len() > 20 || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn signed_message(payload: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(SIGN_DOMAIN.len() + payload.len());
    msg.extend_from_slice(SIGN_DOMAIN);
    msg.extend_from_slice(payload);
    msg
}

/// Check the signature only and return the signed claims.
///
/// # Errors
/// [`TokenError::Malformed`] or [`TokenError::BadSignature`].
pub fn verify_signature(token: &str, admin: &VerifyingKey) -> Result<Claims, TokenError> {
    let bad = TokenError::Malformed;
    if token.len() > MAX_TOKEN_LEN {
        return Err(bad("too long"));
    }
    let mut parts = token.split('.');
    if parts.next() != Some(TOKEN_PREFIX) {
        return Err(bad("prefix"));
    }
    let payload_hex = parts.next().ok_or(bad("missing payload"))?;
    let sig_hex = parts.next().ok_or(bad("missing signature"))?;
    if parts.next().is_some() {
        return Err(bad("extra part"));
    }
    let payload = from_hex(payload_hex).ok_or(bad("payload hex"))?;
    let sig = hex_array::<64>(sig_hex).ok_or(bad("signature hex"))?;
    let sig = Signature::from_bytes(&sig);
    admin
        .verify_strict(&signed_message(&payload), &sig)
        .map_err(|_| TokenError::BadSignature)?;
    let text = std::str::from_utf8(&payload).map_err(|_| bad("payload utf-8"))?;
    Claims::parse_payload(text)
}

/// The exact action a caller wants approved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope<'a> {
    /// Action kind wire name.
    pub action: &'a str,
    /// Target service.
    pub target: &'a str,
    /// The exact plan bytes (hashed here).
    pub plan: &'a [u8],
}

/// Full check: signature, lifetime, expiry and exact scope. Not replay.
///
/// # Errors
/// The first [`TokenError`] found. Signature is checked first, so a forged
/// token never reaches the scope or clock checks.
pub fn verify(
    token: &str,
    admin: &VerifyingKey,
    scope: Scope<'_>,
    now: u64,
    max_ttl_secs: u64,
) -> Result<Claims, TokenError> {
    let claims = verify_signature(token, admin)?;
    let ttl = max_ttl_secs.min(HARD_MAX_TTL_SECS);
    if claims.expires_at <= claims.issued_at || claims.expires_at - claims.issued_at > ttl {
        return Err(TokenError::BadLifetime);
    }
    if claims.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECS) {
        return Err(TokenError::NotYetValid);
    }
    if now >= claims.expires_at {
        return Err(TokenError::Expired);
    }
    if claims.action != scope.action {
        return Err(TokenError::WrongScope("action"));
    }
    if claims.target != scope.target {
        return Err(TokenError::WrongScope("target"));
    }
    if claims.plan_hash != plan_hash(scope.plan) {
        return Err(TokenError::WrongScope("plan"));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn claims(now: u64) -> Claims {
        Claims {
            action: "heal-apply".into(),
            target: "galion-tuner".into(),
            plan_hash: plan_hash(b"restart galion-tuner"),
            nonce: [7; 16],
            issued_at: now,
            expires_at: now + 300,
        }
    }

    fn scope() -> Scope<'static> {
        Scope {
            action: "heal-apply",
            target: "galion-tuner",
            plan: b"restart galion-tuner",
        }
    }

    const NOW: u64 = 1_792_000_000;

    #[test]
    fn good_token_verifies() {
        let k = key(1);
        let t = claims(NOW).sign(&k);
        let got = verify(&t, &k.verifying_key(), scope(), NOW + 10, 900).unwrap();
        assert_eq!(got, claims(NOW));
    }

    #[test]
    fn forged_with_another_key_is_refused() {
        let t = claims(NOW).sign(&key(2));
        assert_eq!(
            verify(&t, &key(1).verifying_key(), scope(), NOW, 900),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn tampered_payload_is_refused() {
        let k = key(1);
        let t = claims(NOW).sign(&k);
        let mut c = claims(NOW);
        c.expires_at += 1;
        let forged_payload = to_hex(c.payload().as_bytes());
        let sig = t.rsplit('.').next().unwrap();
        let forged = format!("{TOKEN_PREFIX}.{forged_payload}.{sig}");
        assert_eq!(
            verify(&forged, &k.verifying_key(), scope(), NOW, 900),
            Err(TokenError::BadSignature)
        );
        // Flip one signature nibble.
        let mut bytes = t.into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'0' { b'1' } else { b'0' };
        let flipped = String::from_utf8(bytes).unwrap();
        assert_eq!(
            verify(&flipped, &k.verifying_key(), scope(), NOW, 900),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn garbage_is_malformed() {
        let k = key(1).verifying_key();
        for t in [
            "",
            "approved-test",
            "hades1",
            "hades1..",
            "hades1.zz.00",
            "hades2.00.00",
            "hades1.00.00.00",
        ] {
            assert!(
                matches!(
                    verify(t, &k, scope(), NOW, 900),
                    Err(TokenError::Malformed(_))
                ),
                "{t}"
            );
        }
        let long = "a".repeat(MAX_TOKEN_LEN + 1);
        assert!(matches!(
            verify(&long, &k, scope(), NOW, 900),
            Err(TokenError::Malformed(_))
        ));
    }

    #[test]
    fn expired_is_refused() {
        let k = key(1);
        let t = claims(NOW).sign(&k);
        assert_eq!(
            verify(&t, &k.verifying_key(), scope(), NOW + 300, 900),
            Err(TokenError::Expired)
        );
        assert!(verify(&t, &k.verifying_key(), scope(), NOW + 299, 900).is_ok());
    }

    #[test]
    fn future_and_overlong_are_refused() {
        let k = key(1);
        let t = claims(NOW + 3600).sign(&k);
        assert_eq!(
            verify(&t, &k.verifying_key(), scope(), NOW, 900),
            Err(TokenError::NotYetValid)
        );
        let mut c = claims(NOW);
        c.expires_at = NOW + 901;
        assert_eq!(
            verify(&c.sign(&k), &k.verifying_key(), scope(), NOW, 900),
            Err(TokenError::BadLifetime)
        );
        // The hard cap wins over a generous config.
        c.expires_at = NOW + HARD_MAX_TTL_SECS + 1;
        assert_eq!(
            verify(&c.sign(&k), &k.verifying_key(), scope(), NOW, u64::MAX),
            Err(TokenError::BadLifetime)
        );
        c.expires_at = NOW;
        assert_eq!(
            verify(&c.sign(&k), &k.verifying_key(), scope(), NOW, 900),
            Err(TokenError::BadLifetime)
        );
    }

    #[test]
    fn wrong_scope_is_refused() {
        let k = key(1);
        let v = k.verifying_key();
        let t = claims(NOW).sign(&k);
        let mut s = scope();
        s.action = "rollback";
        assert_eq!(
            verify(&t, &v, s, NOW, 900),
            Err(TokenError::WrongScope("action"))
        );
        let mut s = scope();
        s.target = "galion-one";
        assert_eq!(
            verify(&t, &v, s, NOW, 900),
            Err(TokenError::WrongScope("target"))
        );
        let mut s = scope();
        s.plan = b"restart galion-tuner ";
        assert_eq!(
            verify(&t, &v, s, NOW, 900),
            Err(TokenError::WrongScope("plan"))
        );
    }

    #[test]
    fn payload_must_be_canonical() {
        let c = claims(NOW);
        assert_eq!(Claims::parse_payload(&c.payload()), Ok(c.clone()));
        let p = c.payload();
        for bad in [
            p.replace("iat=", "iat=0"),
            p.replace("v=1", "v=2"),
            format!("{p};x=1"),
            p.replace("galion-tuner", "galion tuner"),
            p.replace("heal-apply", "HEAL-APPLY"),
        ] {
            assert!(Claims::parse_payload(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(to_hex(&[0, 0xab, 0xff]), "00abff");
        assert_eq!(from_hex("00abff"), Some(vec![0, 0xab, 0xff]));
        assert_eq!(from_hex("00ABFF"), None);
        assert_eq!(from_hex("0"), None);
        // FIPS 180-2 "abc".
        assert_eq!(
            to_hex(&plan_hash(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
