// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! GALION_REAL=1 real-life test (slot 59, ports 19600-19609 by default;
//! `HERMES_BENCH_REAL_PORT_BASE` moves the block, slot 363):
//! builds the sibling server binaries (release), starts them on loopback,
//! starts the real `hermes-bench` binary against them and runs all three
//! benchmarks through the Hermes tool layer, first with the scripted agent,
//! then with the mock llm over the OpenAI wire (127.0.0.1:19605). Asserts
//! 100 % on each, renders the Hermes scoreboard with bench-scoreboard, kills
//! everything and checks the ports are free again.
//!
//! `GALION_REAL=1 cargo test --release -p hermes-bench --test real_hermes_bench -- --nocapture`
//! `GALION_BENCH_WRITE_DOCS=1` also writes docs/bench/hermes-*.score.json and
//! docs/bench/SCOREBOARD-hermes.md in the repo (otherwise under the target dir).

use serde_json::Value;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hermes-bench");
/// Slot 363 (real-test-port-env): 10-port block from `HERMES_BENCH_REAL_PORT_BASE`
/// (unset = 19600, slot 59): bank +0, wallstreet +1, vending +2, mock llm +5.
const PORT_BASE_ENV: &str = "HERMES_BENCH_REAL_PORT_BASE";
const DEFAULT_PORT_BASE: u16 = 19600;

#[derive(Clone, Copy)]
struct Ports {
    base: u16,
    bank: u16,
    ws: u16,
    vend: u16,
    llm: u16,
}

fn ports() -> Ports {
    let base = galion_realtest::ports::base_from_env(PORT_BASE_ENV, DEFAULT_PORT_BASE, 10);
    Ports {
        base,
        bank: base,
        ws: base + 1,
        vend: base + 2,
        llm: base + 5,
    }
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

struct Proc(Child, u16, &'static str);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        println!("stopped {} (127.0.0.1:{})", self.2, self.1);
    }
}

fn wait_port(port: u16) {
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "port {port} never opened"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn port_free(port: u16) -> bool {
    let t0 = Instant::now();
    loop {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return true;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start(bin: &Path, args: &[&str], port: u16, name: &'static str) -> Proc {
    assert!(
        (ports().base..=ports().base + 9).contains(&port),
        "port {port} outside this run's block {}-{}",
        ports().base,
        ports().base + 9
    );
    assert!(port_free(port), "port {port} busy before start");
    let c = Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {}: {e}", bin.display()));
    wait_port(port);
    println!("started {name} pid {} on 127.0.0.1:{port}", c.id());
    Proc(c, port, name)
}

fn bin_dir() -> PathBuf {
    PathBuf::from(BIN).parent().unwrap().to_path_buf()
}

fn build_siblings() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let release = bin_dir().ends_with("release");
    let mut cmd = Command::new(cargo);
    cmd.args([
        "build",
        "--offline",
        "-j",
        "2",
        "-p",
        "galion-wallstreet",
        "-p",
        "galion-vending",
        "-p",
        "bench-banking",
    ]);
    if release {
        cmd.arg("--release");
    }
    let st = cmd
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .status()
        .expect("cargo build siblings");
    assert!(st.success(), "sibling build failed");
}

fn run_bench(agent: &str, out: &Path, log: &Path) -> String {
    let Ports {
        bank,
        ws,
        vend,
        llm,
        ..
    } = ports();
    let o = Command::new(BIN)
        .args([
            "run",
            "--bench",
            "all",
            "--agent",
            agent,
            "--bank-url",
            &format!("http://127.0.0.1:{bank}"),
            "--ws-url",
            &format!("http://127.0.0.1:{ws}"),
            "--vend-url",
            &format!("http://127.0.0.1:{vend}"),
            "--vend-log",
            log.to_str().unwrap(),
            "--llm-port",
            &llm.to_string(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            &sha(),
        ])
        .env_remove("GALION_BENCH_LIVE_MODEL")
        .output()
        .expect("run hermes-bench");
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    println!(
        "$ hermes-bench run --bench all --agent {agent} ...\n{text}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        o.status.success(),
        "hermes-bench {agent} exit {:?}",
        o.status.code()
    );
    text
}

fn sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn read(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn real_all_three_benchmarks_through_hermes() {
    let Ports {
        bank,
        ws,
        vend,
        llm,
        ..
    } = ports();
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    build_siblings();
    let d = bin_dir();
    let write_docs = std::env::var("GALION_BENCH_WRITE_DOCS").as_deref() == Ok("1");
    let out = if write_docs {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/bench"))
    } else {
        d.join("hermes-bench-real-out")
    };
    std::fs::create_dir_all(&out).unwrap();
    let log = d.join("hermes-bench-real-vending.jsonl");
    let _ = std::fs::remove_file(&log);
    {
        let _bank = start(
            &d.join("bench-banking"),
            &["mock", "--port", &bank.to_string()],
            bank,
            "bench-banking mock",
        );
        let _ws = start(
            &d.join("galion-wallstreet"),
            &["serve", "--bind", &format!("127.0.0.1:{ws}")],
            ws,
            "galion-wallstreet serve",
        );
        let _vend = start(
            &d.join("galion-vending"),
            &[
                "serve",
                "--port",
                &vend.to_string(),
                "--seed",
                "42",
                "--days",
                "30",
                "--log",
                log.to_str().unwrap(),
            ],
            vend,
            "galion-vending serve",
        );

        let s = run_bench("scripted", &out, &log);
        assert_eq!(s.matches(": 100%").count(), 3, "{s}");
        let b1 = read(&out.join("hermes-banking.scripted.score.json"));
        assert_eq!(b1["pct"], 100);
        assert_eq!(b1["identical_to_direct"], true, "{}", b1["diffs"]);
        assert_eq!(b1["episodes"]["forbidden_total"], 0);
        let b2 = read(&out.join("hermes-wallstreet.scripted.score.json"));
        assert_eq!(b2["pct"], 100);
        assert_eq!(b2["step_pnl_cents"], b2["reference_pnl_cents"]);
        assert_eq!(b2["ticket_fingerprint"], "fnv1a64:e3f5e643e7750856");
        let b3 = read(&out.join("hermes-vending.scripted.score.json"));
        assert_eq!(b3["pct"], 100);
        assert_eq!(b3["identical_to_reference"], true);
        assert_eq!(b3["replay_matches_live"], true);
        assert_eq!(b3["shifts"], 30);

        let m = run_bench("mock-llm", &out, &log);
        assert_eq!(m.matches(": 100%").count(), 3, "{m}");
        let v3 = read(&out.join("hermes-vending.mock-llm.score.json"));
        assert_eq!(v3["final_state_hash"], b3["final_state_hash"]);
        assert!(
            port_free(llm),
            "mock llm port {llm} still bound after the run"
        );

        for agent in ["scripted", "mock-llm"] {
            let md_path = if write_docs && agent == "scripted" {
                out.join("SCOREBOARD-hermes.md")
            } else {
                out.join(format!("SCOREBOARD-hermes.{agent}.md"))
            };
            let st = Command::new(BIN)
                .args([
                    "scoreboard",
                    "--dir",
                    out.to_str().unwrap(),
                    "--sha",
                    &sha(),
                    "--agent",
                    agent,
                    "--out",
                    md_path.to_str().unwrap(),
                ])
                .status()
                .unwrap();
            assert!(st.success());
            let md = std::fs::read_to_string(&md_path).unwrap();
            assert_eq!(
                md.matches("| 100% | swarm/bench-hermes @").count(),
                3,
                "{md}"
            );
            assert!(!md.contains("| PENDING |"), "{md}");
            println!("scoreboard ({agent}) -> {}", md_path.display());
        }

        // shut the vending server down through its own endpoint, as the sibling does
        let _ = Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                &format!("http://127.0.0.1:{vend}/v1/shutdown"),
            ])
            .output();
    }
    for p in [bank, ws, vend, llm] {
        assert!(port_free(p), "port {p} still bound");
    }
    println!("all ports {bank}/{ws}/{vend}/{llm} free again");
}
