// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real on-box test of the `openmind-discovery` CLI (Rust port of the old
//! `scripts/real/lan-discovery.sh`). Runs only with `GALION_REAL=1`.
//!
//! Ports (slot 11 range by default, `GALION_REAL_PORT_BASE` moves them):
//! base+0/udp mDNS port of the test announcer and the browse (not 5353, so no
//! other responder answers and avahi is untouched); +1 stand-in HTTP port for
//! the announced `_galion._tcp`; +2 for `_exo._tcp`; +3 a port only the probe
//! finds (ollama kind); +4 closed on purpose.
//!
//! Flow: release build -> stubs + announcer -> find (2 pending rows) -> join
//! refused while pending -> deny -> find again (stays rejected) -> approve ->
//! join (health up) -> probe 127.0.0.0/24 host .1 only -> refuse a public /24
//! -> approvals log has Zagreb stamps -> teardown. The probe touches only
//! 127.0.0.1.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-discovery --test real_discovery_cli -- --nocapture
//! ```

use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn d(bin: &PathBuf, reg: &PathBuf, log: &PathBuf, args: &[&str]) -> (i32, Value, String) {
    let out = realrun::run(
        Command::new(bin)
            .args(args)
            .arg("--registry")
            .arg(reg)
            .arg("--log")
            .arg(log)
            .args(["--by", "real-test"]),
    );
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn devices(v: &Value) -> Vec<Value> {
    v["devices"].as_array().cloned().unwrap_or_default()
}

fn count_new_pending(v: &Value) -> usize {
    devices(v)
        .iter()
        .filter(|d| d["status"] == "new" && d["approval"] == "pending")
        .count()
}

#[test]
fn find_deny_approve_join_probe_refuse() {
    if !realrun::enabled("real_discovery_cli") {
        return;
    }
    let base = realrun::port_base(19110);
    let (mdns, galion, exo, ollama, closed) = (base, base + 1, base + 2, base + 3, base + 4);
    realrun::require_free(&[galion, exo, ollama, closed]);
    let bins = realrun::release_bins(&[
        "-p",
        "openmind-discovery",
        "--bin",
        "openmind-discovery",
        "--example",
        "health_stub",
    ]);
    let bin = bins.join("openmind-discovery");
    let stub = bins.join("examples/health_stub");
    let work = Scratch::new("lan-discovery");
    let reg = work.path("providers.json");
    let log = work.path("approvals.log");
    let mut c = Checks::new();
    let ms = mdns.to_string();
    let id = "galion-gaia-real";

    realrun::say(format!("start stand-in HTTP ports {galion} {exo} {ollama}"));
    let _stubs: Vec<Proc> = [galion, exo, ollama]
        .iter()
        .map(|p| {
            let mut cmd = Command::new(&stub);
            cmd.args([p.to_string(), "60".into()]);
            Proc::spawn(&format!("stub-{p}"), cmd, &work.path("stubs.log"))
        })
        .collect();

    realrun::say("start test announcer: _galion._tcp + _exo._tcp");
    let mut ann_cmd = Command::new(&bin);
    ann_cmd
        .args([
            "announce",
            "--mdns-port",
            &ms,
            "--host",
            "gaia-real",
            "--addr",
            "127.0.0.1",
        ])
        .args(["--duration-s", "20", "--service"])
        .arg(format!(
            "galion:gaia-real:{galion}:id=gaia-real;caps=llm;v=1"
        ))
        .arg("--service")
        .arg(format!("exo:gaia-real:{exo}:v=1"));
    let mut ann = Proc::spawn("announcer", ann_cmd, &work.path("announce.log"));
    std::thread::sleep(Duration::from_millis(500));

    let find_mdns = || {
        let (code, v, err) = d(
            &bin,
            &reg,
            &log,
            &[
                "find",
                "--mdns-port",
                &ms,
                "--services",
                "galion,exo",
                "--timeout-ms",
                "1500",
            ],
        );
        assert_eq!(code, 0, "find failed: {err}");
        v
    };

    realrun::say("find (mDNS browse, 1 query + 1 repeat, 1.5 s)");
    let v = find_mdns();
    realrun::say(format!("mdns {}", v["mdns"]));
    c.that(
        "at most 2 query packets",
        v["mdns"]["packets_sent"].as_u64().unwrap_or(99) <= 2,
    );
    c.eq("2 new pending devices", count_new_pending(&v), 2);

    let (_, l) = realrun::run_json(
        Command::new(&bin)
            .args(["list", "--registry"])
            .arg(&reg)
            .args(["--approval", "pending"]),
    );
    c.eq(
        "list --approval pending shows 2",
        l["providers"].as_array().map(Vec::len),
        Some(2),
    );

    realrun::say("join while pending (must be refused)");
    let (code, _, err) = d(&bin, &reg, &log, &["join", id]);
    c.that("join of a pending row refused", code != 0);
    realrun::say(format!("join refused: {}", err.trim()));

    let (code, _, _) = d(
        &bin,
        &reg,
        &log,
        &["deny", id, "--note", "real test: deny first"],
    );
    c.eq("deny exit", code, 0);
    let v = find_mdns();
    let appr = devices(&v)
        .iter()
        .find(|x| x["provider_id"] == id)
        .map(|x| x["approval"].as_str().unwrap_or("").to_string());
    c.eq(
        "denied row stays rejected",
        appr.as_deref(),
        Some("rejected"),
    );

    let (code, _, _) = d(
        &bin,
        &reg,
        &log,
        &["approve", id, "--note", "real test: approve after deny"],
    );
    c.eq("approve exit", code, 0);
    let (_, j, _) = d(&bin, &reg, &log, &["join", id]);
    c.eq("join health up", j["health"].as_str(), Some("up"));

    realrun::say("probe 127.0.0.0/24 host .1 only, 4 ports, 10/s");
    let ports = format!("galion:{galion},exo:{exo},ollama:{ollama},llamacpp:{closed}");
    let (code, v, err) = d(
        &bin,
        &reg,
        &log,
        &[
            "find",
            "--no-mdns",
            "--probe",
            "127.0.0.0/24",
            "--probe-hosts",
            "1-1",
            "--rate",
            "10",
            "--probe-ports",
            &ports,
        ],
    );
    assert_eq!(code, 0, "probe failed: {err}");
    realrun::say(format!("probe {}", v["probe"]));
    c.eq("probe attempts", v["probe"]["attempts"].as_u64(), Some(4));
    c.eq("probe open", v["probe"]["open"].as_u64(), Some(3));
    let galion_row = devices(&v)
        .into_iter()
        .find(|x| x["endpoint"] == format!("http://127.0.0.1:{galion}"))
        .map(|x| {
            format!(
                "{} {}",
                x["provider_id"].as_str().unwrap_or(""),
                x["approval"].as_str().unwrap_or("")
            )
        });
    c.eq(
        "probe hit on the announced port is the same row",
        galion_row.as_deref(),
        Some("galion-gaia-real approved"),
    );
    c.eq("1 new pending probe row", count_new_pending(&v), 1);

    realrun::say("probe of a public /24 is refused before any socket");
    let out = realrun::run(
        Command::new(&bin)
            .args([
                "find",
                "--no-mdns",
                "--probe",
                "203.0.113.0/24",
                "--registry",
            ])
            .arg(&reg),
    );
    c.that("public probe refused", !out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    realrun::say(format!("refuse: {}", err.lines().next().unwrap_or("")));
    c.that("refusal names the rule", err.contains("refusing to probe"));

    let alog = std::fs::read_to_string(&log).unwrap_or_default();
    realrun::say(format!("approvals log:\n{alog}"));
    c.that(
        "approvals log has a join-refused event",
        alog.contains("\"join-refused\""),
    );
    c.that(
        "no UTC Z timestamps in approvals log",
        !alog.contains("Z\""),
    );
    for line in alog.lines() {
        let v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
        if let Some(ts) = v["ts"].as_str() {
            if zagreb_time::require_rfc3339(ts).is_err() {
                c.bad(&format!("non-Zagreb ts {ts}"));
            }
        }
    }

    realrun::say("wait for the announcer to finish its 20 s");
    let rc = ann.wait(Duration::from_secs(25));
    c.eq("announcer exits on its own", rc, Some(0));
    drop(_stubs);
    let open = realrun::ports_closed(&[galion, exo, ollama, closed], Duration::from_secs(3));
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_discovery_cli");
}
