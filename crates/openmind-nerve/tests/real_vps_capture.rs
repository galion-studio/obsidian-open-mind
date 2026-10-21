// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! READ-ONLY capture of the live VPS nerve and its inputs, then a replay of
//! that capture through this build (Rust port of the old
//! `scripts/real/nerve-capture-vps.sh`). Runs only with `GALION_REAL=1` AND
//! `GALION_REAL_VPS=1`.
//!
//! On the VPS only loopback HTTP GETs (`curl -s`), `ss -ltn` and `journalctl`
//! reads run; every byte is streamed back over ssh stdout, nothing is written
//! there and nothing is restarted. The capture stays on the box in
//! `NERVE_CAPTURE_OUT` (default: a private scratch dir removed afterwards);
//! it may hold journal lines, so it is never printed or committed: the test
//! prints file names and sizes only.
//!
//! Env: `NERVE_VPS` (ssh host alias, default `slavik-vps`),
//! `NERVE_CAPTURE_N` (state snapshots, default 3), `NERVE_CAPTURE_GAP_S`
//! (seconds between them, default 35).
//!
//! ```text
//! GALION_REAL=1 GALION_REAL_VPS=1 NERVE_CAPTURE_OUT=/path/dir \
//!   cargo test --release -p openmind-nerve --test real_vps_capture -- --nocapture
//! openmind-nerve replay /path/dir --md
//! ```

use galion_realtest::realrun::{self, Checks, Scratch};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Fixed, read-only remote commands (no caller input reaches the shell).
const JOURNAL: &str = "journalctl --since \"-35min\" --no-pager -q -o json --output-fields=MESSAGE,PRIORITY,_SYSTEMD_UNIT,UNIT -u cloudflared -u galion-one -u openmind-memory -u hermes-rs -u link-watch-rs -u galion-desk -u tuner -u console -u galion-health -u tuner-quick";
const PROBES: &str = "for x in 8091/health 8002/health 8642/health 8420/health 8646/health 8650/health 8080/ 8660/status.json 8670/ 8670/health; do printf \"%s \" $x; curl -s -m 5 -o /dev/null -w \"%{http_code}\\n\" http://127.0.0.1:$x; done";

fn ssh(host: &str, remote: &str, out: &Path) -> bool {
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            host,
            remote,
        ])
        .output()
        .expect("spawn ssh");
    std::fs::write(out, &o.stdout).unwrap();
    o.status.success()
}

fn env_u64(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

#[test]
fn capture_live_nerve_readonly_and_replay() {
    if !realrun::enabled("real_vps_capture") {
        return;
    }
    if !realrun::vps_enabled() {
        eprintln!(
            "real_vps_capture: skipped (set GALION_REAL_VPS=1 for the read-only VPS capture)"
        );
        return;
    }
    let host = std::env::var("NERVE_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let n = env_u64("NERVE_CAPTURE_N", 3).max(1);
    let gap = env_u64("NERVE_CAPTURE_GAP_S", 35);
    let scratch = Scratch::new("nerve-capture");
    let out: PathBuf = std::env::var_os("NERVE_CAPTURE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.0.clone());
    std::fs::create_dir_all(&out).unwrap();
    let bins = realrun::release_bins(&["-p", "openmind-nerve"]);
    let mut c = Checks::new();

    realrun::say(format!("capture: {n} state snapshot(s), {gap} s apart"));
    for i in 1..=n {
        c.that(
            &format!("GET 8670/state #{i}"),
            ssh(
                &host,
                "curl -s -m 10 http://127.0.0.1:8670/state",
                &out.join(format!("state-{i}.json")),
            ),
        );
        if i < n {
            std::thread::sleep(Duration::from_secs(gap));
        }
    }
    for (remote, file) in [
        (
            "curl -s -m 10 http://127.0.0.1:8670/self-eval",
            "live-self-eval.json.txt",
        ),
        (
            "curl -s -m 10 http://127.0.0.1:8670/self-eval.md",
            "live-self-eval.md.txt",
        ),
        (
            "curl -s -m 10 http://127.0.0.1:8660/status.json",
            "health_status.json",
        ),
        (
            "curl -s -m 5 http://127.0.0.1:8002/health",
            "body-8002.json",
        ),
        ("ss -ltn", "ss.txt"),
        (JOURNAL, "journal.jsonl"),
        (PROBES, "probes.txt"),
    ] {
        // curl -s exits non-zero when a port is closed; the file is still the evidence.
        let ok = ssh(&host, remote, &out.join(file));
        realrun::say(format!("{file}: ssh exit ok={ok}"));
    }
    std::fs::write(
        out.join("CAPTURED"),
        format!("captured {}\n", realrun::stamp()),
    )
    .unwrap();
    let mut listing = Vec::new();
    for e in std::fs::read_dir(&out).unwrap().flatten() {
        let len = e.metadata().map(|m| m.len()).unwrap_or(0);
        listing.push(format!("{len:>9}  {}", e.file_name().to_string_lossy()));
    }
    listing.sort_by(|a, b| a[11..].cmp(&b[11..]));
    realrun::say(format!("capture dir (sizes only):\n{}", listing.join("\n")));
    let state1 = std::fs::read_to_string(out.join("state-1.json")).unwrap_or_default();
    c.that(
        "live nerve state is JSON",
        serde_json::from_str::<serde_json::Value>(&state1).is_ok(),
    );
    let probes = std::fs::read_to_string(out.join("probes.txt")).unwrap_or_default();
    c.eq("10 probe lines", probes.lines().count(), 10);

    realrun::say("replay the capture through this build");
    let o = realrun::run(
        Command::new(bins.join("openmind-nerve"))
            .arg("replay")
            .arg(&out)
            .arg("--md"),
    );
    let code = o.status.code().unwrap_or(-1);
    let md = String::from_utf8_lossy(&o.stdout);
    // Exit 0 = PASS, 1 = FAIL verdict (both are a working replay); 2 = error.
    c.that(
        &format!("replay ran (exit {code}, 0 PASS / 1 FAIL verdict)"),
        code == 0 || code == 1,
    );
    c.that(
        "replay rendered a self-eval",
        md.starts_with("# openmind-nerve self-eval"),
    );
    for l in md
        .lines()
        .filter(|l| l.starts_with("- result:") || l.starts_with("- system:"))
    {
        realrun::say(format!("replay {l}"));
    }
    c.finish("real_vps_capture");
}
