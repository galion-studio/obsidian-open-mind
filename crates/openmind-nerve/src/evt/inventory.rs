// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Inventory: everything running on the host, grouped (nerve, cortex, gaia,
//! hermes, console, sandbox, galion-os, swarm, infra, transient, user, other).
//!
//! Sources (read-only, run at start, on unit lifecycle events and on the
//! liveness heartbeat, never on a fast timer): `systemctl list-units
//! --type=service --all`, `ss -tlnpH`, `ps -eo ...`, `docker ps` if docker
//! exists. Parsers are pure functions so they are unit tested with VPS shapes.

use super::classify_unit;
use crate::util::run;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnitRow {
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub desc: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortRow {
    pub addr: String,
    pub port: u16,
    pub procs: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcRow {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    pub etimes: u64,
    pub comm: String,
    pub args: String,
}

pub fn parse_units(text: &str) -> Vec<UnitRow> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim_start_matches(['●', '*', ' ']);
            let mut it = l.split_whitespace();
            let unit = it.next()?.to_string();
            if !unit.ends_with(".service") {
                return None;
            }
            let (load, active, sub) = (
                it.next()?.to_string(),
                it.next()?.to_string(),
                it.next()?.to_string(),
            );
            Some(UnitRow {
                unit,
                load,
                active,
                sub,
                desc: it.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// `ss -tlnpH` (or with header) lines.
pub fn parse_ss(text: &str) -> Vec<PortRow> {
    let mut out: Vec<PortRow> = Vec::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let Some(i) = f.iter().position(|x| {
            x.contains(':')
                && x.rsplit(':')
                    .next()
                    .is_some_and(|p| p.parse::<u16>().is_ok())
        }) else {
            continue;
        };
        if f.first().is_some_and(|s| *s == "State") {
            continue;
        }
        let local = f[i];
        let Some((addr, port)) = local.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let mut procs = Vec::new();
        if let Some(u) = l.find("users:(") {
            for part in l[u..].split("((").flat_map(|s| s.split("),(")) {
                let part = part.trim_start_matches("users:(").trim_start_matches('(');
                let name = part.split('"').nth(1).unwrap_or("").to_string();
                let pid = part
                    .split("pid=")
                    .nth(1)
                    .and_then(|p| p.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|p| p.parse().ok());
                if let (false, Some(pid)) = (name.is_empty(), pid) {
                    procs.push((name, pid));
                }
            }
        }
        let addr = addr.to_string();
        if let Some(r) = out.iter_mut().find(|r| r.addr == addr && r.port == port) {
            for p in procs {
                if !r.procs.contains(&p) {
                    r.procs.push(p);
                }
            }
        } else {
            out.push(PortRow { addr, port, procs });
        }
    }
    out
}

/// `ps -eo pid=,ppid=,user=,etimes=,comm=,args=`
pub fn parse_ps(text: &str) -> Vec<ProcRow> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let user = it.next()?.to_string();
            let etimes = it.next()?.parse().ok()?;
            let comm = it.next()?.to_string();
            let args: String = it.collect::<Vec<_>>().join(" ").chars().take(160).collect();
            Some(ProcRow {
                pid,
                ppid,
                user,
                etimes,
                comm,
                args,
            })
        })
        .collect()
}

/// Process -> group.
pub fn classify_proc(p: &ProcRow) -> &'static str {
    let a = format!("{} {}", p.comm, p.args).to_ascii_lowercase();
    let c = p.comm.as_str();
    if p.ppid == 2 || p.pid == 2 {
        "kernel"
    } else if a.contains("openmind-nerve") || a.contains("galion-nerve") {
        "nerve"
    } else if a.contains("cortex") {
        "cortex"
    } else if a.contains("hermes") {
        "hermes"
    } else if a.contains("console-ide") || c == "galion-desk" {
        "console"
    } else if a.contains("sandbox") || p.user.starts_with("sbx") {
        "sandbox"
    } else if a.contains("galion-os") {
        "galion-os"
    } else if a.contains("/swarm/") || a.contains("slot-") {
        "swarm"
    } else if [
        "openmind-memory",
        "openmind-queue",
        "galion-one",
        "link-watch",
        "galion-health",
        "tuner",
        "tanga-serve",
        "galion-backup",
        "gaia",
    ]
    .iter()
    .any(|n| c.starts_with(n) || a.contains(&format!("/{n}")))
    {
        "gaia"
    } else if c.starts_with("systemd")
        || c.starts_with("(sd-pam")
        || [
            "init",
            "sshd",
            "cron",
            "dbus-daemon",
            "rsyslogd",
            "master",
            "qmgr",
            "pickup",
            "agetty",
            "fail2ban-server",
            "networkd-dispat",
            "cloudflared",
            "dockerd",
            "containerd",
            "containerd-shim",
            "tlsmgr",
            "anvil",
            "showq",
        ]
        .contains(&c)
    {
        "infra"
    } else {
        "user"
    }
}

/// Well-known loopback ports of the stack (2026-10-21 `ss -tlnp`).
pub fn port_hint(port: u16) -> Option<(&'static str, &'static str)> {
    Some(match port {
        8670 => ("nerve", "galion-nerve (live)"),
        8672 => ("nerve", "galion-nerve-shadow"),
        8673 => ("nerve", "nerve event-mode sandbox"),
        19120 => ("gaia", "POSEIDON openmind-queue"),
        8642 => ("hermes", "hermes-rs gateway"),
        8420 => ("gaia", "openmind-memory"),
        8091 => ("gaia", "galion-one text"),
        8002 => ("gaia", "galion-one mouth"),
        8650 => ("gaia", "tuner"),
        8660 => ("gaia", "galion-health"),
        8671 => ("gaia", "tanga-proto"),
        8646 => ("console", "galion-desk"),
        8080 => ("console", "console (vite)"),
        20241 => ("infra", "cloudflared metrics"),
        20242 => ("infra", "tuner-quick cloudflared metrics"),
        22 => ("infra", "ssh"),
        25 => ("infra", "postfix"),
        53 => ("infra", "systemd-resolved"),
        _ => return None,
    })
}

pub struct Snapshot {
    pub units: Vec<UnitRow>,
    pub ports: Vec<PortRow>,
    pub procs: Vec<ProcRow>,
    pub docker: Option<Vec<String>>,
    pub errors: Vec<String>,
}

/// Run the read-only commands.
pub fn discover() -> Snapshot {
    let t = Duration::from_secs(10);
    let mut errors = Vec::new();
    let mut get = |prog: &str, args: &[&str]| -> String {
        match run(prog, args, t) {
            Some(o) if o.code == 0 => o.stdout,
            Some(o) => {
                errors.push(format!("{prog}: exit {}", o.code));
                o.stdout
            }
            None => {
                errors.push(format!("{prog}: not runnable"));
                String::new()
            }
        }
    };
    let units = parse_units(&get(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--all",
            "--no-legend",
            "--plain",
            "--no-pager",
        ],
    ));
    let ports = parse_ss(&get("ss", &["-tlnpH"]));
    let procs = parse_ps(&get("ps", &["-eo", "pid=,ppid=,user=,etimes=,comm=,args="]));
    let docker = run(
        "docker",
        &["ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Status}}"],
        t,
    )
    .filter(|o| o.code == 0)
    .map(|o| o.stdout.lines().map(str::to_string).collect());
    Snapshot {
        units,
        ports,
        procs,
        docker,
        errors,
    }
}

impl Snapshot {
    fn port_group(&self, p: &PortRow, extra: &[(u16, String)]) -> (String, String) {
        if let Some((_, g)) = extra.iter().find(|(x, _)| *x == p.port) {
            return (g.clone(), String::new());
        }
        let hint = port_hint(p.port);
        if let Some((name, pid)) = p.procs.first() {
            if let Some(pr) = self.procs.iter().find(|x| x.pid == *pid) {
                return (
                    classify_proc(pr).into(),
                    hint.map(|h| h.1).unwrap_or(name).into(),
                );
            }
        }
        match hint {
            Some((g, n)) => (g.into(), n.into()),
            None => (
                "other".into(),
                p.procs.first().map(|x| x.0.clone()).unwrap_or_default(),
            ),
        }
    }

    /// Service units the journal source tracks for lifecycle events.
    pub fn lifecycle_units(&self, extra: &[String]) -> Vec<String> {
        let mut v: BTreeSet<String> = self
            .units
            .iter()
            .filter(|u| u.load == "loaded" && super::lifecycle_tracked(&u.unit))
            .map(|u| u.unit.clone())
            .collect();
        v.extend(extra.iter().cloned());
        v.into_iter().collect()
    }

    /// Main processes worth a pidfd (direct children of init in stack groups).
    pub fn pidfd_targets(&self, me: u32) -> Vec<(u32, String, String)> {
        self.procs
            .iter()
            .filter(|p| p.ppid == 1 && p.pid != me)
            .filter_map(|p| {
                let g = classify_proc(p);
                (["nerve", "cortex", "gaia", "hermes", "console", "galion-os"].contains(&g)
                    || p.comm == "cloudflared")
                    .then(|| (p.pid, p.comm.clone(), g.to_string()))
            })
            .take(48)
            .collect()
    }

    pub fn to_json(&self, extra: &[(u16, String)]) -> Value {
        let mut groups: BTreeMap<String, Value> = BTreeMap::new();
        let entry = |groups: &mut BTreeMap<String, Value>, g: &str| {
            groups
                .entry(g.to_string())
                .or_insert_with(|| json!({"units": [], "processes": [], "ports": []}));
        };
        for u in &self.units {
            if u.load == "not-found" || (u.load == "masked" && u.active != "active") {
                continue;
            }
            let g = classify_unit(&u.unit);
            entry(&mut groups, g);
            if let Some(a) = groups.get_mut(g).and_then(|x| x["units"].as_array_mut()) {
                a.push(json!({"unit": u.unit, "active": u.active, "sub": u.sub, "gaia": super::gaia_function(&u.unit)}));
            }
        }
        for p in &self.procs {
            let g = classify_proc(p);
            if g == "kernel" {
                continue;
            }
            entry(&mut groups, g);
            if let Some(a) = groups
                .get_mut(g)
                .and_then(|x| x["processes"].as_array_mut())
            {
                a.push(json!({"pid": p.pid, "user": p.user, "comm": p.comm, "etimes": p.etimes}));
            }
        }
        for p in &self.ports {
            let (g, name) = self.port_group(p, extra);
            entry(&mut groups, &g);
            if let Some(a) = groups.get_mut(&g).and_then(|x| x["ports"].as_array_mut()) {
                a.push(json!({"addr": p.addr, "port": p.port, "name": name, "procs": p.procs}));
            }
        }
        let mut counts = serde_json::Map::new();
        for (g, v) in groups.iter_mut() {
            let running = v["units"]
                .as_array()
                .map(|a| a.iter().filter(|u| u["sub"] == "running").count())
                .unwrap_or(0);
            let c = json!({"units": v["units"].as_array().map(Vec::len), "units_running": running,
                           "processes": v["processes"].as_array().map(Vec::len), "ports": v["ports"].as_array().map(Vec::len)});
            v["counts"] = c.clone();
            counts.insert(g.clone(), c);
        }
        json!({
            "ts": zagreb_time::now_rfc3339(),
            "totals": {"units": self.units.len(), "units_running": self.units.iter().filter(|u| u.sub == "running").count(),
                       "processes": self.procs.iter().filter(|p| classify_proc(p) != "kernel").count(), "listening": self.ports.len()},
            "counts": counts,
            "groups": groups,
            "docker": self.docker,
            "errors": self.errors,
        })
    }

    /// Fingerprint for change detection: running services and listening ports.
    pub fn fingerprint(&self) -> BTreeSet<String> {
        let mut s: BTreeSet<String> = self
            .units
            .iter()
            .filter(|u| u.sub == "running" && classify_unit(&u.unit) != "transient")
            .map(|u| format!("unit:{}", u.unit))
            .collect();
        s.extend(
            self.ports
                .iter()
                .map(|p| format!("port:{}:{}", p.addr, p.port)),
        );
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes from the VPS 2026-10-21 18:13 +02:00 (read-only probe; pids kept, no secrets).
    const SS: &str = r#"LISTEN 0 4096 127.0.0.1:19120 0.0.0.0:* users:(("openmind-queue",pid=17979,fd=6))
LISTEN 0 128 0.0.0.0:22 0.0.0.0:* users:(("sshd",pid=248,fd=3),("systemd",pid=1,fd=66))
LISTEN 0 4096 127.0.0.1:8670 0.0.0.0:* users:(("openmind-nerve-",pid=606906,fd=4))
LISTEN 0 4096 127.0.0.1:8672 0.0.0.0:* users:(("galion-nerve-sh",pid=4008991,fd=3))
LISTEN 0 4096 127.0.0.1:8080 0.0.0.0:* users:(("node",pid=2501884,fd=26))
LISTEN 0 4096 127.0.0.1:8642 0.0.0.0:* users:(("hermes-gateway-",pid=718450,fd=5))
LISTEN 0 4096 [::1]:25 [::]:*"#;
    const PS: &str = "      1       0 root           100 systemd /sbin/init
 606906       1 root           100 openmind-nerve- /opt/galion/rust/bin/openmind-nerve-aa77864 serve
4008991       1 root           100 galion-nerve-sh /opt/galion/rust/bin/galion-nerve-shadow serve
  17979       1 openmin+       100 openmind-queue /usr/local/bin/openmind-queue serve --bind 127.0.0.1:19120 --data /var/lib/openmind-queue
2501884 2501864 root           100 node node /opt/galion/console-ide/node_modules/vite/bin/vite.js dev --host 127.0.0.1 --port 8080
 718450       1 root           100 hermes-gateway- /opt/galion/rust/bin/hermes-gateway-aa77864
    248       1 root           100 sshd sshd: /usr/sbin/sshd -D [listener] 0 of 10-100 startups
 611846       1 root           100 cloudflared /usr/local/bin/cloudflared tunnel --no-autoupdate --config x run
 720043  719995 root             1 bash bash -c ss -tlnp";
    const UNITS: &str = "cloudflared.service loaded active running Cloudflare tunnel
galion-nerve.service loaded active running galion-nerve: Galion self-eval
galion-nerve-shadow.service loaded active running galion-nerve SHADOW
openmind-queue.service loaded active running openmind-queue (POSEIDON) work queue, 127.0.0.1:19120
hermes.service masked inactive dead hermes.service
console-screen.service not-found inactive dead console-screen.service
run-u11093.service loaded failed failed /usr/bin/systemctl is-active ssh.service
galion-backup.service loaded inactive dead Galion snapshot backup";

    #[test]
    fn parse_and_group() {
        let ports = parse_ss(SS);
        assert_eq!(ports.len(), 7);
        assert_eq!(
            ports[1].procs,
            vec![("sshd".to_string(), 248), ("systemd".to_string(), 1)]
        );
        assert!(ports[6].procs.is_empty());
        let procs = parse_ps(PS);
        assert_eq!(procs.len(), 9);
        let units = parse_units(UNITS);
        assert_eq!(units.len(), 8);
        let s = Snapshot {
            units,
            ports,
            procs,
            docker: None,
            errors: vec![],
        };
        let j = s.to_json(&[]);
        assert_eq!(j["counts"]["nerve"]["processes"], 2);
        assert_eq!(j["counts"]["nerve"]["ports"], 2);
        assert_eq!(j["counts"]["nerve"]["units"], 2);
        assert_eq!(j["counts"]["gaia"]["ports"], 1);
        assert_eq!(
            j["groups"]["gaia"]["ports"][0]["name"],
            "POSEIDON openmind-queue"
        );
        assert_eq!(j["counts"]["console"]["processes"], 1);
        assert_eq!(j["counts"]["hermes"]["processes"], 1);
        assert_eq!(j["counts"]["transient"]["units"], 1);
        assert_eq!(j["counts"]["user"]["processes"], 1);
        assert!(
            j["groups"]["hermes"]["units"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "masked+inactive hermes.service hidden"
        );
        let lu = s.lifecycle_units(&["nerve-evt-test.service".into()]);
        assert!(
            lu.contains(&"galion-nerve.service".to_string())
                && lu.contains(&"openmind-queue.service".to_string())
        );
        assert!(
            lu.contains(&"cloudflared.service".to_string())
                && lu.contains(&"nerve-evt-test.service".to_string())
        );
        assert!(!lu.iter().any(|u| u.starts_with("run-u")));
        let pt = s.pidfd_targets(606906);
        let pids: Vec<u32> = pt.iter().map(|x| x.0).collect();
        assert!(
            pids.contains(&17979)
                && pids.contains(&718450)
                && pids.contains(&611846)
                && pids.contains(&4008991)
        );
        assert!(!pids.contains(&606906) && !pids.contains(&248));
        assert!(s.fingerprint().contains("port:127.0.0.1:19120"));
    }
}
