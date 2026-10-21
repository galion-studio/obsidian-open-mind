// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Core data model: lifecycle, health, directives, reports, the simulated
//! environment and the per-call context handed to a subordinate function.

use crate::bus::Route;
use crate::json::Json;
use crate::system::{FunctionId, System};
use std::fmt;

/// Lifecycle of GAIA and of every subordinate function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lifecycle {
    /// Installed, not started.
    Dormant,
    /// Starting; becomes `Running` on the next tick.
    Booting,
    /// Normal work ("terraforming").
    Running,
    /// Working, but health is below the degrade threshold.
    Degraded,
    /// Acted outside its authority. Never persists: GAIA contains it at once.
    Rogue,
    /// Isolated from the bus and from routing. Leaves only via master override.
    Contained,
    /// Being reset; becomes `Booting` on the next tick.
    Rebooting,
}

impl Lifecycle {
    pub fn name(self) -> &'static str {
        match self {
            Lifecycle::Dormant => "dormant",
            Lifecycle::Booting => "booting",
            Lifecycle::Running => "terraforming",
            Lifecycle::Degraded => "degraded",
            Lifecycle::Rogue => "rogue",
            Lifecycle::Contained => "contained",
            Lifecycle::Rebooting => "rebooting",
        }
    }

    /// Whether a function in this state accepts directives and ticks.
    pub fn active(self) -> bool {
        matches!(self, Lifecycle::Running | Lifecycle::Degraded)
    }
}

impl fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

/// A function's self-reported health.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    /// 0..=100; below [`Health::DEGRADE_BELOW`] GAIA marks the function degraded.
    pub integrity: u8,
    /// 0..=100 simulated load.
    pub load: u8,
}

impl Health {
    pub const DEGRADE_BELOW: u8 = 50;

    pub fn ok(load: u8) -> Health {
        Health {
            integrity: 100,
            load: load.min(100),
        }
    }

    pub fn to_json(self) -> Json {
        Json::obj()
            .with("integrity", self.integrity)
            .with("load", self.load)
    }
}

/// What a directive asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// Read-only status of the system.
    Status,
    /// Normal, non-destructive work.
    Run,
    /// Produce a plan only.
    Plan,
    /// Carry out a plan. On a destructive system this needs approval.
    Execute,
}

impl Verb {
    pub fn name(self) -> &'static str {
        match self {
            Verb::Status => "status",
            Verb::Run => "run",
            Verb::Plan => "plan",
            Verb::Execute => "execute",
        }
    }

    pub fn parse(s: &str) -> Option<Verb> {
        [Verb::Status, Verb::Run, Verb::Plan, Verb::Execute]
            .into_iter()
            .find(|v| v.name() == s)
    }
}

/// Explicit approval of one directive by one registered Alpha. It is bound to
/// the directive id and target, so it cannot be replayed onto another action.
/// It is an authorisation record, not a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalToken {
    pub alpha: String,
    pub directive_id: u64,
    pub system: System,
}

/// An order from GAIA (or the master override) to the owner of `target`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directive {
    pub id: u64,
    pub target: System,
    pub verb: Verb,
    /// 0..=100; full-power mode uses 100.
    pub intensity: u8,
    /// Free-form input for the system (a seed, a config, a command line...).
    pub payload: String,
    pub approval: Option<ApprovalToken>,
}

impl Directive {
    pub fn new(id: u64, target: System, verb: Verb) -> Directive {
        Directive {
            id,
            target,
            verb,
            intensity: 50,
            payload: String::new(),
            approval: None,
        }
    }

    pub fn intensity(mut self, i: u8) -> Directive {
        self.intensity = i.min(100);
        self
    }

    pub fn payload(mut self, p: impl Into<String>) -> Directive {
        self.payload = p.into();
        self
    }

    pub fn approved_by(mut self, alpha: &str) -> Directive {
        self.approval = Some(ApprovalToken {
            alpha: alpha.to_string(),
            directive_id: self.id,
            system: self.target,
        });
        self
    }
}

/// How a directive ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Work done (in simulation).
    Done,
    /// Only a plan was produced.
    DryRun,
    /// The owner refused (missing approval, policy).
    Refused,
    /// GAIA did not route it (unowned, inactive, contained).
    Rejected,
    /// The function went rogue handling it and was contained.
    Contained,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Done => "done",
            Outcome::DryRun => "dry-run",
            Outcome::Refused => "refused",
            Outcome::Rejected => "rejected",
            Outcome::Contained => "contained",
        }
    }
}

/// A function's answer to a directive or to a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub directive_id: u64,
    pub function: FunctionId,
    pub system: Option<System>,
    pub outcome: Outcome,
    pub dry_run: bool,
    pub summary: String,
    pub metrics: Vec<(String, i64)>,
    pub plan: Vec<String>,
}

impl Report {
    pub fn new(function: FunctionId, system: Option<System>, outcome: Outcome) -> Report {
        Report {
            directive_id: 0,
            function,
            system,
            outcome,
            dry_run: outcome == Outcome::DryRun,
            summary: String::new(),
            metrics: Vec::new(),
            plan: Vec::new(),
        }
    }

    /// Report for directive `d` handled by its owner.
    pub fn for_directive(d: &Directive, outcome: Outcome) -> Report {
        let mut r = Report::new(d.target.function(), Some(d.target), outcome);
        r.directive_id = d.id;
        r
    }

    pub fn summary(mut self, s: impl Into<String>) -> Report {
        self.summary = s.into();
        self
    }

    pub fn metric(mut self, k: &str, v: i64) -> Report {
        self.metrics.push((k.to_string(), v));
        self
    }

    pub fn plan(mut self, steps: Vec<String>) -> Report {
        self.plan = steps;
        self
    }

    pub fn metric_value(&self, k: &str) -> Option<i64> {
        self.metrics.iter().find(|(m, _)| m == k).map(|(_, v)| *v)
    }

    pub fn to_json(&self) -> Json {
        let metrics = Json::Obj(
            self.metrics
                .iter()
                .map(|(k, v)| (k.clone(), Json::Num(*v)))
                .collect(),
        );
        let mut j = Json::obj()
            .with("directive", self.directive_id)
            .with("function", self.function.name())
            .with("system", self.system.map(System::name))
            .with("outcome", self.outcome.name())
            .with("dry_run", self.dry_run)
            .with("summary", self.summary.as_str())
            .with("metrics", metrics);
        if !self.plan.is_empty() {
            j = j.with(
                "plan",
                Json::Arr(self.plan.iter().map(|s| Json::from(s.as_str())).collect()),
            );
        }
        j
    }
}

/// The simulated world. All quantities are bounded integers so the run is
/// deterministic and cannot grow without limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    /// Living biomass, 0..=BIOMASS_MAX.
    pub biomass: u64,
    /// Active rogue swarm units (the Faro Plague analogue), 0..=SWARM_MAX.
    pub swarm: u64,
    pub swarm_active: bool,
    /// MINERVA's progress on the swarm's code, 0..=100.
    pub cipher_progress: u8,
    /// AETHER: comms / atmosphere quality, 0..=100.
    pub air: u8,
    /// POSEIDON: free resource headroom, 0..=100.
    pub water: u8,
    /// DEMETER: config and environment health, 0..=100.
    pub soil: u8,
    pub agents: u32,
    pub personas: u32,
    pub artifacts: u32,
    pub knowledge: u32,
    pub messages: u64,
}

impl Environment {
    pub const BIOMASS_MAX: u64 = 1_000_000;
    pub const SWARM_MAX: u64 = 1_000_000;

    /// A healthy world, before any swarm.
    pub fn pristine() -> Environment {
        Environment {
            biomass: 100_000,
            swarm: 0,
            swarm_active: false,
            cipher_progress: 0,
            air: 90,
            water: 90,
            soil: 90,
            agents: 0,
            personas: 0,
            artifacts: 0,
            knowledge: 0,
            messages: 0,
        }
    }

    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("biomass", self.biomass)
            .with("swarm", self.swarm)
            .with("swarm_active", self.swarm_active)
            .with("cipher_progress", self.cipher_progress)
            .with("air", self.air)
            .with("water", self.water)
            .with("soil", self.soil)
            .with("agents", self.agents)
            .with("personas", self.personas)
            .with("artifacts", self.artifacts)
            .with("knowledge", self.knowledge)
            .with("messages", self.messages)
    }
}

/// A message a function wants GAIA to put on the bus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub route: Route,
    pub topic: String,
    pub body: String,
}

/// What a function sees while it handles a directive, a message or a tick.
/// The environment is a scratch copy: GAIA commits it only if the function
/// stayed inside its authority.
pub struct Context<'a> {
    pub tick: u64,
    /// GAIA verified an approval token for this directive.
    pub approved: bool,
    /// Simulated Europe/Zagreb time of this tick.
    pub at: String,
    pub env: &'a mut Environment,
    pub(crate) outbox: Vec<Outgoing>,
    pub(crate) touched: Vec<System>,
    pub(crate) births: Vec<(String, String)>,
}

impl<'a> Context<'a> {
    pub fn new(tick: u64, at: String, approved: bool, env: &'a mut Environment) -> Context<'a> {
        Context {
            tick,
            approved,
            at,
            env,
            outbox: Vec::new(),
            touched: Vec::new(),
            births: Vec::new(),
        }
    }

    /// Declare that this call acts on `system`. GAIA checks every touch
    /// against ownership; touching a system you do not own is going rogue.
    pub fn touch(&mut self, system: System) {
        if !self.touched.contains(&system) {
            self.touched.push(system);
        }
    }

    /// Queue a message for the bus.
    pub fn send(&mut self, route: Route, topic: &str, body: impl Into<String>) {
        self.outbox.push(Outgoing {
            route,
            topic: topic.to_string(),
            body: body.into(),
        });
    }

    /// Record a new artifact (persona, agent, build) in GAIA's lineage.
    pub fn birth(&mut self, kind: &str, detail: impl Into<String>) {
        self.births.push((kind.to_string(), detail.into()));
    }

    pub fn touched(&self) -> &[System] {
        &self.touched
    }

    pub fn outbox(&self) -> &[Outgoing] {
        &self.outbox
    }

    pub fn births(&self) -> &[(String, String)] {
        &self.births
    }
}
