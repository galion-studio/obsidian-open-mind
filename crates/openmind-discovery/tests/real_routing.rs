// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life LAN routing test (Rust port of the old `scripts/real/lan-routing.sh`).
//! Runs only with `GALION_REAL=1`. No real model runs and nothing paid is
//! called: the "models" are `examples/mock_openai` stand-ins on 127.0.0.1.
//!
//! Ports (slot 31 range by default, `GALION_REAL_PORT_BASE` moves them):
//! base+0/udp mDNS (not 5353), +1 CPU mock (`--cpu`), +2 `serve-route` (router
//! under test), +3 LAN GPU mock (announced, approved), +4 second LAN mock
//! (announced, never approved), +5 LAN GPU after it "moved", +6 openmind-queue
//! (POSEIDON) reading the same provider registry.
//!
//! Flow: announce 2 devices -> find (2 pending) -> route goes to CPU ->
//! approve lan-gpu (still CPU: not joined) -> join up -> routes to LAN ->
//! POSEIDON pool shows lan-gpu only -> kill lan-gpu -> router falls back to
//! CPU on the same request -> join down -> not even tried -> lan-gpu comes
//! back on a new port -> find notes the move, routing still at the old
//! endpoint -> re-approve applies it -> join -> LAN again -> deny -> CPU,
//! never used again -> lan-other (pending) got 0 requests -> teardown.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-discovery --test real_routing -- --nocapture
//! ```

use galion_realtest::http;
use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct Env {
    bin: PathBuf,
    reg: PathBuf,
    log: PathBuf,
}

impl Env {
    /// `openmind-discovery <args> --registry R --log L --by real-test`
    fn d(&self, args: &[&str]) -> (i32, Value, String) {
        let out = realrun::run(
            Command::new(&self.bin)
                .args(args)
                .arg("--registry")
                .arg(&self.reg)
                .arg("--log")
                .arg(&self.log)
                .args(["--by", "real-test"]),
        );
        let v = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (
            out.status.code().unwrap_or(-1),
            v,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn list(&self) -> Value {
        let (_, v) = realrun::run_json(
            Command::new(&self.bin)
                .arg("list")
                .arg("--registry")
                .arg(&self.reg),
        );
        v
    }
}

fn chat(router: &str) -> String {
    let body = json!({"model":"auto","messages":[{"role":"user","content":"ping"}]}).to_string();
    let r = http::post_json(router, "/v1/chat/completions", &[], &body).expect("router chat");
    let v = r.json().unwrap_or(Value::Null);
    let content = v
        .pointer("/choices/0/message/content")
        .or_else(|| v.pointer("/error/type"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    format!(
        "{}|{}|{}",
        r.header("x-galion-provider").unwrap_or(""),
        r.header("x-galion-attempts").unwrap_or(""),
        content
    )
}

fn hits(log: &Path) -> usize {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("POST /v1/chat/completions"))
        .count()
}

fn get_json(addr: &str, path: &str) -> Value {
    http::get(addr, path)
        .and_then(|r| r.json())
        .unwrap_or(Value::Null)
}

fn post(addr: &str, path: &str, body: &str) -> Value {
    http::post_json(addr, path, &[], body)
        .and_then(|r| r.json())
        .unwrap_or(Value::Null)
}

fn pool(q: &str) -> Value {
    let v = get_json(q, "/vram");
    let devices: Vec<Value> = v["devices"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|d| {
            json!({"id": d["device"]["id"], "state": d["device"]["state"],
                   "healthy": d["device"]["healthy"], "routable": d["routable"]})
        })
        .collect();
    let out = json!({"devices": devices, "approved_gb": v["summary"]["approved_gb"]});
    realrun::say(format!("pool {out}"));
    out
}

#[test]
fn approve_route_kill_fallback_move_deny_end_to_end() {
    if !realrun::enabled("real_routing") {
        return;
    }
    let base = realrun::port_base(19310);
    let (mdns, cpu, router, lan, other, moved, queue) = (
        base,
        base + 1,
        base + 2,
        base + 3,
        base + 4,
        base + 5,
        base + 6,
    );
    realrun::require_free(&[cpu, router, lan, other, moved, queue]);
    let bins = realrun::release_bins(&[
        "-p",
        "openmind-discovery",
        "--bin",
        "openmind-discovery",
        "--example",
        "mock_openai",
        "-p",
        "openmind-queue",
        "--bin",
        "openmind-queue",
    ]);
    let work = Scratch::new("lan-routing");
    let e = Env {
        bin: bins.join("openmind-discovery"),
        reg: work.path("providers.json"),
        log: work.path("approvals.log"),
    };
    let mock = bins.join("examples/mock_openai");
    let qbin = bins.join("openmind-queue");
    let raddr = format!("127.0.0.1:{router}");
    let qaddr = format!("127.0.0.1:{queue}");
    let mut c = Checks::new();
    const G: &str = "llamacpp-lan-gpu";
    const O: &str = "llamacpp-lan-other";

    let mock_proc = |name: &str, port: u16, log: &str| {
        let mut cmd = Command::new(&mock);
        cmd.args([name, &port.to_string(), "300"]);
        Proc::spawn(name, cmd, &work.path(log))
    };
    let announce = |name: &str, port: u16, secs: u32, vram: u32| {
        let mut cmd = Command::new(&e.bin);
        cmd.args([
            "announce",
            "--mdns-port",
            &mdns.to_string(),
            "--host",
            "lan-box",
        ])
        .args(["--addr", "127.0.0.1", "--duration-s", &secs.to_string()])
        .arg("--service")
        .arg(format!("llamacpp:{name}:{port}:caps=llm;vram={vram}"));
        Proc::spawn(&format!("announce-{name}"), cmd, &work.path("announce.log"))
    };
    let find = || {
        let (code, v, err) = e.d(&[
            "find",
            "--mdns-port",
            &mdns.to_string(),
            "--services",
            "llamacpp",
            "--timeout-ms",
            "1500",
        ]);
        assert_eq!(code, 0, "find failed: {err}");
        v
    };
    let expect_chat = |c: &mut Checks, want: String| {
        let got = chat(&raddr);
        c.eq(&format!("chat -> {want}"), got, want);
    };
    let cpu_ok = || "cpu-local|cpu-local:ok|reply from cpu".to_string();
    let lan_ok = || format!("{G}|{G}:ok|reply from lan-gpu");

    realrun::say("start mock OpenAI servers: cpu, lan-gpu, lan-other");
    let _cpu = mock_proc("cpu", cpu, "mock-cpu.err");
    let mut lan_p = mock_proc("lan-gpu", lan, "mock-lan-gpu.err");
    let _other = mock_proc("lan-other", other, "mock-lan-other.err");
    let mut lan_ann = announce("lan-gpu", lan, 30, 24);
    let _other_ann = announce("lan-other", other, 6, 48);
    std::thread::sleep(Duration::from_millis(500));

    realrun::say("find (mDNS browse)");
    let v = find();
    let pending = v["devices"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|d| d["status"] == "new" && d["approval"] == "pending")
                .count()
        })
        .unwrap_or(0);
    c.eq("2 new pending rows after find", pending, 2);

    realrun::say("start serve-route");
    let mut rcmd = Command::new(&e.bin);
    rcmd.args(["serve-route", "--bind", &raddr])
        .arg("--registry")
        .arg(&e.reg)
        .args(["--cpu", &format!("http://127.0.0.1:{cpu}")])
        .args(["--timeout-ms", "1000", "--duration-s", "240"]);
    let mut router_p = Proc::spawn("serve-route", rcmd, &work.path("router.err"));
    assert!(realrun::wait_http(
        &raddr,
        "/healthz",
        Duration::from_secs(5)
    ));
    realrun::say(format!("route {}", get_json(&raddr, "/v1/route")));

    realrun::say("both LAN rows pending: routed to the CPU fallback");
    expect_chat(&mut c, cpu_ok());

    realrun::say("approve lan-gpu (not joined yet: still CPU)");
    let (code, _, _) = e.d(&["approve", G, "--note", "real test: approve"]);
    c.eq("approve exit", code, 0);
    expect_chat(&mut c, cpu_ok());

    realrun::say("join lan-gpu -> up, routed to the LAN GPU");
    let (_, j, _) = e.d(&["join", G]);
    c.eq("join health up", j["health"].as_str(), Some("up"));
    expect_chat(&mut c, lan_ok());
    expect_chat(&mut c, lan_ok());
    let m = http::get(&raddr, "/v1/models").expect("models");
    c.eq(
        "/v1/models provider header",
        m.header("x-galion-provider"),
        Some(G),
    );

    realrun::say("POSEIDON reads the same registry: VRAM pool = approved rows only");
    let mut qcmd = Command::new(&qbin);
    qcmd.args(["serve", "--bind", &qaddr])
        .arg("--data")
        .arg(work.path("poseidon"))
        .arg("--providers")
        .arg(&e.reg);
    let mut queue_p = Proc::spawn("openmind-queue", qcmd, &work.path("queue.out"));
    assert!(realrun::wait_http(
        &qaddr,
        "/healthz",
        Duration::from_secs(5)
    ));
    let p = pool(&qaddr);
    c.eq(
        "pool holds the approved row only",
        p["devices"].as_array().map(|a| {
            a.iter()
                .map(|d| d["id"].as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
        }),
        Some(vec![G.to_string()]),
    );
    let job16 = |extra: &str| {
        post(
            &qaddr,
            "/jobs",
            &format!(
                r#"{{"queue":"gpu","agent":"real","kind":"echo","vram_gb":16{extra},"payload":"hi"}}"#
            ),
        )
    };
    let lease = || {
        post(
            &qaddr,
            "/lease",
            r#"{"queue":"gpu","worker":"w1","wait_ms":300}"#,
        )
    };
    let j1 = job16("");
    c.that("16 GB job accepted", j1["id"].is_u64());
    let l1 = lease();
    c.eq("lease lands on lan-gpu", l1["device"].as_str(), Some(G));

    realrun::say("kill lan-gpu (server and its announcer): next request falls back on its own");
    lan_p.kill();
    lan_ann.kill();
    expect_chat(
        &mut c,
        format!("cpu-local|{G}:connect-failed,cpu-local:ok|reply from cpu"),
    );

    realrun::say("join -> down; the router no longer even tries it");
    let (_, j, _) = e.d(&["join", G]);
    c.eq("join health down", j["health"].as_str(), Some("down"));
    expect_chat(&mut c, cpu_ok());
    realrun::say(format!("sync {}", post(&qaddr, "/vram/sync-providers", "")));
    let p = pool(&qaddr);
    c.eq(
        "down device not routable",
        p["devices"][0]["routable"].as_bool(),
        Some(false),
    );
    let r = http::post_json(
        &qaddr,
        "/jobs",
        &[],
        r#"{"queue":"gpu","agent":"real","kind":"echo","vram_gb":16,"payload":"hi"}"#,
    )
    .expect("job");
    realrun::say(format!(
        "16 GB job without cpu_fallback: {} {}",
        r.status,
        r.text()
    ));
    c.eq(
        "16 GB job without fallback refused",
        r.json()
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string)),
        Some("vram_unavailable".to_string()),
    );
    let jf = job16(r#","cpu_fallback":true"#);
    c.that("16 GB job with cpu_fallback accepted", jf["id"].is_u64());
    let lf = lease();
    c.eq(
        "fallback job leased to cpu",
        lf["device"].as_str(),
        Some("cpu"),
    );

    realrun::say("lan-gpu comes back on a NEW port and is announced there");
    let _lan2 = mock_proc("lan-gpu", moved, "mock-lan-gpu.err.2");
    let _ann2 = announce("lan-gpu", moved, 4, 24);
    std::thread::sleep(Duration::from_millis(500));
    let v = find();
    let row = v["devices"]
        .as_array()
        .and_then(|a| a.iter().find(|d| d["provider_id"] == G).cloned())
        .unwrap_or(Value::Null);
    c.eq(
        "move seen",
        row["seen_endpoint"].as_str().map(str::to_string),
        Some(format!("http://127.0.0.1:{moved}")),
    );
    let reg: Value = serde_json::from_str(&std::fs::read_to_string(&e.reg).unwrap()).unwrap();
    let ep = reg["providers"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["id"] == G))
        .map(|p| p["endpoint"].as_str().unwrap_or("").to_string());
    c.eq(
        "moved row unchanged before re-approval",
        ep,
        Some(format!("http://127.0.0.1:{lan}")),
    );
    let _ = e.d(&["join", G]);
    expect_chat(&mut c, cpu_ok());

    realrun::say("re-approve: the move is applied, health resets until join");
    let note = format!("real test: moved to :{moved}");
    let _ = e.d(&["approve", G, "--note", &note]);
    expect_chat(&mut c, cpu_ok());
    let (_, j, _) = e.d(&["join", G]);
    c.eq("join after move up", j["health"].as_str(), Some("up"));
    expect_chat(&mut c, lan_ok());
    let _ = post(&qaddr, "/vram/sync-providers", "");
    pool(&qaddr);

    realrun::say("deny: never used again");
    let before = hits(&work.path("mock-lan-gpu.err.2"));
    let _ = e.d(&["deny", G, "--note", "real test: deny"]);
    expect_chat(&mut c, cpu_ok());
    expect_chat(&mut c, cpu_ok());
    let (code, _, err) = e.d(&["join", G]);
    c.that("join of a denied row refused", code != 0);
    realrun::say(format!("join denied stderr: {}", err.trim()));
    let after = hits(&work.path("mock-lan-gpu.err.2"));
    c.eq("denied device got no more requests", after, before);
    let _ = post(&qaddr, "/vram/sync-providers", "");
    let p = pool(&qaddr);
    c.eq(
        "denied device not routable in POSEIDON",
        p["devices"][0]["routable"].as_bool(),
        Some(false),
    );

    realrun::say("the pending lan-other was never used (router or POSEIDON)");
    c.eq(
        "lan-other chat requests",
        hits(&work.path("mock-lan-other.err")),
        0,
    );
    c.that(
        "pending row not in pool",
        !p["devices"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|d| d["id"] == O),
    );
    realrun::say(format!("final registry {}", e.list()));
    let log = std::fs::read_to_string(&e.log).unwrap_or_default();
    realrun::say(format!("approvals log:\n{log}"));
    c.that("no UTC Z timestamps in approvals log", !log.contains("Z\""));
    realrun::say(format!("router log:\n{}", router_p.log_text()));

    router_p.kill();
    queue_p.kill();
    drop(_cpu);
    drop(_other);
    drop(_lan2);
    let open = realrun::ports_closed(
        &[cpu, router, lan, other, moved, queue],
        Duration::from_secs(3),
    );
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_routing");
}
