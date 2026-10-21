// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! `galion-backup run`: one periodic backup, step for step what
//! galion-backup.sh did (see docs/galion-backup.md):
//!
//! umask 077; DEST (0700) / KEEP / MINFREE_KB from the same env vars; free
//! space check; non-blocking flock on DEST/.lock; work dir DEST/.work-XXXXXX
//! removed on every exit (also SIGTERM/SIGINT/SIGHUP); online SQLite
//! snapshots + integrity verify + db/MAP; files.tar of the same path list and
//! excludes; SHA256SUMS (C-locale sorted); DEST/.galion-STAMP.tar.gz.part
//! (0600) renamed into place; keep the newest KEEP archives; one ok log line.

use crate::gzip::GzWriter;
use crate::sha256::{hex, Sha256};
use crate::sqlite;
use crate::tarw::{backup_exclude, TarWriter};
use std::ffi::CString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};

/// SQLite databases: (source, name inside db/), in the script's order.
pub const DBS: &[(&str, &str)] = &[
    (
        "/var/lib/memory-tencentdb/instances/galion/vectors.db",
        "memory-vectors.db",
    ),
    (
        "/var/lib/memory-tencentdb/vectors.db",
        "memory-root-vectors.db",
    ),
    ("/var/lib/hermes-rs/rs-state.db", "hermes-rs-state.db"),
    ("/opt/galion/hermes/state.db", "hermes-legacy-state.db"),
];

/// Plain files and trees, in the script's order (DBs and -wal/-shm excluded).
pub const FILES: &[&str] = &[
    "/var/lib/memory-tencentdb",
    "/var/lib/hermes-rs",
    "/opt/galion/galion-src/galion-infrastructure/cloudflared",
    "/root/.cloudflared",
    "/opt/galion/hermes/.env",
    "/opt/galion/hermes/config.yaml",
    "/etc/openmind-memory.env",
    "/etc/memory-gateway.env",
    "/root/galion-secrets/admin-device.json",
    "/etc/systemd/system/hermes-rs.service",
    "/etc/systemd/system/openmind-memory.service",
    "/etc/systemd/system/link-watch-rs.service",
    "/etc/systemd/system/cloudflared.service",
    "/etc/systemd/system/galion-health.service",
];

/// `for f in /root/galion-switch-20261018-2140/dns-backup-*.json`.
pub const DNS_DIR: &str = "/root/galion-switch-20261018-2140";

pub const DEFAULT_DEST: &str = "/var/backups/galion";
pub const DEFAULT_KEEP: u64 = 12; // 12 x 6h = 3 days
pub const DEFAULT_MINFREE_KB: u64 = 2_097_152;
pub const DEFAULT_GZIP_LEVEL: u8 = 6;

#[derive(Debug, Clone)]
pub struct Opts {
    /// Filesystem root the absolute source paths live under (`/` in
    /// production; a fake tree in tests). Not settable from the CLI or env.
    pub root: PathBuf,
    pub dest: PathBuf,
    pub keep: u64,
    /// None = the value was not an integer (bash's `[ -ge ]` then failed,
    /// which took the low-disk branch).
    pub minfree_kb: Option<u64>,
    /// gzip/deflate level 0-9 (GALION_BACKUP_GZIP_LEVEL, default 6 = what
    /// `tar -z` used). Only changes the .tar.gz bytes/size and CPU time, never
    /// what is backed up; see docs/real-tests/backup-perf.md for the trade-off.
    pub gzip_level: u8,
}

fn env_or(name: &str, default: &str) -> String {
    // ${VAR:-default}: unset or empty -> default.
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

impl Opts {
    /// The script's env interface: GALION_BACKUP_DEST, _KEEP, _MINFREE_KB
    /// (+ GALION_BACKUP_GZIP_LEVEL, Rust only, default 6).
    pub fn from_env() -> Result<Opts, String> {
        let keep_s = env_or("GALION_BACKUP_KEEP", &DEFAULT_KEEP.to_string());
        let keep = keep_s.trim().parse::<u64>().map_err(|_| {
            format!("GALION_BACKUP_KEEP must be a non-negative integer, got {keep_s:?}")
        })?;
        let minfree = env_or("GALION_BACKUP_MINFREE_KB", &DEFAULT_MINFREE_KB.to_string());
        let lvl_s = env_or("GALION_BACKUP_GZIP_LEVEL", &DEFAULT_GZIP_LEVEL.to_string());
        let gzip_level = match lvl_s.trim().parse::<u8>() {
            Ok(l) if l <= 9 => l,
            _ => {
                return Err(format!(
                    "GALION_BACKUP_GZIP_LEVEL must be 0-9, got {lvl_s:?}"
                ))
            }
        };
        Ok(Opts {
            root: PathBuf::from("/"),
            dest: PathBuf::from(env_or("GALION_BACKUP_DEST", DEFAULT_DEST)),
            keep,
            minfree_kb: minfree.trim().parse::<u64>().ok(),
            gzip_level,
        })
    }

    fn src(&self, abs: &str) -> PathBuf {
        self.root.join(abs.trim_start_matches('/'))
    }
}

// ---------------------------------------------------------------- signals

static SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: libc::c_int) {
    SIGNAL.store(sig, Ordering::SeqCst);
}

/// Catch SIGTERM/SIGINT/SIGHUP so the work dir is removed (bash's EXIT trap).
pub fn install_signal_handlers() {
    for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only stores into an atomic (async-signal-safe).
        unsafe {
            libc::signal(s, on_signal as *const () as libc::sighandler_t);
        }
    }
}

fn interrupted() -> bool {
    SIGNAL.load(Ordering::SeqCst) != 0
}

// ---------------------------------------------------------------- helpers

/// `TZ=Europe/Zagreb date -Iseconds`.
pub fn now_iso() -> String {
    zagreb_time::now_rfc3339()
}

/// `TZ=Europe/Zagreb date +%Y%m%d-%H%M%S%z` from an RFC 3339 Zagreb stamp.
pub fn stamp_from_rfc3339(s: &str) -> String {
    // 2026-10-20T20:22:05+02:00 -> 20261020-202205+0200
    let b = s.as_bytes();
    if b.len() < 25 {
        return s.replace([':', '-', 'T'], "");
    }
    format!(
        "{}{}{}-{}{}{}{}{}",
        &s[0..4],
        &s[5..7],
        &s[8..10],
        &s[11..13],
        &s[14..16],
        &s[17..19],
        &s[19..22],
        &s[23..25]
    )
}

pub fn log(msg: &str) {
    let mut o = io::stdout().lock();
    let _ = writeln!(o, "ts={} svc=galion-backup {msg}", now_iso());
    let _ = o.flush();
}

fn err(ctx: &str, e: impl std::fmt::Display) -> i32 {
    eprintln!("galion-backup: {ctx}: {e}");
    1
}

/// `df -Pk DIR` "Available" column: f_bavail * f_frsize in KiB, rounded up.
pub fn avail_kb(dir: &Path) -> io::Result<u64> {
    let c = CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c is NUL-terminated, st is a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = (st.f_bavail as u128) * (st.f_frsize as u128);
    Ok(bytes.div_ceil(1024) as u64)
}

/// `du -k FILE`: allocated blocks in KiB, rounded up.
pub fn du_kb(p: &Path) -> io::Result<u64> {
    let m = fs::symlink_metadata(p)?;
    Ok((m.blocks() * 512).div_ceil(1024))
}

fn random_suffix() -> String {
    const CH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut b = [0u8; 6];
    if File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .is_err()
    {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            ^ (std::process::id() as u128) << 20;
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> (i * 8)) as u8;
        }
    }
    b.iter()
        .map(|&x| CH[x as usize % CH.len()] as char)
        .collect()
}

/// `mktemp -d DIR/.work-XXXXXX` (mode 0700), removed on drop (`trap rm -rf`).
struct WorkDir(PathBuf);

impl WorkDir {
    fn create(dest: &Path) -> io::Result<WorkDir> {
        for _ in 0..100 {
            let p = dest.join(format!(".work-{}", random_suffix()));
            match DirBuilder::new().mode(0o700).create(&p) {
                Ok(()) => return Ok(WorkDir(p)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("mktemp: too many collisions"))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Bash glob `galion-*.tar.gz` in DEST (no dotfiles; C-locale order).
pub fn archives(dest: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = match fs::read_dir(dest) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let n = n.as_bytes();
                n.len() >= b"galion-.tar.gz".len()
                    && n.starts_with(b"galion-")
                    && n.ends_with(b".tar.gz")
            })
            .map(|e| e.path())
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
    v
}

/// `ls -1t`: newest mtime first (ns precision), ties by name; symlinks
/// given on the command line are followed.
pub fn newest_first(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    let key = |p: &PathBuf| {
        fs::metadata(p)
            .map(|m| (m.mtime(), m.mtime_nsec()))
            .unwrap_or((i64::MIN, 0))
    };
    v.sort_by(|a, b| {
        key(b)
            .cmp(&key(a))
            .then_with(|| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()))
    });
    v
}

/// `( cd S && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS )`
/// with LANG=C (the VPS service locale): byte order.
pub fn write_sha256sums(s: &Path) -> io::Result<()> {
    let out_path = s.join("SHA256SUMS");
    // The redirect creates/truncates SHA256SUMS before find runs.
    let out = File::create(&out_path)?;
    let mut files: Vec<Vec<u8>> = Vec::new();
    fn walk(dir: &Path, rel: &[u8], acc: &mut Vec<Vec<u8>>) -> io::Result<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let ft = e.file_type()?;
            let mut r = rel.to_vec();
            r.push(b'/');
            r.extend_from_slice(e.file_name().as_bytes());
            if ft.is_dir() {
                walk(&e.path(), &r, acc)?;
            } else if ft.is_file() && e.file_name().as_bytes() != b"SHA256SUMS" {
                acc.push(r);
            }
        }
        Ok(())
    }
    walk(s, b".", &mut files)?;
    files.sort();
    let mut w = BufWriter::new(out);
    let mut buf = vec![0u8; 64 * 1024];
    for rel in files {
        let p = s.join(std::ffi::OsStr::from_bytes(&rel[2..]));
        let mut f = File::open(&p)?;
        let mut h = Sha256::new();
        loop {
            if interrupted() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"));
            }
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        w.write_all(hex(&h.finish()).as_bytes())?;
        w.write_all(b"  ")?;
        w.write_all(&rel)?;
        w.write_all(b"\n")?;
    }
    w.flush()
}

/// The full list of `tar` operands (`${EX[@]#/}`): FILES + sorted
/// dns-backup-*.json, keeping only those that exist (`[ -e ]`, follows links).
pub fn existing_operands(o: &Opts) -> Vec<String> {
    let mut all: Vec<String> = FILES.iter().map(|s| s.to_string()).collect();
    let mut dns: Vec<Vec<u8>> = match fs::read_dir(o.src(DNS_DIR)) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.file_name().as_bytes().to_vec())
            .filter(|n| {
                n.len() >= b"dns-backup-.json".len()
                    && n.starts_with(b"dns-backup-")
                    && n.ends_with(b".json")
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    dns.sort();
    for n in dns {
        all.push(format!("{DNS_DIR}/{}", String::from_utf8_lossy(&n)));
    }
    all.into_iter().filter(|p| o.src(p).exists()).collect()
}

fn sig_code() -> Option<i32> {
    match SIGNAL.load(Ordering::SeqCst) {
        0 => None,
        s => Some(128 + s),
    }
}

/// Run one backup; returns the process exit code.
pub fn run(o: &Opts) -> i32 {
    // SAFETY: umask is always safe to call.
    unsafe {
        libc::umask(0o077);
    }
    let stamp = stamp_from_rfc3339(&now_iso());
    let dest = &o.dest;

    if let Err(e) = fs::create_dir_all(dest) {
        return err(&format!("mkdir {}", dest.display()), e);
    }
    if let Err(e) = fs::set_permissions(dest, fs::Permissions::from_mode(0o700)) {
        return err(&format!("chmod {}", dest.display()), e);
    }
    let avail = avail_kb(dest).ok();
    let enough = matches!((avail, o.minfree_kb), (Some(a), Some(m)) if a >= m);
    if !enough {
        let a = avail.map(|a| a.to_string()).unwrap_or_default();
        log(&format!("level=error msg=\"low disk {a}KB\""));
        return 1;
    }
    let lock = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o666)
        .open(dest.join(".lock"))
    {
        Ok(f) => f,
        Err(e) => return err(&format!("{}/.lock", dest.display()), e),
    };
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            log("level=warn msg=\"already running\"");
            return 0;
        }
        Err(fs::TryLockError::Error(e)) => return err("flock", e),
    }

    let work = match WorkDir::create(dest) {
        Ok(w) => w,
        Err(e) => return err("mktemp", e),
    };
    let code = in_workdir(o, &work.0, &stamp);
    drop(work);
    drop(lock);
    code
}

fn in_workdir(o: &Opts, w: &Path, stamp: &str) -> i32 {
    let dest = &o.dest;
    let top = format!("galion-{stamp}");
    let s = w.join(&top);
    for sub in ["db", "files"] {
        if let Err(e) = DirBuilder::new().recursive(true).create(s.join(sub)) {
            return err("mkdir", e);
        }
    }

    // ---- SQLite snapshots
    for (src, name) in DBS {
        if let Some(c) = sig_code() {
            return c;
        }
        let p = o.src(src);
        if !p.is_file() {
            log(&format!("level=warn msg=\"missing db {src}\""));
            continue;
        }
        let dst = s.join("db").join(name);
        let ok = match sqlite::snapshot(&p, &dst) {
            Err(e) => {
                eprintln!("galion-backup: {e}");
                false
            }
            Ok(()) => match sqlite::verify(&dst) {
                Err(e) => {
                    eprintln!("galion-backup: {e}");
                    false
                }
                Ok((r, _)) => r == "ok",
            },
        };
        let ok = ok
            && OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o666)
                .open(s.join("db/MAP"))
                .and_then(|mut f| f.write_all(format!("{src} {name}\n").as_bytes()))
                .is_ok();
        if !ok {
            log(&format!("level=error msg=\"snapshot failed {src}\""));
            return 1;
        }
    }

    // ---- files.tar
    let ops = existing_operands(o);
    if ops.is_empty() {
        // GNU tar refuses an empty archive (exit 2) -> the script failed.
        return err("tar", "no files to archive");
    }
    let r = (|| -> io::Result<()> {
        let f = File::create(s.join("files/files.tar"))?;
        let mut t = TarWriter::new(BufWriter::with_capacity(256 * 1024, f));
        t.cancel = interrupted;
        for p in &ops {
            t.add_tree(
                &o.src(p),
                p.trim_start_matches('/').as_bytes(),
                &backup_exclude,
            )?;
        }
        t.finish()?.into_inner().map_err(|e| e.into_error())?;
        Ok(())
    })();
    if let Err(e) = r {
        return sig_code().unwrap_or_else(|| err("tar files.tar", e));
    }

    // ---- SHA256SUMS
    if let Err(e) = write_sha256sums(&s) {
        return sig_code().unwrap_or_else(|| err("sha256sum", e));
    }

    // ---- archive
    let part = dest.join(format!(".{top}.tar.gz.part"));
    let final_ = dest.join(format!("{top}.tar.gz"));
    let r = (|| -> io::Result<()> {
        let f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o666)
            .open(&part)?;
        let gz = GzWriter::new(BufWriter::with_capacity(256 * 1024, f), o.gzip_level as i32)?;
        let mut t = TarWriter::new(gz);
        t.cancel = interrupted;
        t.add_tree(&s, top.as_bytes(), &|_| false)?;
        t.finish()?
            .finish()?
            .into_inner()
            .map_err(|e| e.into_error())?;
        Ok(())
    })();
    if let Err(e) = r {
        // tar -czf leaves its partial output behind; so do we (dotfile,
        // 0600, overwritten by the next run with the same name only).
        return sig_code().unwrap_or_else(|| err("tar archive", e));
    }
    if let Err(e) = fs::set_permissions(&part, fs::Permissions::from_mode(0o600)) {
        return err("chmod", e);
    }
    if let Err(e) = fs::rename(&part, &final_) {
        return err("mv", e);
    }

    // ---- rotation: keep newest KEEP
    let ordered = newest_first(archives(dest));
    for old in ordered.iter().skip(o.keep as usize) {
        if let Err(e) = fs::remove_file(old) {
            if e.kind() != io::ErrorKind::NotFound {
                return err(&format!("rm {}", old.display()), e);
            }
        }
    }
    let size = match du_kb(&final_) {
        Ok(k) => k,
        Err(e) => return err("du", e),
    };
    let kept = archives(dest).len();
    log(&format!(
        "level=info msg=\"ok\" archive={top}.tar.gz size_kb={size} kept={kept}"
    ));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_format() {
        assert_eq!(
            stamp_from_rfc3339("2026-10-20T20:22:05+02:00"),
            "20261020-202205+0200"
        );
        assert_eq!(
            stamp_from_rfc3339("2026-12-01T06:17:00+01:00"),
            "20261201-061700+0100"
        );
    }
}
