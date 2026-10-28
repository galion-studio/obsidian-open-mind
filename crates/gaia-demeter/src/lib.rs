// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! DEMETER: flora and environmental stability, here the health of the stack's
//! environment. It owns survival (decide/degrade), the non-destructive healer
//! (heal-dry: findings and suggestions only), the tuner, config health, the
//! guardian-tick health cycle and the boot units. After the deactivation codes
//! go out it regrows the world tick by tick.

use gaia_core::{
    scaled, Context, Directive, Environment, FunctionId, Health, Json, Message, Outcome, Report,
    SubordinateFunction, System, Verb,
};

/// Biomass regrown per tick after the swarm is down.
pub const REGROWTH: u64 = 10_000;

const SECRET_KEYS: &[&str] = &[
    "secret", "token", "password", "passwd", "api_key", "apikey", "private",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Full,
    Degraded,
    Survival,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Full => "full",
            Mode::Degraded => "degraded",
            Mode::Survival => "survival",
        }
    }
}

#[derive(Debug, Default)]
pub struct Demeter {
    regrowing: bool,
    cycles: u64,
    config_findings: u64,
}

impl Demeter {
    pub fn new() -> Demeter {
        Demeter::default()
    }

    /// Survival decision from the world's vital signs.
    pub fn decide(env: &Environment) -> Mode {
        let low = env.air.min(env.water).min(env.soil);
        if env.swarm_active || low < 30 {
            Mode::Survival
        } else if low < 60 {
            Mode::Degraded
        } else {
            Mode::Full
        }
    }

    /// Config health: returns findings (never values).
    pub fn check_config(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().trim_matches('"'));
            if (k.contains("bind") || k.contains("listen") || k.contains("host"))
                && v.starts_with("0.0.0.0")
            {
                out.push(format!(
                    "line {}: {k} binds all interfaces; loopback by default",
                    i + 1
                ));
            }
            if SECRET_KEYS.iter().any(|s| k.contains(s)) && !v.is_empty() {
                out.push(format!(
                    "line {}: {k} holds a value; move it to the secret store (value not shown)",
                    i + 1
                ));
            }
        }
        out
    }

    fn heal_dry(env: &Environment) -> Vec<String> {
        let mut v = Vec::new();
        for (name, val, owner) in [
            ("air", env.air, "AETHER"),
            ("water", env.water, "POSEIDON"),
            ("soil", env.soil, "DEMETER"),
        ] {
            if val < 60 {
                v.push(format!(
                    "{name} at {val}: ask {owner} to restore (suggestion only)"
                ));
            }
        }
        if env.swarm_active {
            v.push("rogue swarm active: MINERVA first; destructive fixes are HADES plans".into());
        }
        v
    }
}

impl SubordinateFunction for Demeter {
    fn id(&self) -> FunctionId {
        FunctionId::Demeter
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::Survival,
            System::Healer,
            System::Tuner,
            System::ConfigHealth,
            System::GuardianTick,
            System::BootUnits,
        ]
    }

    fn health(&self) -> Health {
        Health::ok(if self.regrowing { 60 } else { 15 })
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let done = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        match d.target {
            System::Survival => {
                let m = Demeter::decide(ctx.env);
                done(format!("mode {}", m.name()))
            }
            System::Healer => {
                let plan = Demeter::heal_dry(ctx.env);
                let mut r = Report::for_directive(d, Outcome::DryRun)
                    .summary(format!(
                        "heal-dry: {} suggestion(s); apply is a HADES plan",
                        plan.len()
                    ))
                    .metric("suggestions", plan.len() as i64)
                    .plan(plan);
                r.dry_run = true;
                r
            }
            System::Tuner => {
                let assets = d
                    .payload
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .count();
                let assets = if assets == 0 {
                    scaled(12, d.intensity) as usize
                } else {
                    assets
                };
                done(format!("tuner serves {assets} static asset(s) on loopback"))
                    .metric("assets", assets as i64)
            }
            System::ConfigHealth => {
                let text = if d.payload.is_empty() {
                    "bind = \"127.0.0.1:8080\"\n"
                } else {
                    d.payload.as_str()
                };
                let f = Demeter::check_config(text);
                self.config_findings += f.len() as u64;
                if f.is_empty() && d.verb != Verb::Status {
                    ctx.env.soil = ctx.env.soil.saturating_add(5).min(100);
                }
                done(format!("{} config finding(s)", f.len()))
                    .metric("findings", f.len() as i64)
                    .plan(f)
            }
            System::GuardianTick => {
                self.cycles += 1;
                let m = Demeter::decide(ctx.env);
                let score =
                    i64::from(ctx.env.air) + i64::from(ctx.env.water) + i64::from(ctx.env.soil);
                done(format!("health cycle {}: mode {}", self.cycles, m.name()))
                    .metric("score", score)
            }
            System::BootUnits => {
                let units: Vec<String> = if d.payload.is_empty() {
                    ["galion-desk", "galion-one", "link-watch"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    d.payload
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };
                let plan = units
                    .iter()
                    .map(|u| format!("{u}.service: Restart=on-failure, Nice=10, loopback only"))
                    .collect::<Vec<_>>();
                done(format!(
                    "{} boot unit(s) rendered (not installed)",
                    plan.len()
                ))
                .plan(plan)
            }
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not a DEMETER system")),
        }
    }

    fn on_message(&mut self, m: &Message, _ctx: &mut Context) {
        if m.topic == "deactivation-codes" {
            self.regrowing = true;
        }
    }

    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if !self.regrowing || ctx.env.swarm_active {
            return None;
        }
        ctx.env.biomass = (ctx.env.biomass + REGROWTH).min(Environment::BIOMASS_MAX);
        ctx.env.soil = ctx.env.soil.saturating_add(4).min(100);
        Some(
            Report::new(FunctionId::Demeter, None, Outcome::Done)
                .summary("regrowing")
                .metric("biomass", ctx.env.biomass as i64),
        )
    }

    fn reset(&mut self) {
        *self = Demeter::default();
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("regrowing", self.regrowing)
            .with("cycles", self.cycles)
            .with("config_findings", self.config_findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Demeter::new().systems(), FunctionId::Demeter.systems());
    }

    #[test]
    fn survival_modes() {
        let mut e = Environment::pristine();
        assert_eq!(Demeter::decide(&e), Mode::Full);
        e.soil = 45;
        assert_eq!(Demeter::decide(&e), Mode::Degraded);
        e.swarm_active = true;
        assert_eq!(Demeter::decide(&e), Mode::Survival);
    }

    #[test]
    fn config_findings_hide_values() {
        let f =
            Demeter::check_config("listen = \"0.0.0.0:80\"\napi_token = \"abc123\"\nport = 8080\n");
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| !x.contains("abc123")));
    }

    #[test]
    fn healer_is_dry_and_regrowth_after_codes() {
        let mut d = Demeter::new();
        let mut env = Environment::pristine();
        env.soil = 10;
        env.biomass = 0;
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        let r = d.handle(&Directive::new(1, System::Healer, Verb::Run), &mut ctx);
        assert!(r.dry_run);
        assert_eq!(r.outcome, Outcome::DryRun);
        assert!(d.tick(&mut ctx).is_none(), "no regrowth before the codes");
        d.regrowing = true;
        d.tick(&mut ctx);
        assert_eq!(env.biomass, REGROWTH);
    }
}
