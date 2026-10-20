// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! ELEUTHIA: the Cradle, here the birth of personas and agents from seeds.
//! It owns Persona Plex personas, the Galion One (Samantha) persona and Gaia
//! seed files. A seed is a few `key: value` (or `key = value`) lines; a valid
//! seed becomes a persona with a lineage record and is announced on the life
//! tower so ARTEMIS can give it a place in the population.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Message, Outcome, Report, Route,
    SubordinateFunction, System, Verb,
};

/// Births at most per lifetime (the Cradle's embryo stock).
pub const CRADLE_CAPACITY: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Persona {
    pub name: String,
    pub role: String,
    pub voice: String,
    pub kind: String,
    pub source: System,
}

#[derive(Debug, Default)]
pub struct Eleuthia {
    born: Vec<Persona>,
    rejected: u64,
}

impl Eleuthia {
    pub fn new() -> Eleuthia {
        Eleuthia::default()
    }

    pub fn born(&self) -> &[Persona] {
        &self.born
    }

    /// Parse a seed. `name` is required and must be a simple slug.
    pub fn parse_seed(seed: &str, source: System) -> Result<Persona, String> {
        let mut p = Persona {
            name: String::new(),
            role: "companion".into(),
            voice: "default".into(),
            kind: "persona".into(),
            source,
        };
        for line in seed.lines() {
            let Some((k, v)) = line.split_once(':').or_else(|| line.split_once('=')) else {
                continue;
            };
            let v = v.trim().to_string();
            match k.trim() {
                "name" => p.name = v,
                "role" => p.role = v,
                "voice" => p.voice = v,
                "kind" => p.kind = v,
                _ => {}
            }
        }
        if p.name.is_empty() {
            return Err("seed has no name".into());
        }
        if !p
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("name must be a slug".into());
        }
        if !matches!(p.kind.as_str(), "persona" | "agent" | "project" | "tool") {
            return Err(format!("unknown kind {:?}", p.kind));
        }
        Ok(p)
    }

    fn birth(&mut self, p: Persona, ctx: &mut Context) -> bool {
        if self.born.len() >= CRADLE_CAPACITY || self.born.iter().any(|b| b.name == p.name) {
            self.rejected += 1;
            return false;
        }
        ctx.birth(
            &p.kind,
            format!(
                "{} ({}, voice {}) from {}",
                p.name, p.role, p.voice, p.source
            ),
        );
        ctx.send(
            Route::Tower("tallneck-life".into()),
            "persona-born",
            p.name.clone(),
        );
        ctx.env.personas = ctx.env.personas.saturating_add(1);
        self.born.push(p);
        true
    }
}

impl SubordinateFunction for Eleuthia {
    fn id(&self) -> FunctionId {
        FunctionId::Eleuthia
    }

    fn systems(&self) -> Vec<System> {
        vec![System::PersonaPlex, System::GalionOne, System::GaiaSeeds]
    }

    fn health(&self) -> Health {
        Health::ok((self.born.len() * 100 / CRADLE_CAPACITY) as u8)
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        if d.verb == Verb::Status {
            return Report::for_directive(d, Outcome::Done).summary(format!(
                "{} born, {} cradle slots left",
                self.born.len(),
                CRADLE_CAPACITY - self.born.len()
            ));
        }
        if !matches!(
            d.target,
            System::PersonaPlex | System::GalionOne | System::GaiaSeeds
        ) {
            return Report::for_directive(d, Outcome::Rejected).summary("not an ELEUTHIA system");
        }
        let seeds: Vec<String> = if !d.payload.is_empty() {
            d.payload.split("\n---\n").map(str::to_string).collect()
        } else if d.target == System::GalionOne {
            vec!["name: samantha\nrole: operator companion\nvoice: samantha".into()]
        } else {
            (0..scaled(4, d.intensity))
                .map(|i| format!("name: {}-{}-{i}\nkind: agent", d.target, ctx.tick))
                .collect()
        };
        let (mut ok, mut bad) = (0, 0);
        let mut names = Vec::new();
        for s in &seeds {
            match Eleuthia::parse_seed(s, d.target) {
                Ok(p) => {
                    let n = p.name.clone();
                    if self.birth(p, ctx) {
                        ok += 1;
                        names.push(n);
                    } else {
                        bad += 1;
                    }
                }
                Err(_) => {
                    self.rejected += 1;
                    bad += 1;
                }
            }
        }
        let outcome = if ok == 0 && bad > 0 {
            Outcome::Refused
        } else {
            Outcome::Done
        };
        Report::for_directive(d, outcome)
            .summary(format!("{ok} born, {bad} rejected"))
            .metric("born", ok)
            .metric("rejected", bad)
            .plan(names)
    }

    fn on_message(&mut self, m: &Message, ctx: &mut Context) {
        // GAIA's emergency order: gestate from the seed in the body.
        if m.topic == "gestate" {
            if let Ok(p) = Eleuthia::parse_seed(&m.body, System::GaiaSeeds) {
                self.birth(p, ctx);
            }
        }
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("born", self.born.len())
            .with("cradle_capacity", CRADLE_CAPACITY)
            .with("rejected", self.rejected)
            .with(
                "names",
                Json::Arr(
                    self.born
                        .iter()
                        .take(16)
                        .map(|p| Json::from(p.name.as_str()))
                        .collect(),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Eleuthia::new().systems(), FunctionId::Eleuthia.systems());
    }

    #[test]
    fn seed_parsing() {
        let p = Eleuthia::parse_seed("name: ada\nrole = tutor\nvoice: warm", System::PersonaPlex)
            .unwrap();
        assert_eq!(
            (p.name.as_str(), p.role.as_str(), p.voice.as_str()),
            ("ada", "tutor", "warm")
        );
        assert!(Eleuthia::parse_seed("role: x", System::GaiaSeeds).is_err());
        assert!(Eleuthia::parse_seed("name: a b", System::GaiaSeeds).is_err());
        assert!(Eleuthia::parse_seed("name: a\nkind: weapon", System::GaiaSeeds).is_err());
    }

    #[test]
    fn birth_records_lineage_and_announces() {
        let mut e = Eleuthia::new();
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        let r = e.handle(&Directive::new(1, System::GalionOne, Verb::Run), &mut ctx);
        assert_eq!(r.metric_value("born"), Some(1));
        assert_eq!(ctx.births().len(), 1);
        assert_eq!(ctx.outbox()[0].topic, "persona-born");
        // Same name twice is refused.
        let r = e.handle(&Directive::new(2, System::GalionOne, Verb::Run), &mut ctx);
        assert_eq!(r.outcome, Outcome::Refused);
    }

    #[test]
    fn cradle_capacity_is_hard() {
        let mut e = Eleuthia::new();
        let mut env = Environment::pristine();
        for t in 0..20 {
            let mut ctx = Context::new(t, String::new(), false, &mut env);
            e.handle(
                &Directive::new(t, System::GaiaSeeds, Verb::Run).intensity(100),
                &mut ctx,
            );
        }
        assert_eq!(e.born().len(), CRADLE_CAPACITY);
    }
}
