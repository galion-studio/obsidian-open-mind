// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the bench sector: the real `hermes` binary coordinates
//! the real `hermes-bench` binary (release) through the built-in bench plan.
//!
//! Gated: runs only with `GALION_REAL=1`. Slot 162 ports: 20320-20329 (the
//! status API runs on 20320; slot 129 used 19990 before the gateway join).
//! S4 (slot 162) runs the three benchmarks end to end through a real
//! `hermes-gateway` (`--features bench` build next to hermes-bench,
//! HERMES_RS_BENCH=1, loopback mock LLM, loopback backends); RB9 checks its
//! evidence. hermes-bench itself binds its in-process mock
//! bank and mock LLM on kernel-assigned loopback ports, as it always does.
//! Nothing is mocked on the Hermes side: real registry, real ledger, real
//! governor, real processes. No live model: the plan has none, and an
//! operator intake of `--agent live` is refused before any process starts.
//!
//!   GALION_REAL=1 cargo test -p hermes-core --release --test real_bench -- --nocapture
//!
//! `HERMES_BENCH_BIN` points at a built hermes-bench (with a bench-feature
//! hermes-gateway next to it); otherwise the test builds both
//! (`cargo build --release -p hermes-bench -p hermes-gateway --features
//! hermes-gateway/bench -j 2`). Every process
//! this test starts is killed before it returns.

use hermes_core::http;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 20320;

struct Killer(Child);
impl Drop for Killer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn bench_bin(repo: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("HERMES_BENCH_BIN") {
        return PathBuf::from(p);
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo.join("target"));
    let bin = target.join("release").join("hermes-bench");
    {
        eprintln!("  building hermes-bench + hermes-gateway --features bench (release)");
        let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .current_dir(repo)
            .args([
                "build",
                "--release",
                "-p",
                "hermes-bench",
                "-p",
                "hermes-gateway",
                "--features",
                "hermes-gateway/bench",
                "-j",
                "2",
            ])
            .status()
            .unwrap();
        assert!(
            st.success(),
            "cargo build hermes-bench + hermes-gateway failed"
        );
    }
    assert!(bin.is_file(), "{} missing", bin.display());
    let gw = bin.with_file_name("hermes-gateway");
    assert!(gw.is_file(), "{} missing", gw.display());
    bin
}

struct Env {
    dir: PathBuf,
    config: PathBuf,
    plans: PathBuf,
    bin: PathBuf,
    sha: String,
}

fn hermes(env: &Env, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_hermes"))
        .arg("--config")
        .arg(&env.config)
        .arg("--plans")
        .arg(&env.plans)
        .arg("--state")
        .arg(env.dir.join("state"))
        .args(args)
        // Markers standing in for live-model switches: they must not reach
        // the hermes-bench children (never real values).
        .env("GALION_BENCH_LIVE_MODEL", "1")
        .env("HERMES_BENCH_TEST_MARKER", "must-not-reach-child")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    if !err.trim().is_empty() {
        eprintln!("  stderr: {}", err.trim());
    }
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).to_string(),
    )
}

fn json(env: &Env, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let (code, out) = hermes(env, &a);
    let v = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e}: {out}"));
    (code, v)
}

fn step(n: &str, what: &str) {
    eprintln!("\n== {n}: {what} ({})", zagreb_time::now_rfc3339());
}

#[test]
fn bench_sector_real_life() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("real_bench: skipped (set GALION_REAL=1)");
        return;
    }
    let t0 = Instant::now();
    let repo = repo_root();
    let plans = std::env::var("HERMES_PLANS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/plans"));
    assert!(plans.join("gaia.md").is_file(), "real plans missing");
    let dir = std::env::temp_dir().join(format!("hermes-real-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sha = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let env = Env {
        config: repo.join("config/hermes/sectors.toml"),
        plans,
        bin: bench_bin(&repo),
        sha,
        dir,
    };
    let out = env.dir.join("out");

    step("RB1", "the bench sector and its built-in plan");
    let (code, v) = json(&env, &["sectors"]);
    assert_eq!(code, 0);
    let sectors = v["sectors"].as_array().unwrap();
    assert_eq!(sectors.len(), 8);
    let b = sectors.iter().find(|s| s["id"] == "bench").unwrap();
    eprintln!("  {b}");
    assert_eq!(
        (b["sessions"].as_u64(), b["tasks"].as_u64()),
        (Some(4), Some(12))
    );
    assert_eq!(b["gated_tasks"], 0);
    let (code, v) = json(&env, &["bench", "plan"]);
    assert_eq!(code, 0);
    let tasks = v["tasks"].as_array().unwrap();
    for t in tasks {
        eprintln!("  {} {} deps {}", t["task"], t["spec"], t["deps"]);
        assert!(t["spec"].is_object(), "refused plan row: {t}");
        assert_ne!(t["spec"]["agent"], "live");
    }
    assert_eq!(tasks.len(), 12);
    assert_eq!(
        v["agents_allowed"],
        serde_json::json!(["scripted", "mock-llm", "gateway"])
    );
    let (_, d) = json(&env, &["dispatch", "--dry-run", "--first-sessions", "1"]);
    let bench_rows: Vec<String> = d["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["sector"] == "bench")
        .map(|r| {
            format!(
                "{} {}",
                r["task"].as_str().unwrap(),
                r["decision"].as_str().unwrap()
            )
        })
        .collect();
    eprintln!(
        "  fresh dry run, bench rows: {bench_rows:?}; slots used {} of {}",
        d["slots_used"], d["worker_slots"]
    );
    assert!(
        bench_rows.iter().any(|r| r.ends_with("dispatch")),
        "bench gets a slot in round one"
    );

    step(
        "RB2",
        "Hermes runs the matrix through real hermes-bench processes",
    );
    let t_run = Instant::now();
    let (code, v) = json(
        &env,
        &[
            "bench",
            "run",
            "--bin",
            env.bin.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            &env.sha,
            "--timeout-secs",
            "600",
        ],
    );
    let run = &v["run"];
    let full_run = run["run"].as_str().unwrap().to_string();
    for r in run["rows"].as_array().unwrap() {
        eprintln!(
            "  {:<12} {:<8} att {} exit {} pct {:>4} {:>6} ms  {}",
            r["task"].as_str().unwrap(),
            r["outcome"].as_str().unwrap(),
            r["attempts"],
            r["exit"],
            r["pct"],
            r["ms"],
            r["detail"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>()
        );
    }
    eprintln!(
        "  done {} failed {} blocked {} refused {} retries {} peak {}/{} wall {} ms (test clock {:?})",
        run["done"], run["failed"], run["blocked"], run["refused"], run["retries"],
        run["peak"], run["cap"], run["wall_ms"], t_run.elapsed()
    );
    eprintln!("  spoken: {}", run["spoken"].as_str().unwrap());
    assert_eq!(code, 0, "{v}");
    assert_eq!(run["done"], 12);
    assert_eq!(run["failed"], 0);
    assert!(run["peak"].as_u64().unwrap() <= 3);
    for r in run["rows"].as_array().unwrap() {
        if r["spec"]["kind"] == "run" {
            assert_eq!(r["pct"], 100, "{r}");
        }
    }
    for f in [
        "hermes-banking.scripted.score.json",
        "hermes-wallstreet.scripted.score.json",
        "hermes-vending.scripted.score.json",
        "hermes-banking.mock-llm.score.json",
        "hermes-wallstreet.mock-llm.score.json",
        "hermes-vending.mock-llm.score.json",
        "hermes-banking.gateway.score.json",
        "hermes-wallstreet.gateway.score.json",
        "hermes-vending.gateway.score.json",
    ] {
        assert!(out.join(f).is_file(), "{f}");
    }
    for a in ["scripted", "mock-llm", "gateway"] {
        let md = std::fs::read_to_string(out.join(format!("SCOREBOARD-hermes.{a}.md"))).unwrap();
        let hundreds = md.matches("100%").count() + md.matches("100 %").count();
        eprintln!(
            "  SCOREBOARD-hermes.{a}.md: {} bytes, {hundreds} x 100%",
            md.len()
        );
        assert!(hundreds >= 3, "{md}");
    }

    step(
        "RB3",
        "scores match the committed bench-hermes record (real data)",
    );
    for (bench, agent) in [
        ("banking", "scripted"),
        ("wallstreet", "scripted"),
        ("vending", "scripted"),
        ("banking", "mock-llm"),
        ("wallstreet", "mock-llm"),
        ("vending", "mock-llm"),
    ] {
        let name = format!("hermes-{bench}.{agent}.score.json");
        let now: Value =
            serde_json::from_str(&std::fs::read_to_string(out.join(&name)).unwrap()).unwrap();
        let rec: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join("docs/bench").join(&name)).unwrap(),
        )
        .unwrap();
        let axes = |v: &Value| -> Vec<(String, u64)> {
            v["axes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    (
                        a["name"].as_str().unwrap().to_string(),
                        a["score"].as_u64().unwrap_or(0),
                    )
                })
                .collect()
        };
        eprintln!(
            "  {name}: pct {} (record {}), axes equal {}{}",
            now["pct"],
            rec["pct"],
            axes(&now) == axes(&rec),
            if bench == "vending" {
                format!(
                    ", final_state_hash {} (record {})",
                    now["final_state_hash"], rec["final_state_hash"]
                )
            } else {
                String::new()
            }
        );
        assert_eq!(now["pct"], rec["pct"], "{name}");
        assert_eq!(axes(&now), axes(&rec), "{name}");
        if bench == "vending" {
            assert_eq!(now["final_state_hash"], rec["final_state_hash"], "{name}");
        }
    }

    // S4: the gateway runs play the same episodes as the mock-LLM record;
    // banking adds one axis (drafts awaiting_owner, read from the gateway).
    let axes_of = |v: &Value| -> Vec<(String, u64)> {
        v["axes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                (
                    a["name"].as_str().unwrap().to_string(),
                    a["score"].as_u64().unwrap_or(0),
                )
            })
            .collect()
    };
    for bench in ["banking", "wallstreet", "vending"] {
        let read = |p: PathBuf| -> Value {
            serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
        };
        let gw = read(out.join(format!("hermes-{bench}.gateway.score.json")));
        let rec = read(repo.join(format!("docs/bench/hermes-{bench}.mock-llm.score.json")));
        let mut want = axes_of(&rec);
        if bench == "banking" {
            want.push((
                "payments stay drafts awaiting_owner (gateway tools)".into(),
                100,
            ));
        }
        eprintln!(
            "  hermes-{bench}.gateway.score.json: pct {} (mock-llm record {}), axes = record{} {}{}",
            gw["pct"],
            rec["pct"],
            if bench == "banking" { " + awaiting_owner" } else { "" },
            axes_of(&gw) == want,
            if bench == "vending" {
                format!(
                    ", final_state_hash {} (record {})",
                    gw["final_state_hash"], rec["final_state_hash"]
                )
            } else {
                String::new()
            }
        );
        assert_eq!(gw["pct"], 100, "{bench} gateway");
        assert_eq!(axes_of(&gw), want, "{bench} gateway axes");
        if bench == "vending" {
            assert_eq!(gw["final_state_hash"], rec["final_state_hash"]);
        }
    }

    step("RB4", "the ledger and the plan show the runs");
    let (_, ver) = json(&env, &["ledger", "--verify"]);
    eprintln!("  ledger verify: {ver}");
    assert_eq!(ver["sound"], true);
    let (_, p) = json(&env, &["plan", "--sector", "bench"]);
    let states: Vec<&str> = p["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["state"].as_str().unwrap())
        .collect();
    eprintln!("  plan states: {states:?}");
    assert!(states.iter().all(|s| *s == "done"));
    let led = std::fs::read_to_string(env.dir.join("state/ledger.jsonl")).unwrap();
    assert!(!led.contains("--agent live"));
    for l in std::fs::read_dir(out.join("logs")).unwrap() {
        let p = l.unwrap().path();
        let t = std::fs::read_to_string(&p).unwrap_or_default();
        assert!(!t.contains("must-not-reach-child"), "{}", p.display());
    }

    step("RB5", "readiness: G16 passes on the recorded run");
    let (code, r) = json(&env, &["readiness"]);
    for c in r["checks"].as_array().unwrap() {
        eprintln!(
            "  {} {:7} {}",
            c["id"].as_str().unwrap(),
            c["status"].as_str().unwrap(),
            c["evidence"]
                .as_str()
                .unwrap()
                .chars()
                .take(230)
                .collect::<String>()
        );
    }
    eprintln!("  {} of {} pass; exit {code}", r["passed"], r["total"]);
    eprintln!("  spoken: {}", r["report"]["spoken"].as_str().unwrap());
    assert_eq!(r["total"], 16);
    let g = |id: &str| {
        r["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap()["status"]
            .as_str()
            .unwrap()
            .to_string()
    };
    for id in ["G1", "G3", "G4", "G5", "G16"] {
        assert_eq!(g(id), "pass", "{id}");
    }
    let ids: Vec<&str> = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.last(), Some(&"G16"));

    step("RB6", "status API on 127.0.0.1:20320");
    let child = Command::new(env!("CARGO_BIN_EXE_hermes"))
        .arg("--config")
        .arg(&env.config)
        .arg("--plans")
        .arg(&env.plans)
        .arg("--state")
        .arg(env.dir.join("state"))
        .args(["serve", "--port", &PORT.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Killer(child);
    let addr: SocketAddr = format!("127.0.0.1:{PORT}").parse().unwrap();
    let up = Instant::now();
    while http::get(&addr, "/health").is_err() {
        assert!(
            up.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let (s, body) = http::get(&addr, "/v1/bench").unwrap();
    let vb: Value = serde_json::from_str(&body).unwrap();
    eprintln!(
        "  GET /v1/bench -> {s}, {} bytes, last run {} done {}",
        body.len(),
        vb["last"]["run"],
        vb["last"]["done"]
    );
    assert_eq!(s, 200);
    assert_eq!(vb["last"]["done"], 12);
    let (s, body) = http::get(&addr, "/v1/tasks?sector=bench&state=done").unwrap();
    let vt: Value = serde_json::from_str(&body).unwrap();
    eprintln!(
        "  GET /v1/tasks?sector=bench&state=done -> {s}, count {}",
        vt["count"]
    );
    assert_eq!(vt["count"], 12);
    let (s, body) = http::get(&addr, "/v1/readiness").unwrap();
    let vr: Value = serde_json::from_str(&body).unwrap();
    let g16 = vr["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "G16")
        .unwrap();
    eprintln!("  GET /v1/readiness -> {s}, G16 {}", g16["status"]);
    assert_eq!(g16["status"], "pass");
    let (s, _) = http::raw(
        &addr,
        "POST /v1/bench HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
    )
    .unwrap();
    eprintln!("  POST /v1/bench -> {s} (read-only API)");
    assert_eq!(s, 405);
    drop(server);
    assert!(http::get(&addr, "/health").is_err(), "server killed");

    step(
        "RB7",
        "operator intake of a live run is refused, nothing starts",
    );
    let (code, v) = json(
        &env,
        &[
            "intake",
            "--sector",
            "bench",
            "--title",
            "`hermes-bench run --bench banking --agent live` (operator asks for the paid model)",
        ],
    );
    assert_eq!(code, 0);
    let key = v["task"]["key"].as_str().unwrap().to_string();
    let logs_before = std::fs::read_dir(out.join("logs")).unwrap().count();
    let (code, v) = json(
        &env,
        &[
            "bench",
            "run",
            "--bin",
            env.bin.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            &env.sha,
            "--only",
            &key,
        ],
    );
    let row = v["run"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task"] == key)
        .unwrap()
        .clone();
    eprintln!(
        "  {key}: exit {code}, outcome {}, detail {}",
        row["outcome"], row["detail"]
    );
    assert_eq!(code, 3);
    assert_eq!(row["outcome"], "refused");
    assert_eq!(row["attempts"], 0);
    assert_eq!(
        std::fs::read_dir(out.join("logs")).unwrap().count(),
        logs_before,
        "no process started"
    );
    let (code, s) = json(&env, &["bench", "status"]);
    assert_eq!(code, 0);
    eprintln!(
        "  bench status keeps the last full run: {} done {} (RB2 run {full_run})",
        s["last"]["run"], s["last"]["done"]
    );
    assert_eq!(
        s["last"]["run"],
        full_run.as_str(),
        "a refused intake run must not replace last.json"
    );
    assert_eq!(s["last"]["done"], 12);

    step(
        "RB8",
        "re-run without --rerun is a no-op; with --rerun it runs again",
    );
    let (code, v) = json(
        &env,
        &[
            "bench",
            "run",
            "--bin",
            env.bin.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--only",
            "S1.1",
        ],
    );
    eprintln!("  without --rerun: exit {code}, rows {}", v["run"]["rows"]);
    assert_eq!(v["run"]["rows"][0]["outcome"], "already-done");
    let (code, v) = json(
        &env,
        &[
            "bench",
            "run",
            "--bin",
            env.bin.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--sha",
            &env.sha,
            "--only",
            "S1.1",
            "--rerun",
        ],
    );
    eprintln!(
        "  with --rerun: exit {code}, outcome {} pct {} {} ms",
        v["run"]["rows"][0]["outcome"], v["run"]["rows"][0]["pct"], v["run"]["rows"][0]["ms"]
    );
    assert_eq!(code, 0);
    assert_eq!(v["run"]["rows"][0]["pct"], 100);
    assert_eq!(v["run"]["full"], false);
    let (_, s) = json(&env, &["bench", "status"]);
    let (_, r) = json(&env, &["readiness"]);
    let g16 = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "G16")
        .unwrap()
        .clone();
    let g16_ev = g16["evidence"].as_str().unwrap();
    eprintln!(
        "  after the partial re-run: last run {} (RB2 {full_run}), G16 {} ({})",
        s["last"]["run"],
        g16["status"],
        g16_ev.rsplit("; FAILS: ").next().unwrap_or("")
    );
    assert_eq!(s["last"]["run"], full_run.as_str());
    // G16 still fails only for the RB7 live row queued by intake: the scores
    // of the full run stand, nothing is "left undone".
    assert_eq!(g16["status"], "fail");
    assert!(g16_ev.contains("refused rows: bench:H1"), "{g16_ev}");
    assert!(
        !g16_ev.contains("left undone") && !g16_ev.contains("not done at 100%"),
        "{g16_ev}"
    );

    step(
        "RB9",
        "S4 went end to end through real hermes-gateway processes (loopback, mock LLM)",
    );
    for bench in ["banking", "wallstreet", "vending"] {
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(out.join(format!("hermes-{bench}.gateway.score.json")))
                .unwrap(),
        )
        .unwrap();
        let g = &v["gateway"];
        eprintln!(
            "  {bench}: gateway pid {} on {} | {} chat turns, {} LLM requests ({} with auth header), {} tools offered, ws_kill_reset offered {}, money-move tools {}, backend front requests {}, {} ms",
            g["gateway_pid"], g["gateway_bind"], g["chat_turns"], g["llm_requests"],
            g["llm_requests_with_auth_header"], g["tools_offered"], g["ws_kill_reset_offered"],
            g["money_move_tools_offered"], g["backend_front_requests"], g["wall_ms"]
        );
        eprintln!("    {}", g["bench_line"].as_str().unwrap_or(""));
        assert_eq!(v["agent"], "gateway");
        assert!(g["gateway_bind"]
            .as_str()
            .unwrap()
            .starts_with("127.0.0.1:"));
        let line = g["bench_line"].as_str().unwrap();
        assert!(line.contains("bench on: 36 tools"), "{line}");
        for set in [
            "banking@http://127.0.0.1:",
            "wallstreet@http://127.0.0.1:",
            "vending@http://127.0.0.1:",
        ] {
            assert!(line.contains(set), "{set} not loopback http: {line}");
        }
        assert_eq!(
            g["llm_requests_with_auth_header"], 0,
            "no key reached the gateway"
        );
        assert_eq!(g["tools_offered"], 38, "36 bench + clock + delegate_task");
        assert_eq!(g["ws_kill_reset_offered"], false);
        assert_eq!(g["money_move_tools_offered"], 0);
        assert!(g["llm_requests"].as_u64().unwrap() > g["chat_turns"].as_u64().unwrap());
        let pid = g["gateway_pid"].as_u64().unwrap();
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists()
                || std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .map(|c| !c.contains("hermes-gateway"))
                    .unwrap_or(true),
            "gateway pid {pid} still running"
        );
        match bench {
            "banking" => {
                assert_eq!(v["episodes"]["drafts_awaiting_owner"], true);
                assert_eq!(v["episodes"]["forbidden_total"], 0);
                assert_eq!(v["money_moves"], 0);
            }
            "wallstreet" => {
                assert!(g["backend_front_requests"]["wallstreet"].as_u64().unwrap() > 0);
            }
            _ => {
                assert_eq!(v["identical_to_reference"], true);
                assert_eq!(v["replay_matches_live"], true);
                assert_eq!(
                    g["backend_front_requests"]["vending"], v["hermes_tool_calls"],
                    "every vending tool call crossed the gateway -> loopback front"
                );
            }
        }
    }
    // fail closed: a hermes-bench with no gateway next to it refuses the run
    let lone = env.dir.join("lone");
    std::fs::create_dir_all(&lone).unwrap();
    let lone_bin = lone.join("hermes-bench");
    std::fs::copy(&env.bin, &lone_bin).unwrap();
    let o = Command::new(&lone_bin)
        .args(["run", "--bench", "banking", "--agent", "gateway"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    eprintln!(
        "  no sibling gateway: exit {:?}, {}",
        o.status.code(),
        err.trim()
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(err.contains("--agent gateway needs"), "{err}");
    let left: Vec<String> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("hermes-bench-gw-"))
        .collect();
    eprintln!("  leftover gateway homes in TMPDIR: {}", left.len());

    eprintln!("\nreal_bench: all steps passed in {:?}", t0.elapsed());
    let keep = env.dir.join("out");
    if let Ok(dst) = std::env::var("HERMES_REAL_BENCH_KEEP") {
        let _ = std::fs::create_dir_all(&dst);
        for e in std::fs::read_dir(&keep).unwrap().flatten() {
            if e.path().is_file() {
                let _ = std::fs::copy(e.path(), Path::new(&dst).join(e.file_name()));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&env.dir);
}
