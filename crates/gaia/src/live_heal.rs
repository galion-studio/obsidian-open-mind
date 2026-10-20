// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The first end-to-end Gaia path (M2), for **box-local test services only**:
//!
//! 1. probe the test service (read-only GET) and record a `service.probe`
//!    nerve event over loopback;
//! 2. GAIA core reads that event back from the nerve (`GET /events`) and
//!    takes it as its input signal; nothing proceeds unless the nerve has it;
//! 3. DEMETER answers with heal-dry (healer) and a guardian-tick health cycle;
//! 4. HADES produces the heal-apply plan (dry-run), then gets the `Execute`
//!    directive, approved only if the approval file passes
//!    [`gaia_live::heal::check_approval`] and its Alpha is registered (GAIA
//!    core binds the approval to the directive id and target);
//! 5. the healer applies the released plan: one restart of the test service,
//!    if the 3-per-hour cap allows it, then a health check;
//! 6. the result is recorded as a `gaia.heal.*` nerve event.
//!
//! [`gaia_live::heal::check_target`] runs first, so live stack ports and
//! service managers are refused before anything else happens.

use crate::{booted, send};
use gaia_core::{Json, Outcome, System, Verb};
use gaia_live::heal::{self, Approval, Ledger, TestTarget};
use gaia_live::nerve;
use serde_json::json;
use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Duration;

/// Source name on every nerve event this path writes.
pub const SOURCE: &str = "gaia-live";

/// Exit codes of `gaia heal-tick`.
pub mod exit {
    /// Healthy (nothing to do) or restarted and verified up.
    pub const OK: u8 = 0;
    /// Refused: bad target, missing or wrong approval.
    pub const REFUSED: u8 = 4;
    /// The 3-restarts-per-hour cap is spent.
    pub const CAPPED: u8 = 5;
    /// Restarted, but it did not come back up in time (or spawn failed).
    pub const APPLY_FAILED: u8 = 6;
}

#[derive(Clone, Debug)]
pub struct HealCfg {
    pub target: TestTarget,
    pub allowed: RangeInclusive<u16>,
    pub nerve: SocketAddr,
    pub state_dir: PathBuf,
    pub trust_file: PathBuf,
    pub approval_file: Option<PathBuf>,
    pub wait: Duration,
}

/// Outcome of one tick.
#[derive(Clone, Debug)]
pub struct HealTick {
    pub exit: u8,
    pub report: Json,
}

fn nerve_post(cfg: &HealCfg, kind: &str, sev: &str, data: serde_json::Value) -> Result<(), String> {
    nerve::post(cfg.nerve, SOURCE, kind, sev, data)
}

/// Record a refusal on the nerve (best effort) and build the result.
fn refuse(cfg: &HealCfg, steps: Vec<Json>, stage: &str, why: &str, probe_id: &str) -> HealTick {
    let posted = nerve_post(
        cfg,
        "gaia.heal.refused",
        "warn",
        json!({"service": cfg.target.name, "port": cfg.target.port, "stage": stage, "reason": why, "probe_id": probe_id}),
    );
    HealTick {
        exit: exit::REFUSED,
        report: result(cfg, steps, "refused", why, posted.err()),
    }
}

fn result(
    cfg: &HealCfg,
    steps: Vec<Json>,
    outcome: &str,
    detail: &str,
    nerve_err: Option<String>,
) -> Json {
    Json::obj()
        .with("mode", "live-heal-test-services-only")
        .with("service", cfg.target.name.as_str())
        .with("port", u64::from(cfg.target.port))
        .with("outcome", outcome)
        .with("detail", detail)
        .with("nerve_error", nerve_err)
        .with("steps", Json::Arr(steps))
}

fn step(name: &str, j: Json) -> Json {
    Json::obj()
        .with("step", name)
        .with("at", zagreb_time::now_rfc3339())
        .with("detail", j)
}

/// One heal tick. `Err` only for setup problems (nerve unreachable, state
/// directory unusable); every policy decision is an `Ok` with an exit code.
pub fn heal_tick(cfg: &HealCfg) -> Result<HealTick, String> {
    let t = &cfg.target;
    let mut steps = Vec::new();
    let probe_id = format!("{}-{}-{}", t.name, heal::epoch_now(), std::process::id());

    // 0. Scope guard: test service on the box, nothing else.
    if let Err(e) = heal::check_target(t, &cfg.allowed) {
        steps.push(step(
            "scope",
            Json::obj().with("ok", false).with("reason", e.as_str()),
        ));
        return Ok(refuse(cfg, steps, "scope", &e, &probe_id));
    }
    steps.push(step(
        "scope",
        Json::obj().with("ok", true).with(
            "allowed",
            format!("127.0.0.1:{}-{}", cfg.allowed.start(), cfg.allowed.end()),
        ),
    ));
    std::fs::create_dir_all(&cfg.state_dir)
        .map_err(|e| format!("state dir {}: {e}", cfg.state_dir.display()))?;
    if !nerve::healthy(cfg.nerve) {
        return Err(format!("nerve at {} is not healthy", cfg.nerve));
    }

    // 1. Probe and record.
    let o = gaia_live::Opts {
        systemd: false,
        connect_timeout: Duration::from_millis(500),
        read_timeout: Duration::from_millis(1000),
        ..gaia_live::Opts::default()
    };
    let r = gaia_live::probe(&heal::test_probe(t), &o);
    let detail = r
        .http
        .as_ref()
        .map(|h| h.detail.clone())
        .unwrap_or_default();
    let up = r.state == gaia_live::State::Up;
    nerve_post(
        cfg,
        "service.probe",
        if up { "info" } else { "warn" },
        json!({"service": t.name, "port": t.port, "state": r.state.name(), "detail": detail, "notes": r.notes, "probe_id": probe_id}),
    )?;
    steps.push(step(
        "probe",
        r.to_json().with("probe_id", probe_id.as_str()),
    ));
    if up {
        return Ok(HealTick {
            exit: exit::OK,
            report: result(cfg, steps, "healthy", "service is up; nothing to do", None),
        });
    }

    // 2. GAIA core takes its input from the nerve, not from the probe.
    let events = nerve::tail(cfg.nerve, 200)?;
    let Some(ev) = events.iter().rev().find(|e| {
        e.get("kind").and_then(|k| k.as_str()) == Some("service.probe")
            && e.pointer("/data/probe_id").and_then(|p| p.as_str()) == Some(probe_id.as_str())
    }) else {
        return Err(format!("nerve did not return the probe event {probe_id}"));
    };
    let mut g = booted();
    let body = format!(
        "{} is {} on 127.0.0.1:{} ({detail})",
        t.name,
        r.state.name(),
        t.port
    );
    g.signal("nerve:service.probe", &body);
    steps.push(step("nerve-in", nerve::to_gaia_json(ev)));

    // 3. DEMETER: heal-dry and a guardian-tick health cycle.
    let healer = send(&mut g, System::Healer, Verb::Plan, 50, &body, None);
    let gtick = send(&mut g, System::GuardianTick, Verb::Run, 50, &body, None);
    // The simulated heal-dry reads GAIA's model vitals; the live suggestion
    // comes from the probe that DEMETER's guardian-tick owns.
    let suggestion = format!(
        "restart {} on 127.0.0.1:{} with its recorded start command ({})",
        t.name, t.port, detail
    );
    steps.push(step(
        "demeter",
        Json::obj()
            .with("live_suggestion", suggestion.as_str())
            .with("healer", healer.to_json())
            .with("guardian_tick", gtick.to_json()),
    ));

    // 4. HADES: dry-run plan, then the approval-gated Execute.
    let plan = send(&mut g, System::HealApply, Verb::Plan, 10, &suggestion, None);
    steps.push(step("hades-plan", plan.to_json()));
    let approval = match &cfg.approval_file {
        None => Err("no approval file given".to_string()),
        Some(p) => std::fs::read_to_string(p)
            .map_err(|e| format!("approval file {}: {e}", p.display()))
            .and_then(|txt| Approval::parse(&txt)),
    };
    let checked = approval.as_ref().map_err(Clone::clone).and_then(|a| {
        let trusted = heal::load_trusted_token(&cfg.trust_file)?;
        heal::check_approval(a, &trusted, &t.name)?;
        if g.alphas_mut().get(&a.alpha).is_none() {
            return Err(format!("alpha {:?} is not registered with GAIA", a.alpha));
        }
        Ok(a.clone())
    });
    let alpha = checked.as_ref().ok().map(|a| a.alpha.clone());
    let exec = send(
        &mut g,
        System::HealApply,
        Verb::Execute,
        10,
        &suggestion,
        alpha.as_deref(),
    );
    let released = exec.outcome == Outcome::Done && !exec.dry_run;
    steps.push(step(
        "approval",
        Json::obj()
            .with("alpha", approval.as_ref().ok().map(|a| a.alpha.as_str()))
            .with(
                "token_fp",
                approval.as_ref().ok().map(Approval::token_fingerprint),
            )
            .with(
                "check",
                match &checked {
                    Ok(_) => "ok".to_string(),
                    Err(e) => e.clone(),
                },
            )
            .with("hades_execute", exec.to_json())
            .with("released", released),
    ));
    if !released {
        let why = match checked {
            Err(e) => e,
            Ok(_) => format!("HADES did not release the plan: {}", exec.summary),
        };
        return Ok(refuse(cfg, steps, "approval", &why, &probe_id));
    }

    // 5. Healer apply: one restart, inside the cap.
    let ledger = Ledger::new(&cfg.state_dir);
    let history = ledger
        .history(&t.name)
        .map_err(|e| format!("ledger: {e}"))?;
    let now = heal::epoch_now();
    let used = match heal::cap_check(&history, now) {
        Ok(used) => used,
        Err(retry) => {
            let why = format!(
                "restart cap reached: {} restarts of {} in the last hour; next allowed in {retry} s",
                heal::MAX_RESTARTS_PER_HOUR,
                t.name
            );
            steps.push(step(
                "cap",
                Json::obj().with("ok", false).with("retry_after_s", retry),
            ));
            let posted = nerve_post(
                cfg,
                "gaia.heal.capped",
                "error",
                json!({"service": t.name, "port": t.port, "restarts_last_hour": heal::MAX_RESTARTS_PER_HOUR, "cap": heal::MAX_RESTARTS_PER_HOUR, "retry_after_s": retry, "probe_id": probe_id}),
            );
            return Ok(HealTick {
                exit: exit::CAPPED,
                report: result(cfg, steps, "capped", &why, posted.err()),
            });
        }
    };
    steps.push(step(
        "cap",
        Json::obj()
            .with("ok", true)
            .with("restarts_last_hour_before", used)
            .with("cap", heal::MAX_RESTARTS_PER_HOUR),
    ));
    let log = cfg.state_dir.join(format!("{}.log", t.name));
    let spawned = heal::spawn_detached(&t.argv, &log);
    let spawn_error = spawned.as_ref().err().map(ToString::to_string);
    let (exit_code, outcome, kind, sev, pid, verified, tries) = match spawned {
        Err(_) => (
            exit::APPLY_FAILED,
            "apply-failed",
            "gaia.heal.failed",
            "error",
            None,
            false,
            0,
        ),
        Ok(pid) => {
            ledger
                .record(&t.name, now, pid)
                .map_err(|e| format!("ledger: {e}"))?;
            let (ok, tries) = heal::wait_up(t, cfg.wait);
            if ok {
                (
                    exit::OK,
                    "restarted",
                    "gaia.heal.applied",
                    "info",
                    Some(pid),
                    true,
                    tries,
                )
            } else {
                (
                    exit::APPLY_FAILED,
                    "apply-failed",
                    "gaia.heal.failed",
                    "error",
                    Some(pid),
                    false,
                    tries,
                )
            }
        }
    };
    let restarts_now = if pid.is_some() { used + 1 } else { used };
    steps.push(step(
        "apply",
        Json::obj()
            .with("action", "restart")
            .with("pid", pid.map(u64::from))
            .with("health_verified", verified)
            .with("health_tries", u64::from(tries))
            .with("restarts_last_hour", restarts_now)
            .with("cap", heal::MAX_RESTARTS_PER_HOUR)
            .with("spawn_error", spawn_error.clone())
            .with("log", log.display().to_string()),
    ));

    // 6. Record the result on the nerve.
    nerve_post(
        cfg,
        kind,
        sev,
        json!({
            "service": t.name, "port": t.port, "action": "restart", "pid": pid,
            "health_verified": verified, "restarts_last_hour": restarts_now,
            "cap": heal::MAX_RESTARTS_PER_HOUR, "approved_by": alpha,
            "hades_directive": exec.directive_id, "probe_id": probe_id, "spawn_error": spawn_error,
        }),
    )?;
    let detail = format!(
        "restart {restarts_now}/{} this hour; health {}",
        heal::MAX_RESTARTS_PER_HOUR,
        if verified { "verified" } else { "NOT verified" }
    );
    Ok(HealTick {
        exit: exit_code,
        report: result(cfg, steps, outcome, &detail, None),
    })
}

/// Parse `gaia heal-tick` arguments. Gaia's own flags come before `--`; the
/// test service's start command (absolute path plus args) comes after it.
pub fn parse_cli(args: &[&str]) -> Result<HealCfg, String> {
    let split = args
        .iter()
        .position(|a| *a == "--")
        .ok_or("heal-tick needs `-- START_CMD...`")?;
    let (own, argv) = (&args[..split], &args[split + 1..]);
    let opt = |k: &str| {
        own.iter()
            .position(|a| *a == k)
            .and_then(|i| own.get(i + 1))
            .copied()
    };
    let need = |k: &str| opt(k).ok_or_else(|| format!("heal-tick needs {k}"));
    let port: u16 = need("--port")?
        .parse()
        .map_err(|_| "--port needs a port number")?;
    let nerve: SocketAddr = need("--nerve")?
        .parse()
        .map_err(|e| format!("--nerve: {e}"))?;
    let wait_ms: u64 = match opt("--wait-ms") {
        Some(v) => v.parse().map_err(|_| "--wait-ms needs a number")?,
        None => 5000,
    };
    Ok(HealCfg {
        target: TestTarget {
            name: need("--service")?.to_string(),
            port,
            health_path: opt("--health").unwrap_or("/health").to_string(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        },
        allowed: heal::parse_range(need("--allow-ports")?)?,
        nerve,
        state_dir: PathBuf::from(need("--state")?),
        trust_file: PathBuf::from(need("--trust-file")?),
        approval_file: opt("--approval-file").map(PathBuf::from),
        wait: Duration::from_millis(wait_ms.clamp(200, 30_000)),
    })
}
