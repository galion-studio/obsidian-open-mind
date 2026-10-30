// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Short-lived pairing invites (owner order t431u/t433u, 2026-10-30 CET).
//!
//! The device store's `codeHash` is the long-lived owner code (shared with
//! the console). An invite is a second, one-time code that expires:
//!
//! - minted by the operator on the box (`galion-one pair-invite [minutes]`,
//!   the code goes to stdout only) or by an already paired device
//!   (`POST /pair/invite`, the code is shown on that device's screen);
//! - stored only as a SHA-256 hash plus expiry in `ONE_INVITE_STORE`
//!   (default /root/galion-secrets/one-invite.json, mode 600); one invite
//!   at a time, a new one replaces the old;
//! - accepted once by `POST /pair` until it expires, then emptied.
//!
//! Codes are 8 characters from an alphabet without 0/O/1/I/L, shown as
//! XXXX-XXXX; dashes and spaces are ignored when typed.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
pub const DEFAULT_MINUTES: u64 = 15;
pub const MAX_MINUTES: u64 = 60;

pub fn store_path() -> String {
    crate::env_path("ONE_INVITE_STORE", "/root/galion-secrets/one-invite.json")
}

/// Upper-case, with dashes and spaces removed.
pub fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, '-' | ' ' | '\t'))
        .collect::<String>()
        .to_ascii_uppercase()
}

/// A fresh 8-character code, or None when the system RNG is unreadable.
pub fn fresh_code() -> Option<String> {
    let mut raw = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut raw))
        .ok()?;
    let mut out = String::new();
    for b in raw {
        // 248 = 8 * 31: reject the top values so every letter is equally likely.
        if b < 248 {
            out.push(ALPHABET[(b % 31) as usize] as char);
            if out.len() == 8 {
                return Some(out);
            }
        }
    }
    None
}

pub fn pretty(code: &str) -> String {
    format!("{}-{}", &code[..4], &code[4..])
}

/// Write a new invite (replacing any old one). Returns the plain code.
pub fn mint_at(path: &str, now_ms: u64, minutes: u64) -> Result<(String, u64), String> {
    let minutes = minutes.clamp(1, MAX_MINUTES);
    let code = fresh_code().ok_or("the system random source is unreadable")?;
    let expires = now_ms + minutes * 60_000;
    let body = format!(
        "{{\"codeHash\":\"{}\",\"expires\":{expires}}}\n",
        crate::sha256(&code)
    );
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("open {path}: {e}"))?;
    f.write_all(body.as_bytes())
        .map_err(|e| format!("write {path}: {e}"))?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok((code, expires))
}

/// True (and the invite is used up) when `code` matches a live invite.
pub fn take_at(path: &str, now_ms: u64, code: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let want = crate::json_string(text.as_bytes(), "codeHash");
    let expires = crate::json_u64(text.as_bytes(), "expires");
    if want.is_empty() {
        return false;
    }
    if now_ms >= expires {
        empty(path);
        return false;
    }
    let code = normalize(code);
    if code.len() != 8 || crate::sha256(&code) != want {
        return false;
    }
    empty(path);
    true
}

/// Clear the invite in place (no delete): the file stays, with no code.
fn empty(path: &str) {
    if let Ok(mut f) = fs::OpenOptions::new().write(true).truncate(true).open(path) {
        let _ = f.write_all(b"{}\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out");
        std::fs::create_dir_all(&d).unwrap();
        d.join(name).display().to_string()
    }

    #[test]
    fn codes_are_8_unambiguous_chars() {
        for _ in 0..200 {
            let c = fresh_code().unwrap();
            assert_eq!(c.len(), 8);
            assert!(c.bytes().all(|b| ALPHABET.contains(&b)), "{c}");
        }
        assert_eq!(pretty("ABCD2345"), "ABCD-2345");
        assert_eq!(normalize(" abcd-2345 "), "ABCD2345");
    }

    #[test]
    fn invite_is_one_time_and_expires() {
        let p = tmp("invite-a.json");
        let (code, exp) = mint_at(&p, 1_000, 15).unwrap();
        assert_eq!(exp, 1_000 + 15 * 60_000);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains(&code), "only the hash is stored");
        assert!(!take_at(&p, 2_000, "WRONG234"));
        assert!(take_at(&p, 2_000, &pretty(&code).to_ascii_lowercase()));
        assert!(!take_at(&p, 2_001, &code), "used up");
        let (code2, exp2) = mint_at(&p, 0, 1).unwrap();
        assert!(!take_at(&p, exp2, &code2), "expired at the deadline");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}\n");
    }

    #[test]
    fn minutes_are_clamped_and_missing_store_refuses() {
        let p = tmp("invite-b.json");
        assert_eq!(mint_at(&p, 0, 0).unwrap().1, 60_000);
        assert_eq!(mint_at(&p, 0, 999).unwrap().1, MAX_MINUTES * 60_000);
        assert!(!take_at(&tmp("invite-none.json"), 0, "ABCD2345"));
    }
}
