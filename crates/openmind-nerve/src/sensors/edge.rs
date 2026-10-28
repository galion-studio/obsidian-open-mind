// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Edge bleeding: tunnel routes whose origin port refuses dials (refused
//! `dial tcp` lines in the cloudflared journal window). One `edge-dead-origins`
//! finding and one `edge` signal. Reads [`Input::logs`] (`dead_origins`),
//! [`Input::listeners`] and the critical tunnel component's counters.

use super::{Sensor, Sink};
use crate::config::Kind;
use crate::cortex::{dur, Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use std::collections::BTreeSet;

/// Built-in sensor `edge`.
pub struct DeadOrigins;

impl Sensor for DeadOrigins {
    fn name(&self) -> &'static str {
        "edge"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if i.logs.dead_origins.is_empty() {
            return;
        }
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
        out.finding(
            Finding::new(FindingKind::Bleeding, "edge-dead-origins", sev, "cloudflared", title.clone())
                .with_evidence(ev)
                .with_fix("either start the services behind those ports or remove the ingress rules / DNS for those hostnames (cloudflared config + DNS: owner approval; the nerve never touches them)")
                .with_cause("edge"),
        );
        out.signal(Signal::new("edge", "cloudflared", sev, title));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::JLine;
    use crate::sensors::Sensors;

    fn refused(t: u64, port: u16) -> JLine {
        JLine {
            t,
            unit: "cloudflared.service".into(),
            prio: 6,
            msg: format!("ERR Request failed error=\"dial tcp [::1]:{port}: connect: connection refused\" dest=https://example.test/"),
        }
    }

    #[test]
    fn slow_bleed_is_warn_fast_bleed_is_error() {
        let mut s = Sensors::empty();
        s.add(DeadOrigins);
        // 2 failures over 30 min: 0.1/min -> warn.
        let slow = crate::logscan::insights(&[refused(1000, 4001), refused(2800, 4001)]);
        let ls = crate::probe::parse_ss("LISTEN 0 1 127.0.0.1:4001 0.0.0.0:*\n");
        let mut i = Input::new(2800, &[], &slow);
        i.listeners = Some(&ls);
        let cx = s.run(&i);
        assert_eq!(cx.bleeding.len(), 1);
        let f = &cx.bleeding[0];
        assert_eq!(
            (f.id.as_str(), f.kind, f.sev, f.source.as_str()),
            (
                "edge-dead-origins",
                FindingKind::Bleeding,
                Sev::Warn,
                "edge"
            )
        );
        assert!(
            f.evidence
                .iter()
                .any(|e| e.contains("ports with a listener now: [4001]")),
            "{:?}",
            f.evidence
        );
        assert_eq!(cx.signals[0].cause, "edge");
        // 60 failures in 60 s -> error.
        let fast: Vec<JLine> = (0..60).map(|k| refused(1000 + k, 3011)).collect();
        let fast = crate::logscan::insights(&fast);
        let cx = s.run(&Input::new(1060, &[], &fast));
        assert_eq!(cx.bleeding[0].sev, Sev::Error, "{}", cx.bleeding[0].title);
        let quiet = crate::logscan::LogInsights::default();
        assert!(s.run(&Input::new(1, &[], &quiet)).bleeding.is_empty());
    }
}
