// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Full box scenario for `openmind-nerve serve` (Rust port of the old
//! `scripts/real/nerve.sh`, slot 10). Runs only with `GALION_REAL=1`.
//!
//! Release binaries: the nerve, a real `tuner` and a real `room` as watched
//! components, plus a dead "ghost" port. Then:
//! 1. live state / incidents show the dead component,
//! 2. kill tuner -> incident opens; restart it -> incident resolves,
//! 3. scanner lines in a simulated reverse-proxy access log are counted,
//! 4. warm-up builds a request baseline, then a flood of the nerve's own API
//!    (6000 GETs, 8 threads) makes the DDoS detector fire,
//! 5. gaia-live compat (POST /event, GET /events, GET /health),
//! 6. quiet: the flood verdict returns to normal and its incident resolves.
//!
//! Ports (slot 10 range by default, `GALION_REAL_PORT_BASE` moves them):
//! base+0 nerve, +1 tuner, +2 room, +3 ghost (nothing listens on purpose).
//! `NERVE_WARMUP_S` (default 200) sets the baseline warm-up; the whole run
//! takes about 5 min. Systemd/journald probes and reflexes are off.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve --test real_nerve_scenario -- --nocapture
//! ```

use galion_realtest::http;
use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn j(addr: &str, path: &str) -> Value {
    http::request(addr, "GET", path, &[], b"", Duration::from_secs(5))
        .and_then(|r| r.json())
        .unwrap_or(Value::Null)
}

fn titles(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i["title"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn stream(addr: &str, name: &str) -> Value {
    j(addr, "/api/v1/ddos")["streams"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["stream"] == name).cloned())
        .unwrap_or(Value::Null)
}

fn start_tuner(bin: &Path, addr: &str, www: &Path, log: &Path) -> Proc {
    let mut cmd = Command::new(bin);
    cmd.env("TUNER_ADDR", addr).env("TUNER_ROOT", www);
    Proc::spawn("tuner", cmd, log)
}

fn sleep_s(s: u64) {
    std::thread::sleep(Duration::from_secs(s));
}

#[test]
fn incidents_scanner_flood_gaia_compat_end_to_end() {
    if !realrun::enabled("real_nerve_scenario") {
        return;
    }
    let base = realrun::port_base(19100);
    let (np, tp, rp, gp) = (base, base + 1, base + 2, base + 3);
    realrun::require_free(&[np, tp, rp, gp]);
    let warmup: u64 = std::env::var("NERVE_WARMUP_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let bins = realrun::release_bins(&["-p", "openmind-nerve", "-p", "tuner", "-p", "room"]);
    let w = Scratch::new("nerve");
    std::fs::create_dir_all(w.path("www")).unwrap();
    std::fs::create_dir_all(w.path("data")).unwrap();
    std::fs::write(w.path("www/index.html"), "<h1>box tuner</h1>\n").unwrap();
    std::fs::write(
        w.path("components.json"),
        format!(
            r#"[{{"name":"tuner","port":{tp},"path":"/health","gaia":"DEMETER","system":"tuner"}},
 {{"name":"room","port":{rp},"path":"/health","gaia":"HEPHAESTUS","system":"room"}},
 {{"name":"ghost","port":{gp},"path":"/health","gaia":"APOLLO","system":"ghost","note":"nothing listens here on purpose"}}]"#
        ),
    )
    .unwrap();
    std::fs::write(w.path("access.log"), "").unwrap();
    let n = format!("127.0.0.1:{np}");
    let taddr = format!("127.0.0.1:{tp}");
    let mut c = Checks::new();

    let mut tuner = start_tuner(
        &bins.join("tuner"),
        &taddr,
        &w.path("www"),
        &w.path("tuner.log"),
    );
    let mut rcmd = Command::new(bins.join("room"));
    rcmd.env("ROOM_BIND", format!("127.0.0.1:{rp}"))
        .arg("--listen");
    let _room = Proc::spawn("room", rcmd, &w.path("room.log"));
    let mut ncmd = Command::new("nice");
    ncmd.arg("-n")
        .arg("10")
        .arg(bins.join("openmind-nerve"))
        .arg("serve");
    for (k, v) in [
        ("NERVE_ADDR", n.clone()),
        ("NERVE_INTERVAL", "5".into()),
        ("NERVE_DATA_DIR", w.path("data").display().to_string()),
        (
            "NERVE_COMPONENTS",
            w.path("components.json").display().to_string(),
        ),
        ("NERVE_SYSTEMD", "0".into()),
        ("NERVE_JOURNAL", "0".into()),
        ("NERVE_GALION_HEALTH_PORT", "0".into()),
        ("NERVE_REFLEX", "0".into()),
        ("NERVE_LISTEN_SCOPE", format!("{base}-{}", base + 9)),
        (
            "NERVE_ACCESS_LOGS",
            format!("proxy={}@tuner", w.path("access.log").display()),
        ),
        (
            "NERVE_EVENT_LOG",
            w.path("events.jsonl").display().to_string(),
        ),
    ] {
        ncmd.env(k, v);
    }
    let mut nerve = Proc::spawn("openmind-nerve", ncmd, &w.path("nerve.log"));
    sleep_s(12);

    realrun::say("1. live state after 2 cycles");
    let h = j(&n, "/health");
    realrun::say(format!("GET /health {h}"));
    c.eq("health status ok", h["status"].as_str(), Some("ok"));
    let st = j(&n, "/api/v1/state");
    realrun::say(format!("system {}", st["system"]));
    let comp = |st: &Value, name: &str| {
        st["components"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["name"] == name).cloned())
            .unwrap_or(Value::Null)
    };
    c.eq(
        "tuner healthy",
        comp(&st, "tuner")["status"].as_str(),
        Some("healthy"),
    );
    c.eq(
        "room healthy",
        comp(&st, "room")["status"].as_str(),
        Some("healthy"),
    );
    c.eq(
        "ghost failing",
        comp(&st, "ghost")["status"].as_str(),
        Some("failing"),
    );
    let open = titles(&j(&n, "/api/v1/incidents")["open"]);
    c.that(
        "open incident for ghost",
        open.iter().any(|t| t.starts_with("ghost:")),
    );

    realrun::say("2. kill tuner -> incident; restart -> resolved");
    tuner.kill();
    sleep_s(11);
    let open = titles(&j(&n, "/api/v1/incidents")["open"]);
    realrun::say(format!("open {open:?}"));
    c.that(
        "tuner incident opens",
        open.iter().any(|t| t.starts_with("tuner:")),
    );
    let _tuner2 = start_tuner(
        &bins.join("tuner"),
        &taddr,
        &w.path("www"),
        &w.path("tuner.log"),
    );
    sleep_s(16);
    let inc = j(&n, "/api/v1/incidents");
    let resolved = titles(&inc["resolved"]);
    realrun::say(format!("resolved {resolved:?}"));
    c.that(
        "tuner incident resolved",
        resolved.iter().any(|t| t.starts_with("tuner:")),
    );
    c.that(
        "tuner incident no longer open",
        !titles(&inc["open"]).iter().any(|t| t.starts_with("tuner:")),
    );

    realrun::say("3. scanner lines in the proxy access log");
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(w.path("access.log"))
            .unwrap();
        // Common Log Format date; the nerve reads the request line and status.
        let date = Command::new("date")
            .arg("+%d/%b/%Y:%H:%M:%S %z")
            .output()
            .unwrap();
        let date = String::from_utf8_lossy(&date.stdout).trim().to_string();
        for p in ["/.env", "/.git/config", "/wp-login.php", "/index.html"] {
            writeln!(
                f,
                "203.0.113.9 - - [{date}] \"GET {p} HTTP/1.1\" 404 0 \"-\" \"scan\""
            )
            .unwrap();
        }
    }
    sleep_s(6);
    let px = stream(&n, "proxy");
    realrun::say(format!("proxy stream {px}"));
    c.eq("proxy requests_60s", px["requests_60s"].as_u64(), Some(4));
    c.that(
        "proxy scan hits >= 3",
        px["scan_hits_60s"].as_u64().unwrap_or(0) >= 3,
    );

    realrun::say(format!(
        "4. warm-up: light polling for {warmup} s to build a baseline"
    ));
    let end = Instant::now() + Duration::from_secs(warmup);
    while Instant::now() < end {
        let _ = http::get(&n, "/healthz");
        std::thread::sleep(Duration::from_secs(2));
    }
    let api = stream(&n, "nerve-api");
    realrun::say(format!("nerve-api before flood {api}"));
    c.that(
        "baseline built",
        api["baseline_rps"].as_f64().unwrap_or(0.0) > 0.0,
    );

    realrun::say("FLOOD: 6000 GETs, 8 parallel, against the nerve API");
    let t0 = Instant::now();
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let a = n.clone();
            std::thread::spawn(move || {
                for _ in 0..750 {
                    let _ = http::request(&a, "GET", "/healthz", &[], b"", Duration::from_secs(2));
                }
            })
        })
        .collect();
    for t in workers {
        t.join().unwrap();
    }
    realrun::say(format!("flood took {} ms", t0.elapsed().as_millis()));
    sleep_s(6);
    let api = stream(&n, "nerve-api");
    realrun::say(format!("ddos verdict {api}"));
    c.eq(
        "nerve-api level flood",
        api["level"].as_str(),
        Some("flood"),
    );
    c.eq(
        "single-source shape",
        api["shape"].as_str(),
        Some("single-source"),
    );
    let sys = j(&n, "/api/v1/state")["system"].clone();
    realrun::say(format!("system {sys}"));
    c.eq("ddos_firing", sys["ddos_firing"].as_bool(), Some(true));
    let open = titles(&j(&n, "/api/v1/incidents")["open"]);
    c.that(
        "flood incident open",
        open.iter().any(|t| t.starts_with("request flood")),
    );
    let ps = Command::new("ps")
        .args([
            "-o",
            "pid,rss,time,etime,cmd",
            "-p",
            &nerve.pid().to_string(),
        ])
        .output()
        .unwrap();
    realrun::say(format!(
        "nerve CPU/RSS:\n{}",
        String::from_utf8_lossy(&ps.stdout)
    ));

    realrun::say("5. gaia-live compat");
    let r = http::post_json(
        &n,
        "/event",
        &[],
        &format!(r#"{{"source":"gaia-live","kind":"probe","severity":"error","data":{{"port":{gp},"ok":false}}}}"#),
    )
    .expect("POST /event");
    let v = r.json().unwrap_or(Value::Null);
    realrun::say(format!("POST /event {} {v}", r.status));
    c.eq("POST /event ok", v["ok"].as_bool(), Some(true));
    let ev = j(&n, "/events?n=5");
    let ours = ev
        .as_array()
        .and_then(|a| a.iter().find(|e| e["source"] == "gaia-live").cloned());
    c.that("GET /events returns it", ours.is_some());
    if let Some(e) = ours {
        let ts = e["ts"].as_str().unwrap_or("");
        c.that(
            &format!("event ts is Zagreb ({ts})"),
            zagreb_time::require_rfc3339(ts).is_ok(),
        );
    }

    realrun::say("6. quiet 75 s: flood ends, verdict returns to normal");
    sleep_s(75);
    let api = stream(&n, "nerve-api");
    realrun::say(format!("nerve-api {api}"));
    c.eq(
        "nerve-api back to normal",
        api["level"].as_str(),
        Some("normal"),
    );
    c.that(
        "floods_total >= 1",
        api["floods_total"].as_u64().unwrap_or(0) >= 1,
    );
    let resolved = titles(&j(&n, "/api/v1/incidents")["resolved"]);
    c.that(
        "flood incident resolved",
        resolved.iter().any(|t| t.starts_with("request flood")),
    );
    let md = http::get(&n, "/api/v1/self-eval.md")
        .map(|r| r.text())
        .unwrap_or_default();
    realrun::say(md.lines().take(8).collect::<Vec<_>>().join("\n"));
    c.that(
        "self-eval.md rendered",
        md.starts_with("# openmind-nerve self-eval"),
    );
    c.that("nerve still alive after the flood", nerve.alive());
    realrun::say(format!("nerve log:\n{}", nerve.log_text()));
    nerve.kill();
    drop(_tuner2);
    drop(_room);
    let open = realrun::ports_closed(&[np, tp, rp], Duration::from_secs(3));
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_nerve_scenario");
}
