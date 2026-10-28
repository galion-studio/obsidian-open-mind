// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! ARTEMIS: fauna, here the agent population. It owns the Hermes agents, the
//! boss node, the room view, spawning, retirement and the ecosystem census.
//! The population has a hard cap, and spawning backs off when POSEIDON's
//! resource headroom (`env.water`) is low. Personas born by ELEUTHIA arrive
//! over the life tower and are given a body here.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Message, Outcome, Report,
    SubordinateFunction, System, Verb,
};

/// Agents alive at most.
pub const MAX_POPULATION: usize = 64;
/// Below this resource headroom no new agent is spawned.
pub const MIN_HEADROOM: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Hermes,
    Boss,
    Worker,
    Persona,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Hermes => "hermes",
            Kind::Boss => "boss",
            Kind::Worker => "worker",
            Kind::Persona => "persona",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    pub born_tick: u64,
}

#[derive(Debug, Default)]
pub struct Artemis {
    population: Vec<Agent>,
    next: u64,
    spawned: u64,
    retired: u64,
    refused: u64,
}

impl Artemis {
    pub fn new() -> Artemis {
        Artemis::default()
    }

    pub fn population(&self) -> &[Agent] {
        &self.population
    }

    /// Spawn up to `n` agents. Returns how many were born.
    pub fn spawn(&mut self, kind: Kind, name: &str, n: u64, ctx: &mut Context) -> u64 {
        let mut born = 0;
        for _ in 0..n {
            if self.population.len() >= MAX_POPULATION || ctx.env.water < MIN_HEADROOM {
                self.refused += 1;
                continue;
            }
            self.next += 1;
            let agent_name = if name.is_empty() {
                format!("{}-{}", kind.name(), self.next)
            } else {
                name.to_string()
            };
            self.population.push(Agent {
                id: self.next,
                kind,
                name: agent_name,
                born_tick: ctx.tick,
            });
            born += 1;
        }
        self.spawned += born;
        ctx.env.agents = self.population.len() as u32;
        born
    }

    fn retire(&mut self, n: u64, ctx: &mut Context) -> u64 {
        let mut gone = 0;
        while gone < n {
            // Retire the oldest worker or hermes agent first; personas and
            // the boss stay.
            match self
                .population
                .iter()
                .position(|a| matches!(a.kind, Kind::Worker | Kind::Hermes))
            {
                Some(i) => {
                    self.population.remove(i);
                    gone += 1;
                }
                None => break,
            }
        }
        self.retired += gone;
        ctx.env.agents = self.population.len() as u32;
        gone
    }

    pub fn census(&self) -> Vec<(Kind, usize)> {
        [Kind::Hermes, Kind::Boss, Kind::Worker, Kind::Persona]
            .into_iter()
            .map(|k| (k, self.population.iter().filter(|a| a.kind == k).count()))
            .collect()
    }

    /// Parse "spent/cap" in cents.
    pub fn boss_budget(payload: &str) -> Option<(u64, u64)> {
        let (a, b) = payload.split_once('/')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    }
}

impl SubordinateFunction for Artemis {
    fn id(&self) -> FunctionId {
        FunctionId::Artemis
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::HermesAgents,
            System::BossNode,
            System::Room,
            System::AgentSpawning,
            System::AgentRetirement,
            System::EcosystemCensus,
        ]
    }

    fn health(&self) -> Health {
        Health::ok((self.population.len() * 100 / MAX_POPULATION) as u8)
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let done = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        let status = d.verb == Verb::Status;
        match d.target {
            System::HermesAgents => {
                let born = if status {
                    0
                } else {
                    self.spawn(Kind::Hermes, "", scaled(2, d.intensity), ctx)
                };
                let n = self
                    .population
                    .iter()
                    .filter(|a| a.kind == Kind::Hermes)
                    .count();
                done(format!("{n} Hermes agent(s), {born} new"))
                    .metric("hermes", n as i64)
                    .metric("born", born as i64)
            }
            System::BossNode => match Artemis::boss_budget(&d.payload) {
                Some((spent, cap)) => {
                    let left = cap.saturating_sub(spent);
                    let exhausted = left * 10 < cap;
                    let r = done(format!(
                        "boss budget {spent}/{cap} cents, {left} left{}",
                        if exhausted {
                            ": nearly exhausted, no paid calls"
                        } else {
                            ""
                        }
                    ))
                    .metric("left_cents", left as i64);
                    r.metric("paid_calls_allowed", i64::from(!exhausted))
                }
                None => {
                    if !status && !self.population.iter().any(|a| a.kind == Kind::Boss) {
                        self.spawn(Kind::Boss, "boss", 1, ctx);
                    }
                    done("boss node present".into())
                }
            },
            System::Room => {
                let lines: Vec<String> = self
                    .population
                    .iter()
                    .take(16)
                    .map(|a| format!("{} {} {}", a.id, a.kind.name(), a.name))
                    .collect();
                done(format!("room page with {} line(s)", lines.len())).plan(lines)
            }
            System::AgentSpawning => {
                let want = if status { 0 } else { scaled(8, d.intensity) };
                let born = self.spawn(Kind::Worker, "", want, ctx);
                done(format!("{born}/{want} worker(s) spawned"))
                    .metric("born", born as i64)
                    .metric("refused", (want - born) as i64)
            }
            System::AgentRetirement => {
                let gone = if status {
                    0
                } else {
                    self.retire(scaled(4, d.intensity), ctx)
                };
                done(format!("{gone} agent(s) retired")).metric("retired", gone as i64)
            }
            System::EcosystemCensus => {
                let mut r = done(format!("{} agent(s) alive", self.population.len()))
                    .metric("total", self.population.len() as i64);
                for (k, n) in self.census() {
                    r = r.metric(k.name(), n as i64);
                }
                r
            }
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not an ARTEMIS system")),
        }
    }

    fn on_message(&mut self, m: &Message, ctx: &mut Context) {
        if m.topic == "persona-born" {
            self.spawn(Kind::Persona, &m.body, 1, ctx);
        }
    }

    fn reset(&mut self) {
        *self = Artemis::default();
    }

    fn state_json(&self) -> Json {
        let mut census = Json::obj();
        for (k, n) in self.census() {
            census = census.with(k.name(), n);
        }
        Json::obj()
            .with("population", self.population.len())
            .with("cap", MAX_POPULATION)
            .with("spawned", self.spawned)
            .with("retired", self.retired)
            .with("refused", self.refused)
            .with("census", census)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    fn run(a: &mut Artemis, d: Directive, env: &mut Environment) -> Report {
        let mut ctx = Context::new(1, String::new(), false, env);
        a.handle(&d, &mut ctx)
    }

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Artemis::new().systems(), FunctionId::Artemis.systems());
    }

    #[test]
    fn spawn_is_capped_and_census_counts() {
        let mut a = Artemis::new();
        let mut env = Environment::pristine();
        for i in 0..20 {
            run(
                &mut a,
                Directive::new(i, System::AgentSpawning, Verb::Run).intensity(100),
                &mut env,
            );
        }
        assert_eq!(a.population().len(), MAX_POPULATION);
        assert_eq!(env.agents as usize, MAX_POPULATION);
        let r = run(
            &mut a,
            Directive::new(99, System::EcosystemCensus, Verb::Run),
            &mut env,
        );
        assert_eq!(r.metric_value("worker"), Some(MAX_POPULATION as i64));
        let r = run(
            &mut a,
            Directive::new(100, System::AgentRetirement, Verb::Run).intensity(100),
            &mut env,
        );
        assert_eq!(r.metric_value("retired"), Some(4));
    }

    #[test]
    fn low_headroom_blocks_spawning() {
        let mut a = Artemis::new();
        let mut env = Environment::pristine();
        env.water = 5;
        let r = run(
            &mut a,
            Directive::new(1, System::AgentSpawning, Verb::Run),
            &mut env,
        );
        assert_eq!(r.metric_value("born"), Some(0));
    }

    #[test]
    fn boss_budget_exhaustion() {
        let mut a = Artemis::new();
        let mut env = Environment::pristine();
        let r = run(
            &mut a,
            Directive::new(1, System::BossNode, Verb::Status).payload("413/500"),
            &mut env,
        );
        assert_eq!(r.metric_value("paid_calls_allowed"), Some(1));
        let r = run(
            &mut a,
            Directive::new(2, System::BossNode, Verb::Status).payload("490/500"),
            &mut env,
        );
        assert_eq!(r.metric_value("paid_calls_allowed"), Some(0));
    }
}
