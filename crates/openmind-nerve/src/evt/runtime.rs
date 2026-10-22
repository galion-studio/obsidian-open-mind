// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The event loop. One thread blocks in `recv_timeout` until a source
//! delivers something or the next *deadline* is due. Deadlines exist only
//! while there is work: a pending batch flush (debounce), a deferred engine
//! cycle after an event, and the slow liveness heartbeat. With nothing
//! happening the thread wakes once per heartbeat (default 15 min = 4/h).
//!
//! The full engine cycle (HTTP health probes, cortex sensors, score) runs at
//! start, on the heartbeat, and debounced after stack events (at most one per
//! `cycle_min_gap_s`), instead of every 30 s.

use super::coalesce::Coalescer;
use super::escalate::{Escalator, Outcome};
use super::inventory::Snapshot;
use super::journal::{JournalSource, Msg};
use super::pidwatch::PidWatch;
use super::recover::UnitFaults;
use super::threshold::{check_all, Crossings};
use super::watch::WatchOut;
use super::{EvtConfig, Hub, NerveEvent, Severity};
use crate::server::EventLog;
use crate::util::now_secs;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Input {
    Journal(Msg),
    Watch(WatchOut),
    Pid(NerveEvent),
    Http(NerveEvent),
    /// Tests: stop the loop.
    Stop,
}

/// The full probe cycle, abstracted so tests can count it.
pub trait CycleRunner {
    /// Run one cycle; return incident open/resolve events.
    fn run_cycle(&mut self) -> Vec<NerveEvent>;
    fn reflex_dry(&self) -> bool;
}

/// No engine (tests / pure event relay).
pub struct NoCycle(pub u64);
impl CycleRunner for NoCycle {
    fn run_cycle(&mut self) -> Vec<NerveEvent> {
        self.0 += 1;
        vec![]
    }
    fn reflex_dry(&self) -> bool {
        true
    }
}

impl CycleRunner for crate::engine::Engine {
    fn run_cycle(&mut self) -> Vec<NerveEvent> {
        use crate::incident::Sev;
        // id (= cause) -> (cause, opened, components) of the incidents open before
        let before: std::collections::BTreeMap<String, (String, u64, Vec<String>)> = self
            .incidents
            .open
            .iter()
            .map(|(id, i)| {
                (
                    id.clone(),
                    (
                        i.cause.clone(),
                        i.opened,
                        i.components.iter().cloned().collect(),
                    ),
                )
            })
            .collect();
        self.cycle();
        let mut out = Vec::new();
        for (id, i) in &self.incidents.open {
            if !before.contains_key(id) {
                let sev = match i.sev {
                    Sev::Critical => Severity::Critical,
                    Sev::Error => Severity::High,
                    Sev::Warn => Severity::Medium,
                    Sev::Info => Severity::Info,
                };
                let comps: Vec<&String> = i.components.iter().collect();
                let first = comps.first().map(|s| s.as_str()).unwrap_or("");
                out.push(
                    NerveEvent::new(
                        "engine",
                        "incident.opened",
                        sev,
                        format!(
                            "{}: {}",
                            i.title,
                            i.why.chars().take(200).collect::<String>()
                        ),
                    )
                    .group(super::classify_unit(&format!("{first}.service")))
                    .key(format!("incident.opened:{id}:{}", i.opened))
                    .data(json!({"id": id, "cause": i.cause, "components": comps, "next": i.next, "opened": i.opened})),
                );
            }
        }
        let now_open: BTreeSet<&String> = self.incidents.open.keys().collect();
        for (id, (cause, opened, comps)) in before.iter().filter(|(b, _)| !now_open.contains(b)) {
            out.push(
                NerveEvent::new(
                    "engine",
                    "incident.resolved",
                    Severity::Info,
                    format!("incident {id} resolved"),
                )
                // keys carry the open time: a cause that opens again inside the hub
                // dedupe window is a new incident, not a duplicate (slot 246)
                .key(format!("incident.resolved:{id}:{opened}"))
                .data(json!({"id": id, "cause": cause, "components": comps, "opened": opened, "resolved_by": "engine"})),
            );
        }
        out
    }
    fn reflex_dry(&self) -> bool {
        !self.reflexes_enabled || self.reflex.dry_run()
    }
}

type InvFn = Box<dyn FnMut() -> Snapshot + Send>;

pub struct Runtime {
    pub cfg: EvtConfig,
    pub hub: Arc<Hub>,
    pub tx: Sender<Input>,
    rx: Receiver<Input>,
    coal: Coalescer,
    esc: Box<dyn Escalator>,
    cross: Crossings,
    journal: Option<JournalSource>,
    pids: PidWatch,
    inventory_fn: InvFn,
    /// Unit fault incidents and their recoveries (slot 246).
    units: UnitFaults,
    /// Lifecycle-tracked units from the last inventory (journal list = these + faulted units).
    lifecycle: Vec<String>,
    fingerprint: Option<BTreeSet<String>>,
    next_heartbeat: u64,
    pending_cycle: Option<u64>,
    pending_inventory: bool,
    last_cycle: u64,
    warm: bool,
    pub evlog: Option<Arc<Mutex<EventLog>>>,
    /// Test clock offset hook: when set, `now()` returns this.
    pub clock: Option<Arc<Mutex<u64>>>,
}

impl Runtime {
    pub fn new(cfg: EvtConfig, hub: Arc<Hub>, esc: Box<dyn Escalator>) -> Self {
        let (tx, rx) = channel();
        *hub.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx.clone());
        hub.lock().config = json!({
            "heartbeat_s": cfg.heartbeat_s, "debounce_s": cfg.debounce_s, "max_wait_s": cfg.max_wait_s,
            "dedupe_s": cfg.dedupe_s, "escalate_min": cfg.escalate_min, "escalate": cfg.escalate.to_json(),
            "cycle_debounce_s": cfg.cycle_debounce_s, "cycle_min_gap_s": cfg.cycle_min_gap_s,
            "journal": cfg.journal, "pidfd": cfg.pidfd, "watch": cfg.watch, "certs": cfg.certs,
            "disk": cfg.disk_paths, "actions_live": cfg.actions_live, "recover_hold_s": cfg.recover_hold_s,
        });
        let coal = Coalescer::new(cfg.escalate_min, cfg.debounce_s, cfg.max_wait_s);
        let units = UnitFaults::new(cfg.recover_hold_s);
        hub.lock().units = units.to_json();
        Runtime {
            coal,
            units,
            lifecycle: vec![],
            esc,
            cross: Crossings::default(),
            journal: None,
            pids: PidWatch::default(),
            inventory_fn: Box::new(super::inventory::discover),
            fingerprint: None,
            next_heartbeat: 0,
            pending_cycle: None,
            pending_inventory: false,
            last_cycle: 0,
            warm: false,
            evlog: None,
            clock: None,
            cfg,
            hub,
            tx,
            rx,
        }
    }

    pub fn with_inventory(mut self, f: InvFn) -> Self {
        self.inventory_fn = f;
        self
    }

    fn now(&self) -> u64 {
        match &self.clock {
            Some(c) => *c.lock().unwrap_or_else(|e| e.into_inner()),
            None => now_secs(),
        }
    }

    /// Journal lifecycle list: tracked units + units with an open fault incident.
    fn journal_units(&self) -> Vec<String> {
        let mut v: BTreeSet<String> = self.lifecycle.iter().cloned().collect();
        v.extend(self.units.open.keys().cloned());
        v.into_iter().collect()
    }

    /// Restart the journal child (after its last cursor) when the list changed.
    fn sync_journal(&self) {
        if let Some(j) = &self.journal {
            j.set_units(self.journal_units());
        }
    }

    fn publish_units(&self) {
        let v = self.units.to_json();
        self.hub.lock().units = v;
    }

    /// Start the kernel-backed sources (journal child, inotify, pidfd).
    pub fn start_sources(&mut self) {
        let snap = (self.inventory_fn)();
        if self.cfg.journal {
            self.lifecycle = snap.lifecycle_units(&self.cfg.extra_units);
            self.journal = Some(JournalSource::start(
                self.cfg.journal_bin.clone(),
                self.journal_units(),
                self.units.tracked.clone(),
                self.tx.clone(),
            ));
        }
        match super::watch::start(
            self.cfg.watch.clone(),
            self.cfg.certs.clone(),
            self.tx.clone(),
        ) {
            Ok(n) => eprintln!("<6>openmind-nerve evt: inotify watches {n}"),
            Err(e) => {
                eprintln!("<4>openmind-nerve evt: inotify: {e}");
                self.hub.lock().stats.source_error("inotify");
            }
        }
        self.apply_inventory(snap, false);
    }

    fn apply_inventory(&mut self, snap: Snapshot, publish_diff: bool) {
        let fp = snap.fingerprint();
        if let Some(old) = &self.fingerprint {
            if publish_diff && *old != fp {
                let added: Vec<&String> = fp.difference(old).collect();
                let removed: Vec<&String> = old.difference(&fp).collect();
                let ev = NerveEvent::new(
                    "inventory",
                    "inventory.changed",
                    Severity::Low,
                    format!("+{} -{}", added.len(), removed.len()),
                )
                .group("nerve")
                .key(format!("inventory.changed:{}", fp.len()))
                .data(json!({"added": added, "removed": removed}));
                self.accept(ev);
            }
        }
        self.fingerprint = Some(fp);
        self.lifecycle = snap.lifecycle_units(&self.cfg.extra_units);
        self.sync_journal();
        // inventory rows confirm (active / inactive) or cancel (failed) unit recoveries
        let now = self.now();
        let evs = self
            .units
            .inventory(&snap.units, snap.errors.is_empty(), now);
        for e in evs {
            self.accept(e);
        }
        self.publish_units();
        if self.cfg.pidfd {
            let mut t = snap.pidfd_targets(std::process::id());
            for p in &self.cfg.extra_pids {
                let comm = std::fs::read_to_string(format!("/proc/{p}/comm"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|_| format!("pid{p}"));
                t.push((*p, comm, "user".into()));
            }
            self.pids.sync(&t, &self.tx);
        }
        let mut inv = snap.to_json(&self.cfg.port_groups);
        inv["pidfd_watching"] = json!(self.pids.count());
        inv["journal_units"] = json!(self.journal.as_ref().map(|j| j
            .units
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()));
        self.hub.lock().inventory = inv;
    }

    /// Publish + side effects of one event (escalation offer, deferred cycle).
    pub fn accept(&mut self, ev: NerveEvent) {
        let now = self.now();
        // Unit faults / recoveries are seen before the hub dedupe: a repeated
        // failure inside the dedupe window still cancels an armed recovery.
        let touches_units = ev.unit.is_some() && ev.source != super::recover::SOURCE;
        let open_before = self.units.open.len();
        let follow = self.units.observe(&ev, now);
        self.accept_one(ev, now);
        if !follow.is_empty() || self.units.open.len() != open_before {
            self.sync_journal();
        }
        for e in follow {
            self.accept_one(e, now);
        }
        if touches_units {
            self.publish_units();
        }
    }

    fn accept_one(&mut self, ev: NerveEvent, now: u64) {
        let Some(ev) = self.hub.publish(ev) else {
            return;
        };
        if ev.severity >= Severity::Medium {
            if let Some(l) = &self.evlog {
                if let Ok(rec) = crate::Event::new(
                    "nerve-evt",
                    ev.kind.clone(),
                    ev.severity.as_str(),
                    &serde_json::to_string(&ev).unwrap_or_default(),
                ) {
                    let mut g = l.lock().unwrap_or_else(|e| e.into_inner());
                    if g.writer.append(&rec).is_ok() {
                        g.appended += 1;
                    }
                }
            }
        }
        // unit incident events are derived from journal events that were offered already
        let loops_back = ev.kind.starts_with("escalation.")
            || ev.kind.starts_with("stream.")
            || ev.source == super::recover::SOURCE;
        if self.warm && !loops_back {
            self.coal.offer(&ev, now);
        }
        let stackish = !matches!(ev.group.as_str(), "transient" | "sandbox" | "other");
        if stackish
            && ev.severity >= Severity::Medium
            && (ev.kind.starts_with("unit.")
                || ev.kind.starts_with("proc.")
                || ev.kind.starts_with("oom."))
        {
            let at =
                (now + self.cfg.cycle_debounce_s).max(self.last_cycle + self.cfg.cycle_min_gap_s);
            self.pending_cycle = Some(self.pending_cycle.map_or(at, |p| p.min(at).max(now)));
            self.pending_inventory = true;
        }
    }

    fn deadline(&self) -> u64 {
        let mut d = self.next_heartbeat;
        if let Some(c) = self.coal.deadline() {
            d = d.min(c);
        }
        if let Some(c) = self.pending_cycle {
            d = d.min(c);
        }
        if let Some(c) = self.units.deadline() {
            d = d.min(c);
        }
        d
    }

    fn run_cycle(&mut self, eng: &mut dyn CycleRunner) {
        let evs = eng.run_cycle();
        let now = self.now();
        self.last_cycle = now;
        self.pending_cycle = None;
        self.hub.lock().stats.engine_cycle(now);
        self.hub
            .reflex_dry
            .store(eng.reflex_dry(), std::sync::atomic::Ordering::SeqCst);
        for e in evs {
            self.accept(e);
        }
    }

    fn heartbeat(&mut self, eng: &mut dyn CycleRunner) {
        let now = self.now();
        self.next_heartbeat = now + self.cfg.heartbeat_s;
        let ths = check_all(
            &mut self.cross,
            &self.cfg.disk_paths,
            &self.cfg.disk_pct,
            &self.cfg.certs,
            &self.cfg.cert_days,
            now,
        );
        for e in ths {
            self.accept(e);
        }
        self.run_cycle(eng);
        let snap = (self.inventory_fn)();
        self.apply_inventory(snap, true);
        self.pending_inventory = false;
        let w = self.hub.lock().stats.wakes_total();
        self.accept(
            NerveEvent::new(
                "nerve",
                "nerve.heartbeat",
                Severity::Info,
                format!("alive; wakes {w}"),
            )
            .key(format!("nerve.heartbeat:{now}")),
        );
    }

    fn escalate_due(&mut self) {
        let now = self.now();
        while let Some(b) = self.coal.flush_due(now) {
            let out = self.esc.escalate(&b, now);
            {
                let mut g = self.hub.lock();
                g.stats.batches += 1;
                match &out {
                    Outcome::Refused(_) | Outcome::Failed(_) => g.stats.escalation_refused += 1,
                    Outcome::Dropped => {}
                    _ => g.stats.escalated(now),
                }
                if let Outcome::Called { usd, .. } = &out {
                    g.stats.llm_calls += 1;
                    g.stats.llm_usd += usd;
                }
                let mut rec = b.to_json();
                rec["outcome"] = json!(out.label());
                rec["escalator"] = json!(self.esc.name());
                match &out {
                    Outcome::Refused(r) | Outcome::Failed(r) => rec["detail"] = json!(r),
                    Outcome::Called { advice, usd } => {
                        rec["advice"] = json!(advice);
                        rec["usd"] = json!(usd);
                    }
                    _ => {}
                }
                g.escalations.push_back(rec);
                while g.escalations.len() > 100 {
                    g.escalations.pop_front();
                }
            }
            if matches!(out, Outcome::Delegated | Outcome::Called { .. }) {
                let mut e = NerveEvent::new(
                    "escalation",
                    "escalation.batch",
                    b.max_severity,
                    b.summary(),
                )
                .group("nerve")
                .key(format!("escalation.batch:{}", b.id))
                .at(now);
                e.data = json!({"batch": b.to_json(), "outcome": out.label(), "events": b.events});
                self.accept(e);
            }
        }
    }

    fn handle(&mut self, inp: Input) -> Option<&'static str> {
        match inp {
            Input::Journal(Msg::Event(e)) => {
                self.accept(e);
                Some("journal")
            }
            Input::Journal(Msg::Source(src, why)) => {
                self.hub.lock().stats.source_error(&src);
                self.accept(
                    NerveEvent::new(
                        "nerve",
                        "nerve.source_down",
                        Severity::High,
                        format!("{src}: {why}"),
                    )
                    .group("nerve")
                    .key(format!("nerve.source_down:{src}")),
                );
                Some("journal")
            }
            Input::Watch(WatchOut::Events(v)) => {
                for e in v {
                    self.accept(e);
                }
                Some("inotify")
            }
            Input::Watch(WatchOut::CertChanged) => {
                let now = self.now();
                for e in check_all(
                    &mut self.cross,
                    &[],
                    &[],
                    &self.cfg.certs,
                    &self.cfg.cert_days,
                    now,
                ) {
                    self.accept(e);
                }
                Some("inotify")
            }
            Input::Watch(WatchOut::Error(e)) => {
                self.hub.lock().stats.source_error("inotify");
                eprintln!("<4>openmind-nerve evt: {e}");
                Some("inotify")
            }
            Input::Pid(e) => {
                self.accept(e);
                Some("pidfd")
            }
            Input::Http(e) => {
                self.accept(e);
                Some("http")
            }
            Input::Stop => None,
        }
    }

    /// Run until `Input::Stop` (tests) or forever.
    pub fn run(&mut self, eng: &mut dyn CycleRunner) {
        // start: one cycle (warm-up; its incidents do not escalate), thresholds, heartbeat timer
        let now = self.now();
        self.next_heartbeat = now + self.cfg.heartbeat_s;
        let ths = check_all(
            &mut self.cross,
            &self.cfg.disk_paths,
            &self.cfg.disk_pct,
            &self.cfg.certs,
            &self.cfg.cert_days,
            now,
        );
        for e in ths {
            self.accept(e);
        }
        self.run_cycle(eng);
        self.warm = true;
        self.accept(
            NerveEvent::new("nerve", "nerve.started", Severity::Info, "event mode up")
                .group("nerve")
                .key(format!("nerve.started:{now}")),
        );
        loop {
            let now = self.now();
            let wait = self.deadline().saturating_sub(now);
            let why = match self.rx.recv_timeout(Duration::from_secs(wait)) {
                Ok(inp) => {
                    let Some(w) = self.handle(inp) else { return };
                    // drain whatever queued meanwhile: part of the same wake
                    while let Ok(more) = self.rx.try_recv() {
                        if self.handle(more).is_none() {
                            return;
                        }
                    }
                    w
                }
                Err(RecvTimeoutError::Timeout) => "timer",
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let now = self.now();
            let mut label = why;
            if now >= self.next_heartbeat {
                label = if why == "timer" { "heartbeat" } else { why };
                self.heartbeat(eng);
            }
            if self.pending_cycle.is_some_and(|c| c <= now) {
                if label == "timer" {
                    label = "deferred-cycle";
                }
                self.run_cycle(eng);
                if std::mem::take(&mut self.pending_inventory) {
                    let snap = (self.inventory_fn)();
                    self.apply_inventory(snap, true);
                }
            }
            let rec = self.units.due(now);
            if !rec.is_empty() {
                if label == "timer" {
                    label = "recovery";
                }
                for e in rec {
                    self.accept(e);
                }
                self.sync_journal();
                self.publish_units();
            }
            if self.coal.deadline().is_some_and(|d| d <= now) && label == "timer" {
                label = "debounce";
            }
            self.escalate_due();
            self.hub.lock().stats.wake(label, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::escalate::Delegate;
    use super::*;

    fn cfg() -> EvtConfig {
        let mut c = EvtConfig::from_env(&|_| None, std::env::temp_dir()).unwrap();
        c.journal = false;
        c.pidfd = false;
        c.disk_paths = vec![];
        c.debounce_s = 1;
        c.max_wait_s = 3;
        c.cycle_debounce_s = 1;
        c.cycle_min_gap_s = 1;
        c
    }

    fn empty() -> Snapshot {
        Snapshot {
            units: vec![],
            ports: vec![],
            procs: vec![],
            docker: None,
            errors: vec![],
        }
    }

    #[test]
    fn idle_is_quiet_and_burst_escalates_once() {
        let hub = Arc::new(Hub::new(600));
        let mut rt =
            Runtime::new(cfg(), hub.clone(), Box::new(Delegate)).with_inventory(Box::new(empty));
        let tx = rt.tx.clone();
        let h = std::thread::spawn(move || {
            let mut eng = NoCycle(0);
            rt.run(&mut eng);
            eng.0
        });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(hub.lock().stats.wakes_total(), 0, "no wake without events");
        // burst: 30 events, 3 distinct keys, high -> one batch
        for i in 0..30 {
            let u = [
                "hermes-rs.service",
                "openmind-memory.service",
                "galion-one.service",
            ][i % 3];
            tx.send(Input::Journal(Msg::Event(
                NerveEvent::new("journal", "unit.exit", Severity::High, "x").unit(u),
            )))
            .unwrap();
        }
        // a low event never escalates
        tx.send(Input::Journal(Msg::Event(
            NerveEvent::new("journal", "unit.failed", Severity::Low, "x").unit("run-u1.service"),
        )))
        .unwrap();
        std::thread::sleep(Duration::from_millis(2600));
        let st = hub.stats_json(now_secs());
        tx.send(Input::Stop).unwrap();
        let cycles = h.join().unwrap();
        assert_eq!(st["batches"], 1, "{st}");
        assert_eq!(st["escalations_total"], 1);
        assert_eq!(st["deduped"], 27);
        // startup cycle + one deferred cycle for the burst
        assert_eq!(cycles, 2);
        let evs = hub.since(0, 100);
        let b: Vec<_> = evs
            .iter()
            .filter(|e| e.kind == "escalation.batch")
            .collect();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].data["events"].as_array().unwrap().len(), 3);
        // wakes: event delivery (1-2, drained) + timers (deferred cycle, debounce) — far below 31
        let w = st["wakes_total"].as_u64().unwrap();
        assert!((2..=8).contains(&w), "wakes {w}: {st}");
    }
}
