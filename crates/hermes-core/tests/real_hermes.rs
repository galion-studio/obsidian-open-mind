// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the `hermes` binary on the real plans.
//!
//! Gated: runs only with `GALION_REAL=1` (otherwise it returns at once).
//! Nothing under test is mocked: the real registry (config/hermes/sectors.toml),
//! the real plans (/workspace/swarm/plans or HERMES_PLANS_DIR), the real
//! hades-gate verifier with a throwaway admin key made here, the real JSONL
//! ledger, the real governor and the real loopback HTTP server on 19380 (or
//! HERMES_REAL_PORT). The
//! only stand-in is the model provider (offline mock), because no paid model
//! call is approved and no credit may be spent.
//!
//!   GALION_REAL=1 cargo test -p hermes-core --release --test real_hermes -- --nocapture
//!
//! Every process this test starts is killed before it returns.

use hades_gate::keys;
use hades_gate::token::{plan_hash, Claims};
use hermes_core::http;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Default ports; a swarm slot overrides them with HERMES_REAL_PORT (status API)
/// and HERMES_REAL_PORT_REFUSED (the 0.0.0.0 bind that must be refused) so
/// the test stays inside the slot's assigned port range.
const DEFAULT_PORT: u16 = 19380;
const DEFAULT_PORT_REFUSED: u16 = 19381;

fn port_from_env(var: &str, default: u16) -> u16 {
    match std::env::var(var) {
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("{var} must be a port number, got {v:?}")),
        Err(_) => default,
    }
}

struct Env {
    dir: PathBuf,
    config: PathBuf,
    plans: PathBuf,
    repo: PathBuf,
}

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

fn setup() -> Option<Env> {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("real_hermes: skipped (set GALION_REAL=1)");
        return None;
    }
    let repo = repo_root();
    let plans = std::env::var("HERMES_PLANS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/plans"));
    assert!(
        plans.join("gaia.md").is_file(),
        "real plans missing at {}",
        plans.display()
    );
    let dir = std::env::temp_dir().join(format!("hermes-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Throwaway HADES admin key for this run only (never the real custody key).
    keys::generate(&dir.join("admin.seed"), &dir.join("admin.pub")).unwrap();
    std::fs::write(
        dir.join("hades.toml"),
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 600\n",
            dir.join("admin.pub").display(),
            dir.join("hades-ledger.jsonl").display(),
            dir.join("DISABLED").display()
        ),
    )
    .unwrap();
    // The real registry, with only [hades] config pointed at the test key.
    let real = std::fs::read_to_string(repo.join("config/hermes/sectors.toml")).unwrap();
    assert!(
        real.contains("config = \"\""),
        "repo registry must ship HADES closed"
    );
    let cfg = real.replacen(
        "config = \"\"",
        &format!("config = \"{}\"", dir.join("hades.toml").display()),
        1,
    );
    let config = dir.join("sectors.toml");
    std::fs::write(&config, cfg).unwrap();
    Some(Env {
        dir,
        config,
        plans,
        repo,
    })
}

/// Run `hermes` with the given state dir; returns (exit code, stdout).
fn hermes(env: &Env, state: &str, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_hermes"))
        .arg("--config")
        .arg(&env.config)
        .arg("--plans")
        .arg(&env.plans)
        .arg("--state")
        .arg(env.dir.join(state))
        .args(args)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    if !err.trim().is_empty() {
        eprintln!("  stderr: {}", err.trim());
    }
    (o.status.code().unwrap_or(-1), out)
}

fn json(env: &Env, state: &str, args: &[&str]) -> (i32, Value) {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let (code, out) = hermes(env, state, &a);
    let v = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e}: {out}"));
    (code, v)
}

fn step(n: &str, what: &str) {
    eprintln!("\n== {n}: {what}");
}

fn mint(env: &Env, action: &str, target: &str, plan: &[u8]) -> PathBuf {
    let seed = keys::load_seed(&env.dir.join("admin.seed")).unwrap();
    let now = hades_gate::approval::unix_now();
    let tok = Claims {
        action: action.into(),
        target: target.into(),
        plan_hash: plan_hash(plan),
        nonce: keys::os_random::<16>().unwrap(),
        issued_at: now,
        expires_at: now + 300,
    }
    .sign(&seed);
    let p = env.dir.join(format!(
        "{action}-{}.token",
        target.replace(['.', ':'], "_")
    ));
    std::fs::write(&p, tok).unwrap();
    p
}

#[test]
fn hermes_real_life() {
    let Some(env) = setup() else { return };
    let t0 = Instant::now();

    step("R1", "real plans load into the registry");
    let (code, v) = json(&env, "main", &["sectors"]);
    assert_eq!(code, 0);
    let sectors = v["sectors"].as_array().unwrap();
    let sum = |k: &str| sectors.iter().map(|s| s[k].as_u64().unwrap()).sum::<u64>();
    eprintln!(
        "  {} sectors, {} sessions, {} tasks, {} target agents, {} useful, gated {}; Gaia systems covered {}",
        sectors.len(),
        sum("sessions"),
        sum("tasks"),
        sum("target_agents"),
        sum("useful_agents"),
        sum("gated_tasks"),
        v["gaia_systems_covered"]
    );
    assert_eq!(sectors.len(), 7);
    for s in sectors {
        for k in ["sessions", "tasks", "target", "useful"] {
            let parsed = match k {
                "target" => &s["target_agents"],
                "useful" => &s["useful_agents"],
                _ => &s[k],
            };
            assert_eq!(parsed, &s["planner"][k], "{} {k} vs planner", s["id"]);
        }
    }
    assert_eq!((sum("sessions"), sum("tasks")), (47, 200));
    assert_eq!((sum("target_agents"), sum("useful_agents")), (645, 407));

    step(
        "R2",
        "dry-run dispatch of the first session of every sector",
    );
    let (code, v) = json(
        &env,
        "main",
        &["dispatch", "--dry-run", "--first-sessions", "1"],
    );
    assert_eq!(code, 0);
    let rows = v["rows"].as_array().unwrap();
    let count = |d: &str| rows.iter().filter(|r| r["decision"] == d).count();
    eprintln!(
        "  cap {} reserved {} worker slots {} used {}; dispatch {} wait-cap {} wait-deps {} held {} refused {}",
        v["cap"], v["reserved"], v["worker_slots"], v["slots_used"],
        count("dispatch"), count("wait-cap"), count("wait-deps"), count("held"), count("refused")
    );
    for r in rows.iter().filter(|r| r["decision"] == "dispatch") {
        eprintln!(
            "    {} -> {} x{} [{}]",
            r["task"].as_str().unwrap(),
            r["agent"].as_str().unwrap_or(""),
            r["slots"],
            r["class"].as_str().unwrap()
        );
    }
    assert_eq!(v["cap"], 16);
    assert!(v["slots_used"].as_u64().unwrap() <= v["worker_slots"].as_u64().unwrap());
    let dispatched_sectors: std::collections::BTreeSet<&str> = rows
        .iter()
        .filter(|r| r["decision"] == "dispatch")
        .map(|r| r["sector"].as_str().unwrap())
        .collect();
    assert_eq!(
        dispatched_sectors.len(),
        7,
        "round-robin: every sector gets work"
    );
    let led = std::fs::read_to_string(env.dir.join("main/ledger.jsonl")).unwrap_or_default();
    assert!(led.is_empty(), "a dry run writes nothing to the ledger");

    step(
        "R3",
        "default cap 16 under a platform limit of 18: no request-limit stops",
    );
    let (code, v) = json(
        &env,
        "cap16",
        &[
            "run",
            "--simulate",
            "--first-sessions",
            "1",
            "--cap",
            "16",
            "--mock-limit",
            "18",
            "--work-ms",
            "40",
        ],
    );
    assert_eq!(code, 0);
    let s = &v["summary"];
    eprintln!("  {}", s);
    eprintln!("  spoken: {}", v["spoken"].as_str().unwrap());
    assert_eq!(s["request_limit_stops"], 0);
    assert_eq!(s["failed"], 0);
    assert!(s["peak_slots"].as_u64().unwrap() <= 15);

    step(
        "R4",
        "cap 24 over a platform limit of 18: stops, retries, all complete",
    );
    let (code, v) = json(
        &env,
        "cap24",
        &[
            "run",
            "--simulate",
            "--first-sessions",
            "1",
            "--cap",
            "24",
            "--mock-limit",
            "18",
            "--work-ms",
            "40",
        ],
    );
    assert_eq!(code, 0);
    let s = &v["summary"];
    eprintln!("  {}", s);
    eprintln!("  spoken: {}", v["spoken"].as_str().unwrap());
    let stops = s["request_limit_stops"].as_u64().unwrap();
    assert!(stops > 0, "a cap above the limit must hit the limit");
    assert_eq!(s["retries"].as_u64().unwrap(), s["stops"].as_u64().unwrap());
    assert_eq!(s["failed"], 0);
    let in_scope = s["in_scope"].as_u64().unwrap();
    let parked = s["held"].as_u64().unwrap()
        + s["refused"].as_u64().unwrap()
        + s["blocked"].as_u64().unwrap();
    assert_eq!(
        s["completed"].as_u64().unwrap() + parked,
        in_scope,
        "every task ends done or parked for Slavik"
    );
    let (_, r) = json(&env, "cap24", &["ledger", "--retries"]);
    let retried: Vec<&Value> = r["retries"].as_array().unwrap().iter().collect();
    eprintln!(
        "  retry ledger: {} tasks retried; first rows:",
        retried.len()
    );
    for row in retried.iter().take(5) {
        eprintln!("    {row}");
    }
    assert!(!retried.is_empty());
    let (_, ver) = json(&env, "cap24", &["ledger", "--verify"]);
    eprintln!("  ledger verify: {ver}");
    assert_eq!(ver["sound"], true);

    step(
        "R4b",
        "harsh limit (8) with stalls every 7th call: retries still drain the queue",
    );
    let (code, v) = json(
        &env,
        "harsh",
        &[
            "run",
            "--simulate",
            "--first-sessions",
            "1",
            "--cap",
            "24",
            "--mock-limit",
            "8",
            "--work-ms",
            "40",
            "--stall-every",
            "7",
        ],
    );
    assert_eq!(code, 0);
    let s = &v["summary"];
    eprintln!("  {}", s);
    eprintln!("  spoken: {}", v["spoken"].as_str().unwrap());
    assert!(s["request_limit_stops"].as_u64().unwrap() > 10);
    assert!(s["stall_stops"].as_u64().unwrap() > 0);
    assert_eq!(s["failed"], 0);
    assert_eq!(s["completed"], 25);

    step("R5", "HADES refuses an unapproved merge (operator intake)");
    let (code, v) = json(
        &env,
        "main",
        &[
            "intake",
            "--sector",
            "integration-ci",
            "--title",
            "Merge swarm/integration-1 (8fd26f6) into main",
            "--class",
            "code",
        ],
    );
    assert_eq!(code, 0);
    let key = v["task"]["key"].as_str().unwrap().to_string();
    eprintln!(
        "  intake {} class {} (operator said code)",
        key, v["task"]["class"]
    );
    assert_eq!(v["task"]["class"], "merge");
    let plan_path = env.dir.join("merge.plan");
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            &key,
            "--plan-out",
            plan_path.to_str().unwrap(),
        ],
    );
    eprintln!("  no token -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 3);
    assert_eq!(v["decision"]["code"], "action");
    let target = format!("hermes.integration-ci.{}", key.rsplit(':').next().unwrap());
    let tok = mint(&env, "merge", &target, &std::fs::read(&plan_path).unwrap());
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            &key,
            "--token-file",
            tok.to_str().unwrap(),
        ],
    );
    eprintln!("  signed merge token -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 3, "HADES has no merge kind: merges stay with Slavik");

    step(
        "R6",
        "HADES deploy: held, approved once with a signed token, replay refused",
    );
    let dkey = "vps-ops:S4.1";
    let dplan = env.dir.join("deploy.plan");
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            dkey,
            "--plan-out",
            dplan.to_str().unwrap(),
        ],
    );
    eprintln!("  no token -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 0);
    assert_eq!(v["decision"]["decision"], "held", "{v}");
    let wrong = mint(
        &env,
        "switch",
        "hermes.vps-ops.S4.2",
        &std::fs::read(&dplan).unwrap(),
    );
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            dkey,
            "--token-file",
            wrong.to_str().unwrap(),
        ],
    );
    eprintln!("  token for S4.2 -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 3);
    let tok = mint(
        &env,
        "switch",
        "hermes.vps-ops.S4.1",
        &std::fs::read(&dplan).unwrap(),
    );
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            dkey,
            "--token-file",
            tok.to_str().unwrap(),
        ],
    );
    eprintln!("  signed switch token -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 0);
    assert_eq!(v["decision"]["decision"], "approved", "{v}");
    let (code, v) = json(
        &env,
        "main",
        &[
            "gate",
            "--task",
            dkey,
            "--token-file",
            tok.to_str().unwrap(),
        ],
    );
    eprintln!("  same token again -> exit {code}: {}", v["decision"]);
    assert_eq!(code, 3);
    assert_eq!(v["decision"]["code"], "replayed");

    let port = port_from_env("HERMES_REAL_PORT", DEFAULT_PORT);
    step("R7", &format!("status API on 127.0.0.1:{port}"));
    let (code, _) = hermes(
        &env,
        "main",
        &[
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            &port_from_env("HERMES_REAL_PORT_REFUSED", DEFAULT_PORT_REFUSED).to_string(),
        ],
    );
    eprintln!("  serve --host 0.0.0.0 -> exit {code} (refused, loopback only)");
    assert_ne!(code, 0);
    let child = Command::new(env!("CARGO_BIN_EXE_hermes"))
        .arg("--config")
        .arg(&env.config)
        .arg("--plans")
        .arg(&env.plans)
        .arg("--state")
        .arg(env.dir.join("cap24"))
        .args(["serve", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Killer(child);
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let up = Instant::now();
    while http::get(&addr, "/health").is_err() {
        assert!(
            up.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    for path in [
        "/health",
        "/v1/sectors",
        "/v1/agents",
        "/v1/tasks?state=done",
        "/v1/dispatch",
        "/v1/ledger?tail=3",
        "/v1/retries",
        "/v1/governor",
        "/v1/readiness",
        "/v1/report",
    ] {
        let (status, body) = http::get(&addr, path).unwrap();
        let ok = serde_json::from_str::<Value>(&body).is_ok();
        eprintln!("  GET {path} -> {status}, {} bytes, json {ok}", body.len());
        assert_eq!(status, 200, "{path}: {body}");
        assert!(ok);
    }
    let (status, _) = http::get(&addr, "/nope").unwrap();
    eprintln!("  GET /nope -> {status}");
    assert_eq!(status, 404);
    let (status, _) = http::raw(
        &addr,
        "POST /v1/dispatch HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
    )
    .unwrap();
    eprintln!("  POST /v1/dispatch -> {status} (read-only API)");
    assert_eq!(status, 405);
    let (_, rep) = http::get(&addr, "/v1/report").unwrap();
    let rep: Value = serde_json::from_str(&rep).unwrap();
    eprintln!(
        "  /v1/report spoken: {}",
        rep["spoken"].as_str().unwrap_or("")
    );
    drop(server);
    assert!(http::get(&addr, "/health").is_err(), "server killed");

    step("R8", "readiness report");
    let (code, v) = json(
        &env,
        "main",
        &["readiness", "--repo", env.repo.to_str().unwrap()],
    );
    for c in v["checks"].as_array().unwrap() {
        eprintln!(
            "  {} {:7} {} :: {}",
            c["id"].as_str().unwrap(),
            c["status"].as_str().unwrap(),
            c["name"].as_str().unwrap(),
            c["evidence"].as_str().unwrap()
        );
    }
    eprintln!(
        "  {} of {} pass; ready {}; exit {code}",
        v["passed"], v["total"], v["ready"]
    );
    eprintln!("  spoken: {}", v["report"]["spoken"].as_str().unwrap());
    assert_eq!(code, if v["ready"] == true { 0 } else { 3 });
    let status_of = |id: &str| {
        v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .map(|c| c["status"].as_str().unwrap().to_string())
            .unwrap()
    };
    assert_eq!(
        status_of("G9"),
        "pass",
        "HADES configured with the test key"
    );
    assert_eq!(
        status_of("G10"),
        "pass",
        "unapproved merge/deploy not dispatchable"
    );
    assert_eq!(
        status_of("G11"),
        "fail",
        "provider is the offline mock until credit exists"
    );

    eprintln!("\nreal_hermes: all steps passed in {:?}", t0.elapsed());
    let _ = std::fs::remove_dir_all(&env.dir);
}
