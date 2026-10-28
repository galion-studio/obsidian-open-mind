// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life sim for wave-4 slot 427 (backup-drop-legacy-root, D-406-3
//! staged): the galion-backup library against a BOX sim root (never the VPS)
//! whose memory DBs are written by the REAL release `openmind-memory`
//! (ports 22970-22971, override `REAL_427_PORT`), in three states of the
//! legacy root `/var/lib/memory-tencentdb/vectors.db`:
//!
//! - present (real server data, or `GALION_LEGACY_ROOT_COPY=<box copy of the
//!   live root DB>`): backed up as before, same MAP order, no new log line;
//! - archived with the docs/ops/legacy-vectors-archive.md runbook (system
//!   `tar`/`sha256sum`/`mv`, literally): one `level=info` skip line naming
//!   the marker, no warn, MAP 3 lines, root never recreated;
//! - absent without marker: the old `missing db` warn (parity);
//! - F-406-2: the real server started WITHOUT `OPENMIND_MEMORY_INSTANCE_ID`
//!   recreates an empty root DB -> NOT backed up, warn line, source bytes
//!   untouched; rollback (runbook) -> backed up again.
//!
//! Instance DB snapshot row counts are checked against the live store each
//! run (unchanged). The outer test re-executes this binary for each backup
//! run so the real stdout log lines can be asserted. Only counts and hashes
//! are printed (never row content). Gated: `GALION_REAL=1`.
//! `OPENMIND_MEMORY_BIN` defaults to `<target>/release/openmind-memory`.

mod common;

use galion_backup::backup::{self, Opts};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MARK: &str = "GALION_BACKUP_427_ROOT";
const DEST_ENV: &str = "GALION_BACKUP_427_DEST";
const ROOT_DB: &str = "var/lib/memory-tencentdb/vectors.db";
const INST_DB: &str = "var/lib/memory-tencentdb/instances/galion/vectors.db";

fn say(m: &str) {
    println!("[{}] {m}", backup::now_iso());
}

fn port(off: u16) -> u16 {
    std::env::var("REAL_427_PORT")
        .ok()
        .map(|v| v.parse().expect("REAL_427_PORT must be a port"))
        .unwrap_or(22970)
        + off
}

/// Inner: one `backup::run` on the sim root; exit code = run's.
#[test]
fn real_legacy_root_drop_inner() {
    let Ok(root) = std::env::var(MARK) else {
        return;
    };
    let o = Opts {
        root: PathBuf::from(root),
        dest: PathBuf::from(std::env::var(DEST_ENV).unwrap()),
        keep: 12,
        minfree_kb: Some(1),
        gzip_level: 6,
    };
    let code = backup::run(&o);
    std::io::stdout().flush().ok();
    std::process::exit(code);
}

struct Run {
    code: i32,
    lines: Vec<String>,
    list: String,
    map: String,
}

fn run_backup(root: &Path, dest: &Path) -> Run {
    // Second-resolution archive stamps: never two runs in the same second.
    std::thread::sleep(Duration::from_millis(1100));
    let o = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_legacy_root_drop_inner",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(MARK, root)
        .env(DEST_ENV, dest)
        .output()
        .unwrap();
    let lines: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| l.contains("svc=galion-backup"))
        .map(|l| l.split_once(" svc=galion-backup ").unwrap().1.to_string())
        .collect();
    let a = backup::newest_first(backup::archives(dest))[0].clone();
    let top = a
        .file_name()
        .unwrap()
        .to_string_lossy()
        .trim_end_matches(".tar.gz")
        .to_string();
    let t = Command::new("tar").arg("-tzf").arg(&a).output().unwrap();
    assert!(t.status.success());
    let m = Command::new("tar")
        .arg("-xzOf")
        .arg(&a)
        .arg(format!("{top}/db/MAP"))
        .output()
        .unwrap();
    assert!(m.status.success());
    // SHA256SUMS consistent.
    let x = dest.join(format!("x-{top}"));
    fs::create_dir_all(&x).unwrap();
    assert!(Command::new("tar")
        .arg("-xzf")
        .arg(&a)
        .arg("-C")
        .arg(&x)
        .status()
        .unwrap()
        .success());
    assert!(Command::new("sha256sum")
        .args(["-c", "--quiet", "SHA256SUMS"])
        .current_dir(x.join(&top))
        .status()
        .unwrap()
        .success());
    let r = Run {
        code: o.status.code().unwrap_or(-1),
        lines,
        list: String::from_utf8_lossy(&t.stdout).into_owned(),
        map: String::from_utf8_lossy(&m.stdout).into_owned(),
    };
    // Instance snapshot == live store row count.
    let snap = x.join(&top).join("db/memory-vectors.db");
    let n_snap = l0_rows(&snap);
    let n_live = l0_rows(&root.join(INST_DB));
    assert_eq!(n_snap, n_live, "instance snapshot rows == live");
    say(&format!(
        "   exit {} | log: {} | MAP {} lines | instance l0 rows {n_snap} (live {n_live})",
        r.code,
        r.lines.join(" | "),
        r.map.lines().count()
    ));
    fs::remove_dir_all(&x).unwrap();
    r
}

fn l0_rows(p: &Path) -> i64 {
    let c = rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    c.query_row("SELECT count(*) FROM l0_conversations", [], |r| r.get(0))
        .unwrap()
}

fn mem_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("OPENMIND_MEMORY_BIN") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("openmind-memory");
    assert!(
        p.is_file(),
        "build it first: cargo build --release -p openmind-memory (or set OPENMIND_MEMORY_BIN); looked at {}",
        p.display()
    );
    p
}

struct Server(Child, u16);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(p: u16, method: &str, path: &str, body: &str) -> std::io::Result<u16> {
    let mut s = TcpStream::connect(("127.0.0.1", p))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{p}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let t = String::from_utf8_lossy(&raw);
    Ok(t.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0))
}

fn start(dir: &Path, p: u16, instance: Option<&str>) -> Server {
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} in use"
    );
    let mut cmd = Command::new(mem_bin());
    for k in [
        "OPENMIND_MEMORY_API_KEY",
        "TDAI_GATEWAY_API_KEY",
        "OPENMIND_MEMORY_READ_ONLY",
        "OPENMIND_MEMORY_INSTANCE_ID",
        "TDAI_INSTANCE_ID",
        "TDAI_DATA_DIR",
        "NERVE_URL",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", p.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(i) = instance {
        cmd.env("OPENMIND_MEMORY_INSTANCE_ID", i);
    }
    let srv = Server(cmd.spawn().expect("spawn openmind-memory"), p);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if http(p, "GET", "/health", "").ok() == Some(200) {
            say(&format!(
                "   openmind-memory :{p} instance={instance:?} /health 200 after {:?}",
                t0.elapsed()
            ));
            return srv;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("openmind-memory :{p} not healthy");
}

fn stop(s: Server) {
    let p = s.1;
    drop(s);
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} still open"
    );
}

fn capture(p: u16, session: &str) {
    // Epoch ms for the /capture wire format, never displayed.
    let ts = SystemTime::now() // repo-check: allow (epoch ms wire field)
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let body = format!(
        r#"{{"user_content":"sim 427","assistant_content":"ok","session_key":"{session}","messages":[{{"role":"user","content":"sim 427 backup test","timestamp":{ts}}},{{"role":"assistant","content":"ok noted","timestamp":{}}}]}}"#,
        ts + 1
    );
    assert_eq!(http(p, "POST", "/capture", &body).unwrap(), 200);
}

fn sha(p: &Path) -> String {
    let o = Command::new("sha256sum").arg(p).output().unwrap();
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn bash(script: &str, d: &Path, a: &Path) -> String {
    let o = Command::new("bash")
        .args(["-euo", "pipefail", "-c", script])
        .env("D", d)
        .env("A", a)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "runbook step failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn trio_absent(root: &Path) -> bool {
    ["", "-wal", "-shm"]
        .iter()
        .all(|x| !root.join(format!("{ROOT_DB}{x}")).exists())
}

const ROOT_MAP: &str = "/var/lib/memory-tencentdb/vectors.db memory-root-vectors.db";

#[test]
fn real_legacy_root_drop_sim() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") || std::env::var(MARK).is_ok() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let t = common::tmp("real427");
    let root = t.join("root");
    let dest = t.join("root/var/backups/galion");
    common::build_tree(&root);
    let d = root.join("var/lib/memory-tencentdb");
    // Replace the fake memory DBs with ones the real server writes.
    for rel in [ROOT_DB, INST_DB] {
        for x in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(root.join(format!("{rel}{x}")));
        }
    }
    say("== setup: real openmind-memory (instance galion) writes the instance DB");
    let s = start(&d, port(0), Some("galion"));
    capture(port(0), "sim-427-a");
    capture(port(0), "sim-427-b");
    stop(s);
    assert!(
        trio_absent(&root),
        "instance server never creates the root DB"
    );
    let legacy_src = std::env::var_os("GALION_LEGACY_ROOT_COPY").map(PathBuf::from);
    match &legacy_src {
        Some(c) => {
            fs::copy(c, root.join(ROOT_DB)).unwrap();
            say(&format!(
                "   legacy root = box copy of the LIVE root DB, sha {}..",
                &sha(&root.join(ROOT_DB))[..16]
            ));
        }
        None => {
            let s = start(&d, port(1), None);
            capture(port(1), "sim-427-legacy");
            stop(s);
            say("   legacy root = written by the real server without instance id + 1 capture");
        }
    }
    let root_rows = l0_rows(&root.join(ROOT_DB));
    let root_sha = sha(&root.join(ROOT_DB));
    say(&format!(
        "   instance l0 rows {}, legacy root l0 rows {root_rows}",
        l0_rows(&root.join(INST_DB))
    ));

    say("== S1 root present (today's VPS state)");
    let r = run_backup(&root, &dest);
    assert_eq!(r.code, 0);
    assert_eq!(r.lines.len(), 1, "only the ok line: {:?}", r.lines);
    assert!(r.lines[0].starts_with("level=info msg=\"ok\""));
    assert_eq!(r.map.lines().count(), 4);
    assert_eq!(r.map.lines().nth(1), Some(ROOT_MAP), "same MAP order");
    assert!(r.list.contains("/db/memory-root-vectors.db"));
    assert_eq!(sha(&root.join(ROOT_DB)), root_sha, "source unchanged");

    say("== S2 owner runs the D-406-1 runbook (steps 1-4, literally) -> archived");
    let a = root.join("var/backups/memory-legacy-root-20261029-091500+0100");
    let out = bash(
        "install -d -m 0700 $A $A/moved
         tar -C $D -cpf $A/legacy-root-vectors.tar $(cd $D && ls vectors.db vectors.db-wal vectors.db-shm 2>/dev/null)
         ( cd $D && sha256sum vectors.db ) > $A/SHA256SUMS
         ( cd $A && sha256sum legacy-root-vectors.tar ) >> $A/SHA256SUMS
         chmod 0600 $A/legacy-root-vectors.tar $A/SHA256SUMS
         install -d -m 0700 $A/verify && tar -C $A/verify -xpf $A/legacy-root-vectors.tar
         ( cd $A/verify && sha256sum -c <(head -1 $A/SHA256SUMS) )
         rm -r $A/verify
         mv $(cd $D && ls -d $D/vectors.db $D/vectors.db-wal $D/vectors.db-shm 2>/dev/null) $A/moved/
         ls $D/vectors.db* 2>&1 || true",
        &d,
        &a,
    );
    say(&format!("   runbook: {}", out.trim().replace('\n', " | ")));
    assert!(out.contains("vectors.db: OK"));
    assert!(trio_absent(&root));
    let r = run_backup(&root, &dest);
    assert_eq!(r.code, 0);
    assert_eq!(r.lines.len(), 2, "{:?}", r.lines);
    assert_eq!(
        r.lines[0],
        "level=info msg=\"legacy db archived, skipped /var/lib/memory-tencentdb/vectors.db\" marker=/var/backups/memory-legacy-root-20261029-091500+0100"
    );
    assert!(
        !r.lines.iter().any(|l| l.contains("level=warn")),
        "quiet: no warn"
    );
    assert_eq!(r.map.lines().count(), 3);
    assert!(!r.map.contains(ROOT_MAP));
    assert!(!r.list.contains("memory-root-vectors.db"));
    assert!(trio_absent(&root), "backup never recreates the root DB");
    // The live instance server keeps running without recreating it.
    let s = start(&d, port(0), Some("galion"));
    capture(port(0), "sim-427-after-archive");
    stop(s);
    assert!(trio_absent(&root));
    let r = run_backup(&root, &dest);
    assert_eq!((r.code, r.lines.len(), r.map.lines().count()), (0, 2, 3));

    say("== S3 absent, archive marker gone (D-406-2 deleted the whole dir) -> old warn");
    let a_away = t.join("archive-parked");
    fs::rename(&a, &a_away).unwrap();
    let r = run_backup(&root, &dest);
    assert_eq!(r.code, 0);
    assert_eq!(
        r.lines[0],
        "level=warn msg=\"missing db /var/lib/memory-tencentdb/vectors.db\""
    );
    assert_eq!(r.map.lines().count(), 3);
    assert!(trio_absent(&root));
    fs::rename(&a_away, &a).unwrap();

    say("== S4 F-406-2: real server WITHOUT instance id recreates an empty root DB");
    let s = start(&d, port(1), None);
    stop(s);
    assert!(
        root.join(ROOT_DB).is_file(),
        "hazard reproduced: root DB recreated"
    );
    let empty_sha = sha(&root.join(ROOT_DB));
    say(&format!(
        "   recreated root DB {} B, l0 rows {}, has_content={:?}",
        fs::metadata(root.join(ROOT_DB)).unwrap().len(),
        l0_rows(&root.join(ROOT_DB)),
        galion_backup::sqlite::has_content(&root.join(ROOT_DB))
    ));
    let r = run_backup(&root, &dest);
    assert_eq!(r.code, 0);
    assert_eq!(
        r.lines[0],
        "level=warn msg=\"empty legacy db not backed up /var/lib/memory-tencentdb/vectors.db\" hint=\"check OPENMIND_MEMORY_INSTANCE_ID (F-406-2)\""
    );
    assert_eq!(r.map.lines().count(), 3);
    assert!(!r.list.contains("memory-root-vectors.db"));
    assert_eq!(
        sha(&root.join(ROOT_DB)),
        empty_sha,
        "empty source untouched"
    );

    say("== S5 rollback (runbook): remove the empty file, mv originals back");
    let out = bash(
        "rm -f $D/vectors.db $D/vectors.db-wal $D/vectors.db-shm
         mv $A/moved/* $D/
         ( cd $D && sha256sum -c <(head -1 $A/SHA256SUMS) )",
        &d,
        &a,
    );
    assert!(out.contains("vectors.db: OK"), "{out}");
    let r = run_backup(&root, &dest);
    assert_eq!(r.code, 0);
    assert_eq!(r.lines.len(), 1, "{:?}", r.lines);
    assert_eq!(r.map.lines().nth(1), Some(ROOT_MAP));
    assert_eq!(sha(&root.join(ROOT_DB)), root_sha);
    assert_eq!(l0_rows(&root.join(ROOT_DB)), root_rows);

    let n = backup::archives(&dest).len();
    say(&format!(
        "== done: {n} archives, all SHA256SUMS ok; sim root removed"
    ));
    fs::remove_dir_all(&t).unwrap();
}
