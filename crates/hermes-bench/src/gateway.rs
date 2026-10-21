// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Benchmark tools inside `hermes-gateway`, behind `HERMES_RS_BENCH=1`.
//!
//! Decision H4 option (b) of `docs/bench/hermes-bench.md`: the same adapters
//! the standalone `hermes-bench` runner uses, registered in the gateway's
//! tool [`Registry`] so a chat client (or a model behind the gateway) can
//! reach B1 banking / B2 Wall Street / B3 vending. Off unless the env says
//! so; hermes-gateway also needs its `bench` cargo feature (off by default).
//!
//! Env (all optional except the switch):
//!
//! | Name | Meaning | Default |
//! |---|---|---|
//! | `HERMES_RS_BENCH` | `1`/`true`/`yes`/`on` turns the wiring on | off |
//! | `HERMES_RS_BENCH_SETS` | comma list of `banking,wallstreet,vending` | all three |
//! | `HERMES_RS_BENCH_BANK_URL` | loopback http base of `bench-banking mock` | in-process mock on 127.0.0.1:0 |
//! | `HERMES_RS_BENCH_WS_URL` | loopback http base of `galion-wallstreet serve` | in-process paper broker |
//! | `HERMES_RS_BENCH_VEND_URL` | loopback http base of `galion-vending serve` | in-process sim |
//! | `HERMES_RS_BENCH_SEED` / `_DAYS` | in-process vending seed / days | 42 / 30 |
//! | `HERMES_RS_BENCH_VEND_LOG` | in-process vending durable log | none |
//!
//! Safety: every backend is a local mock, paper book or simulation; URLs
//! must be loopback `http` (checked before any socket opens); `ws_kill_reset`
//! stays operator-only; registration fails closed if any registered name
//! looks money-moving ([`crate::adapters::is_money_move_name`]) or clashes
//! with an existing gateway tool; and a non-loopback gateway bind refuses.

use crate::adapters::{
    is_money_move_name, register_banking, register_vending, register_wallstreet,
    WALLSTREET_OPERATOR_ONLY,
};
use crate::transport::{HttpTransport, Transport, VendingInProcess, WallStreetInProcess};
use hermes_rs::tools::Registry;
use std::net::TcpListener;
use std::sync::Arc;

/// The switch.
pub const ENV_SWITCH: &str = "HERMES_RS_BENCH";

/// The three tool sets, in registration order.
pub const SETS: [&str; 3] = ["banking", "wallstreet", "vending"];

/// What got wired, for the gateway's start-up log line.
#[derive(Debug, Clone, PartialEq)]
pub struct Wiring {
    /// `(set, backend label)` in registration order.
    pub sets: Vec<(String, String)>,
    /// Bench tool names added to the registry.
    pub tools: Vec<String>,
}

impl Wiring {
    pub fn summary(&self) -> String {
        let sets: Vec<String> = self.sets.iter().map(|(s, b)| format!("{s}@{b}")).collect();
        format!(
            "bench on: {} tools ({}), local mock/paper/sim only, ws_kill_reset operator-only",
            self.tools.len(),
            sets.join(", ")
        )
    }
}

/// True when `HERMES_RS_BENCH` asks for the wiring.
pub fn enabled(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env(ENV_SWITCH).is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn nonempty(env: &dyn Fn(&str) -> Option<String>, k: &str) -> Option<String> {
    env(k)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Parse `HERMES_RS_BENCH_SETS` (default: all three).
pub fn sets(env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<&'static str>, String> {
    let Some(raw) = nonempty(env, "HERMES_RS_BENCH_SETS") else {
        return Ok(SETS.to_vec());
    };
    let mut out = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some(s) = SETS.iter().find(|s| **s == part) else {
            return Err(format!(
                "HERMES_RS_BENCH_SETS: unknown set {part:?} (allowed: banking, wallstreet, vending)"
            ));
        };
        if !out.contains(s) {
            out.push(*s);
        }
    }
    if out.is_empty() {
        return Err("HERMES_RS_BENCH_SETS is empty".into());
    }
    // registration order is fixed, whatever order the env lists
    out.sort_by_key(|s| SETS.iter().position(|x| x == s));
    Ok(out)
}

fn num<T: std::str::FromStr>(
    env: &dyn Fn(&str) -> Option<String>,
    k: &str,
    d: T,
) -> Result<T, String> {
    match nonempty(env, k) {
        None => Ok(d),
        Some(v) => v.parse().map_err(|_| format!("bad {k}")),
    }
}

/// Register the bench tool sets into `reg` if `HERMES_RS_BENCH` is on.
///
/// `Ok(None)`: switch off, `reg` untouched. `Err`: misconfiguration; the
/// caller should refuse to start (fail closed) rather than run half-wired,
/// and `reg` is untouched. `loopback_bind` is the gateway's own bind check.
pub fn register_from_env(
    reg: &mut Registry,
    env: &dyn Fn(&str) -> Option<String>,
    loopback_bind: bool,
) -> Result<Option<Wiring>, String> {
    if !enabled(env) {
        return Ok(None);
    }
    if !loopback_bind {
        return Err(
            "HERMES_RS_BENCH=1 refused: the gateway bind is not loopback (bench tools are for local runs only)"
                .into(),
        );
    }
    let want = sets(env)?;
    let mut staged = Registry::new();
    let mut wired = Vec::new();
    for set in want {
        let label = match set {
            "banking" => {
                let base = match nonempty(env, "HERMES_RS_BENCH_BANK_URL") {
                    Some(u) => u,
                    None => {
                        // in-process bench-banking mock on an ephemeral loopback port
                        let l = TcpListener::bind(("127.0.0.1", 0))
                            .map_err(|e| format!("bench banking mock bind: {e}"))?;
                        let port = l.local_addr().map_err(|e| e.to_string())?.port();
                        bench_banking::mock::Mock::new().spawn(l);
                        format!("http://127.0.0.1:{port}")
                    }
                };
                let tools = crate::banking::new_tools(&base)
                    .map_err(|e| format!("HERMES_RS_BENCH_BANK_URL: {e}"))?;
                register_banking(&mut staged, tools);
                base
            }
            "wallstreet" => {
                let t: Arc<dyn Transport> = match nonempty(env, "HERMES_RS_BENCH_WS_URL") {
                    Some(u) => Arc::new(
                        HttpTransport::new(&u)
                            .map_err(|e| format!("HERMES_RS_BENCH_WS_URL: {e}"))?,
                    ),
                    None => Arc::new(WallStreetInProcess::fixture()),
                };
                let label = t.label();
                register_wallstreet(&mut staged, t);
                label
            }
            _ => {
                let t: Arc<dyn Transport> = match nonempty(env, "HERMES_RS_BENCH_VEND_URL") {
                    Some(u) => Arc::new(
                        HttpTransport::new(&u)
                            .map_err(|e| format!("HERMES_RS_BENCH_VEND_URL: {e}"))?,
                    ),
                    None => {
                        let seed = num(env, "HERMES_RS_BENCH_SEED", 42u64)?;
                        let days = num(env, "HERMES_RS_BENCH_DAYS", 30u32)?;
                        let log = nonempty(env, "HERMES_RS_BENCH_VEND_LOG").map(Into::into);
                        Arc::new(
                            VendingInProcess::new(seed, days, log)
                                .map_err(|e| format!("bench vending sim: {e}"))?,
                        )
                    }
                };
                let label = t.label();
                register_vending(&mut staged, t);
                label
            }
        };
        wired.push((set.to_string(), label));
    }
    let names = staged.names();
    if let Some(bad) = names
        .iter()
        .find(|n| is_money_move_name(n) || n.as_str() == WALLSTREET_OPERATOR_ONLY)
    {
        return Err(format!("bench tool {bad:?} refused by the money guard"));
    }
    if let Some(clash) = names.iter().find(|n| reg.get(n).is_some()) {
        return Err(format!(
            "bench tool {clash:?} clashes with an existing gateway tool"
        ));
    }
    for n in &names {
        if let Some(t) = staged.get(n) {
            reg.add(t);
        }
    }
    Ok(Some(Wiring {
        sets: wired,
        tools: names,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{BANKING_TOOLS, VENDING_TOOLS};
    use crate::session::hermes_call;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn off_by_default_leaves_registry_alone() {
        let mut reg = Registry::new();
        reg.add(Arc::new(hermes_rs::tools::ClockTool));
        let none: &'static [(&str, &str)] = &[];
        for e in [
            none,
            &[("HERMES_RS_BENCH", "0")],
            &[("HERMES_RS_BENCH", "")],
            &[("HERMES_RS_BENCH", "no")],
        ] {
            let env = env_of(e);
            assert_eq!(register_from_env(&mut reg, &env, true).unwrap(), None);
            // off wins even on a public bind
            assert_eq!(register_from_env(&mut reg, &env, false).unwrap(), None);
        }
        assert_eq!(reg.names(), vec!["clock".to_string()]);
    }

    #[test]
    fn on_registers_36_model_tools_in_process() {
        let mut reg = Registry::new();
        reg.add(Arc::new(hermes_rs::tools::ClockTool));
        let env = env_of(&[("HERMES_RS_BENCH", "1")]);
        let w = register_from_env(&mut reg, &env, true).unwrap().unwrap();
        assert_eq!(w.tools.len(), 12 + 13 + 11);
        assert_eq!(reg.names().len(), 37);
        assert!(reg.get(WALLSTREET_OPERATOR_ONLY).is_none());
        assert!(reg.names().iter().all(|n| !is_money_move_name(n)));
        for n in BANKING_TOOLS.iter().chain(VENDING_TOOLS.iter()) {
            assert!(reg.get(n).is_some(), "{n}");
        }
        assert_eq!(w.sets[0].0, "banking");
        assert!(w.sets[0].1.starts_with("http://127.0.0.1:"));
        assert_eq!(w.sets[1].1, "in-process galion-wallstreet");
        assert_eq!(w.sets[2].1, "in-process galion-vending");
        assert!(w.summary().starts_with("bench on: 36 tools"));
        // the tools answer through the real hermes-rs agent loop
        for t in ["vending_status", "bank_balance", "ws_account"] {
            let r = hermes_call(reg.clone(), t, &serde_json::json!({})).unwrap();
            assert!(!r.is_error, "{t}: {:?}", r.content);
        }
    }

    #[test]
    fn sets_subset_and_order() {
        let env = env_of(&[("HERMES_RS_BENCH_SETS", "vending, banking,vending")]);
        assert_eq!(sets(&env).unwrap(), vec!["banking", "vending"]);
        let mut reg = Registry::new();
        let env = env_of(&[
            ("HERMES_RS_BENCH", "on"),
            ("HERMES_RS_BENCH_SETS", "vending"),
        ]);
        let w = register_from_env(&mut reg, &env, true).unwrap().unwrap();
        assert_eq!(w.tools.len(), 11);
        assert!(sets(&env_of(&[("HERMES_RS_BENCH_SETS", "revolut")])).is_err());
        assert!(sets(&env_of(&[("HERMES_RS_BENCH_SETS", " , ")])).is_err());
    }

    #[test]
    fn fails_closed_and_registers_nothing() {
        let mut reg = Registry::new();
        let on = env_of(&[("HERMES_RS_BENCH", "1")]);
        assert!(register_from_env(&mut reg, &on, false)
            .unwrap_err()
            .contains("not loopback"));
        // non-loopback backends are refused before any socket opens
        for (k, v) in [
            ("HERMES_RS_BENCH_BANK_URL", "https://api.revolut.com"),
            ("HERMES_RS_BENCH_WS_URL", "http://10.0.0.5:8080"),
            ("HERMES_RS_BENCH_VEND_URL", "http://example.com"),
        ] {
            let env = move |n: &str| match n {
                "HERMES_RS_BENCH" => Some("1".to_string()),
                x if x == k => Some(v.to_string()),
                _ => None,
            };
            let e = register_from_env(&mut reg, &env, true).unwrap_err();
            assert!(e.contains(k), "{e}");
        }
        let bad = env_of(&[("HERMES_RS_BENCH", "1"), ("HERMES_RS_BENCH_SEED", "x")]);
        assert!(register_from_env(&mut reg, &bad, true).is_err());
        // a clash with an existing gateway tool refuses too
        let mut taken = Registry::new();
        let vend = env_of(&[
            ("HERMES_RS_BENCH", "1"),
            ("HERMES_RS_BENCH_SETS", "vending"),
        ]);
        register_from_env(&mut taken, &vend, true).unwrap();
        let e = register_from_env(&mut taken, &vend, true).unwrap_err();
        assert!(e.contains("clashes"), "{e}");
        assert_eq!(taken.names().len(), 11);
        assert!(reg.is_empty());
    }
}
