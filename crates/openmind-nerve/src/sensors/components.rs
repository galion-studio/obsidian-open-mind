// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Per-component bleeding: stuck "loading" health, restart loops, rising
//! memory, error rate, host pressure, memory-heavy services. Reads
//! [`Input::comps`] (latest sample, reasons, restart history) only. Each
//! finding carries the component's group as its incident cause.

use super::{Sensor, Sink};
use crate::config::Kind;
use crate::cortex::{dur, stuck_loading, Finding, FindingKind, Input};
use crate::incident::Sev;
use crate::util::kib;

/// A service above this resident size (KiB) gets the info `memory-heavy` finding.
/// Was 512 MiB; T3.1 (docs/cortex/calibration.md) tightened it to 256 MiB from
/// 20 real VPS snapshots: the console holds 600 MiB-1.27 GiB, every other
/// service at most 29.8 MiB (cloudflared), so 256 MiB is 8.6x the second
/// largest and still flags only the console. Tighten only (`calibrate::CALIBRATED`).
pub const MEMORY_HEAVY_KIB: u64 = 256 * 1024;

/// Built-in sensor `components`.
pub struct ComponentBleeding;

impl Sensor for ComponentBleeding {
    fn name(&self) -> &'static str {
        "components"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        for c in i.comps {
            let Some(s) = c.latest() else { continue };
            let name = c.spec.name;
            let group = c.spec.group;
            let mut push = |id: &str, sev: Sev, title: String, ev: Vec<String>, fix: &str| {
                out.finding(
                    Finding::new(
                        FindingKind::Bleeding,
                        format!("{id}-{name}"),
                        sev,
                        name,
                        title,
                    )
                    .with_evidence(ev)
                    .with_fix(fix)
                    .with_cause(group),
                )
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
                if m > MEMORY_HEAVY_KIB && c.spec.kind == Kind::Service {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_specs;
    use crate::sensors::Sensors;
    use crate::state::{CompState, Sample, Thresholds};
    use serde_json::json;

    #[test]
    fn memory_heavy_and_error_reasons() {
        let th = Thresholds::default();
        let mut cs: Vec<CompState> = default_specs(None, None)
            .into_iter()
            .map(CompState::new)
            .collect();
        let now = 1_792_520_000;
        let desk = cs
            .iter_mut()
            .find(|c| c.spec.name == "galion-desk")
            .unwrap();
        desk.push(
            Sample {
                t: now,
                present: true,
                mem_kb: Some(900 * 1024),
                http_code: Some(200),
                extra: json!({}),
                ..Default::default()
            },
            &th,
        );
        desk.reasons = vec!["12 errors in 5 min".into()];
        let logs = crate::logscan::LogInsights::default();
        let mut s = Sensors::empty();
        s.add(ComponentBleeding);
        let cx = s.run(&Input::new(now, &cs, &logs));
        let ids: Vec<&str> = cx.bleeding.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            ["error-rate-galion-desk", "memory-heavy-galion-desk"],
            "{ids:?}"
        );
        assert_eq!(cx.bleeding[1].sev, Sev::Info);
        assert_eq!(cx.bleeding[0].cause.as_deref(), Some(desk_group(&cs)));
        assert!(cx.signals.is_empty());
    }

    #[test]
    fn memory_heavy_line_is_the_calibrated_one() {
        // T3.1: above 256 MiB is memory-heavy (was 512 MiB); 256 MiB itself is not.
        let th = Thresholds::default();
        let now = 1_792_520_000;
        for (kb, fires) in [
            (MEMORY_HEAVY_KIB, false),
            (MEMORY_HEAVY_KIB + 1, true),
            (300 * 1024, true),
            (30 * 1024, false),
        ] {
            let mut cs: Vec<CompState> = default_specs(None, None)
                .into_iter()
                .map(CompState::new)
                .collect();
            let desk = cs
                .iter_mut()
                .find(|c| c.spec.name == "galion-desk")
                .unwrap();
            desk.push(
                Sample {
                    t: now,
                    present: true,
                    mem_kb: Some(kb),
                    http_code: Some(200),
                    extra: json!({}),
                    ..Default::default()
                },
                &th,
            );
            let logs = crate::logscan::LogInsights::default();
            let mut s = Sensors::empty();
            s.add(ComponentBleeding);
            let cx = s.run(&Input::new(now, &cs, &logs));
            let got = cx
                .bleeding
                .iter()
                .any(|f| f.id == "memory-heavy-galion-desk");
            assert_eq!(got, fires, "{kb} KiB");
        }
    }

    fn desk_group(cs: &[CompState]) -> &'static str {
        cs.iter()
            .find(|c| c.spec.name == "galion-desk")
            .unwrap()
            .spec
            .group
    }
}
