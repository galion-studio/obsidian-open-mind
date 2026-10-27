// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! systemd journal as an event source: one `journalctl -f -o json` child
//! whose *server-side* matches keep the stream small, read with a blocking
//! pipe read (no polling, no timer).
//!
//! Match groups (journalctl ORs groups separated by `+`; inside a group,
//! different fields AND and repeated fields OR):
//!
//! 1. any unit: failed, failure result (incl. `start-limit-hit` = failed
//!    restart), main process exited non-clean, restart scheduled, OOM kill,
//!    spawn failed;
//! 2. `_PID=1` + tracked units + started / stopped / finished;
//! 3. kernel messages with priority err or worse (OOM killer on hosts with
//!    kmsg; an LXC has none).
//!
//! Measured on the VPS 2026-10-21 18:14 +02:00: the unfiltered journal is
//! ~3,700 lines/h (1,900 cloudflared stdout, 574 ssh, 352 session scopes);
//! group 1 matched 108 lines in 24 h (all sandbox/transient units).

use super::{classify_unit, lifecycle_tracked, NerveEvent, Severity};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::FileTypeExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ID_UNIT_FAILED: &str = "be02cf6855d2428ba40df7e9d022f03d";
pub const ID_RESTART_SCHEDULED: &str = "5eb03494b6584870a536b337290809b3";
pub const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
pub const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";
pub const ID_OOM: &str = "fe6faa94e7774663a0da52717891d8ef";
pub const ID_SPAWN_FAILED: &str = "641257651c1b4ec9a8624d7a40a9e1e7";
pub const ID_STARTED: &str = "39f53479d3a045ac8e11786248231fbf";
pub const ID_STOPPED: &str = "9d1aaa27d60140bd96365438aad20286";
pub const ID_SUCCESS: &str = "7ad2d189f7e94e70a38c781354912448";
/// `Stopping …` (job start of a stop / restart job), slot 310.
pub const ID_STOPPING: &str = "de5b426a63be47a7b6ac3eaac82e2f6f";

pub const FAILURE_IDS: &[&str] = &[
    ID_UNIT_FAILED,
    ID_RESTART_SCHEDULED,
    ID_FAILURE_RESULT,
    ID_PROCESS_EXIT,
    ID_OOM,
    ID_SPAWN_FAILED,
];
pub const LIFECYCLE_IDS: &[&str] = &[ID_STARTED, ID_STOPPED, ID_SUCCESS, ID_STOPPING];

/// journalctl arguments for the tracked unit list. `after` = last cursor
/// (restart without gap) else start at the tail.
pub fn args(units: &[String], after: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = vec!["-f".into(), "-o".into(), "json".into(), "--no-pager".into()];
    match after {
        Some(c) => a.push(format!("--after-cursor={c}")),
        None => a.extend(["-n".into(), "0".into()]),
    }
    a.extend(FAILURE_IDS.iter().map(|i| format!("MESSAGE_ID={i}")));
    if !units.is_empty() {
        a.push("+".into());
        a.push("_PID=1".into());
        a.extend(units.iter().map(|u| format!("UNIT={u}")));
        a.extend(LIFECYCLE_IDS.iter().map(|i| format!("MESSAGE_ID={i}")));
    }
    a.push("+".into());
    a.push("_TRANSPORT=kernel".into());
    a.extend(["0", "1", "2", "3"].iter().map(|p| format!("PRIORITY={p}")));
    a
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// Severity for a unit problem by group: the stack is high, the noise of
/// throwaway units (sandbox runs, transient `systemd-run`, ssh sessions) is low.
fn sev_for(unit: &str, stack: Severity) -> Severity {
    match classify_unit(unit) {
        "transient" | "sandbox" => Severity::Low,
        "other" => Severity::Medium,
        _ => stack,
    }
}

/// One journal JSON entry -> event (None = not interesting).
pub fn parse_entry(v: &Value) -> Option<NerveEvent> {
    parse_entry_with(v, &|_| false)
}

/// Like [`parse_entry`]; `faulted(unit)` = the unit has an open fault
/// incident, so its lifecycle (started / finished) is published even when
/// the unit is not lifecycle-tracked (slot 246: postfix, ssh, ...).
pub fn parse_entry_with(v: &Value, faulted: &dyn Fn(&str) -> bool) -> Option<NerveEvent> {
    let t = s(v, "__REALTIME_TIMESTAMP")
        .and_then(|x| x.parse::<u64>().ok())
        .map(|us| us / 1_000_000);
    let msg: String = s(v, "MESSAGE").unwrap_or("").chars().take(300).collect();
    let unit = s(v, "UNIT")
        .or_else(|| s(v, "USER_UNIT"))
        .unwrap_or("")
        .to_string();
    let id = s(v, "MESSAGE_ID").unwrap_or("");
    let ev = if s(v, "_TRANSPORT") == Some("kernel") {
        let low = msg.to_ascii_lowercase();
        if low.contains("out of memory") || low.contains("oom-kill") || low.contains("oom_reaper") {
            NerveEvent::new("journal", "oom.kill", Severity::Critical, &msg)
                .group("infra")
                .key("oom.kill:kernel")
        } else {
            NerveEvent::new("journal", "kernel.error", Severity::Medium, &msg)
                .group("infra")
                .key(format!(
                    "kernel.error:{}",
                    msg.chars().take(40).collect::<String>()
                ))
        }
    } else if unit.is_empty() {
        return None;
    } else {
        let (kind, sev) = match id {
            ID_OOM => (
                "oom.kill",
                if unit == "-.slice" {
                    Severity::Critical
                } else {
                    sev_for(&unit, Severity::Critical)
                },
            ),
            ID_FAILURE_RESULT => {
                let r = s(v, "UNIT_RESULT").unwrap_or("");
                if r == "start-limit-hit" {
                    ("unit.start_limit", sev_for(&unit, Severity::Critical))
                } else {
                    ("unit.failed", sev_for(&unit, Severity::High))
                }
            }
            ID_UNIT_FAILED => ("unit.start_failed", sev_for(&unit, Severity::High)),
            ID_SPAWN_FAILED => ("unit.spawn_failed", sev_for(&unit, Severity::High)),
            ID_PROCESS_EXIT => ("unit.exit", sev_for(&unit, Severity::High)),
            ID_RESTART_SCHEDULED => ("unit.restart_scheduled", sev_for(&unit, Severity::High)),
            ID_STARTED if lifecycle_tracked(&unit) || faulted(&unit) => {
                ("unit.started", Severity::Medium)
            }
            ID_STOPPED if lifecycle_tracked(&unit) => ("unit.stopped", Severity::Medium),
            ID_SUCCESS if lifecycle_tracked(&unit) || faulted(&unit) => {
                ("unit.finished", Severity::Info)
            }
            _ => return None,
        };
        let mut e = NerveEvent::new("journal", kind, sev, &msg).unit(&unit);
        if kind == "unit.exit" {
            e.key = format!("unit.exit:{unit}:{}", s(v, "EXIT_STATUS").unwrap_or("?"));
        }
        e
    };
    let mut ev = ev.data(json!({
        "message_id": id,
        "unit_result": s(v, "UNIT_RESULT"),
        "exit_code": s(v, "EXIT_CODE"),
        "exit_status": s(v, "EXIT_STATUS"),
        "job_type": s(v, "JOB_TYPE"),
        "job_result": s(v, "JOB_RESULT"),
        "n_restarts": s(v, "N_RESTARTS"),
        "gaia": super::gaia_function(&unit),
    }));
    if let Some(t) = t {
        ev = ev.at(t);
    }
    Some(ev)
}

/// Stop / restart jobs seen in the journal (`Stopping …`, slot 310 / F2 of
/// slot 271): a `unit.exit` / `unit.failed` of the same unit invocation that
/// follows a stop job is a *planned* stop (operator `systemctl restart`,
/// cutover, cortex restart), not a crash. systemd logs `Stopping` before the
/// main process exits (VPS seq 21467446 < 21467449), so no reorder window.
#[derive(Debug, Default)]
pub struct PlannedStops {
    /// unit -> (invocation id, Stopping time, job type, worst exit status seen)
    open: HashMap<String, PlannedRec>,
    pub tagged: u64,
}

#[derive(Debug, Clone)]
struct PlannedRec {
    invocation: Option<String>,
    t: u64,
    job: String,
    exit_status: Option<String>,
}

/// A stop job older than this no longer explains an exit (systemd's default
/// TimeoutStopSec is 90 s; a SIGKILL after it still lands inside).
pub const PLANNED_TTL_S: u64 = 300;

/// Exit statuses a well-behaved service returns on SIGTERM / SIGINT.
fn clean_stop_status(s: Option<&str>) -> bool {
    matches!(s, None | Some("0" | "143" | "130" | "15" | "TERM" | "INT"))
}

impl PlannedStops {
    /// Feed one journal entry and its parsed event (if any); returns the
    /// event to publish. Exit / failure events of an invocation that has an
    /// open stop job become `unit.planned_stop` (`data.planned = true`).
    pub fn apply(&mut self, v: &Value, ev: Option<NerveEvent>) -> Option<NerveEvent> {
        let id = s(v, "MESSAGE_ID").unwrap_or("");
        let from_pid1 = s(v, "_PID").is_none_or(|p| p == "1");
        let unit = s(v, "UNIT").unwrap_or("");
        let inv = s(v, "INVOCATION_ID").map(str::to_string);
        let t = ev.as_ref().map(|e| e.t).unwrap_or_else(|| {
            s(v, "__REALTIME_TIMESTAMP")
                .and_then(|x| x.parse::<u64>().ok())
                .map(|us| us / 1_000_000)
                .unwrap_or_else(crate::util::now_secs)
        });
        if unit.is_empty() || !from_pid1 {
            return ev;
        }
        self.open.retain(|_, r| r.t + PLANNED_TTL_S >= t);
        match id {
            ID_STOPPING => {
                let job = s(v, "JOB_TYPE").unwrap_or("stop").to_string();
                let short: String = inv.as_deref().unwrap_or("").chars().take(8).collect();
                let msg: String = s(v, "MESSAGE").unwrap_or("").chars().take(300).collect();
                // Info: published so the runtime (planned restarts) and the cortex
                // (R3 planned-stop skip) see the stop job; never escalated alone.
                let stopping = NerveEvent::new("journal", "unit.stopping", Severity::Info, msg)
                    .unit(unit)
                    .key(format!("unit.stopping:{unit}:{short}"))
                    .data(json!({
                        "message_id": id,
                        "job_type": job,
                        "invocation_id": inv,
                        "planned": true,
                        "gaia": super::gaia_function(unit),
                    }))
                    .at(t);
                self.open.insert(
                    unit.to_string(),
                    PlannedRec {
                        invocation: inv,
                        t,
                        job,
                        exit_status: None,
                    },
                );
                return ev.or(Some(stopping));
            }
            // a new invocation starts: the stop job is over
            ID_STARTED => {
                self.open.remove(unit);
                return ev;
            }
            _ => {}
        }
        let mut e = ev?;
        if !matches!(e.kind.as_str(), "unit.exit" | "unit.failed") {
            return Some(e);
        }
        let Some(rec) = self.open.get_mut(unit) else {
            return Some(e);
        };
        if let (Some(a), Some(b)) = (&rec.invocation, &inv) {
            if a != b {
                return Some(e);
            }
        }
        if e.kind == "unit.exit" {
            rec.exit_status = s(v, "EXIT_STATUS").map(str::to_string);
        }
        let result = s(v, "UNIT_RESULT").unwrap_or("");
        let clean =
            clean_stop_status(rec.exit_status.as_deref()) && clean_stop_status(s(v, "EXIT_STATUS"));
        // Planned, but a stop that timed out / dumped core is still worth a look.
        let sev = match result {
            "" | "exit-code" | "signal" | "success" if clean => Severity::Info,
            "" | "exit-code" | "signal" | "success" => Severity::Low,
            _ => Severity::Medium,
        };
        let was = std::mem::take(&mut e.kind);
        let short: String = inv.as_deref().unwrap_or("").chars().take(8).collect();
        e.kind = "unit.planned_stop".into();
        e.key = format!("unit.planned_stop:{unit}:{was}:{short}");
        e.severity = sev;
        if let Some(d) = e.data.as_object_mut() {
            d.insert("planned".into(), json!(true));
            d.insert("planned_kind".into(), json!(was));
            d.insert("stop_job".into(), json!(rec.job));
            d.insert("stopping_t".into(), json!(rec.t));
            d.insert("invocation_id".into(), json!(inv));
        }
        self.tagged += 1;
        Some(e)
    }
}

/// What the source thread sends to the runtime.
pub enum Msg {
    Event(NerveEvent),
    /// Source down / restarting (counted, published as nerve.source).
    Source(String, String),
}

/// Running journal source. `restart()` kills the child; the reader thread
/// respawns it right away with the current unit list after the last cursor.
pub struct JournalSource {
    pub units: Arc<Mutex<Vec<String>>>,
    child: Arc<Mutex<Option<Child>>>,
    restart: Arc<AtomicBool>,
}

impl JournalSource {
    pub fn start(
        bin: String,
        units: Vec<String>,
        faulted: Arc<Mutex<BTreeSet<String>>>,
        tx: Sender<super::runtime::Input>,
    ) -> Self {
        let mut planned = PlannedStops::default();
        let mut parse = move |v: &Value| {
            let ev = {
                let g = faulted.lock().unwrap_or_else(|e| e.into_inner());
                parse_entry_with(v, &|u| g.contains(u))
            };
            planned.apply(v, ev)
        };
        let units = Arc::new(Mutex::new(units));
        let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let restart = Arc::new(AtomicBool::new(false));
        let (u2, c2, r2) = (units.clone(), child.clone(), restart.clone());
        let send = move |m: Msg| tx.send(super::runtime::Input::Journal(m)).is_ok();
        let _ = std::thread::Builder::new()
            .name("nerve-evt-journal".into())
            .spawn(move || {
                if let Some(path) = bin.strip_prefix("replay:") {
                    // Test / capture replay: read journal JSON lines from a file or FIFO
                    // (blocking reads, same parser), reopening after each EOF.
                    loop {
                        let Ok(f) = std::fs::File::open(path) else {
                            let _ =
                                send(Msg::Source("journal".into(), format!("replay open {path}")));
                            return;
                        };
                        for line in BufReader::new(f).lines() {
                            let Ok(line) = line else { break };
                            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                                continue;
                            };
                            if let Some(ev) = parse(&v) {
                                if !send(Msg::Event(ev)) {
                                    return;
                                }
                            }
                        }
                        if !std::fs::metadata(path)
                            .map(|m| m.file_type().is_fifo())
                            .unwrap_or(false)
                        {
                            return;
                        }
                    }
                }
                let mut cursor: Option<String> = None;
                let mut backoff = 5u64;
                loop {
                    let list = u2.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let a = args(&list, cursor.as_deref());
                    let spawned = Command::new(&bin)
                        .args(&a)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn();
                    let mut ch = match spawned {
                        Ok(c) => c,
                        Err(e) => {
                            if !send(Msg::Source("journal".into(), format!("spawn {bin}: {e}"))) {
                                return;
                            }
                            std::thread::sleep(Duration::from_secs(backoff));
                            backoff = (backoff * 2).min(300);
                            continue;
                        }
                    };
                    let out = ch.stdout.take();
                    *c2.lock().unwrap_or_else(|e| e.into_inner()) = Some(ch);
                    if let Some(out) = out {
                        for line in BufReader::new(out).lines() {
                            let Ok(line) = line else { break };
                            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                                continue;
                            };
                            if let Some(c) = s(&v, "__CURSOR") {
                                cursor = Some(c.to_string());
                                backoff = 5;
                            }
                            if let Some(ev) = parse(&v) {
                                if !send(Msg::Event(ev)) {
                                    return;
                                }
                            }
                        }
                    }
                    if let Some(mut c) = c2.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    // A requested restart (new unit list) is immediate; an unexpected exit backs off.
                    if r2.swap(false, Ordering::SeqCst) {
                        continue;
                    }
                    if !send(Msg::Source("journal".into(), "journalctl exited".into())) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(300);
                }
            });
        JournalSource {
            units,
            child,
            restart,
        }
    }

    /// Change the unit list (inventory changed) and restart the child after the last cursor.
    pub fn set_units(&self, units: Vec<String>) -> bool {
        let mut g = self.units.lock().unwrap_or_else(|e| e.into_inner());
        if *g == units {
            return false;
        }
        *g = units;
        drop(g);
        if let Some(c) = self
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            self.restart.store(true, Ordering::SeqCst);
            let _ = c.kill();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(j: &str) -> Option<NerveEvent> {
        parse_entry(&serde_json::from_str(j).unwrap())
    }

    #[test]
    fn args_shape() {
        let a = args(&["hermes-rs.service".into()], None);
        let j = a.join(" ");
        assert!(j.starts_with("-f -o json --no-pager -n 0 MESSAGE_ID="));
        assert!(j.contains("+ _PID=1 UNIT=hermes-rs.service MESSAGE_ID=39f5"));
        assert!(j.ends_with("+ _TRANSPORT=kernel PRIORITY=0 PRIORITY=1 PRIORITY=2 PRIORITY=3"));
        let b = args(&[], Some("s=abc"));
        assert!(b.contains(&"--after-cursor=s=abc".to_string()));
        assert!(!b.contains(&"_PID=1".to_string()));
    }

    #[test]
    fn real_vps_shapes() {
        // Shapes copied from the VPS journal (2026-10-21, read-only).
        let r = e(r#"{"__REALTIME_TIMESTAMP":"1761058000000000","MESSAGE_ID":"5eb03494b6584870a536b337290809b3","UNIT":"openmind-queue.service","MESSAGE":"openmind-queue.service: Scheduled restart job, restart counter is at 1.","N_RESTARTS":"1"}"#).unwrap();
        assert_eq!(
            (r.kind.as_str(), r.severity, r.group.as_str()),
            ("unit.restart_scheduled", Severity::High, "gaia")
        );
        assert_eq!(r.t, 1_761_058_000);
        assert_eq!(r.data["gaia"], "POSEIDON");
        let f = e(r#"{"MESSAGE_ID":"d9b373ed55a64feb8242e02dbe79a49c","UNIT":"run-u21491.service","MESSAGE":"run-u21491.service: Failed with result 'exit-code'.","UNIT_RESULT":"exit-code"}"#).unwrap();
        assert_eq!(
            (f.kind.as_str(), f.severity),
            ("unit.failed", Severity::Low)
        );
        let o = e(r#"{"MESSAGE_ID":"fe6faa94e7774663a0da52717891d8ef","UNIT":"-.slice","MESSAGE":"-.slice: A process of this unit has been killed by the OOM killer."}"#).unwrap();
        assert_eq!(
            (o.kind.as_str(), o.severity),
            ("oom.kill", Severity::Critical)
        );
        let sl = e(r#"{"MESSAGE_ID":"d9b373ed55a64feb8242e02dbe79a49c","UNIT":"hermes-rs.service","UNIT_RESULT":"start-limit-hit","MESSAGE":"hermes-rs.service: Failed with result 'start-limit-hit'."}"#).unwrap();
        assert_eq!(
            (sl.kind.as_str(), sl.severity),
            ("unit.start_limit", Severity::Critical)
        );
        let st = e(r#"{"MESSAGE_ID":"39f53479d3a045ac8e11786248231fbf","UNIT":"galion-nerve.service","MESSAGE":"Started galion-nerve.service","JOB_RESULT":"done"}"#).unwrap();
        assert_eq!(
            (st.kind.as_str(), st.severity, st.group.as_str()),
            ("unit.started", Severity::Medium, "nerve")
        );
        // lifecycle of untracked units (ssh sessions, timers) is not an event
        assert!(e(r#"{"MESSAGE_ID":"39f53479d3a045ac8e11786248231fbf","UNIT":"session-71580.scope","MESSAGE":"Started"}"#).is_none());
        assert!(e(r#"{"MESSAGE_ID":"7ad2d189f7e94e70a38c781354912448","UNIT":"sysstat-collect.service","MESSAGE":"x"}"#).is_none());
        let k = e(r#"{"_TRANSPORT":"kernel","PRIORITY":"3","MESSAGE":"Out of memory: Killed process 123 (x)"}"#).unwrap();
        assert_eq!(k.kind, "oom.kill");
        let x1 = e(r#"{"MESSAGE_ID":"98e322203f7a4ed290d09fe03c09fe15","UNIT":"hermes-rs.service","EXIT_STATUS":"1","MESSAGE":"exit"}"#).unwrap();
        let x2 = e(r#"{"MESSAGE_ID":"98e322203f7a4ed290d09fe03c09fe15","UNIT":"hermes-rs.service","EXIT_STATUS":"9","MESSAGE":"exit"}"#).unwrap();
        assert_ne!(x1.key, x2.key);
        assert!(e(r#"{"MESSAGE":"no unit"}"#).is_none());
    }

    #[test]
    fn faulted_untracked_lifecycle_is_published() {
        let started = r#"{"MESSAGE_ID":"39f53479d3a045ac8e11786248231fbf","UNIT":"postfix.service","MESSAGE":"Started postfix.service","_PID":"1"}"#;
        let v: Value = serde_json::from_str(started).unwrap();
        assert!(
            parse_entry(&v).is_none(),
            "untracked: not an event by default"
        );
        let st = parse_entry_with(&v, &|u| u == "postfix.service").unwrap();
        assert_eq!(
            (st.kind.as_str(), st.group.as_str()),
            ("unit.started", "infra")
        );
        let fin: Value = serde_json::from_str(r#"{"MESSAGE_ID":"7ad2d189f7e94e70a38c781354912448","UNIT":"logrotate.service","MESSAGE":"x"}"#).unwrap();
        assert_eq!(
            parse_entry_with(&fin, &|u| u == "logrotate.service")
                .unwrap()
                .kind,
            "unit.finished"
        );
        // stopped stays tracked-only
        let stop: Value = serde_json::from_str(r#"{"MESSAGE_ID":"9d1aaa27d60140bd96365438aad20286","UNIT":"postfix.service","MESSAGE":"x"}"#).unwrap();
        assert!(parse_entry_with(&stop, &|_| true).is_none());
    }

    fn replay(path: &str) -> (Vec<NerveEvent>, PlannedStops) {
        let mut p = PlannedStops::default();
        let txt = std::fs::read_to_string(path).unwrap();
        let mut out = vec![];
        for l in txt.lines() {
            let v: Value = serde_json::from_str(l).unwrap();
            if let Some(ev) = p.apply(&v, parse_entry(&v)) {
                out.push(ev);
            }
        }
        (out, p)
    }

    fn count(evs: &[NerveEvent], unit: &str, kind: &str, sev: Option<Severity>) -> usize {
        evs.iter()
            .filter(|e| e.unit.as_deref() == Some(unit) && e.kind == kind)
            .filter(|e| sev.is_none_or(|s| e.severity == s))
            .count()
    }

    /// Slot 310: the real console restarts of 2026-10-27 17:25-17:28 +01:00
    /// (operator, cortex dec-874, cutover) are planned stops, not failures.
    #[test]
    fn vps_console_restarts_are_planned() {
        let (evs, p) = replay(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/planned-stop/vps-console-restarts-20261027.jsonl"
        ));
        let u = "console.service";
        assert_eq!(count(&evs, u, "unit.failed", None), 0);
        assert_eq!(count(&evs, u, "unit.exit", None), 0);
        assert_eq!(count(&evs, u, "unit.stopping", Some(Severity::Info)), 3);
        assert_eq!(count(&evs, u, "unit.planned_stop", Some(Severity::Info)), 6);
        assert_eq!(count(&evs, u, "unit.started", None), 3);
        assert_eq!(p.tagged, 6);
        let f = evs
            .iter()
            .find(|e| e.kind == "unit.planned_stop" && e.data["planned_kind"] == "unit.failed")
            .unwrap();
        assert_eq!(f.data["planned"], true);
        assert_eq!(f.data["stop_job"], "stop");
        assert_eq!(f.data["unit_result"], "exit-code");
        assert_eq!(f.data["invocation_id"], "e8a553b23a09422d86e214e14b28c551");
        assert!(f
            .key
            .starts_with("unit.planned_stop:console.service:unit.failed:e8a553b2"));
        // nothing planned is Medium or worse: no escalation, no event-triggered cycle
        assert!(evs
            .iter()
            .filter(|e| e.kind.starts_with("unit.planned") || e.kind == "unit.stopping")
            .all(|e| e.severity == Severity::Info));
    }

    /// Crash controls from the same VPS (7 days): crash loops without a stop
    /// job keep `unit.failed` high; only exits inside a stop job are tagged.
    #[test]
    fn vps_7d_crash_loops_stay_failures() {
        let (evs, _) = replay(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/planned-stop/vps-lifecycle-7d-20261027.jsonl"
        ));
        let h = "hermes-rs.service";
        assert_eq!(count(&evs, h, "unit.failed", Some(Severity::High)), 16);
        assert_eq!(count(&evs, h, "unit.exit", Some(Severity::High)), 16);
        assert_eq!(count(&evs, h, "unit.planned_stop", None), 0);
        assert_eq!(count(&evs, h, "unit.stopping", None), 5);
        let c = "gaia-cortex.service";
        assert_eq!(count(&evs, c, "unit.failed", Some(Severity::High)), 4);
        assert_eq!(count(&evs, c, "unit.planned_stop", None), 0);
        let s = "cortex-sbx-crash-20261021-183907.service";
        assert_eq!(count(&evs, s, "unit.failed", Some(Severity::Low)), 4);
    }

    #[test]
    fn planned_stop_edge_cases() {
        let j = |x: &str| -> Value { serde_json::from_str(x).unwrap() };
        let stopping = j(
            r#"{"MESSAGE_ID":"de5b426a63be47a7b6ac3eaac82e2f6f","UNIT":"hermes-rs.service","_PID":"1","JOB_TYPE":"stop","INVOCATION_ID":"aaa","__REALTIME_TIMESTAMP":"1000000000"}"#,
        );
        let exit = |inv: &str, st: &str, t: u64| {
            j(&format!(
                r#"{{"MESSAGE_ID":"98e322203f7a4ed290d09fe03c09fe15","UNIT":"hermes-rs.service","_PID":"1","EXIT_STATUS":"{st}","INVOCATION_ID":"{inv}","__REALTIME_TIMESTAMP":"{}"}}"#,
                t * 1_000_000
            ))
        };
        let failed = |inv: &str, r: &str, t: u64| {
            j(&format!(
                r#"{{"MESSAGE_ID":"d9b373ed55a64feb8242e02dbe79a49c","UNIT":"hermes-rs.service","_PID":"1","UNIT_RESULT":"{r}","INVOCATION_ID":"{inv}","__REALTIME_TIMESTAMP":"{}"}}"#,
                t * 1_000_000
            ))
        };
        let run = |p: &mut PlannedStops, v: &Value| p.apply(v, parse_entry(v));
        let mut p = PlannedStops::default();
        // other invocation: not explained by the stop job
        assert!(run(&mut p, &stopping).unwrap().kind == "unit.stopping");
        assert_eq!(
            run(&mut p, &exit("bbb", "143", 1001)).unwrap().kind,
            "unit.exit"
        );
        // same invocation, unclean exit during stop -> planned but Low
        let e = run(&mut p, &exit("aaa", "1", 1001)).unwrap();
        assert_eq!(
            (e.kind.as_str(), e.severity),
            ("unit.planned_stop", Severity::Low)
        );
        let f = run(&mut p, &failed("aaa", "exit-code", 1001)).unwrap();
        assert_eq!(
            (f.kind.as_str(), f.severity),
            ("unit.planned_stop", Severity::Low)
        );
        // stop that timed out: planned, Medium (worth a look)
        let mut p = PlannedStops::default();
        run(&mut p, &stopping);
        let f = run(&mut p, &failed("aaa", "timeout", 1090)).unwrap();
        assert_eq!(
            (f.kind.as_str(), f.severity),
            ("unit.planned_stop", Severity::Medium)
        );
        // expired stop job: a later failure is a failure again
        let mut p = PlannedStops::default();
        run(&mut p, &stopping);
        let f = run(
            &mut p,
            &failed("aaa", "exit-code", 1000 + PLANNED_TTL_S + 1),
        )
        .unwrap();
        assert_eq!(
            (f.kind.as_str(), f.severity),
            ("unit.failed", Severity::High)
        );
        // Started of a new invocation closes the stop job
        let mut p = PlannedStops::default();
        run(&mut p, &stopping);
        run(
            &mut p,
            &j(
                r#"{"MESSAGE_ID":"39f53479d3a045ac8e11786248231fbf","UNIT":"hermes-rs.service","_PID":"1","INVOCATION_ID":"ccc","__REALTIME_TIMESTAMP":"1001000000"}"#,
            ),
        );
        assert_eq!(
            run(&mut p, &failed("", "exit-code", 1002)).unwrap().kind,
            "unit.failed"
        );
        // a Stopping line not from pid 1 (a service printing the text) is ignored
        let mut p = PlannedStops::default();
        let fake = j(
            r#"{"MESSAGE_ID":"de5b426a63be47a7b6ac3eaac82e2f6f","UNIT":"hermes-rs.service","_PID":"4242","INVOCATION_ID":"aaa","__REALTIME_TIMESTAMP":"1000000000"}"#,
        );
        assert!(run(&mut p, &fake).is_none());
        assert_eq!(
            run(&mut p, &failed("aaa", "exit-code", 1001)).unwrap().kind,
            "unit.failed"
        );
        // the journal match subscribes to Stopping for tracked units
        assert!(
            args(&["console.service".into()], None).contains(&format!("MESSAGE_ID={ID_STOPPING}"))
        );
    }
}
