// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The Hermes coordinator: task intake from the sector plans, dispatch
//! through the governor, the HADES gate on gated work, the request-limit
//! retry loop, and state rebuilt from the ledger.
//!
//! Task states: `queued -> running -> done`, with `running -> retry-wait ->
//! running` on a stop, `failed` when the retry budget is spent, `held` /
//! `refused` from HADES, and `blocked` when a dependency can never finish in
//! this run. A task left `running` by a crashed Hermes is treated as a stall
//! on the next run and retried.

use crate::action::{classify, ActionClass};
use crate::gate::{Gate, GateDecision};
use crate::governor::Governor;
use crate::ledger::{Event, Ledger, NewEvent};
use crate::plan::{Plan, Task};
use crate::provider::{Brief, Provider, RunOutcome};
use crate::registry::Registry;
use crate::retry::{Next, RetryPolicy, StopKind};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Session id of operator intake tasks.
pub const INTAKE_SESSION: &str = "H";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    /// Not yet seen in the ledger.
    New,
    Queued,
    Held,
    Running,
    RetryWait,
    Done,
    Failed,
    Refused,
    Blocked,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::New => "new",
            TaskState::Queued => "queued",
            TaskState::Held => "held",
            TaskState::Running => "running",
            TaskState::RetryWait => "retry-wait",
            TaskState::Done => "done",
            TaskState::Failed => "failed",
            TaskState::Refused => "refused",
            TaskState::Blocked => "blocked",
        }
    }

    /// Will never reach `done` without outside action.
    pub fn dead(self) -> bool {
        matches!(
            self,
            TaskState::Failed | TaskState::Refused | TaskState::Held | TaskState::Blocked
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskRt {
    pub task: Task,
    pub state: TaskState,
    pub attempts: u32,
    pub stops: u32,
    pub approved: bool,
    pub last: String,
    #[serde(skip)]
    due: Option<Instant>,
}

pub struct Hermes {
    pub reg: Registry,
    pub plans: Vec<Plan>,
    pub tasks: BTreeMap<String, TaskRt>,
    /// Task keys in dispatch order: sector order, then plan order.
    pub order: Vec<String>,
}

/// Short unique id (clock, counter, pid; not cryptographic).
pub fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = t ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (u64::from(std::process::id()) << 32);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{prefix}{:012x}", x & 0xffff_ffff_ffff)
}

#[derive(Clone, Debug, Serialize)]
pub struct DispatchRow {
    pub task: String,
    pub sector: String,
    pub session: String,
    pub title: String,
    pub class: String,
    pub slots: usize,
    /// dispatch | wait-cap | wait-deps | held | refused
    pub decision: String,
    pub agent: Option<String>,
    pub model: String,
    pub gate: GateDecision,
    pub deps_pending: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DispatchPlan {
    pub cap: usize,
    pub reserved: usize,
    pub worker_slots: usize,
    pub slots_used: usize,
    pub scope: String,
    pub rows: Vec<DispatchRow>,
    pub per_sector: BTreeMap<String, (usize, usize)>,
}

impl DispatchPlan {
    pub fn count(&self, decision: &str) -> usize {
        self.rows.iter().filter(|r| r.decision == decision).count()
    }
}

#[derive(Clone, Debug)]
pub struct RunOpts {
    /// Only the first N sessions of each sector; None = all.
    pub first_sessions: Option<usize>,
    /// Real seconds per nominal backoff second (0.001 turns 30 s into 30 ms).
    pub time_scale: f64,
    /// Abort the run after this long.
    pub max_wall: Duration,
}

impl Default for RunOpts {
    fn default() -> Self {
        RunOpts {
            first_sessions: None,
            time_scale: 1.0,
            max_wall: Duration::from_secs(3600),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct RunSummary {
    pub run: String,
    pub in_scope: usize,
    pub dispatched: u32,
    pub completed: usize,
    pub stops: u32,
    pub request_limit_stops: u32,
    pub stall_stops: u32,
    pub retries: u32,
    pub failed: usize,
    pub held: usize,
    pub refused: usize,
    pub blocked: usize,
    pub peak_slots: usize,
    pub wall_ms: u128,
}

struct Done {
    key: String,
    attempt: u32,
    agent: String,
    session: String,
    outcome: RunOutcome,
}

impl Hermes {
    /// Load every sector's plan from `plans_dir`.
    ///
    /// # Errors
    /// A plan file is missing or has no sessions.
    pub fn load(reg: Registry, plans_dir: &Path) -> Result<Hermes, String> {
        let mut plans = Vec::new();
        for s in &reg.sectors {
            plans.push(Plan::load(&s.id, &plans_dir.join(&s.plan))?);
        }
        Ok(Hermes::from_plans(reg, plans))
    }

    pub fn from_plans(reg: Registry, plans: Vec<Plan>) -> Hermes {
        let mut tasks = BTreeMap::new();
        let mut order = Vec::new();
        for p in &plans {
            for t in &p.tasks {
                order.push(t.key.clone());
                tasks.insert(
                    t.key.clone(),
                    TaskRt {
                        task: t.clone(),
                        state: TaskState::New,
                        attempts: 0,
                        stops: 0,
                        approved: false,
                        last: String::new(),
                        due: None,
                    },
                );
            }
        }
        Hermes {
            reg,
            plans,
            tasks,
            order,
        }
    }

    /// Rebuild task states from ledger events (operator intake included).
    pub fn apply(&mut self, events: &[Event]) {
        for ev in events {
            if ev.kind == "intake" {
                self.add_intake(ev);
            }
            let Some(key) = &ev.task else { continue };
            let Some(rt) = self.tasks.get_mut(key) else {
                continue;
            };
            if let Some(a) = ev.attempt {
                rt.attempts = rt.attempts.max(a);
            }
            rt.last = ev.detail.clone();
            rt.state = match ev.kind.as_str() {
                "queued" => TaskState::Queued,
                "held" => TaskState::Held,
                "refused" => TaskState::Refused,
                "approved" => {
                    rt.approved = true;
                    TaskState::Queued
                }
                "dispatched" => TaskState::Running,
                "stopped" => {
                    rt.stops += 1;
                    rt.state
                }
                "retry-scheduled" => TaskState::RetryWait,
                "completed" => TaskState::Done,
                "failed" => TaskState::Failed,
                "blocked" => TaskState::Blocked,
                _ => rt.state,
            };
        }
    }

    /// Operator intake: a task that is in no plan, sent to Hermes directly.
    /// The class is the classifier's or the operator's, whichever is more
    /// dangerous: an operator cannot label a merge as code.
    pub fn intake_task(
        &self,
        sector: &str,
        title: &str,
        class: Option<ActionClass>,
        deps: Vec<String>,
    ) -> Result<Task, String> {
        if self.reg.sector(sector).is_none() {
            return Err(format!("unknown sector {sector:?}"));
        }
        let title = title.trim();
        if title.is_empty() || title.len() > 400 {
            return Err("title must be 1-400 characters".into());
        }
        for d in &deps {
            if !self.tasks.contains_key(d) {
                return Err(format!("unknown dependency {d:?}"));
            }
        }
        let n = self
            .tasks
            .values()
            .filter(|t| t.task.sector == sector && t.task.session == INTAKE_SESSION)
            .count()
            + 1;
        let auto = classify(title, "operator intake");
        let class = class.map_or(auto, |c| c.max(auto));
        Ok(Task {
            key: format!("{sector}:H{n}"),
            id: format!("H{n}"),
            sector: sector.to_string(),
            session: INTAKE_SESSION.into(),
            session_ord: 0,
            title: title.to_string(),
            target: Some(1),
            useful: Some(1),
            deps_text: deps.join(", "),
            deps,
            class,
            line: 0,
        })
    }

    fn add_intake(&mut self, ev: &Event) {
        let Ok(t) = serde_json::from_str::<Task>(&ev.detail) else {
            return;
        };
        if self.tasks.contains_key(&t.key) || self.reg.sector(&t.sector).is_none() {
            return;
        }
        self.order.push(t.key.clone());
        self.tasks.insert(
            t.key.clone(),
            TaskRt {
                task: t,
                state: TaskState::Queued,
                attempts: 0,
                stops: 0,
                approved: false,
                last: String::new(),
                due: None,
            },
        );
    }

    pub fn sector_cap(&self, sector: &str) -> usize {
        self.reg.sector(sector).map_or(1, |s| s.max_concurrent)
    }

    fn in_scope(&self, t: &Task, first: Option<usize>) -> bool {
        first.is_none_or(|n| t.session_ord < n)
    }

    fn deps_pending(&self, t: &Task) -> Vec<String> {
        t.deps
            .iter()
            .filter(|d| {
                self.tasks
                    .get(*d)
                    .is_some_and(|r| r.state != TaskState::Done)
            })
            .cloned()
            .collect()
    }

    fn dead_dep(&self, t: &Task) -> Option<String> {
        t.deps
            .iter()
            .find(|d| self.tasks.get(*d).is_some_and(|r| r.state.dead()))
            .cloned()
    }

    fn model_for(&self) -> String {
        self.reg.hermes.sub_model.clone()
    }

    fn lead_for(&self, sector: &str) -> String {
        self.reg
            .sector(sector)
            .map_or_else(|| format!("{sector}.lead"), |s| s.lead.clone())
    }

    /// What `dispatch` would do now, without writing anything.
    pub fn dry_run(&self, gate: &Gate, first: Option<usize>, cap: Option<usize>) -> DispatchPlan {
        let cap = cap.unwrap_or(self.reg.hermes.concurrency_cap);
        let reserved = self.reg.hermes.reserved_slots.min(cap.saturating_sub(1));
        let worker = cap - reserved;
        let caps: BTreeMap<String, usize> = self
            .reg
            .sectors
            .iter()
            .map(|s| (s.id.clone(), s.max_concurrent))
            .collect();
        let gov = Governor::new(worker, caps);
        let mut permits = Vec::new();
        let mut queues: Vec<Vec<&TaskRt>> = self
            .reg
            .sectors
            .iter()
            .map(|s| {
                self.order
                    .iter()
                    .filter_map(|k| self.tasks.get(k))
                    .filter(|r| r.task.sector == s.id)
                    .filter(|r| self.in_scope(&r.task, first))
                    .filter(|r| {
                        matches!(
                            r.state,
                            TaskState::New | TaskState::Queued | TaskState::RetryWait
                        )
                    })
                    .collect()
            })
            .collect();
        let mut rows = Vec::new();
        let mut round = 0;
        while queues.iter().any(|q| round < q.len()) {
            for q in &queues {
                let Some(rt) = q.get(round) else { continue };
                let t = &rt.task;
                let slots = gov.clamp(&t.sector, t.slots(self.sector_cap(&t.sector)));
                let pending = self.deps_pending(t);
                let decision = gate.preview(t);
                let (what, agent) = if !pending.is_empty() {
                    ("wait-deps", None)
                } else if !decision.may_dispatch() {
                    (decision.label(), None)
                } else if let Some(p) = gov.try_acquire(&t.sector, slots) {
                    permits.push(p);
                    (
                        "dispatch",
                        Some(format!(
                            "{}.{}.a{}",
                            t.sector,
                            local_id(&t.key),
                            rt.attempts + 1
                        )),
                    )
                } else {
                    ("wait-cap", None)
                };
                rows.push(DispatchRow {
                    task: t.key.clone(),
                    sector: t.sector.clone(),
                    session: t.session.clone(),
                    title: t.title.clone(),
                    class: t.class.as_str().into(),
                    slots,
                    decision: what.into(),
                    agent,
                    model: self.model_for(),
                    gate: decision,
                    deps_pending: pending,
                });
            }
            round += 1;
        }
        let snap = gov.snapshot();
        drop(permits);
        queues.clear();
        DispatchPlan {
            cap,
            reserved,
            worker_slots: worker,
            slots_used: snap.used,
            scope: first.map_or("all sessions".into(), |n| {
                format!("first {n} session(s) per sector")
            }),
            rows,
            per_sector: snap.per_sector,
        }
    }

    /// Run the in-scope tasks to the end through `provider`, writing every
    /// step to `ledger`. Gated tasks need a token in `tokens` (task key ->
    /// token); without one they are held.
    ///
    /// # Errors
    /// Ledger write failures, or the wall-clock limit.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        ledger: &mut Ledger,
        provider: Arc<dyn Provider>,
        gov: &Governor,
        policy: &RetryPolicy,
        gate: &Gate,
        tokens: &BTreeMap<String, String>,
        opts: &RunOpts,
    ) -> Result<RunSummary, String> {
        let started = Instant::now();
        let run = new_id("run-");
        let scope: Vec<String> = self
            .order
            .iter()
            .filter(|k| self.in_scope(&self.tasks[*k].task, opts.first_sessions))
            .cloned()
            .collect();
        let mut sum = RunSummary {
            run: run.clone(),
            in_scope: scope.len(),
            ..RunSummary::default()
        };
        ledger.append(NewEvent::new("run-start").detail(format!(
            "{run}: provider={} cap={} scope={} tasks={} time_scale={}",
            provider.name(),
            gov.cap(),
            opts.first_sessions
                .map_or("all".into(), |n| format!("first-{n}-sessions")),
            scope.len(),
            opts.time_scale
        )))?;

        // Intake, crash recovery and the HADES gate.
        for key in &scope {
            let rt = &self.tasks[key];
            let (sector, state, gated, approved) = (
                rt.task.sector.clone(),
                rt.state,
                rt.task.class.gated(),
                rt.approved,
            );
            if state == TaskState::New {
                self.set(
                    ledger,
                    key,
                    &sector,
                    "queued",
                    TaskState::Queued,
                    &rt_title(&self.tasks[key]),
                )?;
            }
            if state == TaskState::Running {
                ledger.append(
                    NewEvent::new("stopped")
                        .task(key, &sector)
                        .state("retry-wait")
                        .detail("stall: Hermes restarted while this attempt was running"),
                )?;
                let rt = self.tasks.get_mut(key).expect("in scope");
                rt.stops += 1;
                rt.state = TaskState::RetryWait;
                rt.due = Some(Instant::now());
            }
            if state == TaskState::RetryWait {
                self.tasks.get_mut(key).expect("in scope").due = Some(Instant::now());
            }
            let state = self.tasks[key].state;
            if gated
                && !approved
                && matches!(
                    state,
                    TaskState::Queued | TaskState::Held | TaskState::Refused
                )
            {
                let token = tokens.get(key).map_or("", String::as_str);
                let d = gate.check(&self.tasks[key].task, token);
                let detail = serde_json::to_string(&d).unwrap_or_default();
                match d {
                    GateDecision::Approved { .. } => {
                        self.tasks.get_mut(key).expect("in scope").approved = true;
                        self.set(ledger, key, &sector, "approved", TaskState::Queued, &detail)?;
                    }
                    GateDecision::Held { .. } => {
                        self.set(ledger, key, &sector, "held", TaskState::Held, &detail)?;
                    }
                    GateDecision::Refused { .. } => {
                        self.set(ledger, key, &sector, "refused", TaskState::Refused, &detail)?;
                    }
                    GateDecision::NotNeeded => {}
                }
            }
        }

        let (tx, rx) = mpsc::channel::<Done>();
        let mut running = 0usize;
        let sectors: Vec<String> = self.reg.sectors.iter().map(|s| s.id.clone()).collect();
        loop {
            if started.elapsed() > opts.max_wall {
                ledger.append(NewEvent::new("run-aborted").detail(format!(
                    "{run}: wall-clock limit {} s reached with {running} running",
                    opts.max_wall.as_secs()
                )))?;
                return Err(format!("run {run} aborted at the wall-clock limit"));
            }
            let now = Instant::now();
            // Block tasks whose dependencies can no longer finish.
            for key in &scope {
                let rt = &self.tasks[key];
                if matches!(rt.state, TaskState::Queued | TaskState::RetryWait) {
                    if let Some(dep) = self.dead_dep(&rt.task) {
                        let why =
                            format!("dependency {dep} is {}", self.tasks[&dep].state.as_str());
                        let sector = rt.task.sector.clone();
                        self.set(ledger, key, &sector, "blocked", TaskState::Blocked, &why)?;
                    }
                }
            }
            // Ready tasks, round-robin across sectors.
            let mut per_sector: Vec<Vec<String>> = vec![Vec::new(); sectors.len()];
            let mut waiting_retry: Option<Instant> = None;
            for key in &scope {
                let rt = &self.tasks[key];
                let ready_state = match rt.state {
                    TaskState::Queued => true,
                    TaskState::RetryWait => match rt.due {
                        Some(d) if d <= now => true,
                        Some(d) => {
                            waiting_retry = Some(waiting_retry.map_or(d, |w| w.min(d)));
                            false
                        }
                        None => true,
                    },
                    _ => false,
                };
                let gate_ok = !rt.task.class.gated() || rt.approved;
                if ready_state && gate_ok && self.deps_pending(&rt.task).is_empty() {
                    if let Some(i) = sectors.iter().position(|s| *s == rt.task.sector) {
                        per_sector[i].push(key.clone());
                    }
                }
            }
            let ready_any = per_sector.iter().any(|q| !q.is_empty());
            let mut round = 0;
            let mut launched = false;
            while per_sector.iter().any(|q| round < q.len()) {
                for q in &per_sector {
                    let Some(key) = q.get(round) else { continue };
                    let (sector, want) = {
                        let t = &self.tasks[key].task;
                        (t.sector.clone(), t.slots(self.sector_cap(&t.sector)))
                    };
                    let Some(permit) = gov.try_acquire(&sector, want) else {
                        continue;
                    };
                    let (lead, model) = (self.lead_for(&sector), self.model_for());
                    let rt = self.tasks.get_mut(key).expect("ready");
                    rt.attempts += 1;
                    rt.state = TaskState::Running;
                    let attempt = rt.attempts;
                    let agent = format!("{sector}.{}.a{attempt}", local_id(key));
                    let session = new_id("ses-");
                    let brief = Brief {
                        task: key.clone(),
                        sector: sector.clone(),
                        lead,
                        agent: agent.clone(),
                        session: session.clone(),
                        model,
                        attempt,
                        slots: permit.slots() as u32,
                        title: rt.task.title.clone(),
                        deps: rt.task.deps.clone(),
                    };
                    ledger.append(
                        NewEvent::new("dispatched")
                            .task(key, &sector)
                            .agent(&agent)
                            .session(&session)
                            .attempt(attempt)
                            .slots(permit.slots() as u32)
                            .state("running")
                            .detail(format!(
                                "{run}: model {} via {}",
                                brief.model,
                                provider.name()
                            )),
                    )?;
                    sum.dispatched += 1;
                    running += 1;
                    launched = true;
                    let tx = tx.clone();
                    let provider = provider.clone();
                    let key = key.clone();
                    std::thread::Builder::new()
                        .name(format!("agent-{agent}"))
                        .spawn(move || {
                            let outcome = provider.run(&brief);
                            drop(permit);
                            let _ = tx.send(Done {
                                key,
                                attempt,
                                agent: brief.agent,
                                session: brief.session,
                                outcome,
                            });
                        })
                        .map_err(|e| format!("spawn agent thread: {e}"))?;
                }
                round += 1;
            }
            sum.peak_slots = sum.peak_slots.max(gov.snapshot().peak);
            if running == 0 && !launched && waiting_retry.is_none() {
                if ready_any {
                    // Ready but nothing fits: a task wider than the cap would loop forever.
                    return Err("ready tasks cannot fit the governor".into());
                }
                break;
            }
            let wait = waiting_retry
                .map(|d| d.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_millis(200))
                .clamp(Duration::from_millis(1), Duration::from_millis(200));
            let first = if running > 0 {
                rx.recv_timeout(wait).ok()
            } else {
                std::thread::sleep(wait);
                None
            };
            let mut batch: Vec<Done> = first.into_iter().collect();
            while let Ok(d) = rx.try_recv() {
                batch.push(d);
            }
            for d in batch {
                running -= 1;
                self.finish(ledger, policy, opts, &run, d, &mut sum)?;
            }
        }

        // Anything still queued could not start: its dependency is outside scope.
        for key in &scope {
            let rt = &self.tasks[key];
            if matches!(rt.state, TaskState::Queued | TaskState::RetryWait) {
                let pending = self.deps_pending(&rt.task);
                let why = format!(
                    "dependencies outside this run are not done: {}",
                    pending.join(", ")
                );
                let sector = rt.task.sector.clone();
                self.set(ledger, key, &sector, "blocked", TaskState::Blocked, &why)?;
            }
        }
        for key in &scope {
            match self.tasks[key].state {
                TaskState::Done => sum.completed += 1,
                TaskState::Failed => sum.failed += 1,
                TaskState::Held => sum.held += 1,
                TaskState::Refused => sum.refused += 1,
                TaskState::Blocked => sum.blocked += 1,
                _ => {}
            }
        }
        sum.wall_ms = started.elapsed().as_millis();
        ledger.append(
            NewEvent::new("run-end").detail(serde_json::to_string(&sum).unwrap_or_default()),
        )?;
        Ok(sum)
    }

    fn finish(
        &mut self,
        ledger: &mut Ledger,
        policy: &RetryPolicy,
        opts: &RunOpts,
        run: &str,
        d: Done,
        sum: &mut RunSummary,
    ) -> Result<(), String> {
        let sector = self.tasks[&d.key].task.sector.clone();
        let base = NewEvent::new("")
            .task(&d.key, &sector)
            .agent(&d.agent)
            .session(&d.session)
            .attempt(d.attempt);
        match d.outcome {
            RunOutcome::Completed { summary } => {
                let mut ev = base.state("done").detail(summary);
                ev.kind = "completed".into();
                ledger.append(ev)?;
                self.tasks.get_mut(&d.key).expect("known").state = TaskState::Done;
            }
            RunOutcome::Stopped(kind) => {
                sum.stops += 1;
                match kind {
                    StopKind::RequestLimit => sum.request_limit_stops += 1,
                    StopKind::Stall => sum.stall_stops += 1,
                    StopKind::Error(_) => {}
                }
                let detail = match &kind {
                    StopKind::Error(e) => format!("error: {e}"),
                    k => k.as_str().to_string(),
                };
                let mut ev = base.clone().detail(format!("{run}: {detail}"));
                ev.kind = "stopped".into();
                ledger.append(ev)?;
                let rt = self.tasks.get_mut(&d.key).expect("known");
                rt.stops += 1;
                match policy.next(&kind, d.attempt) {
                    Next::Retry { after_secs } => {
                        let real =
                            Duration::from_secs_f64(after_secs as f64 * opts.time_scale.max(0.0));
                        rt.state = TaskState::RetryWait;
                        rt.due = Some(Instant::now() + real);
                        sum.retries += 1;
                        let mut ev = base.state("retry-wait").detail(format!(
                            "retry {} after {} backoff {after_secs} s (waited {} ms at time scale {})",
                            d.attempt + 1,
                            kind.as_str(),
                            real.as_millis(),
                            opts.time_scale
                        ));
                        ev.kind = "retry-scheduled".into();
                        ledger.append(ev)?;
                    }
                    Next::Fail(why) => {
                        rt.state = TaskState::Failed;
                        let mut ev = base.state("failed").detail(why);
                        ev.kind = "failed".into();
                        ledger.append(ev)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn set(
        &mut self,
        ledger: &mut Ledger,
        key: &str,
        sector: &str,
        kind: &str,
        state: TaskState,
        detail: &str,
    ) -> Result<(), String> {
        ledger.append(
            NewEvent::new(kind)
                .task(key, sector)
                .state(state.as_str())
                .detail(detail),
        )?;
        if let Some(rt) = self.tasks.get_mut(key) {
            rt.state = state;
            rt.last = detail.to_string();
        }
        Ok(())
    }

    /// Count of tasks per state.
    pub fn state_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for rt in self.tasks.values() {
            *m.entry(rt.state.as_str()).or_insert(0) += 1;
        }
        m
    }
}

fn rt_title(rt: &TaskRt) -> String {
    format!("{} [{}]", rt.task.title, rt.task.class.as_str())
}

/// `gaia:S1.1` -> `S1.1`.
pub fn local_id(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

/// One row of the retry ledger (CALL-RECORD.md table shape).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RetryRow {
    pub task: String,
    pub stops: u32,
    pub request_limit: u32,
    pub stalls: u32,
    pub retries: u32,
    pub attempts: u32,
    pub latest_start: String,
    pub outcome: String,
}

/// Every task that was ever stopped, from ledger events.
pub fn retry_table(events: &[Event]) -> Vec<RetryRow> {
    let mut rows: BTreeMap<String, RetryRow> = BTreeMap::new();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    for ev in events {
        let Some(task) = &ev.task else { continue };
        let r = rows.entry(task.clone()).or_insert_with(|| RetryRow {
            task: task.clone(),
            stops: 0,
            request_limit: 0,
            stalls: 0,
            retries: 0,
            attempts: 0,
            latest_start: String::new(),
            outcome: String::new(),
        });
        match ev.kind.as_str() {
            "dispatched" => {
                r.attempts = r.attempts.max(ev.attempt.unwrap_or(0));
                r.latest_start = ev.ts.clone();
            }
            "stopped" => {
                r.stops += 1;
                touched.insert(task.clone());
                if ev.detail.contains("request-limit") {
                    r.request_limit += 1;
                } else if ev.detail.contains("stall") {
                    r.stalls += 1;
                }
            }
            "retry-scheduled" => r.retries += 1,
            _ => {}
        }
        if let Some(s) = &ev.state {
            r.outcome = s.clone();
        }
    }
    rows.into_values()
        .filter(|r| touched.contains(&r.task))
        .collect()
}
