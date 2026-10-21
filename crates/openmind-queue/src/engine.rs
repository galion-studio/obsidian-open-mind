// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The POSEIDON queue engine: an event-sourced state machine.
//!
//! Every public "decide" method (`enqueue`, `lease`, `ack`, ...) looks at the
//! state and either refuses or returns the [`Record`]s that describe the
//! change. The caller makes the records durable (see [`crate::store`]) and
//! then calls [`Engine::apply`]. Replaying the log through `apply` rebuilds
//! exactly the same state, so a crash at any point loses nothing that was
//! acknowledged to a client.
//!
//! Time is always passed in (`now` in epoch ms), which keeps the engine
//! deterministic and testable.

use crate::vram::{Device, VramPool, CPU};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Words in a job `kind` that mean real money movement. POSEIDON tracks
/// spend as numbers only; such jobs are refused (Revolut / real money is
/// NOT approved).
pub const MONEY_WORDS: &[&str] = &[
    "revolut", "payment", "transfer", "iban", "withdraw", "payout", "purchase",
];

/// The scope name used for the global (all agents) budget.
pub const GLOBAL: &str = "*";

/// Where a job is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    /// Can be leased now.
    Ready,
    /// Waiting for its retry backoff to pass.
    Delayed,
    /// Held by a worker until `deadline` (the visibility timeout).
    Leased,
    /// In the dead-letter queue.
    Dead,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Ready => "ready",
            JobState::Delayed => "delayed",
            JobState::Leased => "leased",
            JobState::Dead => "dead",
        }
    }
    pub fn parse(s: &str) -> Option<JobState> {
        Some(match s {
            "ready" => JobState::Ready,
            "delayed" => JobState::Delayed,
            "leased" => JobState::Leased,
            "dead" => JobState::Dead,
            _ => return None,
        })
    }
}

/// What a producer submits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobSpec {
    pub queue: String,
    /// The agent the job runs for. Quotas and budgets are per agent.
    pub agent: String,
    pub kind: String,
    /// Higher runs first. Equal priority is FIFO.
    pub priority: i32,
    /// Deliveries allowed before the job goes to the dead-letter queue.
    pub max_attempts: u32,
    /// Spend units reserved per attempt (a number only; never charged anywhere real).
    pub cost: u64,
    /// Visibility timeout per lease, in ms (0 = engine default).
    pub lease_ms: u64,
    pub payload: String,
    /// VRAM the job needs, in GB (0 = none). Routed GPU -> LAN -> remote,
    /// approved and healthy devices only.
    pub vram_gb: u32,
    /// When no approved device has room, run on CPU instead of waiting.
    pub cpu_fallback: bool,
}

impl Default for JobSpec {
    fn default() -> Self {
        JobSpec {
            queue: "default".into(),
            agent: "anon".into(),
            kind: "job".into(),
            priority: 0,
            max_attempts: 0,
            cost: 0,
            lease_ms: 0,
            payload: String::new(),
            vram_gb: 0,
            cpu_fallback: false,
        }
    }
}

/// A job the engine still tracks (ready, delayed, leased or dead).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub spec: JobSpec,
    pub created_ms: u64,
    pub state: JobState,
    /// Deliveries so far.
    pub attempts: u32,
    /// When a delayed job becomes ready.
    pub ready_at: u64,
    /// Fencing token of the current (or last) lease. Acks with an older token are refused.
    pub token: u64,
    pub worker: String,
    pub deadline: u64,
    pub reserved: u64,
    pub last_error: String,
    /// Device of the current lease: a VRAM-pool device id, `cpu` for offload,
    /// or empty when the job needs no VRAM.
    #[serde(default)]
    pub device: String,
}

/// Every durable change. One line each in the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Record {
    Enqueue {
        id: u64,
        now: u64,
        spec: JobSpec,
    },
    Lease {
        id: u64,
        token: u64,
        worker: String,
        deadline: u64,
        reserved: u64,
        #[serde(default)]
        device: String,
    },
    Extend {
        id: u64,
        token: u64,
        deadline: u64,
    },
    Ack {
        id: u64,
        token: u64,
        charge: u64,
    },
    /// A failed attempt (worker nack, or the lease expired).
    /// `retry_at == None` sends the job to the dead-letter queue.
    Fail {
        id: u64,
        token: u64,
        charge: u64,
        retry_at: Option<u64>,
        expired: bool,
        error: String,
    },
    /// Move a dead job back to ready with a fresh attempt count.
    Redrive {
        id: u64,
        now: u64,
    },
    /// Set (or clear) a spend cap. `scope` is an agent or [`GLOBAL`].
    Cap {
        scope: String,
        cap: Option<u64>,
    },
    /// Set an agent's quota. `agent == GLOBAL` sets the default for every agent.
    Quota {
        agent: String,
        max_pending: u32,
        max_inflight: u32,
    },
    Device(Device),
    DeviceRemove {
        id: String,
    },
    // --- compaction snapshot records ---
    Snapshot(Box<Job>),
    Spent {
        scope: String,
        spent: u64,
    },
    Next {
        id: u64,
        token: u64,
    },
    /// Counters carried across compaction.
    Counters(Stats),
    /// Refusal counters as absolute values (slot 206). Refusals change no
    /// job state, so before this record they lived only in memory and a
    /// crash or restart reset them. `blocked` holds only the agents whose
    /// `budget_blocked` changed since the last tally written.
    Tally(Tally),
}

/// The refusal counters that no job record carries: written as a
/// [`Record::Tally`] before the refused client is answered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub budget_refusals: u64,
    pub quota_refusals: u64,
    pub stale_acks: u64,
    pub charge_clamped: u64,
    pub backpressure_refusals: u64,
    /// Per-agent `budget_blocked`.
    pub blocked: BTreeMap<String, u64>,
}

impl Tally {
    /// What must be written to move a log that holds `self` to `now`:
    /// `None` when nothing changed, else `now` with only changed agents.
    pub fn delta_to(&self, now: &Tally) -> Option<Tally> {
        let blocked: BTreeMap<String, u64> = now
            .blocked
            .iter()
            .filter(|(k, v)| self.blocked.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let same = self.budget_refusals == now.budget_refusals
            && self.quota_refusals == now.quota_refusals
            && self.stale_acks == now.stale_acks
            && self.charge_clamped == now.charge_clamped
            && self.backpressure_refusals == now.backpressure_refusals;
        if same && blocked.is_empty() {
            return None;
        }
        Some(Tally {
            blocked,
            ..now.clone()
        })
    }
}

/// Why a decide method refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    BadRequest(String),
    /// Real money is not approved.
    Money,
    /// The agent already has `max_pending` jobs waiting.
    Quota(String),
    /// The spend cap would be crossed. This is the hard stop.
    Budget(String),
    NotFound,
    /// The lease token is not the current one (the lease expired and the job
    /// was re-delivered, or it was already acked).
    Stale,
    /// The queue is at `max_depth` (backpressure).
    Full(String),
    /// No approved, healthy device could ever hold this much VRAM and CPU
    /// fallback was not allowed.
    Vram(String),
}

impl Refusal {
    pub fn status(&self) -> u16 {
        match self {
            Refusal::BadRequest(_) => 400,
            Refusal::Money => 403,
            Refusal::Quota(_) => 429,
            Refusal::Budget(_) => 402,
            Refusal::NotFound => 404,
            Refusal::Stale => 409,
            Refusal::Full(_) => 503,
            Refusal::Vram(_) => 503,
        }
    }
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::BadRequest(_) => "bad_request",
            Refusal::Money => "real_money_not_approved",
            Refusal::Quota(_) => "quota_exceeded",
            Refusal::Budget(_) => "budget_hard_stop",
            Refusal::NotFound => "not_found",
            Refusal::Stale => "stale_lease",
            Refusal::Full(_) => "backpressure",
            Refusal::Vram(_) => "vram_unavailable",
        }
    }
    pub fn detail(&self) -> String {
        match self {
            Refusal::BadRequest(s)
            | Refusal::Quota(s)
            | Refusal::Budget(s)
            | Refusal::Full(s)
            | Refusal::Vram(s) => s.clone(),
            Refusal::Money => "POSEIDON tracks spend as numbers only".into(),
            Refusal::NotFound => "no such job".into(),
            Refusal::Stale => "lease token is not current".into(),
        }
    }
}

/// Why `lease` returned nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    Empty,
    /// Jobs are waiting but every one of them would cross a spend cap.
    BudgetStopped,
    /// Jobs are waiting but their agents are at `max_inflight`.
    QuotaHeld,
    /// Jobs are waiting for VRAM on an approved device.
    VramHeld,
}

/// Engine tunables.
#[derive(Clone, Debug)]
pub struct Config {
    pub default_lease_ms: u64,
    pub default_max_attempts: u32,
    pub backoff_base_ms: u64,
    pub backoff_max_ms: u64,
    pub default_max_pending: u32,
    pub default_max_inflight: u32,
    /// Live (not dead) jobs across all queues before enqueue answers 503.
    pub max_depth: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_lease_ms: 30_000,
            default_max_attempts: 5,
            backoff_base_ms: 1_000,
            backoff_max_ms: 300_000,
            default_max_pending: 100_000,
            default_max_inflight: 1_000,
            max_depth: 100_000,
        }
    }
}

/// A spend account: cap, committed spend and in-flight reservations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub cap: Option<u64>,
    pub spent: u64,
    pub reserved: u64,
}

impl Budget {
    /// Can `cost` more be reserved without crossing the cap?
    pub fn fits(&self, cost: u64) -> bool {
        match self.cap {
            None => true,
            Some(cap) => {
                self.spent
                    .saturating_add(self.reserved)
                    .saturating_add(cost)
                    <= cap
            }
        }
    }
    pub fn remaining(&self) -> Option<u64> {
        self.cap
            .map(|c| c.saturating_sub(self.spent.saturating_add(self.reserved)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Agent {
    pub budget: Budget,
    pub pending: u32,
    pub inflight: u32,
    pub quota: Option<(u32, u32)>,
    /// Jobs skipped at lease time because the cap would be crossed.
    pub budget_blocked: u64,
}

/// Counters, rebuilt by replay. The refusal counters are carried by
/// [`Record::Tally`] (before slot 206 they were process-local).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub enqueued: u64,
    pub delivered: u64,
    pub redelivered: u64,
    pub acked: u64,
    pub nacked: u64,
    pub expired: u64,
    pub dead: u64,
    pub redriven: u64,
    pub budget_refusals: u64,
    pub quota_refusals: u64,
    pub stale_acks: u64,
    pub charge_clamped: u64,
    pub vram_offloaded: u64,
    pub backpressure_refusals: u64,
}

type ReadyKey = (Reverse<i32>, u64);

/// The queue engine.
#[derive(Debug, Default)]
pub struct Engine {
    pub cfg: Config,
    jobs: HashMap<u64, Job>,
    ready: HashMap<String, BTreeSet<ReadyKey>>,
    delayed: BTreeSet<(u64, u64)>,
    leased: BTreeSet<(u64, u64)>,
    agents: BTreeMap<String, Agent>,
    global: Budget,
    default_quota: Option<(u32, u32)>,
    next_id: u64,
    next_token: u64,
    pub stats: Stats,
    pub vram: VramPool,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

impl Engine {
    pub fn new(cfg: Config) -> Engine {
        Engine {
            cfg,
            next_id: 1,
            next_token: 1,
            ..Engine::default()
        }
    }

    // ------------------------------------------------------------ queries

    pub fn job(&self, id: u64) -> Option<&Job> {
        self.jobs.get(&id)
    }
    pub fn agents(&self) -> &BTreeMap<String, Agent> {
        &self.agents
    }
    pub fn global(&self) -> &Budget {
        &self.global
    }
    pub fn dead_jobs(&self) -> Vec<&Job> {
        let mut v: Vec<&Job> = self
            .jobs
            .values()
            .filter(|j| j.state == JobState::Dead)
            .collect();
        v.sort_by_key(|j| j.id);
        v
    }
    /// (ready, delayed, leased, dead) counts.
    pub fn depth(&self) -> (usize, usize, usize, usize) {
        let ready = self.ready.values().map(|s| s.len()).sum();
        let dead = self.jobs.len() - ready - self.delayed.len() - self.leased.len();
        (ready, self.delayed.len(), self.leased.len(), dead)
    }
    pub fn quota_of(&self, agent: &str) -> (u32, u32) {
        self.agents
            .get(agent)
            .and_then(|a| a.quota)
            .or(self.default_quota)
            .unwrap_or((self.cfg.default_max_pending, self.cfg.default_max_inflight))
    }
    /// The earliest instant something time-based happens (a delayed job
    /// becomes ready or a lease expires).
    pub fn next_wake(&self) -> Option<u64> {
        let d = self.delayed.first().map(|x| x.0);
        let l = self.leased.first().map(|x| x.0);
        match (d, l) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    /// Exponential backoff after `attempts` deliveries.
    pub fn backoff(&self, attempts: u32) -> u64 {
        let shift = attempts.saturating_sub(1).min(32);
        self.cfg
            .backoff_base_ms
            .saturating_mul(1u64 << shift)
            .min(self.cfg.backoff_max_ms)
    }

    // ------------------------------------------------------------ decide

    /// Admit a job, or refuse it (bad name, money, quota, or budget).
    pub fn enqueue(&mut self, mut spec: JobSpec, now: u64) -> Result<Record, Refusal> {
        if !valid_name(&spec.queue) || !valid_name(&spec.agent) || !valid_name(&spec.kind) {
            return Err(Refusal::BadRequest(
                "queue, agent and kind must be 1-64 chars of [A-Za-z0-9-_.:]".into(),
            ));
        }
        let kind = spec.kind.to_ascii_lowercase();
        if MONEY_WORDS.iter().any(|w| kind.contains(w)) {
            return Err(Refusal::Money);
        }
        if spec.payload.len() > 64 * 1024 {
            return Err(Refusal::BadRequest("payload over 64 KiB".into()));
        }
        if spec.max_attempts == 0 {
            spec.max_attempts = self.cfg.default_max_attempts.max(1);
        }
        if spec.lease_ms == 0 {
            spec.lease_ms = self.cfg.default_lease_ms;
        }
        let live = self.jobs.len() - self.dead_count();
        if live >= self.cfg.max_depth {
            self.stats.backpressure_refusals += 1;
            return Err(Refusal::Full(format!(
                "queue depth {live} is at max_depth {}",
                self.cfg.max_depth
            )));
        }
        if spec.vram_gb > 0 && !spec.cpu_fallback && !self.vram.can_ever_fit(spec.vram_gb) {
            return Err(Refusal::Vram(format!(
                "no approved, healthy device can hold {} GB (largest {} GB); set cpu_fallback or get a device approved",
                spec.vram_gb,
                self.vram.largest_routable_gb()
            )));
        }
        let (max_pending, _) = self.quota_of(&spec.agent);
        let pending = self.agents.get(&spec.agent).map_or(0, |a| a.pending);
        if pending >= max_pending {
            self.stats.quota_refusals += 1;
            return Err(Refusal::Quota(format!(
                "agent {} has {pending} pending jobs (max_pending {max_pending})",
                spec.agent
            )));
        }
        // A job whose single attempt can never fit under the cap is refused
        // at the door; that is the visible face of the hard stop.
        let ab = self
            .agents
            .get(&spec.agent)
            .map(|a| a.budget.clone())
            .unwrap_or_default();
        for (scope, b) in [(spec.agent.as_str(), &ab), (GLOBAL, &self.global)] {
            if let Some(cap) = b.cap {
                if b.spent.saturating_add(spec.cost) > cap {
                    self.stats.budget_refusals += 1;
                    return Err(Refusal::Budget(format!(
                        "budget {scope}: spent {} + cost {} > cap {cap}",
                        b.spent, spec.cost
                    )));
                }
            }
        }
        Ok(Record::Enqueue {
            id: self.next_id,
            now,
            spec,
        })
    }

    /// Move delayed jobs whose backoff has passed to ready. In-memory only:
    /// it is derived from time, so it is never logged.
    pub fn promote(&mut self, now: u64) {
        while let Some(&(at, id)) = self.delayed.first() {
            if at > now {
                break;
            }
            self.delayed.pop_first();
            if let Some(j) = self.jobs.get_mut(&id) {
                j.state = JobState::Ready;
                self.ready
                    .entry(j.spec.queue.clone())
                    .or_default()
                    .insert((Reverse(j.spec.priority), id));
            }
        }
    }

    /// Pick the best leasable job in `queue`: highest priority, then oldest,
    /// skipping agents at their in-flight quota and jobs whose reservation
    /// would cross a spend cap (agent or global).
    pub fn lease(
        &mut self,
        queue: &str,
        worker: &str,
        now: u64,
        lease_ms: u64,
    ) -> Result<Record, Miss> {
        self.promote(now);
        let Some(set) = self.ready.get(queue) else {
            return Err(Miss::Empty);
        };
        let mut miss = Miss::Empty;
        let mut blocked: Vec<String> = Vec::new();
        let mut pick = None;
        for &(_, id) in set.iter() {
            let j = &self.jobs[&id];
            let a = self.agents.get(&j.spec.agent);
            let (_, max_inflight) = self.quota_of(&j.spec.agent);
            if a.map_or(0, |a| a.inflight) >= max_inflight {
                if miss == Miss::Empty {
                    miss = Miss::QuotaHeld;
                }
                continue;
            }
            let agent_fits = a.is_none_or(|a| a.budget.fits(j.spec.cost));
            if !agent_fits || !self.global.fits(j.spec.cost) {
                miss = Miss::BudgetStopped;
                blocked.push(j.spec.agent.clone());
                continue;
            }
            let device = if j.spec.vram_gb == 0 {
                String::new()
            } else if let Some(d) = self.vram.place(j.spec.vram_gb) {
                d
            } else if j.spec.cpu_fallback {
                CPU.to_string()
            } else {
                if miss != Miss::BudgetStopped {
                    miss = Miss::VramHeld;
                }
                continue;
            };
            pick = Some((id, device));
            break;
        }
        for b in blocked {
            if let Some(a) = self.agents.get_mut(&b) {
                a.budget_blocked += 1;
            }
        }
        let Some((id, device)) = pick else {
            if miss == Miss::BudgetStopped {
                self.stats.budget_refusals += 1;
            }
            return Err(miss);
        };
        let j = &self.jobs[&id];
        let ms = if lease_ms == 0 {
            j.spec.lease_ms
        } else {
            lease_ms
        };
        Ok(Record::Lease {
            id,
            token: self.next_token,
            worker: worker.to_string(),
            deadline: now.saturating_add(ms.max(1)),
            reserved: j.spec.cost,
            device,
        })
    }

    fn current(&mut self, id: u64, token: u64) -> Result<&Job, Refusal> {
        match self.jobs.get(&id) {
            None => {
                self.stats.stale_acks += 1;
                Err(Refusal::NotFound)
            }
            Some(j) if j.state != JobState::Leased || j.token != token => {
                self.stats.stale_acks += 1;
                Err(Refusal::Stale)
            }
            Some(j) => Ok(j),
        }
    }

    /// Complete a lease. `actual` is the spend the worker reports; it is
    /// clamped to the reservation so the cap can never be crossed.
    pub fn ack(&mut self, id: u64, token: u64, actual: Option<u64>) -> Result<Record, Refusal> {
        let reserved = self.current(id, token)?.reserved;
        let charge = match actual {
            Some(a) if a > reserved => {
                self.stats.charge_clamped += 1;
                reserved
            }
            Some(a) => a,
            None => reserved,
        };
        Ok(Record::Ack { id, token, charge })
    }

    /// Fail a lease: retry after backoff, or dead-letter after `max_attempts`.
    pub fn nack(
        &mut self,
        id: u64,
        token: u64,
        actual: Option<u64>,
        error: &str,
        now: u64,
    ) -> Result<Record, Refusal> {
        let j = self.current(id, token)?.clone();
        let charge = actual.map_or(j.reserved, |a| a.min(j.reserved));
        Ok(self.fail_record(&j, charge, now, false, error))
    }

    fn fail_record(&self, j: &Job, charge: u64, now: u64, expired: bool, error: &str) -> Record {
        let retry_at = if j.attempts >= j.spec.max_attempts {
            None
        } else {
            Some(now.saturating_add(self.backoff(j.attempts)))
        };
        Record::Fail {
            id: j.id,
            token: j.token,
            charge,
            retry_at,
            expired,
            error: error.chars().take(256).collect(),
        }
    }

    /// Push a lease's visibility timeout out (worker heartbeat).
    pub fn extend(
        &mut self,
        id: u64,
        token: u64,
        now: u64,
        lease_ms: u64,
    ) -> Result<Record, Refusal> {
        let j = self.current(id, token)?;
        let ms = if lease_ms == 0 {
            j.spec.lease_ms
        } else {
            lease_ms
        };
        Ok(Record::Extend {
            id,
            token,
            deadline: now.saturating_add(ms.max(1)),
        })
    }

    /// Fail every lease whose visibility timeout has passed. A worker that
    /// died mid-job is assumed to have spent its whole reservation.
    pub fn expire_due(&self, now: u64) -> Vec<Record> {
        self.leased
            .iter()
            .take_while(|(d, _)| *d <= now)
            .filter_map(|(_, id)| self.jobs.get(id))
            .map(|j| {
                let err = format!("lease expired (worker {})", j.worker);
                self.fail_record(j, j.reserved, now, true, &err)
            })
            .collect()
    }

    pub fn redrive(&mut self, id: u64, now: u64) -> Result<Record, Refusal> {
        match self.jobs.get(&id) {
            Some(j) if j.state == JobState::Dead => Ok(Record::Redrive { id, now }),
            Some(_) => Err(Refusal::BadRequest(
                "job is not in the dead-letter queue".into(),
            )),
            None => Err(Refusal::NotFound),
        }
    }

    // ------------------------------------------------------------ apply

    fn agent(&mut self, name: &str) -> &mut Agent {
        if !self.agents.contains_key(name) {
            self.agents.insert(name.to_string(), Agent::default());
        }
        self.agents.get_mut(name).expect("inserted")
    }

    fn index(&mut self, j: &Job) {
        match j.state {
            JobState::Ready => {
                self.ready
                    .entry(j.spec.queue.clone())
                    .or_default()
                    .insert((Reverse(j.spec.priority), j.id));
            }
            JobState::Delayed => {
                self.delayed.insert((j.ready_at, j.id));
            }
            JobState::Leased => {
                self.leased.insert((j.deadline, j.id));
            }
            JobState::Dead => {}
        }
    }

    fn unindex(&mut self, j: &Job) {
        match j.state {
            JobState::Ready | JobState::Delayed => {
                if let Some(s) = self.ready.get_mut(&j.spec.queue) {
                    s.remove(&(Reverse(j.spec.priority), j.id));
                }
                self.delayed.remove(&(j.ready_at, j.id));
            }
            JobState::Leased => {
                self.leased.remove(&(j.deadline, j.id));
            }
            JobState::Dead => {}
        }
    }

    fn release(&mut self, agent: &str, reserved: u64, charge: u64) {
        let a = self.agent(agent);
        a.inflight = a.inflight.saturating_sub(1);
        a.budget.reserved = a.budget.reserved.saturating_sub(reserved);
        a.budget.spent = a.budget.spent.saturating_add(charge);
        self.global.reserved = self.global.reserved.saturating_sub(reserved);
        self.global.spent = self.global.spent.saturating_add(charge);
    }

    /// Apply one durable record. Unknown ids are ignored, so a replay is
    /// always safe.
    pub fn apply(&mut self, r: &Record) {
        match r {
            Record::Enqueue { id, now, spec } => {
                let j = Job {
                    id: *id,
                    spec: spec.clone(),
                    created_ms: *now,
                    state: JobState::Ready,
                    attempts: 0,
                    ready_at: *now,
                    token: 0,
                    worker: String::new(),
                    deadline: 0,
                    reserved: 0,
                    last_error: String::new(),
                    device: String::new(),
                };
                self.index(&j);
                self.agent(&spec.agent).pending += 1;
                self.jobs.insert(*id, j);
                self.next_id = self.next_id.max(id + 1);
                self.stats.enqueued += 1;
            }
            Record::Lease {
                id,
                token,
                worker,
                deadline,
                reserved,
                device,
            } => {
                let Some(mut j) = self.jobs.get(id).cloned() else {
                    return;
                };
                if j.state != JobState::Ready && j.state != JobState::Delayed {
                    return;
                }
                self.unindex(&j);
                j.state = JobState::Leased;
                j.attempts += 1;
                j.token = *token;
                j.worker = worker.clone();
                j.deadline = *deadline;
                j.reserved = *reserved;
                j.device = device.clone();
                if j.device == CPU {
                    self.stats.vram_offloaded += 1;
                }
                self.hold_vram(&j);
                self.index(&j);
                let a = self.agent(&j.spec.agent);
                a.pending = a.pending.saturating_sub(1);
                a.inflight += 1;
                a.budget.reserved += reserved;
                self.global.reserved += reserved;
                self.stats.delivered += 1;
                if j.attempts > 1 {
                    self.stats.redelivered += 1;
                }
                self.jobs.insert(*id, j);
                self.next_token = self.next_token.max(token + 1);
            }
            Record::Extend {
                id,
                token,
                deadline,
            } => {
                let Some(mut j) = self.jobs.get(id).cloned() else {
                    return;
                };
                if j.state != JobState::Leased || j.token != *token {
                    return;
                }
                self.unindex(&j);
                j.deadline = *deadline;
                self.index(&j);
                self.jobs.insert(*id, j);
            }
            Record::Ack { id, token, charge } => {
                let Some(j) = self.jobs.get(id).cloned() else {
                    return;
                };
                if j.state != JobState::Leased || j.token != *token {
                    return;
                }
                self.unindex(&j);
                self.release(&j.spec.agent, j.reserved, *charge);
                self.drop_vram(&j);
                self.jobs.remove(id);
                self.stats.acked += 1;
            }
            Record::Fail {
                id,
                token,
                charge,
                retry_at,
                expired,
                error,
            } => {
                let Some(mut j) = self.jobs.get(id).cloned() else {
                    return;
                };
                if j.state != JobState::Leased || j.token != *token {
                    return;
                }
                self.unindex(&j);
                self.release(&j.spec.agent, j.reserved, *charge);
                self.drop_vram(&j);
                j.device.clear();
                j.reserved = 0;
                j.last_error = error.clone();
                if *expired {
                    self.stats.expired += 1;
                } else {
                    self.stats.nacked += 1;
                }
                match retry_at {
                    Some(at) => {
                        j.state = JobState::Delayed;
                        j.ready_at = *at;
                        self.agent(&j.spec.agent).pending += 1;
                    }
                    None => {
                        j.state = JobState::Dead;
                        self.stats.dead += 1;
                    }
                }
                self.index(&j);
                self.jobs.insert(*id, j);
            }
            Record::Redrive { id, now } => {
                let Some(mut j) = self.jobs.get(id).cloned() else {
                    return;
                };
                if j.state != JobState::Dead {
                    return;
                }
                j.state = JobState::Ready;
                j.attempts = 0;
                j.ready_at = *now;
                self.index(&j);
                self.agent(&j.spec.agent).pending += 1;
                self.jobs.insert(*id, j);
                self.stats.redriven += 1;
            }
            Record::Cap { scope, cap } => {
                if scope == GLOBAL {
                    self.global.cap = *cap;
                } else {
                    self.agent(scope).budget.cap = *cap;
                }
            }
            Record::Quota {
                agent,
                max_pending,
                max_inflight,
            } => {
                let q = Some((*max_pending, *max_inflight));
                if agent == GLOBAL {
                    self.default_quota = q;
                } else {
                    self.agent(agent).quota = q;
                }
            }
            Record::Device(d) => self.vram.upsert(d.clone()),
            Record::DeviceRemove { id } => {
                self.vram.remove(id);
            }
            Record::Snapshot(j) => {
                let j = (**j).clone();
                self.index(&j);
                let a = self.agent(&j.spec.agent);
                match j.state {
                    JobState::Ready | JobState::Delayed => a.pending += 1,
                    JobState::Leased => {
                        a.inflight += 1;
                        a.budget.reserved += j.reserved;
                    }
                    JobState::Dead => {}
                }
                if j.state == JobState::Leased {
                    self.global.reserved += j.reserved;
                    self.hold_vram(&j);
                }
                self.next_id = self.next_id.max(j.id + 1);
                self.next_token = self.next_token.max(j.token + 1);
                self.jobs.insert(j.id, j);
            }
            Record::Spent { scope, spent } => {
                if scope == GLOBAL {
                    self.global.spent = *spent;
                } else {
                    self.agent(scope).budget.spent = *spent;
                }
            }
            Record::Next { id, token } => {
                self.next_id = self.next_id.max(*id);
                self.next_token = self.next_token.max(*token);
            }
            Record::Counters(s) => self.stats = s.clone(),
            Record::Tally(t) => {
                self.stats.budget_refusals = t.budget_refusals;
                self.stats.quota_refusals = t.quota_refusals;
                self.stats.stale_acks = t.stale_acks;
                self.stats.charge_clamped = t.charge_clamped;
                self.stats.backpressure_refusals = t.backpressure_refusals;
                for (name, n) in &t.blocked {
                    self.agent(name).budget_blocked = *n;
                }
            }
        }
    }

    /// The refusal counters as they stand (see [`Record::Tally`]).
    pub fn tally(&self) -> Tally {
        Tally {
            budget_refusals: self.stats.budget_refusals,
            quota_refusals: self.stats.quota_refusals,
            stale_acks: self.stats.stale_acks,
            charge_clamped: self.stats.charge_clamped,
            backpressure_refusals: self.stats.backpressure_refusals,
            blocked: self
                .agents
                .iter()
                .filter(|(_, a)| a.budget_blocked > 0)
                .map(|(n, a)| (n.clone(), a.budget_blocked))
                .collect(),
        }
    }

    /// The minimal record set that rebuilds the current state (compaction).
    pub fn snapshot(&self) -> Vec<Record> {
        let mut out = vec![
            Record::Next {
                id: self.next_id,
                token: self.next_token,
            },
            Record::Counters(self.stats.clone()),
        ];
        out.push(Record::Tally(self.tally()));
        if let Some((p, f)) = self.default_quota {
            out.push(Record::Quota {
                agent: GLOBAL.into(),
                max_pending: p,
                max_inflight: f,
            });
        }
        out.push(Record::Cap {
            scope: GLOBAL.into(),
            cap: self.global.cap,
        });
        out.push(Record::Spent {
            scope: GLOBAL.into(),
            spent: self.global.spent,
        });
        for (name, a) in &self.agents {
            if a.budget.cap.is_some() {
                out.push(Record::Cap {
                    scope: name.clone(),
                    cap: a.budget.cap,
                });
            }
            if a.budget.spent > 0 {
                out.push(Record::Spent {
                    scope: name.clone(),
                    spent: a.budget.spent,
                });
            }
            if let Some((p, f)) = a.quota {
                out.push(Record::Quota {
                    agent: name.clone(),
                    max_pending: p,
                    max_inflight: f,
                });
            }
        }
        for d in self.vram.devices() {
            out.push(Record::Device(d.clone()));
        }
        let mut ids: Vec<&u64> = self.jobs.keys().collect();
        ids.sort();
        for id in ids {
            out.push(Record::Snapshot(Box::new(self.jobs[id].clone())));
        }
        out
    }

    fn hold_vram(&mut self, j: &Job) {
        if !j.device.is_empty() && j.device != CPU {
            self.vram.hold(&j.device, j.spec.vram_gb);
        }
    }

    fn drop_vram(&mut self, j: &Job) {
        if !j.device.is_empty() && j.device != CPU {
            self.vram.unhold(&j.device, j.spec.vram_gb);
        }
    }

    fn dead_count(&self) -> usize {
        self.depth().3
    }

    /// Number of live (not acked) jobs.
    pub fn live_jobs(&self) -> usize {
        self.jobs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(agent: &str, prio: i32, cost: u64) -> JobSpec {
        JobSpec {
            queue: "q".into(),
            agent: agent.into(),
            priority: prio,
            cost,
            lease_ms: 1000,
            max_attempts: 3,
            ..JobSpec::default()
        }
    }

    fn put(e: &mut Engine, s: JobSpec, now: u64) -> u64 {
        let r = e.enqueue(s, now).unwrap();
        e.apply(&r);
        match r {
            Record::Enqueue { id, .. } => id,
            _ => unreachable!(),
        }
    }

    fn take(e: &mut Engine, now: u64) -> Result<(u64, u64), Miss> {
        let r = e.lease("q", "w", now, 0)?;
        e.apply(&r);
        match r {
            Record::Lease { id, token, .. } => Ok((id, token)),
            _ => unreachable!(),
        }
    }

    fn eng() -> Engine {
        Engine::new(Config {
            backoff_base_ms: 100,
            backoff_max_ms: 1000,
            ..Config::default()
        })
    }

    #[test]
    fn priority_then_fifo() {
        let mut e = eng();
        let a = put(&mut e, spec("a", 0, 0), 0);
        let b = put(&mut e, spec("a", 5, 0), 0);
        let c = put(&mut e, spec("a", 5, 0), 0);
        assert_eq!(take(&mut e, 1).unwrap().0, b);
        assert_eq!(take(&mut e, 1).unwrap().0, c);
        assert_eq!(take(&mut e, 1).unwrap().0, a);
        assert_eq!(take(&mut e, 1), Err(Miss::Empty));
    }

    #[test]
    fn lease_expiry_redelivers_once_and_fences_old_token() {
        let mut e = eng();
        let id = put(&mut e, spec("a", 0, 0), 0);
        let (_, t1) = take(&mut e, 0).unwrap();
        assert!(e.expire_due(999).is_empty());
        let ex = e.expire_due(1000);
        assert_eq!(ex.len(), 1);
        e.apply(&ex[0]);
        assert_eq!(e.job(id).unwrap().state, JobState::Delayed);
        // backoff 100 ms after 1 attempt
        assert_eq!(take(&mut e, 1099), Err(Miss::Empty));
        let (id2, t2) = take(&mut e, 1100).unwrap();
        assert_eq!(id2, id);
        assert_ne!(t1, t2);
        assert_eq!(e.ack(id, t1, None), Err(Refusal::Stale));
        let r = e.ack(id, t2, None).unwrap();
        e.apply(&r);
        assert!(e.job(id).is_none());
        assert_eq!(e.stats.delivered, 2);
        assert_eq!(e.stats.redelivered, 1);
        assert_eq!(e.stats.acked, 1);
        // a second ack of the same lease is refused
        assert_eq!(e.ack(id, t2, None), Err(Refusal::NotFound));
    }

    #[test]
    fn retries_backoff_then_dlq_and_redrive() {
        let mut e = eng();
        let id = put(&mut e, spec("a", 0, 0), 0);
        let mut now = 0;
        for attempt in 1..=3u32 {
            let (_, t) = take(&mut e, now).unwrap();
            let r = e.nack(id, t, None, "boom", now).unwrap();
            e.apply(&r);
            if attempt < 3 {
                let want = 100u64 << (attempt - 1);
                assert_eq!(e.job(id).unwrap().ready_at, now + want);
                now += want;
            }
        }
        assert_eq!(e.job(id).unwrap().state, JobState::Dead);
        assert_eq!(e.dead_jobs().len(), 1);
        assert_eq!(take(&mut e, now + 10_000), Err(Miss::Empty));
        let r = e.redrive(id, now).unwrap();
        e.apply(&r);
        assert_eq!(take(&mut e, now).unwrap().0, id);
    }

    #[test]
    fn quotas_pending_and_inflight() {
        let mut e = eng();
        e.apply(&Record::Quota {
            agent: "a".into(),
            max_pending: 2,
            max_inflight: 1,
        });
        put(&mut e, spec("a", 0, 0), 0);
        put(&mut e, spec("a", 0, 0), 0);
        assert!(matches!(
            e.enqueue(spec("a", 0, 0), 0),
            Err(Refusal::Quota(_))
        ));
        put(&mut e, spec("b", -1, 0), 0);
        let (first, _) = take(&mut e, 0).unwrap();
        assert_eq!(e.job(first).unwrap().spec.agent, "a");
        // a is at max_inflight 1: the next lease skips to b's job
        let (second, _) = take(&mut e, 0).unwrap();
        assert_eq!(e.job(second).unwrap().spec.agent, "b");
        assert_eq!(take(&mut e, 0), Err(Miss::QuotaHeld));
    }

    #[test]
    fn budget_hard_stop_never_crosses_cap() {
        let mut e = eng();
        e.apply(&Record::Cap {
            scope: "a".into(),
            cap: Some(25),
        });
        for _ in 0..5 {
            put(&mut e, spec("a", 0, 10), 0);
        }
        let (i1, t1) = take(&mut e, 0).unwrap();
        let (_i2, _t2) = take(&mut e, 0).unwrap();
        // 20 reserved, 10 more would make 30 > 25
        assert_eq!(take(&mut e, 0), Err(Miss::BudgetStopped));
        // worker over-reports: clamped to the reservation
        let r = e.ack(i1, t1, Some(1_000)).unwrap();
        assert_eq!(
            r,
            Record::Ack {
                id: i1,
                token: t1,
                charge: 10
            }
        );
        e.apply(&r);
        assert_eq!(e.stats.charge_clamped, 1);
        assert_eq!(take(&mut e, 0), Err(Miss::BudgetStopped));
        let b = &e.agents()["a"].budget;
        assert!(b.spent + b.reserved <= 25);
        // global cap also enforced, at the door
        e.apply(&Record::Cap {
            scope: GLOBAL.into(),
            cap: Some(5),
        });
        assert!(matches!(
            e.enqueue(spec("z", 0, 10), 0),
            Err(Refusal::Budget(_))
        ));
    }

    #[test]
    fn money_kinds_refused() {
        let mut e = eng();
        let mut s = spec("a", 0, 0);
        s.kind = "revolut-payout".into();
        assert_eq!(e.enqueue(s, 0), Err(Refusal::Money));
    }

    #[test]
    fn replay_and_snapshot_rebuild_the_same_state() {
        let mut e = eng();
        let mut log = Vec::new();
        e.apply(&Record::Cap {
            scope: "a".into(),
            cap: Some(1000),
        });
        log.push(Record::Cap {
            scope: "a".into(),
            cap: Some(1000),
        });
        for i in 0..20 {
            let r = e.enqueue(spec("a", i % 3, 7), i as u64).unwrap();
            e.apply(&r);
            log.push(r);
        }
        for n in 0..8u64 {
            let r = e.lease("q", "w", 100, 0).unwrap();
            e.apply(&r);
            log.push(r.clone());
            if let Record::Lease { id, token, .. } = r {
                let r2 = if n % 2 == 0 {
                    e.ack(id, token, Some(3)).unwrap()
                } else {
                    e.nack(id, token, None, "x", 100).unwrap()
                };
                e.apply(&r2);
                log.push(r2);
            }
        }
        let mut replay = eng();
        for r in &log {
            replay.apply(r);
        }
        let mut snap = eng();
        for r in e.snapshot() {
            snap.apply(&r);
        }
        for other in [&replay, &snap] {
            assert_eq!(other.depth(), e.depth());
            assert_eq!(other.global(), e.global());
            assert_eq!(other.agents()["a"].budget, e.agents()["a"].budget);
            assert_eq!(other.agents()["a"].pending, e.agents()["a"].pending);
            assert_eq!(other.next_id, e.next_id);
            assert_eq!(other.next_token, e.next_token);
        }
    }

    fn gpu(id: &str, gb: u32) -> Record {
        Record::Device(crate::vram::Device {
            id: id.into(),
            kind: crate::vram::DeviceKind::Gpu,
            vram_gb: gb,
            provider: String::new(),
            cents_per_hour: 0,
            state: crate::vram::DeviceState::Approved,
            healthy: true,
        })
    }

    fn vspec(gb: u32, fallback: bool) -> JobSpec {
        JobSpec {
            vram_gb: gb,
            cpu_fallback: fallback,
            ..spec("a", 0, 0)
        }
    }

    #[test]
    fn vram_routing_holds_until_ack_then_frees() {
        let mut e = eng();
        e.apply(&gpu("g24", 24));
        let a = put(&mut e, vspec(16, false), 0);
        let b = put(&mut e, vspec(16, false), 0);
        let c = put(&mut e, vspec(16, true), 0);
        let (ia, ta) = take(&mut e, 0).unwrap();
        assert_eq!(ia, a);
        assert_eq!(e.job(a).unwrap().device, "g24");
        assert_eq!(e.vram.used("g24"), 16);
        // b waits for VRAM; c (cpu_fallback) is offloaded instead
        let (ic, _) = take(&mut e, 0).unwrap();
        assert_eq!(ic, c);
        assert_eq!(e.job(c).unwrap().device, CPU);
        assert_eq!(e.stats.vram_offloaded, 1);
        assert_eq!(take(&mut e, 0), Err(Miss::VramHeld));
        let r = e.ack(ia, ta, None).unwrap();
        e.apply(&r);
        assert_eq!(e.vram.used("g24"), 0);
        assert_eq!(take(&mut e, 0).unwrap().0, b);
        // lease expiry frees VRAM too
        let ex = e.expire_due(10_000);
        assert_eq!(ex.len(), 2);
        for r in &ex {
            e.apply(r);
        }
        assert_eq!(e.vram.used("g24"), 0);
    }

    #[test]
    fn vram_never_fits_is_refused_unless_cpu_fallback() {
        let mut e = eng();
        assert!(matches!(
            e.enqueue(vspec(8, false), 0),
            Err(Refusal::Vram(_))
        ));
        assert!(e.enqueue(vspec(8, true), 0).is_ok());
        e.apply(&gpu("g24", 24));
        assert!(matches!(
            e.enqueue(vspec(48, false), 0),
            Err(Refusal::Vram(_))
        ));
        assert!(e.enqueue(vspec(24, false), 0).is_ok());
    }

    #[test]
    fn backpressure_at_max_depth() {
        let mut e = Engine::new(Config {
            max_depth: 2,
            ..Config::default()
        });
        put(&mut e, spec("a", 0, 0), 0);
        put(&mut e, spec("b", 0, 0), 0);
        let r = e.enqueue(spec("c", 0, 0), 0);
        assert!(matches!(r, Err(Refusal::Full(_))));
        assert_eq!(r.unwrap_err().status(), 503);
        assert_eq!(e.stats.backpressure_refusals, 1);
    }

    #[test]
    fn snapshot_rebuilds_vram_holds_and_counters() {
        let mut e = eng();
        e.apply(&gpu("g80", 80));
        put(&mut e, vspec(30, false), 0);
        put(&mut e, vspec(30, false), 0);
        take(&mut e, 0).unwrap();
        take(&mut e, 0).unwrap();
        let mut s = eng();
        for r in e.snapshot() {
            s.apply(&r);
        }
        assert_eq!(s.vram.used("g80"), 60);
        assert_eq!(s.stats, e.stats);
        assert_eq!(s.vram.summary(), e.vram.summary());
    }

    #[test]
    fn records_roundtrip_through_json() {
        let mut e = eng();
        e.apply(&gpu("g", 8));
        let mut recs = e.snapshot();
        recs.push(e.enqueue(vspec(4, false), 5).unwrap());
        for r in recs {
            let line = crate::store::encode(&r);
            assert_eq!(crate::store::decode(line.trim_end()), Some(r));
        }
    }
}
