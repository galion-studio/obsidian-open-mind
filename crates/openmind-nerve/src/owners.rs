// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port ownership: which component (and Gaia function) owns a listening
//! port, and for ports nobody owns, which process holds them.
//!
//! The map is built from the component table (health and metrics ports),
//! the known infrastructure ports and `NERVE_PORT_OWNERS`
//! (`port=name[@GAIA],...`). Ports in `NERVE_UNOWNED_PORTS` (default: the five
//! VPS loopback listeners no unit owns on 2026-10-20, 8431-8433, 8652 and
//! 8653) and any other unmatched listener report as **"unknown owner"**,
//! with the holding process read from /proc (`/proc/net/tcp*` socket inode
//! -> `/proc/<pid>/fd` -> `comm` and the cgroup unit/scope) or, in a replay,
//! from `ss -ltnp` text. A `session-N.scope` unit means a process started
//! by hand from an ssh session (no systemd unit, no restart policy).
//! Read-only: the nerve never signals or kills these processes.

use crate::config::Spec;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// VPS loopback listeners with no owning unit (docs/real-tests/nerve.md).
pub const VPS_UNOWNED_PORTS: [u16; 5] = [8431, 8432, 8433, 8652, 8653];
pub const UNKNOWN: &str = "unknown owner";

/// An empty map, the neutral default of [`crate::cortex::Input::new`].
pub static EMPTY: OwnerMap = OwnerMap {
    owned: BTreeMap::new(),
    unowned: BTreeSet::new(),
};
/// No process info (replays and tests without `ss -ltnp` text).
pub static NO_PROCS: BTreeMap<u16, ProcInfo> = BTreeMap::new();

/// The process holding a socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub comm: String,
    /// systemd unit or scope from /proc/<pid>/cgroup, when readable.
    pub unit: Option<String>,
}

impl ProcInfo {
    pub fn to_json(&self) -> Value {
        json!({"pid": self.pid, "process": self.comm, "unit": self.unit,
               "manual": self.unit.as_deref().is_some_and(|u| u.starts_with("session-") && u.ends_with(".scope"))})
    }

    pub fn describe(&self) -> String {
        let mut s = format!("process {} (pid {})", self.comm, self.pid);
        if let Some(u) = &self.unit {
            s.push_str(&format!(", {u}"));
            if u.starts_with("session-") && u.ends_with(".scope") {
                s.push_str(": started by hand from a login session, no unit");
            }
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub owner: String,
    pub gaia: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct OwnerMap {
    pub owned: BTreeMap<u16, Entry>,
    /// Documented "no owner" ports (always reported as unknown owner).
    pub unowned: BTreeSet<u16>,
}

/// One port's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Owner {
    pub port: u16,
    pub owner: String,
    pub gaia: Option<String>,
    /// In the documented unowned list.
    pub documented_unowned: bool,
    pub process: Option<ProcInfo>,
}

impl Owner {
    pub fn is_unknown(&self) -> bool {
        self.owner == UNKNOWN
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({"port": self.port, "owner": self.owner, "gaia": self.gaia,
                           "documented_unowned": self.documented_unowned});
        if let Some(p) = &self.process {
            for (k, x) in p.to_json().as_object().into_iter().flatten() {
                v[k] = x.clone();
            }
        }
        v
    }
}

/// Parse `port=name[@GAIA],...` (bad entries are skipped).
pub fn parse_owners(s: &str) -> Vec<(u16, Entry)> {
    s.split(',')
        .filter_map(|x| {
            let (p, rest) = x.trim().split_once('=')?;
            let (name, gaia) = match rest.split_once('@') {
                Some((n, g)) => (n.trim(), Some(g.trim().to_string())),
                None => (rest.trim(), None),
            };
            (!name.is_empty()).then_some((
                p.trim().parse().ok()?,
                Entry {
                    owner: name.to_string(),
                    gaia,
                },
            ))
        })
        .collect()
}

/// Galion's own loopback services that are not nerve components (no
/// health contract, no reflex restarts) but must never be reported as
/// unmanaged/stray listeners. Read-only `ss -ltnp` + `/proc/<pid>/cgroup` on
/// the VPS, 2026-10-22 01:08 +02:00 (slot 238, D-T44-2): 8672
/// galion-nerve-shadow.service, 8674 gaia-cortex.service, 19120
/// openmind-queue.service. Always merged (independent of
/// `NERVE_KNOWN_PORTS`); `NERVE_UNOWNED_PORTS` or `NERVE_PORT_OWNERS` still
/// override. Not listed: :18800 is `sshd` in a `session-*.scope` (a manual
/// ssh port forward, no unit), so it stays a stray (D-233-3, owner's call).
pub const OWN_SERVICES: [(u16, &str, Option<&str>); 3] = [
    (8672, "galion-nerve-shadow", Some("AETHER")),
    (8674, "gaia-cortex", Some("AETHER")),
    (19120, "openmind-queue", Some("POSEIDON")),
];

/// Labels for the built-in known infrastructure ports.
const KNOWN_LABELS: [(u16, &str, Option<&str>); 7] = [
    (22, "sshd", None),
    (25, "postfix", None),
    (53, "systemd-resolved", None),
    (8670, "galion-nerve", Some("AETHER")),
    (8671, "tanga-serve", None),
    (20241, "cloudflared metrics", Some("AETHER")),
    (20242, "tuner-quick metrics", Some("AETHER")),
];

impl OwnerMap {
    /// Own services ([`OWN_SERVICES`]) first, then known labels, then component
    /// ports, then `extra` (overrides); `unowned` wins over all.
    pub fn build(specs: &[Spec], known: &[u16], extra: &[(u16, Entry)], unowned: &[u16]) -> Self {
        let mut m = OwnerMap::default();
        for (p, owner, gaia) in OWN_SERVICES {
            m.owned.insert(
                p,
                Entry {
                    owner: owner.to_string(),
                    gaia: gaia.map(str::to_string),
                },
            );
        }
        for &p in known {
            let (owner, gaia) = KNOWN_LABELS
                .iter()
                .find(|k| k.0 == p)
                .map(|k| (k.1.to_string(), k.2.map(str::to_string)))
                .unwrap_or_else(|| ("known port (NERVE_KNOWN_PORTS)".into(), None));
            m.owned.insert(p, Entry { owner, gaia });
        }
        for s in specs {
            for p in [s.port, s.metrics_port].into_iter().flatten() {
                m.owned.insert(
                    p,
                    Entry {
                        owner: s.name.to_string(),
                        gaia: Some(s.gaia.to_string()),
                    },
                );
            }
        }
        for (p, e) in extra {
            m.owned.insert(*p, e.clone());
        }
        for p in unowned {
            m.owned.remove(p);
            m.unowned.insert(*p);
        }
        m
    }

    /// The map for a running config (component table + env).
    pub fn for_config(specs: &[Spec], cfg: &crate::config::Config) -> Self {
        let mut known = cfg.known_ports.clone();
        known.push(cfg.addr.port());
        known.extend(cfg.public_ok_ports.iter().copied());
        Self::build(specs, &known, &cfg.port_owners, &cfg.unowned_ports)
    }

    pub fn lookup(&self, port: u16, procs: &BTreeMap<u16, ProcInfo>) -> Owner {
        let process = procs.get(&port).cloned();
        match self.owned.get(&port) {
            Some(e) if !self.unowned.contains(&port) => Owner {
                port,
                owner: e.owner.clone(),
                gaia: e.gaia.clone(),
                documented_unowned: false,
                process,
            },
            _ => Owner {
                port,
                owner: UNKNOWN.into(),
                gaia: None,
                documented_unowned: self.unowned.contains(&port),
                process,
            },
        }
    }

    pub fn is_owned(&self, port: u16) -> bool {
        self.owned.contains_key(&port) && !self.unowned.contains(&port)
    }
}

/// Listening socket inode -> port, from `/proc/net/tcp` text (state 0A).
pub fn parse_listen_inodes(text: &str) -> BTreeMap<u64, u16> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.get(3) != Some(&"0A") {
                return None;
            }
            let port = u16::from_str_radix(f.get(1)?.rsplit_once(':')?.1, 16).ok()?;
            let inode: u64 = f.get(9)?.parse().ok()?;
            (inode != 0).then_some((inode, port))
        })
        .collect()
}

/// The unit or scope name from `/proc/<pid>/cgroup` text (v2 or v1 lines).
pub fn parse_cgroup_unit(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| l.splitn(3, ':').nth(2))
        .flat_map(|p| p.rsplit('/'))
        .find(|seg| seg.ends_with(".service") || seg.ends_with(".scope"))
        .map(str::to_string)
}

/// Owners of `ss -ltnp` lines: `users:(("name",pid=N,fd=M))` per port.
pub fn parse_ss_procs(text: &str) -> BTreeMap<u16, ProcInfo> {
    let mut m = BTreeMap::new();
    for l in text.lines().filter(|l| l.starts_with("LISTEN")) {
        let Some(port) = l
            .split_whitespace()
            .nth(3)
            .and_then(|a| a.rsplit_once(':'))
            .and_then(|(_, p)| p.parse::<u16>().ok())
        else {
            continue;
        };
        let Some(u) = l.split("users:((\"").nth(1) else {
            continue;
        };
        let Some((comm, rest)) = u.split_once('"') else {
            continue;
        };
        let pid = rest
            .split("pid=")
            .nth(1)
            .and_then(|r| r.split([',', ')']).next())
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        m.entry(port).or_insert(ProcInfo {
            pid,
            comm: comm.to_string(),
            unit: None,
        });
    }
    m
}

/// Holders of the listening sockets on `ports`, read from /proc. Only
/// processes whose fds we may read are found (the live nerve runs as root;
/// on a shared box only our own). Bounded: at most 4096 pids scanned.
pub fn socket_procs(ports: &BTreeSet<u16>) -> BTreeMap<u16, ProcInfo> {
    let mut out = BTreeMap::new();
    if ports.is_empty() {
        return out;
    }
    let mut inodes = BTreeMap::new();
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(t) = std::fs::read_to_string(f) {
            inodes.extend(
                parse_listen_inodes(&t)
                    .into_iter()
                    .filter(|(_, p)| ports.contains(p)),
            );
        }
    }
    if inodes.is_empty() {
        return out;
    }
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten().take(4096) {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            let t = target.to_string_lossy();
            let Some(ino) = t
                .strip_prefix("socket:[")
                .and_then(|r| r.strip_suffix(']'))
                .and_then(|r| r.parse::<u64>().ok())
            else {
                continue;
            };
            if let Some(port) = inodes.get(&ino) {
                if out.contains_key(port) {
                    continue;
                }
                let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| "?".into());
                let unit = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
                    .ok()
                    .and_then(|c| parse_cgroup_unit(&c));
                out.insert(*port, ProcInfo { pid, comm, unit });
            }
        }
        if out.len() >= inodes.values().collect::<BTreeSet<_>>().len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;

    #[test]
    fn vps_unowned_ports_report_unknown_owner() {
        let specs = default_specs(None, None);
        let m = OwnerMap::build(&specs, &[25, 53, 8670, 20241], &[], &VPS_UNOWNED_PORTS);
        let ss = "LISTEN 0 128 127.0.0.1:8652 0.0.0.0:* users:((\"hermes-gateway\",pid=1395619,fd=4))\nLISTEN 0 128 127.0.0.1:8431 0.0.0.0:* users:((\"openmind-memory\",pid=1375169,fd=6))\nLISTEN 0 128 127.0.0.1:8420 0.0.0.0:* users:((\"openmind-memory\",pid=99,fd=6))\n";
        let procs = parse_ss_procs(ss);
        let o = m.lookup(8652, &procs);
        assert!(o.is_unknown() && o.documented_unowned);
        assert_eq!(o.process.as_ref().unwrap().comm, "hermes-gateway");
        assert_eq!(o.process.as_ref().unwrap().pid, 1395619);
        for p in VPS_UNOWNED_PORTS {
            assert!(m.lookup(p, &procs).is_unknown(), "{p}");
            assert!(!m.is_owned(p));
        }
        let mem = m.lookup(8420, &procs);
        assert_eq!(
            (mem.owner.as_str(), mem.gaia.as_deref()),
            ("openmind-memory", Some("APOLLO"))
        );
        let n = m.lookup(8670, &procs);
        assert_eq!(n.owner, "galion-nerve");
        assert!(m.lookup(9999, &procs).is_unknown());
        assert!(!m.lookup(9999, &procs).documented_unowned);
        let j = o.to_json();
        assert_eq!(j["owner"], "unknown owner");
        assert_eq!(j["process"], "hermes-gateway");
    }

    #[test]
    fn env_overrides_and_parsers() {
        let e = parse_owners("19305=test-shadow@HEPHAESTUS, 8671=tanga ,bad,=x");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].1.gaia.as_deref(), Some("HEPHAESTUS"));
        let m = OwnerMap::build(&[], &[8671], &e, &[]);
        assert_eq!(m.lookup(8671, &BTreeMap::new()).owner, "tanga");
        let t = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:21FC 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 424242 1 0000\n   1: 0100007F:21FC 0100007F:1234 01 00000000:00000000 00:00000000 00000000  1000        0 5 1 0000\n";
        let i = parse_listen_inodes(t);
        assert_eq!(i.get(&424242), Some(&0x21FC));
        assert_eq!(i.len(), 1);
        assert_eq!(
            parse_cgroup_unit("0::/user.slice/user-0.slice/session-48893.scope\n").as_deref(),
            Some("session-48893.scope")
        );
        assert_eq!(
            parse_cgroup_unit("0::/system.slice/hermes-rs.service\n").as_deref(),
            Some("hermes-rs.service")
        );
        let p = ProcInfo {
            pid: 1,
            comm: "x".into(),
            unit: Some("session-1.scope".into()),
        };
        assert!(p.describe().contains("by hand"));
        assert_eq!(p.to_json()["manual"], true);
    }

    #[test]
    fn own_services_are_owned_unless_documented_unowned() {
        // Even with no known ports and no components (NERVE_KNOWN_PORTS set).
        let m = OwnerMap::build(&[], &[], &[], &[]);
        for (p, name, gaia) in OWN_SERVICES {
            let o = m.lookup(p, &NO_PROCS);
            assert!(m.is_owned(p), "{p}");
            assert_eq!((o.owner.as_str(), o.gaia.as_deref()), (name, gaia));
        }
        // :18800 (sshd ssh forward) is not an own service.
        assert!(!m.is_owned(18800));
        // NERVE_PORT_OWNERS relabels, NERVE_UNOWNED_PORTS wins.
        let m = OwnerMap::build(&[], &[], &parse_owners("19120=queue-v2@POSEIDON"), &[8674]);
        assert_eq!(m.lookup(19120, &NO_PROCS).owner, "queue-v2");
        assert!(!m.is_owned(8674));
        assert!(m.lookup(8674, &NO_PROCS).documented_unowned);
        assert!(m.is_owned(8672));
    }

    #[test]
    fn own_listener_is_found_in_proc() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let got = socket_procs(&[port].into_iter().collect());
        match got.get(&port) {
            Some(p) => assert_eq!(p.pid, std::process::id()),
            None => eprintln!("SKIP: /proc/<pid>/fd not readable here"),
        }
    }
}
