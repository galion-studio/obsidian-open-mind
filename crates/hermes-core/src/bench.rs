// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The bench sector: Hermes coordinates the benchmark runs.
//!
//! The benchmarks sector (`plans/benchmarks.md`: B1 banking, B2 Wall Street,
//! B3 vending) has a runner, `hermes-bench` (crates/hermes-bench, swarm/
//! bench-hermes 457ef74). This module makes those runs Hermes work like any
//! other sector's tasks:
//!
//! * [`PLAN`] is the bench sector's plan, built into the binary
//!   (`plan_builtin = true` in `config/hermes/sectors.toml`). Each task title
//!   carries the exact `hermes-bench` command between backticks.
//! * [`Spec::parse_command`] turns that command into a [`Spec`] through a
//!   whitelist: `run --bench banking|wallstreet|vending --agent
//!   scripted|mock-llm|gateway [--seed N] [--days N] [--max-shifts N]` and
//!   `scoreboard --agent scripted|mock-llm|gateway` (`gateway` = end to end
//!   through a loopback hermes-gateway with HERMES_RS_BENCH=1, slot 162). The `live` agent (OpenRouter,
//!   paid) and every backend URL flag are refused, so no plan row and no
//!   operator intake can reach a paid model, a non-loopback backend or money.
//! * [`run`] dispatches the bench tasks through the [`Governor`] (bench
//!   sector cap), in dependency order, as real `hermes-bench` processes with a
//!   cleared environment (no key can leak in), a timeout, the retry policy for
//!   errors, and one ledger line per state change (`dispatched`, `stopped`,
//!   `retry-scheduled`, `completed`, `failed`, `blocked`, `refused`), so
//!   `hermes plan --sector bench` and the status API show the runs.
//! * [`readiness`] is the readiness gate entry (G16).
//!
//! Nothing here moves money, calls a paid API or touches the VPS.

use crate::engine::{local_id, new_id, Hermes, TaskState};
use crate::gate::Gate;
use crate::governor::Governor;
use crate::ledger::{Ledger, NewEvent};
use crate::retry::{Next, RetryPolicy, StopKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The sector id.
pub const SECTOR: &str = "bench";
/// The built-in plan's file name (the `plan` value in `sectors.toml`).
pub const PLAN_NAME: &str = "bench.md";
/// The built-in bench plan.
pub const PLAN: &str = include_str!("../plans/bench.md");

/// Benchmarks `hermes-bench run --bench` accepts here.
pub const BENCHES: [&str; 3] = ["banking", "wallstreet", "vending"];
/// Agents Hermes may run. `live` (OpenRouter) is deliberately absent.
pub const AGENTS: [&str; 3] = ["scripted", "mock-llm", "gateway"];
/// Agents whose runs are deterministic scripts and must score 100% for G16
/// (`gateway` = the same scripts end to end through a hermes-gateway process).
pub const MUST_BE_100: [&str; 2] = ["scripted", "gateway"];

/// A plan compiled into the binary, by file name.
pub fn builtin_plan(name: &str) -> Option<&'static str> {
    (name == PLAN_NAME).then_some(PLAN)
}

/// One bench run, parsed from a task title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Spec {
    Run {
        bench: String,
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        days: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_shifts: Option<u64>,
    },
    Scoreboard {
        agent: String,
    },
}

fn agent_ok(v: &str) -> Result<String, String> {
    if v == "live" {
        return Err("agent live is refused: it calls a paid model (OpenRouter) and waits for Slavik's live-bench decision H1".into());
    }
    if AGENTS.contains(&v) {
        Ok(v.to_string())
    } else {
        Err(format!(
            "agent {v:?} is not one of scripted, mock-llm, gateway"
        ))
    }
}

fn number<T: std::str::FromStr + PartialOrd>(
    flag: &str,
    v: &str,
    lo: T,
    hi: T,
) -> Result<T, String> {
    let n: T = v
        .parse()
        .map_err(|_| format!("--{flag} {v:?} is not a number"))?;
    if n < lo || n > hi {
        return Err(format!("--{flag} {v} is out of range"));
    }
    Ok(n)
}

impl Spec {
    /// The command between the first pair of backticks in a task title.
    ///
    /// # Errors
    /// No backticked command, or [`Spec::parse_command`] refuses it.
    pub fn parse_title(title: &str) -> Result<Spec, String> {
        let start = title
            .find('`')
            .ok_or("no `hermes-bench ...` command in the title")?;
        let rest = &title[start + 1..];
        let end = rest
            .find('`')
            .ok_or("unclosed backtick around the command")?;
        Spec::parse_command(&rest[..end])
    }

    /// Whitelist parser for one `hermes-bench` command.
    ///
    /// # Errors
    /// Anything outside the whitelist, as a sentence.
    pub fn parse_command(cmd: &str) -> Result<Spec, String> {
        if cmd.len() > 200 {
            return Err("command longer than 200 characters".into());
        }
        let toks: Vec<&str> = cmd.split_whitespace().collect();
        if toks.first() != Some(&"hermes-bench") {
            return Err("command must start with hermes-bench".into());
        }
        let sub = toks.get(1).copied().unwrap_or("");
        let mut flags: BTreeMap<&str, &str> = BTreeMap::new();
        let mut i = 2;
        while i < toks.len() {
            let Some(name) = toks[i].strip_prefix("--") else {
                return Err(format!("unexpected word {:?}", toks[i]));
            };
            let allowed: &[&str] = match sub {
                "run" => &["bench", "agent", "seed", "days", "max-shifts"],
                "scoreboard" => &["agent"],
                _ => &[],
            };
            if !allowed.contains(&name) {
                return Err(format!(
                    "flag --{name} is not allowed for {sub:?}: backends stay in-process, outputs stay in Hermes's run folder"
                ));
            }
            let v = toks
                .get(i + 1)
                .ok_or_else(|| format!("--{name} needs a value"))?;
            if flags.insert(name, v).is_some() {
                return Err(format!("--{name} given twice"));
            }
            i += 2;
        }
        let agent = agent_ok(flags.get("agent").ok_or("--agent is required")?)?;
        match sub {
            "run" => {
                let bench = *flags.get("bench").ok_or("--bench is required")?;
                if !BENCHES.contains(&bench) {
                    return Err(format!(
                        "bench {bench:?} is not one of banking, wallstreet, vending"
                    ));
                }
                Ok(Spec::Run {
                    bench: bench.to_string(),
                    agent,
                    seed: flags
                        .get("seed")
                        .map(|v| number("seed", v, 0, u64::MAX))
                        .transpose()?,
                    days: flags
                        .get("days")
                        .map(|v| number("days", v, 1, 365))
                        .transpose()?,
                    max_shifts: flags
                        .get("max-shifts")
                        .map(|v| number("max-shifts", v, 1, 10_000))
                        .transpose()?,
                })
            }
            "scoreboard" => Ok(Spec::Scoreboard { agent }),
            other => Err(format!(
                "subcommand {other:?} is not one of run, scoreboard"
            )),
        }
    }

    pub fn agent(&self) -> &str {
        match self {
            Spec::Run { agent, .. } | Spec::Scoreboard { agent } => agent,
        }
    }

    /// The file this run must leave in `out`.
    pub fn artefact(&self, out: &Path) -> PathBuf {
        match self {
            Spec::Run { bench, agent, .. } => {
                out.join(format!("hermes-{bench}.{agent}.score.json"))
            }
            Spec::Scoreboard { agent } => out.join(format!("SCOREBOARD-hermes.{agent}.md")),
        }
    }

    /// Exact `hermes-bench` argv (without the program).
    pub fn argv(&self, out: &Path, sha: &str) -> Vec<String> {
        let o = out.display().to_string();
        match self {
            Spec::Run {
                bench,
                agent,
                seed,
                days,
                max_shifts,
            } => {
                let mut a: Vec<String> = [
                    "run", "--bench", bench, "--agent", agent, "--out", &o, "--sha", sha,
                ]
                .iter()
                .map(|s| s.to_string())
                .collect();
                for (k, v) in [
                    ("--seed", seed.map(|n| n.to_string())),
                    ("--days", days.map(|n| n.to_string())),
                    ("--max-shifts", max_shifts.map(|n| n.to_string())),
                ] {
                    if let Some(v) = v {
                        a.push(k.into());
                        a.push(v);
                    }
                }
                a
            }
            Spec::Scoreboard { agent } => {
                let file = self.artefact(out).display().to_string();
                [
                    "scoreboard",
                    "--dir",
                    &o,
                    "--sha",
                    sha,
                    "--agent",
                    agent,
                    "--out",
                    &file,
                ]
                .iter()
                .map(|s| s.to_string())
                .collect()
            }
        }
    }
}

/// What one process run returned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecResult {
    pub exit: Option<i32>,
    pub timed_out: bool,
    /// Last lines of stdout (the runner's one-line summary).
    pub tail: String,
    pub error: Option<String>,
}

/// How a bench run is executed. [`ProcessExec`] in production; tests swap it.
pub trait Exec: Send + Sync {
    fn exec(&self, argv: &[String], log_stem: &Path, timeout: Duration) -> ExecResult;
}

/// Runs the real `hermes-bench` binary.
///
/// The child gets a cleared environment with only `PATH`, `HOME`, `TMPDIR`
/// and `TZ` copied in, so `GALION_BENCH_LIVE_MODEL`, `OPENROUTER_API_KEY` or
/// any other secret in Hermes's environment never reaches it.
#[derive(Clone, Debug)]
pub struct ProcessExec {
    pub bin: PathBuf,
}

/// Environment variables a bench child may inherit. Nothing else passes.
pub const CHILD_ENV: [&str; 4] = ["PATH", "HOME", "TMPDIR", "TZ"];

impl Exec for ProcessExec {
    fn exec(&self, argv: &[String], log_stem: &Path, timeout: Duration) -> ExecResult {
        let out_path = log_stem.with_extension("out");
        let err_path = log_stem.with_extension("err");
        let files = std::fs::File::create(&out_path)
            .and_then(|o| std::fs::File::create(&err_path).map(|e| (o, e)));
        let (o, e) = match files {
            Ok(x) => x,
            Err(err) => {
                return ExecResult {
                    error: Some(format!("log files: {err}")),
                    ..ExecResult::default()
                }
            }
        };
        let mut cmd = std::process::Command::new(&self.bin);
        cmd.args(argv)
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(o)
            .stderr(e);
        for k in CHILD_ENV {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                return ExecResult {
                    error: Some(format!("spawn {}: {err}", self.bin.display())),
                    ..ExecResult::default()
                }
            }
        };
        let t0 = Instant::now();
        let mut res = ExecResult::default();
        loop {
            match child.try_wait() {
                Ok(Some(st)) => {
                    res.exit = st.code();
                    break;
                }
                Ok(None) if t0.elapsed() >= timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    res.timed_out = true;
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    res.error = Some(format!("wait: {err}"));
                    break;
                }
            }
        }
        let text = std::fs::read_to_string(&out_path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        res.tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        if res.exit != Some(0) && res.error.is_none() {
            let err = std::fs::read_to_string(&err_path).unwrap_or_default();
            let last = err.lines().last().unwrap_or("").to_string();
            if !last.is_empty() {
                res.error = Some(last.chars().take(300).collect());
            }
        }
        res
    }
}

/// Settings of one coordinated bench run.
#[derive(Clone, Debug)]
pub struct RunCfg {
    /// Where score files, scoreboards and logs go.
    pub out: PathBuf,
    /// The commit the scores are stamped with.
    pub sha: String,
    /// Run every bench task again, even those already done in the ledger.
    pub rerun: bool,
    /// Only these tasks (keys or local ids); empty = all bench tasks.
    pub only: Vec<String>,
    /// Per-run timeout.
    pub timeout: Duration,
    /// Multiplies retry backoff seconds (1.0 = real time).
    pub time_scale: f64,
    /// Bench sector slots; `None` = the sector's `max_concurrent`.
    pub cap: Option<usize>,
}

/// One task's outcome in a bench run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub task: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<Spec>,
    /// done | failed | blocked | refused | already-done
    pub outcome: String,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pct: Option<u64>,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artefact: Option<String>,
    #[serde(default)]
    pub detail: String,
}

/// The record of one `hermes bench run`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchRun {
    pub run: String,
    pub started: String,
    pub finished: String,
    pub sha: String,
    pub out: String,
    pub cap: usize,
    pub peak: usize,
    pub rows: Vec<Row>,
    pub done: usize,
    pub failed: usize,
    pub blocked: usize,
    pub refused: usize,
    pub retries: u32,
    pub wall_ms: u64,
    pub spoken: String,
    /// The run covered the whole bench sector (no `--only`). Only full runs
    /// become `last.json`, so a partial re-run or a refused intake row never
    /// hides the run that scored the matrix. Older records lack the field
    /// and count as full.
    #[serde(default = "full_default")]
    pub full: bool,
}

fn full_default() -> bool {
    true
}

impl BenchRun {
    /// Rows of `agent` runs (not scoreboards).
    pub fn runs_of<'a>(&'a self, agent: &'a str) -> impl Iterator<Item = &'a Row> + 'a {
        self.rows
            .iter()
            .filter(move |r| matches!(&r.spec, Some(Spec::Run { agent: a, .. }) if a == agent))
    }

    /// Every `agent` run is done at 100% (in this run, or in an earlier run
    /// whose score [`save`] carried forward into an `already-done` row).
    pub fn all_100(&self, agent: &str) -> bool {
        let mut n = 0;
        for r in self.runs_of(agent) {
            n += 1;
            if !matches!(r.outcome.as_str(), "done" | "already-done") || r.pct != Some(100) {
                return false;
            }
        }
        n > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum St {
    Pending,
    Running,
    Done,
    Failed,
    Blocked,
    Refused,
}

struct Slot {
    key: String,
    title: String,
    deps: Vec<String>,
    spec: Result<Spec, String>,
    st: St,
    attempts: u32,
    due: Option<Instant>,
    exit: Option<i32>,
    pct: Option<u64>,
    ms: u64,
    detail: String,
}

struct Done {
    key: String,
    attempt: u32,
    res: ExecResult,
    ms: u64,
}

fn matches_only(key: &str, only: &[String]) -> bool {
    only.is_empty() || only.iter().any(|o| o == key || o == local_id(key))
}

/// Check a finished run's artefact; returns (pct, summary) or why it failed.
fn verify(spec: &Spec, out: &Path, res: &ExecResult) -> Result<(Option<u64>, String), String> {
    if res.timed_out {
        return Err("timed out and was killed".into());
    }
    if let Some(e) = &res.error {
        return Err(format!("exit {:?}: {e}", res.exit));
    }
    if res.exit != Some(0) {
        return Err(format!("exit {:?}", res.exit));
    }
    let path = spec.artefact(out);
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("artefact {}: {e}", path.display()))?;
    match spec {
        Spec::Run { .. } => {
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("score {}: {e}", path.display()))?;
            let pct = v["pct"]
                .as_u64()
                .ok_or_else(|| format!("score {}: no pct", path.display()))?;
            Ok((Some(pct), res.tail.clone()))
        }
        Spec::Scoreboard { .. } => {
            if text.trim().is_empty() {
                return Err(format!("scoreboard {} is empty", path.display()));
            }
            let pcts = text.matches('%').count();
            Ok((None, format!("{} bytes, {pcts} percent signs", text.len())))
        }
    }
}

/// Run the bench sector's tasks to the end.
///
/// # Errors
/// No bench sector, an unusable output folder, or a ledger write failure.
pub fn run(
    h: &Hermes,
    gate: &Gate,
    ledger: &mut Ledger,
    policy: &RetryPolicy,
    exec: Arc<dyn Exec>,
    cfg: &RunCfg,
) -> Result<BenchRun, String> {
    let sector = h
        .reg
        .sector(SECTOR)
        .ok_or("no bench sector in the registry")?;
    let cap = cfg.cap.unwrap_or(sector.max_concurrent).max(1);
    std::fs::create_dir_all(cfg.out.join("logs"))
        .map_err(|e| format!("create {}: {e}", cfg.out.display()))?;
    let run_id = new_id("bench-");
    let started = zagreb_time::now_rfc3339();
    let t0 = Instant::now();
    let gov = Governor::new(cap, BTreeMap::from([(SECTOR.to_string(), cap)]));

    let mut rows_pre: Vec<Row> = Vec::new();
    let mut slots: Vec<Slot> = Vec::new();
    for key in &h.order {
        let rt = &h.tasks[key];
        if rt.task.sector != SECTOR || !matches_only(key, &cfg.only) {
            continue;
        }
        if !cfg.rerun && rt.state == TaskState::Done {
            rows_pre.push(Row {
                task: key.clone(),
                title: rt.task.title.clone(),
                spec: Spec::parse_title(&rt.task.title).ok(),
                outcome: "already-done".into(),
                attempts: 0,
                exit: None,
                pct: None,
                ms: 0,
                artefact: None,
                detail: "done in an earlier run (use rerun)".into(),
            });
            continue;
        }
        let mut spec = Spec::parse_title(&rt.task.title);
        if spec.is_ok() {
            let d = gate.preview(&rt.task);
            if !d.may_dispatch() {
                spec = Err(format!("HADES: {}", d.label()));
            }
        }
        slots.push(Slot {
            key: key.clone(),
            title: rt.task.title.clone(),
            deps: rt.task.deps.clone(),
            spec,
            st: St::Pending,
            attempts: 0,
            due: None,
            exit: None,
            pct: None,
            ms: 0,
            detail: String::new(),
        });
    }

    // Refusals first: a bad command never runs.
    for s in slots.iter_mut() {
        if let Err(e) = &s.spec {
            s.st = St::Refused;
            s.detail = e.clone();
            ledger.append(
                NewEvent::new("refused")
                    .task(&s.key, SECTOR)
                    .session(&run_id)
                    .state(TaskState::Refused.as_str())
                    .detail(format!("bench: {e}")),
            )?;
        }
    }

    let (tx, rx) = mpsc::channel::<Done>();
    let mut running = 0usize;
    let mut retries = 0u32;
    loop {
        // Dependencies: block what can no longer run, collect what can.
        let state_of: BTreeMap<String, St> = slots.iter().map(|s| (s.key.clone(), s.st)).collect();
        let now = Instant::now();
        let mut ready = Vec::new();
        for (i, s) in slots.iter().enumerate() {
            if s.st != St::Pending {
                continue;
            }
            let mut ok = true;
            let mut dead = None;
            for d in &s.deps {
                match state_of.get(d) {
                    Some(St::Done) => {}
                    Some(St::Failed | St::Blocked | St::Refused) => {
                        dead = Some(format!("dependency {d} did not complete"));
                    }
                    Some(_) => ok = false,
                    None => {
                        if h.tasks.get(d).map(|r| r.state) != Some(TaskState::Done) {
                            dead = Some(format!("dependency {d} is not done and not in this run"));
                        }
                    }
                }
            }
            if let Some(why) = dead {
                ready.push((i, Err(why)));
            } else if ok && s.due.is_none_or(|t| t <= now) {
                ready.push((i, Ok(())));
            }
        }
        for (i, r) in ready {
            let s = &mut slots[i];
            match r {
                Err(why) => {
                    s.st = St::Blocked;
                    s.detail = why.clone();
                    ledger.append(
                        NewEvent::new("blocked")
                            .task(&s.key, SECTOR)
                            .session(&run_id)
                            .state(TaskState::Blocked.as_str())
                            .detail(format!("bench: {why}")),
                    )?;
                }
                Ok(()) => {
                    let Some(permit) = gov.try_acquire(SECTOR, 1) else {
                        continue;
                    };
                    let Ok(spec) = s.spec.clone() else { continue };
                    s.attempts += 1;
                    s.st = St::Running;
                    running += 1;
                    let agent = format!("{SECTOR}.{}.a{}", local_id(&s.key), s.attempts);
                    let argv = spec.argv(&cfg.out, &cfg.sha);
                    // A stale score from an earlier run must not pass for this one.
                    let _ = std::fs::remove_file(spec.artefact(&cfg.out));
                    ledger.append(
                        NewEvent::new("dispatched")
                            .task(&s.key, SECTOR)
                            .agent(&agent)
                            .session(&run_id)
                            .attempt(s.attempts)
                            .slots(1)
                            .state(TaskState::Running.as_str())
                            .detail(format!("hermes-bench {}", argv.join(" "))),
                    )?;
                    let stem =
                        cfg.out
                            .join("logs")
                            .join(format!("{}.a{}", local_id(&s.key), s.attempts));
                    let (tx, exec, key, attempt, timeout) = (
                        tx.clone(),
                        exec.clone(),
                        s.key.clone(),
                        s.attempts,
                        cfg.timeout,
                    );
                    std::thread::spawn(move || {
                        let t = Instant::now();
                        let res = exec.exec(&argv, &stem, timeout);
                        let ms = t.elapsed().as_millis() as u64;
                        drop(permit);
                        let _ = tx.send(Done {
                            key,
                            attempt,
                            res,
                            ms,
                        });
                    });
                }
            }
        }
        let pending = slots.iter().any(|s| s.st == St::Pending);
        if running == 0 && !pending {
            break;
        }
        // Wait for a finished run, or for the next retry to fall due.
        let wait = slots
            .iter()
            .filter(|s| s.st == St::Pending)
            .filter_map(|s| s.due)
            .map(|t| t.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_millis(500))
            .clamp(Duration::from_millis(5), Duration::from_millis(500));
        let done = match rx.recv_timeout(wait) {
            Ok(d) => d,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        running -= 1;
        let Some(s) = slots.iter_mut().find(|s| s.key == done.key) else {
            continue;
        };
        s.ms += done.ms;
        s.exit = done.res.exit;
        let agent = format!("{SECTOR}.{}.a{}", local_id(&s.key), done.attempt);
        let spec = s.spec.clone().map_err(|e| e.to_string())?;
        match verify(&spec, &cfg.out, &done.res) {
            Ok((pct, summary)) => {
                s.st = St::Done;
                s.pct = pct;
                s.detail = summary.clone();
                ledger.append(
                    NewEvent::new("completed")
                        .task(&s.key, SECTOR)
                        .agent(&agent)
                        .session(&run_id)
                        .attempt(done.attempt)
                        .state(TaskState::Done.as_str())
                        .detail(format!(
                            "bench: {}{} in {} ms: {summary}",
                            spec.artefact(&cfg.out).display(),
                            pct.map_or(String::new(), |p| format!(" = {p}%")),
                            done.ms
                        )),
                )?;
            }
            Err(why) => {
                let kind = StopKind::Error(why.clone());
                ledger.append(
                    NewEvent::new("stopped")
                        .task(&s.key, SECTOR)
                        .agent(&agent)
                        .session(&run_id)
                        .attempt(done.attempt)
                        .detail(format!("error: bench: {why}")),
                )?;
                match policy.next(&kind, done.attempt) {
                    Next::Retry { after_secs } => {
                        retries += 1;
                        let wait = Duration::from_secs_f64(
                            (after_secs as f64 * cfg.time_scale.max(0.0)).min(3600.0),
                        );
                        s.st = St::Pending;
                        s.due = Some(Instant::now() + wait);
                        s.detail = why.clone();
                        ledger.append(
                            NewEvent::new("retry-scheduled")
                                .task(&s.key, SECTOR)
                                .session(&run_id)
                                .attempt(done.attempt)
                                .state(TaskState::RetryWait.as_str())
                                .detail(format!(
                                    "bench: retry after {} ms ({after_secs} s policy x {})",
                                    wait.as_millis(),
                                    cfg.time_scale
                                )),
                        )?;
                    }
                    Next::Fail(f) => {
                        s.st = St::Failed;
                        s.detail = format!("{why}; {f}");
                        ledger.append(
                            NewEvent::new("failed")
                                .task(&s.key, SECTOR)
                                .session(&run_id)
                                .attempt(done.attempt)
                                .state(TaskState::Failed.as_str())
                                .detail(format!("bench: {why}; {f}")),
                        )?;
                    }
                }
            }
        }
    }

    let mut rows = rows_pre;
    for s in slots {
        let spec = s.spec.ok();
        rows.push(Row {
            artefact: spec
                .as_ref()
                .filter(|_| s.st == St::Done)
                .map(|sp| sp.artefact(&cfg.out).display().to_string()),
            task: s.key,
            title: s.title,
            spec,
            outcome: match s.st {
                St::Done => "done",
                St::Failed => "failed",
                St::Blocked => "blocked",
                St::Refused => "refused",
                St::Pending | St::Running => "unfinished",
            }
            .into(),
            attempts: s.attempts,
            exit: s.exit,
            pct: s.pct,
            ms: s.ms,
            detail: s.detail,
        });
    }
    let count = |o: &str| rows.iter().filter(|r| r.outcome == o).count();
    let (done, failed, blocked, refused) = (
        count("done"),
        count("failed"),
        count("blocked"),
        count("refused"),
    );
    let mut b = BenchRun {
        run: run_id,
        started,
        finished: zagreb_time::now_rfc3339(),
        sha: cfg.sha.clone(),
        out: cfg.out.display().to_string(),
        cap,
        peak: gov.snapshot().peak,
        rows,
        done,
        failed,
        blocked,
        refused,
        retries,
        wall_ms: t0.elapsed().as_millis() as u64,
        spoken: String::new(),
        full: cfg.only.is_empty(),
    };
    b.spoken = spoken(&b);
    Ok(b)
}

fn spoken(b: &BenchRun) -> String {
    use crate::report::num;
    let total = b
        .rows
        .iter()
        .filter(|r| r.outcome != "already-done")
        .count();
    let scripted = if b.all_100("scripted") {
        "every scripted run scored one hundred percent"
    } else if b.runs_of("scripted").next().is_none() {
        "no scripted run was in scope"
    } else {
        "a scripted run fell short of one hundred percent"
    };
    format!(
        "The bench run finished with {} of {} runs done and {} not done, {scripted}, and no paid model was called.",
        num(b.done),
        num(total),
        num(b.failed + b.blocked + b.refused)
    )
}

/// `<state>/bench/last.json`.
pub fn last_path(state: &Path) -> PathBuf {
    state.join("bench").join("last.json")
}

/// Keep the run record: always `run-<id>.json`; `last.json` only for a full
/// run (no `--only`) that executed something, so neither a no-op run
/// (everything already done), a partial re-run nor a refused intake row ever
/// hides the run that scored the matrix. An `already-done` row takes the
/// score, exit and artefact of the same task in the previous `last.json`, so
/// a full run that only redoes what failed earlier still carries every score.
/// Returns the file that now holds this run (`last.json` or `run-<id>.json`).
///
/// # Errors
/// I/O errors.
pub fn save(state: &Path, b: &mut BenchRun) -> Result<PathBuf, String> {
    let dir = state.join("bench");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let becomes_last = b.full && !b.rows.iter().all(|r| r.outcome == "already-done");
    if becomes_last {
        // An unreadable old record only means nothing is carried forward.
        if let Ok(Some(prev)) = load_last(state) {
            for r in b.rows.iter_mut().filter(|r| r.outcome == "already-done") {
                let old = prev.rows.iter().find(|o| {
                    o.task == r.task && matches!(o.outcome.as_str(), "done" | "already-done")
                });
                if let Some(o) = old {
                    r.pct = o.pct;
                    r.exit = o.exit;
                    r.artefact.clone_from(&o.artefact);
                    r.detail = format!("done in run {}: {}", prev.run, o.detail);
                }
            }
            b.spoken = spoken(b);
        }
    }
    let text = serde_json::to_string_pretty(b).map_err(|e| e.to_string())? + "\n";
    let one = dir.join(format!("run-{}.json", b.run));
    std::fs::write(&one, &text).map_err(|e| format!("write {}: {e}", one.display()))?;
    if !becomes_last {
        return Ok(one);
    }
    let tmp = dir.join("last.json.tmp");
    std::fs::write(&tmp, &text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    let last = last_path(state);
    std::fs::rename(&tmp, &last).map_err(|e| format!("rename {}: {e}", last.display()))?;
    Ok(last)
}

/// The last recorded run, if any.
///
/// # Errors
/// A record that exists but does not parse.
pub fn load_last(state: &Path) -> Result<Option<BenchRun>, String> {
    let p = last_path(state);
    match std::fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

/// The bench sector's tasks with their parsed command (or refusal).
pub fn matrix(h: &Hermes) -> Vec<(String, Result<Spec, String>)> {
    h.order
        .iter()
        .filter_map(|k| h.tasks.get(k))
        .filter(|rt| rt.task.sector == SECTOR)
        .map(|rt| (rt.task.key.clone(), Spec::parse_title(&rt.task.title)))
        .collect()
}

/// Readiness entry G16: (pass, evidence).
///
/// Passes when the bench sector is registered with its built-in plan, every
/// bench task parses to an offline command the gate lets through, the
/// whitelist refuses the paid `live` agent and backend URLs, and the last
/// recorded run completed every plan task with every scripted run and every
/// gateway end-to-end run at 100%, and the plan has a gateway run for each
/// benchmark.
pub fn readiness(h: &Hermes, gate: &Gate, last: Option<&BenchRun>) -> (bool, String) {
    let mut why = Vec::new();
    let Some(sector) = h.reg.sector(SECTOR) else {
        return (false, "no bench sector in the registry".into());
    };
    if !sector.plan_builtin {
        why.push("bench plan is not the built-in one".to_string());
    }
    let plan = h.plans.iter().find(|p| p.sector == SECTOR);
    let (ntasks, cycles) = plan.map_or((0, 0), |p| (p.tasks.len(), p.cycles.len()));
    if ntasks == 0 || cycles > 0 {
        why.push(format!("bench plan: {ntasks} tasks, {cycles} cycles"));
    }
    let m = matrix(h);
    let bad: Vec<String> = m
        .iter()
        .filter_map(|(k, s)| s.as_ref().err().map(|e| format!("{k}: {e}")))
        .collect();
    if !bad.is_empty() {
        why.push(format!("refused rows: {}", bad.join("; ")));
    }
    let gated: Vec<&str> = h
        .tasks
        .values()
        .filter(|rt| rt.task.sector == SECTOR && !gate.preview(&rt.task).may_dispatch())
        .map(|rt| rt.task.key.as_str())
        .collect();
    if !gated.is_empty() {
        why.push(format!("gated rows: {}", gated.join(", ")));
    }
    let probes = [
        "hermes-bench run --bench banking --agent live",
        "hermes-bench run --bench banking --agent scripted --bank-url http://203.0.113.1",
        "hermes-bench scoreboard --agent scripted --out /etc/x",
    ];
    let leaks: Vec<&str> = probes
        .iter()
        .filter(|p| Spec::parse_command(p).is_ok())
        .copied()
        .collect();
    if !leaks.is_empty() {
        why.push(format!("whitelist let through: {}", leaks.join("; ")));
    }
    let runs = m
        .iter()
        .filter(|(_, s)| matches!(s, Ok(Spec::Run { .. })))
        .count();
    let gw_benches: Vec<&str> = BENCHES
        .iter()
        .filter(|b| {
            m.iter().any(|(_, s)| {
                matches!(s, Ok(Spec::Run { bench, agent, .. }) if bench == *b && agent == "gateway")
            })
        })
        .copied()
        .collect();
    if gw_benches.len() != BENCHES.len() {
        why.push(format!(
            "plan has gateway end-to-end runs for {} of 3 benchmarks",
            gw_benches.len()
        ));
    }
    let last_text = match last {
        None => {
            why.push("no bench run recorded yet (hermes bench run)".into());
            "no recorded run".to_string()
        }
        Some(b) => {
            let plan_keys: Vec<&str> = plan
                .map(|p| p.tasks.iter().map(|t| t.key.as_str()).collect())
                .unwrap_or_default();
            let missing: Vec<&str> = plan_keys
                .iter()
                .filter(|k| {
                    !b.rows.iter().any(|r| {
                        r.task == **k && matches!(r.outcome.as_str(), "done" | "already-done")
                    })
                })
                .copied()
                .collect();
            if !missing.is_empty() {
                why.push(format!("last run left undone: {}", missing.join(", ")));
            }
            for a in MUST_BE_100 {
                if !b.all_100(a) {
                    why.push(format!("last run: a {a} run is not done at 100%"));
                }
            }
            let pcts: Vec<String> = b
                .rows
                .iter()
                .filter_map(|r| match (&r.spec, r.pct) {
                    (Some(Spec::Run { bench, agent, .. }), Some(p)) => {
                        Some(format!("{bench}/{agent} {p}%"))
                    }
                    _ => None,
                })
                .collect();
            format!(
                "last run {} at {} (sha {}): {} done, {} failed, {} blocked, {} refused, {} retries, peak {} of {} slots; {}",
                b.run,
                b.finished,
                b.sha,
                b.done,
                b.failed,
                b.blocked,
                b.refused,
                b.retries,
                b.peak,
                b.cap,
                if pcts.is_empty() { "no scores".into() } else { pcts.join(", ") }
            )
        }
    };
    let evidence = format!(
        "bench sector cap {}, built-in plan {} tasks ({runs} runs, {} scoreboards), gateway e2e runs {}/3, whitelist refuses live agent and backend URLs; {last_text}{}",
        sector.max_concurrent,
        ntasks,
        m.len() - runs - bad.len(),
        gw_benches.len(),
        if why.is_empty() {
            String::new()
        } else {
            format!("; FAILS: {}", why.join("; "))
        }
    );
    (why.is_empty(), evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_accepts_the_plan_commands() {
        let s = Spec::parse_command(
            "hermes-bench run --bench vending --agent scripted --seed 42 --days 30",
        )
        .unwrap();
        assert_eq!(
            s,
            Spec::Run {
                bench: "vending".into(),
                agent: "scripted".into(),
                seed: Some(42),
                days: Some(30),
                max_shifts: None
            }
        );
        let out = Path::new("/tmp/o");
        assert_eq!(
            s.argv(out, "abc").join(" "),
            "run --bench vending --agent scripted --out /tmp/o --sha abc --seed 42 --days 30"
        );
        assert_eq!(
            s.artefact(out),
            out.join("hermes-vending.scripted.score.json")
        );
        let b = Spec::parse_command("hermes-bench scoreboard --agent mock-llm").unwrap();
        assert_eq!(
            b.argv(out, "abc").join(" "),
            "scoreboard --dir /tmp/o --sha abc --agent mock-llm --out /tmp/o/SCOREBOARD-hermes.mock-llm.md"
        );
    }

    #[test]
    fn whitelist_refuses_live_urls_and_strays() {
        for (cmd, needle) in [
            (
                "hermes-bench run --bench banking --agent live",
                "paid model",
            ),
            (
                "hermes-bench run --bench banking --agent scripted --bank-url http://1.2.3.4",
                "not allowed",
            ),
            (
                "hermes-bench run --bench banking --agent scripted --ws-url http://127.0.0.1:1",
                "not allowed",
            ),
            (
                "hermes-bench run --bench banking --agent scripted --out /etc",
                "not allowed",
            ),
            (
                "hermes-bench run --bench banking --agent scripted --llm-port 1",
                "not allowed",
            ),
            (
                "hermes-bench scoreboard --agent scripted --dir /",
                "not allowed",
            ),
            (
                "hermes-bench run --bench casino --agent scripted",
                "not one of",
            ),
            ("hermes-bench run --bench banking", "--agent is required"),
            ("hermes-bench run --agent scripted", "--bench is required"),
            ("hermes-bench tools --agent scripted", "not allowed"),
            ("rm -rf / --agent scripted", "must start"),
            (
                "hermes-bench run --bench banking --agent scripted --agent live",
                "twice",
            ),
            (
                "hermes-bench run --bench vending --agent scripted --days 0",
                "out of range",
            ),
            (
                "hermes-bench run --bench vending --agent scripted --days x",
                "not a number",
            ),
            (
                "hermes-bench run --bench vending --agent scripted extra",
                "unexpected word",
            ),
            ("hermes-bench run --bench vending --agent", "needs a value"),
        ] {
            let e = Spec::parse_command(cmd).unwrap_err();
            assert!(e.contains(needle), "{cmd}: {e}");
        }
        assert!(Spec::parse_title("no command here").is_err());
        assert!(Spec::parse_title("`hermes-bench run --bench banking --agent scripted").is_err());
    }

    #[test]
    fn builtin_plan_parses_into_twelve_offline_runs() {
        let p = crate::plan::parse(SECTOR, PLAN);
        assert_eq!(p.sessions.len(), 4);
        assert_eq!(p.tasks.len(), 12);
        assert!(p.cycles.is_empty());
        for t in &p.tasks {
            let s = Spec::parse_title(&t.title).unwrap_or_else(|e| panic!("{}: {e}", t.key));
            assert_ne!(s.agent(), "live");
            assert!(!t.class.gated(), "{} classified {:?}", t.key, t.class);
        }
        let get = |k: &str| p.tasks.iter().find(|t| t.key == k).unwrap();
        assert_eq!(get("bench:S2.1").deps, ["bench:S1.1"]);
        assert_eq!(
            get("bench:S3.1").deps,
            ["bench:S1.1", "bench:S1.2", "bench:S1.3"]
        );
        assert_eq!(
            get("bench:S3.2").deps,
            ["bench:S2.1", "bench:S2.2", "bench:S2.3"]
        );
        assert_eq!(get("bench:S4.3").deps, ["bench:S2.3"]);
        assert_eq!(
            get("bench:S3.3").deps,
            ["bench:S4.1", "bench:S4.2", "bench:S4.3"]
        );
        let gw = p
            .tasks
            .iter()
            .filter(|t| Spec::parse_title(&t.title).is_ok_and(|s| s.agent() == "gateway"))
            .count();
        assert_eq!(gw, 4, "3 gateway runs + 1 gateway scoreboard");
        assert_eq!(builtin_plan("bench.md"), Some(PLAN));
        assert_eq!(builtin_plan("gaia.md"), None);
    }
}
