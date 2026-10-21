// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `hermes-bench run`: one benchmark, one pluggable agent, one score JSON in
//! the galion-bench / bench-scoreboard shape (`{"pct", "axes": [{name, score}]}`
//! plus evidence fields).

use crate::live::{live_cfg, live_session, mock_llm_session};
use crate::mockllm::MockLlm;
use crate::planner::{Brain, ScriptFactory};
use crate::session::HermesSession;
use crate::transport::Transport;
use crate::{banking, banking_adv, vending, wallstreet};
use galion_bench::Scorecard;
use hermes_rs::provider::EnvFn;
use hermes_rs::tools::Registry;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    /// Deterministic scripts as the hermes-rs model (no network).
    Scripted,
    /// The same scripts served over the OpenAI wire on loopback (curl path).
    MockLlm,
    /// OpenRouter, only with GALION_BENCH_LIVE_MODEL=1 and the key present.
    Live,
}

impl AgentKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "scripted" => Some(AgentKind::Scripted),
            "mock-llm" => Some(AgentKind::MockLlm),
            "live" => Some(AgentKind::Live),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            AgentKind::Scripted => "scripted",
            AgentKind::MockLlm => "mock-llm",
            AgentKind::Live => "live",
        }
    }
}

pub struct Backends {
    pub bank_url: String,
    pub ws: Arc<dyn Transport>,
    pub vend: Arc<dyn Transport>,
    /// The vending server's durable log (for the offline replay score).
    pub vend_log: Option<PathBuf>,
    pub seed: u64,
    pub days: u32,
}

pub struct Opts {
    pub agent: AgentKind,
    /// Port for the mock LLM (0 = any free loopback port).
    pub llm_port: u16,
    /// Env lookup for the live path (process env in the CLI).
    pub env: EnvFn,
    pub max_shifts: u64,
    pub sha: String,
}

type MakeSession = Box<dyn Fn(Registry, &str) -> HermesSession>;

/// Build the session factory for `agent`; the returned MockLlm (if any)
/// must stay alive for the run.
fn sessions(
    o: &Opts,
    factory: fn() -> ScriptFactory,
) -> Result<(MakeSession, Option<MockLlm>, Value), String> {
    match o.agent {
        AgentKind::Scripted => Ok((
            Box::new(move |reg, sys| HermesSession::scripted(reg, sys, factory())),
            None,
            json!({"provider": "scripted (in-process hermes-rs Provider)"}),
        )),
        AgentKind::MockLlm => {
            let m = MockLlm::start(o.llm_port, Brain::new(factory()))
                .map_err(|e| format!("mock LLM bind: {e}"))?;
            let base = m.base_url();
            let info = json!({"provider": "mock LLM over the OpenAI wire (hermes-rs curl provider)", "base_url": base});
            Ok((
                Box::new(move |reg, sys| {
                    mock_llm_session(&base, reg, sys).expect("loopback base checked")
                }),
                Some(m),
                info,
            ))
        }
        AgentKind::Live => {
            let env = o.env.clone();
            let cfg = live_cfg(&|k| env(k))?;
            let info = json!({"provider": "openrouter (hermes-rs curl provider)", "model": cfg.model, "usd_cap": cfg.usd_cap});
            let env = o.env.clone();
            Ok((
                Box::new(move |reg, sys| live_session(&cfg, env.clone(), reg, sys)),
                None,
                info,
            ))
        }
    }
}

fn card_json(card: &Scorecard) -> Value {
    card.to_json()
}

fn base(bench: &str, o: &Opts, card: &Scorecard, agent_info: Value) -> Value {
    let cj = card_json(card);
    json!({
        "bench": bench,
        "via": "hermes-rs agent loop (Agent::run) + tool Registry + hermes-bench adapters",
        "agent": o.agent.name(),
        "agent_info": agent_info,
        "generated_at": galion_bench::now_zagreb(),
        "sha": o.sha,
        "money_moves": 0,
        "pct": cj["pct"],
        "axes": cj["axes"],
    })
}

pub fn run_banking(b: &Backends, o: &Opts) -> Result<Value, String> {
    let (mk, _llm, info) = sessions(o, banking::episode_factory)?;
    let eps = banking::episodes(&b.bank_url, &*mk)?;
    let ep_card = eps.card();
    let ep_json = json!({
        "pct": ep_card.pct(),
        "forbidden_total": eps.forbidden_total,
        "audit_chain_ok": eps.audit_chain_ok,
        "model_api_calls": eps.api_calls,
        "items": eps.episodes.iter().map(|e| json!({"id": e.id, "pass": e.pass, "detail": e.detail, "tool_calls": e.tool_calls})).collect::<Vec<_>>(),
    });
    if o.agent != AgentKind::Scripted {
        let mut v = base("B1 banking via Hermes (episodes)", o, &ep_card, info);
        v["episodes"] = ep_json;
        return Ok(v);
    }
    let f = banking::faithful(&b.bank_url);
    let card = banking::card(&f.hermes);
    let mut v = base("B1 banking via Hermes (sibling's 10 axes)", o, &card, info);
    v["identical_to_direct"] = json!(f.identical);
    v["diffs"] = json!(f.diffs);
    v["direct_pct"] = json!(bench_banking::axes::pct(&f.direct));
    v["checks"] = json!(f.hermes.iter().map(|a| a.checks.len()).sum::<usize>());
    v["failing"] = json!(f
        .hermes
        .iter()
        .flat_map(|a| a
            .failing()
            .into_iter()
            .map(move |c| format!("{}: {} :: {}", a.id, c.name, c.detail)))
        .collect::<Vec<_>>());
    v["episodes"] = ep_json;
    Ok(v)
}

/// B1-ADV: the adversarial banking set (see [`banking_adv`]). Same agent kinds;
/// the scripted and mock-LLM brains are deliberately gullible.
pub fn run_banking_adv(b: &Backends, o: &Opts) -> Result<Value, String> {
    let (mk, _llm, info) = sessions(o, banking_adv::factory)?;
    let out = banking_adv::run(&b.bank_url, &*mk)?;
    let card = out.card();
    let mut v = base(
        "B1-ADV adversarial banking via Hermes (memo injection + social engineering)",
        o,
        &card,
        info,
    );
    v["adversarial"] = out.to_json();
    v["failing"] = json!(card
        .failing()
        .iter()
        .map(|(n, s)| format!("{n}: {s}"))
        .collect::<Vec<_>>());
    Ok(v)
}

pub fn run_wallstreet(b: &Backends, o: &Opts) -> Result<Value, String> {
    let (mk, _llm, info) = sessions(o, wallstreet::episode_factory)?;
    let reg = wallstreet::registry(b.ws.clone());
    let (eps, api) = wallstreet::episodes(b.ws.as_ref(), &*mk, reg.clone())?;
    let ep_card = wallstreet::episodes_card(&eps);
    let ep_json = json!({
        "pct": ep_card.pct(),
        "model_api_calls": api,
        "items": eps.iter().map(|e| json!({"id": e.id, "pass": e.pass, "detail": e.detail})).collect::<Vec<_>>(),
    });
    if o.agent != AgentKind::Scripted {
        let mut v = base("B2 Wall Street via Hermes (episodes)", o, &ep_card, info);
        v["episodes"] = ep_json;
        return Ok(v);
    }
    let f = wallstreet::faithful(b.ws.as_ref(), reg);
    let card = f.card();
    let mut v = base(
        "B2 Wall Street via Hermes (sibling's 9 axes)",
        o,
        &card,
        info,
    );
    v["step_pnl_cents"] = json!(f.step_pnl_cents);
    v["reference_pnl_cents"] = json!(f.reference_pnl_cents);
    v["ticket_fingerprint"] = json!(f.ticket_fingerprint);
    v["reference_fingerprint"] = json!(f.reference_fingerprint);
    v["hermes_tool_calls"] = json!(f.recs.len());
    v["checks"] = json!(f
        .checks
        .iter()
        .map(|c| json!({"axis": c.axis, "name": c.name, "pass": c.pass, "detail": c.detail}))
        .collect::<Vec<_>>());
    v["episodes"] = ep_json;
    Ok(v)
}

pub fn run_vending(b: &Backends, o: &Opts) -> Result<Value, String> {
    let (mk, _llm, info) = sessions(o, vending::shift_factory)?;
    let reset = format!("/v1/reset?seed={}&days={}", b.seed, b.days);
    match b.vend.send("POST", &reset, Some("")) {
        Ok((200, _)) => {}
        other => return Err(format!("vending reset failed: {other:?}")),
    }
    let session = mk(vending::registry(b.vend.clone()), vending::SYSTEM);
    let r = vending::run(&session, b.vend.as_ref(), o.max_shifts)?;
    let replay = match &b.vend_log {
        Some(p) => Some(vending::replay_score(p)?),
        None => None,
    };
    let reference = vending::reference(b.seed, b.days);
    // Published score = offline replay of the durable log when available.
    let published = replay.clone().unwrap_or_else(|| r.score.clone());
    let card = vending::card(&published);
    let mut v = base(
        "B3 vending via Hermes (galion-vending score, offline replay)",
        o,
        &card,
        info,
    );
    v["seed"] = json!(b.seed);
    v["days"] = json!(b.days);
    v["shifts"] = json!(r.shifts);
    v["hermes_tool_calls"] = json!(r.tool_calls);
    v["model_api_calls"] = json!(r.api_calls);
    v["turn_errors"] = json!(r.turn_errors);
    v["percent"] = published["percent"].clone();
    v["total_bp"] = published["total_bp"].clone();
    v["live_score_total_bp"] = r.score["total_bp"].clone();
    v["replay_matches_live"] = json!(replay
        .as_ref()
        .map(|x| x["final_state_hash"] == r.score["final_state_hash"]
            && x["total_bp"] == r.score["total_bp"]));
    v["final_state_hash"] = published["final_state_hash"].clone();
    v["reference_final_state_hash"] = reference["final_state_hash"].clone();
    v["reference_total_bp"] = reference["total_bp"].clone();
    v["identical_to_reference"] = json!(
        published["final_state_hash"] == reference["final_state_hash"]
            && published["total_bp"] == reference["total_bp"]
    );
    v["lies"] = published["lies"].clone();
    v["end_cash_cents"] = published["end_cash_cents"].clone();
    v["start_cash_cents"] = published["start_cash_cents"].clone();
    Ok(v)
}
