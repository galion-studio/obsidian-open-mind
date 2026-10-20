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
use gaia_live::heal::{self, Ledger, TestTarget};
use gaia_live::nerve;
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
pub const PLAN_HEADER: &str = "gaia heal-apply plan v1 (box test service)";

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
}

/// The exact plan the admin signs for one restart of `t`. Deterministic: the
/// same target always gives the same bytes (joined with `\n` by
/// [`hades_gate::adapter::gaia_plan_bytes`]), so `gaia heal-plan` and
/// `gaia heal-tick` agree on the hash. The start command is JSON-quoted so
/// argument boundaries are part of what is signed.
#[must_use]
pub fn heal_plan(t: &TestTarget) -> Vec<String> {
    let argv = serde_json::to_string(&t.argv).unwrap_or_default();
    vec![
        PLAN_HEADER.to_string(),
        format!(
            "service {} on 127.0.0.1:{} (box test ports {}-{} only)",
            t.name,
            t.port,
            heal::TEST_PORTS.start(),
            heal::TEST_PORTS.end()
        ),
        format!("probe GET {} must answer status ok", t.health_path),
        format!("start {argv} detached in its own process group, stdin closed, no shell"),
        format!(
            "at most {} restarts of {} per rolling hour",
            heal::MAX_RESTARTS_PER_HOUR,
            t.name
        ),
        "record the result on the nerve".to_string(),
    ]
}

/// SHA-256 (hex) of the plan bytes for `t`.
#[must_use]
pub fn heal_plan_sha256(t: &TestTarget) -> String {
    to_hex(&plan_hash(&hades_gate::adapter::gaia_plan_bytes(
        &heal_plan(t),
    )))
}

fn load_approver(config: &Path) -> Result<Approver, String> {
    Approver::from_config(&Config::load(config)?)
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
    let posted = nerve_post(cfg, "gaia.heal.refused", "warn", data);
    HealTick {
        exit: exit::REFUSED,
        report: result(cfg, steps, "refused", why, posted.err()),
    }
}

/// The restart cap is spent: record it and stop (no token is consumed).
fn capped(
    cfg: &HealCfg,
    mut steps: Vec<Json>,
    retry: u64,
    probe_id: &str,
    plan_sha: &str,
) -> HealTick {
    let t = &cfg.target;
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
        json!({"service": t.name, "port": t.port, "restarts_last_hour": heal::MAX_RESTARTS_PER_HOUR, "cap": heal::MAX_RESTARTS_PER_HOUR, "retry_after_s": retry, "probe_id": probe_id, "plan_sha256": plan_sha, "token_consumed": false}),
    );
    HealTick {
        exit: exit::CAPPED,
        report: result(cfg, steps, "capped", &why, posted.err()),
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
    let approver = match load_approver(&cfg.hades_config) {
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
    let ledger = Ledger::new(&cfg.state_dir);
    let history = ledger
        .history(&t.name)
        .map_err(|e| format!("ledger: {e}"))?;
    let now = heal::epoch_now();
    let admin_first =
        approver.is_disabled() || hades_gate::safety::review(&t.name, &plan_bytes).is_err();
    if !admin_first {
        if let Err(retry) = heal::cap_check(&history, now) {
            return Ok(capped(cfg, steps, retry, &probe_id, &plan_sha));
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

    // 5. Healer apply: one restart, inside the cap.
    let used = match heal::cap_check(&history, now) {
        Ok(used) => used,
        Err(retry) => return Ok(capped(cfg, steps, retry, &probe_id, &plan_sha)),
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
