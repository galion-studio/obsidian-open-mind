// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! HADES plan templates from cortex incidents (cortex plan T4.2).
//!
//! The cortex says what is wrong; this module writes, per fix family, the
//! plan an admin could approve through HADES. It is **text only**: nothing
//! here spawns a process, opens a socket, writes a file or calls hades-gate.
//! The nerve publishes the plans on its loopback API
//! (`GET /api/v1/cortex/plans`, `state.cortex_plans`) and never runs them;
//! neither does the cortex. Running one needs the owner's single-use
//! hades-gate approval (action `heal-apply`, exact target, exact plan hash)
//! and a human or a later, separately approved executor.
//!
//! | family | from | target |
//! |---|---|---|
//! | `unit-restart` | `stuck-loading-<c>`, `memory-trend-<c>` (warn+) | the component's systemd unit (or its group's unit when it has none) |
//! | `stray-stop` | `unmanaged-listeners`, `stray-owner` | `stray-listener-<port>` |
//! | `ingress-rule-removal` | `edge-dead-origins` (ports with no listener) | `cloudflared` |
//! | `edge-rate-limit` | flood/elevated `ddos.streams` behind an open `ddos:*` incident or `edge-host-rate` | `cloudflare-edge` |
//! | `rebind-loopback` | `public-listener-<port>` | the owning unit, else `listener-<port>` |
//!
//! Hard limits, by construction: no step changes ssh, the host packet
//! filter, DNS records, accounts or permissions; no step deletes data; no
//! plan targets HADES, the admin access path or a protected name
//! ([`PROTECTED_LABELS`]); ssh credential floods, restart loops and every
//! other finding yield **no** plan (they are listed in `skipped` with the
//! reason). The plan bytes the admin signs are `steps` joined with `\n`
//! (`hades_gate::adapter::gaia_plan_bytes`); steps carry no timestamp, so
//! the same incident gives the same plan hash cycle after cycle.
//!
//! Input is the nerve's own `/api/v1/state` JSON, so the same function runs
//! in the engine, on a replayed capture and on a saved state file
//! (`openmind-nerve plans FILE`).

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The only hades-gate action kind a template uses (allow-listed there).
pub const ACTION: &str = "heal-apply";

/// Labels that never become a plan target or an ingress rule to remove:
/// HADES itself and the admin's access path (a subset of hades-gate's
/// protected targets; the gate re-checks every plan anyway).
pub const PROTECTED_LABELS: &[&str] = &[
    "hades",
    "admin",
    "root",
    "ssh",
    "sshd",
    "sudo",
    "login",
    "pam",
    "firewall",
    "secret",
    "secrets",
    "key",
    "keys",
    "token",
    "credential",
    "credentials",
];

/// Ports no template ever touches (ssh).
pub const PROTECTED_PORTS: &[u16] = &[22];

/// Fix family of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    UnitRestart,
    StrayStop,
    IngressRuleRemoval,
    EdgeRateLimit,
    RebindLoopback,
}

impl Family {
    pub const ALL: [Family; 5] = [
        Family::UnitRestart,
        Family::StrayStop,
        Family::IngressRuleRemoval,
        Family::EdgeRateLimit,
        Family::RebindLoopback,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Family::UnitRestart => "unit-restart",
            Family::StrayStop => "stray-stop",
            Family::IngressRuleRemoval => "ingress-rule-removal",
            Family::EdgeRateLimit => "edge-rate-limit",
            Family::RebindLoopback => "rebind-loopback",
        }
    }
}

/// One HADES plan template. Never executed by the nerve or the cortex.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// `plan-<family>-<target>`: stable while the problem lasts.
    pub id: String,
    pub family: Family,
    /// hades-gate target (exact service name the approval is scoped to).
    pub target: String,
    /// Open incident the plan answers (None: a finding-only problem).
    pub incident: Option<String>,
    /// Incident cause or the finding's own cause.
    pub cause: Option<String>,
    /// `<kind>:<id>` of the findings (or `ddos:<stream>`) behind the plan.
    pub findings: Vec<String>,
    /// Worst severity of those findings (`warn`, `error`, `critical`).
    pub severity: String,
    /// The plan text, one step per line: preconditions, change, verify,
    /// rollback. Signed bytes = `steps.join("\n")`.
    pub steps: Vec<String>,
    /// Not signed: why the plan looks like this, risks for the owner.
    pub notes: Vec<String>,
}

impl Plan {
    /// The exact bytes an admin approval is minted over.
    pub fn plan_text(&self) -> String {
        self.steps.join("\n")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "family": self.family.as_str(), "action": ACTION,
            "target": self.target, "incident": self.incident, "cause": self.cause,
            "findings": self.findings, "severity": self.severity,
            "steps": self.steps, "plan_bytes": self.plan_text().len(),
            "notes": self.notes,
            "executed": false,
            "approval": "owner only: a single-use hades-gate heal-apply approval scoped to this target and the exact plan text; the nerve and the cortex never run plans",
        })
    }
}

/// A finding (or stream) that got no plan, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub finding: String,
    pub reason: String,
}

/// Every plan of one state, plus what was skipped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plans {
    pub plans: Vec<Plan>,
    pub skipped: Vec<Skip>,
}

impl Plans {
    pub fn to_json(&self) -> Value {
        let mut by = serde_json::Map::new();
        for f in Family::ALL {
            by.insert(
                f.as_str().into(),
                self.plans.iter().filter(|p| p.family == f).count().into(),
            );
        }
        json!({
            "executed": false,
            "action": ACTION,
            "count": self.plans.len(),
            "by_family": by,
            "plans": self.plans.iter().map(Plan::to_json).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|s| json!({"finding": s.finding, "reason": s.reason})).collect::<Vec<_>>(),
        })
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn sev_rank(s: &str) -> u8 {
    match s {
        "critical" | "crit" => 3,
        "error" | "high" => 2,
        "warn" | "warning" => 1,
        _ => 0,
    }
}

/// True when any `-`/`.`-separated label of `name` is protected.
pub fn is_protected(name: &str) -> bool {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|l| PROTECTED_LABELS.contains(&l))
}

/// Printable-ASCII token safe to put in a plan line (hostnames, units).
fn clean(t: &str) -> Option<&str> {
    let ok = !t.is_empty()
        && t.len() <= 253
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._@:".contains(&b));
    ok.then_some(t)
}

/// A routable public address (a ban at the edge can only match those).
fn public_ip(ip: &str) -> bool {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => {
            !(a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast())
        }
        Ok(std::net::IpAddr::V6(a)) => {
            let s = a.segments()[0];
            !(a.is_loopback()
                || a.is_unspecified()
                || (s & 0xfe00) == 0xfc00
                || (s & 0xffc0) == 0xfe80)
        }
        Err(_) => false,
    }
}

/// `:<port>` tokens of a title (`... owns: :8672 :19120`, `:19570 (uid 1000)`).
fn colon_ports(t: &str) -> Vec<u16> {
    let mut v: Vec<u16> = t
        .split_whitespace()
        .filter_map(|w| w.strip_prefix(':'))
        .filter_map(|p| p.trim_end_matches([',', ';', ')']).parse().ok())
        .collect();
    v.sort();
    v.dedup();
    v
}

struct Comp<'a> {
    name: &'a str,
    unit: Option<&'a str>,
    port: Option<u16>,
    group: Option<&'a str>,
    tunnel: bool,
    host: bool,
}

fn comps(state: &Value) -> Vec<Comp<'_>> {
    state["components"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Comp {
                    name: s(&c["name"]),
                    unit: c["unit"].as_str().filter(|u| !u.is_empty()),
                    port: c["port"].as_u64().and_then(|p| u16::try_from(p).ok()),
                    group: c["group"].as_str(),
                    tunnel: !c["tunnel"].is_null(),
                    host: !c["host"].is_null(),
                })
                .collect()
        })
        .unwrap_or_default()
}

struct Ctx<'a> {
    comps: Vec<Comp<'a>>,
    incidents: Vec<&'a Value>,
    out: Plans,
    seen: BTreeSet<String>,
}

impl<'a> Ctx<'a> {
    /// The open incident that holds `key` as a member, else the one whose
    /// cause is `cause`.
    fn incident_for(&self, key: &str, cause: Option<&str>) -> Option<&'a Value> {
        self.incidents
            .iter()
            .copied()
            .find(|i| {
                i["correlation"]["findings"]
                    .as_array()
                    .is_some_and(|f| f.iter().any(|m| m["finding"] == key))
            })
            .or_else(|| {
                cause.and_then(|c| self.incidents.iter().copied().find(|i| i["cause"] == c))
            })
    }

    fn skip(&mut self, finding: impl Into<String>, reason: impl Into<String>) {
        self.out.skipped.push(Skip {
            finding: finding.into(),
            reason: reason.into(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        family: Family,
        target: &str,
        key: &str,
        cause: Option<&str>,
        sev: &str,
        steps: Vec<String>,
        notes: Vec<String>,
    ) {
        let id = format!("plan-{}-{}", family.as_str(), target);
        let inc = self.incident_for(key, cause);
        if let Some(p) = self.out.plans.iter_mut().find(|p| p.id == id) {
            // Same family and target from a second finding: one plan, both keys.
            if !p.findings.iter().any(|f| f == key) {
                p.findings.push(key.into());
            }
            if sev_rank(sev) > sev_rank(&p.severity) {
                p.severity = sev.into();
            }
            return;
        }
        if !self.seen.insert(id.clone()) {
            return;
        }
        self.out.plans.push(Plan {
            id,
            family,
            target: target.into(),
            incident: inc.map(|i| s(&i["id"]).to_string()),
            cause: inc
                .map(|i| s(&i["cause"]).to_string())
                .or_else(|| cause.map(str::to_string)),
            findings: vec![key.into()],
            severity: sev.into(),
            steps,
            notes,
        });
    }

    fn comp(&self, name: &str) -> Option<&Comp<'a>> {
        self.comps.iter().find(|c| c.name == name)
    }

    /// The unit to restart for component `name`: its own, else the unit of
    /// the component named after its group (galion-one-mouth -> galion-one).
    fn restart_unit(&self, name: &str, group: Option<&str>) -> Option<(String, String)> {
        let c = self.comp(name)?;
        if let Some(u) = c.unit {
            return Some((c.name.to_string(), u.to_string()));
        }
        let g = group.or(c.group)?;
        let gc = self
            .comps
            .iter()
            .find(|x| x.name == g && x.unit.is_some())?;
        Some((gc.name.to_string(), gc.unit?.to_string()))
    }
}

/// All plans for one nerve state (`/api/v1/state` JSON).
pub fn from_state(state: &Value) -> Plans {
    let mut cx = Ctx {
        comps: comps(state),
        incidents: state["incidents"]["open"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        out: Plans::default(),
        seen: BTreeSet::new(),
    };
    let cortex = &state["cortex"];
    let findings: Vec<(&str, &Value)> = [
        ("vulnerability", &cortex["vulnerabilities"]),
        ("bleeding", &cortex["bleeding"]),
        ("ddos", &cortex["ddos"]["findings"]),
    ]
    .into_iter()
    .flat_map(|(k, v)| {
        v.as_array()
            .map(|a| a.iter().map(move |f| (k, f)).collect::<Vec<_>>())
            .unwrap_or_default()
    })
    .collect();
    for (kind, f) in &findings {
        finding_plan(&mut cx, kind, f, state);
    }
    stream_plans(&mut cx, state);
    // Open incidents no plan answers stay manual: say so.
    let planned: BTreeSet<String> = cx
        .out
        .plans
        .iter()
        .filter_map(|p| p.incident.clone())
        .collect();
    let manual: Vec<String> = cx
        .incidents
        .iter()
        .filter(|i| !planned.contains(s(&i["id"])))
        .map(|i| format!("incident:{}", s(&i["cause"])))
        .collect();
    for m in manual {
        cx.skip(
            m,
            "open incident with no plan template: stays a manual owner step",
        );
    }
    let mut out = cx.out;
    out.plans.sort_by(|a, b| {
        sev_rank(&b.severity)
            .cmp(&sev_rank(&a.severity))
            .then(a.family.cmp(&b.family))
            .then(a.id.cmp(&b.id))
    });
    out
}

fn finding_plan(cx: &mut Ctx<'_>, kind: &str, f: &Value, state: &Value) {
    let id = s(&f["id"]);
    let key = format!("{kind}:{id}");
    let sev = s(&f["severity"]);
    let comp = s(&f["component"]);
    let cause = f["incident_cause"].as_str();
    if sev_rank(sev) == 0 {
        return; // info findings never get a plan (and are not listed)
    }
    let restart_kind = ["stuck-loading", "memory-trend"]
        .into_iter()
        .find(|p| id == format!("{p}-{comp}"));
    if let Some(what) = restart_kind {
        return unit_restart(cx, &key, comp, cause, sev, what);
    }
    match id {
        "unmanaged-listeners" => {
            let mut ports: Vec<u16> = state["cortex"]["exposure"]["unmanaged_ports"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_u64().and_then(|p| u16::try_from(p).ok()))
                        .collect()
                })
                .unwrap_or_default();
            if ports.is_empty() {
                ports = colon_ports(s(&f["title"]));
            }
            for p in ports {
                stray_stop(cx, &key, cause, sev, p, "no monitored unit owns it");
            }
        }
        "stray-owner" => {
            for p in colon_ports(s(&f["title"])) {
                stray_stop(cx, &key, cause, sev, p, "it runs as an unexpected uid");
            }
        }
        "edge-dead-origins" => ingress_removal(cx, &key, cause, sev, f),
        "edge-host-rate" => {} // planned per stream in stream_plans
        _ if id.starts_with("public-listener-") => {
            match id["public-listener-".len()..].parse::<u16>() {
                Ok(p) => rebind(cx, &key, cause, sev, p),
                Err(_) => cx.skip(key, "public listener without a port number"),
            }
        }
        _ => {
            let reason = if id.starts_with("restarts-") {
                "restart loop: another restart does not fix it; read the unit journal first (no template)"
            } else if id.starts_with("sshd-") || cause == Some("ssh-flood") {
                "ssh: no template touches ssh or the host packet filter; password auth stays off"
            } else if id == "deleted-binary" {
                "deleted binary: which unit to restart needs the owning process (loopback evidence); no template"
            } else {
                "no fix family for this finding"
            };
            cx.skip(key, reason);
        }
    }
}

fn unit_restart(
    cx: &mut Ctx<'_>,
    key: &str,
    comp: &str,
    cause: Option<&str>,
    sev: &str,
    what: &str,
) {
    let Some(c) = cx.comp(comp) else {
        return cx.skip(key, "component not in the state's component table");
    };
    if c.tunnel || c.host {
        return cx.skip(
            key,
            "tunnels and the host are never restarted by a template",
        );
    }
    let Some((owner, unit)) = cx.restart_unit(comp, cause) else {
        return cx.skip(key, "no systemd unit for this component or its group");
    };
    if is_protected(&owner) || is_protected(&unit) || clean(&unit).is_none() {
        return cx.skip(key, "protected or unreadable unit name");
    }
    let health = cx
        .comp(comp)
        .and_then(|c| c.port)
        .map(|p| format!("127.0.0.1:{p}"))
        .unwrap_or_else(|| "its health endpoint".into());
    let why = match what {
        "stuck-loading" => format!("{comp} answers 503 loading past the grace period"),
        _ => format!("{comp} memory keeps rising (a restart only resets it)"),
    };
    let mut notes = vec![format!("{why}; restart is the only change")];
    if owner != comp {
        notes.push(format!(
            "{comp} has no unit of its own: it is served by {owner}, so the plan restarts {unit} (all of {owner} restarts)"
        ));
    }
    let steps = vec![
        format!("plan unit-restart for {unit}: {why}"),
        format!("precondition: {unit} is active and {comp} still fails its check ({health}) at approval time; otherwise stop"),
        format!("precondition: no deploy or other plan for {unit} is running"),
        format!("record systemctl status {unit} and the last 200 journal lines of {unit} for the owner"),
        format!("run systemctl restart {unit} once; no second restart without a new plan"),
        format!("verify: {unit} is active within 30 s and {comp} answers 200 on {health} within its grace period"),
        "verify: the nerve shows the incident resolving in the next cycles".to_string(),
        format!("rollback: none needed for a restart; if {unit} does not come back, run galion-deploy rollback for {unit} (separate approval)"),
        "nothing is deleted; no config, access, packet filter or DNS change".to_string(),
    ];
    cx.push(Family::UnitRestart, &unit, key, cause, sev, steps, notes);
}

fn stray_stop(cx: &mut Ctx<'_>, key: &str, cause: Option<&str>, sev: &str, port: u16, why: &str) {
    if PROTECTED_PORTS.contains(&port) {
        return cx.skip(format!("{key}:{port}"), "protected port");
    }
    let target = format!("stray-listener-{port}");
    let steps = vec![
        format!("plan stray-stop for the process listening on port {port}: {why}"),
        format!("precondition: ss -ltnp shows exactly one process listening on :{port}; record its pid, uid, executable path and start time"),
        format!("precondition: systemctl status <that pid> names no unit; if a unit owns it this is not a stray: stop here and add {port} to NERVE_KNOWN_PORTS instead (separate decision)"),
        "precondition: the process is not part of the admin access path and not a HADES process".to_string(),
        "send SIGTERM to that one pid only, wait up to 10 s; never signal a process group, no forced stop without a new plan".to_string(),
        "keep its binary, working directory and logs for inspection; nothing is deleted".to_string(),
        format!("verify: ss -ltn no longer lists :{port}; the nerve no longer reports :{port} as unmanaged"),
        "verify: every monitored component stays healthy in the next nerve cycles".to_string(),
        "rollback: start the recorded executable again as the recorded uid from its recorded working directory".to_string(),
    ];
    let notes = vec![format!(
        "the precondition aborts when a unit owns :{port}: unknown to the nerve is not the same as stray"
    )];
    cx.push(Family::StrayStop, &target, key, cause, sev, steps, notes);
}

/// `origin S (port P) refused N dial(s), F failed request(s); hosts: a, b`.
fn parse_origin(line: &str) -> Option<(u16, Vec<String>)> {
    let rest = line.strip_prefix("origin ")?;
    let p = rest.find("(port ")? + "(port ".len();
    let port: u16 = rest[p..].split(')').next()?.trim().parse().ok()?;
    let hosts = rest
        .split_once("; hosts: ")
        .map(|(_, h)| {
            h.split(',')
                .map(str::trim)
                .filter(|h| *h != "-" && !h.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some((port, hosts))
}

/// `ports with a listener now: [8002, 9000] (...)`.
fn listening_now(ev: &[Value]) -> Vec<u16> {
    ev.iter()
        .filter_map(|l| l.as_str()?.strip_prefix("ports with a listener now: ["))
        .flat_map(|r| {
            r.split(']')
                .next()
                .unwrap_or("")
                .split(',')
                .filter_map(|p| p.trim().parse().ok())
                .collect::<Vec<u16>>()
        })
        .collect()
}

fn ingress_removal(cx: &mut Ctx<'_>, key: &str, cause: Option<&str>, sev: &str, f: &Value) {
    let ev: Vec<Value> = f["evidence"].as_array().cloned().unwrap_or_default();
    let live = listening_now(&ev);
    let mut rules: Vec<(u16, Vec<String>)> = vec![];
    for l in ev.iter().filter_map(Value::as_str) {
        let Some((port, hosts)) = parse_origin(l) else {
            continue;
        };
        if live.contains(&port) {
            cx.skip(
                format!("{key}:{port}"),
                "port has a listener now (flapping origin): restart or fix the service, not the route",
            );
            continue;
        }
        if PROTECTED_PORTS.contains(&port) {
            cx.skip(format!("{key}:{port}"), "protected port");
            continue;
        }
        let mut keep = vec![];
        for h in hosts {
            match clean(&h) {
                Some(h) if !is_protected(h) => keep.push(h.to_string()),
                Some(h) => cx.skip(
                    format!("{key}:{h}"),
                    "protected hostname (admin access path): never removed by a template",
                ),
                None => cx.skip(format!("{key}:{port}"), "unreadable hostname"),
            }
        }
        rules.push((port, keep));
    }
    if rules.is_empty() {
        return cx.skip(key, "no closed origin port left to unroute");
    }
    rules.sort();
    let mut steps = vec![
        format!(
            "plan ingress-rule-removal in the cloudflared tunnel config: {} route(s) to closed origin port(s)",
            rules.len()
        ),
        "precondition: each port below still has no listener (ss -ltn) and still refuses tunnel dials in the cloudflared journal".to_string(),
        "precondition: the owner confirms none of these services is coming back soon (else start it instead)".to_string(),
        "copy the cloudflared config file to a dated backup next to it".to_string(),
    ];
    for (port, hosts) in &rules {
        steps.push(if hosts.is_empty() {
            format!("remove only the ingress rule(s) whose service is http://localhost:{port}")
        } else {
            format!(
                "remove only the ingress rule(s) for {} whose service is http://localhost:{port}",
                hosts.join(", ")
            )
        });
    }
    steps.extend([
        "keep every other rule and the catch-all http_status:404 rule last".to_string(),
        "check the edited file with cloudflared tunnel ingress validate; on any error restore the backup and stop".to_string(),
        "restart cloudflared once (reflex budget 1 per hour); the tunnel reconnects in seconds".to_string(),
        "verify: those hostnames answer 404 from the catch-all, every other hostname answers as before, refused dials stop".to_string(),
        "rollback: restore the backup file and restart cloudflared once".to_string(),
        "DNS records of these hostnames are not changed by this plan (separate owner decision)".to_string(),
    ]);
    let notes = vec![
        "only the tunnel config changes; restarting cloudflared drops every tunnelled request for a few seconds".to_string(),
        "a hostname left in DNS with no ingress rule answers 404 instead of 502".to_string(),
    ];
    cx.push(
        Family::IngressRuleRemoval,
        "cloudflared",
        key,
        cause,
        sev,
        steps,
        notes,
    );
}

fn stream_plans(cx: &mut Ctx<'_>, state: &Value) {
    let streams = state["cortex"]["ddos"]["streams"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let has_edge_host_finding = state["cortex"]["ddos"]["findings"]
        .as_array()
        .is_some_and(|a| a.iter().any(|f| f["id"] == "edge-host-rate"));
    for v in &streams {
        let level = s(&v["level"]);
        if level != "flood" && level != "elevated" {
            continue;
        }
        let stream = s(&v["stream"]);
        let (key, cause, host) = match stream.strip_prefix("edge-host:") {
            Some(h) => {
                if !has_edge_host_finding {
                    continue;
                }
                (
                    "ddos:edge-host-rate".to_string(),
                    "ddos:edge-cloudflared".to_string(),
                    Some(h),
                )
            }
            None => (format!("ddos:{stream}"), format!("ddos:{stream}"), None),
        };
        if host.is_none() && !cx.incidents.iter().any(|i| i["cause"] == cause.as_str()) {
            cx.skip(key, "rate stream not behind an open incident");
            continue;
        }
        if clean(stream).is_none() || host.is_some_and(|h| clean(h).is_none()) {
            cx.skip(key, "unreadable stream or hostname");
            continue;
        }
        if host.is_some_and(is_protected) || is_protected(stream) {
            cx.skip(key, "protected hostname (admin access path)");
            continue;
        }
        let comp = v["component"].as_str().unwrap_or(stream);
        let scope = match host {
            Some(h) => format!("hostname {h}"),
            None => format!("the public hostname(s) routed to {comp}"),
        };
        let thr = v["threshold_rps"].as_f64().unwrap_or(0.0);
        let base = v["baseline_rps"].as_f64().unwrap_or(0.0);
        // Per client per 10 s: the detector's threshold, never below 20.
        let per10 = ((thr.max(base * 4.0) * 10.0).ceil() as u64).max(20);
        let sev = if level == "flood" { "critical" } else { "warn" };
        let mut steps = vec![
            format!("plan edge-rate-limit at Cloudflare for stream {stream} ({level})"),
            format!("precondition: the nerve still reports stream {stream} at {level} at approval time; otherwise stop"),
            format!("add one Cloudflare rate limiting rule for {scope}: more than {per10} requests per 10 s from one client address gets a managed challenge for 10 min"),
        ];
        let single = v["shape"] == "single-source";
        let top = v["top_ips"][0]["ip"].as_str().and_then(clean);
        let mut ban_note = None;
        if let (true, Some(ip)) = (single, top) {
            if !public_ip(ip) {
                ban_note = Some(format!(
                    "single source {ip} is not a public address (loopback, private or a proxy): no ban line, the rate limit only"
                ));
            }
        }
        if let (true, Some(ip), None) = (single, top, &ban_note) {
            steps.push(format!(
                "add one Cloudflare custom rule for {scope}: block client address {ip} (single source of the flood), review after 24 h"
            ));
        }
        steps.extend([
            "no change on the host: no packet filter rule, no access change, no DNS change".to_string(),
            format!("verify: stream {stream} drops under its threshold ({thr:.1} req/s) within 5 min and the hostname still answers normal visitors"),
            "rollback: remove the added Cloudflare rule(s); nothing else changed".to_string(),
        ]);
        let notes = vec![
            format!("limit = max(threshold {thr:.1} req/s, 4 x baseline {base:.2} req/s) x 10 s, at least 20; edge-host streams count failed requests only"),
            "behind the tunnel the origin sees Cloudflare, so a ban by address only works at the edge".to_string(),
        ];
        let notes: Vec<String> = notes.into_iter().chain(ban_note).collect();
        let target = "cloudflare-edge";
        let before = cx.out.plans.len();
        let id_target = format!("{target}-{}", stream.replace([':', '.'], "-"));
        cx.push(
            Family::EdgeRateLimit,
            &id_target,
            &key,
            Some(&cause),
            sev,
            steps,
            notes,
        );
        if cx.out.plans.len() > before {
            // One gate target for every edge rule; the plan id keeps the stream.
            if let Some(p) = cx.out.plans.last_mut() {
                p.target = target.into();
            }
        }
    }
}

fn rebind(cx: &mut Ctx<'_>, key: &str, cause: Option<&str>, sev: &str, port: u16) {
    if PROTECTED_PORTS.contains(&port) {
        return cx.skip(key, "protected port (ssh)");
    }
    let owner = cx
        .comps
        .iter()
        .find(|c| c.port == Some(port) && c.unit.is_some())
        .map(|c| (c.name.to_string(), c.unit.unwrap_or("").to_string()));
    let (target, who) = match &owner {
        Some((name, unit)) if !is_protected(name) && clean(unit).is_some() => {
            (unit.clone(), format!("{unit} ({name})"))
        }
        Some(_) => return cx.skip(key, "protected unit"),
        None => (
            format!("listener-{port}"),
            format!("the process on port {port}"),
        ),
    };
    let mut steps = vec![
        format!("plan rebind-loopback for {who}: port {port} listens on a public address"),
        format!("precondition: ss -ltn still shows :{port} bound to a non-loopback address"),
        format!("precondition: nothing outside this host calls :{port} directly (the tunnel reaches it over loopback)"),
    ];
    if owner.is_some() {
        steps.extend([
            format!("copy the config file or unit drop-in of {target} that sets its bind address to a dated backup"),
            format!("set the bind address of {target} to 127.0.0.1:{port}; change nothing else"),
            format!("run systemctl daemon-reload and systemctl restart {target} once"),
        ]);
    } else {
        steps.extend([
            format!("identify the process on :{port} with ss -ltnp; if no unit owns it, use a stray-stop plan instead and stop here"),
            format!("if a unit owns it: set its bind address to 127.0.0.1:{port} as above and restart that unit once"),
        ]);
    }
    steps.extend([
        format!("verify: ss -ltn shows :{port} on 127.0.0.1 only and its health answers as before over loopback"),
        "rollback: restore the backup, daemon-reload, restart once".to_string(),
        "no packet filter, access or DNS change".to_string(),
    ]);
    let notes = vec![
        "binding to loopback is preferred over a packet filter rule: it cannot lock anyone out"
            .to_string(),
    ];
    cx.push(
        Family::RebindLoopback,
        &target,
        key,
        cause,
        sev,
        steps,
        notes,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colon_ports_and_origin_lines() {
        assert_eq!(
            colon_ports("2 loopback listener(s) no monitored unit owns: :8672 :19120"),
            [8672, 19120]
        );
        assert_eq!(
            colon_ports(
                "2 listener(s) run as an unexpected user: :19570 (uid 1000) :19573 (uid 0)"
            ),
            [19570, 19573]
        );
        assert_eq!(
            parse_origin("origin tuner (port 4000) refused 3 dial(s), 2 failed request(s); hosts: a.example, b.example"),
            Some((4000, vec!["a.example".into(), "b.example".into()]))
        );
        assert_eq!(
            parse_origin("origin ? (port 3011) refused 1 dial(s), 0 failed request(s); hosts: -"),
            Some((3011, vec![]))
        );
        assert_eq!(parse_origin("cloudflared counters since its start"), None);
        assert_eq!(
            listening_now(&[json!(
                "ports with a listener now: [8002, 9000] (they may be flapping)"
            )]),
            [8002, 9000]
        );
    }

    #[test]
    fn public_ip_only() {
        for ip in ["203.0.113.7", "51.15.0.1", "2a01:4f8::1"] {
            assert!(public_ip(ip), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "172.16.0.9",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "x",
        ] {
            assert!(!public_ip(ip), "{ip}");
        }
    }

    #[test]
    fn protected_labels() {
        for n in ["ssh.galion.app", "admin-panel", "hades-gate", "x.root.y"] {
            assert!(is_protected(n), "{n}");
        }
        for n in [
            "galion.app",
            "galion-one",
            "tuner",
            "openmind-memory",
            "monkey",
        ] {
            assert!(!is_protected(n), "{n}");
        }
    }

    #[test]
    fn empty_state_has_no_plans() {
        let p = from_state(&json!({}));
        assert!(p.plans.is_empty() && p.skipped.is_empty());
        assert_eq!(p.to_json()["count"], 0);
        assert_eq!(p.to_json()["executed"], false);
    }
}
