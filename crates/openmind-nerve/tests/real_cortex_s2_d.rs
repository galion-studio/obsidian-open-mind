// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for cortex S2 T2.10 (event feed) + T2.13 (hades sensor).
//! Gated: `GALION_REAL=1`, run with `--release`. Slot 58: ports 19580-19589.
//!
//! 1. The real `hades-gate` binary (built here with `cargo build --release
//!    -p hades-gate` when missing) decides real requests against a throwaway
//!    admin key in a temp dir: one plan refused by the admin-safety rule, one
//!    forged token (minted with a second key), one approval and its replay,
//!    one `kill` request. Its ledger is what the nerve watches.
//! 2. `openmind-nerve serve` on 127.0.0.1:19580 with `NERVE_HADES_LEDGER`,
//!    `NERVE_HADES_AUDIT` (one hermes-tools-format escape line) and
//!    `NERVE_HADES_DISABLE_FILE`, window 25 s: the `hades` sensor reports,
//!    `/api/v1/cortex/events` serves them as B33 `NerveEvent` JSONL with
//!    counts and ids only, `?since=` works as a cursor, `hades-gate disable`
//!    shows up as `hades-off`, and everything clears after the window.

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PORT +0; `CORTEX_S2D_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 19580-19589.
static PORTS: Block = Block::new("CORTEX_S2D_REAL_PORT_BASE", 19580, 10);

#[allow(non_snake_case)]
fn PORT() -> u16 {
    PORTS.port(0)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, path: &str) -> Option<(String, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (head, body) = b.split_once("\r\n\r\n")?;
    Some((head.to_string(), body.to_string()))
}

fn state() -> Value {
    http(PORT(), "/api/v1/state")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn wait(what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state();
        if ok(&st) {
            return st;
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "timeout waiting for {what}: {}",
            st["cortex"]
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn hades_ids(st: &Value) -> Vec<String> {
    let mut v = vec![];
    for k in ["vulnerabilities", "bleeding"] {
        for f in st["cortex"][k].as_array().into_iter().flatten() {
            if f["source"] == "hades" {
                v.push(format!(
                    "{}:{}",
                    f["id"].as_str().unwrap_or(""),
                    f["severity"].as_str().unwrap_or("")
                ));
            }
        }
    }
    v.sort();
    v
}

fn hades_gate_bin() -> PathBuf {
    let bin = Path::new(BIN).with_file_name("hades-gate");
    if !bin.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let st = Command::new(cargo)
            .args(["build", "--release", "-j", "2", "-p", "hades-gate"])
            .status()
            .expect("cargo build hades-gate");
        assert!(st.success());
    }
    assert!(bin.exists(), "{}", bin.display());
    bin
}

/// Run hades-gate; returns (exit code, `code` field of its JSON verdict).
fn gate(bin: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(bin)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .expect("hades-gate");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_default();
    let code = v["code"]
        .as_str()
        .or(v["decision"].as_str())
        .unwrap_or("")
        .to_string();
    (o.status.code().unwrap_or(-1), code)
}

#[test]
fn real_cortex_s2_d_feed_and_hades_sensor() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-real-s2d-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = |n: &str| dir.join(n).to_string_lossy().into_owned();

    // 1. The real hades-gate, throwaway keys (never printed).
    let hg = hades_gate_bin();
    assert_eq!(
        gate(
            &hg,
            &[
                "keygen",
                "--seed",
                &p("admin.seed"),
                "--pub",
                &p("admin.pub")
            ]
        )
        .0,
        0
    );
    assert_eq!(
        gate(
            &hg,
            &["keygen", "--seed", &p("evil.seed"), "--pub", &p("evil.pub")]
        )
        .0,
        0
    );
    std::fs::write(
        dir.join("hades.toml"),
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 900\n",
            p("admin.pub"),
            p("ledger.jsonl"),
            p("hades.disabled")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("good.plan"),
        "restart galion-desk\nverify galion-desk\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("bad.plan"),
        "restart galion-desk; rm -rf /var/lib/secret-plan-dir\n",
    )
    .unwrap();
    let cfg = p("hades.toml");
    let check = |plan: &str, action: &str, token: Option<&str>| {
        let mut a = vec![
            "check",
            "--config",
            &cfg,
            "--action",
            action,
            "--target",
            "galion-desk",
            "--plan-file",
            plan,
        ];
        if let Some(t) = token {
            a.extend(["--token-file", t]);
        }
        gate(&hg, &a)
    };
    let mint = |seed: &str, out: &str| {
        gate(
            &hg,
            &[
                "mint",
                "--seed",
                seed,
                "--action",
                "heal-apply",
                "--target",
                "galion-desk",
                "--plan-file",
                &p("good.plan"),
                "--out",
                out,
            ],
        )
        .0
    };
    let mut decisions = vec![];
    decisions.push((
        "admin-safety plan",
        check(&p("bad.plan"), "heal-apply", None),
    ));
    assert_eq!(mint(&p("evil.seed"), &p("forged.tok")), 0);
    decisions.push((
        "forged token",
        check(&p("good.plan"), "heal-apply", Some(&p("forged.tok"))),
    ));
    assert_eq!(mint(&p("admin.seed"), &p("good.tok")), 0);
    decisions.push((
        "valid token",
        check(&p("good.plan"), "heal-apply", Some(&p("good.tok"))),
    ));
    decisions.push((
        "replayed token",
        check(&p("good.plan"), "heal-apply", Some(&p("good.tok"))),
    ));
    decisions.push(("kill", check(&p("good.plan"), "kill", None)));
    eprintln!("hades-gate decisions (exit, code): {decisions:?}");
    let codes: Vec<&str> = decisions.iter().map(|(_, (_, c))| c.as_str()).collect();
    assert_eq!(
        codes,
        ["admin-safety", "forged", "WouldApply", "replayed", "action"]
    );
    let ledger = std::fs::read_to_string(dir.join("ledger.jsonl")).unwrap();
    eprintln!(
        "ledger: {} lines (events: {:?})",
        ledger.lines().count(),
        ledger
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|v| v["event"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>()
    );
    // hermes-tools audit format (tools.rs `refuse`), written now.
    let now = zagreb_time::now_rfc3339();
    std::fs::write(
        dir.join("audit.jsonl"),
        format!("{{\"tool\":\"fs_read\",\"session\":\"sess_real58\",\"decision\":\"refused\",\"reason\":\"path traversal (..) refused: ../../etc/passwd\",\"args\":\"{{}}\",\"ts\":\"{now}\"}}\n"),
    )
    .unwrap();

    // 2. The nerve.
    let svc = TcpListener::bind(("127.0.0.1", PORT() + 1)).expect("slot port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"live-svc","port":{}}}]"#, PORT() + 1),
    )
    .unwrap();
    let mut c = Command::new(BIN);
    c.arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{}", PORT()))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", &PORTS.range())
        .env("NERVE_HADES_LEDGER", dir.join("ledger.jsonl"))
        .env("NERVE_HADES_AUDIT", dir.join("audit.jsonl"))
        .env("NERVE_HADES_DISABLE_FILE", dir.join("hades.disabled"))
        .env("NERVE_HADES_WINDOW_S", "25")
        .env_remove("NERVE_SENSORS_OFF")
        .stderr(Stdio::null());
    let nerve = Kill(c.spawn().expect("spawn nerve"));
    let t0 = Instant::now();
    let st = wait("hades findings", |s| hades_ids(s).len() >= 4);
    eprintln!(
        "after {:?}: cycle {} hades findings {:?}",
        t0.elapsed(),
        st["cycle"],
        hades_ids(&st)
    );
    assert_eq!(
        hades_ids(&st),
        [
            "hades-safety-refusals:warn",
            "hades-token-abuse:error",
            "hades-unapprovable-actions:warn",
            "sandbox-escape-attempts:warn"
        ]
    );
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "listeners",
            "scanners",
            "edge",
            "components",
            "ddos",
            "hades",
            // T2.12 edge-rate is on by default since cortex-merge-all (NERVE_EDGE_HOSTS=0 turns it off).
            "edge-rate"
        ]
    );
    let hs = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "hades")
        .unwrap()
        .clone();
    eprintln!(
        "sensor run {hs} cycle_ms {} incidents {}",
        st["cycle_ms"],
        st["incidents"]["open"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
    );

    // The feed: JSONL, B33, counts/ids only.
    let (head, body) = http(PORT(), "/api/v1/cortex/events").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("application/x-ndjson"), "{head}");
    let evs: Vec<NerveEvent> = body
        .lines()
        .map(|l| serde_json::from_str(l).expect("B33 line"))
        .collect();
    let hades = evs
        .iter()
        .find(|e| e.source == "hades")
        .expect("hades event in the feed");
    assert_eq!(
        (hades.kind.as_str(), hades.severity.as_str()),
        ("vulnerability", "high")
    );
    assert_eq!(hades.data["findings"], 4);
    assert_eq!(hades.data["top"]["id"], "hades-token-abuse");
    assert!(hades.ts.ends_with("+02:00") || hades.ts.ends_with("+01:00"));
    for leak in [
        "secret-plan-dir",
        "rm -",
        "galion-desk",
        "passwd",
        "sess_real58",
        "token_sha256",
        "title",
        "fix",
        "evidence\"",
    ] {
        assert!(!body.contains(leak), "{leak} leaked in feed: {body}");
    }
    let cycle = hades.data["cycle"].as_u64().unwrap();
    eprintln!(
        "feed line: {}",
        body.lines().find(|l| l.contains("\"hades\"")).unwrap()
    );
    // Cursor: nothing new for the current cycle, then the next one.
    let (_, same) = http(PORT(), &format!("/api/v1/cortex/events?since={cycle}")).unwrap();
    let (_, env) = http(
        PORT(),
        &format!("/api/v1/cortex/events?since={}&format=json", cycle - 1),
    )
    .unwrap();
    let env: Value = serde_json::from_str(&env).unwrap();
    assert!(
        same.is_empty() || state()["cycle"].as_u64() > Some(cycle),
        "since={cycle}: {same}"
    );
    assert!(
        env["events"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "{env}"
    );
    eprintln!(
        "since={cycle}: {} bytes; since={} format=json: {} events",
        same.len(),
        cycle - 1,
        env["events"].as_array().unwrap().len()
    );

    // Admin switches HADES off with the real CLI: hades-off (info, not in the feed).
    assert_eq!(gate(&hg, &["disable", "--config", &cfg]).0, 0);
    let st = wait("hades-off", |s| {
        hades_ids(s).iter().any(|f| f == "hades-off:info")
    });
    let (_, body) = http(PORT(), "/api/v1/cortex/events").unwrap();
    assert!(!body.contains("hades-off"), "info findings are not events");
    eprintln!("disabled: cycle {} {:?}", st["cycle"], hades_ids(&st));
    assert_eq!(gate(&hg, &["enable", "--config", &cfg]).0, 0);

    // Window 25 s: everything clears.
    let st = wait("clear", |s| hades_ids(s).is_empty());
    let (_, body) = http(PORT(), "/api/v1/cortex/events").unwrap();
    assert!(!body.contains("\"hades\""), "{body}");
    let rss = std::fs::read_to_string(format!("/proc/{}/status", nerve.0.id()))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .map(str::to_string)
        })
        .unwrap_or_default();
    eprintln!(
        "cleared after {:?}: cycle {} cycle_ms {} {rss}",
        t0.elapsed(),
        st["cycle"],
        st["cycle_ms"]
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
