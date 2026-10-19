// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! `omnode doctor`: one JSON report of what this device can do for the stack.
//! Runs no listeners and changes no system state (seccomp/namespace probes
//! fork a short-lived child that exits immediately).

use crate::config::Config;
use crate::identity::Identity;
use crate::sandbox::{self, Isolation};
use crate::seccomp;
use crate::sysinfo;
use crate::wifi;
use crate::zagreb;
use serde_json::{json, Value};

/// Probe unprivileged user namespaces without keeping one around.
fn userns_ok() -> bool {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return false;
    }
    if pid == 0 {
        let ok = unsafe { libc::unshare(libc::CLONE_NEWUSER) } == 0;
        unsafe { libc::_exit(if ok { 0 } else { 1 }) };
    }
    let mut st = 0;
    let r = unsafe { libc::waitpid(pid, &mut st, 0) };
    r == pid && libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0
}

fn read_sysctl(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Run a no-op under the sandbox and return the isolation report.
fn sandbox_probe() -> Isolation {
    let o = sandbox::Opts::from_cfg(&crate::config::SandboxCfg::default());
    sandbox::run(&["true".into()], &o)
        .map(|r| r.isolation)
        .unwrap_or_else(|e| Isolation {
            platform: sysinfo::platform(),
            notes: vec![format!("sandbox probe failed: {e}")],
            ..Default::default()
        })
}

pub fn run(cfg: &Config, id: Option<&Identity>) -> Value {
    let me = sysinfo::self_info();
    let obs = wifi::observe(true);
    let net = id.and_then(|i| i.current_network(cfg, &obs));
    let iso = sandbox_probe();
    let termux = crate::util::is_termux();

    let ssid_how = if obs.sources.contains(&"termux-wifi-connectioninfo") {
        "termux-wifi-connectioninfo"
    } else if obs.sources.contains(&"iw") {
        "iw"
    } else if obs.sources.contains(&"nmcli") {
        "nmcli"
    } else if obs.ssid.is_some() {
        "other"
    } else {
        "unavailable"
    };

    let mut advice: Vec<String> = Vec::new();
    if termux && !obs.ssid.is_some() {
        advice.push(
            "SSID unread: install Termux:API (F-Droid) and grant Location permission, then retry"
                .into(),
        );
    }
    if termux {
        advice.push(
            "Android drops Wi-Fi multicast for apps without a MulticastLock; Termux holds none, so passive mDNS/SSDP may see little. ARP and `discover --probe` still work on a trusted network.".into(),
        );
    }
    if !iso.net_namespace {
        advice.push(
            "No empty network namespace: sandbox cannot fully cut network. On Termux this is expected; on Linux check unprivileged user namespaces.".into(),
        );
    }
    if iso.landlock == "not-enforced" {
        advice.push(
            "Landlock unavailable: sandbox cannot restrict filesystem beyond the work dir.".into(),
        );
    }
    if iso.seccomp != "deny-list" {
        advice.push(
            "seccomp filter not active: dangerous syscalls are not blocked on this device.".into(),
        );
    }
    if id.is_none() {
        advice.push("No device identity yet: run `omnode identity init`.".into());
    } else if !net.as_ref().map(|n| n.2).unwrap_or(false) {
        advice.push(
            "Current network is not bound: run `omnode identity bind` on a Wi-Fi you own.".into(),
        );
    }

    json!({
        "generated": zagreb::now_rfc3339(),
        "self": me,
        "identity": {
            "present": id.is_some(),
            "device_id": id.map(|i| i.state.device_id.clone()),
            "fingerprint": id.map(|i| i.fingerprint()),
        },
        "wifi": {
            "observed": obs.public(),
            "ssid_readable": obs.ssid.is_some(),
            "ssid_via": ssid_how,
            "network_hash": net.as_ref().map(|n| n.0.clone()),
            "anchor": net.as_ref().map(|n| n.1),
            "trusted": net.as_ref().map(|n| n.2).unwrap_or(false),
        },
        "kernel": {
            "unprivileged_userns": userns_ok(),
            "userns_sysctl": read_sysctl("/proc/sys/kernel/unprivileged_userns_clone"),
            "apparmor_userns_sysctl": read_sysctl("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"),
            "lsm": read_sysctl("/sys/kernel/security/lsm"),
            "landlock_abi": sandbox::landlock_abi(),
            "seccomp_available": seccomp::available(),
            "arp_readable": sysinfo::arp_table().is_some(),
        },
        "sandbox": iso,
        "multicast": {
            "note": if termux {
                "Termux cannot take a MulticastLock; passive mDNS/SSDP is best-effort"
            } else {
                "Linux: UDP 5353/1900 multicast should work if the firewall allows it"
            },
            "termux": termux,
        },
        "advice": advice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_runs_without_identity() {
        let v = run(&Config::default(), None);
        assert!(v["generated"].as_str().unwrap().contains('+'));
        assert_eq!(v["identity"]["present"], false);
        assert!(v["sandbox"]["platform"].as_str().is_some());
        assert!(v["kernel"]["landlock_abi"].as_i64().is_some());
    }
}
