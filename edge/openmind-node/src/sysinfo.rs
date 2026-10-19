// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Local facts read from /proc: CPU, RAM, ARP table, default route.

use crate::util::{is_termux, read_file};
use serde::Serialize;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Serialize)]
pub struct SelfInfo {
    pub arch: &'static str,
    pub os: String,
    pub platform: &'static str,
    pub cores: usize,
    pub cpu_model: Option<String>,
    pub ram_total_mb: u64,
    pub ram_available_mb: u64,
}

pub fn platform() -> &'static str {
    if is_termux() {
        "android-termux"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

pub fn self_info() -> SelfInfo {
    let cpuinfo = read_file("/proc/cpuinfo").unwrap_or_default();
    let (procs, model) = parse_cpuinfo(&cpuinfo);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(procs.max(1));
    let (total, avail) = parse_meminfo(&read_file("/proc/meminfo").unwrap_or_default());
    let os = if is_termux() {
        "Android (Termux)".to_string()
    } else {
        read_file("/etc/os-release")
            .and_then(|s| {
                s.lines().find_map(|l| {
                    l.strip_prefix("PRETTY_NAME=")
                        .map(|v| v.trim_matches('"').to_string())
                })
            })
            .unwrap_or_else(|| std::env::consts::OS.to_string())
    };
    SelfInfo {
        arch: std::env::consts::ARCH,
        os,
        platform: platform(),
        cores,
        cpu_model: model,
        ram_total_mb: total / 1024,
        ram_available_mb: avail / 1024,
    }
}

/// (processor count, model name) from /proc/cpuinfo (x86 and ARM layouts).
pub fn parse_cpuinfo(s: &str) -> (usize, Option<String>) {
    let mut n = 0;
    let mut model = None;
    for l in s.lines() {
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "processor" => n += 1,
            "model name" | "Hardware" | "Model" if model.is_none() && !v.is_empty() => {
                model = Some(v.to_string())
            }
            _ => {}
        }
    }
    (n, model)
}

/// (MemTotal, MemAvailable) in KiB.
pub fn parse_meminfo(s: &str) -> (u64, u64) {
    let get = |key: &str| {
        s.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (get("MemTotal:"), get("MemAvailable:"))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ArpEntry {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub iface: String,
}

/// Complete entries of /proc/net/arp (Android 10+ usually denies this file).
pub fn parse_arp(s: &str) -> Vec<ArpEntry> {
    s.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 6 || f[2] == "0x0" || f[3] == "00:00:00:00:00:00" {
                return None;
            }
            Some(ArpEntry {
                ip: f[0].parse().ok()?,
                mac: f[3].to_ascii_lowercase(),
                iface: f[5].to_string(),
            })
        })
        .collect()
}

pub fn arp_table() -> Option<Vec<ArpEntry>> {
    read_file("/proc/net/arp").map(|s| parse_arp(&s))
}

/// Default IPv4 gateway (iface, ip) from /proc/net/route.
pub fn parse_route(s: &str) -> Option<(String, Ipv4Addr)> {
    for l in s.lines().skip(1) {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 3 || f[1] != "00000000" {
            continue;
        }
        let g = u32::from_str_radix(f[2], 16).ok()?;
        if g == 0 {
            continue;
        }
        return Some((f[0].to_string(), Ipv4Addr::from(g.to_le_bytes())));
    }
    None
}

pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpuinfo_x86_and_arm() {
        let x86 =
            "processor\t: 0\nmodel name\t: AMD EPYC\nprocessor\t: 1\nmodel name\t: AMD EPYC\n";
        assert_eq!(parse_cpuinfo(x86), (2, Some("AMD EPYC".into())));
        let arm =
            "processor\t: 0\nBogoMIPS\t: 38.40\nprocessor\t: 1\nHardware\t: Qualcomm SM8550\n";
        assert_eq!(parse_cpuinfo(arm), (2, Some("Qualcomm SM8550".into())));
    }

    #[test]
    fn meminfo() {
        assert_eq!(
            parse_meminfo("MemTotal:  16384000 kB\nMemFree: 1 kB\nMemAvailable: 8192000 kB\n"),
            (16_384_000, 8_192_000)
        );
    }

    #[test]
    fn arp() {
        let s = "IP address       HW type     Flags       HW address            Mask     Device\n\
                 192.168.1.1      0x1         0x2         AA:BB:CC:DD:EE:FF     *        wlan0\n\
                 192.168.1.7      0x1         0x0         00:00:00:00:00:00     *        wlan0\n";
        let v = parse_arp(s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].mac, "aa:bb:cc:dd:ee:ff");
        assert_eq!(v[0].ip, Ipv4Addr::new(192, 168, 1, 1));
    }

    #[test]
    fn route() {
        let s = "Iface\tDestination\tGateway \tFlags\n\
                 wlan0\t0001A8C0\t00000000\t0001\n\
                 wlan0\t00000000\t0101A8C0\t0003\n";
        assert_eq!(
            parse_route(s),
            Some(("wlan0".into(), Ipv4Addr::new(192, 168, 1, 1)))
        );
    }
}
