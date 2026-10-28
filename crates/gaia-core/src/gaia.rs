// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! GAIA: the central orchestrator. Owns no system itself; it routes
//! directives to the owning subordinate function, runs the tick loop, keeps
//! lineage, enforces containment and the HADES gate, and answers to the
//! master override.

use crate::bus::{Bus, Endpoint, Message, Route};
use crate::json::Json;
use crate::lineage::{Lineage, SimClock};
use crate::model::{Context, Directive, Environment, Health, Lifecycle, Outcome, Report, Verb};
use crate::registry::AlphaRegistry;
use crate::system::{FunctionId, System};
use crate::SubordinateFunction;
use std::collections::BTreeMap;

/// Commands only a master-override holder may give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Override {
    /// Contain a function now.
    Contain(FunctionId),
    /// Reboot a contained (or any) function: Contained -> Rebooting -> Booting -> Running.
    Reboot(FunctionId),
    /// Put every function and GAIA back to Dormant.
    ShutdownAll,
}

struct Slot {
    func: Box<dyn SubordinateFunction>,
    state: Lifecycle,
    reason: Option<String>,
    handled: u64,
    violations: u64,
}

/// Summary of one tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub tick: u64,
    pub reports: Vec<Report>,
    pub contained: Vec<FunctionId>,
    pub transitions: Vec<(FunctionId, Lifecycle, Lifecycle)>,
}

impl TickReport {
    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("tick", self.tick)
            .with(
                "reports",
                Json::Arr(self.reports.iter().map(Report::to_json).collect()),
            )
            .with(
                "contained",
                Json::Arr(
                    self.contained
                        .iter()
                        .map(|f| Json::from(f.name()))
                        .collect(),
                ),
            )
            .with(
                "transitions",
                Json::Arr(
                    self.transitions
                        .iter()
                        .map(|(f, a, b)| {
                            Json::obj()
                                .with("function", f.name())
                                .with("from", a.name())
                                .with("to", b.name())
                        })
                        .collect(),
                ),
            )
    }
}

pub struct Gaia {
    state: Lifecycle,
    tick: u64,
    env: Environment,
    bus: Bus,
    slots: BTreeMap<FunctionId, Slot>,
    alphas: AlphaRegistry,
    lineage: Lineage,
    clock: SimClock,
    next_directive: u64,
    events: Vec<Json>,
    root: u64,
}

/// Hard caps that keep every run bounded.
pub const MAX_EVENTS: usize = 512;
pub const MAX_LINEAGE: usize = 4096;
pub const BUS_CAPACITY: usize = 64;

impl Default for Gaia {
    fn default() -> Self {
        Gaia::new(SimClock::DEFAULT)
    }
}

impl Gaia {
    pub fn new(clock: SimClock) -> Gaia {
        let mut lineage = Lineage::new(MAX_LINEAGE);
        let root = lineage.record(
            None,
            0,
            clock.at(0),
            "GAIA",
            "genesis",
            "GAIA core installed",
        );
        Gaia {
            state: Lifecycle::Dormant,
            tick: 0,
            env: Environment::pristine(),
            bus: Bus::new(BUS_CAPACITY),
            slots: BTreeMap::new(),
            alphas: AlphaRegistry::with_owner(),
            lineage,
            clock,
            next_directive: 1,
            events: Vec::new(),
            root,
        }
    }

    /// Register a subordinate function. Its declared systems must be exactly
    /// the systems the ownership table gives it.
    pub fn register(&mut self, func: Box<dyn SubordinateFunction>) -> Result<(), String> {
        let id = func.id();
        if self.slots.contains_key(&id) {
            return Err(format!("{id} already registered"));
        }
        let mut declared = func.systems();
        declared.sort();
        declared.dedup();
        let expected = id.systems();
        if declared != expected {
            return Err(format!(
                "{id} declares {:?} but owns {:?}",
                declared.iter().map(|s| s.name()).collect::<Vec<_>>(),
                expected.iter().map(|s| s.name()).collect::<Vec<_>>()
            ));
        }
        self.slots.insert(
            id,
            Slot {
                func,
                state: Lifecycle::Dormant,
                reason: None,
                handled: 0,
                violations: 0,
            },
        );
        Ok(())
    }

    pub fn alphas_mut(&mut self) -> &mut AlphaRegistry {
        &mut self.alphas
    }

    pub fn env(&self) -> &Environment {
        &self.env
    }

    pub fn env_mut(&mut self) -> &mut Environment {
        &mut self.env
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn lineage(&self) -> &Lineage {
        &self.lineage
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn now(&self) -> String {
        self.clock.at(self.tick)
    }

    pub fn state(&self) -> Lifecycle {
        self.state
    }

    pub fn function_state(&self, f: FunctionId) -> Option<Lifecycle> {
        self.slots.get(&f).map(|s| s.state)
    }

    pub fn function_health(&self, f: FunctionId) -> Option<Health> {
        self.slots.get(&f).map(|s| s.func.health())
    }

    pub fn function_json(&self, f: FunctionId) -> Option<Json> {
        self.slots.get(&f).map(|s| s.func.state_json())
    }

    pub fn registered(&self) -> Vec<FunctionId> {
        self.slots.keys().copied().collect()
    }

    pub fn events(&self) -> &[Json] {
        &self.events
    }

    /// A fresh directive id.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_directive;
        self.next_directive += 1;
        id
    }

    /// Build a directive with a fresh id.
    pub fn directive(&mut self, target: System, verb: Verb) -> Directive {
        let id = self.next_id();
        Directive::new(id, target, verb)
    }

    fn event(&mut self, kind: &str, detail: Json) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(
                Json::obj()
                    .with("tick", self.tick)
                    .with("at", self.now())
                    .with("kind", kind)
                    .with("detail", detail),
            );
        }
    }

    fn record(&mut self, parent: Option<u64>, actor: &str, kind: &str, detail: &str) -> u64 {
        let at = self.now();
        self.lineage.record(
            parent.or(Some(self.root)),
            self.tick,
            at,
            actor,
            kind,
            detail,
        )
    }

    fn set_state(&mut self, f: FunctionId, to: Lifecycle, out: Option<&mut TickReport>) {
        if let Some(slot) = self.slots.get_mut(&f) {
            let from = slot.state;
            if from != to {
                slot.state = to;
                if let Some(t) = out {
                    t.transitions.push((f, from, to));
                }
                self.event(
                    "transition",
                    Json::obj()
                        .with("function", f.name())
                        .with("from", from.name())
                        .with("to", to.name()),
                );
            }
        }
    }

    /// Start GAIA and every registered function.
    pub fn boot(&mut self) {
        self.state = Lifecycle::Booting;
        let ids = self.registered();
        for f in ids {
            if self.slots[&f].state == Lifecycle::Dormant {
                self.set_state(f, Lifecycle::Booting, None);
            }
        }
        self.record(
            None,
            "GAIA",
            "boot",
            "GAIA and subordinate functions booting",
        );
    }

    /// Contain a function: Rogue then Contained, quarantined on the bus,
    /// rejected by routing. Only the master override can bring it back.
    pub fn contain(&mut self, f: FunctionId, reason: &str, out: Option<&mut TickReport>) {
        let Some(slot) = self.slots.get_mut(&f) else {
            return;
        };
        if slot.state == Lifecycle::Contained {
            return;
        }
        slot.reason = Some(reason.to_string());
        slot.violations += 1;
        let from = slot.state;
        slot.state = Lifecycle::Contained;
        self.bus.quarantine(f);
        if let Some(t) = out {
            t.transitions.push((f, from, Lifecycle::Rogue));
            t.transitions
                .push((f, Lifecycle::Rogue, Lifecycle::Contained));
            t.contained.push(f);
        }
        self.event(
            "containment",
            Json::obj()
                .with("function", f.name())
                .with("reason", reason),
        );
        self.record(None, "GAIA", "containment", &format!("{f}: {reason}"));
        let tick = self.tick;
        self.bus.send(
            tick,
            Endpoint::Gaia,
            Route::AllTowers,
            "containment",
            &format!("{f} contained: {reason}"),
        );
    }

    /// Master override. Only an Alpha holding the override may use it.
    pub fn master_override(&mut self, alpha: &str, cmd: Override) -> Result<Json, String> {
        if !self.alphas.is_override_holder(alpha) {
            self.event("override-denied", Json::obj().with("alpha", alpha));
            return Err(format!("{alpha:?} does not hold the master override"));
        }
        let detail = match cmd {
            Override::Contain(f) => {
                self.contain(f, &format!("master override by {alpha}"), None);
                format!("contain {f}")
            }
            Override::Reboot(f) => {
                let slot = self
                    .slots
                    .get_mut(&f)
                    .ok_or_else(|| format!("{f} not registered"))?;
                slot.reason = None;
                self.bus.release(f);
                self.set_state(f, Lifecycle::Rebooting, None);
                format!("reboot {f}")
            }
            Override::ShutdownAll => {
                for f in self.registered() {
                    self.set_state(f, Lifecycle::Dormant, None);
                }
                self.state = Lifecycle::Dormant;
                "shutdown all".to_string()
            }
        };
        self.record(None, &format!("override:{alpha}"), "override", &detail);
        self.event(
            "override",
            Json::obj()
                .with("alpha", alpha)
                .with("command", detail.as_str()),
        );
        Ok(Json::obj().with("ok", true).with("command", detail))
    }

    /// Inject an outside signal through every tower (the "subversion signal").
    pub fn signal(&mut self, topic: &str, body: &str) {
        let tick = self.tick;
        self.bus
            .send(tick, Endpoint::External, Route::AllTowers, topic, body);
        self.record(None, "external", "signal", &format!("{topic}: {body}"));
        self.event(
            "signal",
            Json::obj().with("topic", topic).with("body", body),
        );
    }

    /// Release a rogue swarm into the world (scenario hook).
    pub fn release_swarm(&mut self, units: u64) {
        self.env.swarm = units.min(Environment::SWARM_MAX);
        self.env.swarm_active = true;
        self.env.cipher_progress = 0;
        self.record(
            None,
            "world",
            "swarm",
            &format!("rogue swarm of {units} units released"),
        );
        self.event("swarm", Json::obj().with("units", units));
    }

    /// Check what a function did and either commit its environment or
    /// contain it. Returns `Some(reason)` when it was contained.
    fn judge(
        &mut self,
        f: FunctionId,
        ctx_touched: &[System],
        report: Option<&Report>,
        approved: bool,
        mutating: bool,
    ) -> Option<String> {
        let foreign: Vec<&str> = ctx_touched
            .iter()
            .filter(|s| s.function() != f)
            .map(|s| s.name())
            .collect();
        if !foreign.is_empty() {
            return Some(format!("acted outside its domain on {}", foreign.join(",")));
        }
        if let Some(r) = report {
            if let Some(s) = r.system {
                if s.function() != f {
                    return Some(format!("reported on foreign system {s}"));
                }
                if mutating
                    && s.destructive()
                    && !approved
                    && !r.dry_run
                    && r.outcome == Outcome::Done
                {
                    return Some(format!("executed destructive {s} without approval"));
                }
            }
        }
        None
    }

    fn apply_ctx(
        &mut self,
        f: FunctionId,
        env: Environment,
        outbox: Vec<crate::model::Outgoing>,
        births: Vec<(String, String)>,
        parent: Option<u64>,
    ) {
        self.env = env;
        let tick = self.tick;
        for o in outbox {
            self.bus
                .send(tick, Endpoint::Function(f), o.route, &o.topic, &o.body);
            self.env.messages = self.env.messages.saturating_add(1);
        }
        for (kind, detail) in births {
            self.record(parent, f.name(), &kind, &detail);
        }
    }

    /// Route a directive to the owner of its target system.
    pub fn dispatch(&mut self, d: &Directive) -> Report {
        let owner = d.target.function();
        let reject = |why: String| Report::for_directive(d, Outcome::Rejected).summary(why);
        let state = match self.slots.get(&owner) {
            None => {
                return reject(format!(
                    "no function owns {} (owner {owner} not registered)",
                    d.target
                ))
            }
            Some(s) => s.state,
        };
        if state == Lifecycle::Contained || state == Lifecycle::Rogue {
            return reject(format!("{owner} is contained"));
        }
        if !state.active() {
            return reject(format!("{owner} is {state}"));
        }
        let approved = self.alphas.verify(d);
        let parent = self.record(
            None,
            "GAIA",
            "directive",
            &format!("#{} {} {} -> {owner}", d.id, d.verb.name(), d.target),
        );
        let mut scratch = self.env.clone();
        let at = self.now();
        let (report, touched, outbox, births) = {
            let slot = self.slots.get_mut(&owner).expect("checked above");
            let mut ctx = Context::new(self.tick, at, approved, &mut scratch);
            ctx.touch(d.target);
            let mut r = slot.func.handle(d, &mut ctx);
            r.directive_id = d.id;
            slot.handled += 1;
            (r, ctx.touched, ctx.outbox, ctx.births)
        };
        if let Some(reason) = self.judge(
            owner,
            &touched,
            Some(&report),
            approved,
            d.verb != Verb::Status,
        ) {
            self.contain(owner, &reason, None);
            return Report::for_directive(d, Outcome::Contained).summary(reason);
        }
        self.apply_ctx(owner, scratch, outbox, births, Some(parent));
        report
    }

    /// Route a directive given by name. Unknown systems are rejected.
    pub fn dispatch_named(&mut self, system: &str, verb: Verb) -> Result<Report, String> {
        let s = System::parse(system).ok_or_else(|| format!("unknown system {system:?}"))?;
        let d = self.directive(s, verb);
        Ok(self.dispatch(&d))
    }

    /// One deterministic step of the world and of every active function.
    pub fn tick(&mut self) -> TickReport {
        self.tick += 1;
        let mut out = TickReport {
            tick: self.tick,
            ..TickReport::default()
        };
        // Lifecycle steps.
        if self.state == Lifecycle::Booting {
            self.state = Lifecycle::Running;
        }
        for f in self.registered() {
            match self.slots[&f].state {
                Lifecycle::Booting => self.set_state(f, Lifecycle::Running, Some(&mut out)),
                Lifecycle::Rebooting => {
                    if let Some(s) = self.slots.get_mut(&f) {
                        s.func.reset();
                    }
                    self.set_state(f, Lifecycle::Booting, Some(&mut out));
                }
                _ => {}
            }
        }
        self.world_step();
        // Functions: messages first, then their own work.
        for f in self.registered() {
            if !self.slots[&f].state.active() {
                continue;
            }
            let inbox: Vec<Message> = self.bus.drain(f);
            let mut scratch = self.env.clone();
            let at = self.now();
            let tick = self.tick;
            let (report, touched, outbox, births) = {
                let slot = self.slots.get_mut(&f).expect("registered");
                let mut ctx = Context::new(tick, at, false, &mut scratch);
                for m in &inbox {
                    slot.func.on_message(m, &mut ctx);
                }
                let r = slot.func.tick(&mut ctx);
                (r, ctx.touched, ctx.outbox, ctx.births)
            };
            if let Some(reason) = self.judge(f, &touched, report.as_ref(), false, true) {
                self.contain(f, &reason, Some(&mut out));
                continue;
            }
            self.apply_ctx(f, scratch, outbox, births, None);
            if let Some(r) = report {
                out.reports.push(r);
            }
            // Health-based degrade / recover.
            let h = self.slots[&f].func.health();
            let st = self.slots[&f].state;
            if h.integrity < Health::DEGRADE_BELOW && st == Lifecycle::Running {
                self.set_state(f, Lifecycle::Degraded, Some(&mut out));
            } else if h.integrity >= Health::DEGRADE_BELOW && st == Lifecycle::Degraded {
                self.set_state(f, Lifecycle::Running, Some(&mut out));
            }
        }
        // GAIA reads its own inbox (requests from functions).
        for m in self.bus.drain_gaia() {
            self.event(
                "to-gaia",
                Json::obj()
                    .with("from", m.from.name())
                    .with("topic", m.topic.as_str())
                    .with("body", m.body.as_str()),
            );
        }
        // GAIA's own state follows its functions.
        if self.state.active() {
            let degraded = self
                .slots
                .values()
                .any(|s| matches!(s.state, Lifecycle::Contained | Lifecycle::Degraded));
            self.state = if degraded {
                Lifecycle::Degraded
            } else {
                Lifecycle::Running
            };
        }
        out
    }

    /// The rogue swarm: it eats biomass and replicates while there is food.
    fn world_step(&mut self) {
        swarm_step(&mut self.env);
    }
}

/// Biomass one swarm unit eats per tick.
pub const SWARM_BITE: u64 = 8;

/// One tick of the rogue swarm, as a pure function so callers can predict it.
pub fn swarm_step(e: &mut Environment) {
    if e.swarm_active && e.swarm > 0 {
        let eat = e.biomass.min(e.swarm.saturating_mul(SWARM_BITE));
        e.biomass -= eat;
        if eat > 0 {
            e.swarm = (e.swarm + e.swarm / 2 + 1).min(Environment::SWARM_MAX);
        }
        e.air = e.air.saturating_sub(3);
        e.water = e.water.saturating_sub(3);
        e.soil = e.soil.saturating_sub(2);
    }
}

impl Gaia {
    /// The command hierarchy, top to bottom.
    pub fn hierarchy_json(&self) -> Json {
        Json::obj()
            .with("master_override", self.alphas.to_json())
            .with("overseer", "GAIA (orchestration only, owns no system)")
            .with(
                "functions",
                Json::Arr(
                    self.slots
                        .iter()
                        .map(|(f, s)| {
                            Json::obj()
                                .with("function", f.name())
                                .with("state", s.state.name())
                                .with(
                                    "machines",
                                    Json::Arr(
                                        f.systems().iter().map(|x| Json::from(x.name())).collect(),
                                    ),
                                )
                        })
                        .collect(),
                ),
            )
    }

    pub fn status_json(&self) -> Json {
        let functions = Json::Arr(
            self.slots
                .iter()
                .map(|(f, s)| {
                    Json::obj()
                        .with("function", f.name())
                        .with("hzd_role", f.hzd_role())
                        .with("domain", f.domain())
                        .with("state", s.state.name())
                        .with("health", s.func.health().to_json())
                        .with("handled", s.handled)
                        .with("violations", s.violations)
                        .with("contained_because", s.reason.clone())
                        .with(
                            "systems",
                            Json::Arr(f.systems().iter().map(|x| Json::from(x.name())).collect()),
                        )
                        .with("detail", s.func.state_json())
                })
                .collect(),
        );
        Json::obj()
            .with("gaia", self.state.name())
            .with("tick", self.tick)
            .with("sim_time", self.now())
            .with("environment", self.env.to_json())
            .with("functions", functions)
            .with("bus", self.bus.to_json())
            .with("lineage_records", self.lineage.records().len())
    }
}
