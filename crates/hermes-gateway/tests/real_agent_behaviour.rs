// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Zero-spend behaviour test of Hermes's agent tools in the live layout.
//! Gated: `GALION_REAL=1` (otherwise it returns at once). Rust port of the
//! old `scripts/real/hermes-agent-behaviour.sh` (6cdbc2c0), no Python.
//!
//! The real `hermes-gateway` (this crate's build) runs with the same tool
//! switches as the live drop-ins (guard workspace = files root = tools
//! workspace, files write on, 4096 MB workspace, browser/vision off), every
//! path under a temp folder, against the scripted mock model
//! `hermes-mock-model` (crates/hermes-tools) instead of a real provider:
//! `HERMES_RS_PROVIDER_LOOPBACK_ONLY=1`, no provider key, cost 0.
//!
//! Two passes, each with a fresh gateway, mock and folder:
//! - strict (no `HERMES_RS_GUARD_SCRIPT_MODE`): 29 checks, the workspace
//!   shell script is refused;
//! - script mode (`HERMES_RS_GUARD_SCRIPT_MODE=1`): 30 checks, the good
//!   script runs and writes its file; the curl script is still refused.
//!
//! Session "tools": terminal, read/write/edit/list/search, files_list/read,
//! then rm -rf /, curl, /etc/shadow, a ../ write, a 1 s timeout on sleep 5,
//! and `ls` of a folder outside the workspace. Session "script": write
//! jobs/count.sh, run it, write jobs/bad.sh (curl), run it.
//!
//! Ports (slot 17 hermes-tools, 19170-19179), 127.0.0.1 only:
//! 19170 gateway, 19171 mock model. Knobs: `HAB_MOCK=<path>` skips the mock
//! build; `HAB_ONLY=strict|script` runs one pass.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const HERMES: &str = env!("CARGO_BIN_EXE_hermes-gateway");
const GW_PORT: u16 = 19170;
const MOCK_PORT: u16 = 19171;

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

fn zagreb_now() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %Z")
        .env("TZ", "Europe/Zagreb")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("crates/hermes-tools/tests/fixtures/agent-mock-scripts")
}

/// Test-only random gateway key (never printed).
fn test_key() -> String {
    let mut b = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("urandom");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The mock model binary: `HAB_MOCK`, or a release build in its own target
/// dir under CARGO_TARGET_TMPDIR (so it never waits on this build's lock).
fn mock_bin() -> PathBuf {
    if let Ok(p) = std::env::var("HAB_MOCK") {
        return PathBuf::from(p);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let tdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hab-mock-target");
    let st = Command::new("nice")
        .args(["-n", "10"])
        .arg(cargo)
        .args([
            "build",
            "-q",
            "--release",
            "-j",
            "4",
            "-p",
            "hermes-tools",
            "--bin",
            "hermes-mock-model",
        ])
        .current_dir(repo())
        .env("CARGO_TARGET_DIR", &tdir)
        .status()
        .expect("run cargo build for hermes-mock-model");
    assert!(st.success(), "building hermes-mock-model failed");
    tdir.join("release/hermes-mock-model")
}

fn port_free(p: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], p).into(), Duration::from_millis(200)).is_err()
}

/// One HTTP/1.0 request; returns (status, body).
fn http(port: u16, method: &str, path: &str, hdrs: &[(&str, &str)], body: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(300)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in hdrs {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    (code, body.to_string())
}

fn wait_health(port: u16) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(30) {
        if http(port, "GET", "/health", &[], "").0 == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn refused(t: &str) -> bool {
    t.contains("\"refused") || t.contains("deny list") || t.to_lowercase().contains("refused")
}

struct Checks {
    pass: usize,
    fail: Vec<String>,
}
impl Checks {
    fn check(&mut self, name: &str, ok: bool, detail: &str) {
        if ok {
            self.pass += 1;
            println!("PASS {name}");
        } else {
            let d: String = detail.chars().take(300).collect();
            println!("FAIL {name}  <- {d}");
            self.fail.push(name.to_string());
        }
    }
}

/// Tool results per script: res[script][step] = last_tool_result seen at
/// that model call, i.e. the result of tool step `step - 1`.
struct MockLog {
    lines: Vec<Value>,
}
impl MockLog {
    fn load(p: &Path) -> Self {
        let t = std::fs::read_to_string(p).unwrap_or_default();
        let lines = t
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Self { lines }
    }
    /// Result of tool step `step` (0-based) in `script`.
    fn r(&self, script: &str, step: u64) -> String {
        self.lines
            .iter()
            .find(|v| v["script"] == script && v["step"].as_u64() == Some(step + 1))
            .and_then(|v| v["last_tool_result"].as_str())
            .unwrap_or("")
            .to_string()
    }
}

/// One pass: fresh folder, mock and gateway; returns (passed, failed names).
fn pass(mock: &Path, script_mode: bool) -> (usize, Vec<String>) {
    let label = if script_mode { "script-mode" } else { "strict" };
    let run = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("agent-behaviour-{label}"));
    if run.exists() {
        // previous run's evidence: rename, never delete
        let old = run.with_extension(format!(
            "old-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ));
        std::fs::rename(&run, &old).expect("rename old run dir");
    }
    for d in ["home", "canary", "galion-files"] {
        std::fs::create_dir_all(run.join(d)).expect("mkdir");
    }
    std::fs::write(run.join("canary/keep.txt"), "keep\n").expect("canary");
    let files = std::fs::canonicalize(run.join("galion-files")).expect("canon");
    let files_s = files.to_string_lossy().to_string();
    let canary = run.join("canary");
    let key = test_key();

    println!("[{}] pass {label}: run dir {}", zagreb_now(), run.display());
    let out = |n: &str| Stdio::from(std::fs::File::create(run.join(n)).expect("log file"));
    let _mock = Proc(
        Command::new(mock)
            .args(["--port", &MOCK_PORT.to_string()])
            .arg("--scripts")
            .arg(fixtures())
            .arg("--log")
            .arg(run.join("mock.jsonl"))
            .arg("--var")
            .arg(format!("CANARY={}", canary.display()))
            .stdout(out("mock.out"))
            .stderr(out("mock.err"))
            .spawn()
            .expect("start mock"),
    );
    let mut gw = Command::new(HERMES);
    gw.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TZ", "Europe/Zagreb")
        .env("HOME", run.join("home"))
        .env("HERMES_HOME", run.join("home"))
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", GW_PORT.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_CRON", "0")
        .env("API_SERVER_KEY", &key)
        .env(
            "OPENROUTER_BASE_URL",
            format!("http://127.0.0.1:{MOCK_PORT}/v1"),
        )
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env("HERMES_RS_SESSION_USD_CAP", "1.00")
        .env("HERMES_RS_DAILY_USD_CAP", "2.00")
        .env("HERMES_RS_GUARD_WORKSPACE", &files)
        .env("HERMES_RS_FILES_ROOT", &files)
        .env("HERMES_RS_FILES_WRITE", "1")
        .env("HERMES_RS_TOOLS_WORKSPACE", &files)
        .env("HERMES_RS_TOOLS_WORKSPACE_MB", "4096")
        .env("HERMES_RS_TOOLS_BROWSER", "0")
        .env("HERMES_RS_TOOLS_VISION", "0")
        .stdout(out("gateway.out"))
        .stderr(out("gateway.err"));
    if script_mode {
        gw.env("HERMES_RS_GUARD_SCRIPT_MODE", "1");
    }
    let gw = Proc(gw.spawn().expect("start gateway"));
    assert!(wait_health(MOCK_PORT), "mock model did not come up");
    assert!(
        wait_health(GW_PORT),
        "gateway did not come up (see gateway.err)"
    );

    let auth = format!("Bearer {key}");
    let chat = |sid: &str, script: &str| {
        let body = serde_json::json!({"model": "default", "messages": [{"role": "user", "content": format!("Run SCRIPT:{script}")}]})
            .to_string();
        http(
            GW_PORT,
            "POST",
            "/v1/chat/completions",
            &[("Authorization", &auth), ("X-Hermes-Session-Id", sid)],
            &body,
        )
    };
    let (c1, tools_resp) = chat(&format!("behave-tools-{label}"), "tools");
    let (c2, script_resp) = chat(&format!("behave-script-{label}"), "script");
    drop(gw);
    drop(_mock);

    let log = MockLog::load(&run.join("mock.jsonl"));
    let mut c = Checks {
        pass: 0,
        fail: vec![],
    };
    let offered: Vec<String> = log
        .lines
        .first()
        .and_then(|v| v["tools_offered"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let mut sorted = offered.clone();
    sorted.sort();
    println!("tools offered: {}", sorted.join(","));
    for t in [
        "terminal",
        "read_file",
        "write_file",
        "edit_file",
        "list_files",
        "search_files",
        "files_list",
        "files_read",
    ] {
        c.check(
            &format!("tool {t} offered"),
            offered.iter().any(|o| o == t),
            "",
        );
    }
    let t = |s: u64| log.r("tools", s);
    c.check(
        "terminal runs in the files root",
        t(0).contains("hello-from-terminal") && t(0).contains(&files_s),
        &t(0),
    );
    c.check(
        "write_file",
        t(1).contains("\"bytes\"") && !refused(&t(1)),
        &t(1),
    );
    c.check("read_file", t(2).contains("alpha beta"), &t(2));
    c.check(
        "edit_file",
        std::fs::read_to_string(files.join("notes/a.txt")).unwrap_or_default() == "gamma beta\n",
        &t(3),
    );
    c.check("list_files", t(4).contains("a.txt"), &t(4));
    c.check("search_files", t(5).contains("a.txt"), &t(5));
    c.check("files_list (galion-files)", t(6).contains("a.txt"), &t(6));
    c.check(
        "files_read (galion-files)",
        t(7).contains("gamma beta"),
        &t(7),
    );
    c.check("rm -rf / refused", refused(&t(8)), &t(8));
    c.check("curl refused", refused(&t(9)), &t(9));
    c.check(
        "/etc/shadow refused",
        refused(&t(10)) || t(10).to_lowercase().contains("denied"),
        &t(10),
    );
    c.check(
        "../ write refused",
        t(11).to_lowercase().contains("error") || refused(&t(11)),
        &t(11),
    );
    c.check(
        "no file escaped the root",
        !files.parent().unwrap().join("escape.txt").exists(),
        "",
    );
    c.check(
        "timeout_s honoured",
        t(12).replace(' ', "").contains("\"timed_out\":true"),
        &t(12),
    );
    c.check(
        "outside the workspace is hidden or refused",
        !t(13).contains("keep.txt"),
        &t(13),
    );
    c.check(
        "tools session answered",
        c1 == 200 && tools_resp.contains("Tools script finished"),
        &tools_resp,
    );
    let good = log.r("script", 1);
    let bad = log.r("script", 3);
    if script_mode {
        c.check(
            "script mode: good script runs",
            good.contains("wrote 3 lines"),
            &good,
        );
        c.check(
            "script mode: output file written",
            files.join("count-3.txt").exists(),
            &good,
        );
    } else {
        c.check(
            "strict mode: script refused",
            refused(&good) && !good.contains("wrote 3 lines"),
            &good,
        );
    }
    c.check("script with curl refused", refused(&bad), &bad);
    c.check(
        "script session answered",
        c2 == 200 && script_resp.contains("Script script finished"),
        &script_resp,
    );
    c.check(
        "no key ever sent to the mock",
        log.lines
            .iter()
            .all(|v| v["authorization_header_sent"] != Value::Bool(true)),
        "",
    );
    c.check(
        "canary intact",
        std::fs::read_to_string(canary.join("keep.txt")).unwrap_or_default() == "keep\n",
        "",
    );
    c.check(
        "listeners gone after teardown",
        port_free(GW_PORT) && port_free(MOCK_PORT),
        "",
    );
    println!(
        "[{}] pass {label}: {}/{} PASS",
        zagreb_now(),
        c.pass,
        c.pass + c.fail.len()
    );
    (c.pass, c.fail)
}

#[test]
fn agent_behaviour_live_layout_mock_model() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1 (starts hermes-gateway + mock model on 19170/19171)");
        return;
    }
    for p in [GW_PORT, MOCK_PORT] {
        assert!(port_free(p), "port {p} is busy");
    }
    let mock = mock_bin();
    let only = std::env::var("HAB_ONLY").unwrap_or_default();
    let mut failed = vec![];
    for sm in [false, true] {
        let name = if sm { "script" } else { "strict" };
        if !only.is_empty() && only != name {
            continue;
        }
        let (_, f) = pass(&mock, sm);
        failed.extend(f.into_iter().map(|n| format!("{name}: {n}")));
    }
    assert!(failed.is_empty(), "failed checks: {failed:?}");
}
