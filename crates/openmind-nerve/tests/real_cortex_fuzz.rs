// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for cortex T5.2 (slot 185, ports 20550-20559), gated by
//! `GALION_REAL=1`; run with `--release -- --nocapture --test-threads 1`.
//!
//! 1. Real VPS data (only with `CORTEX_FUZZ_VPS=1`): READ-ONLY captures over
//!    ssh (`ss -ltn`, `/proc/net/tcp{,6}`, `journalctl -o json` for
//!    cloudflared/ssh/nerve, the cloudflared ingress config, GET nerve
//!    `/api/v1/state`) into a private temp dir. Every parser runs on them
//!    with cross-checks (ss vs /proc listeners, probe vs proc_owner rows,
//!    tunnel-lint over the real config + ss), then a time-boxed fuzz round
//!    uses them as seeds. The temp dir is removed (journal lines carry
//!    client IPs); only counts are printed.
//! 2. The real release `openmind-nerve serve` on 127.0.0.1:20550 with its
//!    own `/event` log fed back through `NERVE_EVENT_FEED_FILE`: fuzzed
//!    `POST /event` bodies and raw fuzzed HTTP requests (bad heads, huge
//!    lengths, binary) over TCP. The process must stay up, answer
//!    `/api/v1/state` with 200, keep cycling, write only valid JSONL, and
//!    the event-feed sensor must read its own log back with 0 bad lines.

mod fuzz;

use fuzz::targets::{self, gens, targets};
use fuzz::{mutate, run_gen, run_target, Budget, Rng};
use openmind_nerve::logscan;
use openmind_nerve::probe;
use openmind_nerve::sensors::{edgerate, proc_owner};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const NERVE: u16 = 20550;
const VPS_INGRESS: &str = "/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn now_z() -> String {
    zagreb_time::from_unix(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    )
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir(tag: &str) -> TempDir {
    let d = std::env::temp_dir().join(format!("cortex-t52-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tempdir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    TempDir(d)
}

/// One read-only command on the VPS; stdout or None.
fn vps(cmd: &str) -> Option<String> {
    // ssh alias (shared swarm convention), not a hard-coded address.
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let o = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            host.as_str(),
            cmd,
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn http(method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let raw = format!(
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    raw_http(raw.as_bytes())
}

/// Send raw bytes, half-close, read the reply. `Some((0, ""))` when the
/// server closed without a status line.
fn raw_http(bytes: &[u8]) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{NERVE}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = s.write_all(bytes);
    let _ = s.shutdown(Shutdown::Write);
    let mut b = Vec::new();
    let _ = s.read_to_end(&mut b);
    let b = String::from_utf8_lossy(&b).into_owned();
    let Some((head, body)) = b.split_once("\r\n\r\n") else {
        return Some((0, String::new()));
    };
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, body.to_string()))
}

fn state() -> Value {
    http("GET", "/api/v1/state", "")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn wait(ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state();
        if ok(&st) || t.elapsed() > Duration::from_secs(40) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| probe::parse_status_kb(&s, "VmRSS:"))
        .unwrap_or(0)
}

/// Part 1: real VPS captures through every parser, then seeded fuzzing.
fn vps_part(secs_per_target: u64) {
    let dir = tempdir("vps");
    let d = &dir.0;
    let t0 = now_z();
    let grab = |name: &str, cmd: &str| -> String {
        let s = vps(cmd).unwrap_or_default();
        std::fs::write(d.join(name), &s).expect("write capture");
        s
    };
    let ss = grab("ss-ltn.txt", "ss -ltn");
    let tcp = grab("proc-net-tcp.txt", "cat /proc/net/tcp");
    let tcp6 = grab("proc-net-tcp6.txt", "cat /proc/net/tcp6");
    let journal = grab(
        "journal.jsonl",
        "journalctl -o json --no-pager -q --output-fields=_SYSTEMD_UNIT,UNIT,PRIORITY,MESSAGE --since -2h -u cloudflared -u ssh -u galion-nerve | tail -n 20000",
    );
    let ingress = grab("ingress.yml", &format!("cat {VPS_INGRESS}"));
    let st = grab(
        "state.json",
        "curl -s -m 10 http://127.0.0.1:8670/api/v1/state",
    );
    let events = grab("events.jsonl", "cat /var/lib/galion-nerve/events.jsonl");
    eprintln!(
        "VPS captures {t0} .. {}: ss {} lines, tcp {} / tcp6 {} lines, journal {} lines, ingress {} lines, state {} bytes, events {} lines",
        now_z(),
        ss.lines().count(),
        tcp.lines().count(),
        tcp6.lines().count(),
        journal.lines().count(),
        ingress.lines().count(),
        st.len(),
        events.lines().count()
    );
    assert!(
        ss.contains("LISTEN") && tcp.lines().count() > 1,
        "VPS capture failed"
    );

    // ss vs /proc: same listening ports.
    let ls = probe::parse_ss(&ss);
    let listen_lines = ss.lines().filter(|l| l.starts_with("LISTEN")).count();
    let mut pl = probe::parse_proc_net_tcp(&tcp, false);
    pl.extend(probe::parse_proc_net_tcp(&tcp6, true));
    let ss_ports: BTreeSet<u16> = ls.iter().map(|l| l.port).collect();
    let proc_ports: BTreeSet<u16> = pl.iter().map(|l| l.port).collect();
    let own: Vec<_> = proc_owner::parse_proc_net_tcp(&tcp)
        .into_iter()
        .chain(proc_owner::parse_proc_net_tcp(&tcp6))
        .collect();
    let own_ports: BTreeSet<u16> = own.iter().map(|r| r.port).collect();
    let lo = ls.iter().filter(|l| l.loopback).count();
    eprintln!(
        "  ss: {listen_lines} LISTEN lines -> {} listeners ({lo} loopback), {} ports; /proc: {} listeners, {} ports; proc_owner {} rows; ss-only {:?} proc-only {:?}",
        ls.len(),
        ss_ports.len(),
        pl.len(),
        proc_ports.len(),
        own.len(),
        ss_ports.difference(&proc_ports).collect::<Vec<_>>(),
        proc_ports.difference(&ss_ports).collect::<Vec<_>>()
    );
    assert_eq!(ls.len(), listen_lines, "every real ss LISTEN line parses");
    assert_eq!(
        own_ports, proc_ports,
        "probe and proc_owner agree on real /proc"
    );
    // Captured a few seconds apart: allow churn of at most 2 ports.
    let churn = ss_ports.symmetric_difference(&proc_ports).count();
    assert!(churn <= 2, "ss vs /proc differ by {churn} ports");

    // Journal.
    let all = probe::parse_journal_lines(&journal, &[]);
    let units = ["cloudflared.service", "ssh.service", "galion-nerve.service"];
    let counts = probe::parse_journal(&journal, &units);
    let cf: Vec<&probe::JLine> = all
        .iter()
        .filter(|l| l.unit == "cloudflared.service")
        .collect();
    let (mut req, mut failed, mut dest, mut refused) = (0, 0, 0, 0);
    let mut hosts = BTreeSet::new();
    for l in &cf {
        match edgerate::parse(&l.msg) {
            Some((h, edgerate::EdgeKind::Request)) => {
                req += 1;
                hosts.insert(h);
            }
            Some((h, edgerate::EdgeKind::Failed)) => {
                failed += 1;
                hosts.insert(h);
            }
            None => {}
        }
        dest += logscan::dest_host(&l.msg).is_some() as u32;
        refused += logscan::refused_port(&l.msg).is_some() as u32;
    }
    let ins = logscan::insights(&all);
    let json_lines = journal.lines().filter(|l| l.starts_with('{')).count();
    eprintln!(
        "  journal: {json_lines} JSON lines -> {} records; per unit {:?}; cloudflared {}: edge requests {req}, failed {failed}, dest= {dest}, refused {refused}, {} hosts; insights dead origins {:?}, scans {}",
        all.len(),
        counts.iter().map(|(u, c)| (u.as_str(), (c.lines, c.errors, c.warnings))).collect::<BTreeMap<_, _>>(),
        cf.len(),
        hosts.len(),
        ins.dead_origins.keys().collect::<Vec<_>>(),
        ins.scans.len()
    );
    assert!(all.len() <= json_lines);
    assert!(
        json_lines - all.len() <= json_lines / 100 + 1,
        "almost every real record parses"
    );

    // Ingress config + ss through tunnel-lint.
    let rules = tunnel_lint::parse(&ingress).expect("real ingress parses");
    let rep = tunnel_lint::lint(&rules, &tunnel_lint::SsSnapshot::parse(&ss));
    let items = ingress
        .lines()
        .filter(|l| l.trim_start().starts_with("- ") && !l.trim_start().starts_with("- team"))
        .count();
    eprintln!(
        "  ingress: {} rules ({items} list items), closed ports {:?}, errors {}",
        rules.len(),
        rep.closed_ports(),
        rep.has_errors()
    );
    // Live nerve view, for comparison.
    if let Ok(v) = serde_json::from_str::<Value>(&st) {
        eprintln!(
            "  live nerve: cycle {} score {} grade {} incidents {}",
            v["cycle"],
            v["system"]["score"],
            v["system"]["grade"],
            v["incidents"].as_array().map_or(0, Vec::len)
        );
    }
    // Real event log through the feed parser.
    let mut f = openmind_nerve::sensors::event_feed::EventFeed::new(vec![], 3600, None);
    for l in events.lines() {
        f.ingest_line(l, targets::NOW + 86_400 * 365);
    }
    eprintln!("  live events.jsonl: {:?}", f.stats);
    assert_eq!(f.stats.bad, 0);

    // Seeded fuzz round over the real captures.
    std::env::set_var("CORTEX_FUZZ_SEEDS", d);
    std::env::set_var("CORTEX_FUZZ_CRASH_DIR", d.join("crashes"));
    let mut fails = 0;
    for t in targets() {
        let r = run_target(&t, Budget::Secs(secs_per_target), fuzz::seed());
        eprintln!("  {}", r.line());
        for (k, m, _) in &r.failures {
            // Inputs stay in the temp dir (may carry real IPs); message only.
            eprintln!("    {k}: {}", m.chars().take(200).collect::<String>());
        }
        fails += !r.ok() as u32;
    }
    std::env::remove_var("CORTEX_FUZZ_SEEDS");
    assert_eq!(fails, 0, "seeded fuzz over real VPS data found failures");
}

/// Part 2: the real server under fuzzed requests.
fn server_part(n_events: usize, n_raw: usize) {
    let dir = tempdir("srv");
    let d = &dir.0;
    let log = d.join("events.jsonl");
    let comps = d.join("components.json");
    std::fs::write(&comps, "[]").expect("components.json");
    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .arg("--log")
            .arg(&log)
            .env("NERVE_ADDR", format!("127.0.0.1:{NERVE}"))
            .env("NERVE_INTERVAL", "3")
            .env("NERVE_DATA_DIR", d.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "20550-20559")
            .env("NERVE_EVENT_FEED_FILE", &log)
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_EVENT_FEED_SOURCES")
            .stderr(
                std::fs::File::create(d.join("nerve.err"))
                    .map(Stdio::from)
                    .unwrap_or(Stdio::null()),
            )
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let pid = nerve.0.id();
    let st = wait(|s| s["cycle"].as_u64().unwrap_or(0) >= 1);
    let c0 = st["cycle"].as_u64().unwrap_or(0);
    assert!(
        c0 >= 1,
        "nerve did not start on :{NERVE}: {:?} stderr: {}",
        http("GET", "/api/v1/state", "").map(|(c, b)| (c, b.chars().take(200).collect::<String>())),
        std::fs::read_to_string(d.join("nerve.err")).unwrap_or_default()
    );
    eprintln!(
        "serve :{NERVE} pid {pid} up at {} cycle {c0} rss {} KiB",
        now_z(),
        rss_kib(pid)
    );

    let ev = targets::target_event();
    let hr = targets::target_http();
    let mut rng = Rng::new(fuzz::seed() ^ 0x2055_0000);
    let mut codes: BTreeMap<u16, u64> = BTreeMap::new();
    let mut conn_fail = 0u64;
    let t = Instant::now();
    // a) POST /event with a fuzzed JSON body (one record per request).
    let bodies: Vec<String> = [
        r#"{"source":"gaia","kind":"service.probe","severity":"warn","data":{"service":"t185","state":"down","detail":"connect 127.0.0.1:20551 refused"}}"#,
        r#"{"source":"gaia","kind":"gaia.heal.capped","severity":"error","data":{"service":"t185","restarts_last_hour":3,"cap":3}}"#,
        r#"{"source":"survival","kind":"probe","severity":"warn","data":{"name":"disk","ok":false,"detail":"/ 97%"}}"#,
        r#"{"source":"event_cortex","kind":"cortex.finding","severity":"critical","data":{"top":{"id":"fz-1","component":"edge","title":"10.1.2.3:443 x"}}}"#,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for i in 0..n_events {
        let base = &bodies[i % bodies.len()];
        let body = if i % 5 == 0 {
            base.clone()
        } else {
            mutate(&mut rng, base, &ev.dict, &bodies)
        };
        match http("POST", "/event", &body) {
            Some((c, _)) => *codes.entry(c).or_default() += 1,
            None => conn_fail += 1,
        }
    }
    let ev_codes = codes.clone();
    // b) raw fuzzed HTTP (heads, lengths, binary).
    codes.clear();
    for i in 0..n_raw {
        let base = &hr.seeds[i % hr.seeds.len()];
        let mut b = mutate(&mut rng, base, &hr.dict, &hr.seeds).into_bytes();
        if i % 17 == 0 {
            b.extend((0..rng.below(4096)).map(|_| rng.next() as u8));
        }
        match raw_http(&b) {
            Some((c, _)) => *codes.entry(c).or_default() += 1,
            None => conn_fail += 1,
        }
    }
    let raw_codes = codes;
    let took = t.elapsed();
    eprintln!(
        "fuzzed POST /event x{n_events}: codes {ev_codes:?}; raw HTTP x{n_raw}: codes {raw_codes:?} (0 = closed without status); connect failures {conn_fail}; {:.1}s",
        took.as_secs_f64()
    );
    assert_eq!(conn_fail, 0, "server stopped accepting connections");
    assert!(
        ev_codes.keys().all(|c| matches!(c, 200 | 400 | 413)),
        "{ev_codes:?}"
    );
    assert!(
        raw_codes
            .keys()
            .all(|c| matches!(c, 0 | 200 | 400 | 404 | 405 | 413 | 429 | 431 | 503)),
        "{raw_codes:?}"
    );

    // Still alive, still cycling, state still JSON.
    let st = wait(|s| s["cycle"].as_u64().unwrap_or(0) >= c0 + 2);
    let c1 = st["cycle"].as_u64().unwrap_or(0);
    assert!(c1 >= c0 + 2, "nerve stopped cycling ({c0} -> {c1})");
    let ok = http("GET", "/api/v1/state", "").map(|x| x.0);
    assert_eq!(ok, Some(200));
    // The log holds only valid JSON lines and reads back with 0 bad.
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let lines = text.lines().count();
    let bad = text
        .lines()
        .filter(|l| serde_json::from_str::<Value>(l).is_err())
        .count();
    let mut f = openmind_nerve::sensors::event_feed::EventFeed::new(vec![], 3600, None);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for l in text.lines() {
        f.ingest_line(l, now);
    }
    let ids: Vec<String> = ["vulnerabilities", "bleeding"]
        .iter()
        .flat_map(|k| st["cortex"][k].as_array().cloned().unwrap_or_default())
        .filter(|x| {
            x["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("event:"))
        })
        .filter_map(|x| x["id"].as_str().map(String::from))
        .collect();
    eprintln!(
        "after: cycle {c0} -> {c1}, /api/v1/state 200, rss {} KiB, cycle_ms {}; log {lines} lines ({bad} invalid); feed stats {:?}; event findings {} (e.g. {:?})",
        rss_kib(pid),
        st["cycle_ms"],
        f.stats,
        ids.len(),
        ids.iter().take(4).collect::<Vec<_>>()
    );
    assert_eq!(bad, 0, "event log has invalid JSON lines");
    assert_eq!(f.stats.bad, 0);
    // One line per accepted POST /event: the fuzzed-body ones plus raw
    // requests that still were a valid POST /event (counted among raw 200s).
    let ev_ok = ev_codes.get(&200).copied().unwrap_or(0);
    let raw_ok = raw_codes.get(&200).copied().unwrap_or(0);
    assert!(
        (ev_ok..=ev_ok + raw_ok).contains(&(lines as u64)),
        "log lines {lines} vs accepted {ev_ok} (+ raw 200s {raw_ok})"
    );
    assert!(
        !ids.is_empty(),
        "event-feed sensor saw the accepted records"
    );
    assert!(
        ids.iter().all(|i| i
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:".contains(c))),
        "clean ids"
    );
    drop(nerve);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists() || rss_kib(pid) == 0,
        "nerve killed"
    );
}

#[test]
fn real_cortex_fuzz() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    eprintln!("real_cortex_fuzz start {}", now_z());
    let secs = std::env::var("CORTEX_FUZZ_REAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    if std::env::var("CORTEX_FUZZ_VPS").as_deref() == Ok("1") {
        vps_part(secs);
    } else {
        eprintln!("VPS part skipped (set CORTEX_FUZZ_VPS=1 for read-only VPS captures)");
    }
    // Generators once more on the release build.
    for g in gens() {
        let r = run_gen(&g, Budget::Iters(2000), fuzz::seed());
        eprintln!("{}", r.line());
        assert!(r.ok(), "{:?}", r.failures);
    }
    let n_ev = std::env::var("CORTEX_FUZZ_REAL_EVENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    server_part(n_ev, n_ev / 2);
    eprintln!("real_cortex_fuzz done {}", now_z());
}
