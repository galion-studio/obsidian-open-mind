// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Per-component bleeding: stuck "loading" health, restart loops, rising
//! memory, error rate, host pressure, memory-heavy services. Reads
//! [`Input::comps`] (latest sample, reasons, restart history) only. Each
//! finding carries the component's group as its incident cause.

use super::{Sensor, Sink};
use crate::config::Kind;
use crate::cortex::{dur, not_deployed, stuck_loading, Finding, FindingKind, Input};
use crate::incident::Sev;
use crate::util::kib;

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
                    if c.spec.unit.is_none() {
                        // Slot 358 (inc-865945dd): no unit owns this listener, so it is
                        // served by its group's process; live galion-one 40454f9 hard-codes
                        // this body with no model worker at all.
                        "no unit owns this listener: it is served by its group's process. If the body never changes across that process's restarts the loading state is hard-coded (no model worker exists): deploy the backend or a build with an honest /health; a restart will not help (owner approval)"
                    } else {
                        "check the voice/model process logs and the model files, then restart that one service (owner approval)"
                    });
            } else if let Some(age) = not_deployed(c, i.now, i.loading_grace_s) {
                let body = s
                    .extra
                    .get("health_body")
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                push("not-deployed", Sev::Warn,
                    format!("{name} answers 503 \"unavailable\": nothing deployed behind it for {} (since {})", dur(age), crate::util::ts(c.status_since)),
                    vec![format!("health :{}{} -> 503 {body}", c.spec.port.unwrap_or(0), c.spec.path),
                         "the service says so itself: nothing is loading, a restart changes nothing".into()],
                    "deploy the backend it names (owner decision) or accept the gap; do not restart");
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

    /// Slot 358 (inc-865945dd): live hard-coded "loading" body -> stuck-loading
    /// with the no-unit fix text; honest "unavailable" body -> not-deployed (warn).
    #[test]
    fn mouth_loading_vs_not_deployed_findings() {
        let th = Thresholds::default();
        let now = 1_792_520_000;
        let run = |body: serde_json::Value, mouth: bool| {
            let mut cs: Vec<CompState> = default_specs(None, None)
                .into_iter()
                .filter(|s| s.name == "galion-one-mouth")
                .map(CompState::new)
                .collect();
            let mut extra = json!({ "health_body": body });
            if mouth {
                extra["mouth"] = json!("model loading");
            }
            cs[0].push(
                Sample {
                    t: now,
                    present: true,
                    http_code: Some(503),
                    extra,
                    ..Default::default()
                },
                &th,
            );
            cs[0].status = crate::state::Status::Degrading;
            cs[0].status_since = now - 6 * 3600;
            let logs = crate::logscan::LogInsights::default();
            let mut s = Sensors::empty();
            s.add(ComponentBleeding);
            s.run(&Input::new(now, &cs, &logs)).bleeding
        };
        let legacy = run(
            json!({"ok": false, "status": "loading", "provider": "local", "model": "chatterbox", "loaded": false}),
            true,
        );
        assert_eq!(legacy.len(), 1, "{legacy:?}");
        assert_eq!(legacy[0].id, "stuck-loading-galion-one-mouth");
        assert_eq!(legacy[0].sev, Sev::Error);
        let fix = &legacy[0].fix;
        assert!(fix.contains("a restart will not help"), "{fix}");
        let honest = run(
            json!({"ok": false, "status": "unavailable", "provider": "stub", "model": null, "loaded": false,
                   "reason": "voice not deployed; set GALION_MOUTH to a galion-voice-serve address"}),
            false,
        );
        assert_eq!(honest.len(), 1, "{honest:?}");
        assert_eq!(honest[0].id, "not-deployed-galion-one-mouth");
        assert_eq!(honest[0].sev, Sev::Warn);
        assert!(
            honest[0]
                .title
                .contains("nothing deployed behind it for 6.0 h"),
            "{}",
            honest[0].title
        );
        assert!(
            honest[0].evidence[0].contains("voice not deployed"),
            "{:?}",
            honest[0].evidence
        );
        assert!(!honest[0].title.contains("loading"), "{}", honest[0].title);
    }

    fn desk_group(cs: &[CompState]) -> &'static str {
        cs.iter()
            .find(|c| c.spec.name == "galion-desk")
            .unwrap()
            .spec
            .group
    }
}
