// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Admin key files.
//!
//! * Public key file (lives on the gate host): one line `ed25519-pub <64 hex>`.
//! * Seed file (lives with the admin, never on the gate host): one line
//!   `ed25519-seed <64 hex>`, created mode 0600 and never overwritten.
//!
//! Key material is never printed. Errors name the path and the problem only.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use ed25519_dalek::{SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use crate::token::{hex_array, to_hex};

/// Public key file tag.
pub const PUB_TAG: &str = "ed25519-pub";
/// Seed file tag.
pub const SEED_TAG: &str = "ed25519-seed";

/// Directories this crate refuses to read or write key files in (project rule:
/// the production secret stores are off limits to tooling).
pub const FORBIDDEN_DIRS: [&str; 2] = ["/opt/galion/galion-light/secrets", "/root/galion-secrets"];

fn forbidden(path: &Path) -> bool {
    let abs = std::fs::canonicalize(path)
        .or_else(|_| {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(std::env::current_dir, std::fs::canonicalize)
                .map(|d| d.join(path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| path.to_path_buf());
    FORBIDDEN_DIRS.iter().any(|d| abs.starts_with(d))
        || FORBIDDEN_DIRS.iter().any(|d| path.starts_with(d))
}

fn read_tagged(path: &Path, tag: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    if forbidden(path) {
        return Err(format!("{}: refused: secret store path", path.display()));
    }
    let mut text = Zeroizing::new(String::new());
    std::fs::File::open(path)
        .and_then(|f| f.take(256).read_to_string(&mut text))
        .map_err(|e| format!("{}: {}", path.display(), e.kind()))?;
    let body = text
        .trim()
        .strip_prefix(tag)
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or_else(|| format!("{}: not a {tag} file", path.display()))?;
    hex_array::<32>(body)
        .map(Zeroizing::new)
        .ok_or_else(|| format!("{}: bad key encoding", path.display()))
}

/// Load the admin public key.
///
/// # Errors
/// Missing file, wrong format, a weak or invalid point, or a forbidden path.
pub fn load_public(path: &Path) -> Result<VerifyingKey, String> {
    let bytes = read_tagged(path, PUB_TAG)?;
    let key = VerifyingKey::from_bytes(&bytes)
        .map_err(|_| format!("{}: invalid ed25519 public key", path.display()))?;
    if key.is_weak() {
        return Err(format!("{}: weak ed25519 public key", path.display()));
    }
    Ok(key)
}

/// Load an admin seed (admin side, `mint` only).
///
/// # Errors
/// Missing file, wrong format or a forbidden path.
pub fn load_seed(path: &Path) -> Result<SigningKey, String> {
    let bytes = read_tagged(path, SEED_TAG)?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// 32 bytes from the OS RNG (`/dev/urandom`).
///
/// # Errors
/// When the RNG device cannot be read.
pub fn os_random<const N: usize>() -> Result<[u8; N], String> {
    let mut out = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut out))
        .map_err(|e| format!("/dev/urandom: {}", e.kind()))?;
    Ok(out)
}

fn write_new(path: &Path, text: &str, mode: u32) -> Result<(), String> {
    if forbidden(path) {
        return Err(format!("{}: refused: secret store path", path.display()));
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut f = opts
        .open(path)
        .map_err(|e| format!("{}: {}", path.display(), e.kind()))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("{}: {}", path.display(), e.kind()))
}

/// Generate a fresh admin key pair. Writes `seed_path` (0600) and `pub_path`
/// (0644). Refuses to overwrite either.
///
/// # Errors
/// RNG failure, an existing file, or a forbidden path.
pub fn generate(seed_path: &Path, pub_path: &Path) -> Result<VerifyingKey, String> {
    let seed = Zeroizing::new(os_random::<32>()?);
    let key = SigningKey::from_bytes(&seed);
    let public = key.verifying_key();
    let seed_line = Zeroizing::new(format!("{SEED_TAG} {}\n", to_hex(seed.as_ref())));
    write_new(seed_path, &seed_line, 0o600)?;
    write_new(
        pub_path,
        &format!("{PUB_TAG} {}\n", to_hex(public.as_bytes())),
        0o644,
    )?;
    Ok(public)
}

/// Short fingerprint of a public key (first 16 hex of SHA-256). Safe to print.
#[must_use]
pub fn fingerprint(key: &VerifyingKey) -> String {
    to_hex(&crate::token::plan_hash(key.as_bytes()))[..16].to_owned()
}
