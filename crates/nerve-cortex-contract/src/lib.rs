// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The nerve <-> cortex wire contract (version [`CONTRACT_VERSION`]).
//!
//! Full prose: `docs/cortex/event-contract.md`.
//!
//! The event-driven nerve (branch `swarm/nerve-event-driven`,
//! `openmind_nerve::evt`) already defines the wire types; this crate
//! **consumes them field for field** so the cortex does not need the whole
//! nerve crate to parse a line, and it also accepts the older B33
//! `NerveEvent` envelope (`{ts, source, kind, severity, data}`) of the
//! polling nerve's `GET /events`.
//!
//! ## nerve -> cortex: events
//!
//! One JSON object per NDJSON line (`evt::NerveEvent`, schema `v` 1):
//!
//! ```json
//! {"v":1,"seq":42,"ts":"2026-10-21T18:40:00+02:00","t":1792600800,"source":"journal",
//!  "kind":"unit.failed","severity":"high","unit":"tuner.service","group":"other",
//!  "key":"unit.failed:tuner.service","msg":"...","data":{"exit_code":"exited"},"count":1}
//! ```
//!
//! `severity`: `info|low|medium|high|critical` (nerve) or `info|warn|error|critical`
//! (B33/B41 `high`/`crit`); see [`sev_rank`]. `kind`: see [`EventKind::parse`].
//!
//! Transports (the cortex dedupes by [`Event::fingerprint`] = source+kind+seq):
//! 1. **subscribe** (default, zero idle work): `GET <nerve>/events/stream?since=SEQ`
//!    NDJSON, held open; the cortex blocks in `read()` and resumes from the
//!    last `seq` after a reconnect (`stream.gap` / `stream.keepalive` lines
//!    are housekeeping);
//! 2. push: `POST <cortex>/v1/events` with one event, a JSON array, or NDJSON;
//! 3. unix socket: write NDJSON lines to the cortex socket;
//! 4. pull (off by default): `GET <nerve>/events?n=N` (polling nerve);
//! 5. file: `gaia-cortex replay FILE`.
//!
//! ## cortex -> nerve: decisions
//!
//! The cortex never runs a command. It asks the nerve: `POST <nerve>/actions`
//! with an [`ActionRequest`] (the nerve's `evt::actions::parse` body:
//! `action`, `unit`, `reason`, `request_id`, `source`, `dry_run`, plus
//! cortex-only fields the nerve ignores). The nerve checks
//! `reflex::permitted`, its caps and its dry-run switches and replies with
//! `{request_id, result: dry-run|executed|refused|rate-limited|failed, ...}`
//! (HTTP 200/403/429/500), read by [`ActionResult::from_nerve`]. The only
//! action the cortex can express is [`ActionKind::Restart`].
//! Alerts go to the nerve's `POST /event` with kind `cortex.alert` /
//! `cortex.escalation` (ignored when they come back on the stream).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire contract version (matches `evt::SCHEMA_VERSION`).
pub const CONTRACT_VERSION: u32 = 1;

/// Nerve (event mode): NDJSON event stream.
pub const NERVE_STREAM_PATH: &str = "/events/stream";
/// Nerve (event mode): ring events after `since` WITH their current `count`
/// (`GET /evt?since=SEQ&n=N`, at most 1000 per call). The stream sends each
/// event once, when it is first published; later duplicates the nerve folds
/// into it only raise its `count` in the ring. The cortex re-reads the ring
/// for the failure events it decided on, so a crash loop inside the nerve's
/// dedupe window still reaches the flap rule (slot 235, T5.1 finding F1).
pub const NERVE_RING_PATH: &str = "/evt";
/// `data` field of a cortex-made repeat event: `{"of_seq", "total", "new"}`.
pub const REPEAT_FIELD: &str = "repeat";
/// Nerve executor caps (`evt::actions::PER_UNIT_PER_HOUR` / `TOTAL_PER_HOUR`
/// on `swarm/nerve-event-driven` bb287cc). The nerve applies them only to
/// EXECUTED restarts, so a dry-run nerve never says `rate-limited`; the
/// cortex applies the same numbers itself in every mode.
pub const NERVE_RESTARTS_PER_UNIT_HOUR: u32 = 2;
pub const NERVE_RESTARTS_TOTAL_HOUR: u32 = 6;
/// Nerve (polling mode): tail of the JSONL event log (`?n=N`).
pub const NERVE_EVENTS_PATH: &str = "/events";
/// Nerve: append one event (cortex alerts land here).
pub const NERVE_EVENT_POST_PATH: &str = "/event";
/// Nerve: remediation request (`evt::actions`).
pub const NERVE_ACTIONS_PATH: &str = "/actions";
/// Cortex: push ingest for events.
pub const CORTEX_INGEST_PATH: &str = "/v1/events";
/// `source` of everything the cortex writes back.
pub const CORTEX_SOURCE: &str = "gaia-cortex";

/// Longest accepted `source`/`kind`/`severity`.
pub const MAX_FIELD: usize = 128;
/// Longest accepted serialized `data`.
pub const MAX_DATA_BYTES: usize = 16 * 1024;
/// Most events accepted in one push body.
pub const MAX_BATCH: usize = 1000;

/// Severity rank 0..=3 (info, warn, error, critical) for both spellings.
/// Nerve `low` ranks as info (below every alert), `medium` as warn,
/// `high` as error. Unknown = warn (never silently dropped).
pub fn sev_rank(s: &str) -> u8 {
    match s.trim().to_ascii_lowercase().as_str() {
        "info" | "debug" | "notice" | "event" | "low" => 0,
        "warn" | "warning" | "medium" => 1,
        "error" | "err" | "high" => 2,
        "critical" | "crit" | "fatal" | "alert" | "emerg" => 3,
        _ => 1,
    }
}

/// What happened, as the event-driven nerve reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    /// Unit active state change / lifecycle (`unit.started`, `unit.stopped`,
    /// `unit.finished`; `data.to` for B33 producers).
    UnitState,
    /// Unit entered `failed` (`unit.failed`, `unit.start_failed`, `unit.spawn_failed`).
    UnitCrash,
    /// systemd already scheduled an automatic restart (`unit.restart_scheduled`).
    RestartScheduled,
    /// systemd gave up (`unit.start_limit`, start-limit-hit).
    StartLimit,
    /// A process exited (`unit.exit` main process, `proc.exit` pidfd).
    ProcessExit,
    /// Kernel or cgroup OOM kill (`oom.kill`).
    Oom,
    /// A queued task finished (`task.done` ok, `task.failed`).
    TaskFinished,
    /// A swarm slot finished (`slot.done` ok, `slot.failed`; `slot.started` is info).
    SlotFinished,
    /// A metric crossed a threshold (`disk.crossed`, `cert.crossed`, `threshold.crossed`).
    Threshold,
    /// A process no inventory rule classifies.
    UnclassifiedProcess,
    /// The nerve's remediation outcome (`action.result`).
    Remediation,
    /// The nerve opened a circuit breaker for a unit.
    CircuitOpen,
    /// The cortex's own alert/escalation echoed back (ignored: no loops).
    CortexAlert,
    /// A polling-engine incident the nerve forwards (`incident.opened`,
    /// `incident.resolved`; `data.cause`).
    NerveIncident,
    /// Nerve bookkeeping, no decision: `stream.keepalive`, `stream.gap`, and
    /// `escalation.batch` (the nerve's delegate-to-cortex batch summary; its
    /// member events are already on the stream, and deciding on it would
    /// loop: cortex alert -> nerve batch -> cortex alert).
    Housekeeping,
    /// Anything else (`kernel.error`, `file.changed`, ...).
    Other,
}

impl EventKind {
    pub const ALL: [EventKind; 16] = [
        EventKind::UnitState,
        EventKind::UnitCrash,
        EventKind::RestartScheduled,
        EventKind::StartLimit,
        EventKind::ProcessExit,
        EventKind::Oom,
        EventKind::TaskFinished,
        EventKind::SlotFinished,
        EventKind::Threshold,
        EventKind::UnclassifiedProcess,
        EventKind::Remediation,
        EventKind::CircuitOpen,
        EventKind::NerveIncident,
        EventKind::CortexAlert,
        EventKind::Housekeeping,
        EventKind::Other,
    ];

    /// Canonical wire name (the nerve's spelling where it has one).
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::UnitState => "unit.state",
            EventKind::UnitCrash => "unit.failed",
            EventKind::RestartScheduled => "unit.restart_scheduled",
            EventKind::StartLimit => "unit.start_limit",
            EventKind::ProcessExit => "unit.exit",
            EventKind::Oom => "oom.kill",
            EventKind::TaskFinished => "task.done",
            EventKind::SlotFinished => "slot.done",
            EventKind::Threshold => "threshold.crossed",
            EventKind::UnclassifiedProcess => "process.unclassified",
            EventKind::Remediation => "action.result",
            EventKind::CircuitOpen => "circuit.open",
            EventKind::CortexAlert => "cortex.alert",
            EventKind::NerveIncident => "incident.opened",
            EventKind::Housekeeping => "stream.keepalive",
            EventKind::Other => "other",
        }
    }

    /// Parse a wire kind: the nerve's names plus common spellings.
    pub fn parse(s: &str) -> EventKind {
        let k = s.trim().to_ascii_lowercase().replace(['_', '-'], ".");
        match k.as_str() {
            "unit.state" | "unit.state.change" | "unit.state.changed" | "state.change"
            | "unit.changed" | "unit.started" | "unit.stopped" | "unit.finished" => {
                EventKind::UnitState
            }
            "unit.failed" | "unit.start.failed" | "unit.spawn.failed" | "unit.crash" | "crash"
            | "unit.crashed" | "service.crash" => EventKind::UnitCrash,
            "unit.restart.scheduled" => EventKind::RestartScheduled,
            "unit.start.limit" | "unit.start.limit.hit" => EventKind::StartLimit,
            "unit.exit" | "proc.exit" | "process.exit" | "exit" | "process.exited" => {
                EventKind::ProcessExit
            }
            "oom.kill" | "oom" | "oom.killed" | "oomkill" => EventKind::Oom,
            "task.done" | "task.failed" | "task.finished" | "task.end" => EventKind::TaskFinished,
            "slot.done" | "slot.failed" | "slot.finished" | "slot.end" | "slot.started" => {
                EventKind::SlotFinished
            }
            "threshold.crossed" | "threshold" | "threshold.crossing" | "disk.crossed"
            | "cert.crossed" | "mem.crossed" | "load.crossed" => EventKind::Threshold,
            "process.unclassified"
            | "unclassified.process"
            | "unclassified"
            | "proc.unclassified"
            | "inventory.unclassified" => EventKind::UnclassifiedProcess,
            "action.result" | "remediation.result" | "remediation" | "reflex.action" => {
                EventKind::Remediation
            }
            "circuit.open" | "circuit.opened" | "breaker.open" => EventKind::CircuitOpen,
            "cortex.alert" | "cortex.escalation" | "cortex.decision" => EventKind::CortexAlert,
            "incident.opened" | "incident.updated" | "incident.resolved" => {
                EventKind::NerveIncident
            }
            "stream.keepalive" | "stream.gap" | "escalation.batch" | "escalation.delegated" => {
                EventKind::Housekeeping
            }
            _ => EventKind::Other,
        }
    }
}

/// One event: the nerve's `evt::NerveEvent` v1, or a B33 envelope (the
/// nerve-only fields are then absent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default)]
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<u64>,
    pub source: String,
    pub kind: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    #[serde(default)]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

impl Event {
    pub fn new(source: &str, kind: &str, severity: &str, data: Value) -> Event {
        Event {
            v: None,
            seq: None,
            ts: String::new(),
            t: None,
            source: source.into(),
            kind: kind.into(),
            severity: severity.into(),
            unit: None,
            group: None,
            key: None,
            msg: None,
            data,
            count: None,
        }
    }

    pub fn kind(&self) -> EventKind {
        EventKind::parse(&self.kind)
    }

    pub fn rank(&self) -> u8 {
        sev_rank(&self.severity)
    }

    fn data_str(&self, k: &str) -> Option<&str> {
        self.data
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }

    /// The systemd unit (top-level `unit`, else `data.unit` / `data.service`),
    /// `.service` appended when no unit suffix is present. `-` and `-.slice`
    /// (kernel / root slice) are not units the cortex can act on.
    pub fn unit(&self) -> Option<String> {
        let u = self
            .unit
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| self.data_str("unit"))
            .or_else(|| self.data_str("service"))?;
        let u = u.trim();
        if u.is_empty() || u == "-" || u == "-.slice" || u.len() > 256 {
            return None;
        }
        // `.path` etc.: live on the VPS 2026-10-21 23:06 (gaia-hermes-handoff.path
        // trigger-limit-hit) became `gaia-hermes-handoff.path.service` (slot 235).
        const SUFFIX: [&str; 11] = [
            ".service",
            ".scope",
            ".slice",
            ".timer",
            ".socket",
            ".target",
            ".mount",
            ".path",
            ".automount",
            ".swap",
            ".device",
        ];
        Some(if SUFFIX.iter().any(|s| u.ends_with(s)) {
            u.to_string()
        } else {
            format!("{u}.service")
        })
    }

    pub fn host(&self) -> Option<&str> {
        self.data_str("host")
    }

    pub fn comm(&self) -> Option<&str> {
        self.data_str("comm").or_else(|| self.data_str("process"))
    }

    /// Threshold subject: `data.metric`, else `data.path` (disk/cert crossings).
    pub fn metric(&self) -> Option<&str> {
        self.data_str("metric").or_else(|| self.data_str("path"))
    }

    /// How many occurrences this event stands for: the nerve's `count`
    /// (duplicates it folded into the event inside its dedupe window, 1 =
    /// itself), at least 1. A cortex repeat event (see [`REPEAT_FIELD`])
    /// carries only the NEW occurrences in `count`.
    pub fn occurrences(&self) -> u32 {
        self.count.unwrap_or(1).max(1)
    }

    /// `data.repeat.of_seq` when this is a cortex-made repeat event (the
    /// nerve folded more occurrences into an event already decided on).
    pub fn repeat_of(&self) -> Option<u64> {
        self.data
            .get(REPEAT_FIELD)
            .and_then(|r| r.get("of_seq"))
            .and_then(Value::as_u64)
    }

    /// Nerve sequence number (top-level, else `data.seq`).
    pub fn seq(&self) -> Option<u64> {
        self.seq
            .filter(|s| *s > 0)
            .or_else(|| self.data.get("seq").and_then(Value::as_u64))
    }

    /// Task/slot outcome: `data.ok`, `data.status`, else the kind suffix
    /// (`*.done` ok, `*.failed` not ok, `*.started` none).
    pub fn ok(&self) -> Option<bool> {
        if let Some(b) = self.data.get("ok").and_then(Value::as_bool) {
            return Some(b);
        }
        if let Some(s) = self.data_str("status") {
            return Some(matches!(s, "ok" | "success" | "succeeded" | "done"));
        }
        let k = self.kind.to_ascii_lowercase();
        if k.ends_with(".failed") {
            Some(false)
        } else if k.ends_with(".done") || k.ends_with(".finished") {
            Some(true)
        } else {
            None
        }
    }

    /// Unit target state: `data.to` / `data.active_state`, else from the
    /// lifecycle kind (`unit.started` -> active, `unit.stopped` -> inactive,
    /// `unit.failed` -> failed).
    pub fn to_state(&self) -> Option<&str> {
        if let Some(s) = self
            .data_str("to")
            .or_else(|| self.data_str("active_state"))
        {
            return Some(s);
        }
        match self.kind.as_str() {
            "unit.started" => Some("active"),
            "unit.stopped" | "unit.finished" => Some("inactive"),
            "unit.failed" | "unit.start_failed" | "unit.spawn_failed" => Some("failed"),
            _ => None,
        }
    }

    /// Stable identity for dedupe across transports: source, kind, seq when
    /// present, else ts + unit + data.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |s: &str| {
            for b in s.bytes().chain(std::iter::once(0xff)) {
                h ^= b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        };
        eat(&self.source);
        eat(&self.kind);
        match self.seq() {
            Some(n) => eat(&n.to_string()),
            None => {
                eat(&self.ts);
                eat(self.unit.as_deref().unwrap_or(""));
                eat(&self.data.to_string());
            }
        }
        h
    }

    /// Envelope checks (the nerve's own POST /event limits).
    pub fn validate(&self) -> Result<(), String> {
        for (k, v) in [
            ("source", &self.source),
            ("kind", &self.kind),
            ("severity", &self.severity),
        ] {
            if v.is_empty() || v.len() > MAX_FIELD {
                return Err(format!(
                    "`{k}` must be a non-empty string of at most {MAX_FIELD} bytes"
                ));
            }
        }
        if self.data.to_string().len() > MAX_DATA_BYTES {
            return Err(format!("`data` larger than {MAX_DATA_BYTES} bytes"));
        }
        Ok(())
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Parse a push body: one JSON event, a JSON array of events, or NDJSON.
/// Each item is parsed and validated on its own; at most [`MAX_BATCH`]
/// are accepted, every item beyond that is an `Err` (so the sender sees
/// `invalid > 0` instead of a silent cut; T5.1 chaos S5).
pub fn parse_events(body: &str) -> Vec<Result<Event, String>> {
    let t = body.trim();
    if t.is_empty() {
        return vec![];
    }
    let one = |v: Value| -> Result<Event, String> {
        let e: Event = serde_json::from_value(v).map_err(|e| format!("event: {e}"))?;
        e.validate()?;
        Ok(e)
    };
    let over = |i: usize| Err(format!("item {} over the batch limit {MAX_BATCH}", i + 1));
    if t.starts_with('[') {
        return match serde_json::from_str::<Vec<Value>>(t) {
            Ok(vs) => vs
                .into_iter()
                .enumerate()
                .map(|(i, v)| if i < MAX_BATCH { one(v) } else { over(i) })
                .collect(),
            Err(e) => vec![Err(format!("json array: {e}"))],
        };
    }
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return vec![one(v)];
    }
    t.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            if i >= MAX_BATCH {
                return over(i);
            }
            serde_json::from_str::<Value>(l)
                .map_err(|e| format!("ndjson line: {e}"))
                .and_then(one)
        })
        .collect()
}

/// The only remediation the cortex can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    /// `systemctl restart --no-block` of one allowlisted unit, done by the nerve.
    Restart,
}

/// Who chose the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecidedBy {
    Rules,
    LlmAdvisor,
}

/// cortex -> nerve: `POST /actions` (the nerve reads `action`, `unit`,
/// `reason`, `request_id`, `source`, `dry_run`; the rest is for the audit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub v: u32,
    pub request_id: String,
    pub source: String,
    pub ts: String,
    pub incident: String,
    pub action: ActionKind,
    pub unit: String,
    /// `true` = simulate and log only. The nerve also needs its own
    /// `NERVE_EVT_ACTIONS=live` and reflex live mode: an action runs only
    /// when every switch says live.
    pub dry_run: bool,
    /// Rule id (`R3-known-crash`, ...).
    pub rule: String,
    pub decided_by: DecidedBy,
    pub reason: String,
}

/// Nerve verdict on an [`ActionRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus {
    /// Dry-run: logged, nothing run (nerve `result: "dry-run"`).
    Simulated,
    /// The nerve ran it.
    Executed,
    /// Not on the nerve allowlist / never-touch / malformed (HTTP 403/400).
    Refused,
    /// Per-unit or total hourly cap (HTTP 429).
    RateLimited,
    /// The nerve's circuit breaker is open for this unit.
    CircuitOpen,
    /// The nerve ran it and it failed (HTTP 500).
    Failed,
}

impl ActionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionStatus::Simulated => "simulated",
            ActionStatus::Executed => "executed",
            ActionStatus::Refused => "refused",
            ActionStatus::RateLimited => "rate-limited",
            ActionStatus::CircuitOpen => "circuit-open",
            ActionStatus::Failed => "failed",
        }
    }

    /// The nerve's `result` word.
    pub fn from_result(s: &str) -> Option<Self> {
        Some(match s {
            "dry-run" | "simulated" => ActionStatus::Simulated,
            "executed" => ActionStatus::Executed,
            "refused" => ActionStatus::Refused,
            "rate-limited" => ActionStatus::RateLimited,
            "circuit-open" => ActionStatus::CircuitOpen,
            "failed" => ActionStatus::Failed,
            _ => return None,
        })
    }
}

/// nerve -> cortex: reply to `POST /actions`.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionResult {
    pub request_id: String,
    pub status: ActionStatus,
    pub reason: String,
}

impl ActionResult {
    /// Read the nerve's reply body (`{request_id, result, error?, would_run?, why?}`).
    pub fn from_nerve(v: &Value) -> Option<ActionResult> {
        let status = ActionStatus::from_result(v.get("result")?.as_str()?)?;
        let mut reason = String::new();
        for k in ["error", "would_run", "ran", "detail"] {
            if let Some(s) = v.get(k).and_then(Value::as_str) {
                if !reason.is_empty() {
                    reason.push_str("; ");
                }
                reason.push_str(k);
                reason.push_str(": ");
                reason.push_str(s);
            }
        }
        if let Some(w) = v.get("why").and_then(Value::as_array) {
            let w: Vec<&str> = w.iter().filter_map(Value::as_str).collect();
            reason.push_str(&format!("; why: {}", w.join(", ")));
        }
        Some(ActionResult {
            request_id: v
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            status,
            reason: reason.chars().take(400).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds_round_trip_and_nerve_names() {
        for k in EventKind::ALL {
            assert_eq!(EventKind::parse(k.as_str()), k);
        }
        for (s, k) in [
            ("unit.failed", EventKind::UnitCrash),
            ("unit.start_failed", EventKind::UnitCrash),
            ("unit.spawn_failed", EventKind::UnitCrash),
            ("unit.restart_scheduled", EventKind::RestartScheduled),
            ("unit.start_limit", EventKind::StartLimit),
            ("unit.exit", EventKind::ProcessExit),
            ("proc.exit", EventKind::ProcessExit),
            ("unit.started", EventKind::UnitState),
            ("oom.kill", EventKind::Oom),
            ("task.failed", EventKind::TaskFinished),
            ("slot.done", EventKind::SlotFinished),
            ("disk.crossed", EventKind::Threshold),
            ("cert.crossed", EventKind::Threshold),
            ("action.result", EventKind::Remediation),
            ("stream.keepalive", EventKind::Housekeeping),
            ("stream.gap", EventKind::Housekeeping),
            ("escalation.batch", EventKind::Housekeeping),
            ("incident.opened", EventKind::NerveIncident),
            ("incident.resolved", EventKind::NerveIncident),
            ("kernel.error", EventKind::Other),
            ("unit_state_change", EventKind::UnitState),
            ("crash", EventKind::UnitCrash),
        ] {
            assert_eq!(EventKind::parse(s), k, "{s}");
        }
    }

    #[test]
    fn severities() {
        assert_eq!(sev_rank("low"), 0);
        assert_eq!(sev_rank("medium"), 1);
        assert_eq!(sev_rank("high"), 2);
        assert_eq!(sev_rank("error"), 2);
        assert_eq!(sev_rank("critical"), 3);
        assert_eq!(sev_rank("weird"), 1);
    }

    #[test]
    fn parses_a_nerve_evt_line() {
        let l = r#"{"v":1,"seq":42,"ts":"2026-10-21T18:40:00+02:00","t":1792600800,"source":"journal","kind":"unit.failed","severity":"high","unit":"tuner.service","group":"other","key":"unit.failed:tuner.service","msg":"m","data":{"exit_code":"exited"},"count":1}"#;
        let e = parse_events(l).pop().unwrap().unwrap();
        assert_eq!(e.seq(), Some(42));
        assert_eq!(e.unit().as_deref(), Some("tuner.service"));
        assert_eq!(e.kind(), EventKind::UnitCrash);
        assert_eq!(e.to_state(), Some("failed"));
        assert_eq!(e.rank(), 2);
        let k = Event::new("journal", "oom.kill", "critical", json!({}));
        assert_eq!(
            Event {
                unit: Some("-.slice".into()),
                ..k
            }
            .unit(),
            None
        );
    }

    #[test]
    fn unit_normalised() {
        let e = Event::new("nerve", "crash", "error", json!({"unit": "tuner"}));
        assert_eq!(e.unit().as_deref(), Some("tuner.service"));
        let e = Event::new("nerve", "crash", "error", json!({"unit": "run-u7.scope"}));
        assert_eq!(e.unit().as_deref(), Some("run-u7.scope"));
        let e = Event::new(
            "nerve",
            "crash",
            "error",
            json!({"unit": "gaia-hermes-handoff.path"}),
        );
        assert_eq!(e.unit().as_deref(), Some("gaia-hermes-handoff.path"));
        let e = Event::new("nerve", "crash", "error", json!({}));
        assert_eq!(e.unit(), None);
    }

    #[test]
    fn parse_object_array_ndjson() {
        let o = r#"{"source":"nerve","kind":"crash","severity":"error","data":{"seq":1}}"#;
        assert_eq!(parse_events(o).len(), 1);
        let a = format!("[{o},{o}]");
        assert_eq!(parse_events(&a).len(), 2);
        let n = format!("{o}\n\n{o}\nnot json\n");
        let r = parse_events(&n);
        assert_eq!(r.len(), 3);
        assert!(r[2].is_err());
        assert!(parse_events(r#"{"source":"","kind":"x","severity":"info"}"#)[0].is_err());
    }

    #[test]
    fn over_batch_items_are_reported_not_dropped() {
        let arr: Vec<Value> = (0..1500)
            .map(|i| json!({"source": "s", "kind": "unit.failed", "severity": "high", "unit": "tuner.service", "seq": i + 1}))
            .collect();
        let r = parse_events(&Value::Array(arr).to_string());
        assert_eq!(r.len(), 1500);
        assert_eq!(r.iter().filter(|x| x.is_ok()).count(), MAX_BATCH);
        assert!(r[MAX_BATCH].as_ref().unwrap_err().contains("batch limit"));
        let nd: String = (0..1200)
            .map(|i| {
                format!(
                    "{{\"source\":\"s\",\"kind\":\"k\",\"severity\":\"warn\",\"seq\":{}}}\n",
                    i + 1
                )
            })
            .collect();
        let r = parse_events(&nd);
        assert_eq!(r.iter().filter(|x| x.is_err()).count(), 200);
    }

    #[test]
    fn fingerprint_uses_seq() {
        let mut a = Event::new("nerve", "crash", "error", json!({"seq": 7, "x": 1}));
        let mut b = Event::new("nerve", "crash", "error", json!({"seq": 7, "x": 2}));
        a.ts = "t1".into();
        b.ts = "t2".into();
        assert_eq!(a.fingerprint(), b.fingerprint());
        let c = Event::new("nerve", "crash", "error", json!({"x": 1}));
        let d = Event::new("nerve", "crash", "error", json!({"x": 2}));
        assert_ne!(c.fingerprint(), d.fingerprint());
    }

    #[test]
    fn action_wire_shape_matches_nerve_actions_parse() {
        let r = ActionRequest {
            v: CONTRACT_VERSION,
            request_id: "dec-1".into(),
            source: CORTEX_SOURCE.into(),
            ts: "t".into(),
            incident: "inc-1".into(),
            action: ActionKind::Restart,
            unit: "tuner.service".into(),
            dry_run: true,
            rule: "R3-known-crash".into(),
            decided_by: DecidedBy::Rules,
            reason: "crash".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        // The fields evt::actions::parse reads.
        assert_eq!(v["action"], "restart");
        assert_eq!(v["unit"], "tuner.service");
        assert_eq!(v["request_id"], "dec-1");
        assert_eq!(v["source"], "gaia-cortex");
        assert_eq!(v["dry_run"], true);
        let mut bad = v.clone();
        bad["action"] = json!("stop");
        assert!(serde_json::from_value::<ActionRequest>(bad).is_err());
        // Nerve replies (evt::actions::Actions::handle bodies).
        let dry = json!({"request_id":"dec-1","action":"restart","unit":"tuner.service","result":"dry-run",
                         "would_run":"systemctl restart --no-block tuner.service","why":["request dry_run"]});
        let r = ActionResult::from_nerve(&dry).unwrap();
        assert_eq!(r.status, ActionStatus::Simulated);
        assert!(r.reason.contains("would_run"));
        let refused = json!({"request_id":"x","result":"refused","error":"unit is not on the reflex allowlist"});
        assert_eq!(
            ActionResult::from_nerve(&refused).unwrap().status,
            ActionStatus::Refused
        );
        assert_eq!(
            ActionResult::from_nerve(&json!({"result":"rate-limited"}))
                .unwrap()
                .status,
            ActionStatus::RateLimited
        );
        assert!(ActionResult::from_nerve(&json!({"result":"noted"})).is_none());
    }
}

#[cfg(test)]
mod occurrences {
    use super::*;
    use serde_json::json;

    #[test]
    fn count_and_repeat_marker() {
        let l = r#"{"v":1,"seq":12,"source":"journal","kind":"unit.failed","severity":"high","unit":"gaia-hermes-handoff.path","key":"unit.failed:gaia-hermes-handoff.path","data":{"unit_result":"trigger-limit-hit"},"count":2}"#;
        let e = parse_events(l).pop().unwrap().unwrap();
        assert_eq!(e.occurrences(), 2);
        assert_eq!(e.repeat_of(), None);
        let mut r = Event::new(
            "journal",
            "unit.failed",
            "high",
            json!({"repeat": {"of_seq": 12, "total": 5, "new": 3}}),
        );
        r.count = Some(3);
        assert_eq!((r.occurrences(), r.repeat_of()), (3, Some(12)));
        // no count / zero count = itself
        let mut z = Event::new("nerve", "crash", "error", json!({}));
        assert_eq!(z.occurrences(), 1);
        z.count = Some(0);
        assert_eq!(z.occurrences(), 1);
        assert_eq!(NERVE_RING_PATH, "/evt");
        assert_eq!(
            (NERVE_RESTARTS_PER_UNIT_HOUR, NERVE_RESTARTS_TOTAL_HOUR),
            (2, 6)
        );
    }
}
