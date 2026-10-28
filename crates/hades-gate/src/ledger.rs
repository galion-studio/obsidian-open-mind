// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Append-only replay ledger.
//!
//! One JSON object per line. Every write is a single `O_APPEND` write
//! followed by `fsync` (`File::sync_all`); a new ledger file also fsyncs its
//! directory. Check-and-append happens under an exclusive `flock` on the
//! ledger file, so two gate processes cannot both consume one nonce.
//!
//! The ledger fails closed: if it cannot be read, locked, written or parsed,
//! no approval is granted. It never stores a token string, only the nonce of
//! a consumed token and a short SHA-256 prefix of a refused one.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::token::{to_hex, Claims};

/// Why the ledger refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    /// The nonce was already consumed.
    Replayed,
    /// A line could not be parsed (line number, 1-based).
    Corrupt(usize),
    /// Open, lock, read, write or fsync failed.
    Io(String),
}

impl LedgerError {
    /// Human-readable reason.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Replayed => "replayed token: nonce already consumed".into(),
            Self::Corrupt(n) => format!("ledger corrupt at line {n}: failing closed"),
            Self::Io(e) => format!("ledger unavailable ({e}): failing closed"),
        }
    }
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for LedgerError {}

fn io(e: &std::io::Error) -> LedgerError {
    LedgerError::Io(e.kind().to_string())
}

/// The ledger file.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    /// Open (or create, mode 0600) the ledger at `path`.
    ///
    /// # Errors
    /// [`LedgerError::Io`] when the file or its directory cannot be created.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let path = path.into();
        let existed = path.exists();
        let file = open_append(&path).map_err(|e| io(&e))?;
        if !existed {
            file.sync_all().map_err(|e| io(&e))?;
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                sync_dir(dir).map_err(|e| io(&e))?;
            }
        }
        Ok(Self { path })
    }

    /// The ledger path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mark `claims.nonce` as used. Fails with [`LedgerError::Replayed`] if it
    /// already was. Atomic against other processes using the same file.
    ///
    /// # Errors
    /// [`LedgerError`]; on any error the token must be treated as refused.
    pub fn consume(&self, claims: &Claims, now: u64) -> Result<(), LedgerError> {
        let mut file = open_append(&self.path).map_err(|e| io(&e))?;
        file.lock().map_err(|e| io(&e))?;
        let nonce = to_hex(&claims.nonce);
        let seen = scan(&self.path)?;
        if seen.contains(&nonce) {
            return Err(LedgerError::Replayed);
        }
        let line = serde_json::json!({
            "ts": zagreb_time::from_unix(to_i64(now)),
            "event": "consumed",
            "nonce": nonce,
            "action": claims.action,
            "target": claims.target,
            "plan_sha256": to_hex(&claims.plan_hash),
            "iat": zagreb_time::from_unix(to_i64(claims.issued_at)),
            "exp": zagreb_time::from_unix(to_i64(claims.expires_at)),
        });
        append(&mut file, &line)
        // The lock is released when `file` drops.
    }

    /// Whether `nonce` was already consumed.
    ///
    /// # Errors
    /// [`LedgerError::Corrupt`] or [`LedgerError::Io`].
    pub fn is_consumed(&self, nonce: &[u8; 16]) -> Result<bool, LedgerError> {
        let hex = to_hex(nonce);
        Ok(scan(&self.path)?.contains(&hex))
    }

    /// Append an audit line for a decision that did not consume a token.
    ///
    /// `token` is hashed; only the first 16 hex digits of its SHA-256 are kept.
    ///
    /// # Errors
    /// [`LedgerError::Io`].
    pub fn note(
        &self,
        event: &str,
        action: &str,
        target: &str,
        reason: &str,
        token: &str,
        now: u64,
    ) -> Result<(), LedgerError> {
        let mut file = open_append(&self.path).map_err(|e| io(&e))?;
        file.lock().map_err(|e| io(&e))?;
        let token_id = if token.is_empty() {
            String::new()
        } else {
            to_hex(&crate::token::plan_hash(token.as_bytes()))[..16].to_owned()
        };
        let line = serde_json::json!({
            "ts": zagreb_time::from_unix(to_i64(now)),
            "event": event,
            "action": action,
            "target": target,
            "reason": reason,
            "token_sha256_16": token_id,
        });
        append(&mut file, &line)
    }
}

fn to_i64(secs: u64) -> i64 {
    i64::try_from(secs).unwrap_or(i64::MAX)
}

fn open_append(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn append(file: &mut File, line: &serde_json::Value) -> Result<(), LedgerError> {
    let mut text = line.to_string();
    text.push('\n');
    file.write_all(text.as_bytes()).map_err(|e| io(&e))?;
    file.sync_all().map_err(|e| io(&e))
}

/// Every consumed nonce. Any unparsable line fails closed.
fn scan(path: &Path) -> Result<Vec<String>, LedgerError> {
    let mut text = String::new();
    File::open(path)
        .and_then(|mut f| f.read_to_string(&mut text))
        .map_err(|e| io(&e))?;
    let mut nonces = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| LedgerError::Corrupt(i + 1))?;
        let event = value
            .get("event")
            .and_then(serde_json::Value::as_str)
            .ok_or(LedgerError::Corrupt(i + 1))?;
        if event == "consumed" {
            let nonce = value
                .get("nonce")
                .and_then(serde_json::Value::as_str)
                .filter(|n| n.len() == 32)
                .ok_or(LedgerError::Corrupt(i + 1))?;
            nonces.push(nonce.to_owned());
        }
    }
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(LedgerError::Corrupt(text.lines().count()));
    }
    Ok(nonces)
}
