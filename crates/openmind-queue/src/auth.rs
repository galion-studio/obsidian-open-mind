// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Token auth for POSEIDON before any LAN bind (Gaia S4.1).
//!
//! Model: one shared secret read from a file the service user owns with
//! mode 0600 or 0400 (no group/other bits). A request carries it as
//! `Authorization: Bearer <secret>`.
//!
//! * Loopback peers (127.0.0.0/8, ::1, ::ffff:127.x) need no token by default,
//!   so every existing loopback client keeps working unchanged. With
//!   `require_on_loopback` they need one too.
//! * Every non-loopback peer needs the token, on every path (including
//!   `/healthz`). No secret configured means every non-loopback request is
//!   refused (fail closed); the server also refuses to start on a
//!   non-loopback bind without a secret.
//! * The compare is constant-time; the secret is never logged, never put in
//!   a reply, and wiped from memory on drop. `Debug` prints a redaction.
//!
//! std only, no new crates.

use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::Path;

/// Shortest accepted secret (bytes). 32 random printable bytes ~ 190+ bits.
pub const MIN_SECRET: usize = 32;
/// Longest accepted secret (bytes); keeps the header well inside MAX_HEADER.
pub const MAX_SECRET: usize = 1024;

/// A shared secret held in memory. Wiped on drop; never printed.
pub struct Secret(Vec<u8>);

impl Secret {
    /// Build from bytes after validation (length, visible ASCII, no spaces).
    pub fn new(bytes: Vec<u8>) -> Result<Secret, String> {
        let s = Secret(bytes);
        if s.0.len() < MIN_SECRET {
            return Err(format!("secret too short (< {MIN_SECRET} bytes)"));
        }
        if s.0.len() > MAX_SECRET {
            return Err(format!("secret too long (> {MAX_SECRET} bytes)"));
        }
        if !s.0.iter().all(|b| b.is_ascii_graphic()) {
            return Err("secret must be visible ASCII without spaces".into());
        }
        Ok(s)
    }

    /// Constant-time (in the secret's content) comparison.
    pub fn matches(&self, presented: &[u8]) -> bool {
        ct_eq(&self.0, presented)
    }

    /// Bytes for a client to send. Callers must not log them.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            // SAFETY-free wipe: volatile-ish via black_box so it is not elided.
            *b = 0;
            std::hint::black_box(&*b);
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret(<redacted {} bytes>)", self.0.len())
    }
}

/// Compare without an early exit on the first differing byte. Length is not
/// secret (it is fixed per deployment), but a length mismatch still walks the
/// stored secret once so timing does not depend on where the mismatch is.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff: u8 = (a.len() != b.len()) as u8;
    for (i, x) in a.iter().enumerate() {
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

/// Load and check a secret file: regular file (symlinks refused), no
/// group/other permission bits, owned by the effective user, one line.
pub fn load_secret_file(path: &Path) -> io::Result<Secret> {
    use std::os::unix::fs::MetadataExt;
    let bad = |m: String| io::Error::new(io::ErrorKind::PermissionDenied, m);
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        return Err(bad(format!(
            "token file {} is not a regular file",
            path.display()
        )));
    }
    let mode = meta.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(bad(format!(
            "token file {} has mode {mode:04o}; need 0600 or 0400",
            path.display()
        )));
    }
    if let Some(euid) = effective_uid() {
        if meta.uid() != euid {
            return Err(bad(format!(
                "token file {} is owned by uid {}, not by this process (uid {euid})",
                path.display(),
                meta.uid()
            )));
        }
    }
    let mut raw = fs::read(path)?;
    while matches!(raw.last(), Some(b'\n' | b'\r')) {
        raw.pop();
    }
    let res = Secret::new(std::mem::take(&mut raw));
    for b in raw.iter_mut() {
        *b = 0;
    }
    res.map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("token file {}: {e}", path.display()),
        )
    })
}

/// The effective uid without libc: `/proc/self` is owned by it.
fn effective_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// True for 127.0.0.0/8, ::1 and IPv4-mapped loopback.
pub fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// What the server does with each request.
#[derive(Debug, Default)]
pub struct Policy {
    pub secret: Option<Secret>,
    pub require_on_loopback: bool,
}

/// Outcome of a check. Never carries the presented token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Loopback peer, no token needed.
    Loopback,
    /// A valid token was presented.
    Token,
    /// No `Authorization: Bearer` header.
    Missing,
    /// A token was presented and is wrong (or no secret is configured).
    Invalid,
    /// Non-loopback peer and the server has no secret: always refused.
    NoSecret,
}

impl Verdict {
    pub fn allowed(self) -> bool {
        matches!(self, Verdict::Loopback | Verdict::Token)
    }
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Loopback => "loopback",
            Verdict::Token => "token",
            Verdict::Missing => "missing",
            Verdict::Invalid => "invalid",
            Verdict::NoSecret => "no_secret",
        }
    }
}

/// Pull the bearer credential out of an `Authorization` header value.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let v = header?.trim();
    let (scheme, rest) = v.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let t = rest.trim();
    (!t.is_empty()).then_some(t)
}

impl Policy {
    pub fn mode(&self) -> &'static str {
        match (&self.secret, self.require_on_loopback) {
            (None, _) => "loopback-only",
            (Some(_), false) => "token-for-remote",
            (Some(_), true) => "token-for-all",
        }
    }

    /// Decide for a peer and its `Authorization` header (if any).
    pub fn check(&self, peer: Option<IpAddr>, authorization: Option<&str>) -> Verdict {
        // An unknown peer address is treated as remote (fail closed).
        let local = peer.is_some_and(is_loopback);
        if local && !self.require_on_loopback {
            return Verdict::Loopback;
        }
        let presented = bearer(authorization);
        match (&self.secret, presented) {
            (None, _) if !local => Verdict::NoSecret,
            (None, None) => Verdict::Missing,
            (None, Some(_)) => Verdict::Invalid,
            (Some(_), None) => Verdict::Missing,
            (Some(s), Some(t)) => {
                if s.matches(t.as_bytes()) {
                    Verdict::Token
                } else {
                    Verdict::Invalid
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::os::unix::fs::PermissionsExt;

    const S: &str = "0123456789abcdef0123456789abcdef-test-only";

    fn pol(req_lo: bool) -> Policy {
        Policy {
            secret: Some(Secret::new(S.as_bytes().to_vec()).unwrap()),
            require_on_loopback: req_lo,
        }
    }
    fn lan() -> Option<IpAddr> {
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)))
    }
    fn lo() -> Option<IpAddr> {
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    }

    #[test]
    fn ct_eq_basics() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(!ct_eq(b"", b"x"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn loopback_classes() {
        assert!(is_loopback(IpAddr::V4(Ipv4Addr::new(127, 9, 9, 9))));
        assert!(is_loopback(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_loopback(IpAddr::V6(
            Ipv4Addr::LOCALHOST.to_ipv6_mapped()
        )));
        assert!(!is_loopback(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_loopback(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(!is_loopback(IpAddr::V6(
            Ipv4Addr::new(192, 168, 1, 2).to_ipv6_mapped()
        )));
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer   abc  ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(Some("Bearer")), None);
        assert_eq!(bearer(None), None);
    }

    #[test]
    fn loopback_unchanged_by_default() {
        let p = Policy::default();
        assert_eq!(p.check(lo(), None), Verdict::Loopback);
        assert_eq!(pol(false).check(lo(), None), Verdict::Loopback);
        assert_eq!(
            pol(false).check(lo(), Some("Bearer wrong")),
            Verdict::Loopback
        );
    }

    #[test]
    fn remote_needs_token() {
        let p = pol(false);
        assert_eq!(p.check(lan(), None), Verdict::Missing);
        assert_eq!(p.check(lan(), Some("Bearer nope")), Verdict::Invalid);
        assert_eq!(p.check(lan(), Some(&format!("Basic {S}"))), Verdict::Missing);
        assert_eq!(p.check(lan(), Some(&format!("Bearer {S}x"))), Verdict::Invalid);
        assert_eq!(p.check(lan(), Some(&format!("Bearer {S}"))), Verdict::Token);
        // Unknown peer address counts as remote.
        assert_eq!(p.check(None, None), Verdict::Missing);
    }

    #[test]
    fn no_secret_fails_closed_for_remote() {
        let p = Policy::default();
        assert_eq!(p.check(lan(), None), Verdict::NoSecret);
        assert_eq!(p.check(lan(), Some(&format!("Bearer {S}"))), Verdict::NoSecret);
        assert_eq!(p.check(None, None), Verdict::NoSecret);
        assert!(!Verdict::NoSecret.allowed());
    }

    #[test]
    fn require_on_loopback() {
        let p = pol(true);
        assert_eq!(p.check(lo(), None), Verdict::Missing);
        assert_eq!(p.check(lo(), Some(&format!("Bearer {S}"))), Verdict::Token);
        assert_eq!(p.mode(), "token-for-all");
        assert_eq!(pol(false).mode(), "token-for-remote");
        assert_eq!(Policy::default().mode(), "loopback-only");
    }

    #[test]
    fn secret_validation_and_debug_redacts() {
        assert!(Secret::new(b"short".to_vec()).is_err());
        assert!(Secret::new(vec![b'a'; MAX_SECRET + 1]).is_err());
        assert!(Secret::new(b"0123456789abcdef 0123456789abcdef".to_vec()).is_err());
        let s = Secret::new(S.as_bytes().to_vec()).unwrap();
        let d = format!("{s:?}");
        assert!(!d.contains("0123"), "{d}");
        assert!(d.contains("redacted"));
    }

    #[test]
    fn secret_file_permissions() {
        let dir = std::env::temp_dir().join(format!("poseidon-auth-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("token");
        fs::write(&f, format!("{S}\n")).unwrap();
        for (mode, ok) in [(0o600, true), (0o400, true), (0o640, false), (0o604, false), (0o644, false)] {
            fs::set_permissions(&f, fs::Permissions::from_mode(mode)).unwrap();
            let r = load_secret_file(&f);
            assert_eq!(r.is_ok(), ok, "mode {mode:o}: {:?}", r.err());
            if let Err(e) = r {
                assert!(!e.to_string().contains("0123456789"), "error leaks secret");
            }
        }
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load_secret_file(&f).unwrap().matches(S.as_bytes()));
        // Symlink refused even if the target is fine.
        let l = dir.join("link");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        assert!(load_secret_file(&l).is_err());
        // Directory refused; too-short content refused without echoing it.
        assert!(load_secret_file(&dir).is_err());
        fs::write(&f, "tiny-secret\n").unwrap();
        let e = load_secret_file(&f).unwrap_err().to_string();
        assert!(!e.contains("tiny-secret"), "{e}");
        assert!(load_secret_file(&dir.join("absent")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
