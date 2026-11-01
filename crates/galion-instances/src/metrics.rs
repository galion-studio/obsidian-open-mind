// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! What a device has and how busy it is: CPU, RAM and GPU/VRAM.
//!
//! Linux reads `/proc` and `/sys` (no programs run except `nvidia-smi` when
//! it is on PATH). macOS asks `sysctl`/`vm_stat`; Windows asks PowerShell
//! (CIM). Every reading is optional: a missing value is `None`, never a
//! guess. The parsers are pure functions so they are tested on fixtures.

use serde_json::{json, Map, Value};
use std::process::{Command, Stdio};
use std::time::Duration;

/// One graphics card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_total_mb: Option<u64>,
    pub vram_used_mb: Option<u64>,
    pub busy_pct: Option<f64>,
}

/// One reading of a device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub cores: Option<u64>,
    pub cpu_pct: Option<f64>,
    pub ram_total_mb: Option<u64>,
    pub ram_used_mb: Option<u64>,
    pub gpus: Vec<Gpu>,
    /// Local model servers that answered on loopback (e.g. "ollama").
    pub models: Vec<String>,
}

fn num(v: Option<f64>) -> Value {
    match v {
        Some(x) if x.is_finite() => json!((x * 10.0).round() / 10.0),
        _ => Value::Null,
    }
}

fn int(v: Option<u64>) -> Value {
    v.map(Value::from).unwrap_or(Value::Null)
}

/// Longest text kept from a device for any one field.
pub const MAX_TEXT: usize = 120;

/// Trim, drop control characters, cap length.
pub fn clean(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

impl Metrics {
    pub fn to_json(&self) -> Value {
        json!({
            "hostname": self.hostname,
            "os": self.os,
            "arch": self.arch,
            "cpu_model": self.cpu_model,
            "cores": int(self.cores),
            "cpu_pct": num(self.cpu_pct),
            "ram_total_mb": int(self.ram_total_mb),
            "ram_used_mb": int(self.ram_used_mb),
            "gpus": self.gpus.iter().map(|g| json!({
                "name": g.name,
                "vram_total_mb": int(g.vram_total_mb),
                "vram_used_mb": int(g.vram_used_mb),
                "busy_pct": num(g.busy_pct),
            })).collect::<Vec<_>>(),
            "models": self.models,
        })
    }

    /// Read a reading sent by a device. Unknown keys are ignored, numbers
    /// are range-checked and text is cleaned, so a device cannot inject
    /// markup or absurd values.
    pub fn from_json(v: &Value) -> Metrics {
        let text = |k: &str| clean(v.get(k).and_then(Value::as_str).unwrap_or(""), MAX_TEXT);
        let u = |x: Option<&Value>, max: u64| {
            x.and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0 && *n <= max as f64)
                .map(|n| n as u64)
        };
        let pct = |x: Option<&Value>| {
            x.and_then(Value::as_f64)
                .filter(|n| n.is_finite())
                .map(|n| n.clamp(0.0, 100.0))
        };
        const TB: u64 = 64 * 1024 * 1024; // 64 TB in MB
        let gpus = v
            .get("gpus")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .take(8)
                    .map(|g| Gpu {
                        name: clean(
                            g.get("name").and_then(Value::as_str).unwrap_or(""),
                            MAX_TEXT,
                        ),
                        vram_total_mb: u(g.get("vram_total_mb"), TB),
                        vram_used_mb: u(g.get("vram_used_mb"), TB),
                        busy_pct: pct(g.get("busy_pct")),
                    })
                    .filter(|g| !g.name.is_empty() || g.vram_total_mb.is_some())
                    .collect()
            })
            .unwrap_or_default();
        let models = v
            .get("models")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| clean(s, 40))
                    .filter(|s| !s.is_empty())
                    .take(8)
                    .collect()
            })
            .unwrap_or_default();
        Metrics {
            hostname: text("hostname"),
            os: text("os"),
            arch: text("arch"),
            cpu_model: text("cpu_model"),
            cores: u(v.get("cores"), 4096).filter(|c| *c > 0),
            cpu_pct: pct(v.get("cpu_pct")),
            ram_total_mb: u(v.get("ram_total_mb"), TB),
            ram_used_mb: u(v.get("ram_used_mb"), TB),
            gpus,
            models,
        }
    }
}

/// `/proc/meminfo` -> (total MB, used MB). Used = total - available.
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse::<u64>().ok()
        })
    };
    let total = field("MemTotal")?;
    let avail = field("MemAvailable").or_else(|| {
        Some(field("MemFree")? + field("Buffers").unwrap_or(0) + field("Cached").unwrap_or(0))
    })?;
    Some((total / 1024, total.saturating_sub(avail) / 1024))
}

/// First `cpu ` line of `/proc/stat` -> (busy, total) jiffies.
pub fn parse_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let total: u64 = v.iter().take(8).sum();
    Some((total - idle, total))
}

/// Percent busy between two `/proc/stat` samples.
pub fn busy_between(a: (u64, u64), b: (u64, u64)) -> Option<f64> {
    let dt = b.1.checked_sub(a.1)?;
    let db = b.0.checked_sub(a.0)?;
    (dt > 0).then(|| (db as f64 * 100.0 / dt as f64).clamp(0.0, 100.0))
}

/// `model name` from `/proc/cpuinfo` (x86) or `Hardware`/`Model` (ARM).
pub fn parse_cpuinfo(text: &str) -> String {
    for key in ["model name", "Hardware", "Model", "cpu model"] {
        if let Some(v) = text.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_string())
        }) {
            if !v.is_empty() {
                return clean(&v, MAX_TEXT);
            }
        }
    }
    String::new()
}

/// `nvidia-smi --query-gpu=name,memory.total,memory.used,utilization.gpu
/// --format=csv,noheader,nounits` -> cards.
pub fn parse_nvidia_smi(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if f.len() < 2 || f[0].is_empty() {
                return None;
            }
            let n = |i: usize| f.get(i).and_then(|x| x.parse::<f64>().ok());
            Some(Gpu {
                name: clean(f[0], MAX_TEXT),
                vram_total_mb: n(1).map(|x| x as u64),
                vram_used_mb: n(2).map(|x| x as u64),
                busy_pct: n(3).map(|x| x.clamp(0.0, 100.0)),
            })
        })
        .take(8)
        .collect()
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Bounded wait: a hung driver tool must not stall the heartbeat.
    for _ in 0..50 {
        if let Ok(Some(_)) = child.try_wait() {
            let out = child.wait_with_output().ok()?;
            return out
                .status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn nvidia() -> Vec<Gpu> {
    run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,memory.used,utilization.gpu",
            "--format=csv,noheader,nounits",
        ],
    )
    .map(|t| parse_nvidia_smi(&t))
    .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn linux_amd_gpus() -> Vec<Gpu> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/drm") else {
        return out;
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("card") && !n.contains('-'))
        .collect();
    names.sort();
    for n in names {
        let dev = format!("/sys/class/drm/{n}/device");
        let read = |f: &str| {
            std::fs::read_to_string(format!("{dev}/{f}"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
        };
        if let Some(total) = read("mem_info_vram_total") {
            out.push(Gpu {
                name: "AMD graphics".into(),
                vram_total_mb: Some(total / 1024 / 1024),
                vram_used_mb: read("mem_info_vram_used").map(|u| u / 1024 / 1024),
                busy_pct: read("gpu_busy_percent").map(|p| p as f64),
            });
        }
    }
    out
}

fn hostname() -> String {
    let h = std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| run("hostname", &[]))
        .unwrap_or_default();
    clean(&h, 63)
}

/// Loopback model servers that answer a TCP connect (Ollama, llama.cpp).
pub fn local_models() -> Vec<String> {
    let mut out = Vec::new();
    for (port, name) in [(11434u16, "ollama"), (8080, "llama.cpp")] {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok() {
            out.push(name.to_string());
        }
    }
    out
}

/// Read this machine now. Takes about `sample` for the CPU busy figure.
pub fn probe(sample: Duration) -> Metrics {
    let mut m = Metrics {
        hostname: hostname(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cores: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as u64),
        ..Metrics::default()
    };
    probe_os(&mut m, sample);
    if m.gpus.is_empty() && !cfg!(target_os = "linux") {
        m.gpus = nvidia();
    }
    m
}

#[cfg(target_os = "linux")]
fn probe_os(m: &mut Metrics, sample: Duration) {
    m.os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| clean(v.trim_matches('"'), 60))
            })
        })
        .unwrap_or_else(|| "Linux".into());
    m.cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .map(|t| parse_cpuinfo(&t))
        .unwrap_or_default();
    if let Some((t, u)) = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| parse_meminfo(&t))
    {
        m.ram_total_mb = Some(t);
        m.ram_used_mb = Some(u);
    }
    let stat = || {
        std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|t| parse_stat(&t))
    };
    if let Some(a) = stat() {
        std::thread::sleep(sample);
        m.cpu_pct = stat().and_then(|b| busy_between(a, b));
    }
    m.gpus = nvidia();
    if m.gpus.is_empty() {
        m.gpus = linux_amd_gpus();
    }
}

#[cfg(target_os = "macos")]
fn probe_os(m: &mut Metrics, _sample: Duration) {
    m.os = run("sw_vers", &["-productVersion"])
        .map(|v| format!("macOS {}", v.trim()))
        .unwrap_or_else(|| "macOS".into());
    m.cpu_model = run("sysctl", &["-n", "machdep.cpu.brand_string"])
        .map(|s| clean(&s, MAX_TEXT))
        .unwrap_or_default();
    m.ram_total_mb = run("sysctl", &["-n", "hw.memsize"])
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|b| b / 1024 / 1024);
    if let (Some(total), Some(vm)) = (m.ram_total_mb, run("vm_stat", &[])) {
        m.ram_used_mb = parse_vm_stat(&vm).map(|free| total.saturating_sub(free));
    }
    // Apple silicon shares RAM with the GPU: show the chip as the GPU.
    if m.cpu_model.starts_with("Apple") {
        m.gpus = vec![Gpu {
            name: format!("{} GPU (shared memory)", m.cpu_model),
            vram_total_mb: m.ram_total_mb,
            vram_used_mb: None,
            busy_pct: None,
        }];
    }
    if let Some(load) = run("sysctl", &["-n", "vm.loadavg"]) {
        let l1 = load
            .trim_matches(|c| c == '{' || c == '}' || c == ' ' || c == '\n')
            .split_whitespace()
            .next()
            .and_then(|x| x.parse::<f64>().ok());
        if let (Some(l), Some(c)) = (l1, m.cores) {
            m.cpu_pct = Some((l * 100.0 / c as f64).clamp(0.0, 100.0));
        }
    }
}

/// `vm_stat` -> free + inactive MB (macOS).
pub fn parse_vm_stat(text: &str) -> Option<u64> {
    let page = text
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let pages = |k: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(k)?;
            rest.trim_start_matches(':')
                .trim()
                .trim_end_matches('.')
                .parse::<u64>()
                .ok()
        })
    };
    let free = pages("Pages free")? + pages("Pages inactive").unwrap_or(0);
    Some(free * page / 1024 / 1024)
}

#[cfg(windows)]
fn probe_os(m: &mut Metrics, _sample: Duration) {
    m.os = "Windows".into();
    let ps = |q: &str| {
        run(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", q],
        )
    };
    m.cpu_model = ps("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name")
        .map(|s| clean(&s, MAX_TEXT))
        .unwrap_or_default();
    if let Some(t) = ps("$o=Get-CimInstance Win32_OperatingSystem; \"$($o.TotalVisibleMemorySize) $($o.FreePhysicalMemory) $($o.Caption)\"") {
        let mut it = t.split_whitespace();
        let total = it.next().and_then(|x| x.parse::<u64>().ok());
        let free = it.next().and_then(|x| x.parse::<u64>().ok());
        let cap: Vec<&str> = it.collect();
        if !cap.is_empty() {
            m.os = clean(&cap.join(" "), 60);
        }
        m.ram_total_mb = total.map(|k| k / 1024);
        if let (Some(t), Some(f)) = (total, free) {
            m.ram_used_mb = Some(t.saturating_sub(f) / 1024);
        }
    }
    m.cpu_pct = ps("(Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average")
        .and_then(|s| s.trim().parse::<f64>().ok());
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn probe_os(_m: &mut Metrics, _sample: Duration) {}

/// The reading as a JSON object map (for embedding).
pub fn to_map(m: &Metrics) -> Map<String, Value> {
    match m.to_json() {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo() {
        let t = "MemTotal:       16303428 kB\nMemFree:  1000 kB\nMemAvailable:   12000000 kB\n";
        assert_eq!(parse_meminfo(t), Some((15921, 4202)));
        let old = "MemTotal: 2048000 kB\nMemFree: 1024000 kB\nBuffers: 0 kB\nCached: 0 kB\n";
        assert_eq!(parse_meminfo(old), Some((2000, 1000)));
        assert_eq!(parse_meminfo("nope"), None);
    }

    #[test]
    fn stat_busy() {
        let a = parse_stat("cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        let b = parse_stat("cpu  150 0 150 900 0 0 0 0 0 0\n").unwrap();
        assert_eq!(a, (200, 1000));
        assert_eq!(busy_between(a, b), Some(50.0));
        assert_eq!(busy_between(b, a), None);
        assert_eq!(busy_between(a, a), None);
    }

    #[test]
    fn cpuinfo_and_nvidia() {
        let t = "processor : 0\nmodel name\t: AMD Ryzen 7 7840HS  w/ Radeon 780M\n";
        assert_eq!(parse_cpuinfo(t), "AMD Ryzen 7 7840HS w/ Radeon 780M");
        assert_eq!(parse_cpuinfo("Hardware\t: BCM2835\n"), "BCM2835");
        let g = parse_nvidia_smi("NVIDIA GeForce RTX 4060 Laptop GPU, 8188, 512, 3\n, 1, 2\n");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].vram_total_mb, Some(8188));
        assert_eq!(g[0].vram_used_mb, Some(512));
        assert_eq!(g[0].busy_pct, Some(3.0));
        let g = parse_nvidia_smi("Tesla T4, [N/A], [N/A], [N/A]\n");
        assert_eq!(g[0].vram_total_mb, None);
    }

    #[test]
    fn vm_stat() {
        let t = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:      65536.\nPages inactive:  65536.\n";
        assert_eq!(parse_vm_stat(t), Some(2048));
    }

    #[test]
    fn json_round_trip_and_hostile_input() {
        let m = Metrics {
            hostname: "studio".into(),
            os: "Linux".into(),
            arch: "x86_64".into(),
            cpu_model: "Ryzen".into(),
            cores: Some(16),
            cpu_pct: Some(12.345),
            ram_total_mb: Some(65536),
            ram_used_mb: Some(1024),
            gpus: vec![Gpu {
                name: "RTX".into(),
                vram_total_mb: Some(8192),
                vram_used_mb: Some(100),
                busy_pct: Some(1.0),
            }],
            models: vec!["ollama".into()],
        };
        let back = Metrics::from_json(&m.to_json());
        assert_eq!(back.cpu_pct, Some(12.3));
        assert_eq!(back.gpus, m.gpus);
        assert_eq!(back.models, m.models);
        let bad = serde_json::json!({
            "hostname": "<script>\u{1b}x</script>".repeat(40),
            "cores": -3, "cpu_pct": 900, "ram_total_mb": 1e30,
            "gpus": [{"name": ""}, {"name": "ok", "vram_total_mb": "8"}],
        });
        let m = Metrics::from_json(&bad);
        assert!(m.hostname.chars().count() <= MAX_TEXT);
        assert!(!m.hostname.contains('\u{1b}'));
        assert_eq!(m.cores, None);
        assert_eq!(m.cpu_pct, Some(100.0));
        assert_eq!(m.ram_total_mb, None);
        assert_eq!(m.gpus.len(), 1);
        assert_eq!(m.gpus[0].vram_total_mb, None);
    }

    #[test]
    fn probe_this_box() {
        let m = probe(Duration::from_millis(50));
        assert!(m.cores.unwrap_or(0) >= 1);
        if cfg!(target_os = "linux") {
            assert!(m.ram_total_mb.unwrap_or(0) > 0);
            assert!(m.cpu_pct.is_some());
        }
    }
}
