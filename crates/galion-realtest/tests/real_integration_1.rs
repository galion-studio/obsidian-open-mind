// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cross-crate integration real-life run (Rust port of the old
//! `scripts/real/integration-1.sh`). Runs only with `GALION_REAL=1`; box
//! only, nothing touches the VPS.
//!
//! * `stack_plain_and_with_nerve`: `galion-realtest stack` twice from release
//!   binaries, plain and `--with-nerve` (nerve-ingest against
//!   `openmind-nerve`). Ports `GALION_REAL_STACK_BASE`..+51 (default 19500).
//! * `gaia_heal_tick_drives_nerve`: `gaia heal-tick` driving a real
//!   `openmind-nerve serve --log` (POST /event, GET /events) and a throwaway
//!   `gaia-dummy`, with the hades-gate signed-token flow done through the
//!   real CLIs (`hades-gate keygen`, `gaia heal-plan`, `hades-gate mint`).
//!   The old script's `--trust-file/--approval-file` flags no longer exist on
//!   integration-2 (replaced by gaia-hades-tokens), so this port uses tokens.
//!   Ports base+1 (nerve), base+2 (dummy), allow range base..base+9
//!   (default base 19220, `GALION_REAL_PORT_BASE` moves it).
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-realtest --test real_integration_1 -- --test-threads=1 --nocapture
//! ```

use galion_realtest::http;
use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn checks_line(report: &Path) -> String {
    std::fs::read_to_string(report)
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Checks:"))
        .unwrap_or("Checks: (missing)")
        .to_string()
}

#[test]
fn stack_plain_and_with_nerve() {
    if !realrun::enabled("real_integration_1::stack") {
        return;
    }
    let base: u16 = std::env::var("GALION_REAL_STACK_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(19500);
    let ports: Vec<u16> = (base..=base + 51).collect();
    realrun::require_free(&ports);
    let mut args = vec!["-p", "galion-realtest", "-p", "openmind-nerve"];
    for b in galion_realtest::stack::REQUIRED {
        args.push("-p");
        args.push(b);
    }
    let bins = realrun::release_bins(&args);
    let work = Scratch::new("integration-1-stack");
    let mut c = Checks::new();
    for (label, nerve) in [("stack", false), ("stack-nerve", true)] {
        realrun::say(format!("galion-realtest {label}"));
        let out = work.path(label);
        let mut cmd = Command::new(bins.join("galion-realtest"));
        cmd.arg("stack")
            .arg("--bin-dir")
            .arg(&bins)
            .args(["--base-port", &base.to_string(), "--out"])
            .arg(&out);
        if nerve {
            cmd.arg("--with-nerve");
        }
        let st = cmd.status().expect("run galion-realtest stack");
        let line = checks_line(&out.join("report.md"));
        realrun::say(format!("{label}: {line} (exit {:?})", st.code()));
        c.that(&format!("{label} exit 0"), st.success());
        c.that(&format!("{label} 0 FAIL"), line.contains(" 0 FAIL"));
        if nerve {
            c.that("with-nerve: nothing skipped", line.contains(" 0 skipped"));
        }
        let open = realrun::ports_closed(&ports, Duration::from_secs(5));
        c.eq(
            &format!("{label}: every port free after teardown"),
            open,
            vec![],
        );
    }
    c.finish("real_integration_1::stack");
}

#[test]
fn gaia_heal_tick_drives_nerve() {
    if !realrun::enabled("real_integration_1::heal") {
        return;
    }
    let base = realrun::port_base(19220);
    let (nerve_port, dummy_port) = (base + 1, base + 2);
    let allow = format!("{base}-{}", base + 9);
    realrun::require_free(&[nerve_port, dummy_port]);
    let bins = realrun::release_bins(&["-p", "gaia", "-p", "openmind-nerve", "-p", "hades-gate"]);
    let (gaia, dummy, nerve_bin, gate) = (
        bins.join("gaia"),
        bins.join("gaia-dummy"),
        bins.join("openmind-nerve"),
        bins.join("hades-gate"),
    );
    let work = Scratch::new("integration-1-heal");
    for d in ["state", "nerve-data", "gate"] {
        std::fs::create_dir_all(work.path(d)).unwrap();
    }
    let mut c = Checks::new();
    let naddr = format!("127.0.0.1:{nerve_port}");
    let daddr = format!("127.0.0.1:{dummy_port}");

    // Admin key pair and gate config, made with the real CLI.
    let (seed, public) = (work.path("admin.seed"), work.path("gate/admin.pub"));
    let out = realrun::run(
        Command::new(&gate)
            .arg("keygen")
            .arg("--seed")
            .arg(&seed)
            .arg("--pub")
            .arg(&public),
    );
    assert!(
        out.status.success(),
        "keygen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let config = work.path("gate/hades.toml");
    std::fs::write(
        &config,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            public.display(),
            work.path("gate/ledger.jsonl").display(),
            work.path("gate/DISABLED").display()
        ),
    )
    .unwrap();

    let mut ncmd = Command::new(&nerve_bin);
    ncmd.args(["serve", "--listen", &naddr, "--log"])
        .arg(work.path("nerve.jsonl"))
        .env("NERVE_DATA_DIR", work.path("nerve-data"))
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_TELEGRAM_ENV", work.path("absent.env"))
        .env("NERVE_DRY_RUN", "1");
    let mut nerve = Proc::spawn("openmind-nerve", ncmd, &work.path("nerve.stderr"));
    let mut dcmd = Command::new(&dummy);
    dcmd.args(["--port", &dummy_port.to_string()]);
    let mut first = Proc::spawn("gaia-dummy", dcmd, &work.path("state/gaia-dummy.log"));
    assert!(realrun::wait_http(
        &naddr,
        "/health",
        Duration::from_secs(5)
    ));
    assert!(realrun::wait_http(
        &daddr,
        "/health",
        Duration::from_secs(5)
    ));
    let h = http::get(&naddr, "/health")
        .and_then(|r| r.json())
        .unwrap_or(Value::Null);
    realrun::say(format!("nerve /health {h}"));
    c.eq("nerve health ok", h["status"].as_str(), Some("ok"));

    let target_args = |cmd: &mut Command| {
        cmd.args(["--service", "gaia-dummy", "--port", &dummy_port.to_string()])
            .args(["--allow-ports", &allow]);
    };
    let tick = |label: &str, token: Option<&Path>| -> (i32, Value, String) {
        let mut cmd = Command::new(&gaia);
        cmd.args(["heal-tick", "--compact"]);
        target_args(&mut cmd);
        cmd.args(["--nerve", &naddr])
            .arg("--state")
            .arg(work.path("state"))
            .arg("--hades-config")
            .arg(&config);
        if let Some(t) = token {
            cmd.arg("--token-file").arg(t);
        }
        cmd.arg("--")
            .arg(&dummy)
            .args(["--port", &dummy_port.to_string()]);
        let out = realrun::run(&mut cmd);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let code = out.status.code().unwrap_or(-1);
        realrun::say(format!(
            "{label:<14} exit={code} outcome={}",
            v["result"]["outcome"].as_str().unwrap_or("?")
        ));
        (code, v, text)
    };

    let (code, v, t1) = tick("1-healthy", None);
    c.eq(
        "healthy tick",
        (code, v["result"]["outcome"].as_str()),
        (0, Some("healthy")),
    );
    first.kill();
    let (code, v, t2) = tick("2-no-token", None);
    c.eq(
        "no token refused",
        (code, v["result"]["outcome"].as_str()),
        (4, Some("refused")),
    );

    // Admin side: the exact plan, then a scoped single-use token.
    let plan = work.path("plan.bin");
    let mut pcmd = Command::new(&gaia);
    pcmd.arg("heal-plan");
    target_args(&mut pcmd);
    pcmd.arg("--out")
        .arg(&plan)
        .arg("--")
        .arg(&dummy)
        .args(["--port", &dummy_port.to_string()]);
    let out = realrun::run(&mut pcmd);
    assert!(
        out.status.success(),
        "heal-plan: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tok = work.path("valid.tok");
    let out = realrun::run(
        Command::new(&gate)
            .arg("mint")
            .arg("--seed")
            .arg(&seed)
            .args([
                "--action",
                "heal-apply",
                "--target",
                "gaia-dummy",
                "--plan-file",
            ])
            .arg(&plan)
            .arg("--out")
            .arg(&tok),
    );
    assert!(
        out.status.success(),
        "mint: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (code, v, t3) = tick("3-approved", Some(&tok));
    c.eq(
        "signed token restarts",
        (code, v["result"]["outcome"].as_str()),
        (0, Some("restarted")),
    );
    let pid = http::get(&daddr, "/health")
        .and_then(|r| r.json())
        .ok()
        .and_then(|v| v["pid"].as_u64());
    realrun::say(format!("dummy after restart: pid {pid:?}"));
    c.that("dummy back up with a pid", pid.is_some());

    let ev = http::get(&naddr, "/events?n=100")
        .map(|r| r.text())
        .unwrap_or_default();
    let evv: Value = serde_json::from_str(&ev).unwrap_or(Value::Null);
    let mut kinds: Vec<String> = evv
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["kind"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let n_events = kinds.len();
    kinds.sort();
    kinds.dedup();
    realrun::say(format!(
        "nerve events: {n_events}, kinds: {}",
        kinds.join(",")
    ));
    for k in ["gaia.heal.applied", "gaia.heal.refused", "service.probe"] {
        c.that(&format!("nerve saw {k}"), kinds.iter().any(|x| x == k));
    }
    let jsonl = std::fs::read_to_string(work.path("nerve.jsonl")).unwrap_or_default();
    c.eq(
        "nerve JSONL lines on disk = events",
        jsonl.lines().count(),
        n_events,
    );
    let token = std::fs::read_to_string(&tok).unwrap();
    let leaked = [&t1, &t2, &t3, &jsonl, &ev]
        .iter()
        .any(|s| s.contains(token.trim()));
    c.that("test token not in tick output or nerve log", !leaked);

    // Teardown: the restarted dummy runs detached in its own process group.
    if let Some(p) = pid {
        let _ = Command::new("kill").arg(p.to_string()).status();
    }
    nerve.kill();
    let open = realrun::ports_closed(&[nerve_port, dummy_port], Duration::from_secs(3));
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_integration_1::heal");
}
