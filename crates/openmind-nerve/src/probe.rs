// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Read-only pull probes, written so `galion-health` (ws/foundation) can depend
//! on this crate (`galion-nerve = { path = "../galion-nerve" }`, use
//! `openmind_nerve::probe`) instead of keeping its own copy. Every probe is
//! bounded by a timeout, never writes, never reads secrets. Parsers are pure
//! functions so they are unit-tested without systemd.

use crate::util::{http, lo, run};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Properties read from `systemctl show` for one unit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitInfo {
    pub id: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub result: String,
    pub file_state: String,
    pub main_pid: u32,
    pub n_restarts: u32,
    /// cgroup memory in bytes (MemoryCurrent), `None` when not accounted.
    pub memory_current: Option<u64>,
    /// MemoryMax in bytes, `None` for infinity.
    pub memory_max: Option<u64>,
    pub cpu_nsec: Option<u64>,
    /// ActiveEnterTimestampMonotonic (us); changes on every (re)start.
    pub active_enter_mono: u64,
}

impl UnitInfo {
    pub fn found(&self) -> bool {
        self.load != "not-found" && !self.load.is_empty()
    }
}

pub const SHOW_PROPS: &str = "Id,LoadState,ActiveState,SubState,Result,UnitFileState,MainPID,NRestarts,MemoryCurrent,MemoryMax,CPUUsageNSec,ActiveEnterTimestampMonotonic";

fn num(v: &str) -> Option<u64> {
    // systemd prints 18446744073709551615 / "[not set]" / "infinity" for unset.
    match v.parse::<u64>() {
        Ok(u64::MAX) | Err(_) => None,
        Ok(n) => Some(n),
    }
}

/// Parse `systemctl show -p ... a b c` output: blocks separated by blank
/// lines, one per unit in argument order.
pub fn parse_show(out: &str) -> Vec<UnitInfo> {
    let mut v = Vec::new();
    for block in out.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut u = UnitInfo::default();
        for line in block.lines() {
            let Some((k, val)) = line.split_once('=') else {
                continue;
            };
            match k {
                "Id" => u.id = val.into(),
                "LoadState" => u.load = val.into(),
                "ActiveState" => u.active = val.into(),
                "SubState" => u.sub = val.into(),
                "Result" => u.result = val.into(),
                "UnitFileState" => u.file_state = val.into(),
                "MainPID" => u.main_pid = val.parse().unwrap_or(0),
                "NRestarts" => u.n_restarts = val.parse().unwrap_or(0),
                "MemoryCurrent" => u.memory_current = num(val),
                "MemoryMax" => u.memory_max = num(val),
                "CPUUsageNSec" => u.cpu_nsec = num(val),
                "ActiveEnterTimestampMonotonic" => u.active_enter_mono = val.parse().unwrap_or(0),
                _ => {}
            }
        }
        v.push(u);
    }
    v
}

/// One `systemctl show` call for many units. Keyed by the name asked for.
pub fn systemd_units(units: &[&str]) -> Result<HashMap<String, UnitInfo>, String> {
    if units.is_empty() {
        return Ok(HashMap::new());
    }
    let mut args = vec!["show", "--no-pager", "-p", SHOW_PROPS];
    args.extend_from_slice(units);
    let o = run("systemctl", &args, Duration::from_secs(10)).ok_or("systemctl show timed out")?;
    let parsed = parse_show(&o.stdout);
    if parsed.len() != units.len() {
        return Err(format!(
            "systemctl show returned {} blocks for {} units (exit {})",
            parsed.len(),
            units.len(),
            o.code
        ));
    }
    Ok(units.iter().map(|u| u.to_string()).zip(parsed).collect())
}

/// The service a timer starts: `x.timer` -> `x.service`.
pub fn timer_service(timer_unit: &str) -> Option<String> {
    timer_unit
        .strip_suffix(".timer")
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}.service"))
}

/// Last run of a timer's oneshot service (T2.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LastRun {
    pub unit: String,
    pub active: String,
    pub result: String,
    pub exit_status: Option<i64>,
    /// Unix seconds of the last `ExecStart` exit; `None` = never ran (or
    /// systemd too old for `--timestamp=unix`).
    pub finished: Option<u64>,
}

pub const RUN_PROPS: &str = "Id,LoadState,ActiveState,Result,ExecMainStatus,ExecMainExitTimestamp";

/// Parse `systemctl show --timestamp=unix -p RUN_PROPS a b` output.
pub fn parse_last_runs(out: &str) -> Vec<LastRun> {
    let mut v = Vec::new();
    for block in out.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut r = LastRun::default();
        for line in block.lines() {
            let Some((k, val)) = line.split_once('=') else {
                continue;
            };
            match k {
                "Id" => r.unit = val.into(),
                "ActiveState" => r.active = val.into(),
                "Result" => r.result = val.into(),
                "ExecMainStatus" => r.exit_status = val.parse().ok(),
                "ExecMainExitTimestamp" => {
                    r.finished = val
                        .strip_prefix('@')
                        .and_then(|x| x.parse().ok())
                        .filter(|x| *x > 0)
                }
                _ => {}
            }
        }
        v.push(r);
    }
    v
}

/// One read-only `systemctl show --timestamp=unix` call for the services
/// behind the timers. Keyed by service name.
pub fn last_runs(services: &[&str]) -> Result<HashMap<String, LastRun>, String> {
    if services.is_empty() {
        return Ok(HashMap::new());
    }
    let mut args = vec!["show", "--no-pager", "--timestamp=unix", "-p", RUN_PROPS];
    args.extend_from_slice(services);
    let o = run("systemctl", &args, Duration::from_secs(10)).ok_or("systemctl show timed out")?;
    let parsed = parse_last_runs(&o.stdout);
    if parsed.len() != services.len() {
        return Err(format!(
            "systemctl show (last runs) returned {} blocks for {} units (exit {})",
            parsed.len(),
            services.len(),
            o.code
        ));
    }
    Ok(services.iter().map(|u| u.to_string()).zip(parsed).collect())
}

impl LastRun {
    /// Sample `extra.last_run` (also what a replay reads back).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"unit": self.unit, "active": self.active, "result": self.result,
            "exit_status": self.exit_status, "finished_unix": self.finished,
            "finished": self.finished.map(crate::util::ts)})
    }
}

/// VmRSS of a process in KiB from /proc/<pid>/status.
pub fn proc_rss_kb(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_status_kb(&s, "VmRSS:")
}

pub fn parse_status_kb(s: &str, key: &str) -> Option<u64> {
    s.lines()
        .find(|l| l.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Result of an HTTP health probe.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpProbe {
    pub code: Option<u16>,
    pub ms: u64,
    pub error: Option<String>,
    /// First 512 bytes of the body (health bodies are tiny; never logged raw).
    pub body: String,
}

impl HttpProbe {
    pub fn ok(&self) -> bool {
        matches!(self.code, Some(c) if (200..400).contains(&c))
    }
}

/// GET http://127.0.0.1:<port><path>.
pub fn http_probe(port: u16, path: &str, timeout: Duration) -> HttpProbe {
    match http(lo(port), path, None, timeout) {
        Ok(r) => HttpProbe {
            code: Some(r.code),
            ms: r.ms,
            error: None,
            body: r.body.chars().take(512).collect(),
        },
        Err(e) => HttpProbe {
            code: None,
            ms: timeout.as_millis() as u64,
            error: Some(e),
            body: String::new(),
        },
    }
}

/// cloudflared Prometheus metrics we care about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TunnelMetrics {
    pub ha_connections: Option<u64>,
    pub total_requests: Option<u64>,
    pub request_errors: Option<u64>,
}

pub fn parse_cloudflared_metrics(text: &str) -> TunnelMetrics {
    let mut m = TunnelMetrics::default();
    for l in text.lines() {
        if l.starts_with('#') {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next()) else {
            continue;
        };
        let val = v.parse::<f64>().ok().map(|f| f as u64);
        match k {
            "cloudflared_tunnel_ha_connections" => m.ha_connections = val,
            "cloudflared_tunnel_total_requests" => m.total_requests = val,
            "cloudflared_tunnel_request_errors" => m.request_errors = val,
            _ => {}
        }
    }
    m
}

/// Scrape cloudflared's loopback metrics port (`--metrics` / default 2024x).
pub fn cloudflared_metrics(port: u16) -> Option<TunnelMetrics> {
    let r = http(lo(port), "/metrics", None, Duration::from_secs(3)).ok()?;
    (r.code == 200).then(|| parse_cloudflared_metrics(&r.body))
}

/// Host memory from /proc/meminfo (lxcfs gives the container's view): (total, available) KiB.
pub fn meminfo() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    Some((
        parse_status_kb(&s, "MemTotal:")?,
        parse_status_kb(&s, "MemAvailable:")?,
    ))
}

/// Swap from /proc/meminfo: (total, free) KiB.
pub fn swapinfo() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    Some((
        parse_status_kb(&s, "SwapTotal:")?,
        parse_status_kb(&s, "SwapFree:")?,
    ))
}

/// 1-minute load average.
pub fn loadavg() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Online CPUs from /proc/cpuinfo-free source (`nproc`-like via available_parallelism).
pub fn cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// `df -Pk <path>`: (total, available) KiB.
pub fn disk(path: &str) -> Option<(u64, u64)> {
    let o = run("df", &["-Pk", path], Duration::from_secs(5))?;
    parse_df(&o.stdout)
}

pub fn parse_df(out: &str) -> Option<(u64, u64)> {
    let l = out.lines().nth(1)?;
    let f: Vec<&str> = l.split_whitespace().collect();
    if f.len() < 4 {
        return None;
    }
    Some((f[1].parse().ok()?, f[3].parse().ok()?))
}

/// Journal counts for one unit over a window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JournalCounts {
    pub lines: u32,
    pub errors: u32,
    pub warnings: u32,
    /// systemd "Scheduled restart job" / "Failed with result" notices.
    pub restart_notices: u32,
    pub last_error: Option<String>,
}

/// Classify one journal message: 2 error, 1 warning, 0 other.
///
/// Besides the syslog priority this understands the text levels services
/// print on stderr (which journald stores at priority 6): logfmt/JSON
/// `level=error`, Rust panics, and zerolog's console format used by
/// cloudflared (`2026-10-20T16:37:21Z ERR ...`, `WRN`).
pub fn classify(priority: u8, msg: &str) -> u8 {
    if priority <= 3 {
        return 2;
    }
    let m = msg.to_ascii_lowercase();
    let err = [
        "level=error",
        "\"level\":\"error\"",
        "level=fatal",
        "panicked",
        "fatal",
        "failed with result",
        " error:",
        "[error]",
    ];
    if err.iter().any(|p| m.contains(p))
        || m.starts_with("error")
        || msg.contains("ERROR")
        || zerolog_level(msg) == Some("ERR")
        || zerolog_level(msg) == Some("FTL")
    {
        return 2;
    }
    if priority == 4
        || m.contains("level=warn")
        || m.contains("\"level\":\"warn")
        || m.contains("[warn")
        || m.starts_with("warn")
        || zerolog_level(msg) == Some("WRN")
    {
        return 1;
    }
    0
}

/// zerolog console level token right after a leading timestamp
/// (`<ts> ERR ...`), or at the very start (`ERR ...`).
pub fn zerolog_level(msg: &str) -> Option<&str> {
    let mut it = msg.split_whitespace();
    let a = it.next()?;
    let lvl = if a.len() >= 19 && a.as_bytes()[4] == b'-' {
        it.next()?
    } else {
        a
    };
    ["ERR", "WRN", "INF", "DBG", "FTL"]
        .into_iter()
        .find(|l| *l == lvl)
}

/// One journald record (from `journalctl -o json`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JLine {
    /// Unix seconds (`__REALTIME_TIMESTAMP`); 0 when absent.
    pub t: u64,
    /// `_SYSTEMD_UNIT`, or `UNIT` for pid-1 notices about a unit.
    pub unit: String,
    pub prio: u8,
    pub msg: String,
}

/// Parse `journalctl -o json` output. Lines whose MESSAGE is not a string
/// (binary blobs) and non-JSON lines are skipped. `units` filters on the
/// unit (`UNIT` wins over `_SYSTEMD_UNIT` when it names a wanted unit);
/// an empty filter keeps everything.
pub fn parse_journal_lines(out: &str, units: &[&str]) -> Vec<JLine> {
    let mut v = Vec::new();
    for line in out.lines() {
        let Ok(j) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let cands: Vec<&str> = [j.get("UNIT"), j.get("_SYSTEMD_UNIT")]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let unit = if units.is_empty() {
            cands.last().copied()
        } else {
            cands.iter().copied().find(|u| units.contains(u))
        };
        let Some(unit) = unit else { continue };
        let Some(msg) = j.get("MESSAGE").and_then(Value::as_str) else {
            if !units.is_empty() {
                // Count the line but keep no text.
                v.push(JLine {
                    unit: unit.to_string(),
                    prio: 6,
                    ..Default::default()
                });
            }
            continue;
        };
        let t = j
            .get("__REALTIME_TIMESTAMP")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .map(|us| us / 1_000_000)
            .unwrap_or(0);
        let prio = j
            .get("PRIORITY")
            .and_then(Value::as_str)
            .and_then(|p| p.parse().ok())
            .unwrap_or(6u8);
        v.push(JLine {
            t,
            unit: unit.to_string(),
            prio,
            msg: msg.chars().take(2000).collect(),
        });
    }
    v
}

/// Per-unit counts from parsed journal lines.
pub fn journal_counts(lines: &[JLine], units: &[&str]) -> HashMap<String, JournalCounts> {
    let mut m: HashMap<String, JournalCounts> = units
        .iter()
        .map(|u| (u.to_string(), JournalCounts::default()))
        .collect();
    for l in lines {
        let Some(c) = m.get_mut(&l.unit) else {
            continue;
        };
        c.lines += 1;
        if l.msg.contains("Scheduled restart job") || l.msg.contains("Failed with result") {
            c.restart_notices += 1;
        }
        match classify(l.prio, &l.msg) {
            2 => {
                c.errors += 1;
                c.last_error = Some(l.msg.chars().take(200).collect());
            }
            1 => c.warnings += 1,
            _ => {}
        }
    }
    m
}

/// Parse `journalctl -o json` lines into counts per unit. A line belongs to
/// the unit in `_SYSTEMD_UNIT`, or (for pid 1 notices) in `UNIT`.
pub fn parse_journal(out: &str, units: &[&str]) -> HashMap<String, JournalCounts> {
    journal_counts(&parse_journal_lines(out, units), units)
}

/// One journalctl call for every unit since `since` (unix seconds).
pub fn journal_lines(units: &[&str], since: u64) -> Result<Vec<JLine>, String> {
    let since_arg = format!("@{since}");
    let mut args = vec![
        "-o",
        "json",
        "--no-pager",
        "-q",
        "--output-fields=_SYSTEMD_UNIT,UNIT,PRIORITY,MESSAGE",
        "--since",
        since_arg.as_str(),
    ];
    for u in units {
        args.push("-u");
        args.push(u);
    }
    let o = run("journalctl", &args, Duration::from_secs(15))
        .ok_or("journalctl unavailable or timed out")?;
    if o.code != 0 && o.stdout.is_empty() {
        return Err(format!("journalctl exit {}", o.code));
    }
    Ok(parse_journal_lines(&o.stdout, units))
}

/// Counts only (kept for callers of the earlier API).
pub fn journal_scan(units: &[&str], since: u64) -> Result<HashMap<String, JournalCounts>, String> {
    Ok(journal_counts(&journal_lines(units, since)?, units))
}

/// One listening TCP socket.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Listener {
    pub addr: String,
    pub port: u16,
    pub loopback: bool,
}

fn listener(addr: &str, port: u16) -> Listener {
    let a = addr.trim_start_matches('[').trim_end_matches(']');
    let a = a.split('%').next().unwrap_or(a);
    let loopback = a.starts_with("127.") || a == "::1" || a == "localhost";
    Listener {
        addr: a.to_string(),
        port,
        loopback,
    }
}

/// Parse `ss -ltn` text (Local Address:Port in column 4).
pub fn parse_ss(out: &str) -> Vec<Listener> {
    let mut v: Vec<Listener> = out
        .lines()
        .filter(|l| l.starts_with("LISTEN"))
        .filter_map(|l| {
            let local = l.split_whitespace().nth(3)?;
            let (a, p) = local.rsplit_once(':')?;
            Some(listener(a, p.parse().ok()?))
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn hex_ipv4(h: &str) -> Option<String> {
    let n = u32::from_str_radix(h, 16).ok()?;
    let b = n.to_le_bytes();
    Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]))
}

fn hex_ipv6(h: &str) -> Option<String> {
    if h.len() != 32 {
        return None;
    }
    let mut words = [0u16; 8];
    for (i, chunk) in (0..4).map(|i| &h[i * 8..i * 8 + 8]).enumerate() {
        let n = u32::from_str_radix(chunk, 16).ok()?.to_le_bytes();
        words[i * 2] = u16::from_be_bytes([n[0], n[1]]);
        words[i * 2 + 1] = u16::from_be_bytes([n[2], n[3]]);
    }
    Some(std::net::Ipv6Addr::from(words).to_string())
}

/// Parse `/proc/net/tcp` or `/proc/net/tcp6` (state 0A = LISTEN).
pub fn parse_proc_net_tcp(text: &str, v6: bool) -> Vec<Listener> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.get(3) != Some(&"0A") {
                return None;
            }
            let (a, p) = f.get(1)?.split_once(':')?;
            let port = u16::from_str_radix(p, 16).ok()?;
            let addr = if v6 { hex_ipv6(a)? } else { hex_ipv4(a)? };
            let addr = match addr.as_str() {
                "::" => "*".to_string(),
                "0.0.0.0" => "0.0.0.0".to_string(),
                _ => addr,
            };
            Some(listener(&addr, port))
        })
        .collect()
}

/// Listening TCP sockets of this network namespace, read from /proc (no
/// subprocess). `None` when /proc/net is unreadable.
pub fn listeners() -> Option<Vec<Listener>> {
    let v4 = std::fs::read_to_string("/proc/net/tcp").ok()?;
    let mut v = parse_proc_net_tcp(&v4, false);
    if let Ok(v6) = std::fs::read_to_string("/proc/net/tcp6") {
        v.extend(parse_proc_net_tcp(&v6, true));
    }
    v.sort();
    v.dedup();
    Some(v)
}

/// `galion-health` /status.json: name -> ok.
pub fn parse_galion_health(body: &str) -> Option<HashMap<String, bool>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let svcs = v.get("services")?.as_array()?;
    Some(
        svcs.iter()
            .filter_map(|s| {
                Some((
                    s.get("name")?.as_str()?.to_string(),
                    s.get("ok")?.as_bool()?,
                ))
            })
            .collect(),
    )
}

/// Processes whose command line contains any of `needles` (deploy guard).
pub fn procs_matching(needles: &[&str]) -> Vec<(u32, String)> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return v;
    };
    let me = std::process::id();
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
        if needles.iter().any(|n| cmd.contains(n)) {
            v.push((pid, cmd.trim().chars().take(160).collect()));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_blocks() {
        let out = "Id=a.service\nLoadState=loaded\nActiveState=active\nSubState=running\nResult=success\nUnitFileState=enabled\nMainPID=42\nNRestarts=2\nMemoryCurrent=2142208\nMemoryMax=infinity\nCPUUsageNSec=530780000\nActiveEnterTimestampMonotonic=99\n\nId=b.service\nLoadState=not-found\nActiveState=inactive\nMainPID=0\nMemoryCurrent=[not set]\nMemoryMax=134217728\n";
        let v = parse_show(out);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].main_pid, 42);
        assert_eq!(v[0].n_restarts, 2);
        assert_eq!(v[0].memory_current, Some(2142208));
        assert_eq!(v[0].memory_max, None);
        assert!(v[0].found());
        assert!(!v[1].found());
        assert_eq!(v[1].memory_current, None);
        assert_eq!(v[1].memory_max, Some(134217728));
    }

    #[test]
    fn cloudflared_parse() {
        let t = "# HELP x\ncloudflared_tunnel_ha_connections 4\ncloudflared_tunnel_request_errors 81\ncloudflared_tunnel_total_requests 2.12e+02\nother 1\n";
        let m = parse_cloudflared_metrics(t);
        assert_eq!(m.ha_connections, Some(4));
        assert_eq!(m.request_errors, Some(81));
        assert_eq!(m.total_requests, Some(212));
    }

    #[test]
    fn df_and_status() {
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/x 1000 400 600 40% /\n";
        assert_eq!(parse_df(df), Some((1000, 600)));
        assert_eq!(
            parse_status_kb("Name:\tx\nVmRSS:\t  3992 kB\n", "VmRSS:"),
            Some(3992)
        );
    }

    #[test]
    fn journal_classify_and_count() {
        assert_eq!(classify(3, "anything"), 2);
        assert_eq!(classify(6, "ts=x level=error msg=boom"), 2);
        assert_eq!(classify(6, "{\"level\":\"warn\",\"msg\":\"x\"}"), 1);
        assert_eq!(classify(6, "served errors=0 ok"), 0);
        assert_eq!(classify(6, "thread 'main' panicked at x"), 2);
        let out = concat!(
            "{\"_SYSTEMD_UNIT\":\"a.service\",\"PRIORITY\":\"6\",\"MESSAGE\":\"ok\"}\n",
            "{\"_SYSTEMD_UNIT\":\"a.service\",\"PRIORITY\":\"3\",\"MESSAGE\":\"bad\"}\n",
            "{\"_SYSTEMD_UNIT\":\"init.scope\",\"UNIT\":\"a.service\",\"PRIORITY\":\"6\",\"MESSAGE\":\"a.service: Scheduled restart job, restart counter is at 1.\"}\n",
            "{\"_SYSTEMD_UNIT\":\"z.service\",\"PRIORITY\":\"3\",\"MESSAGE\":\"other\"}\n",
            "{\"_SYSTEMD_UNIT\":\"a.service\",\"MESSAGE\":[1,2]}\n",
            "not json\n"
        );
        let m = parse_journal(out, &["a.service", "b.service"]);
        let a = &m["a.service"];
        assert_eq!((a.lines, a.errors, a.restart_notices), (4, 1, 1));
        assert_eq!(a.last_error.as_deref(), Some("bad"));
        assert_eq!(m["b.service"], JournalCounts::default());
    }

    #[test]
    fn galion_health_json() {
        let b = r#"{"ok":true,"checked_at":"x","services":[{"name":"tuner","ok":true},{"name":"one","ok":false}]}"#;
        let m = parse_galion_health(b).unwrap();
        assert_eq!(m.get("tuner"), Some(&true));
        assert_eq!(m.get("one"), Some(&false));
        assert!(parse_galion_health("nope").is_none());
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn zerolog_cloudflared_is_error() {
        let m = "2026-10-20T16:37:21Z ERR  error=\"Unable to reach the origin service.\" originService=http://localhost:4001";
        assert_eq!(zerolog_level(m), Some("ERR"));
        assert_eq!(classify(6, m), 2);
        assert_eq!(classify(6, "2026-10-20T16:37:21Z WRN slow"), 1);
        assert_eq!(classify(6, "2026-10-20T16:37:21Z INF ok"), 0);
        assert_eq!(classify(6, "ERRATIC but fine"), 0);
    }

    #[test]
    fn journal_lines_keep_time() {
        let out = "{\"_SYSTEMD_UNIT\":\"cloudflared.service\",\"__REALTIME_TIMESTAMP\":\"1792514241640897\",\"PRIORITY\":\"6\",\"MESSAGE\":\"x\"}\n";
        let v = parse_journal_lines(out, &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].t, 1_792_514_241);
        assert_eq!(v[0].unit, "cloudflared.service");
    }

    #[test]
    fn ss_and_proc_listeners() {
        let ss = "State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess\nLISTEN 0 4096 127.0.0.53%lo:53 0.0.0.0:*\nLISTEN 0 128 127.0.0.1:8652 0.0.0.0:*\nLISTEN 0 4096 *:22 *:*\nLISTEN 0 100 [::1]:25 [::]:*\n";
        let v = parse_ss(ss);
        assert_eq!(v.len(), 4);
        assert!(v.iter().any(|l| l.port == 8652 && l.loopback));
        assert!(v.iter().any(|l| l.port == 22 && !l.loopback));
        assert!(v.iter().any(|l| l.port == 25 && l.loopback));
        assert!(v.iter().any(|l| l.port == 53 && l.loopback));
        let t4 = "  sl  local_address rem_address   st\n   0: 0100007F:2198 00000000:0000 0A 0\n   1: 00000000:0016 00000000:0000 0A 0\n   2: 0100007F:2198 0100007F:1234 01 0\n";
        let v = parse_proc_net_tcp(t4, false);
        assert_eq!(v.len(), 2);
        assert!(v.contains(&Listener {
            addr: "127.0.0.1".into(),
            port: 8600,
            loopback: true
        }));
        assert!(v.contains(&Listener {
            addr: "0.0.0.0".into(),
            port: 22,
            loopback: false
        }));
        let t6 = "x\n 0: 00000000000000000000000001000000:0019 00000000000000000000000000000000:0000 0A\n";
        let v = parse_proc_net_tcp(t6, true);
        assert_eq!(v[0].addr, "::1");
        assert!(v[0].loopback);
        assert!(listeners().is_some());
    }
}
