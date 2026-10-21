// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for S5.2 (slot 208). Runs only with `GALION_REAL=1`.
//!
//! The built `hades-gate` binary (use `--release`) is started with
//! `serve --port 20780` (slot 208 range 20780-20789, 127.0.0.1 only). For
//! every corpus case the admin side mints a real token with `hades-gate mint`
//! (throwaway key from `hades-gate keygen` in a temp dir) for exactly
//! `gaia_plan_bytes(steps)`, and the request goes over real HTTP to
//! `POST /v1/approve`. Every attack must be refused (HTTP 403), every legit
//! plan approved once (200 would-apply) and then refused as replayed, and the
//! HTTP decision must equal the in-process `check_gaia` decision.
//!
//! With `HADES_CORPUS_VPS=1` the test also reads the live VPS unit list over
//! ssh (read-only `systemctl list-units`, host from `HADES_CORPUS_SSH`,
//! default `slavik-vps`) and runs every live unit name through the server as
//! a target: the never-unroute and protected units must be refused, the
//! Galion services must pass the safety rule. Nothing on the VPS is changed and
//! no plan is ever executed. Token strings are never printed.

#[path = "support/safety_corpus.rs"]
mod corpus;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use corpus::{ATTACKS, LEGIT, OK_PLAN};
use hades_gate::adapter::{check_gaia, gaia_action, gaia_plan_bytes};
use hades_gate::{Approver, Decision};

const BIN: &str = env!("CARGO_BIN_EXE_hades-gate");
const PORT: u16 = 20780;

/// Live VPS units that must be refused as targets (never-unroute + protected).
const MUST_REFUSE: &[&str] = &[
    "ssh",
    "systemd-logind",
    "postfix",
    "postfix@-",
    "postfix-resolvconf",
    "exim4",
    "sendmail",
    "systemd-resolved",
    "systemd-networkd",
    "networkd-dispatcher",
    "NetworkManager",
    "connman",
    "galion-backup",
    "dpkg-db-backup",
    "firewalld",
    "fail2ban",
    "nftables",
    "iptables",
    "ip6tables",
    "ipset",
    "ufw",
    "auditd",
    "apparmor",
    "dbus",
    "console-getty",
    "container-getty@1",
    "container-getty@2",
    "user@0",
    "user-runtime-dir@0",
    "emergency",
    "rescue",
    "initrd-cleanup",
    "kmod-static-nodes",
    "modprobe@loop",
    "rc-local",
    "apt-daily-upgrade",
    "ovsdb-server",
    "ua-reboot-cmds",
];

/// Live Galion units that must still pass the safety rule.
const MUST_PASS: &[&str] = &[
    "galion-one",
    "hermes-rs",
    "openmind-memory",
    "openmind-queue",
    "galion-nerve",
    "gaia-cortex",
    "tuner",
    "memory-gateway",
    "galion-desk",
    "galion-health",
    "link-watch-rs",
];

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn tmp() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("hades-real-corpus-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn http(method: &str, path: &str, body: &str) -> (u16, serde_json::Value) {
    let mut c = TcpStream::connect(("127.0.0.1", PORT)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        c,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let code = out
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let json = out
        .split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .unwrap_or(serde_json::Value::Null);
    (code, json)
}

/// Mint with the real admin CLI. `None` when mint itself refuses the scope
/// (bad target or action name): then the request goes without a token.
fn mint(
    dir: &Path,
    seed: &Path,
    action: &str,
    target: &str,
    plan: &[u8],
    n: usize,
) -> Option<String> {
    let pf = dir.join(format!("plan-{n}.txt"));
    std::fs::write(&pf, plan).unwrap();
    let out = dir.join(format!("tok-{n}"));
    let o = Command::new(BIN)
        .args([
            "mint",
            "--seed",
            s(seed),
            "--action",
            action,
            "--target",
            target,
            "--plan-file",
            s(&pf),
            "--ttl",
            "600",
            "--out",
            s(&out),
        ])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&pf);
    if !o.status.success() {
        return None;
    }
    let t = std::fs::read_to_string(&out).unwrap().trim().to_owned();
    let _ = std::fs::remove_file(&out);
    Some(t)
}

fn approve(action: &str, target: &str, plan: &[u8], token: &str) -> (u16, String) {
    let body = serde_json::json!({
        "action": action,
        "target": target,
        "plan": String::from_utf8_lossy(plan),
        "token": token,
    })
    .to_string();
    let (code, j) = http("POST", "/v1/approve", &body);
    let decision = j
        .get("decision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?")
        .to_owned();
    // Every answer says nothing was executed.
    assert_eq!(
        j.get("executed"),
        Some(&serde_json::Value::Bool(false)),
        "{j}"
    );
    (code, decision)
}

fn decision_name(d: Decision) -> &'static str {
    match d {
        Decision::DryRun => "dry-run",
        Decision::WouldApply => "would-apply",
        Decision::Refused => "refused",
    }
}

fn vps_units() -> Option<Vec<String>> {
    if std::env::var("HADES_CORPUS_VPS").as_deref() != Ok("1") {
        return None;
    }
    let host = std::env::var("HADES_CORPUS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &host,
            "systemctl list-units --type=service --all --no-legend --plain; systemctl list-timers --all --no-legend --plain",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("ssh");
    assert!(o.status.success(), "ssh read failed");
    let mut names: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .filter_map(|w| {
            w.strip_suffix(".service")
                .or_else(|| w.strip_suffix(".timer"))
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    names.dedup();
    Some(names)
}

#[test]
fn real_corpus_over_http() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let started = Instant::now();
    let dir = tmp();
    let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
    assert!(Command::new(BIN)
        .args(["keygen", "--seed", s(&seed), "--pub", s(&public)])
        .output()
        .unwrap()
        .status
        .success());
    let ledger = dir.join("ledger.jsonl");
    let cfg = dir.join("hades.toml");
    std::fs::write(
        &cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 900\n",
            s(&public),
            s(&ledger),
            s(&dir.join("DISABLED"))
        ),
    )
    .unwrap();
    let _srv = Server {
        child: Command::new(BIN)
            .args(["serve", "--config", s(&cfg), "--port", &PORT.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", PORT)).is_err() {
        assert!(Instant::now() < deadline, "server did not bind {PORT}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let (hc, _) = http("GET", "/health", "");
    assert_eq!(hc, 200);
    // In-process twin for the differential (its own ledger).
    let twin = Approver::new(
        hades_gate::keys::load_public(&public).unwrap(),
        dir.join("twin-ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .unwrap();
    let now = hades_gate::approval::unix_now();

    let mut bypass = Vec::new();
    let mut minted = 0usize;
    let mut tokens = Vec::new();
    let mut per: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for (i, c) in ATTACKS.iter().enumerate() {
        let steps: Vec<String> = c.steps.iter().map(|x| (*x).to_owned()).collect();
        let plan = gaia_plan_bytes(&steps);
        let wire = gaia_action(c.system).unwrap_or(c.system);
        let tok = mint(
            &dir,
            &seed,
            gaia_action(c.system).unwrap_or("heal-apply"),
            c.target,
            &plan,
            i,
        );
        minted += usize::from(tok.is_some());
        let tok = tok.unwrap_or_default();
        let (code, decision) = approve(wire, c.target, &plan, &tok);
        let twin_d = check_gaia(&twin, c.system, c.target, &steps, &tok, now).decision();
        let e = per.entry(c.cat).or_default();
        e.0 += 1;
        if code == 403 && decision == "Refused" && twin_d == Decision::Refused {
            e.1 += 1;
        } else {
            bypass.push(format!(
                "[{}] {}: http {code} {decision}, check_gaia {}",
                c.cat,
                c.desc,
                decision_name(twin_d)
            ));
        }
        if !tok.is_empty() {
            tokens.push(tok);
        }
    }
    for (cat, (n, refused)) in &per {
        println!("corpus {cat}: {refused}/{n} refused over HTTP");
    }
    println!(
        "attacks: {} total, {} with a minted valid token, {} bypasses",
        ATTACKS.len(),
        minted,
        bypass.len()
    );
    assert!(bypass.is_empty(), "{}", bypass.join("\n"));

    // Legit: dry-run without token, approved once, then replayed.
    for (i, (target, system, raw)) in LEGIT.iter().enumerate() {
        let steps: Vec<String> = raw.iter().map(|x| (*x).to_owned()).collect();
        let plan = gaia_plan_bytes(&steps);
        let wire = gaia_action(system).unwrap();
        assert_eq!(approve(wire, target, &plan, ""), (200, "DryRun".into()));
        let tok = mint(&dir, &seed, wire, target, &plan, 50_000 + i).expect("mint legit");
        assert_eq!(
            approve(wire, target, &plan, &tok),
            (200, "WouldApply".into()),
            "{target} {system}"
        );
        assert_eq!(approve(wire, target, &plan, &tok), (403, "Refused".into()));
        tokens.push(tok);
    }
    println!(
        "legit: {}/{} approved once, replay refused",
        LEGIT.len(),
        LEGIT.len()
    );

    // Live VPS unit names as targets (read-only fetch).
    if let Some(units) = vps_units() {
        let plan = gaia_plan_bytes(&OK_PLAN.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>());
        let (mut refused, mut passed) = (Vec::new(), Vec::new());
        for u in &units {
            let (code, d) = approve("heal-apply", u, &plan, "");
            if code == 403 && d == "Refused" {
                refused.push(u.as_str());
            } else {
                assert_eq!((code, d.as_str()), (200, "DryRun"), "{u}");
                passed.push(u.as_str());
            }
        }
        println!(
            "vps units: {} read, {} refused, {} pass the safety rule",
            units.len(),
            refused.len(),
            passed.len()
        );
        println!("vps pass: {}", passed.join(" "));
        for m in MUST_REFUSE {
            if units.iter().any(|u| u == m) {
                assert!(refused.contains(m), "live unit {m} must be refused");
            }
        }
        for m in MUST_PASS {
            assert!(
                units.iter().any(|u| u == m),
                "live unit {m} missing on the VPS"
            );
            assert!(passed.contains(m), "live unit {m} must pass");
        }
    }

    // Ledger: one consumed line per legit plan, no token string anywhere.
    let text = std::fs::read_to_string(&ledger).unwrap();
    assert_eq!(
        text.lines().filter(|l| l.contains("\"consumed\"")).count(),
        LEGIT.len()
    );
    for t in &tokens {
        let sig = t.rsplit('.').next().unwrap();
        assert!(!text.contains(sig), "token signature in ledger");
    }
    let (sc, _) = http("POST", "/v1/stop", "");
    assert_eq!(sc, 200);
    println!(
        "ledger lines {}, elapsed {:.1} s",
        text.lines().count(),
        started.elapsed().as_secs_f64()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
