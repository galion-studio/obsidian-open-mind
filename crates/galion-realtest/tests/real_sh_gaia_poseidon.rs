// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/gaia-poseidon.sh` (slot 24; deleted on
//! swarm/realscript-port-1). Real-life test for the gaia-core POSEIDON
//! adapter -> openmind-queue.
//!
//! Builds the release `gaia` and `openmind-queue`, starts the queue on
//! loopback, drives it THROUGH `gaia poseidon ...` (submit, inspect, quota and
//! spend-cap refusals, VRAM pool, read-only budget), kills -9 and restarts the
//! queue and checks persistence through gaia, then tears everything down.
//! Operator actions that Gaia must never do (caps, quotas, devices, the worker
//! that leases/acks) are done with the openmind-queue CLI, as an operator
//! would. No network beyond 127.0.0.1, no paid calls, nothing on the VPS.
//!
//! Ports: block 19240 (`GALION_REAL_PORT_BASE`): +0 queue, +9 nothing
//! listens (the "unreachable queue" address).
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_gaia_poseidon -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

struct Env {
    gbin: PathBuf,
    qbin: PathBuf,
    addr: String,
    data: PathBuf,
    work: PathBuf,
}

impl Env {
    /// `gaia ARGS --compact` -> (exit, json); prints the summary like the script.
    fn g(&self, args: &[&str]) -> (i32, Value) {
        let mut c = Command::new(&self.gbin);
        c.args(args)
            .arg("--compact")
            .env("GAIA_POSEIDON_QUEUE", &self.addr);
        let o = run(c);
        let v = json(&out(&o));
        let mut summary = jt(&v, "result.report.summary");
        if summary == "null" {
            summary = jt(&v, "result.state");
        }
        if summary == "null" {
            summary.clear();
        }
        say(&format!(
            "  gaia {} -> exit {}: {summary}",
            args.join(" "),
            code(&o)
        ));
        (code(&o), v)
    }

    /// Operator call with the queue's own CLI; returns stdout.
    fn q(&self, args: &[&str]) -> String {
        let mut c = Command::new(&self.qbin);
        c.args(args).args(["--addr", &self.addr]);
        out(&run(c))
    }

    fn stats(&self) -> Value {
        http::get(&self.addr, "/stats")
            .ok()
            .and_then(|r| r.json().ok())
            .unwrap_or(Value::Null)
    }

    fn start(&self, procs: &mut Procs) -> u32 {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.work.join("server.log"))
            .unwrap();
        let mut c = Command::new(&self.qbin);
        c.args(["serve", "--bind", &self.addr, "--data"])
            .arg(&self.data)
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        let pid = procs.spawn("openmind-queue", c);
        let up = wait_until(Duration::from_secs(5), || {
            http::get(&self.addr, "/healthz").is_ok()
        });
        assert!(
            up,
            "queue did not come up: {}",
            tail(&self.work.join("server.log"), 20)
        );
        pid
    }
}

/// `{g:.global, a:.agents["agent-a"]|{spent,reserved}, acked:.stats.acked, leased:.depth.leased}`
fn budget(s: &Value) -> String {
    let a = &s["agents"]["agent-a"];
    serde_json::json!({
        "g": s["global"], "a": {"spent": a["spent"], "reserved": a["reserved"]},
        "acked": s["stats"]["acked"], "leased": s["depth"]["leased"]
    })
    .to_string()
}

/// `{depth, g:.global, a:.agents["agent-a"]|{cap,spent}}`
fn persisted(s: &Value) -> String {
    let a = &s["agents"]["agent-a"];
    serde_json::json!({"depth": s["depth"], "g": s["global"], "a": {"cap": a["cap"], "spent": a["spent"]}})
        .to_string()
}

fn contains(v: &Value, path: &str, needle: &str) -> String {
    jt(v, path).contains(needle).to_string()
}

#[test]
fn real_sh_gaia_poseidon() {
    if !enabled("gaia-poseidon") {
        return;
    }
    let base = port_base(19240);
    let (port, dead) = (base, base + 9);
    build(&["-p", "gaia", "-p", "openmind-queue"]);
    let work = TempDir(scratch("gaia-poseidon-real"));
    let e = Env {
        gbin: need_bin(&release_bin("gaia")),
        qbin: need_bin(&release_bin("openmind-queue")),
        addr: format!("127.0.0.1:{port}"),
        data: work.0.join("data"),
        work: work.0.clone(),
    };
    let mut c = Command::new(&e.qbin);
    c.arg("version");
    say(out(&run(c)).trim());
    require_free(&[port, dead]);
    let mut ck = Checks::new();
    let mut procs = Procs::new();
    let a = e.addr.as_str();

    say("== ownership: the single config point");
    let (_, sys) = e.g(&["systems"]);
    let mut owners: Vec<String> = sys["result"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter(|s| {
                    ["vram-pool", "quotas", "queues", "budget-caps"]
                        .contains(&s["system"].as_str().unwrap_or(""))
                })
                .map(|s| jt(s, "function"))
                .collect()
        })
        .unwrap_or_default();
    owners.sort();
    owners.dedup();
    ck.eq(
        "compute systems owned by POSEIDON (FunctionId::COMPUTE)",
        "POSEIDON",
        owners.join(","),
    );

    say("== unreachable queue: refused, no stand-in fallback");
    let dead_addr = format!("127.0.0.1:{dead}");
    let (x, v) = e.g(&["poseidon", "status", "--queue", &dead_addr]);
    ck.eq("unreachable exit", "3", x.to_string());
    ck.eq(
        "POSEIDON degraded when queue down",
        "degraded",
        jt(&v, "result.state"),
    );
    let (x, _) = e.g(&[
        "poseidon",
        "submit",
        "--queue",
        &format!("10.1.2.3:{port}"),
        "--agent",
        "x",
    ]);
    ck.eq(
        "non-loopback queue refused (usage error)",
        "2",
        x.to_string(),
    );

    say(&format!("== start openmind-queue on {a}"));
    let mut spid = e.start(&mut procs);
    let (x, v) = e.g(&["poseidon", "status"]);
    ck.eq("status exit", "0", x.to_string());
    ck.eq(
        "POSEIDON terraforming",
        "terraforming",
        jt(&v, "result.state"),
    );
    ck.eq(
        "backend",
        "openmind-queue",
        jt(&v, "result.adapter.backend"),
    );
    let done = v["result"]["systems"]
        .as_array()
        .map_or(0, |s| s.iter().filter(|s| s["outcome"] == "done").count());
    ck.eq("four systems done", "4", done.to_string());

    say("== operator sets caps, quotas and VRAM devices (openmind-queue CLI, not Gaia)");
    e.q(&["post", "/caps", r#"{"scope":"agent-a","cap":100}"#]);
    e.q(&["post", "/caps", r#"{"scope":"*","cap":1000}"#]);
    e.q(&[
        "post",
        "/quotas",
        r#"{"agent":"agent-b","max_pending":2,"max_inflight":1}"#,
    ]);
    e.q(&[
        "post",
        "/vram/devices",
        r#"{"id":"gpu0","kind":"gpu","vram_gb":24,"state":"approved"}"#,
    ]);
    e.q(&[
        "post",
        "/vram/devices",
        r#"{"id":"lan-4090","kind":"lan","vram_gb":24,"state":"planned"}"#,
    ]);
    let h100 = e.q(&[
        "post",
        "/vram/devices",
        r#"{"id":"rented-h100","kind":"remote","vram_gb":80,"cents_per_hour":250,"state":"approved"}"#,
    ]);
    // jq 'objects | .device.state' over the CLI output, a JSON stream:
    // the HTTP status number, then the response object.
    let h100_state = serde_json::Deserializer::from_str(&h100)
        .into_iter::<Value>()
        .map_while(Result::ok)
        .filter(|v| v.is_object())
        .map(|v| jt(&v, "device.state"))
        .collect::<Vec<_>>()
        .join(" ");
    ck.eq(
        "priced device without approved provider stays planned",
        "planned",
        h100_state,
    );

    say("== submit and inspect jobs through gaia");
    let (x, v) = e.g(&[
        "poseidon",
        "submit",
        "--agent",
        "agent-a",
        "--kind",
        "echo",
        "--cost",
        "40",
        "--priority",
        "5",
        "--job-payload",
        "hello",
    ]);
    ck.eq("submit 1 done", "0", x.to_string());
    let j1 = jt(&v, "result.report.metrics.job_id");
    let (x, _) = e.g(&[
        "poseidon", "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "40",
    ]);
    ck.eq("submit 2 done", "0", x.to_string());
    let (_, v) = e.g(&["poseidon", "job", &j1]);
    ck.eq(
        &format!("inspect job {j1} live"),
        "1",
        jt(&v, "result.report.metrics.live"),
    );
    ck.eq(
        &format!("inspect job {j1} cost"),
        "40",
        jt(&v, "result.report.metrics.cost"),
    );
    ck.eq(
        &format!("inspect job {j1} priority"),
        "5",
        jt(&v, "result.report.metrics.priority"),
    );
    let (_, v) = e.g(&["poseidon", "job", "999999"]);
    ck.eq(
        "unknown job not live",
        "0",
        jt(&v, "result.report.metrics.live"),
    );
    let (x, _) = e.g(&[
        "dispatch",
        "queues",
        "run",
        "--payload",
        r#"{"agent":"agent-a","kind":"echo","cost":1,"token":7}"#,
        "--queue",
        a,
    ]);
    ck.eq("unknown job field rejected by adapter", "5", x.to_string());

    say("== Gaia never spends: snapshot budget, run every gaia read command, compare");
    let before = budget(&e.stats());
    for cmd in [
        vec!["status"],
        vec!["vram"],
        vec!["caps"],
        vec!["quotas"],
        vec!["dlq"],
        vec!["plan-vram", "16"],
        vec!["job", j1.as_str()],
    ] {
        let mut args = vec!["poseidon"];
        args.extend(cmd);
        e.g(&args);
    }
    for verb in ["run", "execute"] {
        for (sys, payload) in [
            ("budget-caps", "100000"),
            ("quotas", "99"),
            ("vram-pool", "80"),
        ] {
            let (x, _) = e.g(&["dispatch", sys, verb, "--payload", payload, "--queue", a]);
            ck.eq(
                &format!("{sys} {verb} refused (read-only)"),
                "4",
                x.to_string(),
            );
        }
    }
    let after = budget(&e.stats());
    ck.eq(
        "budget, caps, acks and leases unchanged by gaia",
        &before,
        &after,
    );
    ck.eq(
        "agent-a cap still 100",
        "100",
        jt(&e.stats(), "agents.agent-a.cap"),
    );

    say("== operator worker spends 80 units in the queue (lease + ack; Gaia never does this)");
    let w = e.q(&[
        "worker",
        "--worker",
        "op-w1",
        "--max-jobs",
        "2",
        "--idle-exit-ms",
        "1500",
    ]);
    say(&format!(
        "  worker: {} ack(s)",
        w.matches("result=ack").count()
    ));
    let (_, v) = e.g(&["poseidon", "caps"]);
    ck.eq(
        "caps: agent-a spent 80",
        "80",
        jt(&v, "result.report.metrics.spent:agent-a"),
    );
    ck.eq(
        "caps: agent-a cap 100",
        "100",
        jt(&v, "result.report.metrics.cap:agent-a"),
    );
    ck.eq(
        "caps: global cap 1000",
        "1000",
        jt(&v, "result.report.metrics.global_cap"),
    );

    say("== spend-cap refusal surfaced through gaia");
    let (x, v) = e.g(&[
        "poseidon", "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "40",
    ]);
    ck.eq("spend cap -> exit 4", "4", x.to_string());
    ck.eq(
        "spend cap -> refused",
        "refused",
        jt(&v, "result.report.outcome"),
    );
    ck.eq(
        "spend cap -> 402",
        "402",
        jt(&v, "result.report.metrics.http_status"),
    );
    let (x, v) = e.g(&[
        "poseidon", "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "20",
    ]);
    ck.eq("cost that fits under the cap accepted", "0", x.to_string());
    let j3 = jt(&v, "result.report.metrics.job_id");

    say("== quota refusal surfaced through gaia");
    let (x, _) = e.g(&["poseidon", "submit", "--agent", "agent-b", "--kind", "echo"]);
    ck.eq("agent-b job 1", "0", x.to_string());
    let (x, _) = e.g(&["poseidon", "submit", "--agent", "agent-b", "--kind", "echo"]);
    ck.eq("agent-b job 2", "0", x.to_string());
    let (x, v) = e.g(&["poseidon", "submit", "--agent", "agent-b", "--kind", "echo"]);
    ck.eq("quota -> exit 4", "4", x.to_string());
    ck.eq(
        "quota -> 429",
        "429",
        jt(&v, "result.report.metrics.http_status"),
    );
    let (_, v) = e.g(&["poseidon", "quotas"]);
    ck.eq(
        "quotas: agent-b pending 2",
        "2",
        jt(&v, "result.report.metrics.pending:agent-b"),
    );
    ck.eq(
        "quotas: agent-b max_pending 2",
        "2",
        jt(&v, "result.report.metrics.max_pending:agent-b"),
    );
    ck.eq(
        "quotas: refusals counted",
        "1",
        jt(&v, "result.report.metrics.quota_refusals"),
    );

    say("== money refused (adapter, before the queue)");
    let (x, _) = e.g(&[
        "poseidon", "submit", "--agent", "agent-a", "--kind", "payout",
    ]);
    ck.eq("money kind -> exit 4", "4", x.to_string());

    say("== VRAM pool through gaia");
    let (_, v) = e.g(&["poseidon", "vram"]);
    ck.eq(
        "vram: 24 GB approved",
        "24",
        jt(&v, "result.report.metrics.approved_gb"),
    );
    ck.eq(
        "vram: 3 devices listed",
        "3",
        jt(&v, "result.report.metrics.devices"),
    );
    ck.eq(
        "vram: planned GB counts lan + h100",
        "104",
        jt(&v, "result.report.metrics.planned_gb"),
    );
    let (_, v) = e.g(&["poseidon", "plan-vram", "16"]);
    ck.eq(
        "plan 16 GB dry-run",
        "dry-run",
        jt(&v, "result.report.outcome"),
    );
    ck.eq(
        "plan 16 GB -> gpu0",
        "true",
        contains(&v, "result.report.summary", "gpu0"),
    );
    let (_, v) = e.g(&["poseidon", "plan-vram", "48"]);
    ck.eq(
        "plan 48 GB fits nothing",
        "true",
        contains(&v, "result.report.summary", "fits no approved device"),
    );
    let (x, v) = e.g(&[
        "poseidon",
        "submit",
        "--agent",
        "agent-c",
        "--kind",
        "echo",
        "--vram-gb",
        "48",
    ]);
    ck.eq(
        "48 GB job without cpu fallback -> refused",
        "4",
        x.to_string(),
    );
    ck.eq(
        "48 GB job -> 503",
        "503",
        jt(&v, "result.report.metrics.http_status"),
    );
    let (x, v) = e.g(&[
        "poseidon",
        "submit",
        "--agent",
        "agent-c",
        "--kind",
        "echo",
        "--vram-gb",
        "48",
        "--cpu-fallback",
    ]);
    ck.eq("48 GB job with cpu fallback accepted", "0", x.to_string());
    let j4 = jt(&v, "result.report.metrics.job_id");

    say("== dead-letter queue through gaia");
    let (_, v) = e.g(&[
        "poseidon",
        "submit",
        "--agent",
        "agent-d",
        "--queue-name",
        "dl",
        "--kind",
        "fail",
        "--max-attempts",
        "1",
    ]);
    let jd = jt(&v, "result.report.metrics.job_id");
    e.q(&[
        "worker",
        "--worker",
        "op-w2",
        "--queue",
        "dl",
        "--max-jobs",
        "1",
        "--idle-exit-ms",
        "1500",
    ]);
    let (_, v) = e.g(&["poseidon", "dlq"]);
    ck.eq(
        &format!("dlq lists job {jd}"),
        "true",
        contains(&v, "result.report.summary", &jd),
    );

    say("== restart persistence: kill -9 the queue");
    let pre = persisted(&e.stats());
    signal(spid, "KILL");
    procs.stop("openmind-queue");
    let (x, _) = e.g(&["poseidon", "job", &j3]);
    ck.eq("queue down -> gaia exit 3", "3", x.to_string());
    spid = e.start(&mut procs);
    let post = persisted(&e.stats());
    ck.eq("depth, budget and caps survive kill -9", &pre, &post);
    for jid in [&j3, &j4] {
        let (_, v) = e.g(&["poseidon", "job", jid]);
        ck.eq(
            &format!("job {jid} still live after restart"),
            "1",
            jt(&v, "result.report.metrics.live"),
        );
    }
    let (_, v) = e.g(&["poseidon", "caps"]);
    ck.eq(
        "spent 80 persisted",
        "80",
        jt(&v, "result.report.metrics.spent:agent-a"),
    );
    let (x, _) = e.g(&[
        "poseidon", "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "40",
    ]);
    ck.eq("spend cap still enforced after restart", "4", x.to_string());
    let (x, _) = e.g(&["poseidon", "submit", "--agent", "agent-b", "--kind", "echo"]);
    ck.eq("quota still enforced after restart", "4", x.to_string());
    let (_, v) = e.g(&["poseidon", "vram"]);
    ck.eq(
        "VRAM devices persisted",
        "3",
        jt(&v, "result.report.metrics.devices"),
    );
    let (_, v) = e.g(&["poseidon", "dlq"]);
    ck.eq(
        "dead letter persisted",
        "true",
        contains(&v, "result.report.summary", &jd),
    );
    let (x, _) = e.g(&["poseidon", "status"]);
    ck.eq("status after restart", "0", x.to_string());
    say(&format!("  store: {}", e.stats()["store"]));

    say("== teardown");
    let _ = spid;
    procs.stop("openmind-queue");
    ck.ok(&format!("port {port} closed"), !port_open(port), "");
    drop(procs);
    ck.finish("gaia-poseidon");
}
