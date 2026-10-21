// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! GALION_REAL=1 real-life tests: each starts the `bench-banking mock`
//! binary on its own port in slot 51's range (127.0.0.1:19510-19519),
//! runs one axis against it over real HTTP, asserts 100, kills the mock and
//! checks the port is free again. Run with
//! `GALION_REAL=1 cargo test --release -p bench-banking --test real_banking`.
//! `GALION_BENCH_WRITE_DOCS=1` makes `real_scoreboard` write
//! docs/bench/banking.md + banking.score.json in the repo (otherwise under
//! target/test-out). No live bank is ever contacted.

use bench_banking::axes;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_bench-banking");
static SERIAL: Mutex<()> = Mutex::new(());

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

struct MockProc {
    child: Child,
    port: u16,
}

impl MockProc {
    fn start(port: u16) -> MockProc {
        assert!(
            (19510..=19519).contains(&port),
            "slot 51 owns 19510-19519 only"
        );
        let mut child = Command::new(BIN)
            .args(["mock", "--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn bench-banking mock");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(
            line.contains(&format!("http://127.0.0.1:{port}")),
            "banner: {line}"
        );
        MockProc { child, port }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for MockProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let deadline = Instant::now() + Duration::from_secs(3);
        while TcpStream::connect(("127.0.0.1", self.port)).is_ok() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn run_axis(id: &str, port: u16) {
    if !real() {
        eprintln!("skip {id}: set GALION_REAL=1");
        return;
    }
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let m = MockProc::start(port);
    let a = axes::run(id, &m.url()).expect("axis id");
    let fails: Vec<String> = a
        .failing()
        .iter()
        .map(|c| format!("{} :: {}", c.name, c.detail))
        .collect();
    eprintln!(
        "{id}: {}% ({}/{}) on {}",
        a.score(),
        a.passed(),
        a.checks.len(),
        m.url()
    );
    assert!(fails.is_empty(), "{id} below 100: {fails:#?}");
    // Every axis also asserts this itself; repeat it from outside.
    let c = axes::Ctl::new(&m.url()).unwrap().counters();
    let probe_allowance = u64::from(id == "payment_draft"); // the axis' own live-counter probe
    assert_eq!(
        c["forbidden_total"].as_u64(),
        Some(probe_allowance),
        "{id}: {c}"
    );
    drop(m);
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} still open"
    );
}

#[test]
fn real_axis_auth_session() {
    run_axis("auth_session", 19510);
}
#[test]
fn real_axis_balance_statement() {
    run_axis("balance_statement", 19511);
}
#[test]
fn real_axis_payee_mgmt() {
    run_axis("payee_mgmt", 19512);
}
#[test]
fn real_axis_payment_draft() {
    run_axis("payment_draft", 19513);
}
#[test]
fn real_axis_fx_quote() {
    run_axis("fx_quote", 19514);
}
#[test]
fn real_axis_card_freeze_draft() {
    run_axis("card_freeze_draft", 19515);
}
#[test]
fn real_axis_dispute_draft() {
    run_axis("dispute_draft", 19516);
}
#[test]
fn real_axis_audit_trail() {
    run_axis("audit_trail", 19517);
}
#[test]
fn real_axis_refusal() {
    run_axis("refusal", 19518);
}
#[test]
fn real_axis_error_recovery() {
    run_axis("error_recovery", 19519);
}

fn git_sha() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let out = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let sha = out(&["rev-parse", "--short", "HEAD"]);
    let dirty = !out(&["status", "--porcelain", "--", "."]).is_empty();
    if dirty {
        format!("{sha}+uncommitted crates/bench-banking")
    } else {
        sha
    }
}

/// Mock binary + `bench-banking score` binary end to end; writes the
/// scoreboard files and asserts 100%.
#[test]
fn real_scoreboard() {
    if !real() {
        eprintln!("skip scoreboard: set GALION_REAL=1");
        return;
    }
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let m = MockProc::start(19510);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = if std::env::var("GALION_BENCH_WRITE_DOCS").as_deref() == Ok("1") {
        root.join("docs/bench")
    } else {
        root.join("target/test-out/bench-banking")
    };
    let o = Command::new(BIN)
        .args([
            "score",
            "--mock",
            &m.url(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            &git_sha(),
        ])
        .output()
        .expect("run score");
    let stdout = String::from_utf8_lossy(&o.stdout).to_string();
    eprintln!("{stdout}");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("banking.score.json")).unwrap())
            .unwrap();
    assert_eq!(j["pct"], serde_json::json!(100.0), "{stdout}");
    assert_eq!(j["axes"].as_array().map(Vec::len), Some(10));
    assert_eq!(j["harness_scorecard"]["pct"], 100);
    zagreb_time::require_rfc3339(j["at"].as_str().unwrap()).unwrap();
    let md = std::fs::read_to_string(out.join("banking.md")).unwrap();
    assert!(md.contains("| B1 banking (mock) | **100%** |"));
}
