// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! SEC-3: tighten the Hermes home permissions (`galion-purge sec3-perms`).
//! Rust port of the former staged shell script, same behaviour:
//!
//! * dry run by default: prints current -> target and changes nothing;
//! * `--apply` writes a rollback file (modes and owners only) **before**
//!   changing anything, then sets the target modes and `root:root`;
//! * `--fix-world-writable` also clears `o+w` on every other world-writable
//!   file or directory under the Hermes home (the dry run reports them);
//! * `--rollback FILE` restores exactly the modes and owners recorded.
//!
//! It never reads file contents, never follows symlinks (a symlinked target
//! is refused, `lchown` is used), never leaves the Hermes home (no crossing
//! into other filesystems), and refuses `secrets/persist`, anything named
//! `galion-control`, and homes such as `/`, `/opt`, `/root` or `/etc`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// One SEC-3 target: path relative to the Hermes home, target mode, type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// Path relative to the Hermes home.
    pub rel: &'static str,
    /// Target permission bits.
    pub mode: u32,
    /// `true` for a directory, `false` for a regular file.
    pub dir: bool,
}

/// The fixed SEC-3 target list (all become `root:root`).
pub const TARGETS: [Target; 5] = [
    // holds api_key / client_secret / sudo_password fields
    Target {
        rel: "config.yaml",
        mode: 0o600,
        dir: false,
    },
    // read-only persona text; only root writes it
    Target {
        rel: "SOUL.md",
        mode: 0o644,
        dir: false,
    },
    // local browser vault dir
    Target {
        rel: "vault",
        mode: 0o700,
        dir: true,
    },
    Target {
        rel: "vault/.vault.lock",
        mode: 0o600,
        dir: false,
    },
    // operator profile (personal data)
    Target {
        rel: "USER.md",
        mode: 0o600,
        dir: false,
    },
];

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Show current -> target; change nothing.
    Dry,
    /// Record a rollback file, then change.
    Apply,
    /// Restore modes and owners from a rollback file.
    Rollback(PathBuf),
}

/// Options for [`run`].
#[derive(Debug, Clone)]
pub struct Opts {
    /// Hermes home (default `/opt/galion/hermes`).
    pub home: PathBuf,
    /// Where rollback files go (default `/root/sec3-perms`).
    pub state_dir: PathBuf,
    /// Dry run, apply or rollback.
    pub mode: Mode,
    /// Also clear `o+w` on the other world-writable entries.
    pub fix_world_writable: bool,
    /// Skip chown (fixture tests as non-root).
    pub no_chown: bool,
}

/// Errors: usage (exit 2) or a refusal (exit 3), with a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sec3Error {
    /// Bad input: exit 2.
    Usage(String),
    /// Unsafe target: exit 3.
    Refuse(String),
}

impl std::fmt::Display for Sec3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sec3Error::Usage(m) => write!(f, "{m}"),
            Sec3Error::Refuse(m) => write!(f, "REFUSE: {m}"),
        }
    }
}

type R<T> = Result<T, Sec3Error>;

fn usage<E: std::fmt::Display>(ctx: &str) -> impl FnOnce(E) -> Sec3Error + '_ {
    move |e| Sec3Error::Usage(format!("{ctx}: {e}"))
}

/// Resolve and vet the Hermes home: canonical, not a forbidden location.
pub fn resolve_home(home: &Path) -> R<PathBuf> {
    let h = fs::canonicalize(home)
        .map_err(|e| Sec3Error::Usage(format!("no such HERMES_HOME {}: {e}", home.display())))?;
    let s = h.to_string_lossy();
    let forbidden = ["/", "/opt", "/opt/galion", "/root", "/etc"];
    if forbidden.contains(&s.as_ref())
        || s.contains("/secrets/persist")
        || s.contains("galion-control")
    {
        return Err(Sec3Error::Usage(format!("refusing HERMES_HOME={s}")));
    }
    if !h.is_dir() {
        return Err(Sec3Error::Usage(format!(
            "HERMES_HOME {s} is not a directory"
        )));
    }
    Ok(h)
}

/// Name lookup for uid/gid from /etc/passwd and /etc/group (names only;
/// falls back to numbers).
#[derive(Debug, Default)]
pub struct Names {
    users: BTreeMap<u32, String>,
    groups: BTreeMap<u32, String>,
}

impl Names {
    /// Load from the local account databases (missing files are fine).
    pub fn load() -> Names {
        fn parse(p: &str) -> BTreeMap<u32, String> {
            let mut m = BTreeMap::new();
            if let Ok(t) = fs::read_to_string(p) {
                for l in t.lines() {
                    let f: Vec<&str> = l.split(':').collect();
                    if f.len() >= 3 {
                        if let Ok(id) = f[2].parse::<u32>() {
                            m.entry(id).or_insert_with(|| f[0].to_string());
                        }
                    }
                }
            }
            m
        }
        Names {
            users: parse("/etc/passwd"),
            groups: parse("/etc/group"),
        }
    }

    /// `user:group` for display.
    pub fn owner(&self, uid: u32, gid: u32) -> String {
        let u = self
            .users
            .get(&uid)
            .cloned()
            .unwrap_or_else(|| uid.to_string());
        let g = self
            .groups
            .get(&gid)
            .cloned()
            .unwrap_or_else(|| gid.to_string());
        format!("{u}:{g}")
    }
}

/// Every world-writable regular file or directory under `home` (not
/// symlinks, same filesystem), relative paths, excluding the SEC-3 targets.
pub fn world_writable(home: &Path) -> Vec<String> {
    let dev = match fs::symlink_metadata(home) {
        Ok(m) => m.dev(),
        Err(_) => return Vec::new(),
    };
    let skip: BTreeSet<&str> = TARGETS.iter().map(|t| t.rel).collect();
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let Ok(rd) = fs::read_dir(home.join(&rel)) else {
            continue;
        };
        for e in rd.flatten() {
            let r = rel.join(e.file_name());
            let Ok(m) = fs::symlink_metadata(home.join(&r)) else {
                continue;
            };
            let ft = m.file_type();
            if ft.is_symlink() || m.dev() != dev || !(ft.is_file() || ft.is_dir()) {
                continue;
            }
            let rs = r.to_string_lossy().into_owned();
            if m.mode() & 0o002 != 0 && !skip.contains(rs.as_str()) {
                out.push(rs);
            }
            if ft.is_dir() {
                stack.push(r);
            }
        }
    }
    out.sort();
    out
}

fn stat_nofollow(p: &Path) -> R<fs::Metadata> {
    fs::symlink_metadata(p).map_err(usage("stat"))
}

fn chmod(p: &Path, mode: u32) -> R<()> {
    // refuse to follow a symlink that appeared after planning
    let m = stat_nofollow(p)?;
    if m.file_type().is_symlink() {
        return Err(Sec3Error::Refuse(format!(
            "{} became a symlink",
            p.display()
        )));
    }
    fs::set_permissions(p, fs::Permissions::from_mode(mode))
        .map_err(|e| Sec3Error::Usage(format!("chmod {}: {e}", p.display())))
}

fn chown(p: &Path, uid: u32, gid: u32) -> R<()> {
    std::os::unix::fs::lchown(p, Some(uid), Some(gid))
        .map_err(|e| Sec3Error::Usage(format!("chown {}: {e}", p.display())))
}

/// Run SEC-3. Human output goes to `out`. Returns the rollback file path
/// written by `--apply` (if any).
pub fn run(o: &Opts, out: &mut dyn Write) -> R<Option<PathBuf>> {
    let h = resolve_home(&o.home)?;
    let names = Names::load();
    let w = |out: &mut dyn Write, s: String| {
        let _ = writeln!(out, "{s}");
    };

    if let Mode::Rollback(file) = &o.mode {
        let text = fs::read_to_string(file)
            .map_err(|e| Sec3Error::Usage(format!("no rollback file {}: {e}", file.display())))?;
        let entries = parse_rollback(&text)?;
        for (rel, mode, uid, gid) in entries {
            if rel.split('/').any(|c| c == ".." || c.is_empty()) || rel.starts_with('/') {
                return Err(Sec3Error::Refuse(format!(
                    "rollback path {rel} leaves HERMES_HOME"
                )));
            }
            let p = h.join(&rel);
            match fs::symlink_metadata(&p) {
                Ok(m) if !m.file_type().is_symlink() => {}
                _ => {
                    w(out, format!("skip (missing or symlink): {rel}"));
                    continue;
                }
            }
            chmod(&p, mode)?;
            if !o.no_chown {
                chown(&p, uid, gid)?;
            }
        }
        w(
            out,
            format!(
                "{} rollback from {} done",
                crate::zagreb::now_rfc3339(),
                file.display()
            ),
        );
        return Ok(None);
    }

    let mode_name = if o.mode == Mode::Apply {
        "apply"
    } else {
        "dry"
    };
    w(
        out,
        format!(
            "{} SEC-3 {mode_name} on {}",
            crate::zagreb::now_rfc3339(),
            h.display()
        ),
    );
    w(out, format!("{:<22} {:<16} -> target", "path", "current"));
    let mut plan: Vec<Target> = Vec::new();
    for t in TARGETS {
        let p = h.join(t.rel);
        let m = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => {
                w(out, format!("{:<22} {:<16} -> skip", t.rel, "missing"));
                continue;
            }
        };
        let ft = m.file_type();
        if ft.is_symlink() {
            return Err(Sec3Error::Refuse(format!("{} is a symlink", t.rel)));
        }
        if (t.dir && !ft.is_dir()) || (!t.dir && !ft.is_file()) {
            let want = if t.dir { "d" } else { "f" };
            return Err(Sec3Error::Refuse(format!("{} is not type {want}", t.rel)));
        }
        let cur = format!("{:o} {}", m.mode() & 0o7777, names.owner(m.uid(), m.gid()));
        w(
            out,
            format!("{:<22} {:<16} -> {:o} root:root", t.rel, cur, t.mode),
        );
        plan.push(t);
    }

    let ww = world_writable(&h);
    if !ww.is_empty() {
        w(
            out,
            format!(
                "other world-writable entries under HERMES_HOME ({}):",
                ww.len()
            ),
        );
        for r in ww.iter().take(50) {
            w(out, format!("  {r}"));
        }
        w(
            out,
            if o.fix_world_writable {
                "  -> chmod o-w (--fix-world-writable)".to_string()
            } else {
                "  -> report only (add --fix-world-writable to fix)".to_string()
            },
        );
    }

    if o.mode == Mode::Dry {
        w(out, "dry run: nothing changed".to_string());
        return Ok(None);
    }

    // rollback file first: modes and owners only, written 0600 in a 0700 dir
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&o.state_dir)
        .map_err(|e| Sec3Error::Usage(format!("state dir {}: {e}", o.state_dir.display())))?;
    let stamp = file_stamp();
    let bk = o.state_dir.join(format!("sec3-before-{stamp}.tsv"));
    let mut body = format!(
        "# SEC-3 rollback file, {}, HERMES_HOME={} (modes and numeric uid:gid only)\n",
        crate::zagreb::now_rfc3339(),
        h.display()
    );
    let mut recorded: Vec<String> = plan.iter().map(|t| t.rel.to_string()).collect();
    recorded.extend(ww.iter().cloned());
    for rel in &recorded {
        let m = stat_nofollow(&h.join(rel))?;
        body.push_str(&format!(
            "{rel}\t{:o}\t{}:{}\n",
            m.mode() & 0o7777,
            m.uid(),
            m.gid()
        ));
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&bk)
        .map_err(|e| Sec3Error::Usage(format!("rollback file {}: {e}", bk.display())))?;
    f.write_all(body.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| Sec3Error::Usage(format!("rollback file {}: {e}", bk.display())))?;
    w(out, format!("rollback file: {}", bk.display()));

    for t in &plan {
        let p = h.join(t.rel);
        if !o.no_chown {
            chown(&p, 0, 0)?;
        }
        chmod(&p, t.mode)?;
    }
    if o.fix_world_writable {
        for rel in &ww {
            let p = h.join(rel);
            let m = stat_nofollow(&p)?;
            if m.file_type().is_symlink() {
                continue;
            }
            chmod(&p, (m.mode() & 0o7777) & !0o002)?;
        }
    }
    w(out, "after:".to_string());
    for t in &plan {
        let m = stat_nofollow(&h.join(t.rel))?;
        w(
            out,
            format!(
                "  {:<22} {:o} {}",
                t.rel,
                m.mode() & 0o7777,
                names.owner(m.uid(), m.gid())
            ),
        );
    }
    w(
        out,
        format!(
            "{} SEC-3 applied; remaining world-writable under HERMES_HOME: {}",
            crate::zagreb::now_rfc3339(),
            world_writable(&h).len()
        ),
    );
    Ok(Some(bk))
}

/// `YYYYMMDDTHHMMSS` (Zagreb) plus nanoseconds, unique per run.
fn file_stamp() -> String {
    let (secs, nanos) = crate::zagreb::now_parts();
    let ts = crate::zagreb::from_unix(secs);
    let digits: String = ts.chars().take(19).filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 14 {
        format!("{}T{}{nanos:09}", &digits[..8], &digits[8..])
    } else {
        format!("{digits}{nanos:09}")
    }
}

/// Parse a rollback file: `rel<TAB>octal mode<TAB>uid:gid`, `#` comments.
pub fn parse_rollback(text: &str) -> R<Vec<(String, u32, u32, u32)>> {
    let mut v = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let bad = || Sec3Error::Usage(format!("rollback file line {}: malformed", i + 1));
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 3 {
            return Err(bad());
        }
        let mode = u32::from_str_radix(f[1], 8).map_err(|_| bad())?;
        if mode > 0o7777 {
            return Err(bad());
        }
        let (u, g) = f[2].split_once(':').ok_or_else(bad)?;
        let uid = u.parse().map_err(|_| bad())?;
        let gid = g.parse().map_err(|_| bad())?;
        v.push((f[0].to_string(), mode, uid, gid));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_parse() {
        let t = "# c\nconfig.yaml\t666\t0:0\nvault\t777\t1000:1000\n";
        let v = parse_rollback(t).unwrap();
        assert_eq!(v[0], ("config.yaml".into(), 0o666, 0, 0));
        assert_eq!(v[1], ("vault".into(), 0o777, 1000, 1000));
        assert!(parse_rollback("a\t99\t0:0\n").is_err());
        assert!(parse_rollback("a\t666\t0\n").is_err());
        assert!(parse_rollback("a\t17777\t0:0\n").is_err());
    }

    #[test]
    fn forbidden_homes() {
        for h in ["/", "/root", "/etc"] {
            if Path::new(h).exists() {
                assert!(resolve_home(Path::new(h)).is_err(), "{h}");
            }
        }
        assert!(resolve_home(Path::new("/nonexistent-sec3-home")).is_err());
    }

    #[test]
    fn stamp_shape() {
        let s = file_stamp();
        assert_eq!(s.len(), 24, "{s}");
        assert_eq!(&s[8..9], "T");
    }
}
