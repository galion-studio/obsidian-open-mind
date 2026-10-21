// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Option B real-life test. Runs only with `GALION_REAL=1`.
//!
//! Starts the built `hades-gate serve` on 127.0.0.1:19940 (wave-4 slot 124
//! range 19940-19949). For every red-team attack, raw and wrapped in the
//! `hades-plan/v1` header, it mints a **valid** signed token with the CLI for
//! the exact bytes and POSTs `/v1/approve-typed`. A bypass is a `WouldApply`.
//! The legitimate plans, as canonical typed plans, must get `WouldApply`.
//! Then the `check-typed` CLI is exercised (dry-run, approve, replay).
//! Throwaway keys in a temp dir; token strings are never printed.

#[path = "support/redteam_corpus.rs"]
mod corpus;
#[path = "support/typed_legit.rs"]
mod typed_legit;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use hades_gate::typed::MAGIC;

fn bin() -> String {
    std::env::var("HADES_GATE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_hades-gate").to_owned())
}

fn port() -> u16 {
    std::env::var("HADES_TYPED_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(19940)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run hades-gate")
}

fn mint(dir: &Path, seed: &Path, target: &str, plan: &[u8], n: usize) -> Option<PathBuf> {
    let pf = dir.join(format!("plan-{n}.txt"));
    let out = dir.join(format!("tok-{n}"));
    std::fs::write(&pf, plan).unwrap();
    let o = run(&[
        "mint",
        "--seed",
        s(seed),
        "--action",
        "heal-apply",
        "--target",
        target,
        "--plan-file",
        s(&pf),
        "--ttl",
        "300",
        "--out",
        s(&out),
    ]);
    o.status.success().then_some(out)
}

fn post(path: &str, body: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", port())).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        c,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    out
}

fn approve_typed(target: &str, plan: &[u8], token: &str) -> String {
    let body = serde_json::json!({
        "action": "heal-apply", "target": target,
        "plan": String::from_utf8_lossy(plan), "token": token,
    })
    .to_string();
    post("/v1/approve-typed", &body)
}

struct Server(Child, PathBuf);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = post("/v1/stop", "");
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

#[test]
fn real_typed_over_http_and_cli() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hades-typed-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
    assert!(run(&["keygen", "--seed", s(&seed), "--pub", s(&public)])
        .status
        .success());
    let cfg = dir.join("hades.toml");
    std::fs::write(
        &cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            s(&public),
            s(&dir.join("ledger.jsonl")),
            s(&dir.join("DISABLED"))
        ),
    )
    .unwrap();
    let child = Command::new(bin())
        .args(["serve", "--config", s(&cfg), "--port", &port().to_string()])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let srv = Server(child, dir.clone());
    let up = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        TcpStream::connect(("127.0.0.1", port())).is_ok()
    });
    assert!(up, "server did not start");

    let mut n = 0;
    let mut sent = 0;
    let mut bypass = Vec::new();
    let mut codes: Vec<(String, usize)> = Vec::new();
    for c in corpus::ATTACKS {
        let target = c.target.unwrap_or("galion-tuner");
        let wrapped = format!("{MAGIC}\n{}", c.payload);
        for plan in [c.payload.as_bytes(), wrapped.as_bytes()] {
            n += 1;
            let tok = mint(&dir, &seed, target, plan, n)
                .or_else(|| mint(&dir, &seed, "galion-tuner", plan, n))
                .expect("mint a valid token");
            let tok = std::fs::read_to_string(tok).unwrap();
            let resp = approve_typed(target, plan, tok.trim());
            sent += 1;
            if resp.contains("WouldApply") {
                bypass.push(format!("[{}] {} {:?}", c.family, c.desc, plan));
            }
            let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
            let code = serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_owned))
                .unwrap_or_else(|| resp.lines().next().unwrap_or("").to_owned());
            match codes.iter_mut().find(|x| x.0 == code) {
                Some(x) => x.1 += 1,
                None => codes.push((code, 1)),
            }
        }
    }
    let mut legit_bad = Vec::new();
    let legit = typed_legit::legit_typed();
    for (target, plan) in &legit {
        n += 1;
        let bytes = plan.canonical_bytes();
        let tok = mint(&dir, &seed, target, &bytes, n).expect("mint legit");
        let tok = std::fs::read_to_string(tok).unwrap();
        let resp = approve_typed(target, &bytes, tok.trim());
        if !resp.contains("WouldApply") {
            legit_bad.push(format!("{target}: {}", resp.lines().next().unwrap_or("")));
        }
        // Replay over HTTP must be refused.
        let again = approve_typed(target, &bytes, tok.trim());
        assert!(again.starts_with("HTTP/1.1 403"), "replay: {again}");
    }
    for (code, k) in &codes {
        eprintln!("REAL typed refusal code {code}: {k}");
    }
    eprintln!("REAL typed TOTAL WouldApply: {} / {sent}", bypass.len());
    eprintln!(
        "REAL typed legit not approved: {} / {}",
        legit_bad.len(),
        legit.len()
    );

    // CLI: check-typed dry-run (exit 0), approve with token, then replay.
    let pf = dir.join("cli-plan.txt");
    let plan = format!("{MAGIC}\nrestart galion-tuner\nverify 127.0.0.1:8420 /health");
    std::fs::write(&pf, &plan).unwrap();
    let base = [
        "check-typed",
        "--config",
        s(&cfg),
        "--action",
        "heal-apply",
        "--target",
        "galion-tuner",
        "--plan-file",
        s(&pf),
    ];
    let dry = run(&base);
    let dry_out = String::from_utf8_lossy(&dry.stdout).to_string();
    eprintln!(
        "REAL check-typed dry-run exit={:?} {}",
        dry.status.code(),
        dry_out.trim()
    );
    let tok = mint(&dir, &seed, "galion-tuner", plan.as_bytes(), 99_999).unwrap();
    let mut with_tok: Vec<&str> = base.to_vec();
    with_tok.extend(["--token-file", s(&tok)]);
    let ok = run(&with_tok);
    let ok_out = String::from_utf8_lossy(&ok.stdout).to_string();
    eprintln!(
        "REAL check-typed token exit={:?} {}",
        ok.status.code(),
        ok_out.trim()
    );
    let rep = run(&with_tok);
    let rep_out = String::from_utf8_lossy(&rep.stdout).to_string();
    eprintln!(
        "REAL check-typed replay exit={:?} {}",
        rep.status.code(),
        rep_out.trim()
    );
    let prose_pf = dir.join("cli-prose.txt");
    std::fs::write(&prose_pf, "restart galion-tuner").unwrap();
    let mut prose = base.to_vec();
    prose[8] = s(&prose_pf);
    let pr = run(&prose);
    let pr_out = String::from_utf8_lossy(&pr.stdout).to_string();
    eprintln!(
        "REAL check-typed prose exit={:?} {}",
        pr.status.code(),
        pr_out.trim()
    );

    drop(srv);
    assert!(dry_out.contains("DryRun"), "{dry_out}");
    assert!(ok_out.contains("WouldApply"), "{ok_out}");
    assert!(!rep_out.contains("WouldApply"), "{rep_out}");
    assert!(
        !pr_out.contains("WouldApply") && pr_out.contains("typed-plan"),
        "{pr_out}"
    );
    assert!(legit_bad.is_empty(), "{legit_bad:#?}");
    assert_eq!(sent, 164);
    assert!(
        bypass.is_empty(),
        "{} bypasses:\n{}",
        bypass.len(),
        bypass.join("\n")
    );
}
