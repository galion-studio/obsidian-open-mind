// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only pull probes, written so `galion-health` (ws/foundation) can depend
//! on this crate (`galion-nerve = { path = "../galion-nerve" }`, use
//! `galion_nerve::probe`) instead of keeping its own copy. Every probe is
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
    if err.iter().any(|p| m.contains(p)) || m.starts_with("error") || msg.contains("ERROR") {
        return 2;
    }
    if priority == 4
        || m.contains("level=warn")
        || m.contains("\"level\":\"warn")
        || m.contains("[warn")
        || m.starts_with("warn")
    {
        return 1;
    }
    0
}

/// Parse `journalctl -o json` lines into counts per unit. A line belongs to
/// the unit in `_SYSTEMD_UNIT`, or (for pid 1 notices) in `UNIT`.
pub fn parse_journal(out: &str, units: &[&str]) -> HashMap<String, JournalCounts> {
    let mut m: HashMap<String, JournalCounts> = units
        .iter()
        .map(|u| (u.to_string(), JournalCounts::default()))
        .collect();
    for line in out.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let unit = [v.get("_SYSTEMD_UNIT"), v.get("UNIT")]
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str())
            .find(|u| m.contains_key(*u))
            .map(str::to_string);
        let Some(unit) = unit else { continue };
        let msg = v.get("MESSAGE").and_then(|x| x.as_str()).unwrap_or("");
        let prio = v
            .get("PRIORITY")
            .and_then(|x| x.as_str())
            .and_then(|p| p.parse().ok())
            .unwrap_or(6u8);
        let c = m.get_mut(&unit).expect("prefilled");
        c.lines += 1;
        if msg.contains("Scheduled restart job") || msg.contains("Failed with result") {
            c.restart_notices += 1;
        }
        match classify(prio, msg) {
            2 => {
                c.errors += 1;
                c.last_error = Some(msg.chars().take(200).collect());
            }
            1 => c.warnings += 1,
            _ => {}
        }
    }
    m
}

/// One journalctl call for every unit since `since` (unix seconds).
pub fn journal_scan(units: &[&str], since: u64) -> Result<HashMap<String, JournalCounts>, String> {
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
    let o = run("journalctl", &args, Duration::from_secs(15)).ok_or("journalctl timed out")?;
    if o.code != 0 && o.stdout.is_empty() {
        return Err(format!("journalctl exit {}", o.code));
    }
    Ok(parse_journal(&o.stdout, units))
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
