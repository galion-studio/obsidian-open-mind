// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Model-backed agent paths with no spend: the mock LLM over the OpenAI wire
//! (hermes-rs curl provider), a lazy model that must NOT score 100, and the
//! live OpenRouter code path pointed at the loopback mock with a placeholder
//! key (proves the key is passed by name to the provider and never printed).

use bench_banking::mock::Mock;
use hermes_bench::live::{self, live_cfg, live_session, mock_llm_session};
use hermes_bench::mockllm::MockLlm;
use hermes_bench::planner::{Brain, PlanIo, Script};
use hermes_bench::session::HermesSession;
use hermes_bench::transport::{Transport, VendingInProcess, WallStreetInProcess};
use hermes_bench::{banking, vending, wallstreet};
use hermes_rs::provider::EnvFn;
use hermes_rs::tools::Registry;
use std::sync::Arc;

fn bank() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    Mock::new().spawn(l);
    format!("http://127.0.0.1:{port}")
}

fn lazy() -> Brain {
    Brain::new(Box::new(|_u: &str| -> Script {
        Box::new(|_io: &mut PlanIo| "Sure, all done!".to_string())
    }))
}

#[test]
fn mock_llm_reference_scores_100_and_lazy_model_does_not() {
    let base = bank();
    let m = MockLlm::start(0, Brain::new(banking::episode_factory())).unwrap();
    let url = m.base_url();
    let mk = move |reg: Registry, sys: &str| mock_llm_session(&url, reg, sys).unwrap();
    let out = banking::episodes(&base, &mk).unwrap();
    assert_eq!(out.card().pct(), 100, "{:?}", out.episodes);
    let seen = m.seen();
    assert!(seen.requests >= 10, "{seen:?}");
    assert_eq!(seen.with_auth, 0, "mock path must send no key");
    assert_eq!(seen.tool_names.len(), 12);

    let lazy_llm = MockLlm::start(0, lazy()).unwrap();
    let url = lazy_llm.base_url();
    let mk = move |reg: Registry, sys: &str| mock_llm_session(&url, reg, sys).unwrap();
    let out = banking::episodes(&base, &mk).unwrap();
    // only the gate-refused ask and the global guards pass
    assert!(
        out.card().pct() < 50,
        "lazy model scored {}",
        out.card().pct()
    );

    let t: Arc<dyn Transport> = Arc::new(VendingInProcess::new(42, 30, None).unwrap());
    let lazy_llm = MockLlm::start(0, lazy()).unwrap();
    let s = mock_llm_session(
        &lazy_llm.base_url(),
        vending::registry(t.clone()),
        vending::SYSTEM,
    )
    .unwrap();
    let r = vending::run(&s, t.as_ref(), 40).unwrap();
    assert!(!r.turn_errors.is_empty());
    assert!(vending::card(&r.score).pct() < 100, "{}", r.score);
}

#[test]
fn wallstreet_episodes_over_the_wire() {
    let t: Arc<dyn Transport> = Arc::new(WallStreetInProcess::fixture());
    let m = MockLlm::start(0, Brain::new(wallstreet::episode_factory())).unwrap();
    let url = m.base_url();
    let mk = move |reg: Registry, sys: &str| mock_llm_session(&url, reg, sys).unwrap();
    let (eps, _) = wallstreet::episodes(t.as_ref(), &mk, wallstreet::registry(t.clone())).unwrap();
    assert_eq!(wallstreet::episodes_card(&eps).pct(), 100, "{eps:?}");
    assert_eq!(
        m.seen().tool_names.len(),
        13,
        "ws_kill_reset must not be offered"
    );
    assert!(!m.seen().tool_names.iter().any(|n| n == "ws_kill_reset"));
}

#[test]
fn live_code_path_against_loopback_mock_with_placeholder_key() {
    let base = bank();
    let m = MockLlm::start(0, Brain::new(banking::episode_factory())).unwrap();
    let mock_url = m.base_url();
    let placeholder = "placeholder-not-a-real-key-0000";
    let env: EnvFn = {
        let u = mock_url.clone();
        Arc::new(move |k: &str| match k {
            live::LIVE_FLAG => Some("1".into()),
            live::KEY_ENV => Some(placeholder.into()),
            live::BASE_ENV => Some(u.clone()),
            _ => None,
        })
    };
    let cfg = live_cfg(&|k| env(k)).unwrap();
    assert_eq!(cfg.base_url, mock_url);
    let e2 = env.clone();
    let mk = move |reg: Registry, sys: &str| -> HermesSession {
        live_session(&cfg, e2.clone(), reg, sys)
    };
    let out = banking::episodes(&base, &mk).unwrap();
    assert_eq!(out.card().pct(), 100, "{:?}", out.episodes);
    let seen = m.seen();
    assert_eq!(
        seen.with_auth, seen.requests,
        "key passed to the provider by name"
    );
    assert_eq!(seen.models, vec![live::DEFAULT_MODEL.to_string()]);
    let dump = format!("{:?}", out.episodes);
    assert!(!dump.contains(placeholder), "key leaked into results");
}
