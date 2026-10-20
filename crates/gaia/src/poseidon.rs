// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia poseidon ...` and `gaia dispatch --queue`: GAIA with the real
//! POSEIDON backend (`gaia_poseidon::queue`) talking to a running
//! `openmind-queue`. Every command goes through `Gaia::dispatch`, so routing
//! follows the ownership table (`FunctionId::COMPUTE`), lineage is recorded
//! and containment applies, exactly as for the stand-in.
//!
//! Exit codes: 0 done or dry-run, 3 queue unreachable, 4 refused (quota,
//! spend cap, money, backpressure, VRAM, read-only budget), 5 rejected.

use gaia_core::{FunctionId, Gaia, Json, Outcome, Report, System, Verb};
use gaia_poseidon::{Poseidon, QueueConfig};
use serde_json::{json, Map, Value};

pub const USAGE: &str = "  poseidon [--queue ADDR] SUB      the real POSEIDON backend: forwards to openmind-queue on loopback
                                 (ADDR or $GAIA_POSEIDON_QUEUE, e.g. 127.0.0.1:19240); budget is read-only
                                 and Gaia never leases or acks, so it never spends. SUB is one of:
    status                       health plus the four compute systems (vram-pool, quotas, queues, budget-caps)
    submit [--queue-name Q] [--agent A] [--kind K] [--priority P] [--cost C] [--max-attempts N]
           [--lease-ms N] [--vram-gb G] [--cpu-fallback] [--job-payload TEXT]
                                 submit one job (queues run); quota, spend-cap, money, backpressure and
                                 VRAM refusals come back as outcome refused (exit 4)
    job ID                       inspect one live job (queues status job:ID)
    dlq                          list dead-lettered jobs
    vram                         VRAM pool status with devices
    plan-vram GB                 dry-run placement of GB of VRAM
    quotas | caps                quota / spend-cap status (read-only)
  dispatch ... --queue ADDR      route a directive with the real POSEIDON backend";

/// GAIA with all nine functions, POSEIDON on the real queue backend.
pub fn build_with_queue(cfg: QueueConfig) -> Gaia {
    crate::build_with(Box::new(Poseidon::with_queue(cfg)))
}

/// Booted (one tick, which also refreshes POSEIDON's health from the queue).
pub fn booted_with_queue(cfg: QueueConfig) -> Gaia {
    let mut g = build_with_queue(cfg);
    g.boot();
    g.tick();
    g
}

/// Queue config from `--queue ADDR` or `$GAIA_POSEIDON_QUEUE`.
pub fn queue_config(flag: Option<&str>) -> Result<QueueConfig, String> {
    match flag {
        Some(a) => QueueConfig::new(a),
        None => QueueConfig::from_env().unwrap_or_else(|| {
            Err(format!(
                "no queue address: pass --queue ADDR or set {}",
                gaia_poseidon::queue::ENV_ADDR
            ))
        }),
    }
}

/// Exit code for a report.
pub fn exit_for(r: &Report) -> u8 {
    if r.metric_value("unreachable") == Some(1) {
        return 3;
    }
    match r.outcome {
        Outcome::Done | Outcome::DryRun => 0,
        Outcome::Refused => 4,
        Outcome::Rejected | Outcome::Contained => 5,
    }
}

fn send(g: &mut Gaia, s: System, v: Verb, payload: &str) -> Report {
    let d = g.directive(s, v).payload(payload);
    g.dispatch(&d)
}

fn wrap(g: &Gaia, r: &Report, addr: &str) -> Json {
    Json::obj()
        .with("owner", FunctionId::COMPUTE.name())
        .with("backend", "openmind-queue")
        .with("queue", addr)
        .with("report", r.to_json())
        .with(
            "poseidon_state",
            g.function_state(FunctionId::COMPUTE)
                .map(gaia_core::Lifecycle::name),
        )
}

fn opt<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| *a == name)
        .and_then(|i| args.get(i + 1).copied())
}

/// Build the job JSON from CLI flags. Numbers are checked here so a typo is
/// a usage error, not a queue 400.
pub fn job_spec(args: &[&str]) -> Result<String, String> {
    let mut m = Map::new();
    for (flag, key) in [
        ("--queue-name", "queue"),
        ("--agent", "agent"),
        ("--kind", "kind"),
        ("--job-payload", "payload"),
    ] {
        if let Some(v) = opt(args, flag) {
            m.insert(key.into(), json!(v));
        }
    }
    for (flag, key) in [
        ("--cost", "cost"),
        ("--max-attempts", "max_attempts"),
        ("--lease-ms", "lease_ms"),
        ("--vram-gb", "vram_gb"),
    ] {
        if let Some(v) = opt(args, flag) {
            let n: u64 = v
                .parse()
                .map_err(|_| format!("{flag} needs a non-negative number, got {v:?}"))?;
            m.insert(key.into(), json!(n));
        }
    }
    if let Some(v) = opt(args, "--priority") {
        let n: i32 = v
            .parse()
            .map_err(|_| format!("--priority needs a number, got {v:?}"))?;
        m.insert("priority".into(), json!(n));
    }
    if args.contains(&"--cpu-fallback") {
        m.insert("cpu_fallback".into(), json!(true));
    }
    Ok(Value::Object(m).to_string())
}

/// Run `gaia poseidon ...`. Returns the JSON and the exit code.
pub fn run(args: &[&str]) -> Result<(Json, u8), String> {
    let cfg = queue_config(opt(args, "--queue"))?;
    let addr = cfg.addr.clone();
    // Positional words, skipping flags and their values.
    let mut pos = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i].starts_with("--") {
            i += if args[i] == "--cpu-fallback" { 1 } else { 2 };
        } else {
            pos.push(args[i]);
            i += 1;
        }
    }
    let mut g = booted_with_queue(cfg);
    let one = |g: &mut Gaia, s, v, p: &str| {
        let r = send(g, s, v, p);
        let code = exit_for(&r);
        (wrap(g, &r, &addr), code)
    };
    let out = match pos.first().copied() {
        Some("status") => {
            let mut reports = Vec::new();
            let mut code = 0;
            for s in System::COMPUTE {
                let r = send(&mut g, s, Verb::Status, "");
                code = code.max(exit_for(&r));
                reports.push(r.to_json());
            }
            let f = FunctionId::COMPUTE;
            (
                Json::obj()
                    .with("owner", f.name())
                    .with("backend", "openmind-queue")
                    .with("queue", addr.as_str())
                    .with("state", g.function_state(f).map(gaia_core::Lifecycle::name))
                    .with("health", g.function_health(f).map(|h| h.to_json()))
                    .with("adapter", g.function_json(f))
                    .with("systems", Json::Arr(reports)),
                code,
            )
        }
        Some("submit") => one(&mut g, System::Queues, Verb::Run, &job_spec(args)?),
        Some("job") => {
            let id = pos.get(1).ok_or("job needs an ID")?;
            id.parse::<u64>()
                .map_err(|_| format!("job ID must be a number, got {id:?}"))?;
            one(&mut g, System::Queues, Verb::Status, &format!("job:{id}"))
        }
        Some("dlq") => one(&mut g, System::Queues, Verb::Status, "dlq"),
        Some("vram") => one(&mut g, System::VramPool, Verb::Status, ""),
        Some("plan-vram") => {
            let gb = pos.get(1).ok_or("plan-vram needs GB")?;
            one(&mut g, System::VramPool, Verb::Plan, gb)
        }
        Some("quotas") => one(&mut g, System::Quotas, Verb::Status, ""),
        Some("caps") => one(&mut g, System::BudgetCaps, Verb::Status, ""),
        Some(other) => return Err(format!("unknown poseidon command {other:?}")),
        None => return Err("poseidon needs a command".into()),
    };
    Ok(out)
}

/// `gaia dispatch SYSTEM VERB --payload P --queue ADDR`.
pub fn dispatch(
    cfg: QueueConfig,
    system: &str,
    verb: &str,
    payload: &str,
    approve: Option<&str>,
) -> Result<(Json, u8), String> {
    let s = System::parse(system).ok_or_else(|| format!("unknown system {system:?}"))?;
    let v = Verb::parse(verb).ok_or_else(|| format!("unknown verb {verb:?}"))?;
    let mut g = booted_with_queue(cfg);
    let mut d = g.directive(s, v).payload(payload);
    if let Some(a) = approve {
        d = d.approved_by(a);
    }
    let r = g.dispatch(&d);
    let code = if s.is_compute() { exit_for(&r) } else { 0 };
    Ok((r.to_json(), code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_spec_from_flags() {
        let s = job_spec(&[
            "submit",
            "--agent",
            "a1",
            "--cost",
            "20",
            "--vram-gb",
            "16",
            "--cpu-fallback",
            "--priority",
            "-1",
        ])
        .unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["agent"], "a1");
        assert_eq!(v["cost"], 20);
        assert_eq!(v["vram_gb"], 16);
        assert_eq!(v["cpu_fallback"], true);
        assert_eq!(v["priority"], -1);
        assert!(job_spec(&["--cost", "-3"]).is_err());
    }

    #[test]
    fn unreachable_queue_degrades_poseidon_and_refuses() {
        // Port 19249 in this slot's range; nothing listens there in unit tests.
        let cfg = QueueConfig::new("127.0.0.1:19249").unwrap();
        let mut g = booted_with_queue(cfg);
        g.tick();
        assert_eq!(
            g.function_state(FunctionId::COMPUTE),
            Some(gaia_core::Lifecycle::Degraded)
        );
        let r = send(&mut g, System::Queues, Verb::Status, "");
        assert_eq!(r.outcome, Outcome::Refused);
        assert_eq!(exit_for(&r), 3);
    }
}
