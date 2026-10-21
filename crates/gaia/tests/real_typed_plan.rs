// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1), slot 152: the built `gaia typed-plan` binary
//! emits canonical `hades-plan/v1` typed plans, and the built `hades-gate`
//! binary (`mint`, `plan-hash`, `check-typed`, `serve` on 127.0.0.1) approves
//! exactly those bytes once, over the CLI and over HTTP
//! `POST /v1/approve-typed`, and refuses replays, cross-intensity tokens,
//! the prose plan bytes and the systems with no v1 form.
//!
//! Throwaway keys in a temp dir; the gate's port is `GAIA_TYPED_PORT`
//! (default 20220, wave-4 slot 152 range 20220-20229). Nothing is executed.
//! Run (release, as the swarm rules ask):
//! `GALION_REAL=1 HADES_GATE_BIN=$T/release/hades-gate cargo test --release -p gaia --test real_typed_plan -- --nocapture`

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

fn port() -> u16 {
    std::env::var("GAIA_TYPED_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(20220)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn gate(args: &[&str]) -> Output {
    let bin = std::env::var("HADES_GATE_BIN").expect("HADES_GATE_BIN");
    Command::new(bin)
        .args(args)
        .output()
        .expect("run hades-gate")
}

fn gaia(args: &[&str]) -> (i32, serde_json::Value, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .arg("--compact")
        .output()
        .expect("run gaia");
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let v = serde_json::from_str(&out).unwrap_or(serde_json::Value::Null);
    (o.status.code().unwrap_or(-1), v, out)
}

fn out_str(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn mint(dir: &Path, seed: &Path, action: &str, target: &str, plan: &Path, tag: &str) -> String {
    let out = dir.join(format!("tok-{tag}"));
    let o = gate(&[
        "mint",
        "--seed",
        s(seed),
        "--action",
        action,
        "--target",
        target,
        "--plan-file",
        s(plan),
        "--ttl",
        "300",
        "--out",
        s(&out),
    ]);
    assert!(o.status.success(), "mint {tag}: {}", out_str(&o));
    std::fs::read_to_string(out).unwrap().trim().to_string()
}

fn check_typed(
    cfg: &Path,
    action: &str,
    target: &str,
    plan: &Path,
    tok: Option<&Path>,
) -> (i32, String) {
    let mut a = vec![
        "check-typed",
        "--config",
        s(cfg),
        "--action",
        action,
        "--target",
        target,
        "--plan-file",
        s(plan),
    ];
    if let Some(t) = tok {
        a.extend(["--token-file", s(t)]);
    }
    let o = gate(&a);
    (o.status.code().unwrap_or(-1), out_str(&o))
}

fn post_typed(action: &str, target: &str, plan: &str, token: &str) -> String {
    let body = serde_json::json!({
        "action": action, "target": target, "plan": plan, "token": token,
    })
    .to_string();
    let mut c = TcpStream::connect(("127.0.0.1", port())).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        c,
        "POST /v1/approve-typed HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    out
}

struct Server(Child, PathBuf);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

#[test]
fn real_gaia_typed_plan_through_hades_gate_cli_and_http() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 and HADES_GATE_BIN");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gaia-typed-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
    assert!(gate(&["keygen", "--seed", s(&seed), "--pub", s(&public)])
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
    let child = Command::new(std::env::var("HADES_GATE_BIN").unwrap())
        .args(["serve", "--config", s(&cfg), "--port", &port().to_string()])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let srv = Server(child, dir.clone());
    let up = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        TcpStream::connect(("127.0.0.1", port())).is_ok()
    });
    assert!(up, "hades-gate serve did not start on {}", port());
    eprintln!("REAL hades-gate serve pid={pid} on 127.0.0.1:{}", port());

    let target = "tuner";
    let cases: &[(&str, u8, &str)] = &[
        ("rollback", 100, "rollback"),
        ("rollback", 0, "rollback"),
        ("heal-apply", 0, "heal-apply"),
        ("heal-apply", 50, "heal-apply"),
        ("heal-apply", 100, "heal-apply"),
        ("heal-apply", 255, "heal-apply"),
    ];
    let (mut cli_ok, mut http_ok, mut replay_refused, mut hash_eq) = (0, 0, 0, 0);
    for (i, (sys, intensity, action)) in cases.iter().enumerate() {
        let pf = dir.join(format!("plan-{i}.txt"));
        let (code, v, raw) = gaia(&[
            "typed-plan",
            sys,
            "--intensity",
            &intensity.to_string(),
            "--out",
            s(&pf),
        ]);
        assert_eq!(code, 0, "{raw}");
        let r = &v["result"];
        assert_eq!(r["ok"], true, "{raw}");
        assert_eq!(r["action"], *action, "{raw}");
        let bytes = std::fs::read(&pf).unwrap();
        assert_eq!(r["canonical"].as_str().unwrap().as_bytes(), &bytes[..]);
        assert!(bytes.starts_with(b"hades-plan/v1\n") && !bytes.ends_with(b"\n"));
        // gaia and the gate agree on the hash of the exact bytes.
        let h = gate(&["plan-hash", "--plan-file", s(&pf)]);
        if out_str(&h) == r["plan_sha256"].as_str().unwrap() {
            hash_eq += 1;
        }
        eprintln!(
            "REAL gaia typed-plan {sys} --intensity {intensity}: sha256={} steps={}",
            r["plan_sha256"].as_str().unwrap(),
            r["steps"].as_array().unwrap().len()
        );

        // CLI: no token -> dry-run; token -> WouldApply; replay -> refused.
        let (c0, dry) = check_typed(&cfg, action, target, &pf, None);
        assert!(c0 == 0 && dry.contains("DryRun"), "{dry}");
        let tok = mint(&dir, &seed, action, target, &pf, &format!("cli-{i}"));
        let tf = dir.join(format!("tok-cli-{i}"));
        let (c1, ok) = check_typed(&cfg, action, target, &pf, Some(&tf));
        assert!(c1 == 0 && ok.contains("WouldApply"), "{ok}");
        cli_ok += 1;
        let (c2, rep) = check_typed(&cfg, action, target, &pf, Some(&tf));
        assert!(c2 == 2 && !rep.contains("WouldApply"), "{rep}");
        replay_refused += 1;
        let _ = tok;

        // HTTP: a fresh token for the same bytes, then replay -> 403.
        let tok = mint(&dir, &seed, action, target, &pf, &format!("http-{i}"));
        let plan = String::from_utf8(bytes.clone()).unwrap();
        let a = post_typed(action, target, &plan, &tok);
        assert!(
            a.starts_with("HTTP/1.1 200") && a.contains("WouldApply"),
            "{a}"
        );
        http_ok += 1;
        let b = post_typed(action, target, &plan, &tok);
        assert!(b.starts_with("HTTP/1.1 403"), "{b}");
        replay_refused += 1;
    }

    // A heal-apply token for intensity 50 must not approve intensity 100.
    let (p50, p100) = (dir.join("plan-3.txt"), dir.join("plan-4.txt"));
    mint(&dir, &seed, "heal-apply", target, &p50, "x50");
    let (cx, cross) = check_typed(
        &cfg,
        "heal-apply",
        target,
        &p100,
        Some(&dir.join("tok-x50")),
    );
    assert!(cx == 2 && !cross.contains("WouldApply"), "{cross}");
    // ... nor under another action.
    let (ca, act) = check_typed(&cfg, "rollback", target, &p50, Some(&dir.join("tok-x50")));
    assert!(ca == 2 && !act.contains("WouldApply"), "{act}");

    // The prose plan (unchanged path) is refused on the typed path even
    // with a valid token for its exact bytes.
    let (pc, pv, praw) = gaia(&["plan", "rollback"]);
    assert_eq!(pc, 0, "{praw}");
    let prose_steps: Vec<&str> = pv["result"]["plan"]
        .as_array()
        .or_else(|| pv["result"]["report"]["plan"].as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    assert!(!prose_steps.is_empty(), "prose plan: {praw}");
    let ppf = dir.join("prose.txt");
    std::fs::write(&ppf, prose_steps.join("\n")).unwrap();
    mint(&dir, &seed, "rollback", target, &ppf, "prose");
    let (pc2, pout) = check_typed(&cfg, "rollback", target, &ppf, Some(&dir.join("tok-prose")));
    assert!(pc2 == 2 && pout.contains("typed-plan"), "{pout}");
    eprintln!("REAL prose rollback plan on typed path: exit={pc2} {pout}");

    // Systems with no v1 form fail closed and write nothing.
    let mut not_expr = 0;
    for sys in ["teardown", "kill-switch", "secret-rotation-plan"] {
        let pf = dir.join(format!("none-{sys}.txt"));
        let (code, v, raw) = gaia(&["typed-plan", sys, "--out", s(&pf)]);
        assert_eq!(code, 4, "{raw}");
        assert_eq!(v["result"]["code"], "not-expressible", "{raw}");
        assert!(!pf.exists(), "{sys} wrote a plan file");
        not_expr += 1;
    }
    let (code, v, raw) = gaia(&["typed-plan", "repo-check"]);
    assert_eq!(code, 4, "{raw}");
    assert_eq!(v["result"]["code"], "not-hades", "{raw}");
    // --out never overwrites.
    let (code, _, _) = gaia(&[
        "typed-plan",
        "rollback",
        "--out",
        s(&dir.join("plan-0.txt")),
    ]);
    assert_eq!(code, 2, "--out must not overwrite");

    let ledger = std::fs::read_to_string(dir.join("ledger.jsonl")).unwrap_or_default();
    eprintln!(
        "REAL typed: cases={} hash_eq={hash_eq} cli_WouldApply={cli_ok} http_WouldApply={http_ok} \
         replays_refused={replay_refused} cross_refused=2 prose_refused=1 not_expressible={not_expr} \
         not_hades=1 ledger_lines={}",
        cases.len(),
        ledger.lines().count()
    );
    assert_eq!(hash_eq, cases.len());
    assert_eq!(cli_ok, cases.len());
    assert_eq!(http_ok, cases.len());
    assert_eq!(replay_refused, 2 * cases.len());
    drop(srv);
    assert!(
        TcpStream::connect(("127.0.0.1", port())).is_err(),
        "gate still listening after teardown"
    );
}
