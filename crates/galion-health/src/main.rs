// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
// Source: base.git ws/foundation e43d7ab crates/galion-health/src/main.rs (landed onto main 13a296c by swarm/health-land).
// v0.2.0 (swarm/health-fixes): fixes health-land findings 1 (split-request reads) and 2 (one thread
// served every request and ran all probes on the request path). The proven v0.1.0 binary stays on
// swarm/health-land f4a66ea; see docs/health-fixes.md.
//! galion-health: std-only loopback status page.
//! Env: GALION_HEALTH_ADDR (default 127.0.0.1:8660), GALION_HEALTH_CHECKS (default list below),
//! GALION_LOG_FORMAT=json|kv (default kv), GALION_HEALTH_WORKERS (1..=6, default 3).
//! Never reads secrets; probes only GET <path> on loopback.
//! Check spec: name|unit|port|path  (unit "-" = no systemd unit, port 0 = no probe), ';'-separated.
//! Task budget (the unit sets TasksMax=8): main + workers + 1 refresher + at most 1 systemctl child.
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_CHECKS: &str = "openmind-memory|openmind-memory.service|8420|/health;\
hermes-rs|hermes-rs.service|8642|/health;\
link-watch-rs|link-watch-rs.service|0|-;\
galion-desk|galion-desk.service|8646|/health;\
tuner|tuner.service|8650|/health;\
tuner-quick|tuner-quick.service|0|-;\
one|-|8091|/health;\
console|console.service|8080|/;\
cloudflared|cloudflared.service|20241|/ready;\
journald|systemd-journald.service|0|-;\
ssh|ssh.service|22|-;\
galion-backup.timer|galion-backup.timer|0|-";

/// Whole request head must arrive within this time (v0.1.0 used the same 3 s, per read()).
const HEAD_DEADLINE: Duration = Duration::from_secs(3);
/// Request heads larger than this get 431.
const MAX_HEAD: usize = 8192;
/// Results are reused for this long so page reloads never fan out into probe storms (same as v0.1.0).
const CACHE_TTL: Duration = Duration::from_secs(5);
/// With older results in hand, a request waits at most this long for a refresh, then serves the old ones.
const STALE_WAIT: Duration = Duration::from_secs(4);
/// With no results yet (first request after start), a request waits at most this long.
const COLD_WAIT: Duration = Duration::from_secs(10);
/// Accepted connections waiting for a free worker; beyond this the server answers 503 at once.
const QUEUE: usize = 32;
/// systemctl is-active must answer within this time, else every unit reads "unknown".
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_CONNECT: Duration = Duration::from_millis(1500);
const PROBE_WRITE: Duration = Duration::from_millis(1500);
const PROBE_READ: Duration = Duration::from_millis(2500);
/// Small thread stacks keep the service far below MemoryMax=16M.
const STACK: usize = 256 * 1024;

struct Check {
    name: String,
    unit: String,
    port: u16,
    path: String,
}
#[derive(Clone)]
struct Res {
    name: String,
    unit: String,
    unit_state: String,
    port: u16,
    http: String,
    ms: u128,
    ok: bool,
}

/// One finished probe round: results, checked_at stamp, failing names.
struct Snap {
    rs: Vec<Res>,
    at: String,
    bad: Vec<String>,
}

#[derive(Default)]
struct State {
    last: Option<Instant>,
    snap: Option<Arc<Snap>>,
    refreshing: bool,
    requested: bool,
}

struct Shared {
    st: Mutex<State>,
    cv: Condvar,
    checks: Vec<Check>,
}

fn log(level: &str, msg: &str) {
    let ts = zagreb_time::now_rfc3339();
    if std::env::var("GALION_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false)
    {
        eprintln!(
            "{{\"ts\":\"{}\",\"level\":\"{}\",\"svc\":\"galion-health\",\"msg\":\"{}\"}}",
            ts,
            level,
            esc(msg)
        );
    } else {
        eprintln!(
            "ts={} level={} svc=galion-health msg=\"{}\"",
            ts,
            level,
            msg.replace('"', "'")
        );
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '<' => o.push_str("\\u003c"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse(spec: &str) -> Vec<Check> {
    spec.split(';')
        .filter_map(|e| {
            let p: Vec<&str> = e.trim().split('|').collect();
            if p.len() != 4 {
                return None;
            }
            Some(Check {
                name: p[0].into(),
                unit: p[1].into(),
                port: p[2].parse().unwrap_or(0),
                path: p[3].into(),
            })
        })
        .collect()
}

/// Runs `systemctl is-active <units...>` once with a timeout; returns stdout or None.
fn systemctl_is_active(units: &[&str]) -> Option<String> {
    let mut child = Command::new("systemctl")
        .arg("is-active")
        .args(units)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let t = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t.elapsed() < SYSTEMCTL_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut s = String::new();
    let _ = out.read_to_string(&mut s);
    Some(s)
}

/// Unit states for every check, with one systemctl process for all units (one task, not twelve).
/// `systemctl is-active a b c` prints one state per unit, in order. If the line count does not
/// match (old systemd, error), it falls back to one call per unit, which is what v0.1.0 did.
fn unit_states(checks: &[Check]) -> Vec<String> {
    let units: Vec<&str> = checks
        .iter()
        .filter(|c| c.unit != "-")
        .map(|c| c.unit.as_str())
        .collect();
    let mut states: Vec<String> = Vec::new();
    if !units.is_empty() {
        let joint = systemctl_is_active(&units);
        let lines: Vec<String> = joint
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(|l| l.trim().to_string())
            .collect();
        if lines.len() == units.len() {
            states = lines;
        } else if joint
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            // No systemd (box, container): v0.1.0 printed "" per unit; keep that (finding 3, not in scope).
            states = vec![String::new(); units.len()];
        } else {
            states = units
                .iter()
                .map(|u| {
                    systemctl_is_active(&[u])
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "unknown".into())
                })
                .collect();
        }
    }
    let mut it = states.into_iter();
    checks
        .iter()
        .map(|c| {
            if c.unit == "-" {
                "n/a".into()
            } else {
                it.next().unwrap_or_else(|| "unknown".into())
            }
        })
        .collect()
}

/// Status code from the first bytes of an HTTP reply ("HTTP/1.1 200 OK" -> "200").
fn status_code(first: &[u8]) -> String {
    String::from_utf8_lossy(first)
        .split_whitespace()
        .nth(1)
        .unwrap_or("no-reply")
        .to_string()
}

struct Pending {
    idx: usize,
    s: TcpStream,
    t: Instant,
    /// When the request was sent; the read timeout counts from here, as in v0.1.0.
    rt: Instant,
    buf: Vec<u8>,
}

/// Probes every check with a port, all at once from this one thread: connect + send, then
/// read the replies round-robin on non-blocking sockets until each has its status line, closed,
/// or hit PROBE_READ. Worst case is about one PROBE_READ, not the sum of all checks.
fn probe_all(checks: &[Check]) -> Vec<(String, u128)> {
    let mut out: Vec<(String, u128)> = vec![("-".into(), 0); checks.len()];
    let mut pending: Vec<Pending> = Vec::new();
    for (idx, c) in checks.iter().enumerate() {
        if c.port == 0 {
            continue;
        }
        let t = Instant::now();
        let addr: SocketAddr = ([127, 0, 0, 1], c.port).into();
        let mut s = match TcpStream::connect_timeout(&addr, PROBE_CONNECT) {
            Ok(s) => s,
            Err(_) => {
                out[idx] = ("down".into(), t.elapsed().as_millis());
                continue;
            }
        };
        if c.path == "-" {
            out[idx] = ("tcp-open".into(), t.elapsed().as_millis());
            continue;
        }
        let _ = s.set_write_timeout(Some(PROBE_WRITE));
        let req = format!(
            "GET {} HTTP/1.0\r\nHost: 127.0.0.1\r\nUser-Agent: galion-health\r\nConnection: close\r\n\r\n",
            c.path
        );
        if s.write_all(req.as_bytes()).is_err() || s.set_nonblocking(true).is_err() {
            out[idx] = ("write-err".into(), t.elapsed().as_millis());
            continue;
        }
        pending.push(Pending {
            idx,
            s,
            t,
            rt: Instant::now(),
            buf: Vec::with_capacity(64),
        });
    }
    while !pending.is_empty() {
        let mut i = 0;
        while i < pending.len() {
            let p = &mut pending[i];
            let mut chunk = [0u8; 64];
            let room = 64 - p.buf.len();
            let done = match p.s.read(&mut chunk[..room]) {
                Ok(0) => true,
                Ok(n) => {
                    p.buf.extend_from_slice(&chunk[..n]);
                    p.buf.len() >= 64 || p.buf.contains(&b'\n')
                }
                Err(e)
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted =>
                {
                    p.rt.elapsed() >= PROBE_READ
                }
                Err(_) => true,
            };
            if done {
                let p = pending.swap_remove(i);
                out[p.idx] = (status_code(&p.buf), p.t.elapsed().as_millis());
            } else {
                i += 1;
            }
        }
        if !pending.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    out
}

fn run(checks: &[Check]) -> Vec<Res> {
    let states = unit_states(checks);
    let probes = probe_all(checks);
    checks
        .iter()
        .zip(states)
        .zip(probes)
        .map(|((c, us), (http, ms))| {
            let unit_ok = c.unit == "-" || us == "active";
            let http_ok =
                c.port == 0 || http == "tcp-open" || http.starts_with('2') || http.starts_with('3');
            Res {
                name: c.name.clone(),
                unit: c.unit.clone(),
                unit_state: us,
                port: c.port,
                http,
                ms,
                ok: unit_ok && http_ok,
            }
        })
        .collect()
}

fn to_json(rs: &[Res], at: &str) -> String {
    let ok = rs.iter().all(|r| r.ok);
    let items: Vec<String> = rs.iter().map(|r| format!(
        "{{\"name\":\"{}\",\"unit\":\"{}\",\"unit_state\":\"{}\",\"port\":{},\"http\":\"{}\",\"ms\":{},\"ok\":{}}}",
        esc(&r.name), esc(&r.unit), esc(&r.unit_state), r.port, esc(&r.http), r.ms, r.ok)).collect();
    format!(
        "{{\"ok\":{},\"checked_at\":\"{}\",\"services\":[{}]}}",
        ok,
        at,
        items.join(",")
    )
}

fn to_html(rs: &[Res], at: &str) -> String {
    let mut rows = String::new();
    for r in rs {
        rows.push_str(&format!("<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{} ms</td></tr>",
            if r.ok { "ok" } else { "bad" }, html(&r.name), html(&r.unit), html(&r.unit_state),
            if r.port == 0 { "-".into() } else { r.port.to_string() }, html(&r.http), r.ms));
    }
    let up = rs.iter().filter(|r| r.ok).count();
    format!("<!doctype html><html><head><meta charset=utf-8><meta http-equiv=refresh content=15><title>galion-health</title>\
<style>body{{font:14px system-ui;background:#111;color:#ddd;margin:2em}}td,th{{padding:4px 10px;text-align:left}}\
tr.ok td:first-child{{color:#4c4}}tr.bad td{{color:#f55}}</style></head><body><h2>Galion health: {up}/{} ok</h2>\
<p>checked {at} (Europe/Zagreb) &middot; <a href=/status.json style=color:#8af>json</a></p>\
<table><tr><th>service</th><th>unit</th><th>state</th><th>port</th><th>http</th><th>latency</th></tr>{rows}</table></body></html>", rs.len())
}

#[derive(Debug, PartialEq)]
enum Head {
    /// Request path from a complete request line.
    Path(String),
    /// Peer connected and closed without sending anything (port scanners, TCP checks).
    Empty,
    /// Nothing arrived before the deadline.
    Timeout,
    /// Bytes arrived but no usable request line.
    Bad,
    TooLarge,
}

fn head_end(b: &[u8]) -> bool {
    b.windows(4).any(|w| w == b"\r\n\r\n") || b.windows(2).any(|w| w == b"\n\n")
}

/// Path from the request line, if the line is complete ("GET /x HTTP/1.0\r\n").
fn request_path(b: &[u8]) -> Option<String> {
    let nl = b.iter().position(|&c| c == b'\n')?;
    let line = String::from_utf8_lossy(&b[..nl]);
    let mut it = line.split_whitespace();
    let _method = it.next()?;
    it.next().map(|p| p.to_string())
}

/// Reads the request head across as many read() calls as the client needs (finding 1).
/// Stops at the blank line, at MAX_HEAD, at EOF or at the deadline. A complete request line is
/// enough on EOF/timeout, so sloppy clients that never send the blank line still get an answer.
fn read_head(s: &mut TcpStream, deadline: Duration) -> Head {
    let t = Instant::now();
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    let timed_out = loop {
        if head_end(&buf) {
            break false;
        }
        if buf.len() > MAX_HEAD {
            return Head::TooLarge;
        }
        let left = deadline.saturating_sub(t.elapsed());
        if left.is_zero() || s.set_read_timeout(Some(left)).is_err() {
            break true;
        }
        match s.read(&mut chunk) {
            Ok(0) => break false,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                break true
            }
            Err(_) => break false,
        }
    };
    if buf.len() > MAX_HEAD {
        return Head::TooLarge;
    }
    match request_path(&buf) {
        Some(p) => Head::Path(p),
        None if buf.is_empty() && timed_out => Head::Timeout,
        None if buf.is_empty() => Head::Empty,
        None => Head::Bad,
    }
}

/// Fresh results, or the last ones if a refresh takes too long; None before the first round ends.
fn snapshot(sh: &Shared) -> Option<Arc<Snap>> {
    let mut st = sh.st.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = st.last.map(|t| t.elapsed() <= CACHE_TTL).unwrap_or(false);
    if fresh {
        return st.snap.clone();
    }
    if !st.refreshing {
        st.refreshing = true;
        st.requested = true;
        sh.cv.notify_all();
    }
    let wait = if st.snap.is_some() {
        STALE_WAIT
    } else {
        COLD_WAIT
    };
    let (st, _) = sh
        .cv
        .wait_timeout_while(st, wait, |s| s.refreshing)
        .unwrap_or_else(|e| e.into_inner());
    st.snap.clone()
}

/// The one thread that runs probe rounds, only when a request asks for fresh results (finding 2).
fn refresher(sh: Arc<Shared>) {
    loop {
        {
            let mut st = sh.st.lock().unwrap_or_else(|e| e.into_inner());
            while !st.requested {
                st = sh.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            st.requested = false;
        }
        let rs = run(&sh.checks);
        let bad: Vec<String> = rs
            .iter()
            .filter(|r| !r.ok)
            .map(|r| r.name.clone())
            .collect();
        let at = zagreb_time::now_rfc3339();
        let mut st = sh.st.lock().unwrap_or_else(|e| e.into_inner());
        let prev_bad = st.snap.as_ref().map(|s| s.bad.clone()).unwrap_or_default();
        if bad != prev_bad {
            log(
                if bad.is_empty() { "info" } else { "warn" },
                &format!("failing=[{}]", bad.join(",")),
            );
        }
        st.snap = Some(Arc::new(Snap { rs, at, bad }));
        st.last = Some(Instant::now());
        st.refreshing = false;
        sh.cv.notify_all();
    }
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &str) {
    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
    let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = s.flush();
}

/// Close without RST: send FIN, then read and drop what the client still sends (a body, a late
/// header) for a short while. v0.1.0 closed with unread bytes, which makes the kernel send RST.
fn linger_close(s: TcpStream) {
    let _ = s.shutdown(Shutdown::Write);
    let t = Instant::now();
    let mut sink = [0u8; 1024];
    let mut s = s;
    let mut total = 0usize;
    while t.elapsed() < Duration::from_millis(500) && total < 65536 {
        let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
        match s.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n,
        }
    }
}

/// One request: answer it and report it to the nerve (no-op unless NERVE_URL
/// is set). An empty connection (no head at all) is not a request.
fn handle(sh: &Shared, s: TcpStream) {
    let t0 = Instant::now();
    let (what, code) = serve(sh, s);
    if code != 0 {
        nerve_client::observe_request("galion-health", &what, code, t0.elapsed());
    }
}

/// Answer one connection; returns (`GET <path>` or the error kind, status code; 0 = no request).
fn serve(sh: &Shared, mut s: TcpStream) -> (String, u16) {
    let path = match read_head(&mut s, HEAD_DEADLINE) {
        Head::Path(p) => p,
        Head::Empty => return (String::new(), 0),
        Head::Timeout => {
            respond(
                &mut s,
                "408 Request Timeout",
                "text/plain",
                "request timeout\n",
            );
            linger_close(s);
            return ("timeout".into(), 408);
        }
        Head::Bad => {
            respond(&mut s, "400 Bad Request", "text/plain", "bad request\n");
            linger_close(s);
            return ("bad_request".into(), 400);
        }
        Head::TooLarge => {
            respond(
                &mut s,
                "431 Request Header Fields Too Large",
                "text/plain",
                "request head too large\n",
            );
            linger_close(s);
            return ("bad_request".into(), 431);
        }
    };
    let what = format!("GET {path}");
    let known = matches!(path.as_str(), "/status.json" | "/json" | "/healthz" | "/");
    if !known {
        respond(&mut s, "404 Not Found", "text/plain", "not found\n");
        linger_close(s);
        return (what, 404);
    }
    let Some(snap) = snapshot(sh) else {
        respond(
            &mut s,
            "503 Service Unavailable",
            "text/plain",
            "warming up\n",
        );
        linger_close(s);
        return (what, 503);
    };
    let all_ok = snap.rs.iter().all(|r| r.ok);
    let (status, ctype, body) = match path.as_str() {
        "/status.json" | "/json" => ("200 OK", "application/json", to_json(&snap.rs, &snap.at)),
        "/healthz" => (
            if all_ok {
                "200 OK"
            } else {
                "503 Service Unavailable"
            },
            "text/plain",
            if all_ok {
                "ok\n".into()
            } else {
                format!("failing: {}\n", snap.bad.join(","))
            },
        ),
        _ => (
            "200 OK",
            "text/html; charset=utf-8",
            to_html(&snap.rs, &snap.at),
        ),
    };
    respond(&mut s, status, ctype, &body);
    linger_close(s);
    let code = if status.starts_with("200") { 200 } else { 503 };
    (what, code)
}

fn worker(sh: Arc<Shared>, rx: Arc<Mutex<Receiver<TcpStream>>>) {
    loop {
        let next = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
        match next {
            Ok(s) => handle(&sh, s),
            Err(_) => return,
        }
    }
}

fn workers_from_env() -> usize {
    std::env::var("GALION_HEALTH_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(3)
        .clamp(1, 6)
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> bool {
    std::thread::Builder::new()
        .name(name.into())
        .stack_size(STACK)
        .spawn(f)
        .map_err(|e| log("error", &format!("spawn {name}: {e}")))
        .is_ok()
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("galion-health {VERSION}");
        return;
    }
    let addr = std::env::var("GALION_HEALTH_ADDR").unwrap_or_else(|_| "127.0.0.1:8660".into());
    let checks =
        parse(&std::env::var("GALION_HEALTH_CHECKS").unwrap_or_else(|_| DEFAULT_CHECKS.into()));
    if std::env::args().nth(1).as_deref() == Some("--once") {
        let rs = run(&checks);
        println!("{}", to_json(&rs, &zagreb_time::now_rfc3339()));
        std::process::exit(if rs.iter().all(|r| r.ok) { 0 } else { 1 });
    }
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        log("error", &format!("bind {addr}: {e}"));
        std::process::exit(1)
    });
    let n_workers = workers_from_env();
    log(
        "info",
        &format!(
            "galion-health {VERSION} listening on {addr} checks={} workers={n_workers}",
            checks.len()
        ),
    );
    // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
    nerve_client::event("galion-health", "started", &format!("listening on {addr}"));
    nerve_client::start_heartbeat("galion-health", 60);
    let sh = Arc::new(Shared {
        st: Mutex::new(State::default()),
        cv: Condvar::new(),
        checks,
    });
    let r = sh.clone();
    if !spawn("refresher", move || refresher(r)) {
        std::process::exit(1);
    }
    let (tx, rx) = sync_channel::<TcpStream>(QUEUE);
    let rx = Arc::new(Mutex::new(rx));
    let mut started = 0;
    for i in 0..n_workers {
        let (sh, rx) = (sh.clone(), rx.clone());
        if spawn(&format!("worker-{i}"), move || worker(sh, rx)) {
            started += 1;
        }
    }
    if started == 0 {
        std::process::exit(1);
    }
    for stream in listener.incoming() {
        let Ok(s) = stream else { continue };
        match tx.try_send(s) {
            Ok(()) => {}
            Err(TrySendError::Full(mut s)) | Err(TrySendError::Disconnected(mut s)) => {
                // Every worker busy and the queue full: answer at once instead of stalling accept().
                respond(&mut s, "503 Service Unavailable", "text/plain", "busy\n");
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn res(name: &str, ok: bool) -> Res {
        Res {
            name: name.into(),
            unit: "-".into(),
            unit_state: "n/a".into(),
            port: 0,
            http: "-".into(),
            ms: 0,
            ok,
        }
    }

    #[test]
    fn default_checks_parse_to_twelve() {
        let c = parse(DEFAULT_CHECKS);
        assert_eq!(c.len(), 12, "live 8660 logs checks=12");
        assert_eq!(c[0].name, "openmind-memory");
        assert_eq!(c[0].port, 8420);
        assert_eq!(c[10].name, "ssh");
        assert_eq!(c[10].path, "-");
        assert_eq!(c[11].unit, "galion-backup.timer");
    }

    #[test]
    fn parse_skips_malformed_and_bad_ports() {
        let c = parse("a|-|x|/;bad;b|u.service|80|/h; ");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].port, 0);
        assert_eq!(c[1].unit, "u.service");
    }

    #[test]
    fn esc_handles_quotes_controls_and_lt() {
        assert_eq!(esc("a\"b\\c<\n"), "a\\\"b\\\\c\\u003c\\u000a");
        assert_eq!(html("<a&b>"), "&lt;a&amp;b&gt;");
    }

    #[test]
    fn json_shape_matches_live_key_set() {
        let j = to_json(
            &[res("x", true), res("y<", false)],
            "2026-10-20T22:52:05+02:00",
        );
        assert!(j.starts_with("{\"ok\":false,\"checked_at\":\"2026-10-20T22:52:05+02:00\",\"services\":[{\"name\":\"x\",\"unit\":\"-\",\"unit_state\":\"n/a\",\"port\":0,\"http\":\"-\",\"ms\":0,\"ok\":true}"));
        assert!(j.contains("\"name\":\"y\\u003c\""));
    }

    #[test]
    fn html_counts_ok_rows() {
        let h = to_html(&[res("x", true), res("y", false)], "t");
        assert!(h.contains("Galion health: 1/2 ok"));
        assert!(h.contains("<tr class=\"bad\"><td>y</td>"));
    }

    #[test]
    fn port_zero_and_no_unit_is_ok() {
        let r = run(&parse("n|-|0|-"));
        assert!(r[0].ok);
        assert_eq!(r[0].unit_state, "n/a");
        assert_eq!(r[0].http, "-");
    }

    /// Loopback pair on an ephemeral port (unit tests only; real tests use the slot range).
    fn pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (s, _) = l.accept().unwrap();
        (c, s)
    }

    #[test]
    fn read_head_joins_split_writes() {
        let (mut c, mut s) = pair();
        let w = std::thread::spawn(move || {
            for part in [
                "GE",
                "T /stat",
                "us.json HTTP/1.0\r\n",
                "Host: x\r\n",
                "\r\n",
            ] {
                c.write_all(part.as_bytes()).unwrap();
                std::thread::sleep(Duration::from_millis(60));
            }
            c
        });
        assert_eq!(
            read_head(&mut s, Duration::from_secs(3)),
            Head::Path("/status.json".into())
        );
        drop(w.join().unwrap());
    }

    #[test]
    fn read_head_request_line_without_blank_line_on_eof() {
        let (mut c, mut s) = pair();
        c.write_all(b"GET /healthz HTTP/1.0\r\n").unwrap();
        c.shutdown(Shutdown::Write).unwrap();
        assert_eq!(
            read_head(&mut s, Duration::from_secs(3)),
            Head::Path("/healthz".into())
        );
    }

    #[test]
    fn read_head_empty_timeout_bad_and_too_large() {
        let (c, mut s) = pair();
        drop(c);
        assert_eq!(read_head(&mut s, Duration::from_secs(3)), Head::Empty);

        let (_c, mut s) = pair();
        let t = Instant::now();
        assert_eq!(read_head(&mut s, Duration::from_millis(300)), Head::Timeout);
        assert!(t.elapsed() < Duration::from_secs(2));

        let (mut c, mut s) = pair();
        c.write_all(b"GET").unwrap();
        c.shutdown(Shutdown::Write).unwrap();
        assert_eq!(read_head(&mut s, Duration::from_secs(3)), Head::Bad);

        let (mut c, mut s) = pair();
        let w = std::thread::spawn(move || {
            let _ = c.write_all(b"GET / HTTP/1.0\r\nX: ");
            let _ = c.write_all(&[b'a'; MAX_HEAD + 100]);
            c
        });
        assert_eq!(read_head(&mut s, Duration::from_secs(3)), Head::TooLarge);
        drop(w.join().unwrap());
    }

    #[test]
    fn deadline_is_total_not_per_read() {
        // A client that trickles one byte every 100 ms never completes the head:
        // the deadline must still end the read (v0.1.0 had 3 s per read()).
        let (mut c, mut s) = pair();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let st = stop.clone();
        let w = std::thread::spawn(move || {
            while !st.load(std::sync::atomic::Ordering::Relaxed) {
                if c.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let t = Instant::now();
        assert_eq!(read_head(&mut s, Duration::from_millis(500)), Head::Bad);
        assert!(t.elapsed() < Duration::from_millis(1500));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        w.join().unwrap();
    }

    #[test]
    fn status_code_parsing() {
        assert_eq!(status_code(b"HTTP/1.1 200 OK\r\n"), "200");
        assert_eq!(status_code(b"HTTP/1.0 302"), "302");
        assert_eq!(status_code(b""), "no-reply");
    }

    #[test]
    fn probe_all_runs_slow_upstreams_side_by_side() {
        // Three upstreams that accept and never answer: v0.1.0 would take 3 x 2.5 s.
        let ls: Vec<TcpListener> = (0..3)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let spec: Vec<String> = ls
            .iter()
            .enumerate()
            .map(|(i, l)| format!("s{i}|-|{}|/h", l.local_addr().unwrap().port()))
            .collect();
        let checks = parse(&spec.join(";"));
        let t = Instant::now();
        let out = probe_all(&checks);
        let took = t.elapsed();
        assert!(out.iter().all(|(h, _)| h == "no-reply"), "{:?}", out);
        assert!(took >= PROBE_READ, "{took:?}");
        assert!(took < PROBE_READ + Duration::from_millis(1500), "{took:?}");
    }

    #[test]
    fn probe_all_reads_split_status_line() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let up = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 256];
            let _ = s.read(&mut b);
            s.write_all(b"HTTP/1.1 2").unwrap();
            std::thread::sleep(Duration::from_millis(100));
            s.write_all(b"04 No Content\r\n\r\n").unwrap();
        });
        let out = probe_all(&parse(&format!("u|-|{port}|/x;z|-|0|-")));
        up.join().unwrap();
        assert_eq!(out[0].0, "204");
        assert_eq!(out[1].0, "-");
    }

    #[test]
    fn unit_states_without_units_never_call_systemctl() {
        let st = unit_states(&parse("a|-|0|-;b|-|0|-"));
        assert_eq!(st, vec!["n/a".to_string(), "n/a".to_string()]);
    }
}
