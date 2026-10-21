// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life scenario for POSEIDON (`openmind-queue`), Rust port of the old
//! `scripts/real/poseidon.sh` (slot 12). Runs only with `GALION_REAL=1`.
//!
//! Release `openmind-queue serve` on base+0 and a real `tuner` on base+2 (a
//! box-local HTTP endpoint for the probe dry-run); default base 19120,
//! `GALION_REAL_PORT_BASE` moves them. Drives the queue over real HTTP and
//! with the built-in worker: ordering, refusals (money, budget hard stop,
//! quota, bad input), retry/backoff/dead letters/redrive, lease timeout and
//! fencing, the VRAM pool (provider-gated devices, GPU->LAN->CPU), GAIA
//! dispatch, kill -9 + torn tail + replay, compaction, and the loopback-only
//! probe. No network beyond 127.0.0.1, no paid calls, nothing on the VPS.
//! The SIGKILL-mid-lease check on its own port is `tests/real_binary.rs`.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-queue --test real_poseidon -- --nocapture
//! ```

use galion_realtest::http;
use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct Q {
    addr: String,
    last: u16,
}

impl Q {
    fn req(&mut self, method: &str, path: &str, body: &str) -> Value {
        let r = http::request(
            &self.addr,
            method,
            path,
            &[("Content-Type", "application/json")],
            body.as_bytes(),
            Duration::from_secs(10),
        )
        .unwrap_or_else(|e| panic!("{method} {path}: {e}"));
        self.last = r.status;
        r.json().unwrap_or(Value::Null)
    }
    fn post(&mut self, path: &str, body: &str) -> Value {
        self.req("POST", path, body)
    }
    fn get(&mut self, path: &str) -> Value {
        self.req("GET", path, "")
    }
    fn submit(&mut self, body: &str) -> u64 {
        let v = self.post("/jobs", body);
        v["id"]
            .as_u64()
            .unwrap_or_else(|| panic!("submit {body}: {} {v}", self.last))
    }
    fn lease(&mut self, body: &str) -> Value {
        self.post("/lease", body)
    }
    fn ack(&mut self, l: &Value) -> Value {
        let path = format!("/jobs/{}/ack", l["id"]);
        self.post(&path, &format!("{{\"token\":{}}}", l["token"]))
    }
}

fn start_server(bin: &Path, addr: &str, data: &Path, providers: &Path, log: &Path) -> Proc {
    let mut cmd = Command::new(bin);
    cmd.args(["serve", "--bind", addr])
        .arg("--data")
        .arg(data)
        .arg("--providers")
        .arg(providers)
        .args(["--lease-ms", "1500", "--max-attempts", "3"])
        .args(["--backoff-base-ms", "200", "--backoff-max-ms", "1000"]);
    let p = Proc::spawn("openmind-queue", cmd, log);
    assert!(
        realrun::wait_http(addr, "/healthz", Duration::from_secs(5)),
        "server did not come up"
    );
    p
}

/// Run the built-in worker to completion; returns (exit code, its output).
fn worker(bin: &Path, addr: &str, args: &[&str]) -> (i32, String) {
    let o = realrun::run(
        Command::new(bin)
            .args(["worker", "--addr", addr])
            .args(args),
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    println!("{}", text.trim_end());
    (o.status.code().unwrap_or(-1), text)
}

fn count(text: &str, all: &[&str]) -> usize {
    text.lines()
        .filter(|l| all.iter().all(|p| l.contains(p)))
        .count()
}

fn short(v: &Value) -> String {
    v.to_string().chars().take(200).collect()
}

#[test]
fn poseidon_queue_end_to_end() {
    if !realrun::enabled("real_poseidon") {
        return;
    }
    let base = realrun::port_base(19120);
    let (port, tport) = (base, base + 2);
    realrun::require_free(&[port, tport]);
    let bins = realrun::release_bins(&["-p", "openmind-queue", "-p", "tuner"]);
    let (bin, tbin) = (bins.join("openmind-queue"), bins.join("tuner"));
    let addr = format!("127.0.0.1:{port}");
    let w = Scratch::new("poseidon");
    let data: PathBuf = w.path("data");
    let providers = w.path("providers.json");
    let slog = w.path("server.log");
    let v = realrun::run(Command::new(&bin).arg("version"));
    realrun::say(format!(
        "binary {} {} bytes; {}",
        bin.display(),
        std::fs::metadata(&bin).map(|m| m.len()).unwrap_or(0),
        String::from_utf8_lossy(&v.stdout).trim()
    ));
    std::fs::write(
        &providers,
        r#"{"version":1,"providers":[
 {"id":"lan-box","kind":"lan","endpoint":"http://192.168.1.50:8080","capabilities":["llm"],"approval":"approved","health":"up"},
 {"id":"runpod-pod","kind":"remote","endpoint":"https://api.runpod.example/pod","capabilities":["llm"],"approval":"pending","health":"unknown"}
]}"#,
    )
    .unwrap();
    let mut c = Checks::new();
    let mut q = Q {
        addr: addr.clone(),
        last: 0,
    };

    realrun::say("start server");
    let mut server = start_server(&bin, &addr, &data, &providers, &slog);
    let h = q.get("/healthz");
    realrun::say(format!("healthz {h}"));
    c.eq("healthz function", h["function"].as_str(), Some("POSEIDON"));
    let t = h["time"].as_str().unwrap_or("");
    c.that(
        &format!("Zagreb offset in time ({t})"),
        t.ends_with("+02:00") || t.ends_with("+01:00"),
    );

    realrun::say("== 1. ordering / priority");
    let i1 = q.submit(r#"{"queue":"order","kind":"echo","priority":1}"#);
    let i9a = q.submit(r#"{"queue":"order","kind":"echo","priority":9}"#);
    let i5 = q.submit(r#"{"queue":"order","kind":"echo","priority":5}"#);
    let i9b = q.submit(r#"{"queue":"order","kind":"echo","priority":9}"#);
    let mut got = Vec::new();
    for _ in 0..4 {
        let l = q.lease(r#"{"queue":"order","worker":"cli"}"#);
        got.push(l["id"].as_u64().unwrap_or(0));
        q.ack(&l);
    }
    c.eq(
        "lease order prio 9,9,5,1 (FIFO within 9)",
        got,
        vec![i9a, i9b, i5, i1],
    );
    c.eq(
        "empty queue miss",
        q.lease(r#"{"queue":"order","worker":"cli"}"#)["miss"]
            .as_str()
            .map(str::to_string),
        Some("empty".into()),
    );

    realrun::say("== 2. refusals: money, budget hard stop, quota, bad input");
    q.post("/jobs", r#"{"kind":"revolut-payout"}"#);
    c.eq("money kind refused", q.last, 403);
    q.post("/jobs", r#"{"kind":"echo","nope":1}"#);
    c.eq("unknown field 400", q.last, 400);
    q.post("/caps", r#"{"scope":"capped","cap":25}"#);
    c.eq("set cap", q.last, 200);
    for _ in 0..3 {
        q.submit(r#"{"queue":"cap","agent":"capped","kind":"echo","cost":10}"#);
    }
    let c1 = q.lease(r#"{"queue":"cap","worker":"cli"}"#);
    q.lease(r#"{"queue":"cap","worker":"cli"}"#);
    c.eq(
        "third lease hits budget hard stop",
        q.lease(r#"{"queue":"cap","worker":"cli"}"#)["miss"]
            .as_str()
            .map(str::to_string),
        Some("budget_hard_stop".into()),
    );
    let path = format!("/jobs/{}/ack", c1["id"]);
    let r = q.post(
        &path,
        &format!("{{\"token\":{},\"cost\":999}}", c1["token"]),
    );
    c.eq(
        "over-report clamped to reservation",
        r["charged"].as_u64(),
        Some(10),
    );
    q.post(
        "/jobs",
        r#"{"queue":"cap","agent":"capped","kind":"echo","cost":30}"#,
    );
    c.eq("cost over cap refused 402", q.last, 402);
    let s = q.get("/stats");
    let capped = &s["agents"]["capped"];
    c.that(
        "capped spent+reserved <= cap",
        capped["spent"].as_u64().unwrap_or(99) + capped["reserved"].as_u64().unwrap_or(99) <= 25,
    );
    q.post(
        "/quotas",
        r#"{"agent":"q1","max_pending":2,"max_inflight":1}"#,
    );
    q.submit(r#"{"queue":"quota","agent":"q1","kind":"echo"}"#);
    q.submit(r#"{"queue":"quota","agent":"q1","kind":"echo"}"#);
    q.post("/jobs", r#"{"queue":"quota","agent":"q1","kind":"echo"}"#);
    c.eq("max_pending quota 429", q.last, 429);

    realrun::say("== 3. retry with backoff, dead letters, redrive (built-in worker)");
    let fl = q.submit(r#"{"queue":"work","kind":"flaky","payload":"3","max_attempts":3}"#);
    let fa = q.submit(r#"{"queue":"work","kind":"fail","max_attempts":2}"#);
    let ec = q.submit(r#"{"queue":"work","kind":"sleep","payload":"50","priority":5}"#);
    let (_, wl) = worker(
        &bin,
        &addr,
        &[
            "--queue",
            "work",
            "--worker",
            "w1",
            "--idle-exit-ms",
            "2500",
        ],
    );
    let jfl = format!("job={fl} kind=flaky");
    c.eq(
        "flaky acked on attempt 3",
        count(&wl, &[&jfl, "attempt=3", "result=ack"]),
        1,
    );
    c.eq(
        "flaky nacked twice first",
        count(&wl, &[&jfl, "result=nack"]),
        2,
    );
    let first = wl
        .split_whitespace()
        .find(|t| t.starts_with("job="))
        .map(str::to_string);
    c.eq(
        "sleep job (prio 5) ran first",
        first,
        Some(format!("job={ec}")),
    );
    let d = q.get("/dlq");
    let att = d["dead"].as_array().and_then(|a| {
        a.iter()
            .find(|j| j["id"].as_u64() == Some(fa))
            .map(|j| j["attempts"].as_u64())
    });
    c.eq("fail job dead after 2 attempts", att, Some(Some(2)));
    q.post(&format!("/jobs/{fa}/redrive"), "");
    c.eq("redrive", q.last, 200);
    c.eq(
        "redriven job ready",
        q.get(&format!("/jobs/{fa}"))["state"]
            .as_str()
            .map(str::to_string),
        Some("ready".into()),
    );

    realrun::say("== 4. lease timeout -> redelivery, stale token fenced");
    let to = q.submit(r#"{"queue":"timeout","kind":"echo","lease_ms":700,"max_attempts":3}"#);
    let l1 = q.lease(r#"{"queue":"timeout","worker":"slow"}"#);
    std::thread::sleep(Duration::from_millis(1200));
    let j = q.get(&format!("/jobs/{to}"));
    let state = j["state"].as_str().unwrap_or("");
    c.that(
        &format!("expired lease requeued ({state})"),
        state == "ready" || state == "delayed",
    );
    c.that(
        "last_error says expired",
        j["last_error"]
            .as_str()
            .unwrap_or("")
            .starts_with("lease expired"),
    );
    let l2 = q.lease(r#"{"queue":"timeout","worker":"fast","wait_ms":3000}"#);
    c.eq("redelivered same job", l2["id"].as_u64(), Some(to));
    c.eq("attempt 2", l2["job"]["attempts"].as_u64(), Some(2));
    q.post(
        &format!("/jobs/{to}/ack"),
        &format!("{{\"token\":{}}}", l1["token"]),
    );
    c.eq("old token fenced 409", q.last, 409);
    q.ack(&l2);
    c.eq("new token acks", q.last, 200);
    let cr = q.submit(r#"{"queue":"crash","kind":"crash","lease_ms":600,"max_attempts":2}"#);
    let (rc, _) = worker(&bin, &addr, &["--queue", "crash", "--worker", "crasher"]);
    c.eq("worker crashed mid-lease (exit 86)", rc, 86);
    worker(&bin, &addr, &["--queue", "crash", "--worker", "crasher2"]);
    std::thread::sleep(Duration::from_secs(1));
    let j = q.get(&format!("/jobs/{cr}"));
    c.eq(
        "crashing job dead after 2 expired leases",
        format!("{} {}", j["state"].as_str().unwrap_or(""), j["attempts"]),
        "dead 2".to_string(),
    );

    realrun::say("== 5. VRAM pool: provider-gated devices, GPU->LAN->CPU routing, goal");
    q.post(
        "/vram/devices",
        r#"{"id":"lan-gpu-24","kind":"lan","vram_gb":24,"provider":"lan-box"}"#,
    );
    c.eq(
        "lan device planned until provider sync",
        q.get("/vram")["devices"][0]["device"]["state"]
            .as_str()
            .map(str::to_string),
        Some("planned".into()),
    );
    c.eq(
        "sync-providers changed 1",
        q.post("/vram/sync-providers", "")["changed"].as_u64(),
        Some(1),
    );
    let r = q.post("/vram/devices", r#"{"id":"runpod-a100-80","kind":"remote","vram_gb":80,"provider":"runpod-pod","cents_per_hour":199,"state":"approved"}"#);
    c.eq(
        "paid device demoted to planned",
        r["device"]["state"].as_str().map(str::to_string),
        Some("planned".into()),
    );
    realrun::say(format!("device note: {}", r["note"]));
    let s = &q.get("/vram")["summary"];
    c.eq(
        "approved/planned/gap GB",
        format!("{}/{}/{}", s["approved_gb"], s["planned_gb"], s["gap_gb"]),
        "24/80/1476".to_string(),
    );
    q.post("/jobs", r#"{"queue":"gpu","kind":"echo","vram_gb":48}"#);
    c.eq("48 GB without fallback 503", q.last, 503);
    let g1 = q.submit(r#"{"queue":"gpu","kind":"echo","vram_gb":16,"priority":3}"#);
    let g2 = q.submit(r#"{"queue":"gpu","kind":"echo","vram_gb":16,"priority":2}"#);
    let g3 =
        q.submit(r#"{"queue":"gpu","kind":"echo","vram_gb":16,"priority":1,"cpu_fallback":true}"#);
    let a1 = q.lease(r#"{"queue":"gpu","worker":"g"}"#);
    let a2 = q.lease(r#"{"queue":"gpu","worker":"g"}"#);
    let idd = |l: &Value| format!("{} {}", l["id"], l["device"].as_str().unwrap_or(""));
    c.eq(
        "first 16 GB job on lan-gpu-24",
        idd(&a1),
        format!("{g1} lan-gpu-24"),
    );
    c.eq(
        "second skipped (no room), fallback job offloaded to cpu",
        idd(&a2),
        format!("{g3} cpu"),
    );
    c.eq(
        "vram_held miss",
        q.lease(r#"{"queue":"gpu","worker":"g"}"#)["miss"]
            .as_str()
            .map(str::to_string),
        Some("vram_held".into()),
    );
    q.ack(&a1);
    let a3 = q.lease(r#"{"queue":"gpu","worker":"g"}"#);
    c.eq(
        "freed VRAM goes to waiting job",
        idd(&a3),
        format!("{g2} lan-gpu-24"),
    );
    let out = |v: Value| v["outcome"].as_str().unwrap_or("").to_string();
    c.eq(
        "gaia vram-pool plan is dry-run",
        out(q.post(
            "/gaia/dispatch",
            r#"{"system":"vram-pool","verb":"plan","payload":"8"}"#,
        )),
        "dry-run".to_string(),
    );
    c.eq(
        "gaia money refused",
        out(q.post(
            "/gaia/dispatch",
            r#"{"system":"budget-caps","verb":"run","payload":"RunPod payment top-up"}"#,
        )),
        "refused".to_string(),
    );
    for sys in ["vram-pool", "quotas", "queues", "budget-caps"] {
        let r = q.post(
            "/gaia/dispatch",
            &format!("{{\"system\":\"{sys}\",\"verb\":\"status\"}}"),
        );
        c.eq(
            &format!("gaia {sys} status done"),
            out(r.clone()),
            "done".to_string(),
        );
        realrun::say(format!("gaia {sys}: {}", r["summary"]));
    }

    realrun::say("== 6. crash-restart persistence (kill -9, torn tail, replay)");
    for p in [2, 7, 4, 7, 1] {
        q.submit(&format!(
            "{{\"queue\":\"persist\",\"kind\":\"echo\",\"priority\":{p}}}"
        ));
    }
    let lp = q.lease(r#"{"queue":"persist","worker":"survivor","lease_ms":60000}"#);
    let lpid = lp["id"].as_u64().unwrap_or(0);
    let snap = |q: &mut Q| {
        let s = q.get("/stats");
        serde_json::json!({"depth": s["depth"], "acked": s["stats"]["acked"], "enq": s["stats"]["enqueued"], "g": s["global"], "vram": s["vram"]["used_gb"]})
    };
    let before = snap(&mut q);
    server.kill(); // SIGKILL
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(data.join("queue.log"))
            .unwrap();
        f.write_all(br#"deadbeef {"t":"enqueue","id":99"#).unwrap(); // a torn write
    }
    let lines = std::fs::read_to_string(data.join("queue.log"))
        .map(|s| s.lines().count())
        .unwrap_or(0);
    realrun::say(format!(
        "killed -9; appended a torn line; log is {lines} lines"
    ));
    server = start_server(&bin, &addr, &data, &providers, &slog);
    let after = snap(&mut q);
    c.eq(
        "state identical after kill -9 + replay",
        short(&after),
        short(&before),
    );
    c.that(
        "torn tail cut",
        q.get("/stats")["store"]["truncated_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0,
    );
    q.ack(&lp);
    c.eq("pre-crash lease acks after restart", q.last, 200);
    let mut prios = Vec::new();
    for _ in 0..4 {
        let l = q.lease(r#"{"queue":"persist","worker":"cli"}"#);
        prios.push(l["job"]["spec"]["priority"].as_u64().unwrap_or(0));
        q.ack(&l);
    }
    c.eq("priority order kept after restart", prios, vec![7, 4, 2, 1]);
    let nid = q.submit(r#"{"kind":"echo"}"#);
    c.that(
        "ids continue after restart (new id > pre-crash id)",
        nid > lpid,
    );
    let pre = q.get("/stats");
    let pre = serde_json::json!({"depth": pre["depth"], "enq": pre["stats"]["enqueued"], "acked": pre["stats"]["acked"]});
    realrun::say(format!("compact: {}", q.post("/admin/compact", "")));
    server.stop(Duration::from_secs(3));
    server = start_server(&bin, &addr, &data, &providers, &slog);
    let post = q.get("/stats");
    let post = serde_json::json!({"depth": post["depth"], "enq": post["stats"]["enqueued"], "acked": post["stats"]["acked"]});
    c.eq("state after compaction + restart", post, pre);

    realrun::say("== 7. dry-run probe against a real local endpoint (tuner), read-only GET");
    std::fs::create_dir_all(w.path("www")).unwrap();
    std::fs::write(
        w.path("www/index.html"),
        "<!doctype html><title>tuner</title>\n",
    )
    .unwrap();
    let mut tcmd = Command::new(&tbin);
    tcmd.env("TUNER_ADDR", format!("127.0.0.1:{tport}"))
        .env("TUNER_ROOT", w.path("www"));
    let tuner = Proc::spawn("tuner", tcmd, &w.path("tuner.log"));
    assert!(realrun::wait_http(
        &format!("127.0.0.1:{tport}"),
        "/",
        Duration::from_secs(3)
    ));
    let p1 = q.submit(&format!(r#"{{"queue":"probe","kind":"probe","payload":"http://127.0.0.1:{tport}/index.html","max_attempts":1}}"#));
    let p2 = q.submit(&format!(r#"{{"queue":"probe","kind":"probe","payload":"http://127.0.0.1:{port}/healthz","max_attempts":1}}"#));
    let p3 = q.submit(
        r#"{"queue":"probe","kind":"probe","payload":"http://10.0.0.1/","max_attempts":1}"#,
    );
    let (_, wp) = worker(
        &bin,
        &addr,
        &["--queue", "probe", "--worker", "prober", "--max-jobs", "3"],
    );
    c.eq(
        "tuner probe 200",
        count(&wp, &[&format!("job={p1} "), "-> 200"]),
        1,
    );
    c.eq(
        "self probe 200",
        count(&wp, &[&format!("job={p2} "), "-> 200"]),
        1,
    );
    c.eq(
        "non-loopback probe refused",
        count(&wp, &[&format!("job={p3} "), "probe refused"]),
        1,
    );

    let fin = q.get("/stats");
    realrun::say(format!(
        "final stats depth={} stats={} store={}",
        fin["depth"], fin["stats"], fin["store"]
    ));
    realrun::say(format!("server log:\n{}", server.log_text()));
    drop(tuner);
    server.kill();
    let open = realrun::ports_closed(&[port, tport], Duration::from_secs(3));
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_poseidon");
}
