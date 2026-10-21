// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/desk-linkwatch.sh` (slot 18; deleted on
//! swarm/realscript-port-1). Real-life run for galion-desk + desk-hermes +
//! link-watch, wired together on the BOX (never the VPS):
//!
//! ```text
//!   +0 galion-desk (release)          desk   (live role :8646)
//!   +1 galion-one  (release)          one    (live role :8091)
//!   +2 hermes-gateway --mock          hermes (live role :8642, no provider calls)
//!   +3 galion-one mouth               voice  (live role :8002)
//!   +9 nothing listens                Ollama/laptop, tunnel, console, origin, room, tulu
//! ```
//!
//! link-watch (release, loop mode) probes +9 as Ollama and +2 as Hermes,
//! GETs a few public pages (`GALION_REAL_URLS`, default
//! `https://example.com/ https://one.galion.app/`; rate-limited to one GET
//! per URL per 60 s) and writes link.json into the desk's state dir.
//! galion-desk reads that link.json and runs the release desk-hermes against
//! a fixture Hermes home. Then the gated galion-desk real tests
//! (`real_wired`, `live_ports`) run against the stack, and everything is torn
//! down. Children get a cleared environment (`env_clear`), so no provider key
//! can leak into them.
//!
//! Ports: block 19180 (`GALION_REAL_PORT_BASE`), offsets above.
//! `GALION_REAL_LONG=1` also waits out the 60 s link-watch interval.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_desk_linkwatch -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use serde_json::{json as j_, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn base_env(c: &mut Command, run: &Path) {
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", run)
        .env("TZ", "Europe/Zagreb")
        .env("OPENMIND_CONFIG", run.join("no-openmind.toml"))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
}

fn probe(addr: &str, method: &str, path: &str, data: &str) -> (String, Value) {
    let t = Instant::now();
    let r = if method == "POST" {
        http::request(
            addr,
            "POST",
            path,
            &[("Content-Type", "application/x-www-form-urlencoded")],
            data.as_bytes(),
            Duration::from_secs(10),
        )
    } else {
        http::request(addr, "GET", path, &[], b"", Duration::from_secs(10))
    };
    match r {
        Ok(r) => (
            format!(
                "{} {} {}B {:.6}s",
                r.status,
                r.header("content-type").unwrap_or(""),
                r.body.len(),
                t.elapsed().as_secs_f64()
            ),
            r.json().unwrap_or(Value::Null),
        ),
        Err(e) => (format!("000 error {e}"), Value::Null),
    }
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| (k.to_string(), v[*k].clone()))
            .collect(),
    )
}

fn checked(link: &Path) -> String {
    let v = json(&read(link));
    Value::Array(
        v["urls"]
            .as_array()
            .map(|u| u.iter().map(|x| x["checked_unix"].clone()).collect())
            .unwrap_or_default(),
    )
    .to_string()
}

#[test]
fn real_sh_desk_linkwatch() {
    if !enabled("desk-linkwatch") {
        return;
    }
    let b = port_base(19180);
    let (desk_p, one_p, gw_p, mouth_p, closed_p) = (b, b + 1, b + 2, b + 3, b + 9);
    let urls = std::env::var("GALION_REAL_URLS")
        .unwrap_or_else(|_| "https://example.com/ https://one.galion.app/".into());
    build(&[
        "-p",
        "galion-desk",
        "-p",
        "desk-hermes",
        "-p",
        "link-watch",
        "-p",
        "hermes-gateway",
        "-p",
        "galion-one",
        "--bins",
        "--example",
        "fixture_home",
    ]);
    let rel = target_dir().join("release");
    for n in [
        "galion-desk",
        "desk-hermes",
        "link-watch",
        "hermes-gateway",
        "galion-one",
        "examples/fixture_home",
    ] {
        need_bin(&rel.join(n));
    }
    require_free(&[desk_p, one_p, gw_p, mouth_p, closed_p]);

    let dir = TempDir(scratch("real-desk-linkwatch"));
    let run_d = dir.0.clone();
    for d in ["state", "one", "gw-home", "galion-home/state", "logs"] {
        fs::create_dir_all(run_d.join(d)).unwrap();
    }
    let o = run({
        let mut c = Command::new(rel.join("examples/fixture_home"));
        c.arg(run_d.join("hermes"));
        c
    });
    assert!(o.status.success(), "fixture_home: {}", err(&o));
    fs::write(
        run_d.join("state/laptop.json"),
        r#"{"name": "Box test laptop", "gpu": "NVIDIA GeForce RTX 4060 Laptop GPU, 8188 MiB", "cpu": "13th Gen Intel(R) Core(TM) i7-13620H", "ram_gb": 32, "free_gb": 20}"#,
    )
    .unwrap();
    fs::write(
        run_d.join("hermes/gateway_state.json"),
        r#"{"active_agents": 2, "code_version": "box-real"}"#,
    )
    .unwrap();
    fs::write(run_d.join("one/voice"), "jarvis\n").unwrap();
    let closed = format!("127.0.0.1:{closed_p}");
    let log = |n: &str| fs::File::create(run_d.join("logs").join(n)).unwrap();
    let mut procs = Procs::new();

    say(&format!("start hermes-gateway --mock on {gw_p}"));
    // hermes-gateway refuses to start without an inbound API key. This mock
    // instance gets a throwaway random one that is never printed or stored.
    let gw_key = random_hex(24);
    let mut c = Command::new(rel.join("hermes-gateway"));
    base_env(&mut c, &run_d);
    c.arg("--mock")
        .env("HERMES_HOME", run_d.join("gw-home"))
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", gw_p.to_string())
        .env("HERMES_RS_MCP", "off")
        .env("HERMES_RS_CRON", "0")
        .env("API_SERVER_KEY", &gw_key)
        .stderr(log("hermes.log"));
    procs.spawn("hermes-gateway", c);

    say(&format!("start galion-one on {one_p} (mouth {mouth_p})"));
    let mut c = Command::new(rel.join("galion-one"));
    base_env(&mut c, &run_d);
    c.env("GALION_HOME", run_d.join("galion-home"))
        .env("ONE_NO_WARM", "1")
        .env("ONE_BIND", format!("127.0.0.1:{one_p}"))
        .env("ONE_MOUTH_BIND", format!("127.0.0.1:{mouth_p}"))
        .env("GALION_MIND", format!("127.0.0.1:{gw_p}"))
        .env("GALION_MOUTH", format!("127.0.0.1:{mouth_p}"))
        .env("GALION_EAR", &closed)
        .env("GALION_EYE", &closed)
        .stderr(log("one.log"));
    procs.spawn("galion-one", c);

    say(&format!("start galion-desk on {desk_p}"));
    let mut c = Command::new(rel.join("galion-desk"));
    base_env(&mut c, &run_d);
    c.env("DESK_BIND", format!("127.0.0.1:{desk_p}"))
        .env("DESK_STATE_DIR", run_d.join("state"))
        .env("DESK_ONE_DIR", run_d.join("one"))
        .env("DESK_HERMES_DIR", run_d.join("hermes"))
        .env("DESK_HERMES_BIN", rel.join("desk-hermes"))
        .env("HERMES_HOME", run_d.join("hermes"))
        .env("DESK_OLLAMA", &closed)
        .env("DESK_PROBE_HERMES", format!("127.0.0.1:{gw_p}"))
        .env("DESK_PROBE_ONE", format!("127.0.0.1:{mouth_p}"));
    for k in [
        "DESK_PROBE_TUNNEL",
        "DESK_PROBE_CONSOLE",
        "DESK_PROBE_ORIGIN",
        "DESK_PROBE_ROOM",
        "DESK_PROBE_TULU",
    ] {
        c.env(k, &closed);
    }
    c.stderr(log("desk.log"));
    procs.spawn("galion-desk", c);

    for p in [desk_p, one_p, gw_p, mouth_p] {
        let up = wait_until(Duration::from_secs(10), || {
            http::get(&format!("127.0.0.1:{p}"), "/health").is_ok()
        });
        assert!(
            up,
            "port {p} did not come up; logs:\n{}\n{}\n{}",
            tail(&run_d.join("logs/hermes.log"), 10),
            tail(&run_d.join("logs/one.log"), 10),
            tail(&run_d.join("logs/desk.log"), 10)
        );
    }

    let link = run_d.join("state/link.json");
    say(&format!(
        "start link-watch (loop) -> state/link.json; URLs: {urls}"
    ));
    let mut c = Command::new(rel.join("link-watch"));
    base_env(&mut c, &run_d);
    c.env("LINK_WATCH_OUT", &link)
        .env("LINK_WATCH_LAPTOP", run_d.join("state/laptop.json"))
        .env("LINK_WATCH_LOG", run_d.join("state/link-log.json"))
        .env("LINK_WATCH_OLLAMA_URL", format!("http://{closed}"))
        .env("LINK_WATCH_HERMES_URL", format!("http://127.0.0.1:{gw_p}"))
        .env("LINK_WATCH_URLS", &urls)
        .env("LINK_WATCH_URLS_EVERY", "60")
        .stderr(log("link-watch.log"));
    procs.spawn("link-watch", c);
    // link-watch fetches the public URLs on its first pass (up to ~20 s).
    wait_until(Duration::from_secs(30), || {
        json(&read(&link))["urls"]
            .as_array()
            .is_some_and(|u| !u.is_empty())
    });
    say("listeners:");
    let ss = out(&run({
        let mut c = Command::new("ss");
        c.arg("-ltnp");
        c
    }));
    for l in ss.lines() {
        if [desk_p, one_p, gw_p, mouth_p]
            .iter()
            .any(|p| l.contains(&format!(":{p} ")))
        {
            println!("  {l}");
        }
    }

    let desk = format!("127.0.0.1:{desk_p}");
    say("desk route matrix (GET):");
    for p in [
        "/",
        "/health",
        "/healthz",
        "/health/x",
        "/desk",
        "/desk/",
        "/deskx",
        "/desk?x=1",
        "/nope",
        "/DESK",
    ] {
        println!("  {p:<10} {}", probe(&desk, "GET", p, "").0);
    }
    say("one/hermes health:");
    for p in [one_p, gw_p, mouth_p] {
        let st = http::get(&format!("127.0.0.1:{p}"), "/health").map_or(0, |r| r.status);
        println!("  {p} {st}");
    }

    say("link.json from link-watch:");
    let lj = json(&read(&link));
    let urls_v: Vec<Value> = lj["urls"]
        .as_array()
        .map(|u| {
            u.iter()
                .map(|x| {
                    pick(
                        x,
                        &["url", "ok", "status", "ms", "checked_at_local", "error"],
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    println!(
        "  {}",
        j_!({"laptop_port": lj["laptop_port"], "model": lj["model"], "hermes": lj["hermes"], "hermes_ms": lj["hermes_ms"],
            "france": lj["france"], "laptop": lj["laptop"]["name"], "events": lj["events"].as_array().map_or(0, |e| e.len()),
            "urls": urls_v})
    );

    say("desk snapshot (selected fields):");
    let (_, d) = probe(&desk, "GET", "/desk", "");
    let services: Vec<String> = d["services"]
        .as_array()
        .map(|s| {
            s.iter()
                .map(|x| format!("{}={}", jt(x, "name"), jt(x, "up")))
                .collect()
        })
        .unwrap_or_default();
    let tasks: Vec<Value> = d["mindpack"]["tasks"]
        .as_array()
        .map(|t| t.iter().map(|x| x["id"].clone()).collect())
        .unwrap_or_default();
    let mut snap = pick(
        &d,
        &["foundation", "line", "hermes", "one", "tunnel", "voice"],
    );
    snap["services"] = j_!(services);
    snap["board"] = pick(
        &d["board"],
        &[
            "studio",
            "gpu",
            "cpu",
            "laptop_ram",
            "hermes_ms",
            "agents",
            "version",
            "france",
        ],
    );
    snap["mindpack"] = j_!({"sessions": d["mindpack"]["mind"]["sessions"], "messages": d["mindpack"]["mind"]["messages"],
        "line": d["mindpack"]["mind"]["line"], "road": d["mindpack"]["road"].as_array().map_or(0, |r| r.len()), "tasks": tasks});
    println!("  {snap}");

    say("desk acts (POST):");
    let acts: [(&str, &str, &str); 7] = [
        (
            "note",
            r#"{"act":"note","text":"real-life note from slot 18"}"#,
            "notes",
        ),
        ("stamp", r#"{"act":"stamp"}"#, "last-note"),
        (
            "focus",
            r#"{"act":"focus","text":"desk-linkwatch"}"#,
            "focus",
        ),
        ("hold", r#"{"act":"hold"}"#, "hold"),
        ("hold", r#"{"act":"hold"}"#, "hold"),
        ("drop", r#"{"act":"drop"}"#, "notes-left"),
        ("voice", r#"{"act":"voice","name":"samantha"}"#, "voice"),
    ];
    for (name, body, show) in acts {
        let (line, v) = probe(&desk, "POST", "/desk", body);
        println!("  {name:<6} {line}");
        let shown = match show {
            "last-note" => v["notes"]
                .as_array()
                .and_then(|n| n.last())
                .map_or(Value::Null, |n| n["text"].clone()),
            "notes-left" => j_!(v["notes"].as_array().map_or(0, |n| n.len())),
            k => v[k].clone(),
        };
        println!("    {show}: {shown}");
    }

    let mut ck = Checks::new();
    say("gated galion-desk real tests against the stack:");
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    let o = run({
        let mut c = Command::new("nice");
        c.args(["-n", "10"])
            .arg(cargo())
            .args([
                "test",
                "-q",
                "-j",
                &jobs,
                "-p",
                "galion-desk",
                "--test",
                "real_wired",
                "--test",
                "live_ports",
                "--",
                "--include-ignored",
            ])
            .current_dir(repo_root())
            .env("CARGO_INCREMENTAL", "0")
            .env("GALION_REAL", "1")
            .env("GALION_REAL_DESK", &desk)
            .env("GALION_REAL_STATE", run_d.join("state"))
            .env("GALION_DESK_LIVE_DESK_PORT", desk_p.to_string())
            .env("GALION_DESK_LIVE_ONE_PORT", one_p.to_string())
            .env("GALION_DESK_LIVE_REQUIRED", "1");
        c
    });
    let text = format!("{}{}", out(&o), err(&o));
    for l in text.lines().filter(|l| {
        l.contains("test result")
            || l.contains("panicked")
            || l.contains("FAILED")
            || l.contains("SKIP")
    }) {
        println!("  {l}");
    }
    let bad = text.contains("FAILED") || text.contains("panicked") || !text.contains("test result");
    ck.ok(
        "galion-desk real_wired + live_ports against the stack",
        o.status.success() && !bad,
        &format!("exit {}", code(&o)),
    );

    say("link-watch rate limit (LINK_WATCH_URLS_EVERY=60):");
    let t0 = checked(&link);
    std::thread::sleep(Duration::from_secs(9));
    let t1 = checked(&link);
    let at = jt(&json(&read(&link)), "at_local");
    println!("  checked_unix at start: {t0}; after 9 s (more loop passes, last at {at}): {t1}");
    ck.ok("public URLs fetched on the first pass", t0 != "[]", &t0);
    ck.eq("URLs not re-fetched inside the 60 s interval", &t0, &t1);
    if std::env::var("GALION_REAL_LONG").as_deref() == Ok("1") {
        std::thread::sleep(Duration::from_secs(55));
        let t2 = checked(&link);
        println!("  after 64 s: {t2}");
        ck.ok("URLs refreshed after the interval", t2 != t1, &t2);
    }
    say("process RSS:");
    for l in procs.rss() {
        println!("  {l}");
    }
    ck.ok(
        "test gateway key never in logs",
        !fs::read_dir(run_d.join("logs"))
            .unwrap()
            .flatten()
            .any(|e| read(&e.path()).contains(&gw_key)),
        "",
    );
    drop(procs);
    std::thread::sleep(Duration::from_millis(300));
    for p in [desk_p, one_p, gw_p, mouth_p] {
        ck.ok(
            &format!("port {p} closed after teardown"),
            !port_open(p),
            "",
        );
    }
    ck.finish("desk-linkwatch");
}
