// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 378 real-life test (GALION_REAL=1): the nerve `restore-test`
//! component (`kind: "restore"`, staged in
//! `deploy/staged/nerve/components-live.json`) judges REAL restore-test logs.
//!
//! The logs are the real stdout+stderr (one run per file, like the staged
//! drop-in `StandardOutput=truncate:` + `StandardError=inherit`) of the real
//! slot-361 `galion-backup restore-test` (strict checksums, built from
//! swarm/restore-checksum-fail 2b24e88b; path in `NERVE_RESTORE_BIN`), run on
//! a placeholder backup archive in the backup layout (real SQLite DBs made by
//! that binary, files.tar, db/MAP, C-sorted SHA256SUMS) and on tampered
//! copies made the slot-361 way (digest mismatch, dropped SHA256SUMS line,
//! unlisted file, flipped gzip CRC trailer), plus a MISSING presence path, a
//! stale log, an interrupted run, no log at all, and the live
//! `galion-restore-test.sh` (verbatim, F-326-1) on the mismatch archive.
//!
//! The real `openmind-nerve serve` (release with `--release`) runs with the
//! staged `restore-test` entry (log path moved into the temp dir) and is
//! read over its HTTP `/api/v1/state` after every log change. Ports
//! 22480-22489 (nerve on 22480). Nothing is restarted: the nerve runs with
//! NERVE_SYSTEMD=0, NERVE_JOURNAL=0, NERVE_REFLEX=0, NERVE_DRY_RUN=1.
//!
//! `GALION_REAL=1 NERVE_RESTORE_BIN=<galion-backup 2b24e88b> cargo test --release -p openmind-nerve --test real_restore_component -- --nocapture --test-threads=1`

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 22480;
const STAGED: &str = "../../deploy/staged/nerve/components-live.json";

/// VPS `/opt/galion/rust/bin/galion-restore-test.sh`, verbatim (slot 361
/// `tests/real_checksum_fail.rs`; sha256 checked below).
const ORACLE: &str = r#"#!/bin/bash
# Restore the newest (or given) backup into a temp dir and verify it. Touches nothing live.
set -euo pipefail
umask 077
DEST=${GALION_BACKUP_DEST:-/var/backups/galion}
A=${1:-$(ls -1t "$DEST"/galion-*.tar.gz | head -1)}
T=$(mktemp -d /tmp/galion-restore-XXXXXX); trap 'rm -rf "$T"' EXIT
tar -C "$T" -xzf "$A"
cd "$T"/galion-*
sha256sum --quiet -c SHA256SUMS && echo "checksums ok"
for d in db/*.db; do /opt/galion/rust/bin/galion-backup verify "$d" | sed "s|$T/||"; done
mkdir -p restored && tar -C restored -xf files/files.tar
echo "files restored: $(find restored -type f | wc -l)"
for p in var/lib/memory-tencentdb/persona.md var/lib/hermes-rs root/galion-secrets/admin-device.json etc/openmind-memory.env; do
  [ -e "restored/$p" ] && echo "present: /$p" || echo "MISSING: /$p"
done
echo "restore test ok: $(basename "$A")"
"#;
const ORACLE_SHA256: &str = "264397750a31083298e8e926934d02ce84418c2b3501a12bb0af74ada8170f14";

fn real() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return None;
    }
    let bin = std::env::var("NERVE_RESTORE_BIN").expect(
        "NERVE_RESTORE_BIN=<galion-backup built from swarm/restore-checksum-fail 2b24e88b> is required with GALION_REAL=1",
    );
    let p = PathBuf::from(bin);
    assert!(p.is_file(), "{}: not a file", p.display());
    Some(p)
}

fn say(m: &str) {
    println!("[{}] {m}", zagreb_time::now_rfc3339());
}

fn sh(cmd: &str) -> String {
    let o = Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{cmd}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn q(p: &Path) -> String {
    format!("'{}'", p.display())
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A placeholder backup in the galion-backup layout (no real data).
fn make_tree(bin: &Path, s: &Path, missing_env: bool) {
    fs::create_dir_all(s.join("db")).unwrap();
    fs::create_dir_all(s.join("files")).unwrap();
    for db in ["hermes-rs-state.db", "memory-vectors.db"] {
        let o = Command::new(bin)
            .args(["migrate-baseline"])
            .arg(s.join("db").join(db))
            .args(["1", "placeholder"])
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{db}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    fs::write(
        s.join("db/MAP"),
        "hermes-rs-state.db /var/lib/hermes-rs/rs-state.db\nmemory-vectors.db /var/lib/memory-tencentdb/instances/galion/vectors.db\n",
    )
    .unwrap();
    let src = s.with_extension("src");
    let _ = fs::remove_dir_all(&src);
    for (rel, body) in [
        (
            "var/lib/memory-tencentdb/persona.md",
            "# placeholder persona\n",
        ),
        ("var/lib/hermes-rs/README", "placeholder\n"),
        (
            "root/galion-secrets/admin-device.json",
            "{\"placeholder\":true}\n",
        ),
        ("etc/openmind-memory.env", "PLACEHOLDER=1\n"),
    ] {
        if missing_env && rel.starts_with("etc/") {
            continue;
        }
        let p = src.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    let dirs = if missing_env {
        "var root"
    } else {
        "var root etc"
    };
    sh(&format!(
        "cd {} && tar -cf {} {dirs}",
        q(&src),
        q(&s.join("files/files.tar"))
    ));
    fs::remove_dir_all(&src).unwrap();
    sums(s);
}

fn sums(s: &Path) {
    sh(&format!(
        "cd {} && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS",
        q(s)
    ));
}

fn pack(s: &Path, out: &Path) {
    let parent = s.parent().unwrap();
    let top = s.file_name().unwrap().to_string_lossy();
    sh(&format!("cd {} && tar -czf {} {top}", q(parent), q(out)));
}

/// stdout+stderr of one run into `log` (truncated), like the staged drop-in.
fn run_into(cmd: &mut Command, log: &Path) -> Option<i32> {
    let f = fs::File::create(log).unwrap();
    let e = f.try_clone().unwrap();
    cmd.stdout(Stdio::from(f))
        .stderr(Stdio::from(e))
        .status()
        .unwrap()
        .code()
}

fn get_state() -> Option<serde_json::Value> {
    let mut s = TcpStream::connect_timeout(
        &format!("127.0.0.1:{NERVE_PORT}").parse().ok()?,
        Duration::from_secs(2),
    )
    .ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET /api/v1/state HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    if h.split_whitespace().nth(1)? != "200" {
        return None;
    }
    serde_json::from_str(body).ok()
}

fn comp(st: &serde_json::Value) -> serde_json::Value {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == "restore-test"))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// The staged table reduced to `restore-test`, log moved to `log`.
fn table(dir: &Path, log: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(STAGED);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(src).unwrap()).unwrap();
    let mut r = v
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "restore-test")
        .expect("staged restore-test entry")
        .clone();
    assert_eq!(r["log"], "/var/log/galion/restore-test.log");
    r["log"] = log.display().to_string().into();
    let f = dir.join("components.json");
    fs::write(&f, serde_json::to_string(&vec![r]).unwrap()).unwrap();
    f
}

fn nerve(dir: &Path, comps: &Path, event: bool) -> Kill {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    cmd.arg("serve");
    if event {
        cmd.args(["--mode", "event"]);
    }
    Kill(
        cmd.env("NERVE_ADDR", format!("127.0.0.1:{NERVE_PORT}"))
            .env("NERVE_INTERVAL", "5")
            .env(
                "NERVE_DATA_DIR",
                dir.join(if event { "data-evt" } else { "data" }),
            )
            .env("NERVE_COMPONENTS", comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_ROLLBACK", "0")
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_EVT_ESCALATE", "off")
            .env(
                "NERVE_LISTEN_SCOPE",
                format!("{NERVE_PORT}-{}", NERVE_PORT + 9),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    )
}

/// Current nerve cycle (waits until the API answers).
fn cycle() -> u64 {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(40) {
        if let Some(st) = get_state() {
            if let Some(c) = st["cycle"].as_u64() {
                return c;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    panic!("nerve API not answering on {NERVE_PORT}");
}

/// Wait until cycle >= `min` (a cycle that STARTED after the log change) and
/// return the component.
fn sample_at(min: u64) -> serde_json::Value {
    let t = Instant::now();
    let mut last = serde_json::Value::Null;
    while t.elapsed() < Duration::from_secs(40) {
        std::thread::sleep(Duration::from_millis(400));
        if let Some(st) = get_state() {
            let c = comp(&st);
            if st["cycle"].as_u64().unwrap_or(0) >= min && c["samples"].as_u64().unwrap_or(0) > 0 {
                return c;
            }
            last = c;
        }
    }
    panic!("no nerve cycle >= {min}: {last}");
}

fn reasons(c: &serde_json::Value) -> Vec<String> {
    c["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r.as_str().map(str::to_string))
        .collect()
}

struct Case {
    name: &'static str,
    rc: Option<i32>,
    status: &'static str,
    state: &'static str,
    reason: &'static str,
}

#[test]
fn real_restore_component_judges_real_logs() {
    let Some(bin) = real() else { return };
    assert_eq!(
        galion_sha(ORACLE.as_bytes()),
        ORACLE_SHA256,
        "oracle = VPS .sh"
    );
    let dir = std::env::temp_dir().join(format!("nerve-378-real-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("rtmp")).unwrap();
    let log = dir.join("log/restore-test.log");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let oracle = dir.join("oracle-restore-test.sh");
    let body = ORACLE.replace(
        "/opt/galion/rust/bin/galion-backup",
        &bin.display().to_string(),
    );
    fs::write(&oracle, body).unwrap();

    // Archives: clean, the slot-361 tampers, and a MISSING presence path.
    let stamp = "20261028-093300+0100";
    let s = dir.join(format!("w/galion-{stamp}"));
    make_tree(&bin, &s, false);
    let arch = |n: &str| dir.join(format!("a-{n}/galion-{stamp}.tar.gz"));
    let mk = |n: &str, edit: &dyn Fn(&Path)| {
        let w = dir.join(format!("w-{n}/galion-{stamp}"));
        sh(&format!(
            "mkdir -p {} && cp -a {} {}",
            q(w.parent().unwrap()),
            q(&s),
            q(&w)
        ));
        edit(&w);
        let a = arch(n);
        fs::create_dir_all(a.parent().unwrap()).unwrap();
        pack(&w, &a);
        a
    };
    let clean = mk("clean", &|_| {});
    let mismatch = mk("mismatch", &|w| {
        let p = w.join("db/MAP");
        let mut b = fs::read(&p).unwrap();
        b[3] ^= 0x01;
        fs::write(p, b).unwrap();
    });
    let dropped = mk("dropped", &|w| {
        let p = w.join("SHA256SUMS");
        let t = fs::read_to_string(&p).unwrap();
        let keep: Vec<&str> = t.lines().filter(|l| !l.ends_with("./db/MAP")).collect();
        assert_eq!(keep.len() + 1, t.lines().count());
        fs::write(p, keep.join("\n") + "\n").unwrap();
    });
    let unlisted = mk("unlisted", &|w| {
        fs::write(w.join("db/extra.txt"), "x\n").unwrap()
    });
    let gz = mk("gztrailer", &|_| {});
    {
        // Flip one bit of the gzip CRC32 trailer (F-361-1).
        let mut b = fs::read(&gz).unwrap();
        let n = b.len();
        b[n - 8] ^= 0x01;
        fs::write(&gz, b).unwrap();
    }
    let sm = dir.join(format!("w-missing/galion-{stamp}"));
    make_tree(&bin, &sm, true);
    let missing = arch("missing");
    fs::create_dir_all(missing.parent().unwrap()).unwrap();
    pack(&sm, &missing);
    say(&format!(
        "archives: clean {} B; tampered mismatch/dropped/unlisted/gztrailer; missing-env",
        fs::metadata(&clean).unwrap().len()
    ));

    let comps = table(&dir, &log);
    let _n = nerve(&dir, &comps, false);
    let rs = |a: &Path| {
        let mut c = Command::new(&bin);
        c.arg("restore-test")
            .arg(a)
            .env("GALION_RESTORE_TMP", dir.join("rtmp"));
        c
    };
    let mut results = vec![];
    let mut check = |case: Case, prep: &dyn Fn() -> Option<i32>| {
        let rc = prep();
        // The cycle running now may have read the log mid-change: wait for
        // the one after it.
        let c0 = cycle();
        assert_eq!(rc, case.rc, "{}: restore-test exit", case.name);
        let snap = fs::read(&log).ok();
        let mt = fs::metadata(&log).and_then(|m| m.modified()).ok();
        let c = sample_at(c0 + 2);
        let r = reasons(&c);
        let st = c["extra"]["restore"]["state"]
            .as_str()
            .unwrap_or("?")
            .to_string();
        assert_eq!(c["status"], case.status, "{}: {c}", case.name);
        assert_eq!(st, case.state, "{}: {c}", case.name);
        if case.reason.is_empty() {
            assert!(r.is_empty(), "{}: {r:?}", case.name);
        } else {
            assert!(
                r.iter().any(|x| x.contains(case.reason)),
                "{}: {r:?}",
                case.name
            );
        }
        assert_eq!(c["consecutive_down"], 0, "{}: never hard down", case.name);
        assert_eq!(c["reflex_restarts_per_hour"], 0);
        // Read-only: the nerve changed neither the log bytes nor its mtime.
        assert_eq!(fs::read(&log).ok(), snap, "{}: log changed", case.name);
        assert_eq!(fs::metadata(&log).and_then(|m| m.modified()).ok(), mt);
        let line = r
            .first()
            .cloned()
            .unwrap_or_default()
            .replace(&dir.display().to_string(), "<tmp>");
        say(&format!(
            "PASS {}: restore-test rc {:?} -> nerve {} state {} | {}",
            case.name, case.rc, case.status, st, line
        ));
        results.push(case.name);
    };

    check(
        Case {
            name: "S0 no log yet",
            rc: None,
            status: "degrading",
            state: "never",
            reason: "never recorded",
        },
        &|| None,
    );
    check(
        Case {
            name: "S1 clean archive",
            rc: Some(0),
            status: "healthy",
            state: "ok",
            reason: "",
        },
        &|| run_into(&mut rs(&clean), &log),
    );
    for (name, a, what) in [
        ("S2 digest mismatch", &mismatch, "checksum check FAILED"),
        (
            "S3 dropped SHA256SUMS line",
            &dropped,
            "checksum check FAILED",
        ),
        ("S4 unlisted file", &unlisted, "checksum check FAILED"),
        ("S5 gzip CRC trailer flip", &gz, "gzip check FAILED"),
    ] {
        let a = a.clone();
        let lg = log.clone();
        let rsc = &rs;
        check(
            Case {
                name,
                rc: Some(1),
                status: "degrading",
                state: "failed",
                reason: what,
            },
            &move || run_into(&mut rsc(&a), &lg),
        );
    }
    check(
        Case {
            name: "S6 MISSING presence path",
            rc: Some(0),
            status: "degrading",
            state: "ok",
            reason: "MISSING: /etc/openmind-memory.env",
        },
        &|| run_into(&mut rs(&missing), &log),
    );
    check(
        Case {
            name: "S7 stale ok log (30 h old)",
            rc: Some(0),
            status: "degrading",
            state: "ok",
            reason: "restore test stale: last run 30 h ago",
        },
        &|| {
            let rc = run_into(&mut rs(&clean), &log);
            sh(&format!("touch -d '30 hours ago' {}", q(&log)));
            rc
        },
    );
    check(
        Case {
            name: "S8 interrupted run (20 min old)",
            rc: None,
            status: "degrading",
            state: "incomplete",
            reason: "no verdict",
        },
        &|| {
            fs::write(
                &log,
                "checksums ok\nverify db/hermes-rs-state.db integrity=ok tables=1\n",
            )
            .unwrap();
            sh(&format!("touch -d '20 minutes ago' {}", q(&log)));
            None
        },
    );
    check(
        Case {
            name: "S9 live .sh on mismatch (F-326-1: exit 0 + 'restore test ok')",
            rc: Some(0),
            status: "degrading",
            state: "failed",
            reason: "restore test FAILED",
        },
        &|| {
            let mut c = Command::new("bash");
            c.arg(&oracle).arg(&mismatch).env("LC_ALL", "C");
            let rc = run_into(&mut c, &log);
            let t = fs::read_to_string(&log).unwrap();
            assert!(
                t.contains("restore test ok") && t.contains(": FAILED"),
                "{t}"
            );
            rc
        },
    );
    check(
        Case {
            name: "S10 recovery: clean run again",
            rc: Some(0),
            status: "healthy",
            state: "ok",
            reason: "",
        },
        &|| run_into(&mut rs(&clean), &log),
    );
    drop(_n);
    assert_eq!(results.len(), 11);
    assert!(
        fs::read_dir(dir.join("rtmp")).unwrap().next().is_none(),
        "restore temp left"
    );

    // The live mode (`--mode event`): start-up cycle sees a failed log.
    run_into(&mut rs(&mismatch), &log);
    let _e = nerve(&dir, &comps, true);
    let c = sample_at(1);
    assert_eq!(c["status"], "degrading", "event: {c}");
    assert_eq!(c["extra"]["restore"]["state"], "failed", "event: {c}");
    say(&format!(
        "PASS event mode: nerve {} | {}",
        c["status"],
        reasons(&c)[0].replace(&dir.display().to_string(), "<tmp>")
    ));
    drop(_e);
    let _ = fs::remove_dir_all(&dir);
}

fn galion_sha(b: &[u8]) -> String {
    let mut c = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(b).unwrap();
    let o = c.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}
