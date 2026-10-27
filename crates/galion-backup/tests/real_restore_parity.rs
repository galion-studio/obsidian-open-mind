// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life RESTORE round trip (GALION_REAL=1), slot 279 backup-restore-parity.
//!
//! `real_parity.rs` proves the Rust `galion-backup run` writes the same
//! archive as the bash galion-backup.sh. This test proves the archives are
//! actually *restorable* and that a restore from either one gives back the
//! same machine state:
//!
//! * R1 both archives: `gzip -t`, GNU tar extract, `sha256sum -c SHA256SUMS`.
//! * R2 restore into an empty root (`tar -xpf files.tar` + copy `db/*` per
//!   `db/MAP`), then diff against the source tree: bytes, modes, uid/gid,
//!   mtimes, symlink targets, hard links, fifos, excludes, nothing extra.
//! * R3 DBs: integrity_check `ok` via `galion-backup verify`, logical dump
//!   (every row of every table) equals the live source, including a row that
//!   only lives in the uncheckpointed WAL of a connection held open.
//! * R4 restore(bash) == restore(rust), whole tree incl. DB bytes.
//! * R5 GNU-free restore: miniz_oxide inflate + galion-tar reader read both
//!   archives (cross-tool) and every member matches the source.
//! * R6 disaster round trip: wipe the source tree, restore it from the Rust
//!   archive in place, back it up again with Rust: same files.tar bytes and
//!   same DB contents (backup -> restore -> backup is a fixed point).
//! * R7 negative: a flipped byte fails `sha256sum -c`; a truncated archive
//!   fails `gzip -t` / extraction (the checks above are not vacuous).
//!
//! Same unprivileged `unshare -rm` + chroot jail as real_parity.rs; nothing
//! touches the host's /var, /opt, /etc, /root or the VPS. No ports used.
//!
//! `GALION_REAL=1 cargo test --release -p galion-backup --test real_restore_parity -- --nocapture`
//! (`GALION_BACKUP_BIN=<path>` tests a given ELF, e.g. the deployed
//! galion-backup-98466ab static musl build.)

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_galion-backup");
const ORACLE_REV: &str = "e43d7ab:deploy/foundation/galion-backup.sh";
const ORACLE_SHA256: &str = "0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4";
const MARK: &str = "GALION_BACKUP_RESTORE_ROOT";
const WAL_MARKER: &str = "wal-only-marker-279";

fn say(m: &str) {
    println!("[{}] {m}", galion_backup::backup::now_iso());
}

fn sha(b: &[u8]) -> String {
    let mut h = galion_backup::sha256::Sha256::new();
    h.update(b);
    galion_backup::sha256::hex(&h.finish())
}

fn oracle() -> Option<Vec<u8>> {
    if let Ok(p) = std::env::var("GALION_BACKUP_ORACLE") {
        return fs::read(p).ok();
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["show", ORACLE_REV])
        .output()
        .ok()?;
    o.status.success().then_some(o.stdout)
}

fn bin_under_test() -> String {
    std::env::var("GALION_BACKUP_BIN").unwrap_or_else(|_| BIN.to_string())
}

#[test]
fn real_restore_round_trip() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let Some(script) = oracle() else {
        eprintln!("SKIP: oracle {ORACLE_REV} not in this repo (set GALION_BACKUP_ORACLE)");
        return;
    };
    assert_eq!(sha(&script), ORACLE_SHA256, "oracle is the VPS script");
    let f = common::tmp("restore");
    for d in [
        "usr",
        "dev",
        "proc",
        "tmp",
        "var/backups",
        "opt/galion/rust/bin",
        "etc",
    ] {
        fs::create_dir_all(f.join(d)).unwrap();
    }
    fs::set_permissions(f.join("tmp"), fs::Permissions::from_mode(0o1777)).unwrap();
    for (l, t) in [
        ("bin", "usr/bin"),
        ("sbin", "usr/sbin"),
        ("lib", "usr/lib"),
        ("lib64", "usr/lib64"),
    ] {
        std::os::unix::fs::symlink(t, f.join(l)).unwrap();
    }
    for e in ["passwd", "group", "nsswitch.conf"] {
        let _ = fs::copy(Path::new("/etc").join(e), f.join("etc").join(e));
    }
    let bin = bin_under_test();
    let bytes = fs::read(&bin).unwrap();
    say(&format!(
        "rust binary under test: {bin} ({} B, sha256 {})",
        bytes.len(),
        sha(&bytes)
    ));
    let bd = f.join("opt/galion/rust/bin");
    fs::write(bd.join("galion-backup"), &bytes).unwrap();
    fs::set_permissions(bd.join("galion-backup"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(bd.join("galion-backup.sh"), &script).unwrap();
    fs::set_permissions(
        bd.join("galion-backup.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    // GALION_RESTORE_TREE=vps: the VPS-sized tree (46 MB DBs, 1845 members,
    // a WAL held open by this process like the memory gateway does).
    let vps = std::env::var("GALION_RESTORE_TREE").as_deref() == Ok("vps");
    let _held = if vps {
        common::vps_tree::build_vps_sized_tree(&f)
    } else {
        common::build_tree(&f);
        Vec::new()
    };
    say(&format!(
        "tree: {}",
        if vps {
            "vps-sized"
        } else {
            "fake (edge cases)"
        }
    ));

    let me = std::env::current_exe().unwrap();
    let st = Command::new("unshare")
        .args(["-rm", "--propagation", "private"])
        .arg(&me)
        .args([
            "--exact",
            "real_restore_inner",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MARK, &f)
        .env("GALION_RESTORE_TREE", if vps { "vps" } else { "fake" })
        .status()
        .expect("unshare");
    if st.success() {
        let _ = fs::remove_dir_all(&f);
    }
    assert!(
        st.success(),
        "inner restore run failed (tree kept at {})",
        f.display()
    );
}

// ------------------------------------------------------------------ inner

fn bind(src: &str, dst: &Path) {
    let s = std::ffi::CString::new(src).unwrap();
    let d = std::ffi::CString::new(dst.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: valid C strings; plain recursive bind mount in our own mount ns.
    let rc = unsafe {
        libc::mount(
            s.as_ptr(),
            d.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REC,
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "bind {src}: {}", std::io::Error::last_os_error());
}

struct Jail {
    root: PathBuf,
}

impl Jail {
    fn host(&self, p: &str) -> PathBuf {
        self.root.join(p.trim_start_matches('/'))
    }

    fn run(&self, bash: bool, dest: &str) -> Output {
        let argv: &[&str] = if bash {
            &["/bin/bash", "/opt/galion/rust/bin/galion-backup.sh"]
        } else {
            &["/opt/galion/rust/bin/galion-backup", "run"]
        };
        let mut c = Command::new(argv[0]);
        c.args(&argv[1..])
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .env("LANG", "C")
            .env("GALION_BACKUP_DEST", dest)
            .stdin(Stdio::null());
        let root = std::ffi::CString::new(self.root.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: only async-signal-safe syscalls between fork and exec.
        unsafe {
            c.pre_exec(move || {
                if libc::chroot(root.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::umask(0o022);
                Ok(())
            });
        }
        c.output().unwrap()
    }
}

struct Tally {
    pass: u32,
    fail: u32,
}

impl Tally {
    fn check(&mut self, what: &str, ok: bool, detail: impl FnOnce() -> String) {
        if ok {
            self.pass += 1;
            say(&format!("PASS {what}"));
        } else {
            self.fail += 1;
            say(&format!("FAIL {what}\n{}", detail()));
        }
    }
}

fn sh(args: &[&str], cwd: Option<&Path>) -> Output {
    let mut c = Command::new(args[0]);
    c.args(&args[1..]).env("LANG", "C");
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.output().unwrap()
}

fn sh_ok(args: &[&str], cwd: Option<&Path>) -> bool {
    sh(args, cwd).status.success()
}

fn p(x: &Path) -> &str {
    x.to_str().unwrap()
}

/// One filesystem entry as restore cares about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ent {
    kind: char,
    mode: u32,
    uid: u32,
    gid: u32,
    mtime: i64,
    data: Vec<u8>,
    /// For files with nlink > 1: the sorted list of relative paths sharing the inode.
    links: Vec<String>,
}

/// Entries under `base` for the operand-relative paths in `rels`.
fn snapshot_tree(base: &Path, rels: &[String], skip_db: bool) -> BTreeMap<String, Ent> {
    let mut m = BTreeMap::new();
    let mut inodes: BTreeMap<(u64, u64), Vec<String>> = BTreeMap::new();
    fn walk(
        base: &Path,
        rel: &str,
        skip_db: bool,
        m: &mut BTreeMap<String, Ent>,
        ino: &mut BTreeMap<(u64, u64), Vec<String>>,
    ) {
        let path = base.join(rel);
        let Ok(md) = fs::symlink_metadata(&path) else {
            return;
        };
        if skip_db && excluded(rel) {
            return;
        }
        let ft = md.file_type();
        let (kind, data) = if ft.is_dir() {
            ('d', vec![])
        } else if ft.is_symlink() {
            (
                'l',
                fs::read_link(&path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec(),
            )
        } else if ft.is_fifo() {
            ('p', vec![])
        } else if ft.is_file() {
            ('f', fs::read(&path).unwrap())
        } else {
            ('?', vec![])
        };
        if kind == 'f' && md.nlink() > 1 {
            ino.entry((md.dev(), md.ino()))
                .or_default()
                .push(rel.to_string());
        }
        m.insert(
            rel.to_string(),
            Ent {
                kind,
                mode: md.mode() & 0o7777,
                uid: md.uid(),
                gid: md.gid(),
                mtime: if kind == 'l' { 0 } else { md.mtime() },
                data,
                links: vec![],
            },
        );
        if kind == 'd' {
            let mut kids: Vec<String> = fs::read_dir(&path)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            kids.sort();
            for k in kids {
                walk(base, &format!("{rel}/{k}"), skip_db, m, ino);
            }
        }
    }
    for r in rels {
        walk(base, r, skip_db, &mut m, &mut inodes);
    }
    for (_, mut v) in inodes {
        v.sort();
        for r in &v {
            if let Some(e) = m.get_mut(r) {
                e.links = v.clone();
            }
        }
    }
    m
}

/// GNU tar `--exclude='*.db' '*.db-wal' '*.db-shm' '.backup'` (unanchored).
fn excluded(rel: &str) -> bool {
    rel.split('/').any(|c| {
        c.ends_with(".db") || c.ends_with(".db-wal") || c.ends_with(".db-shm") || c == ".backup"
    })
}

fn first_diff(a: &BTreeMap<String, Ent>, b: &BTreeMap<String, Ent>) -> String {
    let mut out = Vec::new();
    for (k, v) in a {
        match b.get(k) {
            None => out.push(format!("missing in restore: {k}")),
            Some(w) if w != v => out.push(format!(
                "differs: {k} kind {}/{} mode {:o}/{:o} uid {}/{} mtime {}/{} len {}/{} links {:?}/{:?}",
                v.kind, w.kind, v.mode, w.mode, v.uid, w.uid, v.mtime, w.mtime,
                v.data.len(), w.data.len(), v.links, w.links
            )),
            _ => {}
        }
        if out.len() > 8 {
            break;
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            out.push(format!("extra in restore: {k}"));
        }
    }
    out.join("\n")
}

/// Every row of every user table, in rowid order (logical DB content).
fn dump(db: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let mut tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    tables.sort();
    let mut out = Vec::new();
    for t in tables {
        let mut s = c
            .prepare(&format!("SELECT rowid, * FROM \"{t}\" ORDER BY rowid"))
            .unwrap();
        let n = s.column_count();
        let rows = s
            .query_map([], |r| {
                let mut v = Vec::with_capacity(n);
                for i in 0..n {
                    v.push(format!("{:?}", r.get_ref(i).unwrap()));
                }
                Ok(v.join("|"))
            })
            .unwrap();
        for r in rows {
            out.push(format!("{t}:{}", r.unwrap()));
        }
    }
    out
}

fn the_archive(dir: &Path) -> PathBuf {
    let v = galion_backup::backup::archives(dir);
    assert_eq!(v.len(), 1, "one archive in {}", dir.display());
    v[0].clone()
}

/// Extract outer archive with GNU tar; returns the galion-STAMP dir.
fn extract_outer(a: &Path, into: &Path) -> Option<PathBuf> {
    fs::create_dir_all(into).unwrap();
    if !sh_ok(&["tar", "-xpzf", p(a), "-C", p(into)], None) {
        return None;
    }
    let top = fs::read_dir(into)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|q| {
            q.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("galion-")
        })?;
    Some(top)
}

fn read_map(top: &Path) -> Vec<(String, String)> {
    fs::read_to_string(top.join("db/MAP"))
        .unwrap()
        .lines()
        .map(|l| {
            let (a, b) = l.split_once(' ').unwrap();
            (a.to_string(), b.to_string())
        })
        .collect()
}

/// The restore procedure into `root`: DBs first (per db/MAP, stale
/// -wal/-shm removed), THEN `tar -xpf files.tar`, so GNU tar sets the
/// directory mtimes last (DBs copied after tar would bump them).
fn restore_into(top: &Path, root: &Path) -> bool {
    fs::create_dir_all(root).unwrap();
    let mut ok = true;
    for (src, name) in read_map(top) {
        let dst = root.join(src.trim_start_matches('/'));
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        for x in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{x}", dst.display()));
        }
        ok &= fs::copy(top.join("db").join(&name), &dst).is_ok();
    }
    ok &= sh_ok(
        &[
            "tar",
            "-xpf",
            p(&top.join("files/files.tar")),
            "-C",
            p(root),
        ],
        None,
    );
    ok
}

fn operands(j: &Jail) -> Vec<String> {
    let mut v: Vec<String> = galion_backup::backup::FILES
        .iter()
        .map(|s| s.trim_start_matches('/').to_string())
        .collect();
    let dns = j.host(galion_backup::backup::DNS_DIR);
    let mut g: Vec<String> = fs::read_dir(&dns)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("dns-backup-") && n.ends_with(".json"))
        .filter(|n| dns.join(n).exists()) // [ -e ] drops the dangling one
        .collect();
    g.sort();
    for n in g {
        v.push(format!(
            "{}/{n}",
            galion_backup::backup::DNS_DIR.trim_start_matches('/')
        ));
    }
    v.retain(|r| fs::symlink_metadata(j.root.join(r)).is_ok());
    v
}

/// GNU-free read of an archive: inflate (miniz_oxide) + galion-tar reader.
/// Returns (outer member names, files.tar members name->(kind,mode,data/link)).
#[allow(clippy::type_complexity)]
fn rust_read(a: &Path) -> Result<(Vec<String>, BTreeMap<String, (char, u32, Vec<u8>)>), String> {
    let gz = fs::read(a).map_err(|e| e.to_string())?;
    // gzip member: 10-byte header (+ optional FNAME etc.), deflate, 8-byte trailer.
    if gz.len() < 18 || gz[0] != 0x1f || gz[1] != 0x8b || gz[2] != 8 {
        return Err("not gzip".into());
    }
    let flg = gz[3];
    let mut i = 10usize;
    if flg & 4 != 0 {
        let xl = u16::from_le_bytes([gz[i], gz[i + 1]]) as usize;
        i += 2 + xl;
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            while gz[i] != 0 {
                i += 1;
            }
            i += 1;
        }
    }
    if flg & 2 != 0 {
        i += 2;
    }
    let tar = miniz_oxide::inflate::decompress_to_vec(&gz[i..gz.len() - 8])
        .map_err(|e| format!("inflate: {e:?}"))?;
    let isize = u32::from_le_bytes(gz[gz.len() - 4..].try_into().unwrap());
    if isize != tar.len() as u32 {
        return Err("ISIZE mismatch".into());
    }
    let outer = galion_tar::read::read_all(&tar[..]).map_err(|e| e.to_string())?;
    let names: Vec<String> = outer.iter().map(|(m, _)| m.name_lossy()).collect();
    let ft = outer
        .iter()
        .find(|(m, _)| m.name_lossy().ends_with("files/files.tar"))
        .ok_or("no files.tar")?;
    let inner = galion_tar::read::read_all(&ft.1[..]).map_err(|e| e.to_string())?;
    let mut m = BTreeMap::new();
    for (mem, data) in inner {
        use galion_tar::read::MemberKind as K;
        let (k, d) = match mem.kind {
            K::File => ('f', data),
            K::Hardlink => ('h', mem.link.clone()),
            K::Symlink => ('l', mem.link.clone()),
            K::Dir => ('d', vec![]),
            K::Fifo => ('p', vec![]),
            _ => ('?', vec![]),
        };
        m.insert(
            mem.name_lossy().trim_end_matches('/').to_string(),
            (k, mem.mode & 0o7777, d),
        );
    }
    Ok((names, m))
}

#[test]
fn real_restore_inner() {
    let Ok(root) = std::env::var(MARK) else {
        return; // only meaningful when re-executed by real_restore_round_trip
    };
    let j = Jail {
        root: PathBuf::from(&root),
    };
    fs::create_dir_all(j.root.join("etc/alternatives")).unwrap();
    for (s, d) in [
        ("/usr", "usr"),
        ("/dev", "dev"),
        ("/proc", "proc"),
        ("/etc/alternatives", "etc/alternatives"),
    ] {
        bind(s, &j.root.join(d));
    }
    let vps = std::env::var("GALION_RESTORE_TREE").as_deref() == Ok("vps");
    let bin = j.host("/opt/galion/rust/bin/galion-backup");
    let mut t = Tally { pass: 0, fail: 0 };

    // A committed row that lives only in the WAL of a connection held open
    // across both backups (autocheckpoint off): must survive the round trip.
    let rsdb = j.host("/var/lib/hermes-rs/rs-state.db");
    let holder = rusqlite::Connection::open(&rsdb).unwrap();
    holder
        .execute_batch(&format!(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE IF NOT EXISTS restore_marker(v TEXT); INSERT INTO restore_marker(v) VALUES ('{WAL_MARKER}');"
        ))
        .unwrap();
    let wal_len = fs::metadata(format!("{}-wal", rsdb.display()))
        .map(|m| m.len())
        .unwrap_or(0);
    say(&format!("   rs-state.db-wal held open: {wal_len} B"));
    t.check(
        "WAL marker setup: -wal file non-empty",
        wal_len > 0,
        String::new,
    );

    let ops = operands(&j);
    say(&format!(
        "   {} tar operands present in the jail",
        ops.len()
    ));
    let src_tree = snapshot_tree(&j.root, &ops, true);
    let src_dbs: BTreeMap<String, Vec<String>> = galion_backup::backup::DBS
        .iter()
        .map(|(s, _)| (s.to_string(), dump(&j.host(s))))
        .collect();
    say(&format!(
        "   source: {} file-tree entries, {} DBs, {} rows total",
        src_tree.len(),
        src_dbs.len(),
        src_dbs.values().map(|v| v.len()).sum::<usize>()
    ));

    // ---------------------------------------------------------- backups
    let ob = j.run(true, "/var/backups/sh");
    let or = j.run(false, "/var/backups/rs");
    for (n, o) in [("bash", &ob), ("rust", &or)] {
        say(&format!(
            "   {n}: exit {:?} {}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim()
        ));
    }
    t.check(
        "both backups exit 0",
        ob.status.success() && or.status.success(),
        || format!("{ob:?}\n{or:?}"),
    );
    drop(holder);

    let mut restored: BTreeMap<&str, PathBuf> = BTreeMap::new();
    let mut tops: BTreeMap<&str, PathBuf> = BTreeMap::new();
    for side in ["sh", "rs"] {
        let a = the_archive(&j.host(&format!("/var/backups/{side}")));
        say(&format!(
            "== {side}: {} ({} B)",
            a.file_name().unwrap().to_string_lossy(),
            fs::metadata(&a).unwrap().len()
        ));
        // R1
        t.check(
            &format!("R1 {side}: gzip -t"),
            sh_ok(&["gzip", "-t", p(&a)], None),
            String::new,
        );
        let x = j.root.join(format!("tmp/x-{side}"));
        let top = extract_outer(&a, &x).expect("outer extract");
        t.check(
            &format!("R1 {side}: GNU tar outer extract"),
            true,
            String::new,
        );
        let c = sh(&["sha256sum", "-c", "--strict", "SHA256SUMS"], Some(&top));
        let lines = String::from_utf8_lossy(&c.stdout).lines().count();
        let oks = String::from_utf8_lossy(&c.stdout)
            .lines()
            .filter(|l| l.ends_with(": OK"))
            .count();
        say(&format!("   sha256sum -c: {oks}/{lines} OK"));
        t.check(
            &format!("R1 {side}: sha256sum -c SHA256SUMS all OK (files.tar, MAP, 4 DBs)"),
            c.status.success() && lines == 6 && oks == 6,
            || {
                String::from_utf8_lossy(&c.stdout).into_owned()
                    + &String::from_utf8_lossy(&c.stderr)
            },
        );
        // R2
        let r = j.root.join(format!("tmp/restore-{side}"));
        t.check(
            &format!("R2 {side}: restore (tar -xpf files.tar + db/MAP copy)"),
            restore_into(&top, &r),
            String::new,
        );
        let got = snapshot_tree(&r, &ops, true);
        t.check(
            &format!("R2 {side}: restored file tree == source ({} entries: bytes, modes, uid/gid, mtimes, symlinks, hard links, fifo; excludes honoured)", src_tree.len()),
            got == src_tree,
            || first_diff(&src_tree, &got),
        );
        let excl_in_restore: Vec<String> = snapshot_tree(&r, &ops, false)
            .keys()
            .filter(|k| {
                excluded(k)
                    && !galion_backup::backup::DBS
                        .iter()
                        .any(|(s, _)| s.trim_start_matches('/') == k.as_str())
            })
            .cloned()
            .collect();
        t.check(
            &format!("R2 {side}: no excluded member (.backup, dir.db/, -wal, -shm) restored"),
            excl_in_restore.is_empty(),
            || format!("{excl_in_restore:?}"),
        );
        // R3
        // Checked on scratch copies: opening a WAL DB creates -wal/-shm and
        // would bump the restored directory mtimes compared in R4.
        let scratch = j.root.join(format!("tmp/r3-{side}"));
        fs::create_dir_all(&scratch).unwrap();
        for (src, name) in galion_backup::backup::DBS {
            let db = scratch.join(name);
            fs::copy(r.join(src.trim_start_matches('/')), &db).unwrap();
            let v = sh(&[p(&bin), "verify", p(&db)], None);
            let rows = dump(&db);
            t.check(
                &format!(
                    "R3 {side}: {src} integrity ok + {} rows == live source",
                    rows.len()
                ),
                v.status.success()
                    && String::from_utf8_lossy(&v.stdout).contains("integrity=ok")
                    && Some(&rows) == src_dbs.get(*src),
                || {
                    format!(
                        "{:?} rows {} vs {:?}",
                        v,
                        rows.len(),
                        src_dbs.get(*src).map(|x| x.len())
                    )
                },
            );
        }
        let marker: i64 = rusqlite::Connection::open(scratch.join("hermes-rs-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM restore_marker WHERE v = ?1",
                [WAL_MARKER],
                |x| x.get(0),
            )
            .unwrap();
        let marker = marker == 1;
        t.check(
            &format!("R3 {side}: WAL-only row restored"),
            marker,
            String::new,
        );
        restored.insert(side, r);
        tops.insert(side, top);
    }

    // R4
    let all_rel: Vec<String> = {
        let mut v = ops.clone();
        v.extend(
            galion_backup::backup::DBS
                .iter()
                .map(|(s, _)| s.trim_start_matches('/').to_string()),
        );
        v
    };
    let (tb, tr) = (
        snapshot_tree(&restored["sh"], &all_rel, false),
        snapshot_tree(&restored["rs"], &all_rel, false),
    );
    let db_bytes_eq = galion_backup::backup::DBS.iter().all(|(s, _)| {
        let k = s.trim_start_matches('/');
        tb.get(k).map(|e| &e.data) == tr.get(k).map(|e| &e.data)
    });
    let strip = |m: &BTreeMap<String, Ent>| -> BTreeMap<String, Ent> {
        m.iter()
            .map(|(k, e)| {
                let mut e = e.clone();
                if galion_backup::backup::DBS
                    .iter()
                    .any(|(s, _)| s.trim_start_matches('/') == k)
                {
                    e.mtime = 0; // copied at restore time
                }
                (k.clone(), e)
            })
            .collect()
    };
    t.check(
        &format!(
            "R4 restore(bash) == restore(rust): {} entries incl. 4 DBs",
            tb.len()
        ),
        strip(&tb) == strip(&tr),
        || first_diff(&strip(&tb), &strip(&tr)),
    );
    say(&format!(
        "   R4 DB files byte-identical across implementations: {db_bytes_eq}"
    ));

    // R5 GNU-free cross-tool read
    for side in ["sh", "rs"] {
        let a = the_archive(&j.host(&format!("/var/backups/{side}")));
        match rust_read(&a) {
            Err(e) => t.check(
                &format!("R5 {side}: miniz_oxide + galion-tar read"),
                false,
                || e,
            ),
            Ok((outer, inner)) => {
                let mut bad = Vec::new();
                let mut files = 0;
                for (k, e) in &src_tree {
                    match inner.get(k) {
                        None => bad.push(format!("missing {k}")),
                        Some((kind, mode, data)) => {
                            let ok = match (e.kind, *kind) {
                                ('f', 'f') => {
                                    files += 1;
                                    data == &e.data
                                }
                                // hard link: second name points at the first
                                ('f', 'h') => {
                                    e.links.len() > 1
                                        && e.links.iter().any(|l| l.as_bytes() == data.as_slice())
                                }
                                ('l', 'l') => data == &e.data,
                                ('d', 'd') | ('p', 'p') => true,
                                _ => false,
                            } && (*kind == 'l' || *kind == 'h' || *mode == e.mode);
                            if !ok {
                                bad.push(format!("differs {k} ({} vs {kind})", e.kind));
                            }
                        }
                    }
                }
                let extra: Vec<&String> = inner
                    .keys()
                    .filter(|k| !src_tree.contains_key(*k))
                    .collect();
                say(&format!(
                    "   R5 {side}: outer {} members, files.tar {} members ({files} regular files byte-checked)",
                    outer.len(),
                    inner.len()
                ));
                t.check(
                    &format!("R5 {side}: GNU-free read (miniz_oxide inflate + galion-tar) matches source"),
                    bad.is_empty()
                        && extra.is_empty()
                        && outer.len() == 10
                        && (!vps || inner.len() == 1845),
                    || format!("{bad:?} extra {extra:?} outer {outer:?}"),
                );
            }
        }
    }

    // R6 disaster round trip, in place, from the Rust archive.
    say("== R6 wipe the source, restore from the Rust archive in place, back up again");
    let first_files_tar = fs::read(tops["rs"].join("files/files.tar")).unwrap();
    for r in &all_rel {
        let q = j.root.join(r);
        let _ = fs::remove_dir_all(&q).or_else(|_| fs::remove_file(&q));
        for x in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{x}", q.display()));
        }
    }
    let gone = all_rel
        .iter()
        .all(|r| fs::symlink_metadata(j.root.join(r)).is_err());
    t.check("R6 source wiped (operands + DBs gone)", gone, String::new);
    // The excluded *.db / .backup content is NOT in the archive by design; the
    // DBs come back from db/ via MAP.
    t.check(
        "R6 in-place restore",
        restore_into(&tops["rs"], &j.root),
        String::new,
    );
    let back = snapshot_tree(&j.root, &ops, true);
    t.check(
        "R6 restored live tree == original source tree",
        back == src_tree,
        || first_diff(&src_tree, &back),
    );
    let o2 = j.run(false, "/var/backups/rs2");
    t.check(
        "R6 second Rust backup of the restored tree exit 0",
        o2.status.success(),
        || format!("{o2:?}"),
    );
    let top2 = extract_outer(
        &the_archive(&j.host("/var/backups/rs2")),
        &j.root.join("tmp/x-rs2"),
    )
    .unwrap();
    let second = fs::read(top2.join("files/files.tar")).unwrap();
    t.check(
        &format!(
            "R6 backup->restore->backup fixed point: files.tar byte-identical ({} B)",
            second.len()
        ),
        second == first_files_tar,
        || format!("len {} vs {}", first_files_tar.len(), second.len()),
    );
    for (src, name) in galion_backup::backup::DBS {
        let rows = dump(&top2.join("db").join(name));
        t.check(
            &format!("R6 second backup {name}: rows == original live source"),
            Some(&rows) == src_dbs.get(*src),
            String::new,
        );
    }
    t.check(
        "R6 db/MAP identical across the round trip",
        fs::read(top2.join("db/MAP")).unwrap() == fs::read(tops["rs"].join("db/MAP")).unwrap(),
        String::new,
    );

    // R7 negative controls.
    say("== R7 tamper / truncation must be detected");
    for side in ["sh", "rs"] {
        let top = &tops[side];
        let ft = top.join("files/files.tar");
        let mut b = fs::read(&ft).unwrap();
        let mid = b.len() / 2;
        b[mid] ^= 0x01;
        fs::write(&ft, &b).unwrap();
        let c = sh(&["sha256sum", "-c", "--quiet", "SHA256SUMS"], Some(top));
        t.check(
            &format!("R7 {side}: 1 flipped bit in files.tar -> sha256sum -c fails"),
            !c.status.success()
                && String::from_utf8_lossy(&c.stdout).contains("files/files.tar: FAILED"),
            || format!("{c:?}"),
        );
        let a = the_archive(&j.host(&format!("/var/backups/{side}")));
        let mut g = Vec::new();
        fs::File::open(&a).unwrap().read_to_end(&mut g).unwrap();
        let tr = j.root.join(format!("tmp/trunc-{side}.tar.gz"));
        fs::write(&tr, &g[..g.len() / 2]).unwrap();
        t.check(
            &format!("R7 {side}: truncated archive -> gzip -t and tar -x fail"),
            !sh_ok(&["gzip", "-t", p(&tr)], None)
                && extract_outer(&tr, &j.root.join(format!("tmp/xt-{side}"))).is_none(),
            String::new,
        );
        t.check(
            &format!("R7 {side}: truncated archive -> Rust reader refuses"),
            rust_read(&tr).is_err(),
            String::new,
        );
    }

    say(&format!("RESULT: {} passed, {} failed", t.pass, t.fail));
    assert_eq!(t.fail, 0);
}
