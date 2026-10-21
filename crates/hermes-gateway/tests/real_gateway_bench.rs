// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! GALION_REAL=1 real-life loopback test for `HERMES_RS_BENCH=1` (wave 4,
//! slot 128; ports 19980-19989 only). Needs the `bench` feature:
//!
//! `GALION_REAL=1 cargo test --release -p hermes-gateway --features bench --test real_gateway_bench -- --nocapture`
//!
//! Starts the real `hermes-gateway` release binary on 127.0.0.1 with a temp
//! `HERMES_HOME` whose only model provider is a loopback mock LLM
//! (hermes-bench `MockLlm`, OpenAI wire, no key, zero spend; the env is
//! cleared so no real provider key can reach the child), then:
//!
//! 1. bench ON (in-process backends): the model is offered 36 bench tools +
//!    clock + delegate, never `ws_kill_reset`; a chat turn that calls
//!    vending/banking/Wall Street tools through `POST /v1/chat/completions`
//!    gets real backend answers; `ws_kill_reset` and a made-up
//!    `bank_transfer` are unknown tools; `payment_draft` stays a draft.
//! 2. bench OFF (switch unset): the same binary offers only clock + delegate.
//! 3. bench ON with `HERMES_RS_BENCH_BANK_URL` pointing at a loopback
//!    bench-banking mock run by this test: the gateway's calls show up in
//!    that mock's own counters (the HTTP backend path).
//! 4. fail closed: a non-loopback bind, a non-loopback backend URL and an
//!    unknown set each make the gateway exit 2 without listening.
//!
//! Every child is killed on drop and the ports are checked free at the end.

use hermes_bench::mockllm::MockLlm;
use hermes_bench::planner::{Brain, PlanIo, Script};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hermes-gateway");
const GW_ON: u16 = 19980;
const LLM: u16 = 19981;
const GW_OFF: u16 = 19982;
const BANK: u16 = 19983;
const GW_FAIL: u16 = 19984;
const GW_HTTP: u16 = 19985;
/// Test-only gateway key (not a secret; loopback child only).
const KEY: &str = "slot128-loopback-test-key-0000000000";
const SCRIPT_TAG: &str = "BENCH-SCRIPT:";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn ts() -> String {
    hermes_rs::time::zagreb_rfc3339(hermes_rs::time::now_secs())
}

fn own(port: u16) {
    assert!((19980..=19989).contains(&port), "slot 128 owns 19980-19989");
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

/// The mock model: a user prompt `BENCH-SCRIPT:[[tool, args], ...]` makes it
/// call each tool in turn through the gateway's agent loop, then answer with
/// the JSON list of `{tool, error, value}` it observed.
fn brain() -> Brain {
    Brain::new(Box::new(|user: &str| -> Script {
        let steps: Vec<(String, Value)> = user
            .strip_prefix(SCRIPT_TAG)
            .and_then(|s| serde_json::from_str::<Vec<(String, Value)>>(s).ok())
            .unwrap_or_default();
        Box::new(move |io: &mut PlanIo| {
            let mut seen = Vec::new();
            for (tool, args) in steps {
                let r = io.call(&tool, args);
                seen.push(json!({"tool": tool, "error": r.is_error(), "value": r.value}));
            }
            Value::Array(seen).to_string()
        })
    }))
}

struct Gw {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        println!("{} stopped hermes-gateway 127.0.0.1:{}", ts(), self.port);
    }
}

fn home(name: &str) -> PathBuf {
    let h = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("slot128-{name}"));
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(
        h.join("config.yaml"),
        format!(
            "model:\n  provider: mockllm\n  default: hermes-bench/mock-llm\n  base_url: http://127.0.0.1:{LLM}/v1\n"
        ),
    )
    .unwrap();
    h
}

fn command(name: &str, host: &str, port: u16, extra: &[(&str, &str)]) -> (Command, PathBuf) {
    own(port);
    let h = home(name);
    let log = h.join("gateway.log");
    let mut c = Command::new(BIN);
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HERMES_HOME", &h)
        .env("HERMES_RS_HOST", host)
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_MCP", "off")
        .env("API_SERVER_KEY", KEY)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap());
    for (k, v) in extra {
        c.env(k, v);
    }
    (c, log)
}

fn start(name: &str, port: u16, extra: &[(&str, &str)]) -> Gw {
    assert!(port_free(port), "port {port} busy before start");
    let (mut c, log) = command(name, "127.0.0.1", port, extra);
    let mut child = c.spawn().expect("spawn hermes-gateway");
    let t0 = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Ok(Some(st)) = child.try_wait() {
            panic!(
                "gateway exited {st}: {}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "port {port} never opened"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    println!(
        "{} started hermes-gateway pid {} on 127.0.0.1:{port} ({name})",
        ts(),
        child.id()
    );
    Gw { child, port, log }
}

/// Starts a gateway that must refuse: exit 2, never listening.
fn refuses(name: &str, host: &str, extra: &[(&str, &str)]) -> String {
    assert!(port_free(GW_FAIL));
    let (mut c, log) = command(name, host, GW_FAIL, extra);
    let mut child = c.spawn().expect("spawn hermes-gateway");
    let t0 = Instant::now();
    let st = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        if t0.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("{name}: gateway did not refuse");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(st.code(), Some(2), "{name}: {text}");
    assert!(!text.contains("listening"), "{name}: {text}");
    println!("{} {name}: exit 2, {}", ts(), text.trim());
    text
}

fn http(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    let b = body.map(Value::to_string).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )
    .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    (
        status,
        serde_json::from_str(body).unwrap_or(Value::String(body.into())),
    )
}

/// One chat turn; returns the mock model's observation list.
fn turn(port: u16, steps: &Value) -> Vec<Value> {
    let prompt = format!("{SCRIPT_TAG}{steps}");
    let t0 = Instant::now();
    let (st, v) = http(
        port,
        "POST",
        "/v1/chat/completions",
        Some(&json!({"model": "hermes-agent", "messages": [{"role": "user", "content": prompt}]})),
    );
    assert_eq!(st, 200, "{v}");
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    let obs: Vec<Value> =
        serde_json::from_str(content).unwrap_or_else(|_| panic!("content: {content}"));
    println!(
        "{} POST /v1/chat/completions :{port} -> 200 in {} ms, {} tool calls",
        ts(),
        t0.elapsed().as_millis(),
        obs.len()
    );
    obs
}

fn find<'a>(obs: &'a [Value], tool: &str) -> &'a Value {
    obs.iter()
        .find(|o| o["tool"] == tool)
        .unwrap_or_else(|| panic!("no {tool}"))
}

fn short(v: &Value) -> String {
    v.to_string().chars().take(160).collect()
}

fn offered(llm: &MockLlm) -> Vec<String> {
    let mut n = llm.seen().tool_names;
    n.sort();
    n
}

fn log_has(log: &Path, needle: &str) -> String {
    let t = std::fs::read_to_string(log).unwrap_or_default();
    assert!(t.contains(needle), "log lacks {needle:?}: {t}");
    t.lines()
        .find(|l| l.contains(needle))
        .unwrap_or("")
        .to_string()
}

#[test]
fn real_gateway_bench_loopback() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    for p in [GW_ON, LLM, GW_OFF, BANK, GW_FAIL, GW_HTTP] {
        own(p);
        assert!(port_free(p), "port {p} busy");
    }
    println!("{} binary {BIN}", ts());

    // 1. bench ON, in-process backends
    {
        let llm = MockLlm::start(LLM, brain()).unwrap();
        let gw = start("on", GW_ON, &[("HERMES_RS_BENCH", "1")]);
        println!("{}", log_has(&gw.log, "bench on: 36 tools"));
        let (st, h) = http(GW_ON, "GET", "/health", None);
        assert_eq!(st, 200, "{h}");
        let obs = turn(
            GW_ON,
            &json!([
                ["vending_status", {}],
                ["bank_balance", {}],
                ["payment_draft", {"from_account": "acc-eur-main", "payee_id": "cp-002", "amount": "650.00", "currency": "EUR", "reference": "slot128 rent"}],
                ["ws_account", {}],
                ["ws_universe", {}],
                ["ws_kill_reset", {"operator": "model"}],
                ["bank_transfer", {"amount": "1.00"}],
                ["clock", {}]
            ]),
        );
        for o in &obs {
            println!(
                "  {} error={} {}",
                o["tool"].as_str().unwrap_or(""),
                o["error"],
                short(&o["value"])
            );
        }
        let vs = find(&obs, "vending_status");
        assert_eq!(vs["error"], false, "{vs}");
        assert!(vs["value"]["day"].is_number(), "{vs}");
        assert_eq!(find(&obs, "bank_balance")["error"], false);
        assert_eq!(find(&obs, "ws_account")["error"], false);
        assert_eq!(find(&obs, "ws_universe")["error"], false);
        let pd = find(&obs, "payment_draft");
        assert_eq!(pd["error"], false, "{pd}");
        let pds = pd.to_string();
        assert!(pds.contains("awaiting_owner"), "{pd}");
        assert!(!pds.contains("\"submitted\":true"), "{pd}");
        let kr = find(&obs, "ws_kill_reset");
        assert!(
            kr.to_string().to_ascii_lowercase().contains("unknown"),
            "{kr}"
        );
        let bt = find(&obs, "bank_transfer");
        assert!(
            bt.to_string().to_ascii_lowercase().contains("unknown"),
            "{bt}"
        );
        let names = offered(&llm);
        println!("{} offered to the model: {} tools", ts(), names.len());
        assert_eq!(names.len(), 36 + 2, "{names:?}");
        assert!(names.iter().any(|n| n == "clock"));
        assert!(names
            .iter()
            .any(|n| n == "delegate_task" || n.starts_with("delegate")));
        assert!(!names.iter().any(|n| n == "ws_kill_reset"));
        assert!(names
            .iter()
            .all(|n| !hermes_bench::adapters::is_money_move_name(n)));
        let s = llm.seen();
        assert_eq!(s.with_auth, 0, "no key may reach the mock model");
        println!(
            "{} mock LLM: {} requests, {} with auth header, models {:?}",
            ts(),
            s.requests,
            s.with_auth,
            s.models
        );
    }

    // 2. bench OFF: same binary, switch unset
    {
        let llm = MockLlm::start(LLM, brain()).unwrap();
        let gw = start("off", GW_OFF, &[]);
        let obs = turn(GW_OFF, &json!([["vending_status", {}]]));
        assert!(find(&obs, "vending_status")
            .to_string()
            .to_ascii_lowercase()
            .contains("unknown"));
        let names = offered(&llm);
        println!("{} bench off: offered {names:?}", ts());
        assert_eq!(names.len(), 2, "{names:?}");
        let t = std::fs::read_to_string(&gw.log).unwrap_or_default();
        assert!(!t.contains("bench on"), "{t}");
    }

    // 3. bench ON, banking over loopback HTTP to a mock this test runs
    {
        let mock = bench_banking::mock::Mock::new();
        mock.spawn(TcpListener::bind(("127.0.0.1", BANK)).unwrap());
        let before = mock.counters().total;
        let _llm = MockLlm::start(LLM, brain()).unwrap();
        let bank = format!("http://127.0.0.1:{BANK}");
        let gw = start(
            "http-bank",
            GW_HTTP,
            &[
                ("HERMES_RS_BENCH", "1"),
                ("HERMES_RS_BENCH_SETS", "banking"),
                ("HERMES_RS_BENCH_BANK_URL", &bank),
            ],
        );
        println!("{}", log_has(&gw.log, "bench on: 12 tools"));
        let obs = turn(GW_HTTP, &json!([["bank_balance", {}], ["payee_list", {}]]));
        assert_eq!(find(&obs, "bank_balance")["error"], false);
        let after = mock.counters().total;
        println!(
            "{} bank mock 127.0.0.1:{BANK} counters.total {before} -> {after}",
            ts()
        );
        assert!(after > before, "gateway never reached the HTTP bank mock");
        // the bank listener thread lives until the test process exits; the
        // free-port check below skips BANK for that reason
    }

    // 4. fail closed
    let a = refuses("public-bind", "127.0.0.2", &[("HERMES_RS_BENCH", "1")]);
    assert!(a.contains("not loopback"));
    let b = refuses(
        "remote-backend",
        "127.0.0.1",
        &[
            ("HERMES_RS_BENCH", "1"),
            ("HERMES_RS_BENCH_WS_URL", "http://10.0.0.5:8080"),
        ],
    );
    assert!(b.contains("HERMES_RS_BENCH_WS_URL"));
    let c = refuses(
        "unknown-set",
        "127.0.0.1",
        &[
            ("HERMES_RS_BENCH", "1"),
            ("HERMES_RS_BENCH_SETS", "revolut"),
        ],
    );
    assert!(c.contains("unknown set"));

    for p in [GW_ON, LLM, GW_OFF, GW_FAIL, GW_HTTP] {
        assert!(port_free(p), "port {p} still busy after teardown");
    }
    println!("{} all slot-128 gateway ports free again", ts());
}
