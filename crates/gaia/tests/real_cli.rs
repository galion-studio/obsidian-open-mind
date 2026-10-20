// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the release `gaia` binary. Rust only (no shell script):
//!
//! ```text
//! CARGO_INCREMENTAL=0 GALION_REAL=1 GAIA_VPS=root@<host> \
//!   cargo test --release -j 3 -p gaia --test real_cli -- --nocapture --test-threads 1
//! ```
//!
//! * `real_gaia_commands` starts the real binary for status, plan, dry-run,
//!   dispatch, simulate-faro, ownership and view (fixture) and checks the JSON.
//! * `real_live_vps_view` (only with `GAIA_VPS`) takes a READ-ONLY capture of
//!   the live host over ssh (`systemctl list-units ...`, `ss -ltnp`), writes it
//!   under the target dir and prints Gaia's view of it. Nothing on the host is
//!   changed. The gaia binary itself opens no socket and spawns nothing, so it
//!   needs no port (swarm slot 01 range 19010-19019 stays unused).
//!
//! Skips unless `GALION_REAL=1`.

use std::path::PathBuf;
use std::process::Command;

fn enabled() -> bool {
    if std::env::var("GALION_REAL").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("SKIP: set GALION_REAL=1 to run the real gaia binary");
    false
}

fn gaia(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .arg("--compact")
        .output()
        .expect("start gaia");
    assert!(
        out.status.success(),
        "gaia {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).expect("utf8");
    assert!(s.starts_with("{\"generated_at\":\"20"), "{s}");
    assert!(
        !s.contains("Z\","),
        "timestamps must carry the Zagreb offset"
    );
    s
}

/// The first `"key":value` (up to the next `,` or `}`), or the first
/// `"key":{...}` object, for printing.
fn pick(json: &str, key: &str) -> String {
    let needle = format!("\"{key}\":");
    let Some(i) = json.find(&needle) else {
        return format!("{key}: (absent)");
    };
    let rest = &json[i..];
    let v = &rest[needle.len()..];
    let end = if v.starts_with('{') || v.starts_with('[') {
        let (open, close) = if v.starts_with('{') {
            ('{', '}')
        } else {
            ('[', ']')
        };
        let mut depth = 0usize;
        let mut end = v.len();
        for (j, ch) in v.char_indices() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    end = j + 1;
                    break;
                }
            }
        }
        needle.len() + end
    } else {
        needle.len() + v.find([',', '}']).unwrap_or(v.len())
    };
    rest[..end].to_string()
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn real_gaia_commands() {
    if !enabled() {
        return;
    }
    let status = gaia(&["status"]);
    println!("status: {}", pick(&status, "generated_at"));
    assert!(status.contains("\"function\":\"ELEUTHIA\""));
    for s in ["rollback", "kill-switch", "deploy-switch"] {
        let p = gaia(&["plan", s]);
        println!("plan {s}: {} {}", pick(&p, "outcome"), pick(&p, "summary"));
        assert!(p.contains("\"outcome\":\"dry-run\""), "{p}");
    }
    let dry = gaia(&["dry-run"]);
    println!("dry-run: {}", pick(&dry, "totals"));
    assert!(dry.contains("\"destructive_refused\":5,"));
    assert!(dry.contains("\"destructive_acted\":0"));
    let ex = gaia(&["dispatch", "teardown", "execute"]);
    println!(
        "dispatch teardown execute (no approval): {}",
        pick(&ex, "outcome")
    );
    assert!(ex.contains("\"outcome\":\"refused\""));
    let faro = gaia(&["simulate-faro", "--full-power"]);
    println!("simulate-faro --full-power: {}", pick(&faro, "summary"));
    println!("  sweep: {}", pick(&faro, "totals"));
    assert!(faro.contains("\"deterministic\":true"));
    let own = gaia(&["ownership"]);
    println!(
        "ownership: {} {} {}",
        pick(&own, "crates_total"),
        pick(&own, "system_crates"),
        pick(&own, "systems_with_a_crate")
    );
    assert!(own.contains("\"crates_total\":46"));
    let view = gaia(&[
        "view",
        "--units",
        &fixture("vps-units-20261020.txt"),
        "--listen",
        &fixture("vps-ss-20261020.txt"),
    ]);
    println!("view (fixture): {}", pick(&view, "summary"));
    assert!(view.contains("\"units_unmapped\":0"));
    let bad = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(["plan", "no-such-system"])
        .output()
        .expect("start gaia");
    assert_eq!(bad.status.code(), Some(2), "bad input exits 2");
}

/// Run one READ-ONLY command on the host and return stdout.
fn ssh_read(host: &str, cmd: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let key = std::env::var("GAIA_SSH_KEY").unwrap_or(format!("{home}/.ssh/id_ed25519_vps"));
    let out = Command::new("ssh")
        .args([
            "-i",
            &key,
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
        ])
        .args(["-o", "ConnectTimeout=10", host, cmd])
        .output()
        .expect("start ssh");
    assert!(out.status.success(), "ssh {cmd:?} failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn real_live_vps_view() {
    if !enabled() {
        return;
    }
    let Ok(host) = std::env::var("GAIA_VPS") else {
        eprintln!("SKIP: GAIA_VPS not set (no live probe)");
        return;
    };
    // The probes are fixed strings: read-only listings, nothing else.
    let probe = "systemctl list-units --type=service 'galion-*' 'openmind-*' --no-pager";
    let gm = ssh_read(&host, probe);
    let all = ssh_read(
        &host,
        "systemctl list-units --type=service --all --no-pager 'galion-*' 'openmind-*' \
         'hermes*' 'link-watch*' 'memory-gateway*' 'tuner*' 'console.service' \
         'cloudflared*' 'tanga-*'",
    );
    // Drop fd numbers from the process column; keep names and pids.
    let ss: String = ssh_read(&host, "ss -ltnp")
        .lines()
        .map(|l| {
            let mut s = l.to_string();
            while let Some(i) = s.find(",fd=") {
                let end = s[i + 4..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map_or(s.len(), |e| i + 4 + e);
                s.replace_range(i..end, "");
            }
            s + "\n"
        })
        .collect();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gaia-land-live");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let (pu, ps) = (dir.join("units.txt"), dir.join("ss.txt"));
    std::fs::write(&pu, &all).expect("write units");
    std::fs::write(&ps, &ss).expect("write ss");
    std::fs::write(dir.join("units-galion-openmind.txt"), &gm).expect("write gm");
    println!("live probe `{probe}`:");
    for l in gm.lines().filter(|l| l.contains(".service")) {
        println!("  {}", l.trim());
    }
    let view = gaia(&[
        "view",
        "--units",
        pu.to_str().expect("utf8 path"),
        "--listen",
        ps.to_str().expect("utf8 path"),
    ]);
    std::fs::write(dir.join("view.json"), &view).expect("write view");
    println!("gaia view (live): {}", pick(&view, "summary"));
    let mut line = String::new();
    let mut rest = view.as_str();
    while let Some(i) = rest.find("\"observed\":\"") {
        let before = &rest[..i];
        let sys = before
            .rfind("\"system\":\"")
            .map(|j| &before[j + 10..before.len() - 2])
            .unwrap_or("?");
        let v = &rest[i + 12..];
        let obs = &v[..v.find('"').unwrap_or(0)];
        line.push_str(&format!("{sys}={obs} "));
        rest = v;
    }
    println!("per system: {line}");
    println!("{}", pick(&view, "unmapped_listeners"));
    println!("{}", pick(&view, "unmapped_units"));
    println!("captures: {}", dir.display());
    assert!(view.contains("\"mode\":\"observe-only"));
    assert!(view.contains("\"system\":\"memory-gateway\""));
}
