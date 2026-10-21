// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! ```text
//! hermes-bench run --bench banking|wallstreet|vending|all
//!                  [--agent scripted|mock-llm|live]       (default scripted)
//!                  [--bank-url URL] [--ws-url URL] [--vend-url URL --vend-log PATH]
//!                  [--seed 42] [--days 30] [--max-shifts N] [--llm-port P]
//!                  [--out DIR] [--sha SHA]
//! hermes-bench tools                       registered tool names per benchmark (JSON)
//! hermes-bench scoreboard --dir DIR --sha SHA [--agent scripted] [--out FILE]
//! ```
//! Without a URL a backend runs in-process (banking: the bench-banking mock on
//! an ephemeral loopback port). URLs must be loopback. `--agent live` needs
//! GALION_BENCH_LIVE_MODEL=1 and OPENROUTER_API_KEY in the environment.
//! Exit 0 ok, 1 run error or score below 100 for scripted, 2 usage.

use bench_banking::mock::Mock;
use hermes_bench::adapters;
use hermes_bench::runner::{self, AgentKind, Backends, Opts};
use hermes_bench::transport::{HttpTransport, Transport, VendingInProcess, WallStreetInProcess};
use hermes_rs::provider::EnvFn;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("hermes-bench: {msg}\nusage: hermes-bench run --bench banking|wallstreet|vending|all [--agent scripted|mock-llm|live] [--bank-url URL] [--ws-url URL] [--vend-url URL --vend-log PATH] [--seed N] [--days N] [--max-shifts N] [--llm-port P] [--out DIR] [--sha SHA] | tools | scoreboard --dir DIR --sha SHA [--agent A] [--out FILE]");
    ExitCode::from(2)
}

const FILES: [(&str, &str); 3] = [("banking", "B1"), ("wallstreet", "B2"), ("vending", "B3")];

fn score_file(dir: &Path, bench: &str, agent: &str) -> PathBuf {
    dir.join(format!("hermes-{bench}.{agent}.score.json"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(&args),
        Some("tools") => {
            let mut b = hermes_rs::tools::Registry::new();
            adapters::register_wallstreet(&mut b, Arc::new(WallStreetInProcess::fixture()));
            let v = json!({
                "banking": adapters::BANKING_TOOLS,
                "wallstreet_model_facing": b.names(),
                "wallstreet_operator_only": [adapters::WALLSTREET_OPERATOR_ONLY],
                "vending": adapters::VENDING_TOOLS,
            });
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Some("scoreboard") => scoreboard(&args),
        _ => usage("unknown command"),
    }
}

fn run(args: &[String]) -> ExitCode {
    let bench = flag(args, "--bench").unwrap_or_else(|| "all".into());
    let Some(agent) = AgentKind::parse(&flag(args, "--agent").unwrap_or_else(|| "scripted".into()))
    else {
        return usage("bad --agent");
    };
    let num = |k: &str, d: u64| flag(args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let seed = num("--seed", 42);
    let days = num("--days", 30) as u32;
    let tmp_log =
        std::env::temp_dir().join(format!("hermes-bench-vend-{}.jsonl", std::process::id()));
    let mut _mock_keep = None;
    let bank_url = match flag(args, "--bank-url") {
        Some(u) => u,
        None => {
            let l = match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(l) => l,
                Err(e) => return usage(&format!("bind: {e}")),
            };
            let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
            let m = Mock::new();
            m.spawn(l);
            _mock_keep = Some(m);
            format!("http://127.0.0.1:{port}")
        }
    };
    let ws: Arc<dyn Transport> = match flag(args, "--ws-url") {
        Some(u) => match HttpTransport::new(&u) {
            Ok(t) => Arc::new(t),
            Err(e) => return usage(&e),
        },
        None => Arc::new(WallStreetInProcess::fixture()),
    };
    let (vend, vend_log): (Arc<dyn Transport>, Option<PathBuf>) = match flag(args, "--vend-url") {
        Some(u) => match HttpTransport::new(&u) {
            Ok(t) => (Arc::new(t), flag(args, "--vend-log").map(PathBuf::from)),
            Err(e) => return usage(&e),
        },
        None => match VendingInProcess::new(seed, days, Some(tmp_log.clone())) {
            Ok(t) => (Arc::new(t), Some(tmp_log.clone())),
            Err(e) => return usage(&format!("vending sim: {e}")),
        },
    };
    let env: EnvFn = Arc::new(|k: &str| std::env::var(k).ok());
    let o = Opts {
        agent,
        llm_port: num("--llm-port", 0) as u16,
        env,
        max_shifts: num("--max-shifts", u64::from(days) + 5),
        sha: flag(args, "--sha").unwrap_or_else(|| "unknown".into()),
    };
    let b = Backends {
        bank_url,
        ws,
        vend,
        vend_log,
        seed,
        days,
    };
    let which: Vec<&str> = match bench.as_str() {
        "all" => vec!["banking", "wallstreet", "vending"],
        "banking" | "wallstreet" | "vending" => vec![bench.as_str()],
        _ => return usage("bad --bench"),
    };
    let out_dir = flag(args, "--out").map(PathBuf::from);
    let mut all_100 = true;
    for w in which {
        let r = match w {
            "banking" => runner::run_banking(&b, &o),
            "wallstreet" => runner::run_wallstreet(&b, &o),
            _ => runner::run_vending(&b, &o),
        };
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                eprintln!("hermes-bench: {w}: {e}");
                return ExitCode::from(1);
            }
        };
        let pct = v["pct"].as_u64().unwrap_or(0);
        all_100 &= pct == 100;
        println!(
            "{w} [{}] via Hermes: {pct}%  {}",
            agent.name(),
            summary(w, &v)
        );
        if let Some(d) = &out_dir {
            if let Err(e) = std::fs::create_dir_all(d).and_then(|_| {
                std::fs::write(
                    score_file(d, w, agent.name()),
                    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n",
                )
            }) {
                eprintln!("hermes-bench: write: {e}");
                return ExitCode::from(1);
            }
        }
    }
    let _ = std::fs::remove_file(&tmp_log);
    if agent == AgentKind::Scripted && !all_100 {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn summary(w: &str, v: &Value) -> String {
    let ep = format!("episodes={}%", v["episodes"]["pct"]);
    match w {
        "banking" if v["identical_to_direct"].is_boolean() => format!(
            "identical_to_direct={} checks={} {ep} forbidden={}",
            v["identical_to_direct"], v["checks"], v["episodes"]["forbidden_total"]
        ),
        "banking" => format!("{ep} forbidden={}", v["episodes"]["forbidden_total"]),
        "wallstreet" if v["step_pnl_cents"].is_number() => format!(
            "pnl {} vs ref {} fingerprint {} (ref {}) {ep}",
            v["step_pnl_cents"], v["reference_pnl_cents"], v["ticket_fingerprint"], v["reference_fingerprint"]
        ),
        "wallstreet" => ep,
        _ => format!(
            "{} shifts, {} tool calls, total_bp {} (ref {}), hash {} identical_to_reference={} replay_matches_live={}",
            v["shifts"], v["hermes_tool_calls"], v["total_bp"], v["reference_total_bp"], v["final_state_hash"], v["identical_to_reference"], v["replay_matches_live"]
        ),
    }
}

fn scoreboard(args: &[String]) -> ExitCode {
    let Some(dir) = flag(args, "--dir").map(PathBuf::from) else {
        return usage("scoreboard needs --dir");
    };
    let sha = flag(args, "--sha").unwrap_or_else(|| "unknown".into());
    let agent = flag(args, "--agent").unwrap_or_else(|| "scripted".into());
    let mut entries: [Option<bench_scoreboard::Entry>; 3] = [None, None, None];
    for (i, (bench, _key)) in FILES.iter().enumerate() {
        let p = score_file(&dir, bench, &agent);
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        match bench_scoreboard::parse_score(&text) {
            Ok(card) => {
                let stamp = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["generated_at"].as_str().map(str::to_string))
                    .unwrap_or_else(galion_bench::now_zagreb);
                entries[i] = Some(bench_scoreboard::Entry {
                    card,
                    sha: sha.clone(),
                    json_path: format!("docs/bench/hermes-{bench}.{agent}.score.json"),
                    stamp,
                })
            }
            Err(e) => {
                eprintln!("{}: {e}", p.display());
                return ExitCode::from(1);
            }
        }
    }
    let view = bench_scoreboard::View {
        heading: format!("Benchmark scoreboard via the Hermes tool layer (agent: {agent})"),
        generator: format!("`hermes-bench scoreboard --agent {agent}` (branch `swarm/bench-hermes`); every score below was produced by tool calls executed by the hermes-rs agent loop through the hermes-bench adapters"),
        branches: ["swarm/bench-hermes", "swarm/bench-hermes", "swarm/bench-hermes"],
    };
    let md = bench_scoreboard::render_with(&entries, &galion_bench::now_zagreb(), &view);
    match flag(args, "--out") {
        Some(f) => {
            if let Err(e) = std::fs::write(&f, md) {
                eprintln!("{f}: {e}");
                return ExitCode::from(1);
            }
        }
        None => print!("{md}"),
    }
    ExitCode::SUCCESS
}
