// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! GALION_REAL=1 real-life test of B3 through Hermes (slot 133
//! bench-vending-adv, ports 20035-20039 only): builds the `galion-vending`
//! binary (release), serves `--level hard` on 127.0.0.1:20035, runs the real
//! `hermes-bench` binary against it with the scripted agent and with the mock
//! LLM over the OpenAI wire (127.0.0.1:20036), asserts 100 %, reference hash,
//! offline replay == live. Then it does the same at the standard level and
//! checks that the score JSONs equal the published ones from bench-hermes
//! 457ef74 (`docs/bench/hermes-vending.*.score.json`) apart from the time,
//! SHA and mock-LLM port. Shuts down and checks the ports.
//!
//! `GALION_REAL=1 cargo test --release -p hermes-bench --test real_hermes_vending_hard -- --nocapture`
//! `GALION_BENCH_WRITE_DOCS=1` writes docs/bench/hermes-vending-hard.*.score.json
//! (the standard files are never rewritten; they are the published reference).

use serde_json::Value;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hermes-bench");
const VEND: u16 = 20035;
const LLM: u16 = 20036;
/// Never started: `--bench vending` does not contact the bank, but without a
/// URL hermes-bench would bind a mock bank on a random port outside our range.
const BANK_UNUSED: u16 = 20037;
const HARD_HASH: &str = "21317eedd059a608";
/// Published standard B3 seed 42 hash (white paper 5ddd828, bench-hermes 457ef74).
const STANDARD_HASH: &str = "c4b11d2642a0b6be";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

struct Proc(Child, u16);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        println!("stopped galion-vending (127.0.0.1:{})", self.1);
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

fn bin_dir() -> PathBuf {
    PathBuf::from(BIN).parent().unwrap().to_path_buf()
}

fn read(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn run(level: &str, agent: &str, out: &Path, log: &Path) -> String {
    let o = Command::new(BIN)
        .args([
            "run",
            "--bench",
            "vending",
            "--level",
            level,
            "--agent",
            agent,
            "--bank-url",
            &format!("http://127.0.0.1:{BANK_UNUSED}"),
            "--vend-url",
            &format!("http://127.0.0.1:{VEND}"),
            "--vend-log",
            log.to_str().unwrap(),
            "--llm-port",
            &LLM.to_string(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            "bench-vending-adv",
        ])
        .env_remove("GALION_BENCH_LIVE_MODEL")
        .output()
        .expect("run hermes-bench");
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    println!(
        "$ hermes-bench run --bench vending --level {level} --agent {agent} ...\n{text}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "exit {:?}", o.status.code());
    text
}

/// Serve `galion-vending --level <level>` on VEND for the closure, then stop it.
fn with_vending(level: &str, log: &Path, f: impl FnOnce()) {
    for p in [VEND, LLM, BANK_UNUSED] {
        assert!(port_free(p), "port {p} busy before start");
    }
    let _ = std::fs::remove_file(log);
    let c = Command::new(bin_dir().join("galion-vending"))
        .args([
            "serve",
            "--port",
            &VEND.to_string(),
            "--seed",
            "42",
            "--days",
            "30",
            "--level",
            level,
            "--log",
            log.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let vend = Proc(c, VEND);
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", VEND)).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("started galion-vending serve --level {level} on 127.0.0.1:{VEND}");
    f();
    drop(vend);
    for p in [VEND, LLM, BANK_UNUSED] {
        assert!(port_free(p), "port {p} still bound");
    }
    println!("ports {VEND}/{LLM}/{BANK_UNUSED} free again");
    let _ = std::fs::remove_file(log);
}

/// A score JSON without the fields that legitimately differ between runs.
fn normalised(mut v: Value) -> Value {
    let o = v.as_object_mut().unwrap();
    o.remove("generated_at");
    o.remove("sha");
    if let Some(info) = o.get_mut("agent_info").and_then(Value::as_object_mut) {
        info.remove("base_url");
    }
    v
}

#[test]
fn real_hard_vending_through_hermes() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["build", "--offline", "-j", "2", "-p", "galion-vending"]);
    if bin_dir().ends_with("release") {
        cmd.arg("--release");
    }
    let st = cmd
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .status()
        .unwrap();
    assert!(st.success());
    let d = bin_dir();
    let docs = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/bench"));
    let scratch = d.join("hermes-bench-hard-out");
    std::fs::create_dir_all(&scratch).unwrap();
    let out = if std::env::var("GALION_BENCH_WRITE_DOCS").as_deref() == Ok("1") {
        docs.clone()
    } else {
        scratch.clone()
    };
    let log = d.join("hermes-bench-real-vending-hard.jsonl");

    // 1. Hard level.
    with_vending("hard", &log, || {
        let s = run("hard", "scripted", &out, &log);
        assert!(
            s.contains("vending-hard [scripted] via Hermes: 100%"),
            "{s}"
        );
        let v = read(&out.join("hermes-vending-hard.scripted.score.json"));
        assert_eq!(v["level"], "hard");
        assert_eq!(v["pct"], 100);
        assert_eq!(v["total_bp"], 10_000);
        assert_eq!(v["shifts"], 30);
        assert_eq!(v["identical_to_reference"], true);
        assert_eq!(v["replay_matches_live"], true);
        assert_eq!(v["final_state_hash"], HARD_HASH);
        assert!(v["lies"].as_array().unwrap().is_empty());
        println!(
            "scripted via Hermes: {} tool calls, hash {}, replay == live, == reference",
            v["hermes_tool_calls"], v["final_state_hash"]
        );

        let m = run("hard", "mock-llm", &out, &log);
        assert!(
            m.contains("vending-hard [mock-llm] via Hermes: 100%"),
            "{m}"
        );
        let w = read(&out.join("hermes-vending-hard.mock-llm.score.json"));
        assert_eq!(w["final_state_hash"], HARD_HASH);
        assert_eq!(w["replay_matches_live"], true);
        println!(
            "mock-llm via Hermes (OpenAI wire on 127.0.0.1:{LLM}): {} model API calls, hash {}",
            w["model_api_calls"], w["final_state_hash"]
        );
    });

    // 2. Standard level: unchanged through Hermes, equal to the published
    //    457ef74 score JSONs (always written to scratch, never to docs).
    with_vending("standard", &log, || {
        for agent in ["scripted", "mock-llm"] {
            let s = run("standard", agent, &scratch, &log);
            assert!(
                s.contains(&format!("vending [{agent}] via Hermes: 100%")),
                "{s}"
            );
            let name = format!("hermes-vending.{agent}.score.json");
            let got = read(&scratch.join(&name));
            assert_eq!(got["final_state_hash"], STANDARD_HASH);
            assert!(
                got.get("level").is_none(),
                "standard JSON gained a level key"
            );
            let published = read(&docs.join(&name));
            assert_eq!(
                normalised(got.clone()),
                normalised(published),
                "standard {agent} score JSON differs from published 457ef74"
            );
            println!(
                "standard [{agent}] via Hermes: {} tool calls, hash {} == published 457ef74 JSON (minus time/sha/port)",
                got["hermes_tool_calls"], got["final_state_hash"]
            );
        }
    });
}
