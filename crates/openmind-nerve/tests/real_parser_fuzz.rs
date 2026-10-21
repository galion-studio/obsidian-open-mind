// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for nerve T3.1 (wave-4 slot 212, ports 20820-20829),
//! Rust only, gated by `GALION_REAL=1`:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve --test real_parser_fuzz -- --nocapture --test-threads 1
//! GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_parser_fuzz vps_ -- --nocapture
//! ```
//!
//! `box_binary_under_fuzz`: starts the real `openmind-nerve serve` (release
//! under `--release`) on 127.0.0.1:20820 (dummy component on 20821, dry-run,
//! no systemd/journal, event log in a temp dir) and sends it fuzzed
//! `POST /signal` and `POST /event` bodies, mutated raw HTTP heads, deep JSON,
//! and the 8192-byte head boundary in one write and in 1-byte segments.
//! Every request must get a well-formed HTTP answer with an expected code,
//! and the process must still be alive and cycling at the end.
//!
//! `vps_live_parsers`: over ssh (`GALION_VPS_SSH`, default `slavik-vps`),
//! READ-ONLY: GETs on the live nerve (127.0.0.1:8670) and cloudflared's
//! metrics port, and `journalctl -o json` reads of cloudflared / ssh /
//! galion-nerve; every captured line goes through the same fuzz-target
//! checks as the box runs. Nothing is written on the VPS; only counts are
//! printed (no log text).

mod fuzz;

use fuzz::nerve::{self, targets};
use fuzz::{mutate, Rng};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE: u16 = 20820;
const DUMMY: u16 = 20821;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn addr(p: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], p))
}

/// Send raw bytes (optionally in `seg`-byte writes), return the status code
/// of the answer, or None when no well-formed status line came back.
fn send(raw: &[u8], seg: Option<usize>) -> (Option<u16>, String) {
    let Ok(mut s) = TcpStream::connect_timeout(&addr(NERVE), Duration::from_secs(2)) else {
        return (None, String::new());
    };
    let _ = s.set_nodelay(true);
    let _ = s.set_read_timeout(Some(Duration::from_secs(8)));
    match seg {
        None => {
            let _ = s.write_all(raw);
        }
        Some(n) => {
            for c in raw.chunks(n) {
                if s.write_all(c).is_err() {
                    break;
                }
            }
        }
    }
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let code = text
        .strip_prefix("HTTP/1.1 ")
        .and_then(|r| r.get(..3))
        .and_then(|c| c.parse().ok());
    let body = text
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, body)
}

fn post(path: &str, body: &str) -> (Option<u16>, String) {
    let raw = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    send(raw.as_bytes(), None)
}

fn get(path: &str) -> (Option<u16>, String) {
    send(
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes(),
        None,
    )
}

fn dummy() {
    let l = TcpListener::bind(addr(DUMMY)).expect("dummy port 20821 free");
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut s) = c else { continue };
            let mut b = [0u8; 2048];
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}");
        }
    });
}

fn start(dir: &Path) -> Proc {
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"t31-dummy","port":{DUMMY},"path":"/health","gaia":"DEMETER","unit":"t31-dummy.service","restarts_per_hour":1}}]"#),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    c.arg("serve")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "Europe/Zagreb")
        .env("NERVE_ADDR", format!("127.0.0.1:{NERVE}"))
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_GAIA_BIN", "/bin/false")
        .env("NERVE_DEPLOY_BIN", "/bin/false")
        .env("NERVE_DEPLOY_BACKUPS", dir.join("backups"))
        .env("NERVE_SWITCH_DIR", dir.join("switch"))
        .env("NERVE_TELEGRAM_ENV", dir.join("absent.env"))
        .env("NERVE_VOICE_UNIT", "t31-none.service")
        .env("NERVE_DISK_PATH", dir)
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_EVENT_LOG", dir.join("events.jsonl"))
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(dir.join("nerve.log")).unwrap(),
        ));
    let p = Proc(c.spawn().expect("start openmind-nerve"));
    let t = Instant::now();
    while get("/healthz").0 != Some(200) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "nerve did not come up"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    p
}

fn cycle() -> u64 {
    serde_json::from_str::<serde_json::Value>(&get("/state").1)
        .ok()
        .and_then(|v| v["cycle"].as_u64())
        .unwrap_or(0)
}

#[derive(Default)]
struct Tally(std::collections::BTreeMap<String, u64>);
impl Tally {
    fn add(&mut self, code: Option<u16>) {
        *self
            .0
            .entry(code.map(|c| c.to_string()).unwrap_or("none".into()))
            .or_default() += 1;
    }
}

#[test]
fn box_binary_under_fuzz() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-t31-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dummy();
    let mut p = start(&dir);
    println!(
        "== box {} nerve :{NERVE} pid {}",
        zagreb_time::now_rfc3339(),
        p.0.id()
    );
    let c0 = cycle();
    let ts = targets();
    let n: usize = std::env::var("NERVE_REAL_FUZZ_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let mut rng = Rng::new(0x212);
    let t0 = Instant::now();
    for (name, path, ok) in [
        ("signal-body", "/signal", &[202u16, 400, 429][..]),
        ("event-body", "/event", &[200, 400][..]),
    ] {
        let t = ts.iter().find(|t| t.name == name).unwrap();
        let mut tally = Tally::default();
        for i in 0..n {
            let base = &t.seeds[i % t.seeds.len()];
            let body = if i < t.seeds.len() {
                base.clone()
            } else {
                mutate(&mut rng, base, &t.dict, &t.seeds)
            };
            let (code, _) = post(path, &body);
            tally.add(code);
            assert!(
                code.is_some_and(|c| ok.contains(&c) || c == 413 || c == 431 || c == 503),
                "{path}: unexpected answer {code:?}"
            );
        }
        println!("{name:<14} {n} POST {path}: codes {:?}", tally.0);
    }
    let t = ts.iter().find(|t| t.name == "http-read-req").unwrap();
    let mut tally = Tally::default();
    for i in 0..n {
        let base = &t.seeds[i % t.seeds.len()];
        let raw = mutate(&mut rng, base, &t.dict, &t.seeds);
        let seg = if i % 10 == 0 {
            Some(1 + rng.below(64))
        } else {
            None
        };
        let (code, _) = send(raw.as_bytes(), seg);
        tally.add(code);
        assert!(
            code.is_some_and(|c| [200, 202, 400, 404, 405, 413, 429, 431, 503].contains(&c)),
            "raw: unexpected answer {code:?}"
        );
    }
    println!("http-raw       {n} mutated requests: codes {:?}", tally.0);
    // Deep JSON at and past serde_json's nesting limit.
    for depth in [127, 129, 1000, 4000] {
        for obj in [false, true] {
            let body = nerve::deep_json(depth, obj);
            if body.len() > 8192 {
                continue;
            }
            let (a, _) = post("/signal", &body);
            let (b, _) = post("/event", &body);
            println!("deep depth {depth:>4} obj {obj:<5}: /signal {a:?} /event {b:?}");
            assert!(
                a.is_some() && b.is_some(),
                "deep JSON {depth} got no answer"
            );
        }
    }
    // The head-limit boundary, one write vs 1-byte segments.
    for total in [8192usize, 8193, 9000] {
        let raw = nerve::big_head(total);
        let one = send(raw.as_bytes(), None).0;
        let seg = send(raw.as_bytes(), Some(1)).0;
        let seg7 = send(raw.as_bytes(), Some(700)).0;
        println!("head {total} B: one write {one:?}, 1-byte segments {seg:?}, 700-byte segments {seg7:?}");
        assert_eq!(one, seg, "head {total}: outcome depends on segmentation");
        assert_eq!(one, seg7, "head {total}: outcome depends on segmentation");
        // `{}` reaches the /signal parser (400 missing source) when the head fits.
        assert_eq!(one, Some(if total <= 8192 { 400 } else { 431 }));
    }
    println!("fuzz phase {:.1} s", t0.elapsed().as_secs_f64());
    // Still alive, still serving, still cycling.
    assert!(
        p.0.try_wait().unwrap().is_none(),
        "nerve exited during fuzz"
    );
    let t = Instant::now();
    while cycle() <= c0 {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "no new cycle after fuzz"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(get("/healthz").0, Some(200));
    let sig: serde_json::Value = serde_json::from_str(&get("/signals").1).unwrap();
    let ev = get("/events?n=1000");
    let evs: serde_json::Value = serde_json::from_str(&ev.1).expect("/events JSON");
    let log = std::fs::read_to_string(dir.join("nerve.log")).unwrap_or_default();
    println!(
        "after: pid {} alive, cycle {c0} -> {}, /signals {} B, /events {} records, stderr panics {}",
        p.0.id(),
        cycle(),
        sig.to_string().len(),
        evs.as_array().map(|a| a.len()).unwrap_or(0),
        log.matches("panicked").count()
    );
    assert_eq!(log.matches("panicked").count(), 0);
    drop(p);
    let _ = std::fs::remove_dir_all(&dir);
}

const VPS_SCRIPT: &str = r#"set -u
echo '@@healthz'; curl -s -m 5 -o /dev/null -w '%{http_code}' http://127.0.0.1:8670/healthz; echo
echo '@@state'; curl -s -m 5 http://127.0.0.1:8670/state | head -c 2000000; echo
echo '@@metrics'; curl -s -m 5 http://127.0.0.1:20241/metrics | grep '^cloudflared_tunnel_\|^# ' | head -400
echo '@@cflog'; journalctl -u cloudflared.service -o cat -n 400 --no-pager 2>/dev/null
echo '@@journal'; journalctl -u cloudflared.service -u ssh.service -u galion-nerve.service -o json -n 1500 --no-pager 2>/dev/null
echo '@@end'"#;

fn section<'a>(out: &'a str, k: &str) -> &'a str {
    let tag = format!("@@{k}\n");
    let Some(i) = out.find(&tag) else { return "" };
    let rest = &out[i + tag.len()..];
    &rest[..rest.find("\n@@").map(|j| j + 1).unwrap_or(rest.len())]
}

#[test]
fn vps_live_parsers() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &host,
            VPS_SCRIPT,
        ])
        .output()
        .expect("ssh");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    println!(
        "== VPS {} read-only (GET + journalctl reads)",
        zagreb_time::now_rfc3339()
    );
    println!("live nerve /healthz: {}", section(&out, "healthz").trim());
    let st: serde_json::Value =
        serde_json::from_str(section(&out, "state").trim()).expect("live /state JSON");
    println!(
        "live nerve /state: cycle {} mode {} system {} score {}",
        st["cycle"], st["mode"], st["system"]["status"], st["system"]["score"]
    );
    let ts = targets();
    let run = |name: &str, text: &str| {
        let t = ts.iter().find(|t| t.name == name).unwrap();
        let r = (t.check)(text);
        println!(
            "{name:<14} live input {} lines -> {:?}",
            text.lines().count(),
            r.as_ref().map(|_| "ok")
        );
        assert!(r.is_ok(), "{name} on live data: {r:?}");
    };
    let metrics = section(&out, "metrics");
    run("cf-metrics", metrics);
    let m = openmind_nerve::probe::parse_cloudflared_metrics(metrics);
    println!(
        "live cloudflared metrics: ha_connections {:?} total_requests {:?} request_errors {:?}",
        m.ha_connections, m.total_requests, m.request_errors
    );
    let cflog = section(&out, "cflog");
    run("cf-log-lines", cflog);
    let journal = section(&out, "journal");
    for chunk in nerve::chunks(journal, 200) {
        run("journald-json", &chunk);
    }
    let jl = openmind_nerve::probe::parse_journal_lines(journal, &[]);
    let ins = openmind_nerve::logscan::insights(&jl);
    println!(
        "live journal: {} records, dead origins {:?}, edge failures {}, scan hits {}",
        jl.len(),
        ins.dead_origins.keys().collect::<Vec<_>>(),
        ins.edge_failures.len(),
        ins.scans.len()
    );
    // Live state shapes through the parsers the nerve exposes.
    for c in st["components"].as_array().into_iter().flatten() {
        if let Some(t) = c.get("tunnel").filter(|t| !t.is_null()) {
            println!("live /state tunnel {}: {t}", c["name"]);
        }
    }
}
