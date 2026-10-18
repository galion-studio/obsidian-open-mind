// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! B54: hardware probe (port of hardware.sh + the machine.json half of
//! stack-boot.sh). CPU count, RAM from /proc/meminfo, GPU from nvidia-smi
//! when present (else a /sys/class/drm card counts as "present, no free
//! memory known"), then the B24 degradation profile picks weights/mouth/line.
//! No public-IP lookup: the host name for machine.json comes from config.

use openmind_survival::degrade::{profile, Signals};
use openmind_survival::Runner;
use serde_json::{json, Value};
use std::path::Path;

pub const WORKER_MIB: u64 = 512;

pub fn kib(meminfo: &str, key: &str) -> u64 {
    meminfo
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub total_mib: u64,
    pub used_mib: u64,
}

/// First row of `nvidia-smi --query-gpu=name,memory.total,memory.used
/// --format=csv,noheader,nounits`.
pub fn parse_nvidia(out: &str) -> Option<Gpu> {
    let row = out.lines().next()?;
    let p: Vec<&str> = row.split(',').map(str::trim).collect();
    if p.len() < 3 {
        return None;
    }
    let num = |s: &str| s.parse::<f64>().ok().map(|f| f as u64);
    Some(Gpu {
        name: p[0].to_string(),
        total_mib: num(p[1])?,
        used_mib: num(p[2])?,
    })
}

/// Any DRM card node (non-NVIDIA GPUs, SoC GPUs on boards and phones).
pub fn drm_card(sys_root: &Path) -> bool {
    std::fs::read_dir(sys_root.join("class/drm"))
        .map(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with("card") && !n.contains('-')
            })
        })
        .unwrap_or(false)
}

pub struct Probe<'a> {
    pub runner: &'a dyn Runner,
    pub proc_root: &'a Path,
    pub sys_root: &'a Path,
    pub lan_ok: bool,
    pub wan_ok: bool,
}

/// hardware.json document.
pub fn hardware(p: &Probe) -> Value {
    let mi = std::fs::read_to_string(p.proc_root.join("meminfo")).unwrap_or_default();
    let total = kib(&mi, "MemTotal:") / 1024;
    let free = kib(&mi, "MemAvailable:") / 1024;
    let gpu = p
        .runner
        .run(
            "nvidia-smi",
            &[
                "--query-gpu=name,memory.total,memory.used",
                "--format=csv,noheader,nounits",
            ],
        )
        .ok()
        .and_then(|o| parse_nvidia(&o));
    let drm = gpu.is_none() && drm_card(p.sys_root);
    let free_gpu = gpu
        .as_ref()
        .map_or(0, |g| g.total_mib.saturating_sub(g.used_mib));
    let prof = profile(&Signals {
        gpu_present: gpu.is_some() || drm,
        gpu_free_mib: free_gpu,
        ram_total_mib: total,
        lan_ok: p.lan_ok,
        wan_ok: p.wan_ok,
    });
    let gpu_v = match &gpu {
        Some(g) => {
            json!({"name": g.name, "total_mib": g.total_mib, "used_mib": g.used_mib, "free_mib": free_gpu})
        }
        None if drm => json!({"name": "drm", "total_mib": 0, "used_mib": 0, "free_mib": 0}),
        None => Value::Null,
    };
    json!({
        "processors": std::thread::available_parallelism().map_or(0, |n| n.get()),
        "arch": std::env::consts::ARCH,
        "os": std::env::consts::OS,
        "ram_total_mib": total,
        "ram_free_mib": free,
        "gpu": gpu_v,
        "worker_mib": WORKER_MIB,
        "cap": (free / WORKER_MIB).min(100),
        "weights": prof.weights,
        "mouth": prof.mouth,
        "llm": prof.llm.as_str(),
        "rule": prof.rule,
        "line": prof.line,
        "at": zagreb_time::now_rfc3339(),
    })
}

/// machine.json (stack-boot.sh), without the IP lookup and without the
/// writes into console-ide/src/lib/machine.ts and /root/galion-link.ps1.
pub fn machine(hw: &Value, hostname: &str, public_host: Option<&str>) -> Value {
    let weights = hw["weights"].as_bool().unwrap_or(false);
    let host = public_host.unwrap_or("");
    json!({
        "studio": "Galion Studio",
        "stack": "Obsidian",
        "stance": "Open Mind",
        "provider": if weights { "gpu" } else { "cpu" },
        "profile": hw["line"],
        "mode": "Obsidian · Open Mind · Galion Studio",
        "pod": hostname,
        "podId": hostname,
        "sshHost": host,
        "publicIp": host,
        "gpu": hw["gpu"]["name"].as_str().unwrap_or("none"),
        "connected": true,
        "ramTotalMib": hw["ram_total_mib"],
        "weights": weights,
        "mouth": hw["mouth"],
        "arch": hw["arch"],
    })
}

pub fn hostname(proc_root: &Path) -> String {
    std::fs::read_to_string(proc_root.join("sys/kernel/hostname"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "localhost".into())
}
