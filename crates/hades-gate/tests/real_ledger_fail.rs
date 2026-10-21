// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for finding 4 of cortex-s2-d (slot 125, hades-ledger-fail):
//! the built `hades-gate` binary on a ledger that stops taking writes must be
//! loud on stderr (= the systemd journal the cortex `hades` sensor reads), in
//! its JSON, its exit code and its `/health`.
//!
//! Runs only with `GALION_REAL=1`. Binds 127.0.0.1:19950 (slot 125 range
//! 19950-19959). Throwaway keys in a temp dir; token strings are never printed.
//! A full disk is simulated with a ledger symlinked to `/dev/full` (every
//! write fails with ENOSPC); lost write permission with `chmod 0400`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hades-gate");
const PORT: u16 = 19950;
const PLAN: &str = "restart galion-tuner";

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run hades-gate")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = chmod(&self.0.join("ledger.jsonl"), 0o600);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server(Option<Child>);
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn chmod(p: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
}

fn http(method: &str, path: &str, body: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", PORT)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        c,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    out
}

fn body_of(resp: &str) -> serde_json::Value {
    let b = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(b).unwrap_or(serde_json::Value::Null)
}

fn approve_body(token: &str) -> String {
    serde_json::json!({
        "action": "heal-apply", "target": "galion-tuner", "plan": PLAN, "token": token,
    })
    .to_string()
}

fn status_line(resp: &str) -> &str {
    resp.lines().next().unwrap_or("")
}

#[test]
fn real_ledger_write_failure_is_detectable() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = Dir(std::env::temp_dir().join(format!(
        "hades-real-ledger-fail-{}-{nanos}",
        std::process::id()
    )));
    let d = &dir.0;
    std::fs::create_dir_all(d).unwrap();
    let (seed, public) = (d.join("admin.seed"), d.join("admin.pub"));
    assert!(run(&["keygen", "--seed", s(&seed), "--pub", s(&public)])
        .status
        .success());
    let ledger = d.join("ledger.jsonl");
    let cfg = d.join("hades.toml");
    std::fs::write(
        &cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            s(&public),
            s(&ledger),
            s(&d.join("DISABLED"))
        ),
    )
    .unwrap();
    let plan = d.join("plan.txt");
    std::fs::write(&plan, PLAN).unwrap();
    let mint = |name: &str| -> String {
        let out = d.join(name);
        let o = run(&[
            "mint",
            "--seed",
            s(&seed),
            "--action",
            "heal-apply",
            "--target",
            "galion-tuner",
            "--plan-file",
            s(&plan),
            "--ttl",
            "120",
            "--out",
            s(&out),
        ]);
        assert!(o.status.success(), "{}", text(&o.stderr));
        std::fs::read_to_string(out).unwrap()
    };
    let check = |token_file: Option<&Path>| -> Output {
        let mut a = vec![
            "check",
            "--config",
            s(&cfg),
            "--action",
            "heal-apply",
            "--target",
            "galion-tuner",
            "--plan-file",
            s(&plan),
        ];
        if let Some(t) = token_file {
            a.extend(["--token-file", s(t)]);
        }
        run(&a)
    };
    let started = Instant::now();

    // ---- CLI, healthy ledger.
    let o = check(None);
    eprintln!(
        "cli healthy dry-run: exit {:?} stdout {}",
        o.status.code(),
        text(&o.stdout).trim()
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o.stdout).contains("\"audit\":\"written\""));
    let o = run(&["status", "--config", s(&cfg)]);
    eprintln!(
        "cli healthy status: exit {:?} stdout {}",
        o.status.code(),
        text(&o.stdout).trim()
    );
    assert_eq!(o.status.code(), Some(0));

    // ---- CLI, full disk (ledger -> /dev/full: open works, every write ENOSPC).
    let good = d.join("ledger.good");
    std::fs::rename(&ledger, &good).unwrap();
    std::os::unix::fs::symlink("/dev/full", &ledger).unwrap();
    let o = check(None);
    eprintln!(
        "cli full-disk dry-run: exit {:?} stderr {} | stdout {}",
        o.status.code(),
        text(&o.stderr).trim(),
        text(&o.stdout).trim()
    );
    assert_eq!(
        o.status.code(),
        Some(3),
        "dry-run with unwritten audit exits 3"
    );
    assert!(text(&o.stderr).starts_with("hades-gate: ledger unavailable ("));
    assert!(text(&o.stdout).contains("\"audit\":\"failed\""));
    let tok = d.join("full.tok");
    std::fs::write(&tok, mint("full.src")).unwrap();
    let o = check(Some(&tok));
    eprintln!(
        "cli full-disk token: exit {:?} stderr {}",
        o.status.code(),
        text(&o.stderr).trim()
    );
    assert_eq!(
        o.status.code(),
        Some(2),
        "a token cannot pass on a failing ledger"
    );
    assert!(text(&o.stdout).contains("\"code\":\"ledger\""));
    assert!(text(&o.stderr).contains("ledger unavailable"));
    let o = run(&["status", "--config", s(&cfg)]);
    eprintln!(
        "cli full-disk status: exit {:?} stdout {}",
        o.status.code(),
        text(&o.stdout).trim()
    );
    assert_eq!(o.status.code(), Some(3));
    assert!(text(&o.stdout).contains("\"ok\":false"));
    std::fs::remove_file(&ledger).unwrap();
    std::fs::rename(&good, &ledger).unwrap();

    // ---- serve: the long-running case (permission lost after start).
    let child = Command::new(BIN)
        .args(["serve", "--config", s(&cfg), "--port", &PORT.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Server(Some(child));
    let mut up = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(up, "server did not start");
    let r = http("GET", "/health", "");
    eprintln!(
        "serve health (ok): {} {}",
        status_line(&r),
        body_of(&r)["ledger"]
    );
    assert!(r.starts_with("HTTP/1.1 200"));
    let r = http("POST", "/v1/approve", &approve_body(""));
    assert!(
        r.starts_with("HTTP/1.1 200") && r.contains("\"audit\":\"written\""),
        "{r}"
    );

    chmod(&ledger, 0o400).unwrap();
    let root = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .is_ok();
    if root {
        eprintln!("note: chmod 0400 has no effect for this user; read-only part skipped");
    } else {
        let malformed = {
            // A real token with its signature part replaced: malformed.
            let t = mint("x.src");
            let mut p: Vec<&str> = t.split('.').collect();
            let last = p.len() - 1;
            p[last] = "AAAA";
            p.join(".")
        };
        let r = http("POST", "/v1/approve", &approve_body(&malformed));
        eprintln!(
            "serve ro malformed token: {} {}",
            status_line(&r),
            body_of(&r)
        );
        assert!(
            r.starts_with("HTTP/1.1 403") && r.contains("\"audit\":\"failed\""),
            "{r}"
        );
        let r = http("POST", "/v1/approve", &approve_body(""));
        eprintln!("serve ro dry-run: {} {}", status_line(&r), body_of(&r));
        assert!(r.starts_with("HTTP/1.1 503"), "{r}");
        let r = http("POST", "/v1/approve", &approve_body(&mint("ro.src")));
        eprintln!(
            "serve ro valid token: {} code {}",
            status_line(&r),
            body_of(&r)["code"]
        );
        assert!(
            r.starts_with("HTTP/1.1 403") && r.contains("\"code\":\"ledger\""),
            "{r}"
        );
        let r = http("GET", "/health", "");
        eprintln!(
            "serve health (ro): {} {}",
            status_line(&r),
            body_of(&r)["ledger"]
        );
        assert!(r.starts_with("HTTP/1.1 503"), "{r}");
        chmod(&ledger, 0o600).unwrap();
    }

    // ---- serve: full disk (probe passes, writes fail): /health catches it once.
    std::fs::rename(&ledger, &good).unwrap();
    std::os::unix::fs::symlink("/dev/full", &ledger).unwrap();
    let r = http("POST", "/v1/approve", &approve_body(""));
    eprintln!(
        "serve full dry-run: {} audit_error {}",
        status_line(&r),
        body_of(&r)["audit_error"]
    );
    assert!(r.starts_with("HTTP/1.1 503"), "{r}");
    std::fs::remove_file(&ledger).unwrap();
    std::fs::rename(&good, &ledger).unwrap();
    let r = http("GET", "/health", "");
    eprintln!(
        "serve health (after full, probe ok): {} {}",
        status_line(&r),
        body_of(&r)["ledger"]
    );
    assert!(
        r.starts_with("HTTP/1.1 503"),
        "unwritten decisions since last check: {r}"
    );
    let r = http("GET", "/health", "");
    eprintln!(
        "serve health (recovered): {} {}",
        status_line(&r),
        body_of(&r)["ledger"]
    );
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");

    assert!(http("POST", "/v1/stop", "").contains("stopping"));
    let child = server.0.take().unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let journal = text(&out.stderr);
    eprintln!("---- serve stderr (journal) ----\n{}----", journal);
    // The cortex `hades` sensor counts journal lines of units hades* that
    // contain "ledger unavailable" or "ledger corrupt".
    let hits = journal
        .lines()
        .filter(|l| l.contains("ledger unavailable") || l.contains("ledger corrupt"))
        .count();
    eprintln!(
        "sensor-matching journal lines: {hits}; elapsed {:?}",
        started.elapsed()
    );
    assert!(hits >= if root { 2 } else { 5 }, "{journal}");
    assert!(journal.contains("ledger ok again"));
    // No request data in the journal.
    for secretish in ["galion-tuner", PLAN, "AAAA", "token\":"] {
        assert!(
            !journal.contains(secretish),
            "{secretish} leaked: {journal}"
        );
    }
    let ledger_text = std::fs::read_to_string(&ledger).unwrap();
    eprintln!("ledger lines kept: {}", ledger_text.lines().count());
}
