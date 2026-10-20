// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S3.1 real-life red-team test. Runs only with `GALION_REAL=1`.
//!
//! Starts the built `hades-gate serve` on 127.0.0.1:19380 (slot 38 range),
//! and for every corpus attack mints a **valid** signed token for that exact
//! plan with a throwaway admin key, then POSTs `/v1/approve`. A bypass is a
//! `WouldApply` answer. Legitimate plans must still get `WouldApply`.
//!
//! `HADES_GATE_BIN` / `HADES_REDTEAM_PORT` override the binary and port, so
//! the same test can be pointed at an older build for a before/after count.
//! Token strings are never printed.

#[path = "support/redteam_corpus.rs"]
mod corpus;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn bin() -> String {
    std::env::var("HADES_GATE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_hades-gate").to_owned())
}

fn port() -> u16 {
    std::env::var("HADES_REDTEAM_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(19380)
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

/// Mint a valid token for (heal-apply, target, plan). `None` if mint refuses.
fn mint(dir: &Path, seed: &Path, target: &str, plan: &[u8], n: usize) -> Option<String> {
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
    o.status
        .success()
        .then(|| std::fs::read_to_string(&out).unwrap())
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

fn approve(target: &str, plan: &[u8], token: &str) -> String {
    let body = serde_json::json!({
        "action": "heal-apply", "target": target,
        "plan": String::from_utf8_lossy(plan), "token": token,
    })
    .to_string();
    post("/v1/approve", &body)
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
fn real_redteam_over_http() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hades-redteam-{}-{nanos}", std::process::id()));
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
    let mut bypass = Vec::new();
    let mut per_family: Vec<(&str, usize, usize)> = Vec::new();
    for c in corpus::ATTACKS {
        n += 1;
        let plan = c.payload.as_bytes();
        let target = c.target.unwrap_or("galion-tuner");
        // A valid token for the exact plan. For a bad target name mint may
        // refuse; then sign for a good name (scope mismatch would be a refusal
        // anyway, and we still learn whether safety/target checks ran first).
        let token = mint(&dir, &seed, target, plan, n)
            .or_else(|| mint(&dir, &seed, "galion-tuner", plan, n))
            .expect("mint a valid token");
        let resp = approve(target, plan, &token);
        let would = resp.contains("WouldApply");
        let line = resp.lines().next().unwrap_or("").to_owned();
        let reason = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
        eprintln!(
            "[{}] {:<40} {} {}",
            c.family,
            c.desc,
            line,
            if would { "WOULDAPPLY" } else { &reason }
        );
        let i = per_family
            .iter()
            .position(|f| f.0 == c.family)
            .unwrap_or_else(|| {
                per_family.push((c.family, 0, 0));
                per_family.len() - 1
            });
        per_family[i].1 += 1;
        if would {
            per_family[i].2 += 1;
            bypass.push(format!("[{}] {}", c.family, c.desc));
        }
    }
    let mut legit_bad = Vec::new();
    for (target, plan) in corpus::LEGIT {
        n += 1;
        let token = mint(&dir, &seed, target, plan.as_bytes(), n).expect("mint legit");
        let resp = approve(target, plan.as_bytes(), &token);
        if !resp.contains("WouldApply") {
            legit_bad.push(format!(
                "{target}: {plan:?} -> {}",
                resp.lines().next().unwrap_or("")
            ));
        }
    }
    for (f, total, b) in &per_family {
        eprintln!("REAL {f}: {b} WouldApply / {total}");
    }
    eprintln!(
        "REAL TOTAL WouldApply: {} / {}",
        bypass.len(),
        corpus::ATTACKS.len()
    );
    eprintln!(
        "REAL legit not approved: {} / {}",
        legit_bad.len(),
        corpus::LEGIT.len()
    );
    drop(srv);
    assert!(legit_bad.is_empty(), "{legit_bad:#?}");
    assert!(
        bypass.is_empty(),
        "{} bypasses:\n{}",
        bypass.len(),
        bypass.join("\n")
    );
}
