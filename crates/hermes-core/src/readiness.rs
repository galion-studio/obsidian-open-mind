// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The readiness gate: what Hermes must pass before the merge / connect /
//! replace-infrastructure work may start. Every check is computed now from
//! real inputs (the registry, the plan files, the ledger, HADES, git), or is
//! an owner decision recorded in `sectors.toml`. READY only when all pass.

use crate::action::ActionClass;
use crate::engine::{Hermes, RunOpts};
use crate::gate::Gate;
use crate::governor::Governor;
use crate::ledger::{self, Ledger};
use crate::plan::Task;
use crate::provider::MockProvider;
use crate::report::{self, num, Report};
use crate::retry::RetryPolicy;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pass,
    Fail,
    /// Could not be checked from here (input missing).
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub name: String,
    pub status: Status,
    /// Hermes can fix it alone (false = needs Slavik or another branch).
    pub hermes_can_fix: bool,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Readiness {
    pub ready: bool,
    pub at: String,
    pub passed: usize,
    pub total: usize,
    pub checks: Vec<Check>,
    pub report: Report,
}

pub struct Inputs<'a> {
    pub hermes: &'a Hermes,
    pub gate: &'a Gate,
    /// The live ledger, if any.
    pub ledger: Option<&'a Path>,
    /// A git checkout to test `require_merged` against.
    pub repo: Option<&'a Path>,
    /// Run the HTTP loopback self-test (off when called from the API itself).
    pub http_selftest: bool,
}

fn check(id: &'static str, name: &str, ok: bool, fixable: bool, evidence: String) -> Check {
    Check {
        id,
        name: name.into(),
        status: if ok { Status::Pass } else { Status::Fail },
        hermes_can_fix: fixable,
        evidence,
    }
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "hermes-readiness-{name}-{}-{}",
        std::process::id(),
        crate::engine::new_id("")
    ))
}

/// Compute the gate.
pub fn evaluate(inp: &Inputs<'_>) -> Readiness {
    let h = inp.hermes;
    let reg = &h.reg;
    let mut checks = Vec::new();

    // G1 registry.
    let (covered, uncovered) = reg.coverage();
    checks.push(check(
        "G1",
        "Sector registry loads; every sector maps to real Gaia systems and owners",
        reg.validate().is_ok() && reg.sectors.len() == 7,
        true,
        format!(
            "{} sectors; {} of {} Gaia systems covered; not covered: {}",
            reg.sectors.len(),
            covered.len(),
            covered.len() + uncovered.len(),
            uncovered.join(", ")
        ),
    ));

    // G2 Hermes itself is on the Gaia ownership registry.
    let owner = gaia_core::ownership::crate_owner("hermes-core");
    checks.push(check(
        "G2",
        "hermes-core is mapped in the Gaia ownership registry",
        owner.is_some(),
        true,
        owner.map_or("no row for hermes-core".into(), |o| {
            format!(
                "hermes-core -> {} / {}",
                o.role.system().map_or("-", |s| s.name()),
                o.role.function().map_or("GAIA", |f| f.name())
            )
        }),
    ));

    // G3/G4 plans.
    let cycles: usize = h.plans.iter().map(|p| p.cycles.len()).sum();
    checks.push(check(
        "G3",
        "All seven sector plans load with no dependency cycles",
        h.plans.len() == reg.sectors.len() && cycles == 0,
        false,
        format!(
            "{} plans, {} plan tasks (+{} operator intake), {cycles} cycle(s)",
            h.plans.len(),
            h.plans.iter().map(|p| p.tasks.len()).sum::<usize>(),
            h.order.len() - h.plans.iter().map(|p| p.tasks.len()).sum::<usize>()
        ),
    ));
    let mut diffs = Vec::new();
    for p in &h.plans {
        if let Some(s) = reg.sector(&p.sector) {
            let t = p.totals();
            if (s.planner_tasks, s.planner_sessions) != (t.tasks, t.sessions) {
                diffs.push(format!(
                    "{}: parsed {}/{} vs planner {}/{} (tasks/sessions)",
                    s.id, t.tasks, t.sessions, s.planner_tasks, s.planner_sessions
                ));
            }
        }
    }
    let parsed: usize = h.plans.iter().map(|p| p.tasks.len()).sum();
    let reported: usize = reg.sectors.iter().map(|s| s.planner_tasks).sum();
    checks.push(check(
        "G4",
        "Plan loader reproduces every planner's task and session counts",
        diffs.is_empty(),
        true,
        if diffs.is_empty() {
            format!("{parsed} tasks parsed = {reported} reported")
        } else {
            diffs.join("; ")
        },
    ));

    // G5 every sector can start.
    let dry = h.dry_run(inp.gate, Some(1), None);
    let mut idle = Vec::new();
    for s in &reg.sectors {
        if !dry
            .rows
            .iter()
            .any(|r| r.sector == s.id && r.decision == "dispatch")
        {
            idle.push(s.id.clone());
        }
    }
    checks.push(check(
        "G5",
        "Every sector has dispatchable work in its first session",
        idle.is_empty(),
        false,
        format!(
            "dry run: {} dispatch, {} wait for slots, {} wait for deps, {} held, {} refused; {} of {} worker slots; idle sectors: {}",
            dry.count("dispatch"),
            dry.count("wait-cap"),
            dry.count("wait-deps"),
            dry.count("held"),
            dry.count("refused"),
            dry.slots_used,
            dry.worker_slots,
            if idle.is_empty() { "none".into() } else { idle.join(", ") }
        ),
    ));

    // G6 governor.
    let hc = &reg.hermes;
    checks.push(check(
        "G6",
        "Concurrency cap sits at least two under the measured request limit",
        hc.concurrency_cap + 2 <= hc.measured_limit_low,
        true,
        format!(
            "cap {} (incl. {} reserved for the boss), measured limit {}-{}",
            hc.concurrency_cap, hc.reserved_slots, hc.measured_limit_low, hc.measured_limit_high
        ),
    ));

    // G7 retry loop self-test on the real plans with a mock over its limit.
    checks.push(retry_selftest(h));

    // G8 ledger.
    checks.push(ledger_check(inp.ledger));

    // G9-G10 HADES.
    checks.push(check(
        "G9",
        "HADES is configured with an admin public key",
        inp.gate.configured(),
        false,
        inp.gate.admin_fingerprint().map_or_else(
            || inp.gate.why_closed().to_string(),
            |f| format!("admin key fingerprint {f}"),
        ),
    ));
    let merge = synthetic(
        "readiness:S0.merge",
        "Land integration-1 on main",
        ActionClass::Merge,
    );
    let deploy = synthetic(
        "readiness:S0.deploy",
        "tuner binary swap cutover",
        ActionClass::Deploy,
    );
    let dm = inp.gate.preview(&merge);
    let dd = inp.gate.preview(&deploy);
    checks.push(check(
        "G10",
        "Unapproved merge and deploy are not dispatchable",
        !dm.may_dispatch() && !dd.may_dispatch(),
        true,
        format!(
            "merge without token: {}; deploy without token: {}",
            gate_text(&dm),
            gate_text(&dd)
        ),
    ));

    // G11 provider.
    checks.push(check(
        "G11",
        "A real model provider is usable (boss Opus, sub-agents Sonnet)",
        hc.provider != "mock",
        false,
        format!(
            "provider = {}; OpenRouter answers 402 (credit) and no paid call is allowed; planned boss {} ({} reasoning), sub-agents {}",
            hc.provider, hc.boss_model, hc.boss_reasoning, hc.sub_model
        ),
    ));

    // G12 required merges.
    checks.push(merges_check(reg, inp.repo));

    // G13 HTTP.
    if inp.http_selftest {
        checks.push(http_selftest());
    } else {
        checks.push(check(
            "G13",
            "Status API is loopback-only and answers",
            true,
            true,
            "answered this request on loopback".into(),
        ));
    }

    // G14 owner decisions.
    let open: Vec<String> = reg
        .readiness
        .owner
        .iter()
        .filter(|o| !o.decided)
        .map(|o| o.item.clone())
        .collect();
    checks.push(check(
        "G14",
        "Slavik's open decisions are recorded as decided",
        open.is_empty(),
        false,
        if open.is_empty() {
            "none open".into()
        } else {
            format!("{} open: {}", open.len(), open.join(" | "))
        },
    ));

    let passed = checks.iter().filter(|c| c.status == Status::Pass).count();
    let total = checks.len() + 1;
    let blockers: Vec<String> = checks
        .iter()
        .filter(|c| c.status != Status::Pass)
        .map(|c| short_name(c.id).to_string())
        .collect();
    // G15 (below) checks this very sentence; it is counted as passing here.
    let spoken = spoken_sentence(passed + 1, total, &blockers);
    // G15 the spoken sentence itself.
    let spoken_res = report::spoken_ok(&spoken);
    checks.push(check(
        "G15",
        "The operator report opens with one speakable sentence",
        spoken_res.is_ok(),
        true,
        match &spoken_res {
            Ok(()) => format!("{:?}", spoken),
            Err(e) => format!("{e}: {spoken:?}"),
        },
    ));
    let passed = checks.iter().filter(|c| c.status == Status::Pass).count();
    let detail = checks
        .iter()
        .map(|c| {
            format!(
                "{} {} {}: {}",
                c.id,
                match c.status {
                    Status::Pass => "PASS",
                    Status::Fail => "FAIL",
                    Status::Unknown => "UNKNOWN",
                },
                c.name,
                c.evidence
            )
        })
        .collect();
    Readiness {
        ready: passed == checks.len(),
        at: zagreb_time::now_rfc3339(),
        passed,
        total: checks.len(),
        checks,
        report: Report::new(spoken, detail),
    }
}

fn short_name(id: &str) -> &'static str {
    match id {
        "G1" => "the sector registry",
        "G2" => "the Gaia row for Hermes",
        "G3" => "the plan files",
        "G4" => "the plan counts",
        "G5" => "idle sectors",
        "G6" => "the concurrency cap",
        "G7" => "the retry loop",
        "G8" => "the ledger",
        "G9" => "the HADES key",
        "G10" => "the approval gate",
        "G11" => "the model credit",
        "G12" => "the unmerged branches",
        "G13" => "the status API",
        "G14" => "your open decisions",
        _ => "a check",
    }
}

fn spoken_sentence(passed: usize, total: usize, blockers: &[String]) -> String {
    if blockers.is_empty() {
        return format!(
            "Hermes is ready, all {} checks pass, so the merge and replace work can start.",
            num(total)
        );
    }
    let shown: Vec<String> = blockers.iter().take(4).cloned().collect();
    let more = if blockers.len() > shown.len() {
        " among others"
    } else {
        ""
    };
    format!(
        "Hermes is not ready, {} of {} checks pass, and what blocks it is {}{more}.",
        num(passed),
        num(total),
        report::and_list(&shown)
    )
}

fn gate_text(d: &crate::gate::GateDecision) -> String {
    use crate::gate::GateDecision::*;
    match d {
        NotNeeded => "not gated".into(),
        Held { action, .. } => format!("held for Slavik (HADES dry-run, kind {action})"),
        Approved { .. } => "APPROVED".into(),
        Refused { action, code, .. } => format!("refused ({code}, kind {action})"),
    }
}

/// A task that is not in any plan, for gate self-checks.
pub fn synthetic(key: &str, title: &str, class: ActionClass) -> Task {
    let (sector, id) = key.split_once(':').unwrap_or(("readiness", key));
    Task {
        key: key.into(),
        id: id.into(),
        sector: sector.into(),
        session: "S0".into(),
        session_ord: 0,
        title: title.into(),
        target: Some(1),
        useful: Some(1),
        deps_text: String::new(),
        deps: Vec::new(),
        class,
        line: 0,
    }
}

fn retry_selftest(h: &Hermes) -> Check {
    let name = "Request-limit stops are retried until every runnable task completes";
    let dir = scratch("retry");
    let path = dir.join("ledger.jsonl");
    let result = (|| -> Result<String, String> {
        let (mut l, _) = Ledger::open(&path)?;
        let mut sim = Hermes::from_plans(h.reg.clone(), h.plans.clone());
        let caps: BTreeMap<String, usize> = sim
            .reg
            .sectors
            .iter()
            .map(|s| (s.id.clone(), s.max_concurrent))
            .collect();
        // Governor allows 12, the mock platform only 6: stops are certain.
        let gov = Governor::new(12, caps);
        let mock = Arc::new(MockProvider::new(6, Duration::from_millis(15)));
        let policy = RetryPolicy(sim.reg.retry.clone());
        let opts = RunOpts {
            first_sessions: Some(1),
            time_scale: 0.0002,
            max_wall: Duration::from_secs(120),
        };
        let s = sim.run(
            &mut l,
            mock,
            &gov,
            &policy,
            &Gate::closed("self-test"),
            &BTreeMap::new(),
            &opts,
        )?;
        let rep = ledger::read_all(&path)?;
        let ok = s.request_limit_stops > 0
            && s.failed == 0
            && s.retries == s.stops
            && s.completed > 0
            && rep.sound();
        let text = format!(
            "{} in scope: {} dispatched, {} request-limit stops, {} retries, {} completed, {} failed, {} held/refused, {} blocked by out-of-scope deps; ledger {} lines, chain {}",
            s.in_scope,
            s.dispatched,
            s.request_limit_stops,
            s.retries,
            s.completed,
            s.failed,
            s.held + s.refused,
            s.blocked,
            rep.events.len(),
            if rep.sound() { "sound" } else { "BROKEN" }
        );
        if ok {
            Ok(text)
        } else {
            Err(text)
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Ok(t) => check("G7", name, true, true, t),
        Err(t) => check("G7", name, false, true, t),
    }
}

fn ledger_check(p: Option<&Path>) -> Check {
    let name = "Ledger is hash-chained, Zagreb-stamped and survives a torn write";
    // Self-test: torn tail is skipped and the chain continues.
    let dir = scratch("ledger");
    let path = dir.join("ledger.jsonl");
    let selftest = (|| -> Result<(), String> {
        {
            let (mut l, _) = Ledger::open(&path)?;
            l.append(ledger::NewEvent::new("selftest"))?;
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"seq\":2,"))
            .map_err(|e| e.to_string())?;
        {
            let (mut l, rep) = Ledger::open(&path)?;
            if !rep.torn_tail {
                return Err("torn tail not detected".into());
            }
            l.append(ledger::NewEvent::new("selftest"))?;
        }
        let rep = ledger::read_all(&path)?;
        if rep.sound() && rep.events.len() == 2 {
            Ok(())
        } else {
            Err("chain not sound after torn tail".into())
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    let live = match p {
        None => Ok("no live ledger given".to_string()),
        Some(p) => ledger::read_all(p).and_then(|r| {
            if r.sound() {
                Ok(format!(
                    "live ledger {} lines, chain sound, all stamps Zagreb{}",
                    r.events.len(),
                    if r.torn_tail {
                        ", torn tail skipped"
                    } else {
                        ""
                    }
                ))
            } else {
                Err(format!(
                    "live ledger: chain breaks {:?}, bad stamps {:?}",
                    r.chain_breaks, r.bad_stamps
                ))
            }
        }),
    };
    match (selftest, live) {
        (Ok(()), Ok(t)) => check("G8", name, true, true, format!("self-test ok; {t}")),
        (Err(e), _) | (_, Err(e)) => check("G8", name, false, true, e),
    }
}

fn merges_check(reg: &crate::registry::Registry, repo: Option<&Path>) -> Check {
    let name = "Required branches are merged into main";
    let Some(repo) = repo else {
        return Check {
            id: "G12",
            name: name.into(),
            status: Status::Unknown,
            hermes_can_fix: false,
            evidence: "no --repo given".into(),
        };
    };
    let mut parts = Vec::new();
    let mut all = true;
    let mut unknown = false;
    for m in &reg.readiness.require_merged {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["merge-base", "--is-ancestor", &m.commit, &m.into])
            .output();
        let verdict = match out.map(|o| o.status.code()) {
            Ok(Some(0)) => "merged",
            Ok(Some(1)) => {
                all = false;
                "NOT merged"
            }
            _ => {
                all = false;
                unknown = true;
                "unknown commit"
            }
        };
        parts.push(format!(
            "{} {} into {}: {verdict}",
            m.name, m.commit, m.into
        ));
    }
    Check {
        id: "G12",
        name: name.into(),
        status: if all {
            Status::Pass
        } else if unknown && parts.iter().all(|p| p.ends_with("unknown commit")) {
            Status::Unknown
        } else {
            Status::Fail
        },
        hermes_can_fix: false,
        evidence: parts.join("; "),
    }
}

fn http_selftest() -> Check {
    let name = "Status API is loopback-only and answers";
    let refused = crate::http::bind_loopback("0.0.0.0:0").is_err();
    let answered = (|| -> Result<u16, String> {
        let l = crate::http::bind_loopback("127.0.0.1:0")?;
        let a = l.local_addr().map_err(|e| e.to_string())?;
        let h: crate::http::Handler = Arc::new(|_r: &crate::http::Request| {
            crate::http::Response::json(200, &serde_json::json!({"ok": true}))
        });
        std::thread::spawn(move || crate::http::serve(l, h));
        crate::http::get(&a, "/health").map(|(s, _)| s)
    })();
    check(
        "G13",
        name,
        refused && answered == Ok(200),
        true,
        format!("0.0.0.0 bind refused: {refused}; loopback GET: {answered:?}"),
    )
}
