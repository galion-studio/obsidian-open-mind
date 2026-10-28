// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 357 cortex-nerve-pair-runbook: Rust preflight for the staged
//! cortex + nerve deploy pair (D-341-1 cortex 58a12e61 + D-333-1 nerve
//! 9baa450c with the staged `components-live.json`), and its rollback pair
//! (cortex 73583ab + nerve 9d1a4f3 = live on 2026-10-28).
//!
//! Source of truth: `deploy/staged/cortex-nerve-pair/pair.json`. Runbook:
//! `docs/cutover/cortex-nerve-pair.md`.
//!
//! Hermetic (always run, no binaries, no network):
//!   * `staged_dropins_match_manifest`: each staged drop-in only swaps
//!     `ExecStart=` (reset + one new line) to the manifest `install_as` path,
//!     sorts after the live `90-deploy-t273u.conf`, names its rollback, and
//!     changes no env / user / sandbox key.
//!   * `staged_components_table`: sha256 = manifest, 13 entries, unique names,
//!     every `restarts_per_hour` 0, `deep` on `console`, alert-only
//!     `console-rust-primary` on :18080.
//!   * `manifest_sizes_inside_budget`.
//!
//! Real (`GALION_REAL=1`; artifacts from `GALION_PAIR_ARTIFACTS`, default
//! `/workspace/swarm/artifacts`), box loopback only, slot 357 ports
//! 22270-22279 (`PAIR_PORT_BASE` at build time moves the block):
//!   * `artifacts_match_manifest`: size, sha256 (galion-common, std only), ELF64
//!     x86_64 static (no PT_INTERP), `--version`, build markers (R5b + llm
//!     advisor in the cortex, planned-stop tag + deep probe in the nerve; the
//!     rollback nerve must NOT carry the tag).
//!   * `deploy_order_rehearsal`: the runbook order on one pair of persistent
//!     data dirs: live pair -> cortex swapped -> nerve swapped -> nerve rolled
//!     back -> cortex rolled back. After each swap: both `/health`, cortex
//!     re-subscribed to the nerve stream, then one real-shape planned restart
//!     (VPS console.service 2026-10-27 17:25:04 lines, unit renamed) and a
//!     crash (exit 1) through the nerve FIFO journal replay. New cortex =>
//!     planned restart is `R5b-planned-stop`; old cortex => `R3` (the live
//!     false positive the rollback brings back); crash => `R3-known-crash` in
//!     every step. Nerve is dry-run, nothing touches systemd.
//!
//! `GALION_REAL=1 cargo test --release -p gaia-cortex --test pair_preflight -- --nocapture --test-threads 1`

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const fn port_block(default: u16) -> u16 {
    match option_env!("PAIR_PORT_BASE") {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            let mut v: u32 = 0;
            assert!(!b.is_empty(), "PAIR_PORT_BASE empty");
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "PAIR_PORT_BASE not a number");
                v = v * 10 + (b[i] - b'0') as u32;
                assert!(v <= 65_526, "PAIR_PORT_BASE too large");
                i += 1;
            }
            v as u16
        }
    }
}

const BLOCK: u16 = port_block(22270);
const NERVE_PORT: u16 = BLOCK;
const CORTEX_PORT: u16 = BLOCK + 1;
const DEAD_PORT: u16 = BLOCK + 9;

const STAGED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/staged/cortex-nerve-pair"
);
const FIXTURE: &str = include_str!("fixtures/console-planned-restart-20261027.jsonl");
const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";
/// The live drop-in that the staged ones override (and fall back to).
const LIVE_DROPIN: &str = "90-deploy-t273u.conf";

fn manifest() -> Value {
    let p = Path::new(STAGED).join("pair.json");
    serde_json::from_str(&std::fs::read_to_string(&p).expect("pair.json")).expect("pair.json JSON")
}

fn sha256_hex(b: &[u8]) -> String {
    galion_common::sha256::hex(b)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[357 {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

// ------------------------------------------------------------------ hermetic

/// Parsed drop-in: (section, key, value) for every non-comment line.
fn dropin_lines(text: &str) -> Vec<(String, String, String)> {
    let mut sec = String::new();
    let mut out = Vec::new();
    for l in text.lines().map(str::trim) {
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            sec = l.to_string();
            continue;
        }
        let (k, v) = l.split_once('=').expect("key=value line");
        out.push((sec.clone(), k.trim().to_string(), v.trim().to_string()));
    }
    out
}

#[test]
fn staged_dropins_match_manifest() {
    let m = manifest();
    for role in ["cortex", "nerve"] {
        let c = &m["candidate"][role];
        let rel = c["dropin"].as_str().unwrap();
        let text = std::fs::read_to_string(Path::new(STAGED).join(rel)).expect(rel);
        assert!(
            text.starts_with(
                "# SPDX-License-Identifier: MIT\n# Copyright (c) 3000 Galion / galion-studio\n"
            ),
            "{rel}: SPDX header"
        );
        assert!(text.contains("STAGED, NOT INSTALLED"), "{rel}");
        let name = Path::new(rel).file_name().unwrap().to_str().unwrap();
        assert!(
            name > LIVE_DROPIN,
            "{rel} must sort after the live {LIVE_DROPIN} to win ExecStart"
        );
        assert!(
            text.contains("Rollback: rm this file") && text.contains(LIVE_DROPIN),
            "{rel}: rollback names the live drop-in it falls back to"
        );
        let sha = c["sha256"].as_str().unwrap();
        assert!(text.contains(sha), "{rel}: carries the artifact sha256");
        let lines = dropin_lines(&text);
        let exec: Vec<&(String, String, String)> =
            lines.iter().filter(|(_, k, _)| k == "ExecStart").collect();
        assert_eq!(exec.len(), 2, "{rel}: ExecStart reset + one line");
        assert_eq!(exec[0].2, "", "{rel}: first ExecStart= resets");
        assert_eq!(exec[0].0, "[Service]");
        let bin = exec[1].2.split_whitespace().next().unwrap();
        let install_as = c["install_as"].as_str().unwrap();
        assert_eq!(bin, install_as, "{rel}: ExecStart -> manifest install_as");
        let short = &c["commit"].as_str().unwrap()[..7];
        assert!(install_as.contains(short), "{install_as} names {short}");
        for (sec, k, _) in &lines {
            let ok =
                (sec == "[Service]" && k == "ExecStart") || (sec == "[Unit]" && k == "Description");
            assert!(
                ok,
                "{rel}: only ExecStart/Description may change, found {sec} {k}"
            );
        }
        // Same subcommand shape as the live drop-in it replaces.
        let args: Vec<&str> = exec[1].2.split_whitespace().skip(1).collect();
        match role {
            "cortex" => assert_eq!(args, ["serve", "--config", "/etc/gaia-cortex/config.json"]),
            _ => assert_eq!(args, ["serve", "--mode", "event"]),
        }
    }
    // rollback binaries are the live ones named in the manifest
    for role in ["cortex", "nerve"] {
        let r = &m["rollback"][role];
        assert_eq!(
            r["dropin"].as_str().unwrap().rsplit('/').next(),
            Some(LIVE_DROPIN)
        );
        let short = &r["commit"].as_str().unwrap()[..7];
        assert!(r["installed_as"].as_str().unwrap().contains(short));
    }
}

#[test]
fn staged_components_table() {
    let m = manifest();
    let raw = std::fs::read(Path::new(STAGED).join("components-live.json")).unwrap();
    assert_eq!(
        sha256_hex(&raw),
        m["candidate"]["components"]["sha256"].as_str().unwrap(),
        "staged components-live.json drifted from the manifest (copy of nerve-merge-6 9baa450c)"
    );
    let v: Value = serde_json::from_slice(&raw).unwrap();
    let a = v.as_array().expect("array");
    assert_eq!(
        a.len(),
        13,
        "live 12 (VPS 05:2x CET) + console-rust-primary"
    );
    let mut names: Vec<&str> = a.iter().map(|c| c["name"].as_str().unwrap()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), a.len(), "unique names");
    for c in a {
        assert_eq!(
            c["restarts_per_hour"].as_u64().unwrap_or(0),
            0,
            "nerve own reflex stays inert: {c}"
        );
    }
    let by = |n: &str| a.iter().find(|c| c["name"] == n).cloned().unwrap();
    let deep = json!(["/api/v1/admin", "/api/v1/services"]);
    assert_eq!(by("console")["deep"], deep);
    let p = by("console-rust-primary");
    assert_eq!(p["port"], 18080);
    assert_eq!(p["critical"], false);
    assert_eq!(p["deep"], deep);
    // one incident with the :8080 console (whose group is its own name)
    assert_eq!(p["group"], "console");
    assert_eq!(p["restarts_per_hour"], 0);
}

#[test]
fn manifest_sizes_inside_budget() {
    let m = manifest();
    let budget = m["size_budget_bytes"].as_u64().unwrap();
    for side in ["candidate", "rollback"] {
        for role in ["cortex", "nerve"] {
            let s = m[side][role]["size"].as_u64().unwrap();
            assert!(s <= budget, "{side} {role} {s} B > {budget} B");
            assert_eq!(m[side][role]["sha256"].as_str().unwrap().len(), 64);
        }
    }
}

// ------------------------------------------------------------------ real: artifacts

fn real() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (GALION_PAIR_ARTIFACTS=<dir>, default /workspace/swarm/artifacts)");
        return None;
    }
    Some(PathBuf::from(
        std::env::var("GALION_PAIR_ARTIFACTS")
            .unwrap_or_else(|_| "/workspace/swarm/artifacts".into()),
    ))
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// ELF64 little-endian x86_64 executable without a PT_INTERP (static).
fn elf_static_x86_64(b: &[u8]) -> Result<(), String> {
    if b.len() < 64 || &b[..4] != b"\x7fELF" {
        return Err("not ELF".into());
    }
    if b[4] != 2 || b[5] != 1 {
        return Err("not ELF64 LE".into());
    }
    if u16le(b, 0x12) != 0x3e {
        return Err(format!("e_machine {:#x} != x86_64", u16le(b, 0x12)));
    }
    let (off, sz, num) = (
        u64le(b, 0x20) as usize,
        u16le(b, 0x36) as usize,
        u16le(b, 0x38) as usize,
    );
    for i in 0..num {
        let p = off + i * sz;
        if p + 4 > b.len() {
            return Err("phdr out of file".into());
        }
        if u32le(b, p) == 3 {
            return Err("PT_INTERP present (dynamic)".into());
        }
    }
    Ok(())
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn bin_of(dir: &Path, m: &Value, side: &str, role: &str) -> PathBuf {
    dir.join(m[side][role]["artifact"].as_str().unwrap())
}

#[test]
fn artifacts_match_manifest() {
    let Some(dir) = real() else { return };
    let m = manifest();
    let budget = m["size_budget_bytes"].as_u64().unwrap();
    let mut table = Vec::new();
    for side in ["candidate", "rollback"] {
        for role in ["cortex", "nerve"] {
            let e = &m[side][role];
            let p = bin_of(&dir, &m, side, role);
            let b = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let sha = sha256_hex(&b);
            assert_eq!(
                b.len() as u64,
                e["size"].as_u64().unwrap(),
                "{side} {role} size"
            );
            assert!(b.len() as u64 <= budget, "{side} {role} over budget");
            assert_eq!(sha, e["sha256"].as_str().unwrap(), "{side} {role} sha256");
            elf_static_x86_64(&b).unwrap_or_else(|x| panic!("{side} {role}: {x}"));
            for mk in e["markers"].as_array().unwrap() {
                let mk = mk.as_str().unwrap();
                assert!(
                    contains(&b, mk),
                    "{side} {role}: build marker {mk:?} missing"
                );
            }
            let out = Command::new(&p)
                .arg("--version")
                .output()
                .expect("--version");
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let crate_name = if role == "cortex" {
                "gaia-cortex"
            } else {
                "openmind-nerve"
            };
            assert!(
                out.status.success() && v.starts_with(crate_name),
                "{side} {role} --version: {v}"
            );
            table.push(format!(
                "{side:9} {role:6} {} {} B sha256 {}.. static ok, --version {v:?}",
                e["commit"].as_str().unwrap(),
                b.len(),
                &sha[..16]
            ));
        }
    }
    // negative controls: the markers really tell the builds apart
    let rb_nerve = std::fs::read(bin_of(&dir, &m, "rollback", "nerve")).unwrap();
    assert!(
        !contains(&rb_nerve, "unit.planned_stop"),
        "rollback nerve must be the untagged 9d1a4f3"
    );
    let rb_cortex = std::fs::read(bin_of(&dir, &m, "rollback", "cortex")).unwrap();
    assert!(
        !contains(&rb_cortex, "R5b-planned-stop"),
        "rollback cortex must be pre-R5b 73583ab"
    );
    for t in table {
        log(t);
    }
    log("negative controls: rollback nerve has no unit.planned_stop, rollback cortex has no R5b");
}

// ------------------------------------------------------------------ real: rehearsal

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let t = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), t) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(t));
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() || s.write_all(body.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).to_string();
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let b = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, b)
}

fn get(port: u16, path: &str) -> Value {
    let (c, b) = http(port, "GET", path, "");
    if c != 200 {
        return Value::Null;
    }
    serde_json::from_str(&b).unwrap_or(Value::Null)
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout ({secs} s) waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    t.elapsed()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Nerve {
    _p: Proc,
    j: std::fs::File,
}

fn start_nerve(bin: &Path, dir: &Path, n: usize) -> (Nerve, Duration) {
    let fifo = dir.join(format!("journal-{n}.fifo"));
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let t = Instant::now();
    let p = Proc(
        Command::new(bin)
            .args([
                "serve",
                "--mode",
                "event",
                "--listen",
                &format!("127.0.0.1:{NERVE_PORT}"),
                "--log",
                &dir.join("nerve-events.jsonl").display().to_string(),
            ])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("NERVE_DATA_DIR", dir.join("nerve-data"))
            .env("NERVE_COMPONENTS", dir.join("components.json"))
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_ROLLBACK", "0")
            .env("NERVE_EVT_ACTIONS", "dry-run")
            .env("NERVE_EVT_ESCALATE", "off")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_EVT_PIDFD", "0")
            .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
            .env("NERVE_EVT_DEDUPE", "600")
            .env("NERVE_EVT_DEBOUNCE", "1")
            .env("NERVE_EVT_MAX_WAIT", "4")
            .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
            .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join(format!("nerve-{n}.log"))).unwrap())
            .spawn()
            .expect("spawn nerve"),
    );
    wait_until("nerve /health", 30, || {
        http(NERVE_PORT, "GET", "/health", "").0 == 200
    });
    let up = t.elapsed();
    let j = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    (Nerve { _p: p, j }, up)
}

fn start_cortex(bin: &Path, dir: &Path, n: usize) -> (Proc, Duration) {
    let t = Instant::now();
    let p = Proc(
        Command::new(bin)
            .arg("serve")
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("CORTEX_CONFIG", dir.join("cortex.json"))
            .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
            .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
            .env("CORTEX_NERVE_STREAM", "1")
            .env("CORTEX_PULL_EVERY_S", "0")
            .env("CORTEX_ACTIONS", "dry-run")
            .env("CORTEX_LLM_ENABLED", "0")
            .env("CORTEX_DATA_DIR", dir.join("cortex-data"))
            .env("CORTEX_COALESCE_MS", "1500")
            .env("CORTEX_REPEAT_POLL_S", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join(format!("cortex-{n}.log"))).unwrap())
            .spawn()
            .expect("spawn cortex"),
    );
    wait_until("cortex /health", 20, || {
        http(CORTEX_PORT, "GET", "/health", "").0 == 200
    });
    (p, t.elapsed())
}

fn subscribed() -> bool {
    get(NERVE_PORT, "/stats")["streams"].as_u64() == Some(1)
        && get(CORTEX_PORT, "/health")["nerve"] == true
}

fn write_line(j: &mut std::fs::File, mut v: Value, ts_us: u64) {
    v["__REALTIME_TIMESTAMP"] = json!(ts_us.to_string());
    v["_PID"] = json!("1");
    writeln!(j, "{v}").unwrap();
    j.flush().unwrap();
}

/// First live restart of the fixture (6 systemd lines, 17:25:04 +01:00) with
/// its real gaps, unit and invocation renamed for this step.
fn planned_restart(j: &mut std::fs::File, unit: &str, step: usize) {
    let lines: Vec<Value> = FIXTURE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let ts = |v: &Value| -> u64 { v["__REALTIME_TIMESTAMP"].as_str().unwrap().parse().unwrap() };
    let t0 = ts(&lines[0]);
    assert!(ts(&lines[5]) - t0 < 5_000_000, "first restart within 5 s");
    let base = now_unix() * 1_000_000;
    let start = Instant::now();
    for v in lines {
        let off = ts(&v) - t0;
        let due = Duration::from_micros(off);
        if due > start.elapsed() {
            std::thread::sleep(due - start.elapsed());
        }
        let s = v.to_string().replace("console.service", unit);
        let mut v: Value = serde_json::from_str(&s).unwrap();
        if let Some(inv) = v["INVOCATION_ID"].as_str() {
            v["INVOCATION_ID"] = json!(format!("357s{step}{}", &inv[5..]));
        }
        write_line(j, v, base + off);
    }
}

fn crash(j: &mut std::fs::File, unit: &str, step: usize) {
    let inv = format!("357crash{step}000000000000000000000000");
    let t = now_unix() * 1_000_000;
    let l = |id: &str, extra: Value| {
        let mut v = json!({"MESSAGE_ID": id, "UNIT": unit, "INVOCATION_ID": inv,
                           "MESSAGE": format!("{unit}: slot 357 crash"), "_TRANSPORT": "journal"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    write_line(
        j,
        l(
            ID_PROCESS_EXIT,
            json!({"EXIT_CODE": "exited", "EXIT_STATUS": "1"}),
        ),
        t,
    );
    write_line(
        j,
        l(ID_FAILURE_RESULT, json!({"UNIT_RESULT": "exit-code"})),
        t + 200,
    );
}

fn rules_for(unit: &str) -> Vec<String> {
    let subj = format!("unit:{unit}");
    let mut v: Vec<Value> = get(CORTEX_PORT, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subj.as_str() && d["rule"] != "R10-remediation")
        .collect();
    v.sort_by_key(|d| {
        d["id"]
            .as_str()
            .and_then(|s| s.strip_prefix("dec-"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    v.iter()
        .map(|d| d["rule"].as_str().unwrap_or("").to_string())
        .collect()
}

fn nerve_tagged(unit: &str) -> bool {
    get(NERVE_PORT, "/evt?since=0&n=1000")
        .as_array()
        .is_some_and(|a| {
            a.iter()
                .any(|e| e["unit"] == unit && e["kind"] == "unit.planned_stop")
        })
}

#[test]
fn deploy_order_rehearsal() {
    let Some(adir) = real() else { return };
    let m = manifest();
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (block {BLOCK}-{})",
            BLOCK + 9
        );
    }
    let new_c = bin_of(&adir, &m, "candidate", "cortex");
    let new_n = bin_of(&adir, &m, "candidate", "nerve");
    let old_c = bin_of(&adir, &m, "rollback", "cortex");
    let old_n = bin_of(&adir, &m, "rollback", "nerve");
    let dir = std::env::temp_dir().join(format!("pair-357-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("nerve-data")).unwrap();
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"p357-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
    )
    .unwrap();
    // live /etc/gaia-cortex/config.json values that matter here
    std::fs::write(
        dir.join("cortex.json"),
        json!({"restarts_per_unit_hour": 3, "decision_cooldown_s": 120}).to_string(),
    )
    .unwrap();

    // (label, swap, nerve, cortex, planned unit, crash unit, cortex has R5b)
    #[derive(Clone, Copy, PartialEq)]
    enum Swap {
        Start,
        Cortex,
        Nerve,
    }
    type Step<'a> = (
        &'a str,
        Swap,
        &'a Path,
        &'a Path,
        &'a str,
        Option<&'a str>,
        bool,
    );
    let steps: [Step; 5] = [
        (
            "0 live pair 9d1a4f3+73583ab",
            Swap::Start,
            &old_n,
            &old_c,
            "console.service",
            Some("hermes-rs.service"),
            false,
        ),
        (
            "1 cortex -> 58a12e6 (nerve 9d1a4f3)",
            Swap::Cortex,
            &old_n,
            &new_c,
            "galion-desk.service",
            Some("galion-voice.service"),
            true,
        ),
        (
            "2 nerve -> 9baa450 (pair live)",
            Swap::Nerve,
            &new_n,
            &new_c,
            "tuner.service",
            Some("link-watch-rs.service"),
            true,
        ),
        (
            "3 rollback nerve -> 9d1a4f3",
            Swap::Nerve,
            &old_n,
            &new_c,
            "galion-one.service",
            None,
            true,
        ),
        (
            "4 rollback cortex -> 73583ab",
            Swap::Cortex,
            &old_n,
            &old_c,
            "openmind-memory.service",
            Some("galion-health.service"),
            false,
        ),
    ];
    let mut nerve: Option<Nerve> = None;
    let mut cortex: Option<Proc> = None;
    let mut summary: Vec<String> = Vec::new();
    let mut fails = Vec::new();
    for (i, (label, swap, nb, cb, planned, crashu, r5b)) in steps.into_iter().enumerate() {
        let t = Instant::now();
        let (mut n_up, mut c_up) = (Duration::ZERO, Duration::ZERO);
        if swap != Swap::Cortex {
            drop(nerve.take()); // "systemctl restart galion-nerve"
            let (n, up) = start_nerve(nb, &dir, i);
            nerve = Some(n);
            n_up = up;
        }
        if swap != Swap::Nerve {
            drop(cortex.take()); // "systemctl restart gaia-cortex"
            let (c, up) = start_cortex(cb, &dir, i);
            cortex = Some(c);
            c_up = up;
        }
        let sub = wait_until("cortex re-subscribed to the nerve stream", 90, subscribed);
        let ch = get(CORTEX_PORT, "/health");
        let nh = get(NERVE_PORT, "/health");
        assert_eq!(ch["ok"], true, "{label}: cortex /health {ch}");
        assert_eq!(ch["busy"], false, "{label}: cortex busy {ch}");
        assert_eq!(nh["status"], "ok", "{label}: nerve /health {nh}");
        std::thread::sleep(Duration::from_secs(2));
        let j = &mut nerve.as_mut().unwrap().j;
        planned_restart(j, planned, i);
        std::thread::sleep(Duration::from_secs(8));
        let pr = rules_for(planned);
        let tagged = nerve_tagged(planned);
        let preq = restart_requests(planned);
        let mut cr = Vec::new();
        let mut creq = 0;
        if let Some(cu) = crashu {
            crash(&mut nerve.as_mut().unwrap().j, cu, i);
            std::thread::sleep(Duration::from_secs(6));
            cr = rules_for(cu);
            creq = restart_requests(cu);
        }
        let actions = get(NERVE_PORT, "/actions")
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        let line = format!(
            "step {label}: nerve up {} ms, cortex up {} ms, re-subscribed {} ms after swap ({} ms total); \
             planned {planned} -> {pr:?} (nerve tag {tagged}, restart requests {preq}); crash {} -> {cr:?} (restart requests {creq}); nerve /actions {actions}; \
             cortex events_in {} incidents_open {}",
            n_up.as_millis(),
            c_up.as_millis(),
            sub.as_millis(),
            t.elapsed().as_millis(),
            crashu.unwrap_or("-"),
            ch["events_in"],
            get(CORTEX_PORT, "/health")["incidents_open"],
        );
        log(&line);
        summary.push(line);
        let crashy = |r: &[String]| r.iter().any(|x| x.starts_with("R3") || x.starts_with("R2"));
        if r5b {
            if !pr.iter().any(|x| x == "R5b-planned-stop") || crashy(&pr) {
                fails.push(format!(
                    "{label}: planned restart expected R5b and no R3/R2, got {pr:?}"
                ));
            }
        } else if !pr.iter().any(|x| x.starts_with("R3")) {
            fails.push(format!(
                "{label}: old cortex expected R3 (live false positive), got {pr:?}"
            ));
        }
        if (nb == new_n.as_path()) != tagged {
            fails.push(format!(
                "{label}: nerve tag {tagged} does not match the nerve build"
            ));
        }
        if r5b && preq != 0 {
            fails.push(format!(
                "{label}: planned restart sent {preq} restart request(s)"
            ));
        }
        if !r5b && preq == 0 {
            fails.push(format!(
                "{label}: old cortex expected a restart request (live false positive)"
            ));
        }
        if crashu.is_some() && creq != 1 {
            fails.push(format!(
                "{label}: crash expected 1 restart request, got {creq}"
            ));
        }
        if crashu.is_some() && !cr.iter().any(|x| x == "R3-known-crash") {
            fails.push(format!(
                "{label}: crash must stay R3-known-crash, got {cr:?}"
            ));
        }
    }
    drop(cortex.take());
    drop(nerve.take());
    for p in [NERVE_PORT, CORTEX_PORT] {
        wait_until("port closed", 10, || {
            TcpListener::bind(("127.0.0.1", p)).is_ok()
        });
    }
    let _ = std::fs::remove_dir_all(&dir);
    log(format!(
        "ports {NERVE_PORT}/{CORTEX_PORT} closed, {} removed",
        dir.display()
    ));
    assert!(
        fails.is_empty(),
        "rehearsal gates failed:\n{}",
        fails.join("\n")
    );
}

/// Restart requests the cortex sent to the nerve `POST /actions` for `unit`
/// (in-memory in the nerve, so per nerve process).
fn restart_requests(unit: &str) -> usize {
    get(NERVE_PORT, "/actions").as_array().map_or(0, |a| {
        a.iter()
            .filter(|x| x["unit"] == unit && x["action"] == "restart")
            .count()
    })
}
