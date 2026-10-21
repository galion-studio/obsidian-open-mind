// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The first end-to-end Gaia path (M2), for **box-local test services only**:
//!
//! 1. probe the test service (read-only GET) and record a `service.probe`
//!    nerve event over loopback;
//! 2. GAIA core reads that event back from the nerve (`GET /events`) and
//!    takes it as its input signal; nothing proceeds unless the nerve has it;
//! 3. DEMETER answers with heal-dry (healer) and a guardian-tick health cycle;
//! 4. HADES produces the heal-apply plan (dry-run) and the exact live plan
//!    ([`heal_plan`]); hades-gate decides through
//!    [`hades_gate::adapter::check_gaia`]: disable switch and admin-safety
//!    rule first, then an Ed25519-signed, expiring token scoped to
//!    `heal-apply`, this service and the SHA-256 of the plan bytes, consumed
//!    once in hades-gate's fsynced replay ledger. Only an approved verdict
//!    lets GAIA core's `Execute` carry the admin Alpha ([`ADMIN_ALPHA`]);
//! 5. the healer applies the released plan: one restart of the test service,
//!    if the 3-per-hour cap allows it (checked before the token is consumed),
//!    then a health check;
//! 6. the result is recorded as a `gaia.heal.*` nerve event.
//!
//! [`gaia_live::heal::check_target`] runs first, so live stack ports and
//! service managers are refused before anything else happens. The token
//! string is never printed or logged; only the first 16 hex digits of its
//! SHA-256 (the same id hades-gate's ledger keeps) and the consumed nonce.
//! `hades-gate disable --config C` needs no token and refuses every heal.

use crate::{booted, send};
use gaia_core::{Json, Outcome, System, Verb};
use gaia_live::heal::{self, Budget, TestTarget};
use gaia_live::nerve::{self, NerveRestarts};
use hades_gate::approval::{Approver, Verdict};
use hades_gate::config::Config;
use hades_gate::token::{plan_hash, to_hex, Claims};
use serde_json::json;
use std::io::Read;
use std::net::SocketAddr;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Source name on every nerve event this path writes.
pub const SOURCE: &str = "gaia-live";

/// The GAIA Alpha a hades-gate approval stands for: the admin key holder is
/// the owner. GAIA core binds it to the `Execute` directive id and target.
pub const ADMIN_ALPHA: &str = "owner";

/// First line of every signed heal plan (bump on any format change).
///
/// v2 (integration-3): every line starts with a hades-gate
/// `safety::PLAN_VERBS` verb and uses only its plan punctuation, so the S3.1
/// allow-list (hades-redteam e6ceca7) admits the plan without being loosened.
/// v1 started lines with `gaia`/`service`/`probe`/`start`/`at` and JSON-quoted
/// the argv, which that allow-list refuses.
pub const PLAN_HEADER: &str = "record gaia heal-apply plan v2 (box test service)";

/// Longest token file read.
const MAX_TOKEN_FILE: u64 = 4096;

/// Exit codes of `gaia heal-tick`.
pub mod exit {
    /// Healthy (nothing to do) or restarted and verified up.
    pub const OK: u8 = 0;
    /// Refused: bad target, missing, forged, expired, replayed or
    /// wrong-scope token, admin-safety or disabled gate.
    pub const REFUSED: u8 = 4;
    /// The 3-restarts-per-hour cap is spent.
    pub const CAPPED: u8 = 5;
    /// Restarted, but it did not come back up in time (or spawn failed).
    pub const APPLY_FAILED: u8 = 6;
    /// The service holds its port but does not answer (hung). Report-only
    /// (decision D-L4, accepted 2026-10-21): a `gaia.heal.hung` event goes to
    /// the nerve; nothing is restarted, killed or stopped, no token is used.
    pub const HUNG: u8 = 8;
}

#[derive(Clone, Debug)]
pub struct HealCfg {
    pub target: TestTarget,
    pub allowed: RangeInclusive<u16>,
    pub nerve: SocketAddr,
    pub state_dir: PathBuf,
    /// hades-gate config (admin public key, replay ledger, disable file).
    pub hades_config: PathBuf,
    /// File holding one signed hades-gate token; `None` = dry-run only.
    pub token_file: Option<PathBuf>,
    pub wait: Duration,
}

/// Outcome of one tick.
#[derive(Clone, Debug)]
pub struct HealTick {
    pub exit: u8,
    pub report: Json,
    /// hades-gate decision code when the tick reached the approval step with
    /// a token question (`approved`, `no-token`, `replayed`, `forged`, ...).
    pub approval_code: Option<String>,
    /// A restart was spawned (and recorded in both ledgers).
    pub spawned: bool,
}

/// The exact plan the admin signs for one restart of `t`. Deterministic: the
/// same target always gives the same bytes (joined with `\n` by
/// [`hades_gate::adapter::gaia_plan_bytes`]), so `gaia heal-plan` and
/// `gaia heal-tick` agree on the hash. Argument boundaries are part of what is
/// signed: the argument count, then one numbered line per argument. An
/// argument with a shell metacharacter makes the plan unapprovable (fail
/// closed), which is the intent for a box test service.
#[must_use]
pub fn heal_plan(t: &TestTarget) -> Vec<String> {
    let mut plan = vec![
        PLAN_HEADER.to_string(),
        format!(
            "restart box test service {} on 127.0.0.1:{} (box test ports {}-{} only)",
            t.name,
            t.port,
            heal::TEST_PORTS.start(),
            heal::TEST_PORTS.end()
        ),
        format!("verify GET {} answers status ok", t.health_path),
        format!(
            "restart with argument count {}, detached in its own process group, stdin closed, no shell",
            t.argv.len()
        ),
    ];
    for (i, arg) in t.argv.iter().enumerate() {
        plan.push(format!("verify argument {i} is {arg}"));
    }
    plan.push(format!(
        "keep at most {} restarts of {} per rolling hour",
        heal::MAX_RESTARTS_PER_HOUR,
        t.name
    ));
    plan.push("record the result on the nerve".to_string());
    plan
}

/// SHA-256 (hex) of the plan bytes for `t`.
#[must_use]
pub fn heal_plan_sha256(t: &TestTarget) -> String {
    to_hex(&plan_hash(&hades_gate::adapter::gaia_plan_bytes(
        &heal_plan(t),
    )))
}

/// Load the gate: the approver and the path of the global restart ledger
/// (next to hades-gate's replay ledger; see [`heal::global_ledger_path`]).
fn load_gate(config: &Path) -> Result<(Approver, PathBuf), String> {
    let c = Config::load(config)?;
    let global = heal::global_ledger_path(&c.ledger);
    Ok((Approver::from_config(&c)?, global))
}

/// Read one token (trimmed). No file means no token (dry-run).
fn read_token(path: Option<&Path>) -> Result<String, String> {
    let Some(p) = path else {
        return Ok(String::new());
    };
    let mut buf = Vec::new();
    std::fs::File::open(p)
        .and_then(|f| f.take(MAX_TOKEN_FILE + 1).read_to_end(&mut buf))
        .map_err(|e| format!("token file {}: {}", p.display(), e.kind()))?;
    if buf.len() as u64 > MAX_TOKEN_FILE {
        return Err(format!("token file {} is too long", p.display()));
    }
    let text =
        String::from_utf8(buf).map_err(|_| format!("token file {} is not UTF-8", p.display()))?;
    let tok = text.trim().to_string();
    if tok.is_empty() {
        return Err(format!("token file {} is empty", p.display()));
    }
    Ok(tok)
}

/// Non-reversible id of a token: first 16 hex of SHA-256 (as in hades-gate's
/// ledger). `None` for no token.
fn token_id(token: &str) -> Option<String> {
    (!token.is_empty()).then(|| to_hex(&plan_hash(token.as_bytes()))[..16].to_string())
}

fn unix_ts(s: u64) -> String {
    zagreb_time::from_unix(i64::try_from(s).unwrap_or(i64::MAX))
}

fn claims_json(c: &Claims) -> Json {
    Json::obj()
        .with("action", c.action.as_str())
        .with("target", c.target.as_str())
        .with("plan_sha256", to_hex(&c.plan_hash))
        .with("nonce", to_hex(&c.nonce))
        .with("iat", unix_ts(c.issued_at))
        .with("exp", unix_ts(c.expires_at))
}

fn nerve_post(cfg: &HealCfg, kind: &str, sev: &str, data: serde_json::Value) -> Result<(), String> {
    nerve::post(cfg.nerve, SOURCE, kind, sev, data)
}

/// Post one event as `gaia-live` to the tick's nerve (used by the loop).
pub fn nerve_event(
    cfg: &HealCfg,
    kind: &str,
    sev: &str,
    data: serde_json::Value,
) -> Result<(), String> {
    nerve_post(cfg, kind, sev, data)
}

/// Record a refusal on the nerve (best effort) and build the result.
fn refuse(cfg: &HealCfg, steps: Vec<Json>, stage: &str, why: &str, probe_id: &str) -> HealTick {
    refuse_with(cfg, steps, stage, why, probe_id, json!({}))
}

/// [`refuse`] with extra fields on the nerve event.
fn refuse_with(
    cfg: &HealCfg,
    steps: Vec<Json>,
    stage: &str,
    why: &str,
    probe_id: &str,
    extra: serde_json::Value,
) -> HealTick {
    let mut data = json!({"service": cfg.target.name, "port": cfg.target.port, "stage": stage, "reason": why, "probe_id": probe_id});
    if let (Some(d), Some(x)) = (data.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            d.insert(k.clone(), v.clone());
        }
    }
    let approval_code = data
        .get("code")
        .and_then(|c| c.as_str())
        .map(str::to_string);
    let posted = nerve_post(cfg, "gaia.heal.refused", "warn", data);
    HealTick {
        exit: exit::REFUSED,
        report: result(cfg, steps, "refused", why, posted.err()),
        approval_code,
        spawned: false,
    }
}

/// The restart cap is spent: record it and stop (no token is consumed).
/// `gaia_n` is Gaia's own restarts in the window, `nr` the nerve's (D-L2).
#[allow(clippy::too_many_arguments)]
fn capped(
    cfg: &HealCfg,
    mut steps: Vec<Json>,
    retry: u64,
    probe_id: &str,
    plan_sha: &str,
    budget: &Budget,
    gaia_n: usize,
    nr: &NerveRestarts,
    token_consumed: bool,
) -> HealTick {
    let t = &cfg.target;
    let total = gaia_n.saturating_add(nr.count);
    let why = if nr.count > 0 {
        format!(
            "restart cap reached: {gaia_n} Gaia + {} nerve restarts of {} in the last hour (combined cap {}); next allowed in at most {retry} s",
            nr.count,
            t.name,
            heal::MAX_RESTARTS_PER_HOUR
        )
    } else {
        format!(
            "restart cap reached: {total} restarts of {} in the last hour (cap {}); next allowed in {retry} s",
            t.name,
            heal::MAX_RESTARTS_PER_HOUR
        )
    };
    steps.push(step(
        "cap",
        Json::obj()
            .with("ok", false)
            .with("retry_after_s", retry)
            .with("gaia_restarts_last_hour", gaia_n as u64)
            .with("nerve_restarts_last_hour", nr.count as u64)
            .with("nerve", nerve::to_gaia_json(&nr.to_value()))
            .with("global_ledger", budget.global_path().display().to_string()),
    ));
    let posted = nerve_post(
        cfg,
        "gaia.heal.capped",
        "error",
        json!({"service": t.name, "port": t.port, "restarts_last_hour": total,
               "gaia_restarts_last_hour": gaia_n, "nerve_restarts_last_hour": nr.count,
               "nerve": nr.to_value(), "cap": heal::MAX_RESTARTS_PER_HOUR, "combined": true,
               "retry_after_s": retry, "probe_id": probe_id, "plan_sha256": plan_sha,
               "token_consumed": token_consumed, "global_ledger": budget.global_path().display().to_string()}),
    );
    HealTick {
        exit: exit::CAPPED,
        report: result(cfg, steps, "capped", &why, posted.err()),
        approval_code: token_consumed.then(|| "approved".to_string()),
        spawned: false,
    }
}

/// The nerve's restarts of the target, read fresh from `GET /state`. An
/// unreachable nerve or an unreadable `/state` is a setup error: the tick
/// fails closed before (or instead of) the restart.
fn nerve_restarts(cfg: &HealCfg) -> Result<NerveRestarts, String> {
    nerve::restarts(cfg.nerve, &cfg.target.name)
        .map_err(|e| format!("nerve restart count (D-L2): {e}"))
}

/// Hung service (D-L4, accepted 2026-10-21): report it to the nerve with the
/// evidence and stop. Nothing is restarted, killed or stopped; no token is
/// read or consumed; the restart budget is untouched.
fn hung(
    cfg: &HealCfg,
    mut steps: Vec<Json>,
    probe_id: &str,
    probes: &[String],
) -> Result<HealTick, String> {
    let t = &cfg.target;
    let listeners = heal::listener_pids(t.port);
    // Which of those Gaia started itself (from the restart ledgers), so an
    // operator can tell a Gaia restart that hung from a foreign process.
    let mut recorded = Vec::new();
    let state_only = Budget::new(
        &cfg.state_dir,
        Path::new("/nonexistent/gaia-restarts.jsonl"),
    );
    let budget = Config::load(&cfg.hades_config)
        .map(|c| Budget::new(&cfg.state_dir, &heal::global_ledger_path(&c.ledger)))
        .unwrap_or(state_only);
    if let Ok(p) = heal::recorded_pids(&budget, &t.name) {
        recorded = p;
    }
    let gaia_started: Vec<u32> = listeners
        .iter()
        .copied()
        .filter(|p| recorded.contains(p))
        .collect();
    let evidence = json!({
        "service": t.name, "port": t.port, "probe_id": probe_id,
        "health_path": t.health_path, "probes": probes,
        "read_timeout_ms": 1000, "connect": "ok",
        "listener_pids": listeners, "gaia_started_pids": gaia_started,
        "action": "report-only", "restart": false, "kill": false, "stop": false,
        "token_consumed": false, "handed_to": "nerve/operator",
        "decision": "D-L4 accepted 2026-10-21: hung services are reported, never killed",
    });
    steps.push(step(
        "hung",
        nerve::to_gaia_json(&evidence).with("at_zagreb", zagreb_time::now_rfc3339()),
    ));
    nerve_post(cfg, "gaia.heal.hung", "error", evidence)?;
    let why = format!(
        "{} on 127.0.0.1:{} holds its port but does not answer ({}); reported to the nerve, not restarted or killed (D-L4)",
        t.name,
        t.port,
        probes.last().map(String::as_str).unwrap_or("?")
    );
    Ok(HealTick {
        exit: exit::HUNG,
        report: result(cfg, steps, "hung", &why, None),
        approval_code: None,
        spawned: false,
    })
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
    heal_tick_at(cfg, None)
}

/// [`heal_tick`] with an explicit wall clock for the restart cap, the token
/// check and the ledger record (`None` = the system clock). For in-process
/// fault tests only (e.g. the 2026-10-25 CEST -> CET switch,
/// gaia-fault-inject); the `gaia` binary never takes a clock from its
/// arguments or environment, so nobody can move the cap window from outside.
pub fn heal_tick_at(cfg: &HealCfg, clock: Option<u64>) -> Result<HealTick, String> {
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
            approval_code: None,
            spawned: false,
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

    // 2b. Hung (D-L4): connected but no answer means a process holds the
    //     port. A restart cannot take the port and Gaia never kills, so the
    //     tick confirms with a second probe and reports it, nothing more.
    if heal::hung_detail(&detail) {
        std::thread::sleep(Duration::from_millis(300));
        let again = gaia_live::probe(&heal::test_probe(t), &o);
        let detail2 = again
            .http
            .as_ref()
            .map(|h| h.detail.clone())
            .unwrap_or_default();
        if again.state == gaia_live::State::Up {
            steps.push(step("reprobe", again.to_json()));
            return Ok(HealTick {
                exit: exit::OK,
                report: result(
                    cfg,
                    steps,
                    "healthy",
                    "slow answer, up on the confirming probe; nothing to do",
                    None,
                ),
                approval_code: None,
                spawned: false,
            });
        }
        if heal::hung_detail(&detail2) {
            return hung(cfg, steps, &probe_id, &[detail.clone(), detail2]);
        }
        steps.push(step("reprobe", again.to_json()));
    }

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

    // 4. HADES: GAIA core's dry-run plan, then the exact live plan the admin
    //    signs, then hades-gate decides (admin safety first, then the token).
    let plan = send(&mut g, System::HealApply, Verb::Plan, 10, &suggestion, None);
    let live_plan = heal_plan(t);
    let plan_bytes = hades_gate::adapter::gaia_plan_bytes(&live_plan);
    let plan_sha = to_hex(&plan_hash(&plan_bytes));
    steps.push(step(
        "hades-plan",
        plan.to_json()
            .with(
                "signed_plan",
                live_plan
                    .iter()
                    .map(|s| Json::from(s.as_str()))
                    .collect::<Vec<_>>(),
            )
            .with("plan_sha256", plan_sha.as_str()),
    ));
    let (approver, global_ledger) = match load_gate(&cfg.hades_config) {
        Ok(a) => a,
        Err(e) => {
            let why = format!("hades-gate unavailable, failing closed: {e}");
            steps.push(step(
                "approval",
                Json::obj().with("ok", false).with("reason", why.as_str()),
            ));
            return Ok(refuse_with(
                cfg,
                steps,
                "hades-approval",
                &why,
                &probe_id,
                json!({"code": "gate-unavailable", "plan_sha256": plan_sha}),
            ));
        }
    };
    let token = match read_token(cfg.token_file.as_deref()) {
        Ok(tok) => tok,
        Err(e) => {
            steps.push(step(
                "approval",
                Json::obj().with("ok", false).with("reason", e.as_str()),
            ));
            return Ok(refuse_with(
                cfg,
                steps,
                "hades-approval",
                &e,
                &probe_id,
                json!({"code": "token-file", "plan_sha256": plan_sha}),
            ));
        }
    };
    let tid = token_id(&token);

    // The cap is checked before the token is consumed, so a capped tick does
    // not burn the admin's approval. A disabled gate or an unsafe plan skips
    // this: hades-gate refuses (and audits) those first, whatever the cap.
    // The budget is the --state ledger plus the global ledger next to
    // hades-gate's replay ledger, so a new --state dir cannot reset it.
    let budget = Budget::new(&cfg.state_dir, &global_ledger);
    let history = budget
        .history(&t.name)
        .map_err(|e| format!("ledger: {e}"))?;
    let now = clock.unwrap_or_else(heal::epoch_now);
    let admin_first =
        approver.is_disabled() || hades_gate::safety::review(&t.name, &plan_bytes).is_err();
    // D-L2: the nerve's own restarts of this service count too, so the
    // combined Gaia + nerve total stays at 3 per rolling hour.
    if !admin_first {
        let nr = nerve_restarts(cfg)?;
        if let Err(retry) = heal::cap_check_combined(&history, nr.count, now) {
            let gaia_n = heal::recent_count(&history, now);
            return Ok(capped(
                cfg, steps, retry, &probe_id, &plan_sha, &budget, gaia_n, &nr, false,
            ));
        }
    }
    let verdict = hades_gate::adapter::check_gaia(
        &approver,
        System::HealApply.name(),
        &t.name,
        &live_plan,
        &token,
        now,
    );
    drop(token);
    let claims = match &verdict {
        Verdict::Approved(c) => Some(c.clone()),
        _ => None,
    };
    let alpha = claims.as_ref().map(|_| ADMIN_ALPHA);
    let exec = send(
        &mut g,
        System::HealApply,
        Verb::Execute,
        10,
        &suggestion,
        alpha,
    );
    let released = claims.is_some() && exec.outcome == Outcome::Done && !exec.dry_run;
    let (code, reason) = match &verdict {
        Verdict::Approved(_) => (
            "approved",
            "signed, in scope, unexpired, first use".to_string(),
        ),
        Verdict::DryRun => (
            "no-token",
            "no approval token: HADES releases the dry-run plan only".to_string(),
        ),
        Verdict::Refused(r) => (r.code(), r.reason()),
    };
    steps.push(step(
        "approval",
        Json::obj()
            .with("gate", "hades-gate")
            .with("admin_fingerprint", approver.admin_fingerprint())
            .with("token_sha256_16", tid.clone())
            .with("decision", format!("{:?}", verdict.decision()))
            .with("code", code)
            .with("reason", reason.as_str())
            .with("approved", claims.as_ref().map(claims_json))
            .with("alpha", alpha)
            .with("hades_execute", exec.to_json())
            .with("released", released),
    ));
    if !released {
        let why = if claims.is_some() {
            format!("HADES did not release the plan: {}", exec.summary)
        } else {
            reason
        };
        return Ok(refuse_with(
            cfg,
            steps,
            "hades-approval",
            &why,
            &probe_id,
            json!({"code": code, "token_sha256_16": tid, "plan_sha256": plan_sha}),
        ));
    }
    let nonce = claims.as_ref().map(|c| to_hex(&c.nonce));
    nerve_post(
        cfg,
        "gaia.heal.approved",
        "info",
        json!({
            "service": t.name, "port": t.port, "action": "heal-apply",
            "nonce": nonce, "plan_sha256": plan_sha,
            "token_exp": claims.as_ref().map(|c| unix_ts(c.expires_at)),
            "admin_fingerprint": approver.admin_fingerprint(),
            "approved_by": ADMIN_ALPHA, "probe_id": probe_id,
        }),
    )?;

    // 5. Healer apply: one restart, inside the cap. The budget lock is held
    //    from this re-read until the restart is recorded, so two ticks or
    //    loops on one service cannot both spend the last restart.
    let budget_lock = budget.lock().map_err(|e| format!("ledger lock: {e}"))?;
    let history = budget
        .history(&t.name)
        .map_err(|e| format!("ledger: {e}"))?;
    // Fresh nerve count under the lock (fail closed if it is unreadable now).
    let nr = nerve_restarts(cfg)?;
    let gaia_n = heal::recent_count(&history, now);
    let used = match heal::cap_check_combined(&history, nr.count, now) {
        Ok(used) => used,
        Err(retry) => {
            return Ok(capped(
                cfg, steps, retry, &probe_id, &plan_sha, &budget, gaia_n, &nr, true,
            ))
        }
    };
    steps.push(step(
        "cap",
        Json::obj()
            .with("ok", true)
            .with("restarts_last_hour_before", used)
            .with("gaia_restarts_last_hour_before", gaia_n as u64)
            .with("nerve_restarts_last_hour", nr.count as u64)
            .with("nerve", nerve::to_gaia_json(&nr.to_value()))
            .with("cap", heal::MAX_RESTARTS_PER_HOUR)
            .with("global_ledger", budget.global_path().display().to_string()),
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
            budget
                .record(&t.name, now, pid)
                .map_err(|e| format!("ledger: {e}"))?;
            drop(budget_lock);
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
            "gaia_restarts_last_hour": gaia_n + usize::from(pid.is_some()), "nerve_restarts_last_hour": nr.count,
            "cap": heal::MAX_RESTARTS_PER_HOUR, "approved_by": alpha,
            "nonce": nonce, "plan_sha256": plan_sha,
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
        approval_code: Some("approved".to_string()),
        spawned: pid.is_some(),
    })
}

/// Split `args` at `--` and read the target flags shared by `heal-tick` and
/// `heal-plan`. Returns the target, the allowed range and Gaia's own flags.
fn parse_target<'a>(
    args: &'a [&'a str],
) -> Result<(TestTarget, RangeInclusive<u16>, &'a [&'a str]), String> {
    let split = args
        .iter()
        .position(|a| *a == "--")
        .ok_or("needs `-- START_CMD...`")?;
    let (own, argv) = (&args[..split], &args[split + 1..]);
    let opt = |k: &str| flag(own, k);
    let need = |k: &str| opt(k).ok_or_else(|| format!("needs {k}"));
    let port: u16 = need("--port")?
        .parse()
        .map_err(|_| "--port needs a port number")?;
    Ok((
        TestTarget {
            name: need("--service")?.to_string(),
            port,
            health_path: opt("--health").unwrap_or("/health").to_string(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        },
        heal::parse_range(need("--allow-ports")?)?,
        own,
    ))
}

fn flag<'a>(own: &[&'a str], k: &str) -> Option<&'a str> {
    own.iter()
        .position(|a| *a == k)
        .and_then(|i| own.get(i + 1))
        .copied()
}

/// Parse `gaia heal-tick` arguments. Gaia's own flags come before `--`; the
/// test service's start command (absolute path plus args) comes after it.
pub fn parse_cli(args: &[&str]) -> Result<HealCfg, String> {
    let (target, allowed, own) = parse_target(args).map_err(|e| format!("heal-tick {e}"))?;
    let need = |k: &str| flag(own, k).ok_or_else(|| format!("heal-tick needs {k}"));
    let nerve: SocketAddr = need("--nerve")?
        .parse()
        .map_err(|e| format!("--nerve: {e}"))?;
    let wait_ms: u64 = match flag(own, "--wait-ms") {
        Some(v) => v.parse().map_err(|_| "--wait-ms needs a number")?,
        None => 5000,
    };
    Ok(HealCfg {
        target,
        allowed,
        nerve,
        state_dir: PathBuf::from(need("--state")?),
        hades_config: PathBuf::from(need("--hades-config")?),
        token_file: flag(own, "--token-file").map(PathBuf::from),
        wait: Duration::from_millis(wait_ms.clamp(200, 30_000)),
    })
}

/// `gaia heal-plan ... [--out F] -- START_CMD...`: print the exact plan the
/// admin signs (and its SHA-256), after the same scope guard as heal-tick.
/// With `--out F` the plan bytes are also written to a NEW file F (never
/// overwritten) for `hades-gate mint --plan-file F`. The admin-safety verdict
/// is reported so an unsafe plan is visible before anyone mints for it.
pub fn heal_plan_cli(args: &[&str]) -> Result<Json, String> {
    let (t, allowed, own) = parse_target(args).map_err(|e| format!("heal-plan {e}"))?;
    heal::check_target(&t, &allowed)?;
    let steps = heal_plan(&t);
    let bytes = hades_gate::adapter::gaia_plan_bytes(&steps);
    if let Some(out) = flag(own, "--out") {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)
            .and_then(|mut f| f.write_all(&bytes))
            .map_err(|e| format!("--out {out}: {e}"))?;
    }
    let safety = match hades_gate::safety::review(&t.name, &bytes) {
        Ok(()) => "ok".to_string(),
        Err(h) => h.reason(),
    };
    Ok(Json::obj()
        .with("action", "heal-apply")
        .with("target", t.name.as_str())
        .with("plan_sha256", to_hex(&plan_hash(&bytes)))
        .with(
            "plan",
            steps
                .iter()
                .map(|s| Json::from(s.as_str()))
                .collect::<Vec<_>>(),
        )
        .with("admin_safety", safety)
        .with("out", flag(own, "--out")))
}

/// `gaia nerve-restarts (--nerve 127.0.0.1:N | --state-file F) --service S
/// [--service S2 ...]`: read-only view of decision D-L2. For each service:
/// the restarts the nerve reports in the last hour and how many of the
/// combined 3 per hour that leaves for Gaia (Gaia's own ledger not counted
/// here). `--state-file` reads a saved `GET /state` capture.
pub fn nerve_restarts_cli(args: &[&str]) -> Result<Json, String> {
    let state = match (flag(args, "--nerve"), flag(args, "--state-file")) {
        (Some(n), None) => nerve::state(n.parse().map_err(|e| format!("--nerve: {e}"))?)?,
        (None, Some(f)) => {
            let mut buf = Vec::new();
            std::fs::File::open(f)
                .and_then(|h| h.take(4 * 1024 * 1024).read_to_end(&mut buf))
                .map_err(|e| format!("{f}: {}", e.kind()))?;
            serde_json::from_slice(&buf).map_err(|e| format!("{f}: {e}"))?
        }
        _ => return Err("nerve-restarts needs --nerve ADDR or --state-file F".into()),
    };
    let services: Vec<&str> = args
        .windows(2)
        .filter(|w| w[0] == "--service")
        .map(|w| w[1])
        .collect();
    if services.is_empty() {
        return Err("nerve-restarts needs --service NAME".into());
    }
    let mut rows = Vec::new();
    for s in services {
        let r = nerve::restarts_for(&state, s)?;
        let left = heal::MAX_RESTARTS_PER_HOUR.saturating_sub(r.count) as u64;
        rows.push(
            nerve::to_gaia_json(&r.to_value())
                .with("service", s)
                .with("combined_cap", heal::MAX_RESTARTS_PER_HOUR as u64)
                .with("left_for_gaia", left),
        );
    }
    Ok(Json::obj()
        .with(
            "decision",
            "D-L2 accepted 2026-10-21: Gaia + nerve restarts <= 3 per service per rolling hour",
        )
        .with(
            "nerve_cycle",
            state.get("cycle").and_then(serde_json::Value::as_u64),
        )
        .with(
            "nerve_ts",
            state
                .get("ts")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        )
        .with("services", Json::Arr(rows)))
}
