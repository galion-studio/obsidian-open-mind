// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `hermes`: the Hermes coordinator CLI.
//!
//! ```text
//! hermes [--config F] [--plans DIR] [--state DIR] [--json] <command>
//!   sectors                       sectors, leads, Gaia functions/systems, plan totals
//!   agents                        boss, leads and sub-agent sessions from the ledger
//!   plan [--sector S]             tasks: class, slots, deps, state
//!   dispatch --dry-run [--first-sessions N] [--cap N]
//!   run --simulate [--first-sessions N] [--cap N] [--mock-limit N] [--work-ms N]
//!                  [--time-scale F] [--stall-every N] [--token TASK=FILE]...
//!   gate --task KEY [--token-file F] [--plan-out F]
//!   brief --task KEY              the self-contained brief a sub-agent receives
//!   ledger [--tail N] [--retries] [--verify]
//!   readiness [--repo DIR]
//!   report                        one spoken sentence plus detail
//!   intake --sector S --title T [--class C] [--deps K,K]   operator task intake
//!   serve --port P [--host 127.0.0.1]
//! ```
//!
//! Defaults: `--config` $HERMES_CONFIG or config/hermes/sectors.toml;
//! `--plans` $HERMES_PLANS_DIR or `hermes.plans_dir`; `--state`
//! $HERMES_STATE or state/hermes-core (ledger at `<state>/ledger.jsonl`).
//! Exit codes: 0 ok, 1 error, 2 usage, 3 readiness not met / gate refused.

use hermes_core::action::ActionClass;
use hermes_core::engine::{local_id, retry_table, Hermes, RunOpts, TaskState};
use hermes_core::gate::{plan_bytes, Gate, GateDecision};
use hermes_core::governor::Governor;
use hermes_core::http::{self, Request, Response};
use hermes_core::ledger::{self, Ledger, NewEvent};
use hermes_core::provider::{Brief, MockProvider, OpenRouter, Provider};
use hermes_core::readiness::{self, Inputs};
use hermes_core::registry::{Agent, Registry, Role};
use hermes_core::report::{num, Report};
use hermes_core::retry::RetryPolicy;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = "usage: hermes [--config F] [--plans DIR] [--state DIR] [--json] <sectors|agents|plan|dispatch --dry-run|run --simulate|gate|brief|ledger|readiness|report|intake|serve> [flags]";

#[derive(Clone, Debug)]
struct Ctx {
    config: PathBuf,
    plans: Option<PathBuf>,
    state: PathBuf,
    json: bool,
}

impl Ctx {
    fn ledger_path(&self) -> PathBuf {
        self.state.join("ledger.jsonl")
    }

    fn load(&self) -> Result<(Hermes, Gate), String> {
        let reg = Registry::load(&self.config)?;
        let plans = reg.plans_dir(self.plans.as_deref());
        let gate = Gate::from_config(&reg.hades.config);
        let mut h = Hermes::load(reg, &plans)?;
        let rep = ledger::read_all(&self.ledger_path())?;
        h.apply(&rep.events);
        Ok((h, gate))
    }
}

struct Args {
    flags: BTreeMap<String, Vec<String>>,
    switches: Vec<String>,
}

const SWITCHES: &[&str] = &["--dry-run", "--simulate", "--retries", "--verify", "--json"];

fn parse(rest: &[String]) -> Result<Args, String> {
    let mut flags: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut switches = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if SWITCHES.contains(&a.as_str()) {
            switches.push(a.clone());
        } else if let Some(k) = a.strip_prefix("--") {
            let v = it.next().ok_or_else(|| format!("--{k} needs a value"))?;
            flags.entry(k.to_string()).or_default().push(v.clone());
        } else {
            return Err(format!("unexpected argument {a:?}"));
        }
    }
    Ok(Args { flags, switches })
}

impl Args {
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.get(k).and_then(|v| v.last()).map(String::as_str)
    }
    fn num<T: std::str::FromStr>(&self, k: &str) -> Result<Option<T>, String> {
        self.get(k)
            .map(|v| {
                v.parse::<T>()
                    .map_err(|_| format!("--{k}: not a number: {v}"))
            })
            .transpose()
    }
    fn has(&self, s: &str) -> bool {
        self.switches.iter().any(|x| x == s)
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut ctx = Ctx {
        config: std::env::var_os("HERMES_CONFIG").map_or_else(
            || PathBuf::from("config/hermes/sectors.toml"),
            PathBuf::from,
        ),
        plans: std::env::var_os("HERMES_PLANS_DIR").map(PathBuf::from),
        state: std::env::var_os("HERMES_STATE")
            .map_or_else(|| PathBuf::from("state/hermes-core"), PathBuf::from),
        json: false,
    };
    let mut i = 0;
    while i < argv.len() && argv[i].starts_with("--") {
        let need = |i: usize| argv.get(i + 1).cloned();
        match argv[i].as_str() {
            "--json" => {
                ctx.json = true;
                i += 1;
                continue;
            }
            "--config" | "--plans" | "--state" => {
                let Some(v) = need(i) else {
                    eprintln!("hermes: {} needs a value\n{USAGE}", argv[i]);
                    return ExitCode::from(2);
                };
                match argv[i].as_str() {
                    "--config" => ctx.config = v.into(),
                    "--plans" => ctx.plans = Some(v.into()),
                    _ => ctx.state = v.into(),
                }
                i += 2;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("hermes: unknown flag {other}\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(cmd) = argv.get(i) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let args = match parse(&argv[i + 1..]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hermes: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.has("--json") {
        ctx.json = true;
    }
    let res = match cmd.as_str() {
        "sectors" => cmd_sectors(&ctx),
        "agents" => cmd_agents(&ctx),
        "plan" => cmd_plan(&ctx, &args),
        "dispatch" => cmd_dispatch(&ctx, &args),
        "run" => cmd_run(&ctx, &args),
        "gate" => cmd_gate(&ctx, &args),
        "brief" => cmd_brief(&ctx, &args),
        "ledger" => cmd_ledger(&ctx, &args),
        "readiness" => cmd_readiness(&ctx, &args),
        "report" => cmd_report(&ctx),
        "intake" => cmd_intake(&ctx, &args),
        "serve" => cmd_serve(&ctx, &args),
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    };
    match res {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("hermes: {e}");
            ExitCode::from(1)
        }
    }
}

fn out(ctx: &Ctx, v: &Value, text: impl FnOnce() -> String) {
    if ctx.json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        print!("{}", text());
    }
}

fn sectors_json(h: &Hermes) -> Value {
    let rows: Vec<Value> = h
        .reg
        .sectors
        .iter()
        .zip(&h.plans)
        .map(|(s, p)| {
            let t = p.totals();
            let gated = p.tasks.iter().filter(|t| t.class.gated()).count();
            json!({
                "id": s.id, "name": s.name, "lead": s.lead, "max_concurrent": s.max_concurrent,
                "gaia_functions": s.gaia_functions, "gaia_systems": s.gaia_systems,
                "plan": s.plan, "sessions": t.sessions, "tasks": t.tasks,
                "target_agents": t.target, "useful_agents": t.useful, "gated_tasks": gated,
                "planner": {"sessions": s.planner_sessions, "tasks": s.planner_tasks,
                            "target": s.planner_target, "useful": s.planner_useful},
            })
        })
        .collect();
    let (covered, uncovered) = h.reg.coverage();
    json!({
        "boss": h.reg.hermes.boss_agent, "boss_model": h.reg.hermes.boss_model,
        "sub_model": h.reg.hermes.sub_model, "provider": h.reg.hermes.provider,
        "concurrency_cap": h.reg.hermes.concurrency_cap,
        "measured_limit": [h.reg.hermes.measured_limit_low, h.reg.hermes.measured_limit_high],
        "sectors": rows, "gaia_systems_covered": covered.len(), "gaia_systems_uncovered": uncovered,
    })
}

fn cmd_sectors(ctx: &Ctx) -> Result<u8, String> {
    let (h, _) = ctx.load()?;
    let v = sectors_json(&h);
    out(ctx, &v, || {
        let mut s = format!(
            "boss {} ({}), sub-agents {}, provider {}, cap {} (measured limit {}-{})\n",
            h.reg.hermes.boss_agent,
            h.reg.hermes.boss_model,
            h.reg.hermes.sub_model,
            h.reg.hermes.provider,
            h.reg.hermes.concurrency_cap,
            h.reg.hermes.measured_limit_low,
            h.reg.hermes.measured_limit_high
        );
        s.push_str(&format!(
            "{:<16} {:<22} {:>3} {:>4} {:>5} {:>6} {:>6} {:>5}  gaia\n",
            "sector", "lead", "cap", "sess", "tasks", "target", "useful", "gated"
        ));
        for r in v["sectors"].as_array().into_iter().flatten() {
            s.push_str(&format!(
                "{:<16} {:<22} {:>3} {:>4} {:>5} {:>6} {:>6} {:>5}  {} :: {}\n",
                r["id"].as_str().unwrap_or(""),
                r["lead"].as_str().unwrap_or(""),
                n(&r["max_concurrent"]),
                n(&r["sessions"]),
                n(&r["tasks"]),
                n(&r["target_agents"]),
                n(&r["useful_agents"]),
                n(&r["gated_tasks"]),
                join(&r["gaia_functions"]),
                join(&r["gaia_systems"])
            ));
        }
        s.push_str(&format!(
            "Gaia systems covered: {} of 52; uncovered: {}\n",
            v["gaia_systems_covered"],
            join(&v["gaia_systems_uncovered"])
        ));
        s
    });
    Ok(0)
}

fn n(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

fn join(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}

fn agents_json(h: &Hermes, events: &[ledger::Event]) -> Value {
    let mut rows: Vec<Value> = h
        .reg
        .standing_agents()
        .iter()
        .map(|a: &Agent| serde_json::to_value(a).unwrap_or_default())
        .collect();
    let mut subs: BTreeMap<String, Value> = BTreeMap::new();
    for ev in events {
        let (Some(agent), Some(task)) = (&ev.agent, &ev.task) else {
            continue;
        };
        let sector = ev.sector.clone().unwrap_or_default();
        let e = subs.entry(agent.clone()).or_insert_with(|| {
            json!({"id": agent, "role": Role::Sub, "sector": sector, "task": task,
                   "model": h.reg.hermes.sub_model, "reports_to": format!("{sector}.lead")})
        });
        if let Some(s) = &ev.session {
            e["session"] = json!(s);
        }
        if ev.kind == "dispatched" {
            e["started"] = json!(ev.ts);
        }
        e["state"] = json!(ev.state.clone().unwrap_or_else(|| ev.kind.clone()));
        e["last"] = json!(ev.ts);
    }
    rows.extend(subs.into_values());
    json!({"agents": rows})
}

fn cmd_agents(ctx: &Ctx) -> Result<u8, String> {
    let (h, _) = ctx.load()?;
    let rep = ledger::read_all(&ctx.ledger_path())?;
    let v = agents_json(&h, &rep.events);
    out(ctx, &v, || {
        let mut s = String::new();
        let all = v["agents"].as_array().cloned().unwrap_or_default();
        let subs = all.iter().filter(|a| a["role"] == "sub").count();
        for a in all.iter().filter(|a| a["role"] != "sub") {
            s.push_str(&format!(
                "{:<5} {:<22} model {:<28} reports to {}\n",
                a["role"].as_str().unwrap_or(""),
                a["id"].as_str().unwrap_or(""),
                a["model"].as_str().unwrap_or(""),
                a["reports_to"].as_str().unwrap_or("operator")
            ));
        }
        s.push_str(&format!("sub-agent sessions in the ledger: {subs}\n"));
        for a in all.iter().filter(|a| a["role"] == "sub").take(40) {
            s.push_str(&format!(
                "  {:<30} {:<12} {:<20} {}\n",
                a["id"].as_str().unwrap_or(""),
                a["state"].as_str().unwrap_or(""),
                a["session"].as_str().unwrap_or(""),
                a["last"].as_str().unwrap_or("")
            ));
        }
        s
    });
    Ok(0)
}

fn cmd_plan(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let (h, _) = ctx.load()?;
    let only = args.get("sector");
    let rows: Vec<Value> = h
        .order
        .iter()
        .map(|k| &h.tasks[k])
        .filter(|rt| only.is_none_or(|s| rt.task.sector == s))
        .map(|rt| {
            json!({"task": rt.task.key, "session": rt.task.session, "class": rt.task.class,
                   "target": rt.task.target, "useful": rt.task.useful,
                   "slots": rt.task.slots(h.sector_cap(&rt.task.sector)),
                   "deps": rt.task.deps, "state": rt.state, "attempts": rt.attempts,
                   "title": rt.task.title, "line": rt.task.line})
        })
        .collect();
    let v = json!({"tasks": rows});
    out(ctx, &v, || {
        let mut s = format!(
            "{:<26} {:<11} {:>3} {:>3} {:>3} {:<10} {:<28} title\n",
            "task", "class", "tgt", "use", "slt", "state", "deps"
        );
        for r in &rows {
            let deps = join(&r["deps"]);
            let deps: String = deps.chars().take(28).collect();
            let title: String = r["title"].as_str().unwrap_or("").chars().take(70).collect();
            s.push_str(&format!(
                "{:<26} {:<11} {:>3} {:>3} {:>3} {:<10} {:<28} {}\n",
                r["task"].as_str().unwrap_or(""),
                r["class"].as_str().unwrap_or(""),
                r["target"].as_u64().map_or("-".into(), |n| n.to_string()),
                r["useful"].as_u64().map_or("-".into(), |n| n.to_string()),
                n(&r["slots"]),
                r["state"].as_str().unwrap_or(""),
                deps,
                title
            ));
        }
        s.push_str(&format!("{} task(s)\n", rows.len()));
        s
    });
    Ok(0)
}

fn cmd_dispatch(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    if !args.has("--dry-run") {
        return Err("live dispatch needs a real model provider; OpenRouter answers 402 and spend is not approved. Use `dispatch --dry-run` or `run --simulate` (offline mock).".into());
    }
    let (h, gate) = ctx.load()?;
    let plan = h.dry_run(&gate, args.num("first-sessions")?, args.num("cap")?);
    let v = serde_json::to_value(&plan).unwrap_or_default();
    out(ctx, &v, || {
        let mut s = format!(
            "dry run ({}): cap {} = {} worker slots + {} reserved for the boss; {} slots would be used\n",
            plan.scope, plan.cap, plan.worker_slots, plan.reserved, plan.slots_used
        );
        for r in &plan.rows {
            let gate = match &r.gate {
                GateDecision::NotNeeded => String::new(),
                GateDecision::Held { action, .. } => format!(" [HADES held: {action}]"),
                GateDecision::Approved { .. } => " [HADES approved]".into(),
                GateDecision::Refused { action, code, .. } => {
                    format!(" [HADES refused {code}: {action}]")
                }
            };
            let title: String = r.title.chars().take(64).collect();
            s.push_str(&format!(
                "{:<10} {:<26} {:<11} slots {:<2} {:<28}{} {}\n",
                r.decision,
                r.task,
                r.class,
                r.slots,
                r.agent.clone().unwrap_or_else(|| {
                    if r.deps_pending.is_empty() {
                        "-".into()
                    } else {
                        format!("after {}", r.deps_pending.join(","))
                    }
                }),
                gate,
                title
            ));
        }
        s.push_str(&format!(
            "per sector (used/cap): {}\n",
            plan.per_sector
                .iter()
                .map(|(k, (u, c))| format!("{k} {u}/{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        s.push_str(&format!(
            "totals: {} dispatch, {} wait for slots, {} wait for deps, {} held, {} refused\n",
            plan.count("dispatch"),
            plan.count("wait-cap"),
            plan.count("wait-deps"),
            plan.count("held"),
            plan.count("refused")
        ));
        s
    });
    Ok(0)
}

fn cmd_run(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    if !args.has("--simulate") {
        return Err("only `run --simulate` exists: the offline mock provider. Real runs wait for OpenRouter credit and Slavik's approval.".into());
    }
    let (mut h, gate) = ctx.load()?;
    let (mut l, _) = Ledger::open(&ctx.ledger_path())?;
    let cap: usize = args.num("cap")?.unwrap_or(h.reg.hermes.concurrency_cap);
    let reserved = h.reg.hermes.reserved_slots.min(cap.saturating_sub(1));
    let caps: BTreeMap<String, usize> = h
        .reg
        .sectors
        .iter()
        .map(|s| (s.id.clone(), s.max_concurrent))
        .collect();
    let gov = Governor::new(cap - reserved, caps);
    let mock_limit: usize = args
        .num("mock-limit")?
        .unwrap_or(h.reg.hermes.measured_limit_low);
    let work = Duration::from_millis(args.num("work-ms")?.unwrap_or(40));
    let stall_every: u64 = args.num("stall-every")?.unwrap_or(0);
    let provider: Arc<dyn Provider> = if h.reg.hermes.provider == "openrouter" {
        Arc::new(OpenRouter)
    } else {
        // The boss holds `reserved` slots on the platform too.
        Arc::new(
            MockProvider::new(mock_limit.saturating_sub(reserved).max(1), work)
                .with_stall_every(stall_every),
        )
    };
    let mut tokens = BTreeMap::new();
    for t in args.flags.get("token").into_iter().flatten() {
        let (task, file) = t.split_once('=').ok_or("--token wants TASK=FILE")?;
        let tok = std::fs::read_to_string(file).map_err(|e| format!("read {file}: {e}"))?;
        tokens.insert(task.to_string(), tok.trim().to_string());
    }
    let opts = RunOpts {
        first_sessions: args.num("first-sessions")?,
        time_scale: args.num("time-scale")?.unwrap_or(0.001),
        max_wall: Duration::from_secs(args.num("max-wall-secs")?.unwrap_or(900)),
    };
    let policy = RetryPolicy(h.reg.retry.clone());
    let sum = h.run(&mut l, provider, &gov, &policy, &gate, &tokens, &opts)?;
    let spoken = format!(
        "The run finished with {} tasks done, {} stops retried and {} failed, and {} wait for Slavik.",
        num(sum.completed),
        num(sum.retries as usize),
        num(sum.failed),
        num(sum.held + sum.refused)
    );
    let v = json!({"summary": sum, "spoken": spoken, "ledger": ctx.ledger_path()});
    out(ctx, &v, || {
        format!(
            "{spoken}\n  run {}: {} in scope, {} dispatched, {} stops ({} request-limit, {} stall), {} retries, {} completed, {} failed, {} held, {} refused, {} blocked; peak {} of {} worker slots; {} ms\n  ledger {}\n",
            sum.run,
            sum.in_scope,
            sum.dispatched,
            sum.stops,
            sum.request_limit_stops,
            sum.stall_stops,
            sum.retries,
            sum.completed,
            sum.failed,
            sum.held,
            sum.refused,
            sum.blocked,
            sum.peak_slots,
            gov.cap(),
            sum.wall_ms,
            ctx.ledger_path().display()
        )
    });
    Ok(if sum.failed == 0 { 0 } else { 3 })
}

fn cmd_intake(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let sector = args.get("sector").ok_or("--sector S is required")?;
    let title = args.get("title").ok_or("--title T is required")?;
    let class = args
        .get("class")
        .map(|c| {
            ActionClass::parse(c).ok_or(format!(
                "--class {c}: read-only|code|merge|deploy|destructive"
            ))
        })
        .transpose()?;
    let deps: Vec<String> = args
        .get("deps")
        .map(|d| {
            d.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let (mut l, _) = Ledger::open(&ctx.ledger_path())?;
    let (h, _) = ctx.load()?;
    let t = h.intake_task(sector, title, class, deps)?;
    let ev = l.append(
        NewEvent::new("intake")
            .task(&t.key, &t.sector)
            .state("queued")
            .detail(serde_json::to_string(&t).map_err(|e| e.to_string())?),
    )?;
    let v = json!({"task": t, "seq": ev.seq, "ts": ev.ts});
    out(ctx, &v, || {
        format!(
            "queued {} ({}){} at {}\n",
            t.key,
            t.class.as_str(),
            if t.class.gated() {
                ", needs HADES approval"
            } else {
                ""
            },
            ev.ts
        )
    });
    Ok(0)
}

fn find_task<'a>(h: &'a Hermes, key: &str) -> Result<&'a hermes_core::engine::TaskRt, String> {
    h.tasks
        .get(key)
        .ok_or_else(|| format!("no task {key:?} (see `hermes plan`)"))
}

fn cmd_gate(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let key = args.get("task").ok_or("--task KEY is required")?;
    let (h, gate) = ctx.load()?;
    let rt = find_task(&h, key)?;
    if let Some(p) = args.get("plan-out") {
        std::fs::write(p, plan_bytes(&rt.task)).map_err(|e| format!("write {p}: {e}"))?;
    }
    let decision = match args.get("token-file") {
        Some(f) => gate.check_with_file(&rt.task, Path::new(f))?,
        None => gate.check(&rt.task, ""),
    };
    let (mut l, _) = Ledger::open(&ctx.ledger_path())?;
    let (kind, state) = match &decision {
        GateDecision::NotNeeded => ("gate", None),
        GateDecision::Held { .. } => ("held", Some(TaskState::Held)),
        GateDecision::Approved { .. } => ("approved", Some(TaskState::Queued)),
        GateDecision::Refused { .. } => ("refused", Some(TaskState::Refused)),
    };
    let mut ev = NewEvent::new(kind)
        .task(&rt.task.key, &rt.task.sector)
        .detail(serde_json::to_string(&decision).unwrap_or_default());
    if let Some(s) = state {
        ev = ev.state(s.as_str());
    }
    l.append(ev)?;
    let v = json!({"task": key, "class": rt.task.class, "decision": decision,
                   "hades_configured": gate.configured()});
    out(ctx, &v, || {
        format!(
            "{key} ({}): {}\n",
            rt.task.class.as_str(),
            serde_json::to_string(&decision).unwrap_or_default()
        )
    });
    Ok(match decision {
        GateDecision::Refused { .. } => 3,
        _ => 0,
    })
}

fn cmd_brief(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let key = args.get("task").ok_or("--task KEY is required")?;
    let (h, _) = ctx.load()?;
    let rt = find_task(&h, key)?;
    let s = &rt.task.sector;
    let b = Brief {
        task: key.into(),
        sector: s.clone(),
        lead: h.reg.sector(s).map_or_else(String::new, |x| x.lead.clone()),
        agent: format!("{s}.{}.a{}", local_id(key), rt.attempts + 1),
        session: "(new session at dispatch)".into(),
        model: h.reg.hermes.sub_model.clone(),
        attempt: rt.attempts + 1,
        slots: rt.task.slots(h.sector_cap(s)) as u32,
        title: rt.task.title.clone(),
        deps: rt.task.deps.clone(),
    };
    let v = json!({"brief": b, "text": b.render()});
    out(ctx, &v, || b.render());
    Ok(0)
}

fn cmd_ledger(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let rep = ledger::read_all(&ctx.ledger_path())?;
    if args.has("--verify") {
        let v = json!({"lines": rep.events.len(), "sound": rep.sound(), "torn_tail": rep.torn_tail,
                       "chain_breaks": rep.chain_breaks, "bad_stamps": rep.bad_stamps, "head": rep.head});
        out(ctx, &v, || {
            format!(
                "{} lines; chain {}; Zagreb stamps {}; torn tail {}; head {}\n",
                rep.events.len(),
                if rep.chain_breaks.is_empty() {
                    "sound".into()
                } else {
                    format!("BROKEN at {:?}", rep.chain_breaks)
                },
                if rep.bad_stamps.is_empty() {
                    "ok".into()
                } else {
                    format!("BAD at {:?}", rep.bad_stamps)
                },
                rep.torn_tail,
                rep.head
            )
        });
        return Ok(if rep.sound() { 0 } else { 3 });
    }
    if args.has("--retries") {
        let rows = retry_table(&rep.events);
        let v = json!({"retries": rows});
        out(ctx, &v, || {
            let mut s = format!(
                "{:<26} {:>5} {:>5} {:>6} {:>7} {:>8}  {:<25} outcome\n",
                "task", "stops", "limit", "stalls", "retries", "attempts", "latest start"
            );
            for r in &rows {
                s.push_str(&format!(
                    "{:<26} {:>5} {:>5} {:>6} {:>7} {:>8}  {:<25} {}\n",
                    r.task,
                    r.stops,
                    r.request_limit,
                    r.stalls,
                    r.retries,
                    r.attempts,
                    r.latest_start,
                    r.outcome
                ));
            }
            s.push_str(&format!(
                "{} task(s) were stopped at least once\n",
                rows.len()
            ));
            s
        });
        return Ok(0);
    }
    let tail: usize = args.num("tail")?.unwrap_or(30);
    let start = rep.events.len().saturating_sub(tail);
    let evs = &rep.events[start..];
    let v = json!({"events": evs, "total": rep.events.len()});
    out(ctx, &v, || {
        let mut s = String::new();
        for e in evs {
            s.push_str(&format!(
                "{:>5} {} {:<15} {:<26} {:<24} {}\n",
                e.seq,
                e.ts,
                e.kind,
                e.task.clone().unwrap_or_default(),
                e.agent.clone().unwrap_or_default(),
                e.detail.chars().take(90).collect::<String>()
            ));
        }
        s
    });
    Ok(0)
}

fn readiness_for(
    ctx: &Ctx,
    repo: Option<&Path>,
    http_selftest: bool,
) -> Result<readiness::Readiness, String> {
    let (h, gate) = ctx.load()?;
    let lp = ctx.ledger_path();
    let ledger = lp.exists().then_some(lp.as_path());
    Ok(readiness::evaluate(&Inputs {
        hermes: &h,
        gate: &gate,
        ledger,
        repo,
        http_selftest,
    }))
}

fn cmd_readiness(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let repo = args.get("repo").map(PathBuf::from);
    let r = readiness_for(ctx, repo.as_deref(), true)?;
    let v = serde_json::to_value(&r).unwrap_or_default();
    out(ctx, &v, || {
        format!(
            "{}  {} of {} pass; ready: {} ({})\n",
            r.report.render(),
            r.passed,
            r.total,
            r.ready,
            r.at
        )
    });
    Ok(if r.ready { 0 } else { 3 })
}

fn status_report(h: &Hermes, events: &[ledger::Event]) -> Report {
    let c = h.state_counts();
    let g = |k: &str| c.get(k).copied().unwrap_or(0);
    let total = h.order.len();
    let running = g("running");
    let waiting = g("held") + g("refused");
    let spoken = format!(
        "Hermes holds {} tasks across {} sectors, {} running, {} done and {} waiting for you.",
        total,
        num(h.reg.sectors.len()),
        num(running),
        num(g("done")),
        num(waiting)
    );
    let mut detail: Vec<String> = c.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let retries = retry_table(events);
    detail.push(format!(
        "retry ledger: {} task(s) stopped at least once, {} retries scheduled",
        retries.len(),
        retries.iter().map(|r| r.retries).sum::<u32>()
    ));
    if let Some(last) = events.last() {
        detail.push(format!(
            "last ledger event {} {} at {}",
            last.seq, last.kind, last.ts
        ));
    }
    Report::new(spoken, detail)
}

fn cmd_report(ctx: &Ctx) -> Result<u8, String> {
    let (h, _) = ctx.load()?;
    let rep = ledger::read_all(&ctx.ledger_path())?;
    let r = status_report(&h, &rep.events);
    let v = serde_json::to_value(&r).unwrap_or_default();
    out(ctx, &v, || r.render());
    Ok(0)
}

fn api(ctx: &Ctx, req: &Request) -> Response {
    let res = (|| -> Result<Response, String> {
        let p = req.path.trim_end_matches('/');
        if p == "/health" || p.is_empty() {
            return Ok(Response::json(
                200,
                &json!({
                "status": "ok", "service": "hermes-core", "version": env!("CARGO_PKG_VERSION"),
                "at": zagreb_time::now_rfc3339(), "read_only": true}),
            ));
        }
        let (h, gate) = ctx.load()?;
        let rep = ledger::read_all(&ctx.ledger_path())?;
        Ok(match p {
            "/v1/sectors" => Response::json(200, &sectors_json(&h)),
            "/v1/agents" => Response::json(200, &agents_json(&h, &rep.events)),
            "/v1/tasks" => {
                let rows: Vec<Value> = h
                    .order
                    .iter()
                    .map(|k| &h.tasks[k])
                    .filter(|rt| req.param("sector").is_none_or(|s| rt.task.sector == s))
                    .filter(|rt| req.param("state").is_none_or(|s| rt.state.as_str() == s))
                    .map(|rt| serde_json::to_value(rt).unwrap_or_default())
                    .collect();
                Response::json(200, &json!({"count": rows.len(), "tasks": rows}))
            }
            "/v1/dispatch" => {
                let first = req.param("first_sessions").and_then(|v| v.parse().ok());
                let cap = req.param("cap").and_then(|v| v.parse().ok());
                Response::json(200, &serde_json::to_value(h.dry_run(&gate, first, cap)).unwrap_or_default())
            }
            "/v1/ledger" => {
                let tail: usize = req.param("tail").and_then(|v| v.parse().ok()).unwrap_or(50).min(1000);
                let start = rep.events.len().saturating_sub(tail);
                Response::json(200, &json!({"total": rep.events.len(), "sound": rep.sound(),
                    "torn_tail": rep.torn_tail, "events": rep.events[start..]}))
            }
            "/v1/retries" => Response::json(200, &json!({"retries": retry_table(&rep.events)})),
            "/v1/governor" => {
                let mut per: BTreeMap<String, u32> = BTreeMap::new();
                let mut slots: BTreeMap<String, u32> = BTreeMap::new();
                for ev in &rep.events {
                    if let (Some(t), Some(n)) = (&ev.task, ev.slots) {
                        slots.insert(t.clone(), n);
                    }
                }
                for rt in h.tasks.values().filter(|r| r.state == TaskState::Running) {
                    *per.entry(rt.task.sector.clone()).or_insert(0) +=
                        slots.get(&rt.task.key).copied().unwrap_or(1);
                }
                let used: u32 = per.values().sum();
                Response::json(200, &json!({
                    "cap": h.reg.hermes.concurrency_cap, "reserved": h.reg.hermes.reserved_slots,
                    "worker_slots": h.reg.worker_slots(), "running_slots": used, "per_sector": per,
                    "sector_caps": h.reg.sectors.iter().map(|s| (s.id.clone(), s.max_concurrent)).collect::<BTreeMap<_,_>>(),
                    "measured_limit": [h.reg.hermes.measured_limit_low, h.reg.hermes.measured_limit_high]}))
            }
            "/v1/readiness" => {
                let lp = ctx.ledger_path();
                let r = readiness::evaluate(&Inputs {
                    hermes: &h,
                    gate: &gate,
                    ledger: lp.exists().then_some(lp.as_path()),
                    repo: None,
                    http_selftest: false,
                });
                Response::json(200, &serde_json::to_value(r).unwrap_or_default())
            }
            "/v1/report" => Response::json(200, &serde_json::to_value(status_report(&h, &rep.events)).unwrap_or_default()),
            _ => Response::error(404, "unknown route; see /health, /v1/{sectors,agents,tasks,dispatch,ledger,retries,governor,readiness,report}"),
        })
    })();
    res.unwrap_or_else(|e| Response::error(500, &e))
}

fn cmd_serve(ctx: &Ctx, args: &Args) -> Result<u8, String> {
    let port: u16 = args.num("port")?.ok_or("--port P is required")?;
    let host = args.get("host").unwrap_or("127.0.0.1");
    let addr = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let listener = http::bind_loopback(&addr)?;
    // Fail fast on a broken config rather than on the first request.
    ctx.load()?;
    eprintln!(
        "{} hermes serve: listening on {} (read-only, loopback)",
        zagreb_time::now_rfc3339(),
        listener.local_addr().map_err(|e| e.to_string())?
    );
    let c = ctx.clone();
    http::serve(listener, Arc::new(move |r: &Request| api(&c, r)));
    Ok(0)
}
