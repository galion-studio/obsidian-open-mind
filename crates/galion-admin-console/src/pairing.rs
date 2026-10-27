// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Device pairing store (state dir, 0700; files 0600; one `flock` on
//! `state.lock` guards every read-modify-write, so the `issue-code` CLI and
//! the running server can share it).
//!
//! * `codes`   — `<sha256(code)> <expires_unix>` (the code itself is never stored)
//! * `devices` — `<sha256(token)> <node> <paired_unix> <revoked 0|1> <label>`
//! * `fails`   — unix seconds of failed pair attempts (lockout window)

use galion_common::{base64, sha256};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// Crockford base32 (no I, L, O, U): easy to read out loud and type on a phone.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const CODE_LEN: usize = 8;
pub const DEFAULT_TTL: i64 = 300;
pub const MAX_FAILS: usize = 5;
pub const FAIL_WINDOW: i64 = 900;
pub const LABEL_MAX: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub node: String,
    pub paired: i64,
    pub revoked: bool,
    pub label: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PairError {
    /// Too many failures in the window; even a right code is refused.
    Locked,
    /// Unknown, expired or already used code.
    Bad,
}

pub struct Store {
    dir: PathBuf,
}

/// Uppercase, drop separators, map look-alikes (O->0, I/L->1).
pub fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, '-' | ' ' | '\t'))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            u => u,
        })
        .collect()
}

/// Labels are display-only: `[A-Za-z0-9 _-]`, at most 32 chars.
pub fn clean_label(s: &str) -> String {
    let l: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .take(LABEL_MAX)
        .collect();
    let l = l.trim().to_string();
    if l.is_empty() {
        "phone".into()
    } else {
        l
    }
}

/// Node id shown everywhere instead of any device identifier.
pub fn node_of(token: &str) -> String {
    let h = sha256::hex(format!("galion-admin-console/node/v1\n{token}").as_bytes());
    format!("node-{}", &h[..8])
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl Store {
    /// Open (create) the state dir. A group/world-writable dir is refused.
    pub fn open(dir: &Path) -> io::Result<Store> {
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir)?.permissions().mode();
            if mode & 0o022 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "state dir is group/world-writable",
                ));
            }
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Store { dir: dir.into() })
    }

    pub fn audit_path(&self) -> PathBuf {
        self.dir.join("audit.jsonl")
    }

    fn lock(&self) -> io::Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join("state.lock"))?;
        f.lock()?;
        Ok(f)
    }

    fn read(&self, name: &str) -> io::Result<Vec<String>> {
        match fs::read_to_string(self.dir.join(name)) {
            Ok(s) => Ok(s
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    fn write(&self, name: &str, lines: &[String]) -> io::Result<()> {
        let tmp = self.dir.join(format!(".{name}.tmp"));
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        {
            use std::io::Write;
            let mut f = File::create(&tmp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                f.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(tmp, self.dir.join(name))
    }

    /// New one-time code (`XXXX-XXXX`), valid `ttl` seconds. Expired codes
    /// are pruned on the way. Only its hash is stored.
    pub fn issue_code(&self, ttl: i64, now: i64) -> io::Result<String> {
        let raw = crate::random_bytes(CODE_LEN)?;
        let code: String = raw
            .iter()
            .map(|b| ALPHABET[(*b & 31) as usize] as char)
            .collect();
        let _g = self.lock()?;
        let mut codes: Vec<String> = self
            .read("codes")?
            .into_iter()
            .filter(|l| expiry(l).is_some_and(|e| e > now))
            .collect();
        codes.push(format!(
            "{} {}",
            sha256::hex(code.as_bytes()),
            now + ttl.max(1)
        ));
        self.write("codes", &codes)?;
        Ok(format!("{}-{}", &code[..4], &code[4..]))
    }

    /// Spend a code. On success returns the new device token (goes only into
    /// the cookie) and its node id.
    pub fn pair(
        &self,
        code: &str,
        label: &str,
        now: i64,
    ) -> io::Result<Result<(String, String), PairError>> {
        let _g = self.lock()?;
        let fails: Vec<String> = self
            .read("fails")?
            .into_iter()
            .filter(|l| l.trim().parse::<i64>().is_ok_and(|t| t > now - FAIL_WINDOW))
            .collect();
        if fails.len() >= MAX_FAILS {
            return Ok(Err(PairError::Locked));
        }
        let want = sha256::hex(normalize(code).as_bytes());
        let codes = self.read("codes")?;
        let mut hit = false;
        let mut keep = Vec::new();
        for l in codes {
            let (h, e) = (l.split(' ').next().unwrap_or(""), expiry(&l).unwrap_or(0));
            if !hit && e > now && ct_eq(h, &want) {
                hit = true; // single use: dropped
            } else if e > now {
                keep.push(l);
            }
        }
        if !hit {
            let mut fails = fails;
            fails.push(now.to_string());
            self.write("fails", &fails)?;
            return Ok(Err(PairError::Bad));
        }
        self.write("codes", &keep)?;
        let token = base64::encode_url(&crate::random_bytes(32)?);
        let node = node_of(&token);
        let mut devices = self.read("devices")?;
        devices.push(format!(
            "{} {node} {now} 0 {}",
            sha256::hex(token.as_bytes()),
            clean_label(label)
        ));
        self.write("devices", &devices)?;
        Ok(Ok((token, node)))
    }

    /// The live (not revoked) device for a cookie token.
    pub fn device(&self, token: &str) -> io::Result<Option<Device>> {
        if token.is_empty() || token.len() > 128 {
            return Ok(None);
        }
        let want = sha256::hex(token.as_bytes());
        Ok(self
            .read("devices")?
            .iter()
            .filter(|l| ct_eq(l.split(' ').next().unwrap_or(""), &want))
            .filter_map(|l| parse_device(l))
            .find(|d| !d.revoked))
    }

    pub fn list(&self) -> io::Result<Vec<Device>> {
        Ok(self
            .read("devices")?
            .iter()
            .filter_map(|l| parse_device(l))
            .collect())
    }

    /// Revoke by node id. True when something changed.
    pub fn revoke(&self, node: &str) -> io::Result<bool> {
        let _g = self.lock()?;
        let mut changed = false;
        let devices: Vec<String> = self
            .read("devices")?
            .into_iter()
            .map(|l| {
                let p: Vec<&str> = l.splitn(5, ' ').collect();
                if p.len() == 5 && p[1] == node && p[3] == "0" {
                    changed = true;
                    format!("{} {} {} 1 {}", p[0], p[1], p[2], p[4])
                } else {
                    l
                }
            })
            .collect();
        if changed {
            self.write("devices", &devices)?;
        }
        Ok(changed)
    }
}

fn expiry(line: &str) -> Option<i64> {
    line.split(' ').nth(1)?.parse().ok()
}

fn parse_device(l: &str) -> Option<Device> {
    let p: Vec<&str> = l.splitn(5, ' ').collect();
    if p.len() != 5 {
        return None;
    }
    Some(Device {
        node: p[1].into(),
        paired: p[2].parse().ok()?,
        revoked: p[3] == "1",
        label: p[4].into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> (Store, PathBuf) {
        let d = std::env::temp_dir().join(format!("gac-pair-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        (Store::open(&d).unwrap(), d)
    }

    #[test]
    fn code_is_single_use_and_hashed() {
        let (s, d) = store("single");
        let code = s.issue_code(DEFAULT_TTL, 1000).unwrap();
        assert_eq!(code.len(), 9);
        let on_disk = fs::read_to_string(d.join("codes")).unwrap();
        assert!(!on_disk.contains(&code.replace('-', "")));
        let (tok, node) = s
            .pair(&code.to_lowercase(), "Redmi <b>", 1001)
            .unwrap()
            .unwrap();
        assert!(node.starts_with("node-") && node.len() == 13);
        assert_eq!(s.pair(&code, "x", 1002).unwrap(), Err(PairError::Bad));
        let dev = s.device(&tok).unwrap().unwrap();
        assert_eq!(dev.label, "Redmi b");
        assert!(!fs::read_to_string(d.join("devices"))
            .unwrap()
            .contains(&tok));
    }

    #[test]
    fn code_expires() {
        let (s, _) = store("exp");
        let code = s.issue_code(60, 1000).unwrap();
        assert_eq!(s.pair(&code, "", 1060).unwrap(), Err(PairError::Bad));
    }

    #[test]
    fn lockout_after_fails_even_with_right_code() {
        let (s, _) = store("lock");
        let code = s.issue_code(DEFAULT_TTL, 1000).unwrap();
        for i in 0..MAX_FAILS {
            assert_eq!(
                s.pair("AAAA-AAAA", "", 1001 + i as i64).unwrap(),
                Err(PairError::Bad)
            );
        }
        assert_eq!(s.pair(&code, "", 1010).unwrap(), Err(PairError::Locked));
        // window passes: the code (still valid at 1000+300) works again
        let s2 = s.pair(&code, "", 1001 + FAIL_WINDOW + 5).unwrap();
        assert_eq!(s2, Err(PairError::Bad), "code expired by then (ttl 300)");
        let c2 = s.issue_code(DEFAULT_TTL, 2000).unwrap();
        assert!(s.pair(&c2, "", 2001).unwrap().is_ok());
    }

    #[test]
    fn revoke_kills_token() {
        let (s, _) = store("rev");
        let code = s.issue_code(DEFAULT_TTL, 1000).unwrap();
        let (tok, node) = s.pair(&code, "p", 1001).unwrap().unwrap();
        assert!(s.revoke(&node).unwrap());
        assert!(!s.revoke(&node).unwrap());
        assert_eq!(s.device(&tok).unwrap(), None);
        assert!(s.list().unwrap()[0].revoked);
    }

    #[test]
    fn normalize_lookalikes() {
        assert_eq!(normalize("ab0o-1il "), "AB00111");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_world_writable_dir() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("gac-ww-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Store::open(&d).is_err());
    }
}
