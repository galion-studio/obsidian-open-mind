// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Gaia S6.3 real-life test (wave-4 slot 214, ports 20840-20849), Rust only,
//! gated with `GALION_REAL=1`. Box only: every server binds 127.0.0.1, no
//! real model runs, nothing paid is called, no real LAN device is used and
//! nothing runs on the VPS.
//!
//! Two loopback "providers" stand in for the real hardware:
//!
//! | port        | process |
//! |-------------|---------|
//! | 20840/udp   | mDNS port for the test announcer and `find` (not 5353) |
//! | 20841       | mock_openai `cpu` = the CPU fallback (`--cpu`) |
//! | 20842       | `openmind-discovery serve-route` (the router under test) |
//! | 20843       | mock_openai `gpu` = the fake GPU provider (registry row, kind gpu, 80 GB) |
//! | 20844       | mock_openai `lan-gpu` = the fake LAN provider (announced, 24 GB) |
//! | 20845       | mock_openai `lan-other` (announced, never approved: must get 0 requests) |
//! | 20846       | mock_openai `lan-gpu` again after it moved |
//! | 20847       | `openmind-queue serve` (POSEIDON) on the same registry |
//!
//! Flow: the GPU row is pending and the LAN devices are found pending ->
//! everything goes to the CPU; approve GPU -> GPU; approve + join LAN ->
//! chain GPU, LAN, CPU, still GPU first; POSEIDON pool = the two approved
//! rows only; GPU hangs (SIGSTOP) -> LAN on the same request after the
//! timeout; GPU dies -> LAN; join GPU down -> not tried; LAN dies -> CPU;
//! LAN comes back on a new port -> routed at the old endpoint until
//! re-approval, then join -> LAN at the new endpoint; GPU comes back ->
//! GPU first again; deny LAN -> never used; CPU dies too -> 503 with every
//! attempt listed. Every process is killed on drop.
//!
//! The mDNS packets go to 224.0.0.251:20840 (link-local multicast, port
//! outside 5353 so no real responder answers) on the box's own interface,
//! as in `real_lan.rs` and the slot-31 test.
//!
//! Knobs: `LAN_FALLBACK_BIN_DIR` = a directory holding release builds of
//! `openmind-discovery`, `openmind-queue` and `examples/mock_openai` (skips
//! the build); `REAL_LAN_FALLBACK_PORT_BASE` moves the 10-port block.
//! Otherwise the test runs `cargo build --release` into
//! `<target>/real-lan-fallback`. Run with `--nocapture` to see the report.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn port_base() -> u16 {
    std::env::var("REAL_LAN_FALLBACK_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20840)
}

const GPU: &str = "gpu-fake";
const LAN: &str = "llamacpp-lan-gpu";
const OTHER: &str = "llamacpp-lan-other";

/// Kills every child on drop, so a failed assert leaves nothing running.
struct Procs(Vec<(String, Child)>);

impl Procs {
    fn add(&mut self, name: &str, child: Child) -> u32 {
        let pid = child.id();
        self.0.push((name.to_string(), child));
        pid
    }
    fn kill(&mut self, name: &str) {
        for (n, c) in self.0.iter_mut() {
            if n == name {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        self.0.retain(|(n, _)| n != name);
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        for (_, c) in self.0.iter_mut() {
            // A stopped process must be continued to die cleanly.
            let _ = Command::new("kill")
                .args(["-CONT", &c.id().to_string()])
                .status();
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn step(msg: &str) {
    println!("\n== {}  {msg}", stamp());
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("LAN_FALLBACK_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-lan-fallback");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args(["-p", "openmind-discovery", "--bin", "openmind-discovery"])
        .args(["--example", "mock_openai"])
        .args(["-p", "openmind-queue", "--bin", "openmind-queue"])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build failed");
    td.join("release")
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Resp {
    fn header(&self, name: &str) -> String {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> Resp {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Resp {
        status,
        headers,
        body: body.to_string(),
    }
}

fn up(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn wait_up(port: u16) {
    let t = Instant::now();
    while !up(port) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "port {port} never came up"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

struct World {
    bin: PathBuf,
    work: PathBuf,
    base: u16,
}

impl World {
    fn p(&self, off: u16) -> u16 {
        self.base + off
    }
    fn reg(&self) -> PathBuf {
        self.work.join("providers.json")
    }
    fn log(&self) -> PathBuf {
        self.work.join("approvals.log")
    }
    /// One `openmind-discovery` subcommand on the test registry. Returns
    /// (exit ok, stdout JSON, stderr).
    fn d(&self, args: &[&str]) -> (bool, Value, String) {
        let out = Command::new(self.bin.join("openmind-discovery"))
            .args(args)
            .arg("--registry")
            .arg(self.reg())
            .arg("--log")
            .arg(self.log())
            .args(["--by", "real-s6.3"])
            .output()
            .expect("run openmind-discovery");
        let v = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (
            out.status.success(),
            v,
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    }
    fn mock(&self, procs: &mut Procs, name: &str, file: &str, port: u16) -> u32 {
        let err = std::fs::File::create(self.work.join(file)).unwrap();
        let child = Command::new(self.bin.join("examples/mock_openai"))
            .args([name, &port.to_string(), "600"])
            .stdout(Stdio::null())
            .stderr(err)
            .spawn()
            .expect("spawn mock_openai");
        let pid = procs.add(file, child);
        wait_up(port);
        pid
    }
    fn announce(&self, procs: &mut Procs, tag: &str, name: &str, port: u16, vram: u32, secs: u32) {
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join("announce.jsonl"))
            .unwrap();
        let child = Command::new(self.bin.join("openmind-discovery"))
            .args(["announce", "--mdns-port", &self.p(0).to_string()])
            .args(["--host", "lan-box", "--addr", "127.0.0.1"])
            .args(["--duration-s", &secs.to_string()])
            .arg("--service")
            .arg(format!("llamacpp:{name}:{port}:caps=llm;vram={vram}"))
            .stdout(out)
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn announcer");
        procs.add(tag, child);
    }
    fn find(&self) -> Value {
        let (ok, v, err) = self.d(&[
            "find",
            "--mdns-port",
            &self.p(0).to_string(),
            "--services",
            "llamacpp",
            "--timeout-ms",
            "1500",
        ]);
        assert!(ok, "find failed: {err}");
        v
    }
    fn hits(&self, file: &str) -> usize {
        std::fs::read_to_string(self.work.join(file))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("POST /v1/chat/completions"))
            .count()
    }
    /// Chain ids from the router's `/v1/route`.
    fn chain(&self) -> Vec<String> {
        let r = http(self.p(2), "GET", "/v1/route", None);
        assert_eq!(r.status, 200, "{}", r.body);
        r.json()["chain"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap_or("").to_string())
            .collect()
    }
    /// One chat completion: (provider, attempts, content, status, ms).
    fn chat(&self) -> (String, String, String, u16, u128) {
        let t = Instant::now();
        let r = http(
            self.p(2),
            "POST",
            "/v1/chat/completions",
            Some(r#"{"model":"auto","messages":[{"role":"user","content":"ping"}]}"#),
        );
        let ms = t.elapsed().as_millis();
        let v = r.json();
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .or_else(|| v["error"]["type"].as_str())
            .unwrap_or("")
            .to_string();
        let got = (
            r.header("X-Galion-Provider"),
            r.header("X-Galion-Attempts"),
            content,
            r.status,
            ms,
        );
        println!(
            "chat -> status {} provider {} attempts [{}] content {:?} in {} ms",
            got.3, got.0, got.1, got.2, got.4
        );
        got
    }
    fn expect_chat(&self, provider: &str, attempts: &str, content: &str) -> u128 {
        let (p, a, c, s, ms) = self.chat();
        assert_eq!(
            (p.as_str(), a.as_str(), c.as_str()),
            (provider, attempts, content),
            "unexpected route"
        );
        assert_eq!(s, 200);
        ms
    }
    fn pool(&self) -> Value {
        let r = http(self.p(7), "GET", "/vram", None);
        assert_eq!(r.status, 200, "{}", r.body);
        let v = r.json();
        let devs: Vec<Value> = v["devices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                json!({"id": d["device"]["id"], "kind": d["device"]["kind"],
                       "gb": d["device"]["vram_gb"], "state": d["device"]["state"],
                       "healthy": d["device"]["healthy"], "routable": d["routable"]})
            })
            .collect();
        let s = json!({"devices": devs, "approved_gb": v["summary"]["approved_gb"],
                       "gap_gb": v["summary"]["gap_gb"],
                       "routable_devices": v["summary"]["routable_devices"]});
        println!("POSEIDON pool: {s}");
        s
    }
    fn sync(&self) {
        let r = http(self.p(7), "POST", "/vram/sync-providers", Some("{}"));
        println!("POSEIDON sync-providers: {} {}", r.status, r.body);
        assert_eq!(r.status, 200);
    }
    fn job(&self, gb: u32, cpu_fallback: bool) -> Resp {
        let body = json!({"queue": "gpu", "agent": "real-s6.3", "kind": "echo",
                          "vram_gb": gb, "cpu_fallback": cpu_fallback, "payload": "hi"});
        let r = http(self.p(7), "POST", "/jobs", Some(&body.to_string()));
        println!(
            "POSEIDON job {gb} GB cpu_fallback={cpu_fallback}: {} {}",
            r.status, r.body
        );
        r
    }
    /// Lease one job and ack it, so the next lease starts from a clean pool.
    fn lease_ack(&self) -> Value {
        let r = http(
            self.p(7),
            "POST",
            "/lease",
            Some(r#"{"queue":"gpu","worker":"w1","wait_ms":300}"#),
        );
        let v = r.json();
        println!(
            "POSEIDON lease: id {} device {} miss {}",
            v["id"], v["device"], v["miss"]
        );
        if let Some(id) = v["id"].as_u64() {
            let token = v["token"].clone();
            let ack = http(
                self.p(7),
                "POST",
                &format!("/jobs/{id}/ack"),
                Some(&json!({"token": token}).to_string()),
            );
            assert!(ack.status < 300, "ack: {} {}", ack.status, ack.body);
        }
        v
    }
    fn row(&self, id: &str) -> Value {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(self.reg()).unwrap()).unwrap();
        v["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    }
}

fn signal(pid: u32, sig: &str) {
    let ok = Command::new("kill")
        .args([sig, &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "kill {sig} {pid}");
}

#[test]
fn two_loopback_providers_approve_route_die_fall_back_to_cpu_and_reapprove_moved() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let base = port_base();
    for off in 1..=7 {
        assert!(port_free(base + off), "port {} busy", base + off);
    }
    step("build (release) or reuse LAN_FALLBACK_BIN_DIR");
    let bin = binaries();
    for b in [
        "openmind-discovery",
        "openmind-queue",
        "examples/mock_openai",
    ] {
        let m = std::fs::metadata(bin.join(b)).expect("binary");
        println!("{b}: {} bytes", m.len());
    }
    let work = std::env::temp_dir().join(format!(
        "real-lan-fallback-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    std::fs::create_dir_all(&work).unwrap();
    let w = World { bin, work, base };
    let mut procs = Procs(Vec::new());
    let started = Instant::now();

    step(&format!(
        "seed the fake GPU as a PENDING registry row (kind gpu, 80 GB, http://127.0.0.1:{})",
        w.p(3)
    ));
    let seed = json!({"version": 1, "providers": [{
        "id": GPU, "kind": "gpu", "endpoint": format!("http://127.0.0.1:{}", w.p(3)),
        "capabilities": ["llm"], "approval": "pending", "health": "unknown", "vram_gb": 80}]});
    std::fs::write(w.reg(), serde_json::to_vec_pretty(&seed).unwrap()).unwrap();

    step("start mocks: cpu, gpu (fake GPU), lan-gpu (fake LAN), lan-other");
    w.mock(&mut procs, "cpu", "mock-cpu.err", w.p(1));
    let gpu_pid = w.mock(&mut procs, "gpu", "mock-gpu.err", w.p(3));
    w.mock(&mut procs, "lan-gpu", "mock-lan.err", w.p(4));
    w.mock(&mut procs, "lan-other", "mock-other.err", w.p(5));

    step("announce lan-gpu (24 GB) and lan-other (48 GB) over mDNS on the slot port, then find");
    w.announce(&mut procs, "ann-lan", "lan-gpu", w.p(4), 24, 60);
    w.announce(&mut procs, "ann-other", "lan-other", w.p(5), 48, 60);
    std::thread::sleep(Duration::from_millis(500));
    let found = w.find();
    let devs = found["devices"].as_array().unwrap();
    println!(
        "find: {}",
        Value::Array(
            devs.iter()
                .map(|d| json!({"id": d["provider_id"], "status": d["status"],
                                "approval": d["approval"], "endpoint": d["endpoint"]}))
                .collect()
        )
    );
    let new_pending = devs
        .iter()
        .filter(|d| d["status"] == "new" && d["approval"] == "pending")
        .count();
    assert_eq!(new_pending, 2, "want 2 new pending LAN rows");
    assert_eq!(
        w.row(GPU)["approval"],
        "pending",
        "find must not touch the GPU row"
    );
    assert_eq!(w.row(LAN)["vram_gb"], 24);

    step("start serve-route with --cpu (CPU is the only operator row)");
    let rerr = std::fs::File::create(w.work.join("router.err")).unwrap();
    let child = Command::new(w.bin.join("openmind-discovery"))
        .args(["serve-route", "--bind", &format!("127.0.0.1:{}", w.p(2))])
        .arg("--registry")
        .arg(w.reg())
        .args(["--cpu", &format!("http://127.0.0.1:{}", w.p(1))])
        .args(["--timeout-ms", "1000", "--duration-s", "300"])
        .stdout(Stdio::null())
        .stderr(rerr)
        .spawn()
        .expect("spawn serve-route");
    procs.add("router", child);
    wait_up(w.p(2));
    assert_eq!(http(w.p(2), "GET", "/healthz", None).status, 200);

    step("S1 nothing approved: chain = [cpu-local], chat -> CPU");
    assert_eq!(w.chain(), ["cpu-local"]);
    w.expect_chat("cpu-local", "cpu-local:ok", "reply from cpu");

    step("S2 approve the fake GPU -> GPU first");
    let (ok, v, e) = w.d(&["approve", GPU, "--note", "S6.3 approve gpu"]);
    assert!(ok, "{e}");
    println!("approve: {v}");
    assert_eq!(w.chain(), [GPU, "cpu-local"]);
    w.expect_chat(GPU, &format!("{GPU}:ok"), "reply from gpu");

    step("S3 approve LAN (not joined: not in chain), join -> chain GPU, LAN, CPU");
    let (ok, _, e) = w.d(&["approve", LAN, "--note", "S6.3 approve lan"]);
    assert!(ok, "{e}");
    assert_eq!(
        w.chain(),
        [GPU, "cpu-local"],
        "unjoined LAN row must not route"
    );
    let (ok, v, e) = w.d(&["join", LAN]);
    assert!(ok, "{e}");
    println!("join lan: {v}");
    assert_eq!(w.row(LAN)["health"], "up");
    assert_eq!(w.chain(), [GPU, LAN, "cpu-local"]);
    w.expect_chat(GPU, &format!("{GPU}:ok"), "reply from gpu");
    w.expect_chat(GPU, &format!("{GPU}:ok"), "reply from gpu");

    step("S4 POSEIDON on the same registry: pool = the two approved rows only");
    let qout = std::fs::File::create(w.work.join("queue.out")).unwrap();
    let child = Command::new(w.bin.join("openmind-queue"))
        .args(["serve", "--bind", &format!("127.0.0.1:{}", w.p(7))])
        .arg("--data")
        .arg(w.work.join("poseidon"))
        .arg("--providers")
        .arg(w.reg())
        .stdout(qout.try_clone().unwrap())
        .stderr(qout)
        .spawn()
        .expect("spawn openmind-queue");
    procs.add("queue", child);
    wait_up(w.p(7));
    let pool = w.pool();
    let mut ids: Vec<String> = pool["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, [GPU, LAN], "pending lan-other must not be in the pool");
    assert_eq!(pool["approved_gb"], 104);
    assert_eq!(pool["gap_gb"], 1500 - 104);
    assert_eq!(pool["routable_devices"], 2);
    assert!(w.job(40, false).status < 300);
    assert_eq!(w.lease_ack()["device"], GPU, "40 GB fits only the GPU");
    assert!(w.job(16, false).status < 300);
    assert_eq!(w.lease_ack()["device"], GPU, "GPU first for 16 GB");

    step("S5 the GPU hangs (SIGSTOP): same request falls back to LAN after the 1000 ms timeout");
    signal(gpu_pid, "-STOP");
    let ms = w.expect_chat(
        LAN,
        &format!("{GPU}:io-error,{LAN}:ok"),
        "reply from lan-gpu",
    );
    assert!((900..4000).contains(&ms), "hang fallback took {ms} ms");
    signal(gpu_pid, "-CONT");
    std::thread::sleep(Duration::from_millis(200));
    w.expect_chat(GPU, &format!("{GPU}:ok"), "reply from gpu");

    step("S6 the GPU dies (killed): next request falls back to LAN on its own");
    let gpu_before_kill = w.hits("mock-gpu.err");
    w.kill_gpu(&mut procs);
    let ms = w.expect_chat(
        LAN,
        &format!("{GPU}:connect-failed,{LAN}:ok"),
        "reply from lan-gpu",
    );
    assert!(ms < 1000, "connect-failed fallback took {ms} ms");
    let (ok, v, e) = w.d(&["join", GPU]);
    assert!(ok, "{e}");
    println!("join gpu: {v}");
    assert_eq!(w.row(GPU)["health"], "down");
    assert_eq!(
        w.chain(),
        [LAN, "cpu-local"],
        "a down GPU is not even tried"
    );
    w.expect_chat(LAN, &format!("{LAN}:ok"), "reply from lan-gpu");
    w.sync();
    let pool = w.pool();
    assert_eq!(pool["routable_devices"], 1);
    let r = w.job(40, false);
    assert!(
        r.status >= 400,
        "40 GB without cpu_fallback must be refused when only 24 GB is routable"
    );

    step("S7 the LAN provider dies too: falls back to CPU");
    w.kill_lan(&mut procs);
    w.expect_chat(
        "cpu-local",
        &format!("{LAN}:connect-failed,cpu-local:ok"),
        "reply from cpu",
    );
    let (ok, _, e) = w.d(&["join", LAN]);
    assert!(ok, "{e}");
    assert_eq!(w.row(LAN)["health"], "down");
    assert_eq!(w.chain(), ["cpu-local"]);
    w.expect_chat("cpu-local", "cpu-local:ok", "reply from cpu");
    w.sync();
    assert_eq!(w.pool()["routable_devices"], 0);
    let r = w.job(16, false);
    assert!(
        r.status >= 400,
        "no routable device: 16 GB without cpu_fallback refused"
    );
    let r = w.job(16, true);
    assert!(r.status < 300, "cpu_fallback job accepted");
    let lease = w.lease_ack();
    assert!(
        lease["device"].as_str().unwrap_or("").contains("cpu"),
        "cpu_fallback job must lease on the CPU, got {}",
        lease["device"]
    );

    step(&format!(
        "S8 lan-gpu comes back on a NEW port :{} and is announced there",
        w.p(6)
    ));
    w.mock(&mut procs, "lan-gpu", "mock-lan-moved.err", w.p(6));
    w.announce(&mut procs, "ann-lan2", "lan-gpu", w.p(6), 24, 6);
    std::thread::sleep(Duration::from_millis(500));
    let found = w.find();
    let seen = found["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["provider_id"] == LAN)
        .cloned()
        .unwrap_or(Value::Null);
    println!("find (moved): {seen}");
    let moved_ep = format!("http://127.0.0.1:{}", w.p(6));
    let old_ep = format!("http://127.0.0.1:{}", w.p(4));
    assert_eq!(seen["seen_endpoint"], moved_ep.as_str(), "move not seen");
    assert_eq!(
        w.row(LAN)["endpoint"],
        old_ep.as_str(),
        "endpoint changed before re-approval"
    );
    assert_eq!(w.row(LAN)["pending_endpoint"], moved_ep.as_str());
    let (ok, _, e) = w.d(&["join", LAN]);
    assert!(ok, "{e}");
    assert_eq!(
        w.row(LAN)["health"],
        "down",
        "join dials the approved (old) endpoint"
    );
    w.expect_chat("cpu-local", "cpu-local:ok", "reply from cpu");
    assert_eq!(
        w.hits("mock-lan-moved.err"),
        0,
        "moved device used before re-approval"
    );

    step(
        "S9 re-approve the moved LAN row: move applied, health reset, join -> LAN at the new port",
    );
    let (ok, v, e) = w.d(&["approve", LAN, "--note", "S6.3 moved"]);
    assert!(ok, "{e}");
    println!("re-approve: {v}");
    assert_eq!(w.row(LAN)["endpoint"], moved_ep.as_str());
    assert_eq!(w.row(LAN)["health"], "unknown");
    assert!(w.row(LAN).get("pending_endpoint").is_none());
    w.expect_chat("cpu-local", "cpu-local:ok", "reply from cpu");
    let (ok, _, e) = w.d(&["join", LAN]);
    assert!(ok, "{e}");
    w.expect_chat(LAN, &format!("{LAN}:ok"), "reply from lan-gpu");
    assert_eq!(w.hits("mock-lan-moved.err"), 1);
    w.sync();
    let pool = w.pool();
    assert_eq!(pool["routable_devices"], 1);

    step("S10 the GPU comes back on its port: join up -> GPU first again");
    let pid2 = w.mock(&mut procs, "gpu", "mock-gpu2.err", w.p(3));
    let (ok, _, e) = w.d(&["join", GPU]);
    assert!(ok, "{e}");
    assert_eq!(w.row(GPU)["health"], "up");
    assert_eq!(w.chain(), [GPU, LAN, "cpu-local"]);
    w.expect_chat(GPU, &format!("{GPU}:ok"), "reply from gpu");
    w.sync();
    assert_eq!(w.pool()["routable_devices"], 2);

    step("S11 deny LAN: never used again, even when the GPU dies");
    let lan_before = w.hits("mock-lan-moved.err");
    let (ok, _, e) = w.d(&["deny", LAN, "--note", "S6.3 deny"]);
    assert!(ok, "{e}");
    assert_eq!(w.chain(), [GPU, "cpu-local"]);
    signal(pid2, "-KILL");
    std::thread::sleep(Duration::from_millis(200));
    w.expect_chat(
        "cpu-local",
        &format!("{GPU}:connect-failed,cpu-local:ok"),
        "reply from cpu",
    );
    let (ok, _, e) = w.d(&["join", LAN]);
    assert!(!ok, "join of a denied row must fail");
    println!("join denied: {}", e.trim());
    assert_eq!(
        w.hits("mock-lan-moved.err"),
        lan_before,
        "denied device was used"
    );
    w.sync();
    let pool = w.pool();
    // Finding: POSEIDON health is the last `join` snapshot. The router saw the
    // GPU die but never writes health back, so the pool still counts it.
    assert_eq!(
        pool["routable_devices"], 1,
        "stale until a join records down"
    );
    assert_eq!(w.row(GPU)["health"], "up");
    let (ok, _, e) = w.d(&["join", GPU]);
    assert!(ok, "{e}");
    w.sync();
    let pool = w.pool();
    assert_eq!(pool["routable_devices"], 0);
    assert_eq!(
        pool["approved_gb"], 80,
        "a denied row leaves the approved total"
    );

    step("S12 the CPU dies too: 503 no_provider, every attempt listed");
    w.kill_cpu(&mut procs);
    let (p, a, c, s, _) = w.chat();
    assert_eq!(s, 503);
    assert_eq!(p, "none");
    assert_eq!(c, "no_provider");
    assert_eq!(
        a, "cpu-local:connect-failed",
        "down GPU and denied LAN not tried"
    );

    step("S13 never-approved lan-other got 0 requests; logs carry Zagreb offsets");
    assert_eq!(w.hits("mock-other.err"), 0, "pending device was used");
    assert_eq!(
        w.row(OTHER)["approval"],
        "pending",
        "lan-other stays pending"
    );
    assert!(
        !w.pool()["devices"].to_string().contains(OTHER),
        "pending row in pool"
    );
    let counts: Vec<String> = [
        "mock-cpu.err",
        "mock-gpu.err",
        "mock-gpu2.err",
        "mock-lan.err",
        "mock-lan-moved.err",
        "mock-other.err",
    ]
    .iter()
    .map(|f| format!("{f}={}", w.hits(f)))
    .collect();
    println!("mock chat counts: {}", counts.join(" "));
    // S2 1 + S3 2 + S5 2 = 5: the request the hung GPU held was still
    // executed after SIGCONT, after the router had already failed over to
    // LAN (a late duplicate; see docs/real-tests/gaia-lan-fallback-test.md).
    assert_eq!(gpu_before_kill, 5, "hung request executed late on the GPU");
    println!(
        "late duplicate: the hung GPU answered the failed-over request after SIGCONT (count 5)"
    );
    let log = std::fs::read_to_string(w.log()).unwrap();
    println!("approvals log: {} lines", log.lines().count());
    for l in log.lines() {
        println!("  {l}");
    }
    assert!(!log.contains("Z\""), "UTC Z timestamp in approvals log");
    assert!(log.contains("+02:00") || log.contains("+01:00"));
    let rlog = std::fs::read_to_string(w.work.join("router.err")).unwrap();
    println!("router log: {} lines", rlog.lines().count());
    for l in rlog.lines() {
        println!("  {l}");
    }

    step("teardown");
    drop(procs);
    std::thread::sleep(Duration::from_millis(300));
    for off in 1..=7 {
        assert!(
            port_free(base + off),
            "port {} still busy after teardown",
            base + off
        );
    }
    let _ = std::fs::remove_dir_all(&w.work);
    println!(
        "PASS in {:.1} s ({})",
        started.elapsed().as_secs_f64(),
        stamp()
    );
}

impl World {
    fn kill_gpu(&self, procs: &mut Procs) {
        procs.kill("mock-gpu.err");
        assert!(!up(self.p(3)));
    }
    fn kill_lan(&self, procs: &mut Procs) {
        procs.kill("mock-lan.err");
        procs.kill("ann-lan");
        assert!(!up(self.p(4)));
    }
    fn kill_cpu(&self, procs: &mut Procs) {
        procs.kill("mock-cpu.err");
        assert!(!up(self.p(1)));
    }
}
