// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-deploy backup`: the pre-cutover snapshot of exactly what a binary
//! swap can break for one unit: its binary, unit file and drop-ins, its state
//! directory and static files, plus the deploy tool itself. Writes
//! `<dest>/<YYYYMMDD-HHMMSS+HHMM>/` with `backup.tar` (ustar), `MANIFEST.tsv`
//! (type, mode, uid, gid, size, mtime, sha256, path), `SHA256SUMS` and
//! `VERIFY.txt`, then re-reads the tar and checks every member against the
//! manifest.
//!
//! One definition of a backup: the data part (SQLite snapshots, configs,
//! tunnel files) is whatever `galion-backup run` produces (swarm/rust-ports
//! 26a2a8e, byte-parity with the live galion-backup.sh). This command runs that
//! binary into `<snapshot>/data/` and only adds the cutover overlay that the
//! periodic backup does not cover: the unit binaries about to be replaced,
//! their unit files and drop-ins, state dirs and static files.
//!
//! Secrets are never read: the two secrets trees, `*.env`/`.env`, `*.key`,
//! `*.pem` and anything with "secret", "token" or "credential" in its name are
//! refused by path before any open. (galion-backup run, like the live script, does
//! archive a few secret files into its 0600 archive; galion-deploy never opens them.)

use crate::tar::{self, Entry, Kind};
use crate::{sha256, zagreb};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// Hex SHA-256 and length of a file.
fn file_sum(p: &Path) -> std::io::Result<(String, u64)> {
    Ok((sha256::file_hex(p)?, std::fs::metadata(p)?.len()))
}

fn log(rest: &str) {
    let line = format!(
        "ts={} svc=galion-deploy {rest}\n",
        zagreb::iso_seconds(zagreb::now_secs())
    );
    let _ = std::io::stdout().lock().write_all(line.as_bytes());
}

/// Never read, never descend.
pub const SECRET_TREES: [&str; 2] = ["/opt/galion/galion-light/secrets", "/root/galion-secrets"];
const MAX_FILE: u64 = 512 * 1024 * 1024;

/// One path to capture (absolute, as on the VPS) and whether it must exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub path: String,
    pub required: bool,
    pub why: &'static str,
}

fn w(path: &str, required: bool, why: &'static str) -> Want {
    Want {
        path: path.into(),
        required,
        why,
    }
}

/// Capture plan per unit, from docs/ops/vps-baseline.md (f403eeb, 2026-10-20 22:52 CEST).
pub fn plan(unit: &str) -> Option<Vec<Want>> {
    Some(match unit {
        "tuner" => vec![
            w(
                "/opt/galion/tuner/bin/tuner",
                true,
                "binary (ExecStart), 508,680 B, sha ecad72a4",
            ),
            w(
                "/opt/galion/tuner",
                true,
                "TUNER_ROOT static files and the bin dir",
            ),
            w("/etc/systemd/system/tuner.service", true, "unit fragment"),
            w(
                "/etc/systemd/system/tuner.service.d",
                false,
                "drop-ins (none at baseline)",
            ),
            w(
                "/run/systemd/transient/tuner-quick.service",
                false,
                "transient quick tunnel to 8650 (does not survive reboot)",
            ),
        ],
        "galion-nerve" => vec![
            w(
                "/opt/galion/rust/bin/galion-nerve",
                true,
                "binary (ExecStart), 816,456 B, sha 3b150dea",
            ),
            w(
                "/etc/systemd/system/galion-nerve.service",
                true,
                "unit fragment",
            ),
            w(
                "/etc/systemd/system/galion-nerve.service.d",
                false,
                "drop-ins (none at baseline)",
            ),
            w(
                "/var/lib/galion-nerve",
                true,
                "StateDirectory / NERVE_DATA_DIR",
            ),
        ],
        _ => return None,
    })
}

/// Always captured: the deploy tool that is about to run the swap.
pub fn common() -> Vec<Want> {
    vec![w(
        "/opt/galion/rust/bin/galion-deploy",
        false,
        "deploy tool (bash 2,830 B, sha 86207af5 at baseline)",
    )]
}

/// Why a path must not be read, if it must not.
pub fn secret_reason(abs: &str) -> Option<&'static str> {
    for t in SECRET_TREES {
        if abs == t || abs.starts_with(&format!("{t}/")) {
            return Some("secrets tree");
        }
    }
    let lower = abs.to_ascii_lowercase();
    if lower
        .split('/')
        .any(|c| c.contains("secret") || c.contains("credential"))
    {
        return Some("secret-like directory or file name");
    }
    let name = lower.rsplit('/').next().unwrap_or("").to_string();
    if name == ".env" || name.ends_with(".env") || name.ends_with(".key") || name.ends_with(".pem")
    {
        return Some("secret-like file name");
    }
    if ["secret", "token", "credential", "id_ed25519", "id_rsa"]
        .iter()
        .any(|k| name.contains(k))
    {
        return Some("secret-like file name");
    }
    None
}

/// Absolute VPS-style path -> path under `root`; rejects `..`.
fn under(root: &Path, abs: &str) -> Option<PathBuf> {
    let p = Path::new(abs);
    if !p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    Some(root.join(abs.trim_start_matches('/')))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: i64,
    /// sha256 for files, `->target` for symlinks, `-` for directories.
    pub sum: String,
    /// Absolute path as on the host.
    pub path: String,
}

impl Row {
    pub fn line(&self) -> String {
        let k = match self.kind {
            Kind::File => "f",
            Kind::Dir => "d",
            Kind::Symlink => "l",
        };
        format!(
            "{k}\t{:04o}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.mode & 0o7777,
            self.uid,
            self.gid,
            self.size,
            zagreb::iso_seconds(self.mtime),
            self.sum,
            self.path
        )
    }

    pub fn parse(l: &str) -> Option<Row> {
        let f: Vec<&str> = l.splitn(8, '\t').collect();
        if f.len() != 8 {
            return None;
        }
        Some(Row {
            kind: match f[0] {
                "f" => Kind::File,
                "d" => Kind::Dir,
                "l" => Kind::Symlink,
                _ => return None,
            },
            mode: u32::from_str_radix(f[1], 8).ok()?,
            uid: f[2].parse().ok()?,
            gid: f[3].parse().ok()?,
            size: f[4].parse().ok()?,
            mtime: 0, // the text stamp is for people; the tar keeps the real mtime
            sum: f[6].into(),
            path: f[7].into(),
        })
    }
}

pub struct Collected {
    pub entries: Vec<Entry>,
    pub rows: Vec<Row>,
    pub notes: Vec<String>,
}

fn collect_path(
    root: &Path,
    abs: &str,
    c: &mut Collected,
    seen: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    if !seen.insert(abs.to_string()) {
        return Ok(());
    }
    if let Some(why) = secret_reason(abs) {
        c.notes.push(format!("skipped {abs}: {why} (not read)"));
        return Ok(());
    }
    let p = under(root, abs).ok_or_else(|| format!("bad path {abs}"))?;
    let md = std::fs::symlink_metadata(&p).map_err(|e| format!("{abs}: {e}"))?;
    let name = abs.trim_start_matches('/').to_string();
    let base = |kind, size, sum: String, link: String, data: Vec<u8>| {
        (
            Entry {
                kind,
                name: name.clone(),
                mode: md.permissions().mode(),
                uid: md.uid(),
                gid: md.gid(),
                mtime: md.mtime(),
                link,
                data,
            },
            Row {
                kind,
                mode: md.permissions().mode(),
                uid: md.uid(),
                gid: md.gid(),
                size,
                mtime: md.mtime(),
                sum,
                path: abs.to_string(),
            },
        )
    };
    let ft = md.file_type();
    if ft.is_symlink() {
        let t = std::fs::read_link(&p).map_err(|e| format!("{abs}: {e}"))?;
        let t = t.to_string_lossy().into_owned();
        let (e, r) = base(Kind::Symlink, 0, format!("->{t}"), t, vec![]);
        c.entries.push(e);
        c.rows.push(r);
    } else if ft.is_file() {
        if md.len() > MAX_FILE {
            return Err(format!(
                "{abs}: {} B is over the {MAX_FILE} B limit",
                md.len()
            ));
        }
        // Read once, hash what was read: a state file rewritten mid-backup is
        // still stored consistently with its manifest line.
        let data = std::fs::read(&p).map_err(|e| format!("{abs}: {e}"))?;
        let sum = sha256::hex(&data);
        let (e, r) = base(Kind::File, data.len() as u64, sum, String::new(), data);
        c.entries.push(e);
        c.rows.push(r);
    } else if ft.is_dir() {
        let (e, r) = base(Kind::Dir, 0, "-".into(), String::new(), vec![]);
        c.entries.push(e);
        c.rows.push(r);
        let mut kids: Vec<String> = std::fs::read_dir(&p)
            .map_err(|e| format!("{abs}: {e}"))?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        kids.sort();
        for k in kids {
            collect_path(root, &format!("{}/{k}", abs.trim_end_matches('/')), c, seen)?;
        }
    } else {
        c.notes
            .push(format!("skipped {abs}: not a file, dir or symlink"));
    }
    Ok(())
}

/// Walk the wanted paths under `root`.
pub fn collect(root: &Path, wants: &[Want]) -> Result<Collected, String> {
    let mut c = Collected {
        entries: vec![],
        rows: vec![],
        notes: vec![],
    };
    let mut seen = std::collections::BTreeSet::new();
    for want in wants {
        if let Some(why) = secret_reason(&want.path) {
            return Err(format!("refusing {}: {why}", want.path));
        }
        let p = under(root, &want.path).ok_or_else(|| format!("bad path {}", want.path))?;
        if std::fs::symlink_metadata(&p).is_err() {
            if want.required {
                return Err(format!(
                    "required path missing: {} ({})",
                    want.path, want.why
                ));
            }
            c.notes
                .push(format!("absent (optional) {}: {}", want.path, want.why));
            continue;
        }
        collect_path(root, &want.path, &mut c, &mut seen)?;
    }
    Ok(c)
}

pub struct Made {
    pub dir: PathBuf,
    pub rows: Vec<Row>,
    pub notes: Vec<String>,
    pub tar_sha: String,
    pub tar_bytes: u64,
    pub data: Option<DataPart>,
}

fn write_file(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    f.write_all(data)?;
    f.sync_all()
}

/// The data part of the snapshot. There is exactly one definition of "a
/// Galion data backup": `galion-backup run` (swarm/rust-ports, byte-parity
/// with the live galion-backup.sh). galion-deploy does not re-implement it; it
/// runs that binary with `GALION_BACKUP_DEST=<snapshot>/data` and records the
/// resulting archive in SHA256SUMS and the manifest header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPart {
    pub exit: i32,
    /// (file name under data/, bytes, sha256)
    pub archive: Option<(String, u64, String)>,
}

/// Run `<bin> run` into `<dir>/data`; its output goes to data/galion-backup.log.
pub fn run_data_backup(bin: &Path, dir: &Path) -> Result<DataPart, String> {
    let data = dir.join("data");
    std::fs::create_dir(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    let out = std::process::Command::new(bin)
        .arg("run")
        .env("GALION_BACKUP_DEST", &data)
        .env("GALION_BACKUP_KEEP", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {} run: {e}", bin.display()))?;
    let mut log = out.stdout.clone();
    log.extend_from_slice(&out.stderr);
    write_file(&data.join("galion-backup.log"), &log).map_err(|e| e.to_string())?;
    let archive = std::fs::read_dir(&data)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("galion-") && n.ends_with(".tar.gz"))
        .max()
        .map(|n| {
            let (sum, bytes) = file_sum(&data.join(&n)).unwrap_or_default();
            (n, bytes, sum)
        });
    Ok(DataPart {
        exit: out.status.code().unwrap_or(128),
        archive,
    })
}

/// Create `<dest>/<stamp>/` with the overlay tar, the galion-backup data
/// archive (unless `data_bin` is None), manifest and sums, then verify it.
pub fn make(
    root: &Path,
    dest: &Path,
    units: &[String],
    stamp_secs: i64,
    data_bin: Option<&Path>,
) -> Result<Made, String> {
    let mut wants = vec![];
    for u in units {
        wants.extend(
            plan(u).ok_or_else(|| {
                format!("no backup plan for unit {u} (known: tuner, galion-nerve)")
            })?,
        );
    }
    wants.extend(common());
    let dest_abs = dest.to_string_lossy().into_owned();
    for want in &wants {
        if let Some(p) = under(root, &want.path) {
            if dest.starts_with(&p) {
                return Err(format!(
                    "destination {dest_abs} is inside captured path {}",
                    want.path
                ));
            }
        }
    }
    let c = collect(root, &wants)?;
    let stamp = zagreb::compact(stamp_secs);
    std::fs::create_dir_all(dest).map_err(|e| format!("{dest_abs}: {e}"))?;
    let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o700));
    let dir = dest.join(&stamp);
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;

    let mut buf = vec![];
    for e in &c.entries {
        tar::write_entry(&mut buf, e).map_err(|e| e.to_string())?;
    }
    tar::finish(&mut buf).map_err(|e| e.to_string())?;
    let tar_sha = sha256::hex(&buf);
    write_file(&dir.join("backup.tar"), &buf).map_err(|e| e.to_string())?;

    let data = match data_bin {
        Some(b) => Some(run_data_backup(b, &dir)?),
        None => None,
    };
    let data_line = match &data {
        None => {
            "# data: skipped (--no-data); the periodic galion-backup archive is the data backup\n"
                .to_string()
        }
        Some(DataPart {
            exit,
            archive: Some((n, b, s)),
        }) => {
            format!("# data: galion-backup run exit {exit}, data/{n} {b} B sha256 {s}\n")
        }
        Some(DataPart {
            exit,
            archive: None,
        }) => {
            format!(
                "# data: galion-backup run exit {exit}, NO archive (see data/galion-backup.log)\n"
            )
        }
    };

    let mut m = String::new();
    m.push_str(&format!(
        "# galion-deploy backup {}\n# created {}\n# root {}\n# units {}\n# overlay backup.tar {} B sha256 {tar_sha}\n{data_line}# restore one overlay path: tar -xpf backup.tar -C / <path without leading />\n# type\tmode\tuid\tgid\tsize\tmtime\tsha256\tpath\n",
        env!("CARGO_PKG_VERSION"),
        zagreb::iso_seconds(stamp_secs),
        root.display(),
        units.join(","),
        buf.len()
    ));
    for r in &c.rows {
        m.push_str(&r.line());
        m.push('\n');
    }
    for n in &c.notes {
        m.push_str(&format!("# note: {n}\n"));
    }
    write_file(&dir.join("MANIFEST.tsv"), m.as_bytes()).map_err(|e| e.to_string())?;
    let mut sums = format!(
        "{tar_sha}  backup.tar\n{}  MANIFEST.tsv\n",
        sha256::hex(m.as_bytes())
    );
    if let Some(DataPart {
        archive: Some((n, _, s)),
        ..
    }) = &data
    {
        sums.push_str(&format!("{s}  data/{n}\n"));
    }
    write_file(&dir.join("SHA256SUMS"), sums.as_bytes()).map_err(|e| e.to_string())?;
    let v = verify(&dir, None);
    let text = v.report();
    write_file(&dir.join("VERIFY.txt"), text.as_bytes()).map_err(|e| e.to_string())?;
    if !v.ok() {
        return Err(format!(
            "verify failed right after writing {}:\n{text}",
            dir.display()
        ));
    }
    if let Some(d) = &data {
        if d.exit != 0 || d.archive.is_none() {
            return Err(format!(
                "overlay written and verified in {}, but galion-backup run exited {} (archive: {}); see data/galion-backup.log",
                dir.display(),
                d.exit,
                d.archive.as_ref().map(|a| a.0.as_str()).unwrap_or("none")
            ));
        }
    }
    Ok(Made {
        dir,
        rows: c.rows,
        notes: c.notes,
        tar_sha,
        tar_bytes: buf.len() as u64,
        data,
    })
}

#[derive(Default)]
pub struct Verified {
    pub checked: usize,
    pub problems: Vec<String>,
    /// Live files that differ from the backup (informational).
    pub drift: Vec<String>,
}

impl Verified {
    pub fn ok(&self) -> bool {
        self.problems.is_empty() && self.checked > 0
    }

    pub fn report(&self) -> String {
        let mut s = format!(
            "verified {} {} member(s) against MANIFEST.tsv and SHA256SUMS: {}\n",
            zagreb::iso_seconds(zagreb::now_secs()),
            self.checked,
            if self.ok() { "OK" } else { "FAILED" }
        );
        for p in &self.problems {
            s.push_str(&format!("problem: {p}\n"));
        }
        for d in &self.drift {
            s.push_str(&format!("live drift: {d}\n"));
        }
        s
    }
}

/// Check a backup dir: SHA256SUMS, then every tar member against its manifest
/// line (type, mode, size, sha256), and nothing missing or extra. With
/// `live_root`, also report live files that changed since (drift, not failure).
pub fn verify(dir: &Path, live_root: Option<&Path>) -> Verified {
    let mut v = Verified::default();
    let read = |n: &str| std::fs::read(dir.join(n));
    let (tar_b, man_b, sums_b) =
        match (read("backup.tar"), read("MANIFEST.tsv"), read("SHA256SUMS")) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            _ => {
                v.problems
                    .push("backup.tar, MANIFEST.tsv or SHA256SUMS missing".into());
                return v;
            }
        };
    let sums = String::from_utf8_lossy(&sums_b);
    for (name, data) in [("backup.tar", &tar_b), ("MANIFEST.tsv", &man_b)] {
        let want = sums
            .lines()
            .find(|l| l.ends_with(&format!("  {name}")))
            .and_then(|l| l.split_whitespace().next());
        if want != Some(sha256::hex(data).as_str()) {
            v.problems
                .push(format!("{name}: sha256 does not match SHA256SUMS"));
        }
    }
    let entries = match tar::read_all(&mut tar_b.as_slice()) {
        Ok(e) => e,
        Err(e) => {
            v.problems.push(format!("backup.tar unreadable: {e}"));
            return v;
        }
    };
    let man = String::from_utf8_lossy(&man_b);
    let rows: Vec<Row> = man
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(Row::parse)
        .collect();
    if rows.len() != entries.len() {
        v.problems.push(format!(
            "manifest has {} rows, tar has {} members",
            rows.len(),
            entries.len()
        ));
    }
    for (r, e) in rows.iter().zip(entries.iter()) {
        v.checked += 1;
        let path = format!("/{}", e.name);
        if r.path != path || r.kind != e.kind || r.mode & 0o7777 != e.mode & 0o7777 {
            v.problems.push(format!(
                "{}: manifest/tar mismatch (path, type or mode)",
                r.path
            ));
            continue;
        }
        match e.kind {
            Kind::File => {
                if r.size != e.data.len() as u64 || r.sum != sha256::hex(&e.data) {
                    v.problems.push(format!(
                        "{}: size or sha256 differs from the manifest",
                        r.path
                    ));
                }
            }
            Kind::Symlink => {
                if r.sum != format!("->{}", e.link) {
                    v.problems
                        .push(format!("{}: symlink target differs", r.path));
                }
            }
            Kind::Dir => {}
        }
        if let (Some(root), Kind::File) = (live_root, e.kind) {
            if let Some(p) = under(root, &r.path) {
                match file_sum(&p) {
                    Ok((h, n)) if h == r.sum && n == r.size => {}
                    Ok((h, n)) => v
                        .drift
                        .push(format!("{} now {n} B sha {}", r.path, &h[..12])),
                    Err(e) => v.drift.push(format!("{} now unreadable: {e}", r.path)),
                }
            }
        }
    }
    v
}

pub const USAGE: &str = "\
usage: galion-deploy backup [--root DIR] [--dest DIR] [--unit NAME]... [--no-data | --galion-backup BIN]
       galion-deploy backup-verify DIR [--live-root DIR]

backup: pre-cutover snapshot into <dest>/<YYYYMMDD-HHMMSS+zzzz>/ (Europe/Zagreb),
default dest /opt/galion/backups, default units tuner galion-nerve. The data
part is `galion-backup run` (one definition of a backup, swarm/rust-ports;
default /opt/galion/rust/bin/galion-backup, env GALION_DEPLOY_GALION_BACKUP)
into <snapshot>/data/. galion-deploy adds only the cutover overlay (binaries,
unit files + drop-ins, state dirs, static files, the deploy tool), writes
MANIFEST.tsv + SHA256SUMS and verifies. It never reads secrets itself.
exit: 0 ok | 1 failed (overlay may be written; see the log) | 2 usage
";

/// `galion-deploy backup ...` (args after `backup`).
pub fn main_backup(args: &[String]) -> i32 {
    let mut root = PathBuf::from("/");
    let mut dest: Option<PathBuf> = None;
    let mut units = vec![];
    let mut data_bin = Some(PathBuf::from(
        std::env::var("GALION_DEPLOY_GALION_BACKUP")
            .unwrap_or_else(|_| "/opt/galion/rust/bin/galion-backup".into()),
    ));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = it.next().map(PathBuf::from).unwrap_or(root),
            "--dest" => dest = it.next().map(PathBuf::from),
            "--no-data" => data_bin = None,
            "--galion-backup" => data_bin = it.next().map(PathBuf::from),
            "--unit" => units.extend(
                it.next()
                    .map(|u| u.trim_end_matches(".service").to_string()),
            ),
            _ => {
                eprint!("galion-deploy backup: unknown argument {a}\n{USAGE}");
                return 2;
            }
        }
    }
    if units.is_empty() {
        units = vec!["tuner".into(), "galion-nerve".into()];
    }
    let dest = dest.unwrap_or_else(|| root.join("opt/galion/backups"));
    match make(
        &root,
        &dest,
        &units,
        zagreb::now_secs(),
        data_bin.as_deref(),
    ) {
        Ok(m) => {
            log(&format!(
                "level=info msg=\"backup written and verified\" dir={} members={} tar_bytes={} tar_sha256={}",
                m.dir.display(),
                m.rows.len(),
                m.tar_bytes,
                m.tar_sha
            ));
            if let Some(d) = &m.data {
                log(&format!(
                    "level=info msg=\"data part: galion-backup run exit {}\" archive={}",
                    d.exit,
                    d.archive.as_ref().map(|a| a.0.as_str()).unwrap_or("none")
                ));
            }
            for n in &m.notes {
                log(&format!("level=info msg=\"{}\"", n.replace('"', "'")));
            }
            0
        }
        Err(e) => {
            log(&format!(
                "level=error msg=\"backup failed: {}\"",
                e.replace('"', "'")
            ));
            1
        }
    }
}

/// `galion-deploy backup-verify DIR [--live-root DIR]`.
pub fn main_verify(args: &[String]) -> i32 {
    let (dir, live) = match args {
        [d] => (d, None),
        [d, f, r] if f == "--live-root" => (d, Some(PathBuf::from(r))),
        _ => {
            eprint!("{USAGE}");
            return 2;
        }
    };
    let v = verify(Path::new(dir), live.as_deref());
    let _ = std::io::stdout().lock().write_all(v.report().as_bytes());
    if v.ok() {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_refused_by_path() {
        assert!(secret_reason("/opt/galion/galion-light/secrets").is_some());
        assert!(secret_reason("/opt/galion/galion-light/secrets/x").is_some());
        assert!(secret_reason("/root/galion-secrets/k").is_some());
        assert!(secret_reason("/opt/galion/hermes/.env").is_some());
        assert!(secret_reason("/etc/openmind-memory.env").is_some());
        assert!(secret_reason("/var/lib/x/api_token").is_some());
        assert!(secret_reason("/opt/galion/galion-light/secretsXX/a").is_some()); // name contains "secret"
        assert!(secret_reason("/opt/galion/tuner/bin/tuner").is_none());
        assert!(secret_reason("/var/lib/galion-nerve/state.json").is_none());
    }

    #[test]
    fn plans_cover_binary_unit_and_state() {
        let t = plan("tuner").unwrap();
        assert!(t
            .iter()
            .any(|w| w.path == "/opt/galion/tuner/bin/tuner" && w.required));
        let n = plan("galion-nerve").unwrap();
        assert!(n
            .iter()
            .any(|w| w.path == "/var/lib/galion-nerve" && w.required));
        assert!(n
            .iter()
            .any(|w| w.path.ends_with(".service.d") && !w.required));
        assert!(plan("hermes-rs").is_none());
        for w in t.iter().chain(n.iter()).chain(common().iter()) {
            assert!(secret_reason(&w.path).is_none(), "{}", w.path);
        }
    }

    #[test]
    fn manifest_rows_roundtrip() {
        let r = Row {
            kind: Kind::File,
            mode: 0o755,
            uid: 0,
            gid: 0,
            size: 508_680,
            mtime: 1_760_814_000,
            sum: "ecad72a4".into(),
            path: "/opt/galion/tuner/bin/tuner".into(),
        };
        let back = Row::parse(&r.line()).unwrap();
        assert_eq!(back, Row { mtime: 0, ..r });
        assert!(Row::parse("# comment").is_none());
    }
}
