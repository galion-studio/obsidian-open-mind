// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Scanner/exploit probes seen in unit journals, and Vite dev servers
//! answering production traffic. Reads [`Input::logs`] (`scans`,
//! `dev_servers`); one finding and one `scan:<component>` signal per unit.

use super::{Sensor, Sink};
use crate::cortex::{comp_of_unit, Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use std::collections::BTreeMap;

/// Built-in sensor `scanners`.
pub struct Scanners;

impl Sensor for Scanners {
    fn name(&self) -> &'static str {
        "scanners"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        let mut by_unit: BTreeMap<&str, Vec<(&str, u32)>> = Default::default();
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
            out.finding(
                Finding::new(
                    FindingKind::Vulnerability,
                    format!("scanner-probes-{comp}"),
                    if dev { Sev::Error } else { Sev::Warn },
                    comp.clone(),
                    format!("{n} scanner/exploit request(s) reached {comp} ({} distinct paths)", paths.len()),
                )
                .with_evidence(ev)
                .with_fix(if dev {
                    "serve a production build (B61) instead of the dev server, and deny dotfiles (.env, .git) at the edge"
                } else {
                    "make sure those paths 404 and nothing sensitive sits under the web root"
                })
                .with_cause(format!("scan:{comp}")),
            );
            out.signal(Signal::new(
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
            out.finding(
                Finding::new(
                    FindingKind::Vulnerability,
                    format!("dev-server-{comp}"),
                    Sev::Warn,
                    comp.clone(),
                    format!("{comp} runs a Vite dev server"),
                )
                .with_evidence(vec![format!("{unit} logs Vite dev-server messages")])
                .with_fix("switch to a production build (B61)"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::JLine;
    use crate::sensors::Sensors;

    fn line(unit: &str, msg: &str) -> JLine {
        JLine {
            t: 100,
            unit: unit.into(),
            prio: 6,
            msg: msg.into(),
        }
    }

    #[test]
    fn scanner_paths_and_dev_server() {
        let logs = crate::logscan::insights(&[
            line(
                "console.service",
                "The request id \"/x/.git/config\" is outside of Vite serving allow list.",
            ),
            line("desk.service", "vite: hmr update /src/App.tsx"),
        ]);
        let mut s = Sensors::empty();
        s.add(Scanners);
        let cx = s.run(&Input::new(200, &[], &logs));
        let ids: Vec<&str> = cx.vulnerabilities.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"scanner-probes-console"), "{ids:?} {logs:?}");
        assert!(ids.contains(&"dev-server-desk"), "{ids:?}");
        let sc = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "scanner-probes-console")
            .unwrap();
        assert_eq!(sc.cause.as_deref(), Some("scan:console"));
        assert_eq!(sc.source, "scanners");
        assert_eq!(cx.signals.len(), 1);
        assert_eq!(cx.signals[0].cause, "scan:console");
        // An empty window reports nothing.
        let empty = crate::logscan::LogInsights::default();
        let cx = s.run(&Input::new(200, &[], &empty));
        assert!(cx.vulnerabilities.is_empty() && cx.signals.is_empty());
    }
}
