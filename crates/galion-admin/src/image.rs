// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Galion OS gold image: manifest, Ed25519 signature and verification.
//!
//! Format spec: `docs/galion-os/gold-image-format.md`. In short, a gold image is a directory
//! holding payload files plus two top-level metadata files:
//!
//! * `galion-image.manifest`: one canonical ASCII text (header, build info, one `file` line per
//!   payload file with mode, size and SHA-256, then `end`);
//! * `galion-image.sig`: Ed25519 signature over [`SIGN_DOMAIN`] followed by the exact manifest
//!   bytes, in the hades-gate key format (`ed25519-pub <64 hex>` public key files,
//!   `ed25519-seed <64 hex>` seed files, lower-case hex, strict verification, weak keys refused).
//!
//! Verification checks the signature FIRST and only then parses the manifest and reads payload
//! files. Every payload file is opened with `O_NOFOLLOW`, read once (streamed, bounded by its
//! listed size + 1) and hashed; unlisted files, symlinks and special files are failures.
//! Nothing is ever written to the image.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Manifest file name (top level of the image directory).
pub const MANIFEST_NAME: &str = "galion-image.manifest";
/// Signature file name (top level of the image directory).
pub const SIG_NAME: &str = "galion-image.sig";
/// First line of every v1 manifest.
pub const MAGIC: &str = "galion-gold-image v1";
/// First line of every v1 signature file.
pub const SIG_MAGIC: &str = "galion-gold-image-sig v1";
/// Domain separation: a hades-gate approval signature (domain
/// `galion-hades-gate/approval/v1\n`) can never verify as an image and vice versa.
pub const SIGN_DOMAIN: &[u8] = b"galion-os/gold-image/v1\n";
/// Public key file tag (hades-gate format).
pub const PUB_TAG: &str = "ed25519-pub";
/// Seed file tag (hades-gate format).
pub const SEED_TAG: &str = "ed25519-seed";
/// Signature line tag.
pub const SIG_TAG: &str = "ed25519-sig";
/// Largest accepted manifest (bytes).
pub const MAX_MANIFEST: u64 = 8 << 20;
/// Largest accepted signature file (bytes).
pub const MAX_SIG: u64 = 1024;
/// Most payload files in one image.
pub const MAX_FILES: usize = 100_000;
/// Longest payload path (bytes).
pub const MAX_PATH: usize = 255;
/// Deepest directory nesting walked.
pub const MAX_DEPTH: usize = 32;
/// Header keys, in their fixed canonical order.
pub const HEADER_KEYS: [&str; 6] = [
    "name",
    "version",
    "arch",
    "build.commit",
    "build.rustc",
    "build.time",
];
/// Same refusal list as hades-gate `keys::FORBIDDEN_DIRS` (project rule: tooling never reads
/// the production secret stores).
pub const FORBIDDEN_DIRS: [&str; 2] = ["/opt/galion/galion-light/secrets", "/root/galion-secrets"];

/// One payload file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub mode: u32,
    pub size: u64,
    pub sha256: [u8; 32],
}

/// A parsed (or built) manifest. `header` holds the values of [`HEADER_KEYS`] in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub header: [String; 6],
    pub files: Vec<Entry>,
}

/// Lower-case hex.
pub fn to_hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[usize::from(b >> 4)] as char);
        s.push(D[usize::from(b & 15)] as char);
    }
    s
}

/// Strict lower-case hex decode of exactly `N` bytes (same rule as hades-gate `hex_array`).
pub fn hex_array<const N: usize>(t: &str) -> Option<[u8; N]> {
    fn nib(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let r = t.as_bytes();
    if r.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, p) in r.chunks(2).enumerate() {
        out[i] = (nib(p[0])? << 4) | nib(p[1])?;
    }
    Some(out)
}

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Short key fingerprint: first 16 hex of SHA-256(public key) (same as hades-gate). Safe to print.
pub fn fingerprint(key: &VerifyingKey) -> String {
    to_hex(&sha256(key.as_bytes()))[..16].to_owned()
}

fn forbidden(path: &Path) -> bool {
    let abs = std::fs::canonicalize(path)
        .or_else(|_| {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(std::env::current_dir, std::fs::canonicalize)
                .map(|d| d.join(path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| path.to_path_buf());
    FORBIDDEN_DIRS
        .iter()
        .any(|d| abs.starts_with(d) || path.starts_with(d))
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
        .and_then(|r| r.strip_prefix(' '))
        .ok_or_else(|| format!("{}: not a {tag} file", path.display()))?;
    hex_array::<32>(body)
        .map(Zeroizing::new)
        .ok_or_else(|| format!("{}: bad key encoding", path.display()))
}

/// Load a public key file (`ed25519-pub <64 hex>`, hades-gate format). Weak and invalid points
/// are refused. Key material is never printed.
pub fn load_public(path: &Path) -> Result<VerifyingKey, String> {
    let b = read_tagged(path, PUB_TAG)?;
    let k = VerifyingKey::from_bytes(&b)
        .map_err(|_| format!("{}: invalid ed25519 public key", path.display()))?;
    if k.is_weak() {
        return Err(format!("{}: weak ed25519 public key", path.display()));
    }
    Ok(k)
}

/// Load a seed file (`ed25519-seed <64 hex>`, hades-gate format). Builder side only.
pub fn load_seed(path: &Path) -> Result<SigningKey, String> {
    let b = read_tagged(path, SEED_TAG)?;
    Ok(SigningKey::from_bytes(&b))
}

fn printable(v: &str, max: usize) -> bool {
    !v.is_empty()
        && v.len() <= max
        && v.bytes().all(|c| (0x20..=0x7e).contains(&c))
        && !v.starts_with(' ')
        && !v.ends_with(' ')
}

fn charset(v: &str, max: usize, extra: &[u8]) -> bool {
    !v.is_empty()
        && v.len() <= max
        && v.bytes()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

/// `YYYY-MM-DDTHH:MM:SS+HH:MM` with an explicit offset (project rule: never `Z`).
pub fn valid_time(t: &str) -> bool {
    let b = t.as_bytes();
    if b.len() != 25 {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 23, 24];
    let num = |i: usize, n: usize| -> u32 {
        t[i..i + n].parse().unwrap_or(u32::MAX)
    };
    digits.iter().all(|&i| b[i].is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && (b[19] == b'+' || b[19] == b'-')
        && b[22] == b':'
        && (1..=12).contains(&num(5, 2))
        && (1..=31).contains(&num(8, 2))
        && num(11, 2) < 24
        && num(14, 2) < 60
        && num(17, 2) < 61
        && num(20, 2) <= 14
        && num(23, 2) < 60
}

/// Check one header value.
pub fn check_header(key: &str, v: &str) -> Result<(), String> {
    let ok = match key {
        "name" => charset(v, 64, b"._-") && v.as_bytes()[0].is_ascii_alphanumeric(),
        "version" => charset(v, 32, b".+-"),
        "arch" => charset(v, 16, b"_"),
        "build.commit" => {
            (7..=64).contains(&v.len())
                && v.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        }
        "build.rustc" => printable(v, 128),
        "build.time" => valid_time(v),
        _ => return Err(format!("unknown header key {key:?}")),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("header {key}: invalid value"))
    }
}

/// Check one payload path: relative, `/`-separated components of `A-Z a-z 0-9 . _ + @ -`,
/// no empty, `.` or `..` component, at most [`MAX_PATH`] bytes, not a metadata file name.
pub fn check_path(p: &str) -> Result<(), String> {
    if p.is_empty() || p.len() > MAX_PATH {
        return Err("path empty or too long".into());
    }
    if p == MANIFEST_NAME || p == SIG_NAME {
        return Err(format!("path {p:?} is reserved"));
    }
    for c in p.split('/') {
        if c.is_empty() || c == "." || c == ".." {
            return Err(format!("path {p:?}: empty, . or .. component"));
        }
        if !c
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+@-".contains(&b))
        {
            return Err(format!("path {p:?}: character outside A-Z a-z 0-9 . _ + @ -"));
        }
    }
    Ok(())
}

impl Manifest {
    pub fn get(&self, key: &str) -> &str {
        HEADER_KEYS
            .iter()
            .position(|k| *k == key)
            .map(|i| self.header[i].as_str())
            .unwrap_or("")
    }

    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|e| e.size).sum()
    }

    /// Canonical text. `parse(to_text()) == self` and `to_text(parse(t)) == t` for every
    /// accepted `t`.
    pub fn to_text(&self) -> String {
        let mut s = format!("{MAGIC}\n");
        for (k, v) in HEADER_KEYS.iter().zip(&self.header) {
            s.push_str(&format!("{k} {v}\n"));
        }
        s.push_str(&format!(
            "files {}\nbytes {}\n",
            self.files.len(),
            self.total_bytes()
        ));
        for e in &self.files {
            s.push_str(&format!(
                "file {:04o} {} {} {}\n",
                e.mode,
                e.size,
                to_hex(&e.sha256),
                e.path
            ));
        }
        s.push_str("end\n");
        s
    }

    /// Structural checks shared by `parse` and the builder.
    pub fn validate(&self) -> Result<(), String> {
        for (k, v) in HEADER_KEYS.iter().zip(&self.header) {
            check_header(k, v)?;
        }
        if self.files.is_empty() {
            return Err("image lists no files".into());
        }
        if self.files.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} files"));
        }
        let mut dirs = BTreeSet::new();
        for (i, e) in self.files.iter().enumerate() {
            check_path(&e.path)?;
            if e.mode & !0o777 != 0 {
                return Err(format!(
                    "{}: mode {:04o} has setuid/setgid/sticky bits",
                    e.path, e.mode
                ));
            }
            if i > 0 && self.files[i - 1].path.as_bytes() >= e.path.as_bytes() {
                return Err(format!(
                    "{}: files not in strict byte order (or duplicate)",
                    e.path
                ));
            }
            let mut acc = String::new();
            for c in e.path.split('/').collect::<Vec<_>>().split_last().unwrap().1 {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(c);
                dirs.insert(acc.clone());
            }
        }
        if let Some(e) = self.files.iter().find(|e| dirs.contains(&e.path)) {
            return Err(format!("{}: listed both as a file and as a directory", e.path));
        }
        self.files
            .iter()
            .try_fold(0u64, |a, e| a.checked_add(e.size))
            .ok_or("total size overflows")?;
        Ok(())
    }

    /// Strict parse of manifest bytes. Anything not byte-identical to the canonical form is
    /// refused (CRLF, leading zeros, upper-case hex, extra spaces, missing `end`, ...).
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "manifest is not UTF-8")?;
        if !text.is_ascii() {
            return Err("manifest is not ASCII".into());
        }
        let mut lines = text.split_inclusive('\n');
        let mut next = |what: &str| -> Result<&str, String> {
            let l = lines
                .next()
                .ok_or_else(|| format!("manifest truncated before {what}"))?;
            l.strip_suffix('\n')
                .ok_or_else(|| format!("manifest truncated in {what} (no newline)"))
        };
        if next("magic")? != MAGIC {
            return Err(format!("not a {MAGIC:?} manifest"));
        }
        let mut header: [String; 6] = Default::default();
        for (i, k) in HEADER_KEYS.iter().enumerate() {
            let l = next(k)?;
            let v = l
                .strip_prefix(k)
                .and_then(|r| r.strip_prefix(' '))
                .ok_or_else(|| format!("expected header {k:?}"))?;
            header[i] = v.to_string();
        }
        let count = |l: &str, k: &str| -> Result<u64, String> {
            l.strip_prefix(k)
                .and_then(|r| r.strip_prefix(' '))
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| format!("expected `{k} N`"))
        };
        let nfiles = count(next("files")?, "files")?;
        let nbytes = count(next("bytes")?, "bytes")?;
        if nfiles as usize > MAX_FILES {
            return Err(format!("more than {MAX_FILES} files"));
        }
        let mut files = Vec::with_capacity(nfiles as usize);
        for n in 0..nfiles {
            let l = next("file line")?;
            let mut it = l.splitn(5, ' ');
            let (tag, mode, size, sha, path) =
                (it.next(), it.next(), it.next(), it.next(), it.next());
            let bad = || format!("file line {} malformed", n + 1);
            if tag != Some("file") {
                return Err(bad());
            }
            let mode = mode
                .filter(|m| m.len() == 4 && m.bytes().all(|c| (b'0'..=b'7').contains(&c)))
                .and_then(|m| u32::from_str_radix(m, 8).ok())
                .ok_or_else(bad)?;
            let size = size.and_then(|s| s.parse::<u64>().ok()).ok_or_else(bad)?;
            let sha256 = sha.and_then(hex_array::<32>).ok_or_else(bad)?;
            let path = path.ok_or_else(bad)?.to_string();
            files.push(Entry {
                path,
                mode,
                size,
                sha256,
            });
        }
        if next("end")? != "end" {
            return Err("expected `end` after the file lines".into());
        }
        if lines.next().is_some() {
            return Err("data after `end`".into());
        }
        let m = Manifest { header, files };
        m.validate()?;
        if m.total_bytes() != nbytes {
            return Err(format!(
                "bytes {nbytes} does not match the file sizes ({})",
                m.total_bytes()
            ));
        }
        if m.to_text().as_bytes() != bytes {
            return Err("manifest is not in canonical form".into());
        }
        Ok(m)
    }
}

/// Signature file text for `manifest` (canonical bytes) signed with `key`.
pub fn sign(manifest: &[u8], key: &SigningKey) -> String {
    let mut msg = SIGN_DOMAIN.to_vec();
    msg.extend_from_slice(manifest);
    let sig = key.sign(&msg);
    format!(
        "{SIG_MAGIC}\nkey {}\n{SIG_TAG} {}\n",
        fingerprint(&key.verifying_key()),
        to_hex(&sig.to_bytes())
    )
}

/// Parsed signature file: (key fingerprint, signature).
pub fn parse_sig(bytes: &[u8]) -> Result<(String, Signature), String> {
    let t = std::str::from_utf8(bytes).map_err(|_| "signature file is not UTF-8")?;
    let mut l = t.split('\n');
    let (magic, key, sig, end, extra) = (l.next(), l.next(), l.next(), l.next(), l.next());
    if magic != Some(SIG_MAGIC) || end != Some("") || extra.is_some() {
        return Err("signature file malformed".into());
    }
    let fp = key
        .and_then(|k| k.strip_prefix("key "))
        .filter(|f| f.len() == 16 && f.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')))
        .ok_or("signature file: bad key line")?;
    let raw = sig
        .and_then(|s| s.strip_prefix(SIG_TAG))
        .and_then(|s| s.strip_prefix(' '))
        .and_then(hex_array::<64>)
        .ok_or("signature file: bad signature line")?;
    Ok((fp.to_string(), Signature::from_bytes(&raw)))
}

/// Builder side: walk `dir`, hash every regular file and return a validated manifest.
/// Symlinks and special files are refused (a gold image carries regular files only).
pub fn build(dir: &Path, header: [String; 6]) -> Result<Manifest, String> {
    let walk = walk(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if let Some((p, k)) = walk.special.first() {
        return Err(format!("{p}: {k} (only regular files are allowed)"));
    }
    let mut files = Vec::new();
    for p in walk.files {
        let (mode, size, sha256) =
            hash_file(&dir.join(&p), u64::MAX).map_err(|e| format!("{p}: {e}"))?;
        files.push(Entry {
            path: p,
            mode,
            size,
            sha256,
        });
    }
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let m = Manifest { header, files };
    m.validate()?;
    Ok(m)
}

/// Result of walking an image directory (metadata files excluded).
#[derive(Debug, Default)]
pub struct Walk {
    pub files: Vec<String>,
    pub dirs: Vec<String>,
    /// (path, kind) of symlinks, fifos, sockets, devices and unreadable names.
    pub special: Vec<(String, &'static str)>,
}

/// Walk `root` without following symlinks.
pub fn walk(root: &Path) -> io::Result<Walk> {
    let mut w = Walk::default();
    let mut stack = vec![(PathBuf::new(), 0usize)];
    while let Some((rel, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(root.join(&rel))?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let Some(name) = name.to_str().map(str::to_string) else {
                w.special
                    .push((format!("{}/<non-UTF-8 name>", rel.display()), "bad name"));
                continue;
            };
            let relp = if rel.as_os_str().is_empty() {
                name.clone()
            } else {
                format!("{}/{name}", rel.display())
            };
            if depth == 0 && (name == MANIFEST_NAME || name == SIG_NAME) {
                continue;
            }
            let ft = e.file_type()?;
            if ft.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    w.special.push((relp, "too deep"));
                    continue;
                }
                w.dirs.push(relp.clone());
                stack.push((PathBuf::from(relp), depth + 1));
            } else if ft.is_file() {
                w.files.push(relp);
            } else {
                let kind = if ft.is_symlink() {
                    "symlink"
                } else {
                    "special file"
                };
                w.special.push((relp, kind));
            }
            if w.files.len() + w.dirs.len() > 4 * MAX_FILES {
                return Err(io::Error::other("too many entries"));
            }
        }
    }
    Ok(w)
}

/// Open without following a final symlink, check it is a regular file, stream-hash at most
/// `limit` bytes. Returns (permission bits, bytes read, sha256). Reading stops at `limit`,
/// so a file that grew is reported as `limit` bytes (callers pass listed size + 1).
pub fn hash_file(p: &Path, limit: u64) -> io::Result<(u32, u64, [u8; 32])> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(p)?;
    let md = f.metadata()?;
    if !md.file_type().is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut n = 0u64;
    let mut r = f.take(limit);
    loop {
        let k = r.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((md.mode() & 0o7777, n, h.finalize().into()))
}

/// Why verification failed before the payload was looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// Signature file or manifest missing / not a regular file / too large.
    Metadata(String),
    /// Signature made by another key than the one given.
    WrongKey { signed_by: String, expected: String },
    /// Signature does not verify (tampered manifest, bad signature).
    BadSignature,
    /// Signed but malformed manifest.
    Manifest(String),
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejection::Metadata(m) => write!(f, "metadata: {m}"),
            Rejection::WrongKey {
                signed_by,
                expected,
            } => write!(
                f,
                "wrong key: image signed by key {signed_by}, verifying key is {expected}"
            ),
            Rejection::BadSignature => write!(
                f,
                "bad signature: manifest or signature changed after signing"
            ),
            Rejection::Manifest(m) => write!(f, "manifest: {m}"),
        }
    }
}

/// One payload problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing(String),
    Truncated { path: String, want: u64, got: u64 },
    Grown { path: String, want: u64 },
    Hash(String),
    Mode { path: String, want: u32, got: u32 },
    NotRegular { path: String, kind: String },
    Unlisted(String),
    UnlistedDir(String),
    Unreadable { path: String, err: String },
}

impl Problem {
    pub fn code(&self) -> &'static str {
        match self {
            Problem::Missing(_) => "missing",
            Problem::Truncated { .. } => "truncated",
            Problem::Grown { .. } => "grown",
            Problem::Hash(_) => "sha256",
            Problem::Mode { .. } => "mode",
            Problem::NotRegular { .. } => "not-regular",
            Problem::Unlisted(_) => "unlisted",
            Problem::UnlistedDir(_) => "unlisted-dir",
            Problem::Unreadable { .. } => "unreadable",
        }
    }
    pub fn path(&self) -> &str {
        match self {
            Problem::Missing(p)
            | Problem::Hash(p)
            | Problem::Unlisted(p)
            | Problem::UnlistedDir(p) => p,
            Problem::Truncated { path, .. }
            | Problem::Grown { path, .. }
            | Problem::Mode { path, .. }
            | Problem::NotRegular { path, .. }
            | Problem::Unreadable { path, .. } => path,
        }
    }
    pub fn detail(&self) -> String {
        match self {
            Problem::Missing(_) => "listed but not present".into(),
            Problem::Truncated { want, got, .. } => format!("size {got}, manifest says {want}"),
            Problem::Grown { want, .. } => format!("larger than the listed {want} bytes"),
            Problem::Hash(_) => "content differs from the signed SHA-256".into(),
            Problem::Mode { want, got, .. } => format!("mode {got:04o}, manifest says {want:04o}"),
            Problem::NotRegular { kind, .. } => kind.clone(),
            Problem::Unlisted(_) => "file not in the manifest".into(),
            Problem::UnlistedDir(_) => "directory holds no listed file".into(),
            Problem::Unreadable { err, .. } => err.clone(),
        }
    }
}

/// Outcome of a verification that got past the signature.
#[derive(Debug, Clone)]
pub struct Report {
    pub manifest: Manifest,
    pub manifest_sha256: [u8; 32],
    pub key: String,
    pub problems: Vec<Problem>,
    pub bytes_hashed: u64,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

fn read_meta(p: &Path, max: u64) -> Result<Vec<u8>, Rejection> {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(p)
        .map_err(|e| Rejection::Metadata(format!("{name}: {}", e.kind())))?;
    let md = f
        .metadata()
        .map_err(|e| Rejection::Metadata(format!("{name}: {}", e.kind())))?;
    if !md.file_type().is_file() {
        return Err(Rejection::Metadata(format!("{name}: not a regular file")));
    }
    let mut v = Vec::new();
    f.take(max + 1)
        .read_to_end(&mut v)
        .map_err(|e| Rejection::Metadata(format!("{name}: {}", e.kind())))?;
    if v.len() as u64 > max {
        return Err(Rejection::Metadata(format!("{name}: larger than {max} bytes")));
    }
    Ok(v)
}

/// Verify the image in `dir` against `key`.
///
/// Order: read signature + manifest (bounded, no symlinks) → key fingerprint → strict Ed25519
/// verification of domain || manifest → strict manifest parse → walk the directory (unlisted,
/// symlink, special files) → open, size-check, mode-check and hash every listed file.
pub fn verify(dir: &Path, key: &VerifyingKey) -> Result<Report, Rejection> {
    let sig_bytes = read_meta(&dir.join(SIG_NAME), MAX_SIG)?;
    let man = read_meta(&dir.join(MANIFEST_NAME), MAX_MANIFEST)?;
    let (fp, sig) = parse_sig(&sig_bytes).map_err(Rejection::Metadata)?;
    let expected = fingerprint(key);
    if fp != expected {
        return Err(Rejection::WrongKey {
            signed_by: fp,
            expected,
        });
    }
    let mut msg = SIGN_DOMAIN.to_vec();
    msg.extend_from_slice(&man);
    key.verify_strict(&msg, &sig)
        .map_err(|_| Rejection::BadSignature)?;
    let manifest = Manifest::parse(&man).map_err(Rejection::Manifest)?;

    let mut problems = Vec::new();
    let listed: BTreeMap<&str, &Entry> =
        manifest.files.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut want_dirs = BTreeSet::new();
    for e in &manifest.files {
        let mut acc = String::new();
        let parts: Vec<&str> = e.path.split('/').collect();
        for c in &parts[..parts.len() - 1] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(c);
            want_dirs.insert(acc.clone());
        }
    }
    match walk(dir) {
        Ok(w) => {
            for (p, k) in w.special {
                problems.push(Problem::NotRegular {
                    path: p,
                    kind: k.to_string(),
                });
            }
            for f in w.files {
                if !listed.contains_key(f.as_str()) {
                    problems.push(Problem::Unlisted(f));
                }
            }
            for d in w.dirs {
                if !want_dirs.contains(&d) {
                    problems.push(Problem::UnlistedDir(d));
                }
            }
        }
        Err(e) => problems.push(Problem::Unreadable {
            path: ".".into(),
            err: e.to_string(),
        }),
    }
    let mut bytes_hashed = 0;
    for e in &manifest.files {
        let p = dir.join(&e.path);
        match hash_file(&p, e.size.saturating_add(1)) {
            Ok((mode, n, h)) => {
                bytes_hashed += n;
                if n < e.size {
                    problems.push(Problem::Truncated {
                        path: e.path.clone(),
                        want: e.size,
                        got: n,
                    });
                } else if n > e.size {
                    problems.push(Problem::Grown {
                        path: e.path.clone(),
                        want: e.size,
                    });
                } else if h != e.sha256 {
                    problems.push(Problem::Hash(e.path.clone()));
                }
                if mode != e.mode {
                    problems.push(Problem::Mode {
                        path: e.path.clone(),
                        want: e.mode,
                        got: mode,
                    });
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                problems.push(Problem::Missing(e.path.clone()))
            }
            Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
                // Already reported by the walk as a symlink; make sure it is listed once.
                if !problems.iter().any(|p| p.path() == e.path) {
                    problems.push(Problem::NotRegular {
                        path: e.path.clone(),
                        kind: "symlink".into(),
                    });
                }
            }
            Err(err) => {
                if !problems.iter().any(|p| p.path() == e.path) {
                    problems.push(Problem::Unreadable {
                        path: e.path.clone(),
                        err: err.to_string(),
                    })
                }
            }
        }
    }
    Ok(Report {
        manifest_sha256: sha256(&man),
        manifest,
        key: expected,
        problems,
        bytes_hashed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn header() -> [String; 6] {
        [
            "galion-os-gold".into(),
            "0.1.0".into(),
            "aarch64".into(),
            "bf33cba".into(),
            "rustc 1.99.1 (58cf8f9c3 2026-10-13)".into(),
            "2026-10-21T18:00:00+02:00".into(),
        ]
    }

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "gimg-unit-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    fn put(dir: &Path, rel: &str, data: &[u8], mode: u32) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A small signed image: returns the verifying key.
    fn image(dir: &Path, k: &SigningKey) -> VerifyingKey {
        put(dir, "bin/galion-admin", &vec![0x7f; 70_000], 0o755);
        put(dir, "bin/galion-box", b"\x7fELF box", 0o755);
        put(dir, "etc/galion/os.toml", b"[os]\nname = \"galion\"\n", 0o644);
        put(dir, "README", b"gold image\n", 0o644);
        let m = build(dir, header()).unwrap();
        let text = m.to_text();
        std::fs::write(dir.join(MANIFEST_NAME), &text).unwrap();
        std::fs::write(dir.join(SIG_NAME), sign(text.as_bytes(), k)).unwrap();
        k.verifying_key()
    }

    #[test]
    fn roundtrip_and_ok() {
        let t = tmp("ok");
        let vk = image(&t.0, &key(7));
        let r = verify(&t.0, &vk).unwrap();
        assert!(r.ok(), "{:?}", r.problems);
        assert_eq!(r.manifest.files.len(), 4);
        assert_eq!(r.bytes_hashed, r.manifest.total_bytes());
        assert_eq!(r.manifest.files[0].path, "README");
        let text = std::fs::read(t.0.join(MANIFEST_NAME)).unwrap();
        assert_eq!(Manifest::parse(&text).unwrap().to_text().as_bytes(), &text[..]);
    }

    #[test]
    fn bit_flip_every_file_position_kind() {
        let t = tmp("flip");
        let vk = image(&t.0, &key(7));
        let p = t.0.join("bin/galion-admin");
        for pos in [0usize, 1, 4095, 65_535, 69_999] {
            let mut d = std::fs::read(&p).unwrap();
            d[pos] ^= 0x01;
            std::fs::write(&p, &d).unwrap();
            let r = verify(&t.0, &vk).unwrap();
            assert_eq!(r.problems, vec![Problem::Hash("bin/galion-admin".into())]);
            d[pos] ^= 0x01;
            std::fs::write(&p, &d).unwrap();
            assert!(verify(&t.0, &vk).unwrap().ok());
        }
    }

    #[test]
    fn truncation_growth_missing_unlisted() {
        let t = tmp("trunc");
        let vk = image(&t.0, &key(7));
        let p = t.0.join("bin/galion-admin");
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(69_999).unwrap();
        drop(f);
        let r = verify(&t.0, &vk).unwrap();
        assert_eq!(
            r.problems,
            vec![Problem::Truncated {
                path: "bin/galion-admin".into(),
                want: 70_000,
                got: 69_999
            }]
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .and_then(|mut f| io::Write::write_all(&mut f, b"\x7fX"))
            .unwrap();
        let r = verify(&t.0, &vk).unwrap();
        assert_eq!(r.problems[0].code(), "grown");
        std::fs::remove_file(&p).unwrap();
        put(&t.0, "bin/extra", b"x", 0o755);
        std::fs::create_dir(t.0.join("empty")).unwrap();
        let r = verify(&t.0, &vk).unwrap();
        let codes: Vec<_> = r.problems.iter().map(|p| (p.code(), p.path())).collect();
        assert!(codes.contains(&("missing", "bin/galion-admin")), "{codes:?}");
        assert!(codes.contains(&("unlisted", "bin/extra")), "{codes:?}");
        assert!(codes.contains(&("unlisted-dir", "empty")), "{codes:?}");
    }

    #[test]
    fn mode_symlink_special() {
        let t = tmp("mode");
        let vk = image(&t.0, &key(7));
        std::fs::set_permissions(
            t.0.join("README"),
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        let r = verify(&t.0, &vk).unwrap();
        assert_eq!(
            r.problems,
            vec![Problem::Mode {
                path: "README".into(),
                want: 0o644,
                got: 0o666
            }]
        );
        std::fs::set_permissions(
            t.0.join("README"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        // Replace a listed file by a symlink to an identical copy elsewhere.
        let outside = t.0.with_extension("outside");
        std::fs::write(&outside, b"gold image\n").unwrap();
        std::fs::remove_file(t.0.join("README")).unwrap();
        std::os::unix::fs::symlink(&outside, t.0.join("README")).unwrap();
        let r = verify(&t.0, &vk).unwrap();
        let _ = std::fs::remove_file(&outside);
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        assert_eq!(r.problems[0].code(), "not-regular");
        assert_eq!(r.problems[0].detail(), "symlink");
        // Builder refuses symlinks too.
        assert!(build(&t.0, header()).unwrap_err().contains("symlink"));
    }

    #[test]
    fn wrong_key_and_bad_signature() {
        let t = tmp("key");
        let _ = image(&t.0, &key(7));
        let other = key(8).verifying_key();
        assert!(matches!(
            verify(&t.0, &other),
            Err(Rejection::WrongKey { .. })
        ));
        // Same fingerprint line forged onto another key's signature: still BadSignature.
        let man = std::fs::read(t.0.join(MANIFEST_NAME)).unwrap();
        let forged = sign(&man, &key(8)).replace(
            &fingerprint(&key(8).verifying_key()),
            &fingerprint(&key(7).verifying_key()),
        );
        std::fs::write(t.0.join(SIG_NAME), forged).unwrap();
        assert_eq!(
            verify(&t.0, &key(7).verifying_key()).unwrap_err(),
            Rejection::BadSignature
        );
    }

    #[test]
    fn manifest_tamper_is_bad_signature() {
        let t = tmp("mtamper");
        let vk = image(&t.0, &key(7));
        let man = std::fs::read_to_string(t.0.join(MANIFEST_NAME)).unwrap();
        for evil in [
            man.replace("0.1.0", "0.1.1"),
            man.replace("0755", "4755"),
            man.replace("end\n", ""),
            format!("{man}x"),
            man.replace('\n', "\r\n"),
        ] {
            std::fs::write(t.0.join(MANIFEST_NAME), &evil).unwrap();
            assert_eq!(verify(&t.0, &vk).unwrap_err(), Rejection::BadSignature);
        }
        // Missing metadata.
        std::fs::remove_file(t.0.join(MANIFEST_NAME)).unwrap();
        assert!(matches!(verify(&t.0, &vk), Err(Rejection::Metadata(_))));
    }

    #[test]
    fn hades_approval_signature_never_verifies() {
        // An approval-domain signature over the same bytes must not pass as an image signature.
        let t = tmp("domain");
        let k = key(7);
        let vk = image(&t.0, &k);
        let man = std::fs::read(t.0.join(MANIFEST_NAME)).unwrap();
        let mut msg = b"galion-hades-gate/approval/v1\n".to_vec();
        msg.extend_from_slice(&man);
        let sig = k.sign(&msg);
        std::fs::write(
            t.0.join(SIG_NAME),
            format!(
                "{SIG_MAGIC}\nkey {}\n{SIG_TAG} {}\n",
                fingerprint(&vk),
                to_hex(&sig.to_bytes())
            ),
        )
        .unwrap();
        assert_eq!(verify(&t.0, &vk).unwrap_err(), Rejection::BadSignature);
    }

    #[test]
    fn strict_parse_rejects() {
        let k = key(1);
        let _ = &k;
        let good = Manifest {
            header: header(),
            files: vec![Entry {
                path: "a".into(),
                mode: 0o644,
                size: 1,
                sha256: [0; 32],
            }],
        };
        let g = good.to_text();
        assert!(Manifest::parse(g.as_bytes()).is_ok());
        let zero = "0".repeat(64);
        for (bad, why) in [
            (g.replace("file 0644 1 ", "file 0644 01 "), "leading zero"),
            (g.replace(&zero, &"A".repeat(64)), "upper hex"),
            (g.replace("bytes 1", "bytes 2"), "bytes mismatch"),
            (g.replace("files 1", "files 2"), "count mismatch"),
            (g.replace(" a\n", " ../a\n"), "dotdot"),
            (g.replace(" a\n", " /a\n"), "absolute"),
            (g.replace(" a\n", " a b\n"), "space in path"),
            (g.replace(" a\n", " galion-image.sig\n"), "reserved"),
            (g.replace("0644", "4755"), "setuid"),
            (g.replace("+02:00", "Z"), "UTC Z time"),
            (g.replace("name galion", "name  galion"), "double space"),
            (g.replace("build.time", "build.date"), "key name"),
            (g.replace("end\n", "end"), "no final newline"),
            (format!("{g}\n"), "trailing data"),
            (g.replace("v1", "v2"), "version"),
        ] {
            assert!(Manifest::parse(bad.as_bytes()).is_err(), "{why} accepted");
        }
        // Order, duplicates and file/dir clash.
        let mut m = good.clone();
        m.files.push(Entry {
            path: "a/b".into(),
            ..m.files[0].clone()
        });
        assert!(m.validate().unwrap_err().contains("file and as a directory"));
        m.files[1].path = "a".into();
        assert!(m.validate().is_err(), "duplicate");
        m.files[1].path = "0".into();
        assert!(m.validate().is_err(), "order");
    }

    #[test]
    fn sig_file_and_keys() {
        let k = key(3);
        let s = sign(b"x", &k);
        assert!(parse_sig(s.as_bytes()).is_ok());
        assert!(parse_sig(s.trim_end().as_bytes()).is_err());
        assert!(parse_sig(s.to_uppercase().as_bytes()).is_err());
        assert!(parse_sig(format!("{s}x").as_bytes()).is_err());
        let t = tmp("keys");
        let p = t.0.join("admin.pub");
        std::fs::write(&p, format!("{PUB_TAG} {}\n", to_hex(k.verifying_key().as_bytes()))).unwrap();
        assert_eq!(load_public(&p).unwrap(), k.verifying_key());
        std::fs::write(&p, format!("{SEED_TAG} {}\n", to_hex(&[3; 32]))).unwrap();
        assert!(load_public(&p).unwrap_err().contains("not a ed25519-pub"));
        assert_eq!(load_seed(&p).unwrap().verifying_key(), k.verifying_key());
        // Identity point (all-zero except first byte = 1) is a weak key.
        let mut weak = [0u8; 32];
        weak[0] = 1;
        std::fs::write(&p, format!("{PUB_TAG} {}\n", to_hex(&weak))).unwrap();
        assert!(load_public(&p).unwrap_err().contains("weak"));
        assert!(load_public(Path::new("/root/galion-secrets/x.pub"))
            .unwrap_err()
            .contains("refused"));
    }

    #[test]
    fn time_and_headers() {
        assert!(valid_time("2026-10-25T02:30:00+01:00"));
        assert!(!valid_time("2026-10-25T02:30:00Z"));
        assert!(!valid_time("2026-13-25T02:30:00+01:00"));
        assert!(check_header("name", "-x").is_err());
        assert!(check_header("build.commit", "BF33CBA").is_err());
        assert!(check_header("build.commit", "bf33cba").is_ok());
    }
}
