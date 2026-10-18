// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! HEPHAESTUS: fabrication. In the story it designs and builds every machine
//! in the Cauldrons. Here it owns builds, release artifacts, the sandboxed
//! terminal, desk tools, the omnode sandbox runner, the console build,
//! crafting and the deploy switch. It produces command lines, manifests and
//! plans; it never starts a process.

use gaia_core::{
    scaled, Context, Directive, FunctionId, Health, Json, Outcome, Report, SubordinateFunction,
    System, Verb,
};

/// UNIVERSAL-CPU.md tier 1: must build.
pub const TIER1: &[&str] = &["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"];
/// UNIVERSAL-CPU.md tier 2: must pass `cargo check`.
pub const TIER2: &[&str] = &["aarch64-linux-android", "armv7-unknown-linux-musleabihf"];
/// Artifacts kept at most.
pub const MAX_ARTIFACTS: usize = 512;

const ALLOWED: &[&str] = &[
    "cargo", "ls", "cat", "rg", "git", "echo", "wc", "head", "date",
];
const GIT_READ: &[&str] = &["status", "log", "diff", "show", "branch"];
/// Desk tools registered when none are named.
const DESK_TOOLS: &[&str] = &[
    "notes",
    "pod-view",
    "grinder",
    "link-json",
    "splash",
    "health",
];
const META: &[&str] = &[";", "|", "&", ">", "<", "`", "$("];

#[derive(Debug, Default)]
pub struct Hephaestus {
    artifacts: Vec<String>,
    tools: Vec<String>,
    commands_reviewed: u64,
    commands_refused: u64,
}

impl Hephaestus {
    pub fn new() -> Hephaestus {
        Hephaestus::default()
    }

    pub fn artifacts(&self) -> &[String] {
        &self.artifacts
    }

    /// Review a terminal command for the sandbox. `Ok` means it would be
    /// handed to openmind-sandbox; nothing is executed here.
    pub fn review_command(cmd: &str) -> Result<(), String> {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return Err("empty command".into());
        }
        if let Some(m) = META.iter().find(|m| cmd.contains(*m)) {
            return Err(format!("shell metacharacter {m:?} not allowed"));
        }
        let mut words = cmd.split_whitespace();
        let prog = words.next().unwrap_or("");
        if !ALLOWED.contains(&prog) {
            return Err(format!("{prog:?} is not on the allow-list"));
        }
        if prog == "git" && !words.next().is_some_and(|w| GIT_READ.contains(&w)) {
            return Err("only read-only git subcommands".into());
        }
        Ok(())
    }

    /// Build and check command lines for one crate on every target.
    pub fn build_plan(krate: &str) -> Vec<String> {
        let base = "CARGO_TARGET_DIR=/workspace/oom-target/gaia nice -n 19 cargo";
        let mut v = Vec::new();
        for t in TIER1 {
            v.push(format!(
                "{base} build --release -j 2 -p {krate} --target {t}"
            ));
        }
        for t in TIER2 {
            v.push(format!("{base} check -j 2 -p {krate} --target {t}"));
        }
        v
    }

    fn add_artifact(&mut self, a: String, ctx: &mut Context) {
        if self.artifacts.len() < MAX_ARTIFACTS {
            ctx.birth("artifact", a.clone());
            self.artifacts.push(a);
            ctx.env.artifacts = self.artifacts.len() as u32;
        }
    }
}

impl SubordinateFunction for Hephaestus {
    fn id(&self) -> FunctionId {
        FunctionId::Hephaestus
    }

    fn systems(&self) -> Vec<System> {
        vec![
            System::Terminal,
            System::CargoBuild,
            System::ReleaseArtifacts,
            System::GalionDesk,
            System::OmnodeSandbox,
            System::Crafting,
            System::Console,
            System::DeploySwitch,
        ]
    }

    fn health(&self) -> Health {
        Health::ok((self.artifacts.len() * 100 / MAX_ARTIFACTS) as u8)
    }

    fn handle(&mut self, d: &Directive, ctx: &mut Context) -> Report {
        let done = |s: String| Report::for_directive(d, Outcome::Done).summary(s);
        if d.verb == Verb::Status {
            return done(format!(
                "{} artifacts, {} tools",
                self.artifacts.len(),
                self.tools.len()
            ));
        }
        match d.target {
            System::Terminal => {
                self.commands_reviewed += 1;
                let cmd = if d.payload.is_empty() {
                    "cargo --version"
                } else {
                    d.payload.as_str()
                };
                match Hephaestus::review_command(cmd) {
                    Ok(()) => done(format!(
                        "{cmd:?} accepted for openmind-sandbox (not executed here)"
                    )),
                    Err(e) => {
                        self.commands_refused += 1;
                        Report::for_directive(d, Outcome::Refused).summary(e)
                    }
                }
            }
            System::CargoBuild => {
                let crates: Vec<&str> = if d.payload.is_empty() {
                    vec!["gaia"]
                } else {
                    d.payload
                        .split(',')
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .collect()
                };
                let mut plan = Vec::new();
                for c in &crates {
                    plan.extend(Hephaestus::build_plan(c));
                    for t in TIER1 {
                        self.add_artifact(format!("{c}@{t}"), ctx);
                    }
                }
                done(format!(
                    "{} build/check lines for {} crate(s)",
                    plan.len(),
                    crates.len()
                ))
                .metric("lines", plan.len() as i64)
                .plan(plan)
            }
            System::ReleaseArtifacts => {
                done(format!("manifest of {} artifact(s)", self.artifacts.len()))
                    .metric("artifacts", self.artifacts.len() as i64)
                    .plan(self.artifacts.iter().take(16).cloned().collect())
            }
            System::GalionDesk => {
                let given = if d.payload.is_empty() {
                    DESK_TOOLS[..(scaled(DESK_TOOLS.len() as u64, d.intensity) as usize)].join(",")
                } else {
                    d.payload.clone()
                };
                for t in given.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                    if !self.tools.iter().any(|x| x == t) && self.tools.len() < 64 {
                        self.tools.push(t.to_string());
                    }
                }
                done(format!("{} desk tool(s) registered", self.tools.len()))
                    .metric("tools", self.tools.len() as i64)
            }
            System::OmnodeSandbox => {
                let cpu = scaled(60, d.intensity);
                let mem = scaled(512, d.intensity);
                done("sandbox job plan".into())
                    .metric("cpu_secs", cpu as i64)
                    .metric("mem_mb", mem as i64)
                    .plan(vec![
                        format!("rlimit cpu {cpu}s, address space {mem} MB"),
                        "cleared env, fixed workdir, wall timeout".into(),
                        "Landlock when the kernel has it".into(),
                    ])
            }
            System::Crafting => {
                let n = scaled(5, d.intensity);
                for i in 0..n {
                    self.add_artifact(format!("cauldron-machine-{}-{i}", ctx.tick), ctx);
                }
                done(format!("cauldron forged {n} machine(s)")).metric("forged", n as i64)
            }
            System::Console => done("console production build plan".into()).plan(vec![
                "copy the console source into stage/ (never the live tree)".into(),
                "vite build in the copy".into(),
                "never `npm run build`: it migrates the live database".into(),
            ]),
            System::DeploySwitch => {
                let mut r = Report::for_directive(d, Outcome::DryRun)
                    .summary("deploy switch is plan-only; the owner runs deploy/switch.sh")
                    .plan(vec![
                        "verify the new release passed fmt, clippy, tests, repo-check".into(),
                        "owner runs deploy/switch.sh".into(),
                        "if /health fails, HADES rollback plan".into(),
                    ]);
                r.dry_run = true;
                r
            }
            other => Report::for_directive(d, Outcome::Rejected)
                .summary(format!("{other} is not a HEPHAESTUS system")),
        }
    }

    fn reset(&mut self) {
        self.commands_reviewed = 0;
        self.commands_refused = 0;
    }

    fn state_json(&self) -> Json {
        Json::obj()
            .with("artifacts", self.artifacts.len())
            .with("tools", self.tools.len())
            .with("commands_reviewed", self.commands_reviewed)
            .with("commands_refused", self.commands_refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::Environment;

    #[test]
    fn declares_exactly_its_systems() {
        assert_eq!(
            Hephaestus::new().systems(),
            FunctionId::Hephaestus.systems()
        );
    }

    #[test]
    fn terminal_review() {
        assert!(Hephaestus::review_command("cargo test -p gaia-core").is_ok());
        assert!(Hephaestus::review_command("git status").is_ok());
        assert!(Hephaestus::review_command("git push").is_err());
        assert!(Hephaestus::review_command("rm -rf /").is_err());
        assert!(Hephaestus::review_command("ls; rm x").is_err());
        assert!(Hephaestus::review_command("curl http://x").is_err());
    }

    #[test]
    fn build_plan_covers_all_targets_and_records_artifacts() {
        let mut h = Hephaestus::new();
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        let r = h.handle(
            &Directive::new(1, System::CargoBuild, Verb::Run).payload("gaia,gaia-core"),
            &mut ctx,
        );
        assert_eq!(r.plan.len(), 8);
        assert!(r
            .plan
            .iter()
            .all(|l| l.contains("-j 2") && l.contains("nice -n 19")));
        assert_eq!(ctx.births().len(), 4);
        assert_eq!(env.artifacts, 4);
    }

    #[test]
    fn deploy_switch_is_plan_only() {
        let mut h = Hephaestus::new();
        let mut env = Environment::pristine();
        let mut ctx = Context::new(1, String::new(), false, &mut env);
        let r = h.handle(
            &Directive::new(1, System::DeploySwitch, Verb::Execute),
            &mut ctx,
        );
        assert!(r.dry_run);
    }
}
