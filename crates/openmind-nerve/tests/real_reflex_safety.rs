// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of nerve plan T3.2 reflex safety (slot 186, ports
//! 20560-20569). Gated: `GALION_REAL=1` (box), plus `GALION_REAL_VPS=1` for
//! the read-only VPS part. Run with `--release` for the release binary.
//!
//! 1. The real binary's `replay DIR --reflex` (forced dry-run) over the
//!    replay corpus: the fixture and every `/workspace/swarm/nerve-capture/
//!    replay*` capture present on the box, plain and `--worst-case --stretch
//!    130` (65 more minutes of the last captured condition, rollback armed):
//!    zero cloudflared restarts, zero rollbacks, edge dead routes alert-only.
//! 2. Positive controls through the same binary: hermes-rs failed -> 3
//!    restarts + 1 rollback (exit 1); cloudflared 0 edge connections -> 1
//!    restart, no rollback (exit 1). The check is not vacuous.
//! 3. `reflex-audit` passes on the built-in table and fails on a table that
//!    asks to restart tuner-quick.
//! 4. `serve` on 127.0.0.1:20560 with NERVE_DRY_RUN=1: `/state` shows mode
//!    dry-run, the allowlist budgets, no actions.
//! 5. VPS (read-only GETs over `ssh slavik-vps`): live `/state` and `/audit`
//!    of galion-nerve :8670 (mode, budgets, actions, per-component reflex
//!    budget vs this build's table), then 3 fresh `/state` snapshots + `ss
//!    -ltn` + 35 min of journal are replayed dry with the worst case armed.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 20560;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-real-186-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `openmind-nerve replay DIR --reflex ...` -> (exit code, verdict).
fn dry(dir: &Path, extra: &[&str]) -> (Option<i32>, Value) {
    let data = tmp("data");
    let o = Command::new(BIN)
        .env_clear()
        .env("NERVE_DATA_DIR", &data)
        .arg("replay")
        .arg(dir)
        .arg("--reflex")
        .args(extra)
        .stderr(Stdio::null())
        .output()
        .expect("run replay --reflex");
    let _ = std::fs::remove_dir_all(&data);
    let v = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    (o.status.code(), v)
}

fn line(name: &str, mode: &str, code: Option<i32>, v: &Value) -> String {
    format!(
        "| {name} | {mode} | {} | {} | {} | {} | {} | {} | {}/{} | {:?} |",
        v["cycles"],
        v["restarts"],
        v["cloudflared_restarts"],
        v["rollbacks"],
        v["holds"],
        v["rate_limited"],
        v["score"],
        v["grade"].as_str().unwrap_or("?"),
        code
    ) + &format!(
        " cloudflared {} consecutive_down {} edge_alert_only {}",
        v["cloudflared"]["status"], v["cloudflared"]["consecutive_down"], v["edge_alert_only"]
    )
}

fn assert_safe(name: &str, code: Option<i32>, v: &Value) {
    assert_eq!(code, Some(0), "{name}: {v:#}");
    assert_eq!(v["pass"], true, "{name}");
    assert_eq!(v["cloudflared_restarts"], 0, "{name}");
    assert_eq!(v["rollbacks"], 0, "{name}");
    assert_eq!(v["mode"], "dry-run (forced; nothing executed)");
}

fn corpus() -> Vec<(String, PathBuf)> {
    let mut v = vec![(
        "fixture cortex-replay".to_string(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay"),
    )];
    let root = PathBuf::from(
        std::env::var("NERVE_CAPTURE_ROOT").unwrap_or("/workspace/swarm/nerve-capture".into()),
    );
    let mut caps: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("replay"))
                && std::fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with("state"))
        })
        .collect();
    caps.sort();
    for p in caps {
        v.push((p.file_name().unwrap().to_string_lossy().into_owned(), p));
    }
    v
}

#[test]
fn real_reflex_dry_replay_over_the_corpus() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    println!("== {} reflex dry-run replay (binary {BIN})", stamp());
    println!("| capture | mode | cycles | restarts | cloudflared | rollbacks | holds | limited | score | exit |");
    let caps = corpus();
    assert!(caps.len() >= 2, "no live capture on this box");
    for (name, dir) in &caps {
        let (c, v) = dry(dir, &[]);
        println!("{}", line(name, "plain", c, &v));
        assert_safe(name, c, &v);
        let (c, v) = dry(dir, &["--worst-case", "--stretch", "130"]);
        println!("{}", line(name, "worst+130", c, &v));
        assert_safe(name, c, &v);
        assert_eq!(v["restarts"], 0, "{name}: {v:#}");
        if v["edge_incidents"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            assert_eq!(v["edge_alert_only"], true, "{name}");
        }
    }
}

/// Copy of the fixture with `edit` applied to every captured component.
fn mutated(tag: &str, edit: &dyn Fn(&mut Value)) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay");
    let d = tmp(tag);
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        let mut t = std::fs::read_to_string(e.path()).unwrap();
        if n.starts_with("state") {
            let mut v: Value = serde_json::from_str(&t).unwrap();
            for c in v["components"].as_array_mut().unwrap() {
                edit(c);
            }
            t = v.to_string();
        }
        std::fs::write(d.join(n), t).unwrap();
    }
    d
}

#[test]
fn real_reflex_positive_controls_and_allowlist_audit() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let d = mutated("hermes", &|c| {
        if c["name"] == "hermes-rs" {
            c["unit_state"] = "failed".into();
            c["http"] = Value::Null;
        }
    });
    let (code, v) = dry(&d, &["--worst-case", "--stretch", "20", "--log"]);
    println!(
        "control hermes-rs failed: exit {code:?} restarts {} rollbacks {} actions {}",
        v["restarts"], v["rollbacks"], v["actions"]
    );
    assert_eq!(code, Some(1));
    assert_eq!(v["actions"]["restart"]["hermes-rs.service"], 3);
    assert_eq!(v["rollbacks"], 1);
    for a in v["log"].as_array().unwrap() {
        assert_eq!(a["mode"], "dry-run");
        if let Some(r) = a["result"].as_str() {
            assert!(
                r.starts_with("would run:")
                    || r.starts_with("no action")
                    || r.starts_with("stopped"),
                "{a}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&d);
    let d = mutated("cf", &|c| {
        if c["name"] == "cloudflared" {
            c["tunnel"]["ha_connections"] = 0.into();
        }
    });
    let (code, v) = dry(&d, &["--worst-case", "--stretch", "30"]);
    println!(
        "control cloudflared 0 conns: exit {code:?} cloudflared_restarts {} rollbacks {}",
        v["cloudflared_restarts"], v["rollbacks"]
    );
    assert_eq!(code, Some(1));
    assert_eq!(v["cloudflared_restarts"], 1);
    assert_eq!(v["rollbacks"], 0);
    let _ = std::fs::remove_dir_all(&d);

    let o = Command::new(BIN)
        .env_clear()
        .arg("reflex-audit")
        .output()
        .unwrap();
    let a: Value = serde_json::from_slice(&o.stdout).unwrap();
    println!(
        "reflex-audit built-in: exit {:?} violations {} notes {}",
        o.status.code(),
        a["violations"],
        a["notes"]
    );
    assert_eq!(o.status.code(), Some(0));
    for r in a["components"].as_array().unwrap() {
        println!(
            "  {} {} -> {} rollback {}",
            r["component"], r["unit"], r["reflex"], r["rollback_eligible"]
        );
    }
    let d = tmp("components");
    let f = d.join("c.json");
    std::fs::write(
        &f,
        r#"[{"name":"q","unit":"tuner-quick.service","restarts_per_hour":1},{"name":"s","unit":"sshd.service","restarts_per_hour":3}]"#,
    )
    .unwrap();
    let o = Command::new(BIN)
        .env_clear()
        .env("NERVE_COMPONENTS", &f)
        .arg("reflex-audit")
        .output()
        .unwrap();
    let a: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    println!(
        "reflex-audit hostile table: exit {:?} violations {}",
        o.status.code(),
        a["violations"]
    );
    assert_eq!(o.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&d);
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

#[test]
fn real_serve_dry_run_on_the_box() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let data = tmp("serve");
    let _k = Kill(
        Command::new(BIN)
            .env_clear()
            .env("NERVE_DATA_DIR", &data)
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .args(["serve", "--listen", &format!("127.0.0.1:{PORT}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("serve"),
    );
    let t = Instant::now();
    let st = loop {
        if let Some(v) = get(PORT, "/state").filter(|v| v["cycle"].as_u64() >= Some(2)) {
            break v;
        }
        assert!(t.elapsed() < Duration::from_secs(40), "no /state");
        std::thread::sleep(Duration::from_millis(500));
    };
    let audit = get(PORT, "/audit").unwrap();
    println!(
        "box serve :{PORT} {} cycle {} mode {} recent {} audit {} budgets {}",
        stamp(),
        st["cycle"],
        st["mode"],
        st["reflex"]["recent"],
        audit,
        st["reflex"]["budgets"].as_object().map_or(0, |m| m.len())
    );
    assert_eq!(st["mode"], "dry-run");
    assert_eq!(st["reflex"]["budgets"].as_object().unwrap().len(), 10);
    assert_eq!(st["reflex"]["limits"]["cloudflared_per_hour"], 1);
    assert!(!data.join("reflex.json").exists() || audit.as_array().is_some());
    drop(_k);
    let _ = std::fs::remove_dir_all(&data);
}

/// Fixed read-only script: GETs on the nerve's loopback API, `ss -ltn`,
/// a journal read. Nothing is written on the VPS.
const VPS_SCRIPT: &str = "for i in 1 2 3; do echo '#STATE'; curl -s -m 10 http://127.0.0.1:8670/state; echo; [ $i -lt 3 ] && sleep 31; done; echo '#AUDIT'; curl -s -m 10 http://127.0.0.1:8670/audit; echo; echo '#SS'; ss -ltn; echo '#JOURNAL'; journalctl -o json --since -35min --output-fields=_SYSTEMD_UNIT,PRIORITY,MESSAGE,__REALTIME_TIMESTAMP -u cloudflared -u hermes-rs -u openmind-memory -u galion-one -u tuner -u galion-desk -u console -u galion-health -u link-watch-rs --no-pager";

#[test]
fn real_vps_live_reflex_state_and_fresh_capture() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "slavik-vps",
            VPS_SCRIPT,
        ])
        .output()
        .expect("ssh");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let mut blocks: Vec<(String, String)> = vec![];
    for l in out.lines() {
        if let Some(k) = l.strip_prefix('#') {
            blocks.push((k.to_string(), String::new()));
        } else if let Some(b) = blocks.last_mut() {
            b.1.push_str(l);
            b.1.push('\n');
        }
    }
    let states: Vec<Value> = blocks
        .iter()
        .filter(|b| b.0 == "STATE")
        .filter_map(|b| serde_json::from_str(b.1.trim()).ok())
        .collect();
    assert_eq!(states.len(), 3, "live /state x3");
    let audit: Value = blocks
        .iter()
        .find(|b| b.0 == "AUDIT")
        .and_then(|b| serde_json::from_str(b.1.trim()).ok())
        .unwrap_or(Value::Null);
    let last = &states[2];
    let budgets = last["reflex"]["budgets"].as_object().unwrap();
    let used: u64 = budgets
        .values()
        .map(|b| b["live_1h"].as_u64().unwrap_or(0) + b["dry_1h"].as_u64().unwrap_or(0))
        .sum();
    println!(
        "VPS {} live nerve cycle {} mode {} score {} system {}; reflex recent {} audit {}; budgets used 1h {used}; allowlist {:?}",
        stamp(),
        last["cycle"],
        last["mode"],
        last["self_eval"]["score"],
        last["system"]["status"],
        last["reflex"]["recent"].as_array().map_or(0, Vec::len),
        audit.as_array().map_or(0, Vec::len),
        budgets.keys().collect::<Vec<_>>()
    );
    let ours: Vec<&str> = openmind_nerve::reflex::ALLOW.to_vec();
    let mut live: Vec<&str> = budgets.keys().map(String::as_str).collect();
    let mut o2 = ours.clone();
    live.sort();
    o2.sort();
    assert_eq!(live, o2, "live allowlist differs from this build");
    let specs = openmind_nerve::config::default_specs(None, None);
    for c in last["components"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap_or("");
        let rph = c["reflex_restarts_per_hour"].as_u64();
        let want = specs
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.restarts_per_hour as u64);
        println!(
            "  live {name}: status {} unit_state {} consecutive_down {} reflex budget {rph:?} (ours {want:?})",
            c["status"], c["unit_state"], c["consecutive_down"]
        );
        if let (Some(r), Some(w)) = (rph, want) {
            assert_eq!(r, w, "{name}: live reflex budget differs");
        }
    }
    // Fresh capture -> dry replay with the worst case armed.
    let dir = tmp("vps-capture");
    for (i, s) in states.iter().enumerate() {
        std::fs::write(dir.join(format!("state-{}.json", i + 1)), s.to_string()).unwrap();
    }
    for (k, f) in [("SS", "ss.txt"), ("JOURNAL", "journal.jsonl")] {
        if let Some(b) = blocks.iter().find(|b| b.0 == k) {
            std::fs::write(dir.join(f), &b.1).unwrap();
        }
    }
    for extra in [&[][..], &["--worst-case", "--stretch", "130"][..]] {
        let (c, v) = dry(&dir, extra);
        println!(
            "{}",
            line(
                "vps-now",
                if extra.is_empty() {
                    "plain"
                } else {
                    "worst+130"
                },
                c,
                &v
            )
        );
        assert_safe("vps-now", c, &v);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
