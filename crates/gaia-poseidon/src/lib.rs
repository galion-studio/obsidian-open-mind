// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! POSEIDON: water, here compute resources. It owns the VRAM pool (GPU,
//! remote pool, offload), CPU/RAM/disk quotas, bounded queues with
//! backpressure and budget **caps**. It tracks caps only: anything that asks
//! for real money movement (payments, transfers, Revolut) is refused, because
//! that access is not approved. `env.water` is the free resource headroom.
//!
//! Two backends:
//!
//! * the **stand-in** ([`Poseidon::new`]): the original in-memory simulation,
//!   used by the deterministic scenarios and the unit tests;
//! * the **real adapter** ([`Poseidon::with_queue`], module [`queue`]): every
//!   directive is forwarded to a running `openmind-queue` service. Budget is
//!   read-only through it and it never leases or acks, so it never spends.
//!
//! Which Gaia function owns these systems is decided in one place,
//! `gaia_core::FunctionId::COMPUTE` (POSEIDON today; see
//! `docs/gaia-poseidon.md` for the HEPHAESTUS question).

pub mod queue;

pub use queue::{QueueAdapter, QueueConfig};

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Outcome, Report, SubordinateFunction,
    System, Verb,
};
use std::collections::VecDeque;

/// Queue slots.
pub const QUEUE_CAP: usize = 32;

/// Words that mean real money movement. Never implemented.
const MONEY_WORDS: &[&str] = &[
    "revolut", "payment", "transfer", "card", "iban", "withdraw", "pay ",
];

#[derive(Debug)]
pub struct Poseidon {
    /// The real backend. `None` means the stand-in fields below are used.
    backend: Option<QueueAdapter>,
    queue: VecDeque<u64>,
    rejected: u64,
    vram_local_gb: u64,
    vram_pool_gb: u64,
    vram_used_gb: u64,
    /// Cap in cents, per budget name.
    caps: Vec<(String, u64, u64)>,
}

impl Default for Poseidon {
    fn default() -> Self {
        Poseidon {
            backend: None,
            queue: VecDeque::new(),
            rejected: 0,
            vram_local_gb: 0,
            vram_pool_gb: 48,
            vram_used_gb: 0,
            caps: vec![("hermes-boss".to_string(), 87, 500)],
        }
    }
}

impl Poseidon {
    /// The stand-in (simulation only).
    pub fn new() -> Poseidon {
        Poseidon::default()
    }

    /// The real adapter: forward every directive to `openmind-queue`.
    pub fn with_queue(cfg: QueueConfig) -> Poseidon {
        Poseidon {
            backend: Some(QueueAdapter::new(cfg)),
            ..Poseidon::default()
        }
    }

    /// `"openmind-queue"` or `"stand-in"`.
    pub fn backend_name(&self) -> &'static str {
        if self.backend.is_some() {
            "openmind-queue"
        } else {
            "stand-in"
        }
    }

    /// The real adapter, if this POSEIDON uses one.
    pub fn adapter(&self) -> Option<&QueueAdapter> {
        self.backend.as_ref()
    }

    /// Per-job quota when `jobs` share `total_mb` of RAM, keeping 10% spare.
    pub fn ram_quota_mb(total_mb: u64, jobs: u64) -> u64 {
        total_mb * 9 / 10 / jobs.max(1)
    }

    pub fn is_money_request(payload: &str) -> bool {
        let p = payload.to_ascii_lowercase();
        MONEY_WORDS.iter().any(|w| p.contains(w))
    }

    fn headroom(&self) -> u8 {
        let q = 100 - (self.queue.len() * 100 / QUEUE_CAP) as u64;
        let v = (self.vram_used_gb * 100)
            .checked_div(self.vram_pool_gb)
            .map_or(100, |used| 100 - used.min(100));
        q.min(v) as u8
    }
}

impl SubordinateFunction for Poseidon {
    fn id(&self) -> FunctionId {
        FunctionId::Poseidon
    }

    fn systems(&self) -> Vec<System> {
        // Follows the single config point `FunctionId::COMPUTE`: if compute is
        // moved to another function, POSEIDON declares the four systems while
        // owning none and GAIA refuses to register it (a loud failure, never a
        // silent split).
        System::COMPUTE.to_vec()
    }

    fn health(&self) -> Health {
        if let Some(q) = &self.backend {
            return q.health();
        }
        Health::ok(100 - self.headroom())
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        if let Some(q) = self.backend.as_mut() {
            let r = q.handle(d);
            ctx.env.water = ctx.env.water.min(100 - q.load().min(100));
            return r;
        }
        if Poseidon::is_money_request(&d.payload) {
            return Report::for_directive(d, Outcome::Refused)
                .summary("real-money access is not approved; POSEIDON tracks caps only");
        }
        let done = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        let status = d.verb == Verb::Status;
        let r = match d.target {
            System::VramPool => {
                let need: u64 = d
                    .payload
                    .trim()
                    .parse()
                    .unwrap_or_else(|_| scaled(16, d.intensity));
                let free = self.vram_pool_gb - self.vram_used_gb;
                if status {
                    done(format!("{free} GB free of {} GB pool", self.vram_pool_gb))
                } else if need <= self.vram_local_gb {
                    done(format!("{need} GB fits local VRAM"))
                } else if need <= free {
                    self.vram_used_gb += need;
                    done(format!(
                        "{need} GB reserved from the pool (simulated, no spend)"
                    ))
                    .metric("reserved_gb", need as i64)
                } else {
                    Report::for_directive(d, Outcome::Refused)
                        .summary(format!(
                            "{need} GB needed, {free} GB free: offload to CPU instead"
                        ))
                        .metric("free_gb", free as i64)
                }
            }
            System::Quotas => {
                let jobs = 7;
                let quota = Poseidon::ram_quota_mb(15 * 1024, jobs);
                done(format!(
                    "{jobs} jobs share RAM: {quota} MB each, nice 19, -j 2"
                ))
                .metric("ram_mb_per_job", quota as i64)
                .metric("cargo_jobs", 2)
            }
            System::Queues => {
                let n = if status { 0 } else { scaled(40, d.intensity) };
                let mut accepted = 0;
                for i in 0..n {
                    if self.queue.len() < QUEUE_CAP {
                        self.queue.push_back(i);
                        accepted += 1;
                    } else {
                        self.rejected += 1;
                    }
                }
                done(format!("{accepted}/{n} queued, depth {}", self.queue.len()))
                    .metric("accepted", accepted as i64)
                    .metric("backpressure", (n - accepted) as i64)
                    .metric("depth", self.queue.len() as i64)
            }
            System::BudgetCaps => {
                let mut r = done(format!("{} budget cap(s), caps only", self.caps.len()));
                for (name, spent, cap) in &self.caps {
                    r = r.metric(
                        &format!("{name}_left_cents"),
                        cap.saturating_sub(*spent) as i64,
                    );
                }
                r
            }
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not a POSEIDON system")),
        };
        ctx.env.water = ctx.env.water.min(self.headroom());
        r
    }

    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if let Some(q) = self.backend.as_mut() {
            // One read-only GET per tick keeps health and water honest.
            q.refresh();
            let headroom = 100 - q.load().min(100);
            if !ctx.env.swarm_active {
                ctx.env.water = ctx.env.water.saturating_add(4).min(headroom);
            }
            return None;
        }
        // Drain the queue a little every tick; recover water after the swarm.
        for _ in 0..4 {
            self.queue.pop_front();
        }
        self.vram_used_gb = self.vram_used_gb.saturating_sub(4);
        if !ctx.env.swarm_active {
            ctx.env.water = ctx.env.water.saturating_add(4).min(self.headroom());
        }
        None
    }

    fn reset(&mut self) {
        let backend = self.backend.take().map(|mut q| {
            q.reset();
            q
        });
        *self = Poseidon {
            backend,
            ..Poseidon::default()
        };
    }

    fn state_json(&self) -> Json {
        if let Some(q) = &self.backend {
            return q.state_json();
        }
        Json::obj()
            .with("queue_depth", self.queue.len())
            .with("queue_cap", QUEUE_CAP)
            .with("rejected", self.rejected)
            .with("vram_used_gb", self.vram_used_gb)
            .with("vram_pool_gb", self.vram_pool_gb)
            .with("headroom", self.headroom())
            .with("backend", "stand-in")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    fn run(p: &mut Poseidon, d: Directive) -> Report {
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        p.handle(&d, &mut ctx)
    }

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Poseidon::new().systems(), FunctionId::Poseidon.systems());
    }

    #[test]
    fn money_is_refused() {
        let mut p = Poseidon::new();
        for s in p.systems() {
            let r = run(
                &mut p,
                Directive::new(1, s, Verb::Run).payload("Revolut transfer 5 EUR"),
            );
            assert_eq!(r.outcome, Outcome::Refused);
        }
    }

    #[test]
    fn queue_backpressure() {
        let mut p = Poseidon::new();
        let r = run(
            &mut p,
            Directive::new(1, System::Queues, Verb::Run).intensity(100),
        );
        assert_eq!(r.metric_value("accepted"), Some(QUEUE_CAP as i64));
        assert_eq!(r.metric_value("backpressure"), Some(8));
    }

    #[test]
    fn vram_pool_reserves_then_refuses() {
        let mut p = Poseidon::new();
        assert_eq!(
            run(
                &mut p,
                Directive::new(1, System::VramPool, Verb::Run).payload("40")
            )
            .outcome,
            Outcome::Done
        );
        assert_eq!(
            run(
                &mut p,
                Directive::new(2, System::VramPool, Verb::Run).payload("40")
            )
            .outcome,
            Outcome::Refused
        );
        assert_eq!(Poseidon::ram_quota_mb(15 * 1024, 7), 1974);
    }
}
