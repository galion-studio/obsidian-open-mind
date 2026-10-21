// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The real POSEIDON backend: forwards GAIA directives for the four compute
//! systems to a running `openmind-queue` service over loopback HTTP.
//!
//! What it may do is fixed by one allow-list, [`allowed`]:
//!
//! | Call | Why |
//! | --- | --- |
//! | `GET /healthz`, `GET /gaia/status`, `GET /stats`, `GET /vram`, `GET /dlq`, `GET /jobs/{id}` | read state |
//! | `POST /gaia/dispatch` (verbs `status` and `plan` only) | the queue's own read-only Gaia answer |
//! | `POST /jobs` | submit a job (`queues run`); the queue enforces quotas, spend caps, money words, backpressure and VRAM fit |
//!
//! It never leases, acks, nacks, extends or redrives a job, so it never
//! charges a reservation; it never writes caps, quotas or VRAM devices
//! (budget is **read-only** through Gaia), and it never compacts the log.
//! Those stay operator actions on `openmind-queue` itself. There is no
//! fallback to the stand-in: if the queue is unreachable the directive is
//! refused and GAIA sees POSEIDON degraded.

use crate::contract::{self, Violation};
use crate::Poseidon;
use gaia_core::{Directive, Health, Json, Outcome, Report, System, Verb};
use serde_json::{json, Value};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

/// Environment variable naming the queue address, e.g. `127.0.0.1:19240`.
pub const ENV_ADDR: &str = "GAIA_POSEIDON_QUEUE";

/// Integrity GAIA sees while the queue cannot be reached (below
/// `Health::DEGRADE_BELOW`, so POSEIDON shows as degraded).
pub const UNREACHABLE_INTEGRITY: u8 = 30;

/// Integrity while the program on the configured port is not
/// `openmind-queue` (POSEIDON), or its last answer broke the contract.
pub const OFF_CONTRACT_INTEGRITY: u8 = 20;

/// Something GAIA should hear about on the bus, taken by
/// `Poseidon::handle` after each directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub topic: &'static str,
    pub body: String,
}

/// Where the queue is and how long to wait for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueConfig {
    pub addr: String,
    pub timeout: Duration,
}

impl QueueConfig {
    /// A loopback `host:port`. Non-loopback addresses are refused: the queue
    /// API has no authentication, so it stays on loopback (as the queue's own
    /// default bind does).
    pub fn new(addr: &str) -> Result<QueueConfig, String> {
        let sa: SocketAddr = addr
            .to_socket_addrs()
            .map_err(|e| format!("queue address {addr:?}: {e}"))?
            .next()
            .ok_or_else(|| format!("queue address {addr:?} did not resolve"))?;
        if !sa.ip().is_loopback() {
            return Err(format!(
                "queue address {addr:?} is not loopback; openmind-queue has no auth, Gaia only talks to it on loopback"
            ));
        }
        Ok(QueueConfig {
            addr: addr.to_string(),
            timeout: Duration::from_millis(2_000),
        })
    }

    /// From [`ENV_ADDR`], if set and non-empty.
    pub fn from_env() -> Option<Result<QueueConfig, String>> {
        std::env::var(ENV_ADDR)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| QueueConfig::new(v.trim()))
    }
}

/// One HTTP exchange. A trait so unit tests can record every call.
pub trait Transport {
    fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> io::Result<(u16, String)>;
}

/// The real transport: `openmind_queue::http::request` (std only).
#[derive(Clone, Debug)]
pub struct Http {
    pub cfg: QueueConfig,
}

impl Transport for Http {
    fn call(&mut self, method: &str, path: &str, body: Option<&str>) -> io::Result<(u16, String)> {
        openmind_queue::http::request(&self.cfg.addr, method, path, body, self.cfg.timeout)
    }
}

/// The adapter's allow-list. Anything else is never sent.
pub fn allowed(method: &str, path: &str) -> bool {
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, segs.as_slice()) {
        ("GET", ["healthz" | "stats" | "vram" | "dlq"]) => true,
        ("GET", ["gaia", "status"]) => true,
        ("GET", ["jobs", id]) => id.parse::<u64>().is_ok(),
        ("POST", ["gaia", "dispatch"]) => true,
        ("POST", ["jobs"]) => true,
        _ => false,
    }
}

/// Fields a Gaia job submission may carry (the queue's `JobSpec`).
const JOB_FIELDS: &[&str] = &[
    "queue",
    "agent",
    "kind",
    "priority",
    "max_attempts",
    "cost",
    "lease_ms",
    "payload",
    "vram_gb",
    "cpu_fallback",
];

/// The POSEIDON backend that talks to `openmind-queue`.
#[derive(Debug)]
pub struct QueueAdapter<T: Transport = Http> {
    transport: T,
    addr: String,
    reachable: Option<bool>,
    load: u8,
    last_error: Option<String>,
    calls: u64,
    submitted: u64,
    refusals: u64,
    /// Version from a verified `GET /healthz` (identity handshake).
    identity: Option<String>,
    /// The last contract violation, until the next good answer.
    off_contract: Option<String>,
    violations: u64,
    events: Vec<Event>,
}

impl QueueAdapter<Http> {
    pub fn new(cfg: QueueConfig) -> QueueAdapter<Http> {
        let addr = cfg.addr.clone();
        QueueAdapter::with_transport(Http { cfg }, &addr)
    }
}

fn int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| i64::try_from(u).unwrap_or(i64::MAX)))
}

fn text(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

impl<T: Transport> QueueAdapter<T> {
    pub fn with_transport(transport: T, addr: &str) -> QueueAdapter<T> {
        QueueAdapter {
            transport,
            addr: addr.to_string(),
            reachable: None,
            load: 0,
            last_error: None,
            calls: 0,
            submitted: 0,
            refusals: 0,
            identity: None,
            off_contract: None,
            violations: 0,
            events: Vec::new(),
        }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// The transport (tests inspect the recorded calls).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Send one allowed call; parse the JSON body.
    fn call(
        &mut self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), String> {
        if !allowed(method, path) {
            // Unreachable by construction; kept as a hard guard.
            return Err(format!(
                "adapter refuses {method} {path}: not on the read/submit allow-list"
            ));
        }
        if path != "/healthz" {
            self.ensure_identity()?;
        }
        self.raw_call(method, path, body)
    }

    fn raw_call(
        &mut self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), String> {
        self.calls += 1;
        let b = body.map(|v| v.to_string());
        match self.transport.call(method, path, b.as_deref()) {
            Ok((code, s)) => {
                self.reachable = Some(true);
                if self.off_contract.is_none() {
                    self.last_error = None;
                }
                Ok((code, serde_json::from_str(&s).unwrap_or(Value::Null)))
            }
            Err(e) => {
                let msg = format!("openmind-queue unreachable at {}: {e}", self.addr);
                if self.reachable != Some(false) {
                    self.events.push(Event {
                        topic: "poseidon-unreachable",
                        body: msg.clone(),
                    });
                }
                self.reachable = Some(false);
                // Whatever answers next on this port must prove itself again.
                self.identity = None;
                self.last_error = Some(msg.clone());
                Err(msg)
            }
        }
    }

    /// Identity handshake: the first call (and the first after an outage or
    /// a reset) checks that `GET /healthz` names `openmind-queue` serving
    /// POSEIDON. Nothing else is sent to an unverified port.
    fn ensure_identity(&mut self) -> Result<(), String> {
        if self.identity.is_some() {
            return Ok(());
        }
        let (code, v) = self.raw_call("GET", "/healthz", None)?;
        match contract::check_identity(code, &v) {
            Ok(version) => {
                self.identity = Some(version);
                self.clear_violation();
                Ok(())
            }
            Err(viol) => Err(self.record_violation(viol)),
        }
    }

    fn record_violation(&mut self, v: Violation) -> String {
        let msg = format!("{v} (openmind-queue at {})", self.addr);
        self.violations += 1;
        if self.off_contract.as_deref() != Some(msg.as_str()) {
            self.events.push(Event {
                topic: "poseidon-off-contract",
                body: msg.clone(),
            });
        }
        self.off_contract = Some(msg.clone());
        self.last_error = Some(msg.clone());
        msg
    }

    fn clear_violation(&mut self) {
        if self.off_contract.take().is_some() {
            self.last_error = None;
        }
    }

    /// Bus events since the last call (unreachable, off-contract, spend-cap
    /// hard stops). `Poseidon::handle` forwards them to GAIA.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Version of the verified queue, once the handshake passed.
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// Contract violations seen so far.
    pub fn violations(&self) -> u64 {
        self.violations
    }

    /// Refresh the cached load from `GET /gaia/status`.
    pub fn refresh(&mut self) {
        if let Ok((200, v)) = self.call("GET", "/gaia/status", None) {
            if let Some(l) = v.pointer("/health/load").and_then(int) {
                self.load = l.clamp(0, 100) as u8;
            }
        }
    }

    pub fn reachable(&self) -> Option<bool> {
        self.reachable
    }

    pub fn load(&self) -> u8 {
        self.load
    }

    pub fn health(&self) -> Health {
        let mut h = Health::ok(self.load);
        if self.reachable == Some(false) {
            h.integrity = UNREACHABLE_INTEGRITY;
        } else if self.off_contract.is_some() {
            h.integrity = OFF_CONTRACT_INTEGRITY;
        }
        h
    }

    pub fn state_json(&self) -> Json {
        Json::obj()
            .with("backend", "openmind-queue")
            .with("addr", self.addr.as_str())
            .with(
                "reachable",
                match self.reachable {
                    Some(true) => "yes",
                    Some(false) => "no",
                    None => "unknown",
                },
            )
            .with("load", self.load)
            .with("calls", self.calls)
            .with("submitted", self.submitted)
            .with("refusals", self.refusals)
            .with("last_error", self.last_error.clone())
            .with("identity", self.identity.clone())
            .with(
                "contract",
                if self.off_contract.is_some() {
                    "violated"
                } else {
                    "ok"
                },
            )
            .with("violations", self.violations)
            .with("budget", "read-only via Gaia (never spends)")
    }

    /// Clear cached state (GAIA reboot). The queue itself is untouched.
    pub fn reset(&mut self) {
        self.reachable = None;
        self.load = 0;
        self.last_error = None;
        self.identity = None;
        self.off_contract = None;
        self.events.clear();
    }

    /// Handle one directive aimed at a compute system.
    pub fn handle(&mut self, d: &Directive) -> Report {
        let r = self.handle_inner(d);
        if r.outcome == Outcome::Refused {
            self.refusals += 1;
        }
        r
    }

    fn handle_inner(&mut self, d: &Directive) -> Report {
        if Poseidon::is_money_request(&d.payload)
            || openmind_queue::engine::MONEY_WORDS
                .iter()
                .any(|w| d.payload.to_ascii_lowercase().contains(w))
        {
            return Report::for_directive(d, Outcome::Refused)
                .summary("real-money access is not approved; POSEIDON tracks caps only");
        }
        if !d.target.is_compute() {
            return Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{} is not a compute system", d.target));
        }
        let out = match (d.target, d.verb) {
            (System::Queues, Verb::Run | Verb::Execute) => self.submit(d),
            (System::Queues, Verb::Status) if !d.payload.trim().is_empty() => self.inspect(d),
            (_, Verb::Status) => self.status(d),
            (_, Verb::Plan) => self.remote_dispatch(d, Verb::Plan),
            (s, _) => Ok(Report::for_directive(d, Outcome::Refused).summary(format!(
                "{s} {}: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget",
                d.verb.name()
            ))),
        };
        out.unwrap_or_else(|e| {
            let kind = if e.starts_with("contract violation") {
                "off_contract"
            } else {
                "unreachable"
            };
            Report::for_directive(d, Outcome::Refused)
                .summary(format!("{e}; nothing done (no fallback to the stand-in)"))
                .metric(kind, 1)
        })
    }

    /// `POST /gaia/dispatch` (verbs `status` and `plan` only) and parse the
    /// answer through the strict contract parser.
    fn remote_dispatch(&mut self, d: &Directive, verb: Verb) -> Result<Report, String> {
        if !contract::dispatchable(verb) {
            // Unreachable by construction; kept as a hard guard.
            return Err(format!(
                "adapter refuses to dispatch verb {}: the queue's Gaia dispatch is read-only",
                verb.name()
            ));
        }
        let system = contract::wire_system(d.target)
            .ok_or_else(|| format!("{} is not served by openmind-queue", d.target))?;
        let body =
            json!({"system": system, "verb": contract::wire_verb(verb), "payload": d.payload});
        let (code, v) = self.call("POST", "/gaia/dispatch", Some(body))?;
        if code != 200 {
            return Ok(Report::for_directive(d, Outcome::Rejected)
                .summary(format!(
                    "openmind-queue answered {code}: {}",
                    text(&v, "error")
                ))
                .metric("http_status", i64::from(code)));
        }
        match contract::parse_dispatch(d, verb, &v) {
            Ok(r) => {
                self.clear_violation();
                Ok(r)
            }
            Err(viol) => Err(self.record_violation(viol)),
        }
    }

    fn status(&mut self, d: &Directive) -> Result<Report, String> {
        let mut r = self.remote_dispatch(d, Verb::Status)?;
        match d.target {
            System::VramPool => {
                let (code, v) = self.call("GET", "/vram", None)?;
                if code == 200 {
                    let devs = v
                        .get("devices")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut names = Vec::new();
                    for x in &devs {
                        let id = x
                            .pointer("/device/id")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        let gb = x.pointer("/device/vram_gb").and_then(int).unwrap_or(0);
                        let free = x.get("free_gb").and_then(int).unwrap_or(0);
                        let st = x
                            .pointer("/device/state")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        let routable = x.get("routable").and_then(Value::as_bool).unwrap_or(false);
                        names.push(format!(
                            "{id} {gb} GB {st}{} free {free}",
                            if routable { "" } else { " (not routable)" }
                        ));
                    }
                    r = r.metric("devices", devs.len() as i64);
                    if !names.is_empty() {
                        r.summary = format!("{}; devices: {}", r.summary, names.join(", "));
                    }
                }
            }
            System::Quotas | System::BudgetCaps => {
                let (code, v) = self.call("GET", "/stats", None)?;
                if code == 200 {
                    if let Some(Value::Object(agents)) = v.get("agents") {
                        for (name, a) in agents {
                            if d.target == System::Quotas {
                                for (k, p) in [
                                    ("pending", "/pending"),
                                    ("inflight", "/inflight"),
                                    ("max_pending", "/quota/0"),
                                    ("max_inflight", "/quota/1"),
                                ] {
                                    if let Some(n) = a.pointer(p).and_then(int) {
                                        r = r.metric(&format!("{k}:{name}"), n);
                                    }
                                }
                            } else {
                                for k in ["reserved", "remaining"] {
                                    if let Some(n) = a.get(k).and_then(int) {
                                        r = r.metric(&format!("{k}:{name}"), n);
                                    }
                                }
                                if a.get("budget_blocked").and_then(Value::as_bool) == Some(true) {
                                    r = r.metric(&format!("blocked:{name}"), 1);
                                }
                            }
                        }
                    }
                    if d.target == System::BudgetCaps {
                        if let Some(c) = v.pointer("/global/cap").and_then(int) {
                            r = r.metric("global_cap", c);
                        }
                    }
                }
            }
            System::Queues => {}
            _ => {}
        }
        if let Ok((200, v)) = self.call("GET", "/gaia/status", None) {
            if let Some(l) = v.pointer("/health/load").and_then(int) {
                self.load = l.clamp(0, 100) as u8;
            }
        }
        Ok(r)
    }

    /// `queues status` with a payload: `job:<id>`, `<id>` or `dlq`.
    fn inspect(&mut self, d: &Directive) -> Result<Report, String> {
        let p = d.payload.trim();
        if p == "dlq" {
            let (code, v) = self.call("GET", "/dlq", None)?;
            let dead = v
                .get("dead")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let ids: Vec<String> = dead
                .iter()
                .filter_map(|j| j.get("id").and_then(int))
                .map(|i| i.to_string())
                .collect();
            return Ok(Report::for_directive(
                d,
                if code == 200 {
                    Outcome::Done
                } else {
                    Outcome::Rejected
                },
            )
            .summary(format!(
                "{} dead-lettered job(s){}",
                dead.len(),
                if ids.is_empty() {
                    String::new()
                } else {
                    format!(": {}", ids.join(", "))
                }
            ))
            .metric("dead", dead.len() as i64));
        }
        let id: u64 = match p.strip_prefix("job:").unwrap_or(p).trim().parse() {
            Ok(n) => n,
            Err(_) => {
                return Ok(Report::for_directive(d, Outcome::Rejected)
                    .summary("queues status payload must be empty, `dlq`, `job:<id>` or `<id>`"))
            }
        };
        let (code, v) = self.call("GET", &format!("/jobs/{id}"), None)?;
        let r = Report::for_directive(d, Outcome::Done).metric("job_id", id as i64);
        if code == 404 {
            return Ok(r
                .summary(format!("job {id} is not live on openmind-queue (acked jobs are not kept, or it never existed)"))
                .metric("live", 0));
        }
        if code != 200 {
            return Ok(Report::for_directive(d, Outcome::Rejected)
                .summary(format!(
                    "openmind-queue answered {code}: {}",
                    text(&v, "error")
                ))
                .metric("http_status", i64::from(code)));
        }
        let spec = v.get("spec").cloned().unwrap_or(Value::Null);
        let mut r = r
            .summary(format!(
                "job {id} {} on queue {} for agent {} (kind {}, attempts {}/{}, cost {}, vram {} GB{}), created {}",
                text(&v, "state"),
                text(&spec, "queue"),
                text(&spec, "agent"),
                text(&spec, "kind"),
                text(&v, "attempts"),
                text(&spec, "max_attempts"),
                text(&spec, "cost"),
                text(&spec, "vram_gb"),
                match text(&v, "device").as_str() { "" => String::new(), dev => format!(", device {dev}") },
                text(&v, "created"),
            ))
            .metric("live", 1);
        for k in ["attempts", "priority"] {
            if let Some(n) = v.get(k).or_else(|| spec.get(k)).and_then(int) {
                r = r.metric(k, n);
            }
        }
        if let Some(n) = spec.get("cost").and_then(int) {
            r = r.metric("cost", n);
        }
        Ok(r)
    }

    /// `queues run`: payload is a JSON object with `JobSpec` fields.
    fn submit(&mut self, d: &Directive) -> Result<Report, String> {
        let p = d.payload.trim();
        let spec: Value = if p.is_empty() {
            json!({})
        } else {
            match serde_json::from_str::<Value>(p) {
                Ok(v @ Value::Object(_)) => v,
                _ => {
                    return Ok(Report::for_directive(d, Outcome::Rejected).summary(
                        "queues run payload must be a JSON object of job fields (queue, agent, kind, priority, max_attempts, cost, lease_ms, payload, vram_gb, cpu_fallback)",
                    ))
                }
            }
        };
        if let Value::Object(m) = &spec {
            if let Some(k) = m.keys().find(|k| !JOB_FIELDS.contains(&k.as_str())) {
                return Ok(Report::for_directive(d, Outcome::Rejected)
                    .summary(format!("unknown job field {k:?}")));
            }
        }
        let agent = text(&spec, "agent");
        let (code, v) = self.call("POST", "/jobs", Some(spec))?;
        if code == 201 {
            self.submitted += 1;
            let id = v.get("id").and_then(int).unwrap_or(-1);
            return Ok(Report::for_directive(d, Outcome::Done)
                .summary(format!(
                    "job {id} queued on openmind-queue {} at {}",
                    self.addr,
                    text(&v, "time")
                ))
                .metric("job_id", id)
                .metric("http_status", 201));
        }
        let err = text(&v, "error");
        let detail = text(&v, "detail");
        if err == "budget_hard_stop" {
            self.events.push(Event {
                topic: "budget-hard-stop",
                body: format!("agent {agent:?}: {detail}"),
            });
        }
        let outcome = match code {
            402 | 403 | 429 | 503 => Outcome::Refused,
            _ => Outcome::Rejected,
        };
        let what = match err.as_str() {
            "budget_hard_stop" => "spend cap hard stop",
            "quota_exceeded" => "quota exceeded",
            "real_money_not_approved" => "real money is not approved",
            "backpressure" => "backpressure (queue full)",
            "vram_unavailable" => "VRAM unavailable",
            "bad_request" => "bad request",
            _ => "not accepted",
        };
        Ok(Report::for_directive(d, outcome)
            .summary(format!(
                "openmind-queue refused the job: {what} ({err}: {detail})"
            ))
            .metric("http_status", i64::from(code)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every call and answers from a script.
    #[derive(Default, Debug)]
    struct Fake {
        calls: Vec<(String, String, Option<String>)>,
        answers: Vec<(u16, String)>,
        down: bool,
        /// Answer `/healthz` as some other program.
        impostor: bool,
    }

    const HEALTHZ: &str =
        r#"{"ok":true,"function":"POSEIDON","service":"openmind-queue","version":"0.1.0"}"#;

    impl Transport for Fake {
        fn call(&mut self, m: &str, p: &str, b: Option<&str>) -> io::Result<(u16, String)> {
            self.calls.push((m.into(), p.into(), b.map(String::from)));
            if self.down {
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, "refused"));
            }
            if p == "/healthz" {
                return Ok(if self.impostor {
                    (200, r#"{"ok":true,"service":"galion-one"}"#.into())
                } else {
                    (200, HEALTHZ.into())
                });
            }
            Ok(if self.answers.is_empty() {
                if p == "/gaia/dispatch" {
                    // An on-contract echo, like the real queue's dispatch.
                    let req: Value = serde_json::from_str(b.unwrap_or("{}")).unwrap();
                    let plan = req["verb"] == "plan";
                    let ans = json!({"function": "POSEIDON", "system": req["system"],
                        "verb": req["verb"], "outcome": if plan { "dry-run" } else { "done" },
                        "dry_run": plan, "summary": "fake", "metrics": []});
                    (200, ans.to_string())
                } else {
                    (200, "{}".into())
                }
            } else {
                self.answers.remove(0)
            })
        }
    }

    fn adapter(answers: Vec<(u16, &str)>) -> QueueAdapter<Fake> {
        QueueAdapter::with_transport(
            Fake {
                answers: answers
                    .into_iter()
                    .map(|(c, s)| (c, s.to_string()))
                    .collect(),
                ..Fake::default()
            },
            "127.0.0.1:19240",
        )
    }

    #[test]
    fn allow_list_is_read_plus_submit_only() {
        for (m, p) in [
            ("GET", "/stats"),
            ("GET", "/vram"),
            ("GET", "/jobs/7"),
            ("POST", "/jobs"),
            ("POST", "/gaia/dispatch"),
        ] {
            assert!(allowed(m, p), "{m} {p}");
        }
        for (m, p) in [
            ("POST", "/caps"),
            ("POST", "/quotas"),
            ("POST", "/lease"),
            ("POST", "/jobs/7/ack"),
            ("POST", "/jobs/7/nack"),
            ("POST", "/jobs/7/extend"),
            ("POST", "/jobs/7/redrive"),
            ("POST", "/vram/devices"),
            ("DELETE", "/vram/devices/gpu0"),
            ("POST", "/vram/sync-providers"),
            ("POST", "/admin/compact"),
            ("GET", "/jobs/x"),
        ] {
            assert!(!allowed(m, p), "{m} {p}");
        }
    }

    #[test]
    fn every_directive_stays_on_the_allow_list() {
        let mut a = adapter(vec![]);
        for s in System::COMPUTE {
            for v in [Verb::Status, Verb::Run, Verb::Plan, Verb::Execute] {
                for p in [
                    "",
                    "40",
                    "job:3",
                    "dlq",
                    r#"{"agent":"a","cost":5}"#,
                    "raise cap 1000",
                ] {
                    let _ = a.handle(&Directive::new(1, s, v).payload(p));
                }
            }
        }
        for (m, p, _) in &a.transport().calls {
            assert!(allowed(m, p), "{m} {p}");
            assert!(!p.contains("caps") && !p.contains("quotas") && !p.contains("lease"));
            assert!(!p.ends_with("/ack"));
        }
        // dispatch is only ever sent status or plan
        for (_, p, b) in &a.transport().calls {
            if p == "/gaia/dispatch" {
                let v: Value = serde_json::from_str(b.as_deref().unwrap()).unwrap();
                assert!(matches!(v["verb"].as_str(), Some("status" | "plan")));
            }
        }
    }

    #[test]
    fn budget_quota_vram_writes_are_refused_without_a_call() {
        let mut a = adapter(vec![]);
        for s in [System::BudgetCaps, System::Quotas, System::VramPool] {
            for v in [Verb::Run, Verb::Execute] {
                let r = a.handle(&Directive::new(1, s, v).payload("100"));
                assert_eq!(r.outcome, Outcome::Refused, "{s} {v:?}");
                assert!(r.summary.contains("read-only"));
            }
        }
        assert!(a.transport().calls.is_empty());
    }

    #[test]
    fn money_refused_before_any_call() {
        let mut a = adapter(vec![]);
        let r =
            a.handle(&Directive::new(1, System::Queues, Verb::Run).payload(r#"{"kind":"payout"}"#));
        assert_eq!(r.outcome, Outcome::Refused);
        assert!(a.transport().calls.is_empty());
    }

    #[test]
    fn submit_maps_queue_refusals() {
        let cases = [
            (201, r#"{"id":4,"time":"t"}"#, Outcome::Done, "job 4 queued"),
            (
                402,
                r#"{"error":"budget_hard_stop","detail":"budget a: spent 90 + cost 20 > cap 100"}"#,
                Outcome::Refused,
                "spend cap hard stop",
            ),
            (
                429,
                r#"{"error":"quota_exceeded","detail":"x"}"#,
                Outcome::Refused,
                "quota exceeded",
            ),
            (
                503,
                r#"{"error":"vram_unavailable","detail":"x"}"#,
                Outcome::Refused,
                "VRAM unavailable",
            ),
            (
                400,
                r#"{"error":"bad_request","detail":"x"}"#,
                Outcome::Rejected,
                "bad request",
            ),
        ];
        for (code, body, outcome, word) in cases {
            let mut a = adapter(vec![(code, body)]);
            let r = a.handle(
                &Directive::new(1, System::Queues, Verb::Run).payload(r#"{"agent":"a","cost":20}"#),
            );
            assert_eq!(r.outcome, outcome, "{code}");
            assert!(r.summary.contains(word), "{}", r.summary);
            assert_eq!(a.transport().calls[0].1, "/healthz");
            assert_eq!(a.transport().calls[1].1, "/jobs");
        }
    }

    #[test]
    fn submit_rejects_unknown_fields_and_non_objects() {
        let mut a = adapter(vec![]);
        for p in [r#"{"token":1}"#, "[1]", "not json"] {
            let r = a.handle(&Directive::new(1, System::Queues, Verb::Run).payload(p));
            assert_eq!(r.outcome, Outcome::Rejected, "{p}");
        }
        assert!(a.transport().calls.is_empty());
    }

    #[test]
    fn inspect_job_and_status_translate() {
        let mut a = adapter(vec![(
            200,
            r#"{"id":3,"state":"ready","attempts":0,"created":"2026-10-20T20:00:00.000+02:00","spec":{"queue":"default","agent":"a","kind":"echo","priority":2,"max_attempts":3,"cost":5,"vram_gb":0}}"#,
        )]);
        let r = a.handle(&Directive::new(1, System::Queues, Verb::Status).payload("job:3"));
        assert_eq!(r.outcome, Outcome::Done);
        assert!(r.summary.contains("job 3 ready"), "{}", r.summary);
        assert_eq!(r.metric_value("cost"), Some(5));
        let mut a = adapter(vec![(
            200,
            r#"{"function":"POSEIDON","system":"queues","verb":"status","outcome":"done","dry_run":false,"summary":"1 ready","metrics":[["ready",1],["dead",0]]}"#,
        )]);
        let r = a.handle(&Directive::new(1, System::Queues, Verb::Status));
        assert_eq!(r.outcome, Outcome::Done);
        assert_eq!(r.metric_value("ready"), Some(1));
    }

    #[test]
    fn unreachable_queue_refuses_and_degrades() {
        let mut a = adapter(vec![]);
        a.transport.down = true;
        let r = a.handle(&Directive::new(1, System::Queues, Verb::Status));
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(r.metric_value("unreachable"), Some(1));
        assert!(a.health().integrity < Health::DEGRADE_BELOW);
    }

    #[test]
    fn config_is_loopback_only() {
        assert!(QueueConfig::new("127.0.0.1:19240").is_ok());
        assert!(QueueConfig::new("[::1]:19240").is_ok());
        assert!(QueueConfig::new("10.0.0.5:19240").is_err());
        assert!(QueueConfig::new("nonsense").is_err());
    }

    #[test]
    fn identity_handshake_once_then_cached() {
        let mut a = adapter(vec![]);
        for _ in 0..3 {
            let _ = a.handle(&Directive::new(1, System::Queues, Verb::Status));
        }
        let hz = a
            .transport()
            .calls
            .iter()
            .filter(|c| c.1 == "/healthz")
            .count();
        assert_eq!(hz, 1);
        assert_eq!(a.transport().calls[0].1, "/healthz");
        assert_eq!(a.identity(), Some("0.1.0"));
    }

    #[test]
    fn impostor_on_the_port_gets_nothing_but_healthz() {
        let mut a = adapter(vec![]);
        a.transport.impostor = true;
        for s in System::COMPUTE {
            for v in [Verb::Status, Verb::Plan, Verb::Run] {
                let p = if v == Verb::Run {
                    r#"{"agent":"a"}"#
                } else {
                    ""
                };
                let r = a.handle(&Directive::new(1, s, v).payload(p));
                assert_eq!(r.outcome, Outcome::Refused, "{s} {v:?}");
            }
        }
        assert!(a.transport().calls.iter().all(|c| c.1 == "/healthz"));
        assert!(a.violations() > 0);
        assert_eq!(a.health().integrity, OFF_CONTRACT_INTEGRITY);
        let ev = a.take_events();
        assert_eq!(ev.len(), 1, "one event per distinct violation: {ev:?}");
        assert_eq!(ev[0].topic, "poseidon-off-contract");
        let r = a.handle(&Directive::new(9, System::Queues, Verb::Status));
        assert_eq!(r.metric_value("off_contract"), Some(1));
        // The right program comes back: handshake passes, health recovers.
        a.transport.impostor = false;
        let r = a.handle(&Directive::new(10, System::Queues, Verb::Status));
        assert_eq!(r.outcome, Outcome::Done, "{}", r.summary);
        assert!(a.health().integrity >= Health::DEGRADE_BELOW);
    }

    #[test]
    fn off_contract_dispatch_answer_is_refused_and_degrades() {
        let mut a = adapter(vec![(
            200,
            r#"{"function":"POSEIDON","system":"queues","verb":"status","outcome":"maybe","summary":""}"#,
        )]);
        let r = a.handle(&Directive::new(1, System::Queues, Verb::Status));
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(r.metric_value("off_contract"), Some(1));
        assert!(r.summary.contains("unknown outcome"), "{}", r.summary);
        assert!(a.health().integrity < Health::DEGRADE_BELOW);
        assert_eq!(a.take_events()[0].topic, "poseidon-off-contract");
    }

    #[test]
    fn plan_answer_keeps_dry_run_flag() {
        let mut a = adapter(vec![(
            200,
            r#"{"function":"POSEIDON","system":"vram-pool","verb":"plan","outcome":"dry-run","dry_run":true,"summary":"16 GB would go to device gpu0 (no spend)","metrics":[["free_gb_on_device",24]]}"#,
        )]);
        let r = a.handle(&Directive::new(1, System::VramPool, Verb::Plan).payload("16"));
        assert_eq!(r.outcome, Outcome::DryRun);
        assert!(r.dry_run, "GAIA judges destructive work by dry_run");
        assert_eq!(r.metric_value("free_gb_on_device"), Some(24));
    }

    #[test]
    fn outage_events_once_and_rehandshake_after() {
        let mut a = adapter(vec![]);
        let _ = a.handle(&Directive::new(1, System::Queues, Verb::Status));
        assert!(a.identity().is_some());
        a.transport.down = true;
        for _ in 0..3 {
            let _ = a.handle(&Directive::new(2, System::Queues, Verb::Status));
        }
        let ev = a.take_events();
        assert_eq!(ev.len(), 1, "{ev:?}");
        assert_eq!(ev[0].topic, "poseidon-unreachable");
        assert!(a.identity().is_none());
        a.transport.down = false;
        let n = a.transport().calls.len();
        let _ = a.handle(&Directive::new(3, System::Queues, Verb::Status));
        assert_eq!(a.transport().calls[n].1, "/healthz");
    }

    #[test]
    fn budget_hard_stop_is_an_event() {
        let mut a = adapter(vec![(
            402,
            r#"{"error":"budget_hard_stop","detail":"budget a: spent 90 + cost 20 > cap 100"}"#,
        )]);
        let _ = a.handle(
            &Directive::new(1, System::Queues, Verb::Run).payload(r#"{"agent":"a","cost":20}"#),
        );
        let ev = a.take_events();
        assert_eq!(ev[0].topic, "budget-hard-stop");
        assert!(ev[0].body.contains("cap 100"));
    }
}
