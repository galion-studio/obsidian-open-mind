// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! MINERVA: decryption and override. In the story it cracked the rogue
//! swarm's encryption and broadcast the shutdown codes. Here it owns the
//! security audit (it runs the real `repo-check` rules over text it is
//! given), secret scanning (counts only, never values), the guardian
//! watchdog and alerting, vision decoding and sandbox policy review. While a
//! rogue swarm is active it works on the swarm's code every tick and, when it
//! breaks through, broadcasts the deactivation codes on every tower.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Outcome, Report, Route,
    SubordinateFunction, System, Verb,
};
use repo_check::{scan_text, Check, Level};

pub const DEACTIVATION_TOPIC: &str = "deactivation-codes";

#[derive(Debug, Default)]
pub struct Minerva {
    audits: u64,
    findings: u64,
    alerts: Vec<String>,
    cracked: bool,
}

/// Alerts kept at most.
const MAX_ALERTS: usize = 64;

/// A built-in, clean sample used when an audit directive has no payload.
const SAMPLE: &str = "// SPDX-License-Identifier: MIT\nfn main() {}\n";

/// The policy reviewed when none is given: the openmind-sandbox shape plus
/// one deliberately risky rule, so the review has something to find.
const DEFAULT_POLICY: &str = "rlimit cpu 60\nrlimit as 512M\nworkdir ./job\nenv:clear\nnet\n";

/// Risky sandbox policy lines and why.
const RISKY_POLICY: &[(&str, &str)] = &[
    ("net", "network access"),
    ("setuid", "privilege change"),
    ("write:/", "writes outside the workdir"),
    ("env:inherit", "inherits the caller's environment"),
    ("no-timeout", "no wall timeout"),
    ("no-rlimit", "no resource limits"),
];

/// A rule line matches a risky key when it is the key, the key plus
/// arguments, or (for path keys ending in `/`) any path under it.
fn policy_matches(line: &str, key: &str) -> bool {
    match line.strip_prefix(key) {
        Some(rest) => key.ends_with('/') || rest.is_empty() || rest.starts_with(' '),
        None => false,
    }
}

impl Minerva {
    pub fn new() -> Minerva {
        Minerva::default()
    }

    /// Split an audit payload into (path, text). Format: first line is the
    /// repo-relative path, the rest is the file text.
    fn audit_input(payload: &str) -> (&str, &str) {
        if payload.is_empty() {
            return ("crates/sample/src/main.rs", SAMPLE);
        }
        match payload.split_once('\n') {
            Some((p, t)) => (p.trim(), t),
            None => ("crates/sample/src/main.rs", payload),
        }
    }

    /// Review a sandbox policy (one rule per line). Returns the risky rules.
    pub fn review_policy(policy: &str) -> Vec<String> {
        policy
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                RISKY_POLICY
                    .iter()
                    .find(|(k, _)| policy_matches(l, k))
                    .map(|(k, why)| format!("{k}: {why}"))
            })
            .collect()
    }

    /// Decode a hex payload (vision frames arrive hex-encoded in simulation).
    pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if !s.len().is_multiple_of(2) {
            return None;
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
            .collect()
    }

    fn alert(&mut self, a: String) {
        if !self.alerts.contains(&a) && self.alerts.len() < MAX_ALERTS {
            self.alerts.push(a);
        }
    }
}

impl SubordinateFunction for Minerva {
    fn id(&self) -> FunctionId {
        FunctionId::Minerva
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::RepoCheck,
            System::SecretScan,
            System::GuardianWatchdog,
            System::Alerting,
            System::Vision,
            System::SandboxPolicyReview,
        ]
    }

    fn health(&self) -> Health {
        Health::ok(if self.cracked { 10 } else { 40 })
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let ok = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        match d.target {
            System::RepoCheck | System::SecretScan => {
                let (path, text) = Minerva::audit_input(&d.payload);
                let passes = if d.verb == Verb::Status {
                    0
                } else {
                    scaled(4, d.intensity)
                };
                let mut found = Vec::new();
                for _ in 0..passes.max(1) {
                    found = scan_text(path, text);
                }
                if d.target == System::SecretScan {
                    found.retain(|f| f.check == Check::Secrets);
                }
                let errors = found.iter().filter(|f| f.level == Level::Error).count();
                self.audits += 1;
                self.findings += found.len() as u64;
                if errors > 0 {
                    self.alert(format!("{}: {errors} error finding(s) in {path}", d.target));
                    ctx.send(Route::Tower("tallneck-forge".into()), "audit-failed", path);
                }
                // Finding text never contains matched secret values.
                ok(format!(
                    "{} findings ({errors} errors) in {path}",
                    found.len()
                ))
                .metric("findings", found.len() as i64)
                .metric("errors", errors as i64)
                .metric("passes", passes as i64)
            }
            System::GuardianWatchdog => {
                let e = ctx.env.clone();
                let mut n = 0;
                for (name, v) in [("air", e.air), ("water", e.water), ("soil", e.soil)] {
                    if v < 40 {
                        self.alert(format!("{name} low: {v}"));
                        n += 1;
                    }
                }
                if e.swarm_active {
                    self.alert(format!("rogue swarm active: {} units", e.swarm));
                    n += 1;
                }
                if n > 0 {
                    ctx.send(
                        Route::Tower("spire".into()),
                        "alert",
                        format!("{n} watchdog alerts"),
                    );
                }
                ok(format!("{n} new watchdog alerts")).metric("alerts", n)
            }
            System::Alerting => ok(format!("{} alerts held", self.alerts.len()))
                .metric("alerts", self.alerts.len() as i64),
            System::Vision => {
                let frames = scaled(30, d.intensity);
                let synthetic;
                let input = if d.payload.is_empty() {
                    // A synthetic frame: "GAIA" repeated, hex-encoded, sized by intensity.
                    synthetic = "47414941".repeat(scaled(16, d.intensity) as usize);
                    synthetic.as_str()
                } else {
                    d.payload.as_str()
                };
                match Minerva::decode_hex(input) {
                    Some(bytes) => {
                        let printable = bytes
                            .iter()
                            .filter(|b| b.is_ascii_graphic() || **b == b' ')
                            .count();
                        ok(format!(
                            "decoded {} bytes over {frames} frames",
                            bytes.len()
                        ))
                        .metric("bytes", bytes.len() as i64)
                        .metric("printable", printable as i64)
                        .metric("frames", frames as i64)
                    }
                    None => {
                        Report::for_directive(d, Outcome::Refused).summary("payload is not hex")
                    }
                }
            }
            System::SandboxPolicyReview => {
                let policy = if d.payload.is_empty() {
                    DEFAULT_POLICY
                } else {
                    d.payload.as_str()
                };
                let risky = Minerva::review_policy(policy);
                ok(format!("{} risky rule(s)", risky.len()))
                    .metric("risky", risky.len() as i64)
                    .plan(risky)
            }
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not a MINERVA system")),
        }
    }

    fn tick(&mut self, ctx: &mut Context) -> Option<Report> {
        if !ctx.env.swarm_active {
            return None;
        }
        ctx.env.cipher_progress = ctx.env.cipher_progress.saturating_add(10).min(100);
        if ctx.env.cipher_progress < 100 {
            return Some(
                Report::new(FunctionId::Minerva, None, Outcome::Done)
                    .summary("working on the swarm's code")
                    .metric("cipher_progress", i64::from(ctx.env.cipher_progress)),
            );
        }
        let units = ctx.env.swarm;
        ctx.env.swarm_active = false;
        ctx.env.swarm = 0;
        self.cracked = true;
        ctx.send(
            Route::AllTowers,
            DEACTIVATION_TOPIC,
            format!("{units} units"),
        );
        Some(
            Report::new(FunctionId::Minerva, None, Outcome::Done)
                .summary("code broken; deactivation codes broadcast on every tower")
                .metric("deactivated", units as i64),
        )
    }

    fn reset(&mut self) {
        *self = Minerva::default();
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("audits", self.audits)
            .with("findings", self.findings)
            .with("alerts", self.alerts.len())
            .with("cracked", self.cracked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    fn run(m: &mut Minerva, d: &Directive, env: &mut Environment) -> Report {
        let mut ctx = Context::new(1, String::new(), false, env);
        m.handle(d, &mut ctx)
    }

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(Minerva::new().systems(), FunctionId::Minerva.systems());
    }

    #[test]
    fn repo_check_flags_missing_spdx() {
        let mut m = Minerva::new();
        let mut env = Environment::pristine();
        let clean = run(
            &mut m,
            &Directive::new(1, System::RepoCheck, Verb::Run),
            &mut env,
        );
        assert_eq!(clean.metric_value("findings"), Some(0));
        let d = Directive::new(2, System::RepoCheck, Verb::Run)
            .payload("crates/x/src/lib.rs\nfn x() {}\n");
        let r = run(&mut m, &d, &mut env);
        assert!(r.metric_value("findings").unwrap() >= 1);
    }

    #[test]
    fn secret_scan_counts_without_showing_values() {
        let mut m = Minerva::new();
        let mut env = Environment::pristine();
        let block = concat!("-----BEGIN ", "PRIVATE ", "KEY-----");
        let d = Directive::new(1, System::SecretScan, Verb::Run)
            .payload(format!("notes.txt\n{block}\n"));
        let r = run(&mut m, &d, &mut env);
        assert_eq!(r.metric_value("findings"), Some(1));
        assert!(!r.summary.contains("BEGIN"));
    }

    #[test]
    fn policy_review_and_vision() {
        let risky = Minerva::review_policy("workdir /tmp/x\nnet\nenv:inherit\n# net comment\n");
        assert_eq!(risky.len(), 2);
        assert_eq!(Minerva::decode_hex("6869"), Some(b"hi".to_vec()));
        assert_eq!(Minerva::decode_hex("6"), None);
    }

    #[test]
    fn cracks_swarm_and_broadcasts() {
        let mut m = Minerva::new();
        let mut env = Environment::pristine();
        env.swarm_active = true;
        env.swarm = 50;
        let mut broadcasts = 0;
        for t in 0..10 {
            let mut ctx = Context::new(t, String::new(), false, &mut env);
            m.tick(&mut ctx);
            broadcasts += ctx
                .outbox()
                .iter()
                .filter(|o| o.topic == DEACTIVATION_TOPIC)
                .count();
        }
        assert!(!env.swarm_active);
        assert_eq!(env.swarm, 0);
        assert_eq!(broadcasts, 1);
    }
}
