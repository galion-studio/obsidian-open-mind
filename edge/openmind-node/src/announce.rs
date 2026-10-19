// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Advertise this node as `_openmind._tcp` over mDNS (crate mdns-sd).
//! This opens a UDP 5353 multicast socket: run it only on the owner's LAN
//! device, never on the VPS. `--dry-run` prints the record and opens nothing.

use crate::config::Config;
use crate::identity::Identity;
use crate::sysinfo;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub fn txt_records(cfg: &Config, id: &Identity) -> Vec<(String, String)> {
    let me = sysinfo::self_info();
    let mut v = vec![
        ("v".to_string(), "1".to_string()),
        ("id".into(), id.state.device_id.clone()),
        ("fp".into(), id.fingerprint()),
        ("arch".into(), me.arch.to_string()),
        ("cores".into(), me.cores.to_string()),
        ("ram_mb".into(), me.ram_total_mb.to_string()),
        ("platform".into(), me.platform.to_string()),
        ("caps".into(), cfg.node.capabilities.join(",")),
    ];
    if let Some(n) = &cfg.node.name {
        v.push(("name".into(), n.chars().take(48).collect()));
    }
    v
}

pub fn record(cfg: &Config, id: &Identity) -> Value {
    json!({
        "service_type": cfg.announce.service_type,
        "instance": id.state.device_id,
        "host": format!("{}.local.", id.state.device_id),
        "port": cfg.announce.port,
        "txt": txt_records(cfg, id).into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(),
        "note": "network hash and raw SSID are never advertised",
    })
}

pub fn run(cfg: &Config, id: &Identity, seconds: u64) -> Result<(), String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns: {e}"))?;
    let host = format!("{}.local.", id.state.device_id);
    let props = txt_records(cfg, id);
    let props: Vec<(&str, &str)> = props
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let info = mdns_sd::ServiceInfo::new(
        &cfg.announce.service_type,
        &id.state.device_id,
        &host,
        "",
        cfg.announce.port,
        &props[..],
    )
    .map_err(|e| format!("mdns record: {e}"))?
    .enable_addr_auto();
    let full = info.get_fullname().to_string();
    daemon
        .register(info)
        .map_err(|e| format!("mdns register: {e}"))?;
    eprintln!(
        "announcing {full} ({}); Ctrl-C to stop",
        if seconds == 0 {
            "until stopped".to_string()
        } else {
            format!("{seconds}s")
        }
    );
    let start = Instant::now();
    while seconds == 0 || start.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_millis(200));
    }
    if let Ok(rx) = daemon.unregister(&full) {
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
    let _ = daemon.shutdown();
    Ok(())
}
