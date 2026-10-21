// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 175, decision D-L3 (accepted 2026-10-21): `gaia tokens` asks for
//! small batches of short-lived single-use hades-gate tokens. No standing
//! grant: at most 3 live tokens per dir, lifetime <= 1 h, every token
//! plan-bound and consumed once; expired ones are moved aside before use.

use gaia::heal_loop::{self, LoopCfg};
use gaia::live_heal::{heal_plan_sha256, HealCfg, HealTick};
use gaia::tokens::{self, MAX_BATCH};
use gaia_core::Json;
use gaia_live::heal::TestTarget;
use hades_gate::approval::unix_now;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-tokens-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn prog() -> String {
    ["/bin/true", "/usr/bin/true"]
        .iter()
        .find(|p| Path::new(p).is_file())
        .unwrap()
        .to_string()
}

struct Env {
    dir: PathBuf,
    toks: PathBuf,
    seed: PathBuf,
    config: PathBuf,
    port: String,
    prog: String,
}

impl Env {
    fn new(tag: &str) -> Env {
        let dir = tmp(tag);
        let gate = dir.join("gate");
        std::fs::create_dir_all(&gate).unwrap();
        let (seed, public) = (dir.join("admin.seed"), gate.join("admin.pub"));
        hades_gate::keys::generate(&seed, &public).unwrap();
        let config = gate.join("hades.toml");
        std::fs::write(
            &config,
            format!(
                "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
                public.display(),
                gate.join("ledger.jsonl").display(),
                gate.join("DISABLED").display()
            ),
        )
        .unwrap();
        Env {
            toks: dir.join("toks"),
            dir,
            seed,
            config,
            port: "20458".into(),
            prog: prog(),
        }
    }
    fn target(&self) -> TestTarget {
        TestTarget {
            name: "dummy-tok".into(),
            port: 20458,
            health_path: "/health".into(),
            argv: vec![self.prog.clone(), "--port".into(), self.port.clone()],
        }
    }
    fn request(&self, extra: &[&str], now: u64) -> Result<serde_json::Value, String> {
        let toks = self.toks.display().to_string();
        let mut a: Vec<&str> = vec!["--token-dir", &toks];
        a.extend_from_slice(extra);
        a.extend_from_slice(&[
            "--service",
            "dummy-tok",
            "--port",
            &self.port,
            "--allow-ports",
            "20450-20459",
            "--",
            &self.prog,
            "--port",
            &self.port,
        ]);
        tokens::request_at(&a, now)
    }
    fn mint(&self, req: &serde_json::Value, now: u64) -> Result<serde_json::Value, String> {
        let seed = self.seed.display().to_string();
        tokens::mint_at(
            &[
                "--request",
                req["request"].as_str().unwrap(),
                "--seed",
                &seed,
            ],
            now,
        )
    }
    fn files(&self, suffix: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.toks)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(suffix))
            .collect();
        v.sort();
        v
    }
    fn token_texts(&self) -> Vec<String> {
        std::fs::read_dir(&self.toks)
            .unwrap()
            .flatten()
            .filter(|e| e.path().to_string_lossy().contains(".tok"))
            .map(|e| {
                std::fs::read_to_string(e.path())
                    .unwrap()
                    .trim()
                    .to_string()
            })
            .collect()
    }
}

#[test]
fn request_mint_status_default_batch_of_three() {
    let e = Env::new("flow");
    let now = unix_now();
    let req = e.request(&[], now).unwrap();
    assert_eq!(req["count"], 3);
    assert_eq!(req["ttl_s"], 900);
    assert_eq!(req["plan_sha256"], heal_plan_sha256(&e.target()));
    let rj: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(req["request"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(rj["standing_grant"], false);
    assert_eq!(rj["hades_mode_change"], false);
    assert!(
        rj["created"].as_str().unwrap().ends_with("+02:00")
            || rj["created"].as_str().unwrap().ends_with("+01:00")
    );

    let m = e.mint(&req, now).unwrap();
    assert_eq!(m["minted"], 3);
    let files = e.files(".tok");
    assert_eq!(files.len(), 3);
    let public = hades_gate::keys::load_public(&e.dir.join("gate/admin.pub")).unwrap();
    for f in &files {
        assert_eq!(
            std::fs::metadata(f).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let tok = std::fs::read_to_string(f).unwrap();
        let c = hades_gate::token::verify_signature(&tok, &public).unwrap();
        assert_eq!(c.action, "heal-apply");
        assert_eq!(c.target, "dummy-tok");
        assert_eq!(c.expires_at - c.issued_at, 900);
        assert_eq!(
            hades_gate::token::to_hex(&c.plan_hash),
            heal_plan_sha256(&e.target())
        );
    }
    // Same request cannot be minted twice.
    assert!(e.mint(&req, now).unwrap_err().contains("already minted"));

    let toks = e.toks.display().to_string();
    let cfgp = e.config.display().to_string();
    let s = tokens::status_at(&["--token-dir", &toks, "--hades-config", &cfgp], now + 10).unwrap();
    assert_eq!(s["ready"], 3);
    for row in s["tokens"].as_array().unwrap() {
        assert_eq!(row["state"], "ready");
        assert_eq!(row["signature_ok"], true);
        assert_eq!(row["consumed_in_ledger"], false);
        assert_eq!(row["remaining_s"], 890);
    }
    // Nothing printed anywhere carries a token.
    let printed = format!("{req}{m}{s}");
    for t in e.token_texts() {
        assert!(!printed.contains(&t));
    }
    assert!(!printed.contains("hades1."));
    // status --hades-config did not create the replay ledger.
    assert!(!e.dir.join("gate/ledger.jsonl").exists());
}

#[test]
fn batch_and_lifetime_limits_hold() {
    let e = Env::new("limits");
    let now = unix_now();
    for bad in [
        &["--count", "4"][..],
        &["--count", "0"],
        &["--ttl-s", "3601"],
        &["--ttl-s", "0"],
        &["--count", "x"],
    ] {
        assert!(e.request(bad, now).is_err(), "{bad:?}");
    }
    assert_eq!(MAX_BATCH, 3);
    let r = e
        .request(&["--count", "1", "--ttl-s", "3600"], now)
        .unwrap();
    assert_eq!(
        (r["count"].as_u64(), r["ttl_s"].as_u64()),
        (Some(1), Some(3600))
    );
    // A live service port is refused by the scope guard (nothing requested).
    let toks = e.toks.display().to_string();
    let p = e.prog.clone();
    let err = tokens::request_at(
        &[
            "--token-dir",
            &toks,
            "--service",
            "hermes-rs",
            "--port",
            "8642",
            "--allow-ports",
            "8642",
            "--",
            &p,
            "--port",
            "8642",
        ],
        now,
    )
    .unwrap_err();
    assert!(err.contains("outside the test range"), "{err}");
}

#[test]
fn no_stockpiling_past_three_live_tokens() {
    let e = Env::new("ceiling");
    let now = unix_now();
    let r = e.request(&["--count", "2"], now).unwrap();
    e.mint(&r, now).unwrap();
    // 2 live + 2 more > 3.
    let err = e.request(&["--count", "2"], now).unwrap_err();
    assert!(err.contains("no stockpiling"), "{err}");
    let r1 = e.request(&["--count", "1"], now).unwrap();
    // Two requests both asking for 1 before either is minted: the second
    // mint is refused at the ceiling.
    let r2 = e.request(&["--count", "1"], now).unwrap();
    e.mint(&r1, now).unwrap();
    let err = e.mint(&r2, now).unwrap_err();
    assert!(err.contains("ceiling"), "{err}");
    assert_eq!(e.files(".tok").len(), 3);
    // One used (renamed .spent by the loop): room for one more.
    let f = &e.files(".tok")[0];
    std::fs::rename(f, format!("{}.spent", f.display())).unwrap();
    assert!(e.mint(&r2, now).is_ok());
    // Expired tokens do not count toward the ceiling.
    let e2 = Env::new("ceiling-exp");
    let old = now - 2000;
    let r = e2
        .request(&["--count", "3", "--ttl-s", "900"], old)
        .unwrap();
    e2.mint(&r, old).unwrap();
    assert!(e2.request(&["--count", "3"], now).is_ok());
}

#[test]
fn mint_refuses_tampered_stale_or_granting_requests() {
    let e = Env::new("tamper");
    let now = unix_now();
    // Plan file changed after the request.
    let r = e.request(&["--count", "1"], now).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(r["plan_file"].as_str().unwrap())
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"\nrestart everything"))
        .unwrap();
    assert!(e.mint(&r, now).unwrap_err().contains("plan_sha256"));
    // Request edited to a standing grant / bigger batch / longer life.
    for (k, v) in [
        ("standing_grant", serde_json::json!(true)),
        ("hades_mode_change", serde_json::json!(true)),
        ("count", serde_json::json!(5)),
        ("ttl_s", serde_json::json!(7200)),
        ("action", serde_json::json!("deploy")),
        ("plan_file", serde_json::json!("../../etc/passwd")),
    ] {
        let r = e.request(&["--count", "1"], now).unwrap();
        let p = r["request"].as_str().unwrap();
        let mut j: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        j[k] = v;
        std::fs::write(p, j.to_string()).unwrap();
        assert!(e.mint(&r, now).is_err(), "{k}");
    }
    // Stale: minted more than an hour after the request.
    let r = e.request(&["--count", "1"], now).unwrap();
    assert!(e.mint(&r, now + 3601).unwrap_err().contains("older than"));
    assert!(e.files(".tok").is_empty(), "nothing was minted");
}

#[test]
fn expired_tokens_are_moved_aside_before_use() {
    let e = Env::new("expiry");
    let now = unix_now();
    // Batch of 2 minted 20 min ago with a 15 min lifetime: both expired.
    let old = e
        .request(&["--count", "2", "--ttl-s", "900"], now - 1200)
        .unwrap();
    e.mint(&old, now - 1200).unwrap();
    // A fresh one.
    let fresh = e.request(&["--count", "1", "--ttl-s", "60"], now).unwrap();
    e.mint(&fresh, now).unwrap();
    let toks = e.toks.display().to_string();
    let s = tokens::status_at(&["--token-dir", &toks], now).unwrap();
    assert_eq!(s["ready"], 1);
    assert_eq!(s["expired_unpruned"], 2);
    // The loop's picker skips (and renames) the expired ones.
    let (next, moved) = tokens::next_live(&e.toks, now);
    assert_eq!(moved, 2);
    assert!(next
        .unwrap()
        .to_string_lossy()
        .contains(fresh["id"].as_str().unwrap()));
    assert_eq!(e.files(".tok.expired").len(), 2);
    // At the fresh token's own expiry it goes too (exp is exclusive).
    let (next, moved) = tokens::next_live(&e.toks, now + 60);
    assert_eq!((next, moved), (None, 1));
    // status --prune on an already clean dir changes nothing.
    let s = tokens::status_at(&["--token-dir", &toks, "--prune"], now + 60).unwrap();
    assert_eq!(
        (s["ready"].as_u64(), s["pruned"].as_u64()),
        (Some(0), Some(0))
    );
}

#[test]
fn loop_never_offers_an_expired_token_to_a_tick() {
    let e = Env::new("loop");
    let now = unix_now();
    let old = e
        .request(&["--count", "1", "--ttl-s", "60"], now - 600)
        .unwrap();
    e.mint(&old, now - 600).unwrap();
    let fresh = e.request(&["--count", "1", "--ttl-s", "600"], now).unwrap();
    e.mint(&fresh, now).unwrap();
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let c = LoopCfg {
        tick: HealCfg {
            target: e.target(),
            allowed: 20450..=20459,
            nerve: closed,
            state_dir: e.dir.join("state"),
            hades_config: e.config.clone(),
            token_file: None,
            wait: Duration::from_millis(200),
        },
        interval: Duration::from_millis(100),
        jitter: Duration::ZERO,
        max_ticks: Some(2),
        max_runtime: None,
        max_restarts: None,
        max_errors: 5,
        stop_file: None,
        token_dir: Some(e.toks.clone()),
        seed: Some(1),
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    let r = heal_loop::run_with(&c, &mut out, &mut |tc| {
        seen.push(tc.token_file.clone());
        Ok(HealTick {
            exit: 0,
            report: Json::obj()
                .with("outcome", "restarted")
                .with("detail", "fake"),
            approval_code: Some("approved".into()),
            spawned: true,
        })
    });
    let s: serde_json::Value = serde_json::from_str(&r.summary.compact()).unwrap();
    assert_eq!(s["tokens_expired"], 1);
    assert_eq!(s["tokens_spent"], 1);
    assert!(seen[0]
        .as_ref()
        .unwrap()
        .to_string_lossy()
        .contains(fresh["id"].as_str().unwrap()));
    assert_eq!(seen[1], None, "no live token left for the second tick");
    assert_eq!(e.files(".tok.expired").len(), 1);
    assert_eq!(e.files(".tok.spent").len(), 1);
}

#[test]
fn peek_claims_reads_expiry_only_from_well_formed_tokens() {
    assert!(tokens::peek_claims("hades1.zz.00").is_none());
    assert!(tokens::peek_claims("nope").is_none());
    assert!(tokens::peek_claims("").is_none());
}
