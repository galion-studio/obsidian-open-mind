// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Small, careful file and encoding helpers shared by the signer and deploy.

use ed25519_dalek::{SigningKey, VerifyingKey};
use hermes_rs::self_modify::{hex, sha256_hex, signing_key, verifying_key};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

/// Largest artifact the signer/deploy will hash (a hermes-gateway is a few MB).
pub const MAX_ARTIFACT: u64 = 512 * 1024 * 1024;
/// Largest log or JSON input.
pub const MAX_SMALL: u64 = 64 * 1024 * 1024;

pub fn now_secs() -> i64 {
    hermes_rs::time::now_secs()
}

pub fn now_zagreb() -> String {
    hermes_rs::time::zagreb_rfc3339(now_secs())
}

// ---------- base64url (RFC 4648 section 5, no padding) ----------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn b64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        let take = c.len() + 1;
        for i in 0..take {
            out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

pub fn unb64url(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim().trim_end_matches('=');
    if s.len() % 4 == 1 {
        return Err("bad base64url length".into());
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err("not base64url".into()),
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if acc != 0 {
        return Err("base64url has non-zero trailing bits".into());
    }
    Ok(out)
}

// ---------- files ----------

/// Open a regular file without following a symlink in the LAST component.
fn open_nofollow(p: &Path) -> Result<std::fs::File, String> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|e| format!("{}: {e} (symlinks are refused)", p.display()))?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err(format!("{}: not a regular file", p.display()));
    }
    Ok(f)
}

/// Read a whole regular file (no symlink), at most `max` bytes.
pub fn read_file(p: &Path, max: u64) -> Result<Vec<u8>, String> {
    let f = open_nofollow(p)?;
    let mut b = vec![];
    f.take(max + 1)
        .read_to_end(&mut b)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    if b.len() as u64 > max {
        return Err(format!("{}: larger than {max} bytes", p.display()));
    }
    Ok(b)
}

/// (sha256 hex, byte count) of a regular file, streamed, no symlink.
pub fn hash_file(p: &Path, max: u64) -> Result<(String, u64), String> {
    let mut f = open_nofollow(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n: u64 = 0;
    loop {
        let k = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        if k == 0 {
            break;
        }
        n += k as u64;
        if n > max {
            return Err(format!("{}: larger than {max} bytes", p.display()));
        }
        h.update(&buf[..k]);
    }
    Ok((hex(&h.finalize()), n))
}

pub fn read_json(p: &Path) -> Result<serde_json::Value, String> {
    serde_json::from_slice(&read_file(p, MAX_SMALL)?).map_err(|e| format!("{}: {e}", p.display()))
}

/// Resolve `rel` (from untrusted input) under `base`: plain relative
/// components only, and the real path (all symlinks resolved) must stay
/// inside the real `base`.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let rp = Path::new(rel);
    if rel.is_empty()
        || rel.len() > 200
        || !rp.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("unsafe path {rel:?} (relative, no '..')"));
    }
    let real_base = base
        .canonicalize()
        .map_err(|e| format!("{}: {e}", base.display()))?;
    let joined = base.join(rp);
    let real = joined
        .canonicalize()
        .map_err(|e| format!("{}: {e}", joined.display()))?;
    if !real.starts_with(&real_base) {
        return Err(format!("{rel:?} leaves {}", base.display()));
    }
    Ok(joined)
}

/// Write via a temp file + rename (same directory).
pub fn write_atomic(p: &Path, body: &[u8], mode: u32) -> Result<(), String> {
    let tmp = p.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(body).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Create a NEW file (fails if it exists or is a symlink).
pub fn write_new(p: &Path, body: &[u8], mode: u32) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    f.write_all(body).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}

// ---------- keys ----------

pub fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Load an Ed25519 seed file. Refused unless it is a regular file owned by
/// the current user and not readable by group/others (0600 or 0400).
pub fn load_seed(p: &Path) -> Result<SigningKey, String> {
    let f = open_nofollow(p)?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if m.mode() & 0o077 != 0 {
        return Err(format!(
            "{}: key file must not be group/other accessible (chmod 600)",
            p.display()
        ));
    }
    if m.uid() != euid() {
        return Err(format!(
            "{}: key file must be owned by the signing account",
            p.display()
        ));
    }
    let b = read_file(p, 256)?;
    let s = String::from_utf8(b).map_err(|_| "key file is not text".to_string())?;
    signing_key(s.trim())
}

/// Load a 64-hex Ed25519 public key file.
pub fn load_pub(p: &Path) -> Result<VerifyingKey, String> {
    let b = read_file(p, 4096)?;
    let s = String::from_utf8(b).map_err(|_| "public key file is not text".to_string())?;
    verifying_key(s.trim()).map_err(|e| format!("{}: {e}", p.display()))
}

/// Fresh Ed25519 key: writes the seed (0600, new file only) and returns the
/// public key hex. The seed is never printed.
pub fn keygen_to(seed_out: &Path) -> Result<String, String> {
    let (seed, pubhex) = hermes_rs::self_modify::keygen()?;
    write_new(seed_out, format!("{seed}\n").as_bytes(), 0o600)?;
    Ok(pubhex)
}

/// Fingerprint used everywhere for files: sha256 of the bytes.
pub fn fingerprint(b: &[u8]) -> String {
    sha256_hex(b)
}

/// 4-char groups for showing a fingerprint to a person.
pub fn grouped(h: &str) -> String {
    h.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64url_roundtrip_and_vectors() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        for n in 0..70u8 {
            let v: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(unb64url(&b64url(&v)).unwrap(), v);
        }
        assert!(unb64url("Zh").is_err(), "non-zero trailing bits");
        assert!(unb64url("Z").is_err());
        assert!(unb64url("Zm9v+").is_err());
    }

    #[test]
    fn safe_join_refuses_escape() {
        let d = std::env::temp_dir().join(format!("gsm-sj-{}", std::process::id()));
        std::fs::create_dir_all(d.join("logs")).unwrap();
        std::fs::write(d.join("logs/a.log"), b"x").unwrap();
        assert!(safe_join(&d, "logs/a.log").is_ok());
        assert!(safe_join(&d, "../x").is_err());
        assert!(safe_join(&d, "/etc/passwd").is_err());
        assert!(safe_join(&d, "").is_err());
        std::os::unix::fs::symlink("/etc", d.join("logs/esc")).unwrap();
        assert!(safe_join(&d, "logs/esc/hostname").is_err());
    }
}
