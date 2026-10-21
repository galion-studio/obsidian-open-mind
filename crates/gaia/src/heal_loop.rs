// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia S3.1: `gaia heal-tick --loop`, a supervised loop around one
//! [`heal_tick`](crate::live_heal::heal_tick) per interval, for **box test
//! services only** (every tick goes through the same scope guard, nerve,
//! HADES token and restart cap as the one-shot command).
//!
//! * **Interval and jitter.** After each tick the loop sleeps
//!   `interval + U(0, jitter)` milliseconds (monotonic clock, so the
//!   2026-10-25 CEST -> CET switch does not shorten or stretch a sleep).
//!   The jitter comes from the OS random source, or from `--seed N` for a
//!   reproducible run.
//! * **Budget.** The loop stops on the first of: `--max-ticks`,
//!   `--max-runtime-s`, `--max-restarts` (restarts spawned by this loop run),
//!   `--max-errors` consecutive tick errors (exit 7), or the `--stop-file`
//!   appearing (checked between ticks and every 100 ms while sleeping). The
//!   3-per-hour restart cap is separate and global: it lives in the ledger
//!   next to hades-gate's replay ledger ([`gaia_live::heal::Budget`]).
//! * **Scope first.** A target that fails the scope guard (live port, range
//!   outside 19000-20999 or a live VPS port, forbidden program) stops the loop before the
//!   first tick with exit 4; each tick still re-checks it.
//! * **Supervised.** A tick that returns a setup error (nerve down, ledger
//!   unreadable) or panics does not end the loop; the loop backs off
//!   (`interval * 2^(errors-1)`, at most 60 s more than the interval) and
//!   tries again until `--max-errors` in a row.
//! * **Tokens.** `--token-dir D` holds pre-minted single-use hades-gate
//!   tokens (`*.tok`). A tick that needs one takes the first by name; after
//!   hades-gate consumed it (`approved`) or refused it for good (`replayed`,
//!   `expired`, `forged`, `malformed`, `wrong-scope`, `lifetime`,
//!   `token-file`) the file is renamed to `*.tok.spent` / `*.tok.refused`
//!   so the next tick takes the next one. Token contents are never printed.
//!   A token whose signed expiry has passed is renamed `*.tok.expired`
//!   before the tick (D-L3, `gaia tokens`), so it is never offered.
//! * **Hung** services (D-L4) come back as outcome `hung` (exit 8): the
//!   tick reported them to the nerve; the loop goes on, restarting nothing.
//! * **Combined cap** (D-L2): every tick counts the nerve's own restarts.
//!
//! * **Reaping.** Restarted test services are children of the loop; the
//!   loop reaps the ones that exited (`waitpid` WNOHANG, never a signal) so
//!   a crash-looping service leaves no zombies behind.
//!
//! One JSON line per tick goes to stdout; the summary is the command result.
//! The loop posts `gaia.loop.started` / `gaia.loop.stopped` on the nerve
//! (best effort; the ticks' own events are unchanged).

use crate::live_heal::{self, HealCfg, HealTick};
use gaia_core::Json;
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Exit code when `--max-errors` consecutive tick errors stopped the loop.
pub const EXIT_ERRORS: u8 = 7;
/// Longest extra back-off after errors.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Sleep slice: how often a sleeping loop looks at the stop file.
pub const SLICE: Duration = Duration::from_millis(100);
/// Most sleep values kept in the summary.
const MAX_SLEEPS_KEPT: usize = 1000;

/// Loop settings (on top of the one-shot [`HealCfg`]).
#[derive(Clone, Debug)]
pub struct LoopCfg {
    pub tick: HealCfg,
    pub interval: Duration,
    pub jitter: Duration,
    pub max_ticks: Option<u64>,
    pub max_runtime: Option<Duration>,
    pub max_restarts: Option<u64>,
    pub max_errors: u64,
    pub stop_file: Option<PathBuf>,
    pub token_dir: Option<PathBuf>,
    pub seed: Option<u64>,
}

/// Why the loop stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    MaxTicks,
    MaxRuntime,
    MaxRestarts,
    Errors,
    StopFile,
    /// The target failed the scope guard before the first tick (exit 4).
    Scope,
}

impl Stop {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Stop::MaxTicks => "max-ticks",
            Stop::MaxRuntime => "max-runtime",
            Stop::MaxRestarts => "max-restarts",
            Stop::Errors => "max-errors",
            Stop::StopFile => "stop-file",
            Stop::Scope => "scope-refused",
        }
    }
}

/// splitmix64: small, well-known mixer (public domain, Vigna); enough for
/// spreading tick times, not for anything secret.
#[derive(Clone, Debug)]
pub struct Jitter(u64);

impl Jitter {
    #[must_use]
    pub fn new(seed: Option<u64>) -> Jitter {
        let s = seed.unwrap_or_else(|| {
            hades_gate::keys::os_random::<8>()
                .map(u64::from_le_bytes)
                .unwrap_or_else(|_| fallback_seed())
        });
        Jitter(s)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish in `0..=max_ms`.
    pub fn below(&mut self, max_ms: u64) -> u64 {
        if max_ms == 0 {
            0
        } else {
            self.next_u64() % (max_ms + 1)
        }
    }
}

/// If the OS random source is unreadable: std's per-process random hasher
/// keys mixed with the pid (jitter only, never a secret).
fn fallback_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.finish()
}

/// Sleep before the next tick: interval plus jitter, or the back-off after
/// `errors` consecutive errors (no jitter subtracted, never below interval).
#[must_use]
pub fn next_sleep(cfg: &LoopCfg, errors: u64, rng: &mut Jitter) -> Duration {
    let jitter = Duration::from_millis(rng.below(cfg.jitter.as_millis() as u64));
    if errors == 0 {
        return cfg.interval + jitter;
    }
    let factor = 1u32 << (errors.min(16) as u32 - 1);
    cfg.interval
        .saturating_mul(factor)
        .min(cfg.interval + MAX_BACKOFF)
        + jitter
}

/// Codes after which a token file is never usable again.
fn token_final(code: &str) -> Option<&'static str> {
    match code {
        "approved" => Some("spent"),
        "replayed" | "expired" | "forged" | "malformed" | "wrong-scope" | "lifetime"
        | "token-file" => Some("refused"),
        _ => None,
    }
}

/// First `*.tok` file in `dir` by name.
pub fn next_token(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("tok") && p.is_file())
        .collect();
    v.sort();
    v.into_iter().next()
}

fn stop_requested(cfg: &LoopCfg) -> bool {
    cfg.stop_file.as_deref().is_some_and(Path::exists)
}

/// Sleep `d` in [`SLICE`]s; `Some(stop)` if the stop file appears or the
/// runtime budget runs out first.
fn sleep_watch(cfg: &LoopCfg, d: Duration, t0: Instant, reaped: &mut u64) -> Option<Stop> {
    let until = Instant::now() + d;
    loop {
        *reaped += gaia_live::heal::reap().0 as u64;
        if stop_requested(cfg) {
            return Some(Stop::StopFile);
        }
        if cfg.max_runtime.is_some_and(|m| t0.elapsed() >= m) {
            return Some(Stop::MaxRuntime);
        }
        let now = Instant::now();
        if now >= until {
            return None;
        }
        std::thread::sleep(SLICE.min(until - now));
    }
}

/// Loop result: summary JSON and exit code.
#[derive(Clone, Debug)]
pub struct LoopRun {
    pub exit: u8,
    pub stop: Stop,
    pub summary: Json,
}

/// Run the loop with the real tick. Per-tick lines go to `out`.
pub fn run(cfg: &LoopCfg, out: &mut dyn Write) -> LoopRun {
    run_with(cfg, out, &mut |c| live_heal::heal_tick(c))
}

/// [`run`] with an injectable tick (tests).
pub fn run_with(
    cfg: &LoopCfg,
    out: &mut dyn Write,
    tick: &mut dyn FnMut(&HealCfg) -> Result<HealTick, String>,
) -> LoopRun {
    let t0 = Instant::now();
    let started = zagreb_time::now_rfc3339();
    let mut rng = Jitter::new(cfg.seed);
    let svc = cfg.tick.target.name.clone();
    let _ = crate::live_heal::nerve_event(
        &cfg.tick,
        "gaia.loop.started",
        "info",
        json!({"service": svc, "port": cfg.tick.target.port, "interval_ms": cfg.interval.as_millis() as u64,
               "jitter_ms": cfg.jitter.as_millis() as u64, "max_ticks": cfg.max_ticks,
               "max_runtime_s": cfg.max_runtime.map(|d| d.as_secs()), "max_restarts": cfg.max_restarts,
               "max_errors": cfg.max_errors, "pid": std::process::id()}),
    );
    // Fail fast on a target that can never pass the scope guard (live port,
    // forbidden program, ...); every tick still re-checks it.
    let scope_err = gaia_live::heal::check_target(&cfg.tick.target, &cfg.tick.allowed).err();
    let (mut ticks, mut restarts, mut errors, mut errors_total, mut panics) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut outcomes: std::collections::BTreeMap<String, u64> = Default::default();
    let mut tokens_spent = 0u64;
    let mut tokens_refused = 0u64;
    let mut tokens_expired = 0u64;
    let mut sleeps: Vec<u64> = Vec::new();
    let mut last_error: Option<String> = None;
    let mut reaped = 0u64;
    let stop = loop {
        if let Some(e) = &scope_err {
            last_error = Some(e.clone());
            break Stop::Scope;
        }
        if stop_requested(cfg) {
            break Stop::StopFile;
        }
        if cfg.max_ticks.is_some_and(|m| ticks >= m) {
            break Stop::MaxTicks;
        }
        if cfg.max_runtime.is_some_and(|m| t0.elapsed() >= m) {
            break Stop::MaxRuntime;
        }
        if cfg.max_restarts.is_some_and(|m| restarts >= m) {
            break Stop::MaxRestarts;
        }
        ticks += 1;
        let mut tc = cfg.tick.clone();
        // D-L3: expired tokens are moved aside (*.tok.expired) before use.
        let token = match cfg.token_dir.as_deref() {
            Some(d) => {
                let (t, moved) = crate::tokens::next_live(d, hades_gate::approval::unix_now());
                tokens_expired += moved;
                t
            }
            None => None,
        };
        if cfg.token_dir.is_some() {
            tc.token_file = token.clone();
        }
        let at = zagreb_time::now_rfc3339();
        let began = Instant::now();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(&tc)))
            .unwrap_or_else(|p| {
                panics += 1;
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic".into());
                Err(format!("tick panicked: {msg}"))
            });
        let ms = began.elapsed().as_millis() as u64;
        reaped += gaia_live::heal::reap().0 as u64;
        let line = match &r {
            Ok(t) => {
                // D-FI3 (slot 199): a restart whose result event could not be
                // posted is still a restart (and its token is spent), but the
                // tick counts as an error for back-off and --max-errors.
                let unrecorded = t.exit == live_heal::exit::UNRECORDED
                    && t.report.get("nerve_error").and_then(Json::as_str).is_some();
                if unrecorded {
                    errors += 1;
                    errors_total += 1;
                    last_error = t
                        .report
                        .get("nerve_error")
                        .and_then(Json::as_str)
                        .map(|e| format!("restart result not recorded on the nerve: {e}"));
                } else {
                    errors = 0;
                }
                let outcome = t
                    .report
                    .get("outcome")
                    .and_then(Json::as_str)
                    .unwrap_or("?")
                    .to_string();
                *outcomes.entry(outcome.clone()).or_default() += 1;
                if t.spawned {
                    restarts += 1;
                }
                let mut moved = None;
                if let (Some(tok), Some(code)) = (&token, t.approval_code.as_deref()) {
                    if let Some(suffix) = token_final(code) {
                        let mut to = tok.as_os_str().to_owned();
                        to.push(format!(".{suffix}"));
                        if std::fs::rename(tok, &to).is_ok() {
                            if suffix == "spent" {
                                tokens_spent += 1;
                            } else {
                                tokens_refused += 1;
                            }
                            moved = Some(suffix);
                        }
                    }
                }
                json!({"tick": ticks, "at": at, "ms": ms, "exit": t.exit, "outcome": outcome,
                       "detail": t.report.get("detail").and_then(Json::as_str),
                       "approval_code": t.approval_code, "spawned": t.spawned,
                       "token_file": token.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()),
                       "token_moved": moved,
                       "nerve_error": t.report.get("nerve_error").and_then(Json::as_str).filter(|_| unrecorded),
                       "consecutive_errors": errors})
            }
            Err(e) => {
                errors += 1;
                errors_total += 1;
                *outcomes.entry("error".into()).or_default() += 1;
                last_error = Some(e.clone());
                json!({"tick": ticks, "at": at, "ms": ms, "exit": 2, "outcome": "error", "error": e,
                       "consecutive_errors": errors})
            }
        };
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
        if errors >= cfg.max_errors.max(1) {
            break Stop::Errors;
        }
        if cfg.max_ticks.is_some_and(|m| ticks >= m) {
            break Stop::MaxTicks;
        }
        if cfg.max_restarts.is_some_and(|m| restarts >= m) {
            break Stop::MaxRestarts;
        }
        let d = next_sleep(cfg, errors, &mut rng);
        if sleeps.len() < MAX_SLEEPS_KEPT {
            sleeps.push(d.as_millis() as u64);
        }
        if let Some(s) = sleep_watch(cfg, d, t0, &mut reaped) {
            break s;
        }
    };
    let exit = match stop {
        Stop::Errors => EXIT_ERRORS,
        Stop::Scope => live_heal::exit::REFUSED,
        _ => 0,
    };
    let runtime_ms = t0.elapsed().as_millis() as u64;
    let outcome_json = outcomes
        .iter()
        .fold(Json::obj(), |j, (k, v)| j.with(k.as_str(), *v));
    let _ = crate::live_heal::nerve_event(
        &cfg.tick,
        "gaia.loop.stopped",
        if exit == 0 { "info" } else { "error" },
        json!({"service": svc, "port": cfg.tick.target.port, "stop": stop.name(), "ticks": ticks,
               "restarts": restarts, "errors_total": errors_total, "panics": panics,
               "runtime_ms": runtime_ms, "outcomes": outcomes, "pid": std::process::id()}),
    );
    let summary = Json::obj()
        .with("mode", "heal-loop-test-services-only")
        .with("service", svc.as_str())
        .with("port", u64::from(cfg.tick.target.port))
        .with("started", started)
        .with("stopped", zagreb_time::now_rfc3339())
        .with("stop", stop.name())
        .with("exit", u64::from(exit))
        .with("ticks", ticks)
        .with("restarts", restarts)
        .with("errors_total", errors_total)
        .with("panics", panics)
        .with("last_error", last_error)
        .with("outcomes", outcome_json)
        .with("tokens_spent", tokens_spent)
        .with("tokens_refused", tokens_refused)
        .with("tokens_expired", tokens_expired)
        .with("interval_ms", cfg.interval.as_millis() as u64)
        .with("jitter_ms", cfg.jitter.as_millis() as u64)
        .with("seeded", cfg.seed.is_some())
        .with(
            "sleeps_ms",
            sleeps.iter().map(|s| Json::from(*s)).collect::<Vec<_>>(),
        )
        .with("children_reaped", reaped)
        .with("children_running", gaia_live::heal::reap().1 as u64)
        .with("runtime_ms", runtime_ms);
    LoopRun {
        exit,
        stop,
        summary,
    }
}

fn num<T: std::str::FromStr>(own: &[&str], k: &str) -> Result<Option<T>, String> {
    match own
        .iter()
        .position(|a| *a == k)
        .and_then(|i| own.get(i + 1))
    {
        None if own.contains(&k) => Err(format!("{k} needs a value")),
        None => Ok(None),
        Some(v) => v
            .parse()
            .map(Some)
            .map_err(|_| format!("{k} needs a number, got {v:?}")),
    }
}

/// Parse `gaia heal-tick --loop ...`. The one-shot flags are read by
/// [`live_heal::parse_cli`]; the loop flags come before `--` too.
pub fn parse_cli(args: &[&str]) -> Result<LoopCfg, String> {
    let tick = live_heal::parse_cli(args)?;
    let split = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    let own = &args[..split];
    let path = |k: &str| {
        own.iter()
            .position(|a| *a == k)
            .and_then(|i| own.get(i + 1))
            .map(PathBuf::from)
    };
    let token_dir = path("--token-dir");
    if token_dir.is_some() && tick.token_file.is_some() {
        return Err("heal-tick --loop takes --token-dir or --token-file, not both".into());
    }
    let interval_ms: u64 = num(own, "--interval-ms")?.unwrap_or(30_000);
    let jitter_ms: u64 = num(own, "--jitter-ms")?.unwrap_or(0);
    if !(100..=86_400_000).contains(&interval_ms) {
        return Err("--interval-ms must be 100..=86400000".into());
    }
    if jitter_ms > interval_ms.max(1000) * 10 {
        return Err("--jitter-ms is larger than 10 x the interval".into());
    }
    let max_ticks: Option<u64> = num(own, "--max-ticks")?;
    let max_runtime_s: Option<u64> = num(own, "--max-runtime-s")?;
    let max_restarts: Option<u64> = num(own, "--max-restarts")?;
    let max_errors: u64 = num(own, "--max-errors")?.unwrap_or(5);
    if max_ticks == Some(0) || max_errors == 0 {
        return Err("--max-ticks and --max-errors must be at least 1".into());
    }
    Ok(LoopCfg {
        tick,
        interval: Duration::from_millis(interval_ms),
        jitter: Duration::from_millis(jitter_ms),
        max_ticks,
        max_runtime: max_runtime_s.map(Duration::from_secs),
        max_restarts,
        max_errors,
        stop_file: path("--stop-file"),
        token_dir,
        seed: num(own, "--seed")?,
    })
}
