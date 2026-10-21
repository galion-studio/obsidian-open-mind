// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Listener exposure: public sockets that should be loopback, and loopback
//! listeners no monitored unit owns (stray manual processes). Owns the
//! cortex `exposure` section. Reads [`Input::listeners`] only.

use super::{Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use crate::owners::ProcInfo;
use crate::probe::Listener;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Built-in sensor `listeners`.
pub struct Listeners;

impl Sensor for Listeners {
    fn name(&self) -> &'static str {
        "listeners"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        let Some(ls) = i.listeners else {
            out.exposure(json!({"note": "listeners not read this cycle"}));
            return;
        };
        let spec_ports: BTreeSet<u16> = i
            .comps
            .iter()
            .flat_map(|c| [c.spec.port, c.spec.metrics_port])
            .flatten()
            .collect();
        let in_scope = |p: u16| i.listen_scope.is_none_or(|(a, b)| p >= a && p <= b);
        let mut public_ok = vec![];
        let mut unmanaged: Vec<&Listener> = vec![];
        for l in ls.iter().filter(|l| in_scope(l.port)) {
            if !l.loopback {
                if i.public_ok_ports.contains(&l.port) {
                    public_ok.push(format!("{}:{}", l.addr, l.port));
                } else {
                    out.finding(
                        Finding::new(
                            FindingKind::Vulnerability,
                            format!("public-listener-{}", l.port),
                            Sev::Error,
                            "host",
                            format!("port {} listens on {} (not loopback)", l.port, l.addr),
                        )
                        .with_evidence(vec![format!("LISTEN {}:{}", l.addr, l.port)])
                        .with_fix("bind it to 127.0.0.1 and publish through the tunnel, or firewall it (owner approval)"),
                    );
                }
            } else if l.port != i.own_port
                && (i.owners.unowned.contains(&l.port)
                    || (!spec_ports.contains(&l.port)
                        && !i.known_ports.contains(&l.port)
                        && !i.owners.is_owned(l.port)))
            {
                unmanaged.push(l);
            }
        }
        let mut ports: Vec<u16> = unmanaged.iter().map(|l| l.port).collect();
        ports.sort();
        ports.dedup();
        if !ports.is_empty() {
            out.finding(
                Finding::new(
                    FindingKind::Vulnerability,
                    "unmanaged-listeners",
                    Sev::Warn,
                    "host",
                    format!(
                        "{} loopback listener(s) no monitored unit owns: {}",
                        ports.len(),
                        ports.iter().map(|p| format!(":{p}")).collect::<Vec<_>>().join(" ")
                    ),
                )
                .with_evidence({
                    let mut ev: Vec<String> = ports
                        .iter()
                        .map(|p| {
                            let o = i.owners.lookup(*p, i.procs);
                            format!(
                                ":{p} {}{} -- {}",
                                o.owner,
                                if o.documented_unowned { " (documented: no unit owns it)" } else { "" },
                                o.process
                                    .as_ref()
                                    .map(ProcInfo::describe)
                                    .unwrap_or_else(|| "process not readable from /proc".into())
                            )
                        })
                        .collect();
                    ev.push("not a component health/metrics port and not in NERVE_KNOWN_PORTS".into());
                    ev.push("typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching)".into());
                    ev
                })
                .with_fix("identify the owners (ss -ltnp), then stop them or give them a unit (owner approval; the nerve never kills processes)"),
            );
        }
        let ownership: Vec<Value> = ls
            .iter()
            .filter(|l| in_scope(l.port))
            .map(|l| l.port)
            .collect::<BTreeSet<u16>>()
            .into_iter()
            .map(|p| {
                if p == i.own_port {
                    json!({"port": p, "owner": "galion-nerve (self)", "gaia": "AETHER", "documented_unowned": false})
                } else {
                    i.owners.lookup(p, i.procs).to_json()
                }
            })
            .collect();
        out.exposure(
            json!({"public_expected": public_ok, "unmanaged_ports": ports, "ownership": ownership,
                            "listeners_judged": ls.iter().filter(|l| in_scope(l.port)).count()}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    #[test]
    fn public_unmanaged_scope_and_exposure() {
        let ls = crate::probe::parse_ss(
            "LISTEN 0 1 0.0.0.0:9000 0.0.0.0:*\nLISTEN 0 1 *:22 *:*\nLISTEN 0 1 127.0.0.1:9001 0.0.0.0:*\nLISTEN 0 1 127.0.0.1:9002 0.0.0.0:*\nLISTEN 0 1 127.0.0.1:9003 0.0.0.0:*\nLISTEN 0 1 127.0.0.1:9900 0.0.0.0:*\n",
        );
        let logs = LogInsights::default();
        let mut i = Input::new(1, &[], &logs);
        i.listeners = Some(&ls);
        i.public_ok_ports = &[22];
        i.known_ports = &[9002];
        i.own_port = 9003;
        i.listen_scope = Some((22, 9100));
        let mut s = Sensors::empty();
        s.add(Listeners);
        let cx = s.run(&i);
        let ids: Vec<&str> = cx.vulnerabilities.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["public-listener-9000", "unmanaged-listeners"]);
        assert_eq!(cx.vulnerabilities[0].sev, Sev::Error);
        assert_eq!(
            cx.vulnerabilities[1].title,
            "1 loopback listener(s) no monitored unit owns: :9001"
        );
        assert!(cx.vulnerabilities.iter().all(|f| f.source == "listeners"));
        assert_eq!(cx.exposure["unmanaged_ports"], json!([9001]));
        assert_eq!(cx.exposure["listeners_judged"], 5);
        assert_eq!(cx.exposure["public_expected"].as_array().unwrap().len(), 1);
        // Not read this cycle: no findings, a note.
        let cx = s.run(&Input::new(1, &[], &logs));
        assert!(cx.vulnerabilities.is_empty());
        assert!(cx.exposure["note"].is_string());
    }
}
