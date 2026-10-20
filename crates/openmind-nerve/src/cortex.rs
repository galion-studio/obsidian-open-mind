// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The cortex: what the nerve's signals mean for the stack as a whole.
//!
//! main.md section 3: the system must know in real time about
//! vulnerabilities, where it is "bleeding", and about DDoS attacks.
//!
//! - **vulnerabilities**: public listeners that should not be public,
//!   unmanaged listeners no monitored unit owns (stray manual processes),
//!   scanner/exploit probes seen in logs, dev servers serving production.
//! - **bleeding**: where requests, memory or uptime leak away: tunnel routes
//!   to dead origin ports, error rates, restart loops, rising memory, host
//!   pressure, a dependency stuck "loading".
//! - **ddos**: the request-rate anomaly verdict per stream ([`crate::access`]).
//!
//! Each finding says what was seen (evidence), why it matters and the next
//! step. The nerve never acts on cortex findings by itself: every fix here
//! touches config, DNS or units and needs the owner.

use crate::access::DdosVerdict;
use crate::config::Kind;
use crate::incident::{Sev, Signal};
use crate::logscan::LogInsights;
use crate::probe::Listener;
use crate::state::{CompState, Status};
use crate::util::kib;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub id: String,
    pub sev: Sev,
    pub component: String,
    pub title: String,
    pub evidence: Vec<String>,
    pub fix: String,
    /// Incident cause this finding is part of (scored there, not twice).
    pub cause: Option<String>,
}

impl Finding {
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "severity": self.sev.as_str(), "component": self.component, "title": self.title,
               "evidence": self.evidence, "fix": self.fix, "incident_cause": self.cause})
    }
}

pub struct Input<'a> {
    pub now: u64,
    pub comps: &'a [CompState],
    pub logs: &'a LogInsights,
    pub listeners: Option<&'a [Listener]>,
    pub ddos: &'a [DdosVerdict],
    pub known_ports: &'a [u16],
    pub public_ok_ports: &'a [u16],
    /// Only listeners in this inclusive range are judged (None: all).
    pub listen_scope: Option<(u16, u16)>,
    pub own_port: u16,
    pub loading_grace_s: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Cortex {
    pub vulnerabilities: Vec<Finding>,
    pub bleeding: Vec<Finding>,
    pub ddos: Vec<Value>,
    pub ddos_firing: bool,
    pub signals: Vec<Signal>,
    pub exposure: Value,
}

impl Cortex {
    pub fn to_json(&self) -> Value {
        json!({
            "vulnerabilities": self.vulnerabilities.iter().map(Finding::to_json).collect::<Vec<_>>(),
            "bleeding": self.bleeding.iter().map(Finding::to_json).collect::<Vec<_>>(),
            "ddos": {"firing": self.ddos_firing, "streams": self.ddos},
            "exposure": self.exposure,
        })
    }
}

fn dur(s: u64) -> String {
    if s >= 86_400 {
        format!("{:.1} d", s as f64 / 86_400.0)
    } else if s >= 3600 {
        format!("{:.1} h", s as f64 / 3600.0)
    } else if s >= 60 {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

/// Seconds a non-healthy component has been in its state, if it looks stuck
/// "loading" (503 with a loading body) for longer than `grace`.
pub fn stuck_loading(c: &CompState, now: u64, grace: u64) -> Option<u64> {
    let s = c.latest()?;
    let loading = s.http_code == Some(503)
        && (s.extra.get("mouth").is_some()
            || s.extra
                .get("health_body")
                .and_then(|b| b.get("status"))
                .and_then(Value::as_str)
                == Some("loading"));
    let age = now.saturating_sub(c.status_since);
    (loading && c.status != Status::Healthy && c.status_since > 0 && age > grace).then_some(age)
}

/// Severity of a component's verdict as an incident signal.
pub fn comp_sev(c: &CompState, now: u64, grace: u64) -> Option<Sev> {
    let base = match c.status {
        Status::Healthy | Status::Absent => return None,
        Status::Failing if c.spec.critical => Sev::Critical,
        Status::Failing | Status::FailingSoon => Sev::Error,
        Status::Degrading => Sev::Warn,
    };
    Some(if stuck_loading(c, now, grace).is_some() {
        base.max(Sev::Error)
    } else {
        base
    })
}

/// Component verdicts as incident signals (cause = the component's group).
pub fn component_signals(comps: &[CompState], now: u64, grace: u64) -> Vec<Signal> {
    let mut v = vec![];
    for c in comps {
        let Some(sev) = comp_sev(c, now, grace) else {
            continue;
        };
        let mut msg = if c.reasons.is_empty() {
            c.status.as_str().to_string()
        } else {
            c.reasons.join("; ")
        };
        if let Some(age) = stuck_loading(c, now, grace) {
            msg.push_str(&format!(" -- stuck loading for {}", dur(age)));
        }
        if let Some(b) = c.latest().and_then(|s| s.extra.get("health_body")) {
            msg.push_str(&format!(" [health body {b}]"));
        }
        let mut s = Signal::new(c.spec.group, c.spec.name, sev, msg);
        s.hard = c.hard_down;
        v.push(s);
    }
    v
}

pub fn analyse(i: &Input) -> Cortex {
    let mut cx = Cortex::default();
    let spec_ports: BTreeSet<u16> = i
        .comps
        .iter()
        .flat_map(|c| [c.spec.port, c.spec.metrics_port])
        .flatten()
        .collect();

    // ---- vulnerabilities: listeners
    if let Some(ls) = i.listeners {
        let in_scope = |p: u16| i.listen_scope.is_none_or(|(a, b)| p >= a && p <= b);
        let mut public_ok = vec![];
        let mut unmanaged: Vec<&Listener> = vec![];
        for l in ls.iter().filter(|l| in_scope(l.port)) {
            if !l.loopback {
                if i.public_ok_ports.contains(&l.port) {
                    public_ok.push(format!("{}:{}", l.addr, l.port));
                } else {
                    cx.vulnerabilities.push(Finding {
                        id: format!("public-listener-{}", l.port),
                        sev: Sev::Error,
                        component: "host".into(),
                        title: format!("port {} listens on {} (not loopback)", l.port, l.addr),
                        evidence: vec![format!("LISTEN {}:{}", l.addr, l.port)],
                        fix: "bind it to 127.0.0.1 and publish through the tunnel, or firewall it (owner approval)".into(),
                        cause: None,
                    });
                }
            } else if !spec_ports.contains(&l.port)
                && !i.known_ports.contains(&l.port)
                && l.port != i.own_port
            {
                unmanaged.push(l);
            }
        }
        let mut ports: Vec<u16> = unmanaged.iter().map(|l| l.port).collect();
        ports.sort();
        ports.dedup();
        if !ports.is_empty() {
            cx.vulnerabilities.push(Finding {
                id: "unmanaged-listeners".into(),
                sev: Sev::Warn,
                component: "host".into(),
                title: format!(
                    "{} loopback listener(s) no monitored unit owns: {}",
                    ports.len(),
                    ports.iter().map(|p| format!(":{p}")).collect::<Vec<_>>().join(" ")
                ),
                evidence: vec![
                    "not a component health/metrics port and not in NERVE_KNOWN_PORTS".into(),
                    "typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching)".into(),
                ],
                fix: "identify the owners (ss -ltnp), then stop them or give them a unit (owner approval; the nerve never kills processes)".into(),
                cause: None,
            });
        }
        cx.exposure = json!({"public_expected": public_ok, "unmanaged_ports": ports,
                             "listeners_judged": ls.iter().filter(|l| in_scope(l.port)).count()});
    } else {
        cx.exposure = json!({"note": "listeners not read this cycle"});
    }

    // ---- vulnerabilities: scanners and dev servers
    let mut by_unit: std::collections::BTreeMap<&str, Vec<(&str, u32)>> = Default::default();
    for s in i.logs.scans.values() {
        by_unit
            .entry(s.unit.as_str())
            .or_default()
            .push((s.path.as_str(), s.count));
    }
    for (unit, paths) in &by_unit {
        let dev = i.logs.dev_servers.contains(*unit);
        let n: u32 = paths.iter().map(|p| p.1).sum();
        let comp = comp_of_unit(i.comps, unit);
        let mut ev: Vec<String> = paths
            .iter()
            .take(8)
            .map(|(p, c)| format!("{p} x{c}"))
            .collect();
        if dev {
            ev.push("the unit prints Vite dev-server messages: a dev server is answering internet traffic".into());
        }
        cx.vulnerabilities.push(Finding {
            id: format!("scanner-probes-{comp}"),
            sev: if dev { Sev::Error } else { Sev::Warn },
            component: comp.clone(),
            title: format!("{n} scanner/exploit request(s) reached {comp} ({} distinct paths)", paths.len()),
            evidence: ev,
            fix: if dev {
                "serve a production build (B61) instead of the dev server, and deny dotfiles (.env, .git) at the edge".into()
            } else {
                "make sure those paths 404 and nothing sensitive sits under the web root".into()
            },
            cause: Some(format!("scan:{comp}")),
        });
        cx.signals.push(Signal::new(
            &format!("scan:{comp}"),
            &comp,
            Sev::Warn,
            format!(
                "{n} scanner request(s): {}",
                paths
                    .iter()
                    .take(4)
                    .map(|p| p.0)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    for unit in &i.logs.dev_servers {
        if by_unit.contains_key(unit.as_str()) {
            continue;
        }
        let comp = comp_of_unit(i.comps, unit);
        cx.vulnerabilities.push(Finding {
            id: format!("dev-server-{comp}"),
            sev: Sev::Warn,
            component: comp.clone(),
            title: format!("{comp} runs a Vite dev server"),
            evidence: vec![format!("{unit} logs Vite dev-server messages")],
            fix: "switch to a production build (B61)".into(),
            cause: None,
        });
    }

    // ---- bleeding: edge (tunnel routes to dead origins)
    if !i.logs.dead_origins.is_empty() {
        let mut hosts: BTreeSet<&str> = BTreeSet::new();
        let mut failed = 0u32;
        let mut ev = vec![];
        let mut ports = vec![];
        for d in i.logs.dead_origins.values() {
            failed += d.failed_requests;
            hosts.extend(d.hosts.iter().map(String::as_str));
            ports.push(d.port);
            ev.push(format!(
                "origin {} (port {}) refused {} dial(s), {} failed request(s); hosts: {}",
                if d.service.is_empty() {
                    "?"
                } else {
                    d.service.as_str()
                },
                d.port,
                d.refusals,
                d.failed_requests,
                if d.hosts.is_empty() {
                    "-".to_string()
                } else {
                    d.hosts.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            ));
        }
        let listening: Vec<u16> = i
            .listeners
            .map(|ls| {
                ports
                    .iter()
                    .copied()
                    .filter(|p| ls.iter().any(|l| l.port == *p))
                    .collect()
            })
            .unwrap_or_default();
        if i.listeners.is_some() {
            ev.push(if listening.is_empty() {
                "none of these ports has a listener on the host: the routes point at services that no longer run".into()
            } else {
                format!("ports with a listener now: {listening:?} (they may be flapping)")
            });
        }
        let span = i
            .logs
            .window_last
            .saturating_sub(i.logs.window_first)
            .max(60);
        let per_min = failed as f64 * 60.0 / span as f64;
        if let Some(cf) = i
            .comps
            .iter()
            .find(|c| c.spec.kind == Kind::Tunnel && c.spec.critical)
        {
            if let Some(s) = cf.latest() {
                if let (Some(r), Some(e)) = (s.tunnel_requests, s.tunnel_req_errors) {
                    ev.push(format!(
                        "cloudflared counters since its start: {e} of {r} requests failed ({:.0}%)",
                        e as f64 * 100.0 / r.max(1) as f64
                    ));
                }
            }
        }
        let title = format!(
            "tunnel routes {} public hostname(s) to {} closed origin port(s) {}: {:.1} failed requests/min over {}",
            hosts.len(),
            ports.len(),
            ports.iter().map(|p| format!(":{p}")).collect::<Vec<_>>().join(" "),
            per_min,
            dur(span)
        );
        let sev = if per_min >= 1.0 {
            Sev::Error
        } else {
            Sev::Warn
        };
        cx.bleeding.push(Finding {
            id: "edge-dead-origins".into(),
            sev,
            component: "cloudflared".into(),
            title: title.clone(),
            evidence: ev,
            fix: "either start the services behind those ports or remove the ingress rules / DNS for those hostnames (cloudflared config + DNS: owner approval; the nerve never touches them)".into(),
            cause: Some("edge".into()),
        });
        cx.signals
            .push(Signal::new("edge", "cloudflared", sev, title));
    }

    // ---- bleeding: per component
    for c in i.comps {
        let Some(s) = c.latest() else { continue };
        let name = c.spec.name;
        let group = Some(c.spec.group.to_string());
        let mut push = |id: &str, sev: Sev, title: String, ev: Vec<String>, fix: &str| {
            cx.bleeding.push(Finding {
                id: format!("{id}-{name}"),
                sev,
                component: name.into(),
                title,
                evidence: ev,
                fix: fix.into(),
                cause: group.clone(),
            })
        };
        if let Some(age) = stuck_loading(c, i.now, i.loading_grace_s) {
            let body = s
                .extra
                .get("health_body")
                .map(|b| b.to_string())
                .unwrap_or_else(|| "model loading".into());
            push("stuck-loading", Sev::Error,
                format!("{name} has answered 503 \"loading\" for {} (since {})", dur(age), crate::util::ts(c.status_since)),
                vec![format!("health :{}{} -> 503 {body}", c.spec.port.unwrap_or(0), c.spec.path),
                     format!("a model load takes minutes, not {}: the loader is stuck or the model is missing", dur(age))],
                "check the voice/model process logs and the model files, then restart that one service (owner approval)");
        }
        let r1h = c.restarts_1h(i.now);
        if r1h >= 1 {
            push(
                "restarts",
                if r1h >= 3 { Sev::Error } else { Sev::Warn },
                format!("{name} restarted {r1h} time(s) in the last hour"),
                vec![format!("systemd NRestarts={}", s.n_restarts)],
                "read the unit journal before the restart",
            );
        }
        for r in &c.reasons {
            if r.contains("memory rising") || r.contains("memory limit reached") {
                push(
                    "memory-trend",
                    Sev::Warn,
                    format!("{name}: {r}"),
                    vec![],
                    "look for a leak; a restart only resets it",
                );
            }
            if r.contains("errors in 5 min") {
                push(
                    "error-rate",
                    Sev::Warn,
                    format!("{name}: {r}"),
                    vec![],
                    "read the unit journal",
                );
            }
            if c.spec.kind == Kind::Host && (r.contains("disk") || r.contains("RAM")) {
                push(
                    "host-pressure",
                    Sev::Warn,
                    format!("host: {r}"),
                    vec![],
                    "free space / memory",
                );
            }
        }
        if let Some(m) = s.mem_kb {
            if m > 512 * 1024 && c.spec.kind == Kind::Service {
                push(
                    "memory-heavy",
                    Sev::Info,
                    format!("{name} holds {}", kib(m)),
                    vec!["largest resident service on the host".into()],
                    "expected for a Node dev server; a production build would use a fraction",
                );
            }
        }
    }

    // ---- DDoS
    for v in i.ddos {
        cx.ddos.push(v.to_json());
        let target = v.component.clone().unwrap_or_else(|| v.stream.clone());
        match v.level {
            "flood" => {
                cx.ddos_firing = true;
                cx.signals.push(Signal::new(
                    &format!("ddos:{}", v.stream),
                    &target,
                    Sev::Critical,
                    format!("request flood: {}", v.explain()),
                ));
            }
            "elevated" => cx.signals.push(Signal::new(
                &format!("ddos:{}", v.stream),
                &target,
                Sev::Warn,
                format!("request rate elevated: {}", v.explain()),
            )),
            _ => {}
        }
    }
    cx.vulnerabilities
        .sort_by(|a, b| b.sev.cmp(&a.sev).then(a.id.cmp(&b.id)));
    cx.bleeding
        .sort_by(|a, b| b.sev.cmp(&a.sev).then(a.id.cmp(&b.id)));
    cx
}

fn comp_of_unit(comps: &[CompState], unit: &str) -> String {
    comps
        .iter()
        .find(|c| c.spec.unit == Some(unit))
        .map(|c| c.spec.name.to_string())
        .unwrap_or_else(|| unit.trim_end_matches(".service").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;
    use crate::probe::JLine;
    use crate::state::{Sample, Thresholds};

    fn comps() -> Vec<CompState> {
        default_specs(None, None)
            .into_iter()
            .map(CompState::new)
            .collect()
    }

    #[test]
    fn vps_like_picture() {
        let th = Thresholds::default();
        let mut cs = comps();
        let now = 1_792_520_000;
        for c in cs.iter_mut() {
            let mut s = Sample {
                t: now,
                present: true,
                extra: json!({}),
                ..Default::default()
            };
            s.unit_active = Some("active".into());
            s.http_code = c.spec.port.map(|_| 200);
            if c.spec.name == "galion-one-mouth" {
                s.http_code = Some(503);
                s.extra = json!({"mouth": "model loading", "health_body": {"status": "loading", "model": "chatterbox", "loaded": false}});
                c.status = Status::Degrading;
                c.status_since = now - 44 * 3600;
            }
            if c.spec.name == "cloudflared" {
                s.tunnel_conns = Some(4);
                s.tunnel_requests = Some(81948);
                s.tunnel_req_errors = Some(48591);
            }
            c.push(s, &th);
        }
        let lines: Vec<JLine> = (0..60u64).map(|k| JLine { t: now - 1800 + k * 30, unit: "cloudflared.service".into(), prio: 6,
            msg: format!("Z ERR Request failed error=\"dial tcp [::1]:{}: connect: connection refused\" dest=https://{}/ ip=1.1.1.1", if k % 2 == 0 { 4001 } else { 3011 }, if k % 2 == 0 { "galion.studio" } else { "merlinelement.com" }) }).chain([ // repo-check: allow (test fixture address, never dialled)
            JLine { t: now - 60, unit: "console.service".into(), prio: 6, msg: "The request id \"/x/.git/config\" is outside of Vite serving allow list.".into() }]).collect();
        let li = crate::logscan::insights(&lines);
        let ls = crate::probe::parse_ss("LISTEN 0 1 127.0.0.1:8652 0.0.0.0:*\nLISTEN 0 1 127.0.0.1:8420 0.0.0.0:*\nLISTEN 0 1 *:22 *:*\nLISTEN 0 1 127.0.0.1:20241 0.0.0.0:*\n");
        let cx = analyse(&Input {
            now,
            comps: &cs,
            logs: &li,
            listeners: Some(&ls),
            ddos: &[],
            known_ports: &[20241],
            public_ok_ports: &[22],
            listen_scope: None,
            own_port: 8670,
            loading_grace_s: 900,
        });
        let ids: Vec<&str> = cx.vulnerabilities.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"unmanaged-listeners"), "{ids:?}");
        assert!(cx.vulnerabilities.iter().any(|f| f.title.contains(":8652")));
        assert!(ids.contains(&"scanner-probes-console"), "{ids:?}");
        assert_eq!(
            cx.vulnerabilities
                .iter()
                .find(|f| f.id == "scanner-probes-console")
                .unwrap()
                .sev,
            Sev::Error
        );
        let edge = cx
            .bleeding
            .iter()
            .find(|f| f.id == "edge-dead-origins")
            .unwrap();
        assert_eq!(edge.sev, Sev::Error, "{}", edge.title);
        assert!(edge.title.contains(":3011 :4001"), "{}", edge.title);
        assert!(edge.evidence.iter().any(|e| e.contains("48591 of 81948")));
        assert!(edge
            .evidence
            .iter()
            .any(|e| e.contains("none of these ports has a listener")));
        let stuck = cx
            .bleeding
            .iter()
            .find(|f| f.id == "stuck-loading-galion-one-mouth")
            .unwrap();
        assert!(stuck.title.contains("1.8 d"), "{}", stuck.title);
        let sig = component_signals(&cs, now, 900);
        let m = sig
            .iter()
            .find(|s| s.component == "galion-one-mouth")
            .unwrap();
        assert_eq!((m.sev, m.cause.as_str()), (Sev::Error, "galion-one"));
        assert!(cx.signals.iter().any(|s| s.cause == "edge"));
        assert!(!cx.ddos_firing);
    }
}
