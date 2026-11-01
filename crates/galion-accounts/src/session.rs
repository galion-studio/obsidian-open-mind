// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Session cookies. The cookie holds 32 random bytes (base64url); the
//! database holds only their SHA-256, so a database copy cannot be replayed
//! as a login. No IP or user agent is tied to a session.

/// Cookie name. One cookie for every galion.app service.
pub const COOKIE: &str = "galion_acct";
/// Sessions last 30 days.
pub const TTL: i64 = 30 * 86_400;

/// A new random token (what goes in the cookie).
pub fn new_token() -> String {
    galion_common::base64::encode_url(&crate::rng::bytes(32))
}

/// What the database stores for a token.
pub fn token_hash(token: &str) -> [u8; 32] {
    galion_common::sha256::digest(token.as_bytes())
}

/// Our cookie's value from a `Cookie:` header.
pub fn from_cookie_header(h: &str) -> Option<String> {
    h.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == COOKIE && !v.is_empty() && v.len() <= 128).then(|| v.to_string())
    })
}

/// `Set-Cookie` value. `domain` (e.g. `galion.app`) shares it with the
/// other galion.app services; `secure` is off only for loopback tests.
pub fn set_cookie(token: &str, domain: Option<&str>, secure: bool) -> String {
    let mut c = format!("{COOKIE}={token}; Path=/; Max-Age={TTL}; HttpOnly; SameSite=Lax");
    if secure {
        c.push_str("; Secure");
    }
    if let Some(d) = domain {
        c.push_str("; Domain=");
        c.push_str(d);
    }
    c
}

/// `Set-Cookie` value that removes the cookie.
pub fn clear_cookie(domain: Option<&str>, secure: bool) -> String {
    let mut c = format!("{COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax");
    if secure {
        c.push_str("; Secure");
    }
    if let Some(d) = domain {
        c.push_str("; Domain=");
        c.push_str(d);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_and_cookie() {
        let t = new_token();
        assert_eq!(t.len(), 43);
        assert_ne!(t, new_token());
        let h = format!("a=1; {COOKIE}={t}; b=2");
        assert_eq!(from_cookie_header(&h).as_deref(), Some(t.as_str()));
        assert_eq!(from_cookie_header("a=1"), None);
        let s = set_cookie(&t, Some("galion.app"), true);
        assert!(s.contains("HttpOnly") && s.contains("Secure") && s.contains("Domain=galion.app"));
    }
}
