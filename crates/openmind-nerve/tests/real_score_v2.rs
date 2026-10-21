// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of score v2 (cortex plan T3.3; gated: `GALION_REAL=1`, run
//! with `--release` for the release binary). Slot 108: ports 19780-19789.
//!
//! Every report the real binary produces is checked against the v2
//! no-double-count invariant ([`check`]): a finding that an open incident
//! already scores (same cause, or an absorbing `ddos:` / `stack-wide`
//! incident listing its component) has no deduction of its own; every other
//! non-info finding is in exactly one deduction; the deductions add up to
//! `100 - score`.
//!
//! 1. `openmind-nerve replay fixtures/cortex-replay`: still 21/F with the
//!    golden deductions (the stream flood `ddos:tuner` costs 25 once).
//! 2. The live capture `/workspace/swarm/nerve-capture/replay1` (or
//!    `$CORTEX_REPLAY_DIR`) still scores 61/D when present.
//! 3. `openmind-nerve serve` on 127.0.0.1:19780 with a live service
//!    (:19781), a dead one (:19782) and two stray listeners (:19783,
//!    :19784): the strays are one finding and one 4-point deduction, the
//!    dead service is one incident; the strays leave and their deduction
//!    goes.

use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

/// Poll `/api/v1/self-eval` until `ok` holds (or 25 s pass).
fn wait_eval(port: u16, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let r = get(port, "/api/v1/self-eval").unwrap_or_default();
        if ok(&r) || t.elapsed() > Duration::from_secs(25) {
            return r;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn replay(dir: &Path, data: &Path) -> Value {
    let o = Command::new(BIN)
        .env_clear()
        .env("NERVE_DATA_DIR", data)
        .arg("replay")
        .arg(dir)
        .stderr(Stdio::null())
        .output()
        .expect("run replay");
    serde_json::from_slice(&o.stdout).expect("replay prints the report")
}

/// The v2 invariant on one self-eval report; returns (covered, scored).
fn check(r: &Value) -> (usize, usize) {
    let deds: Vec<(u64, String, String)> = r["deductions"]
        .as_array()
        .expect("deductions")
        .iter()
        .map(|d| {
            (
                d["points"].as_u64().unwrap(),
                d["what"].as_str().unwrap().to_string(),
                d["why"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let sum: u64 = deds.iter().map(|d| d.0).sum();
    assert_eq!(
        r["score"].as_u64().unwrap(),
        100u64.saturating_sub(sum),
        "deductions add up: {deds:?}"
    );
    let incs: Vec<(String, BTreeSet<String>)> = r["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["cause"].as_str().unwrap().to_string(),
                i["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_string())
                    .collect(),
            )
        })
        .collect();
    let cx = &r["cortex"];
    let empty = vec![];
    let all = cx["vulnerabilities"]
        .as_array()
        .unwrap()
        .iter()
        .chain(cx["bleeding"].as_array().unwrap())
        .chain(cx["ddos"]["findings"].as_array().unwrap_or(&empty));
    let (mut covered, mut scored) = (0, 0);
    for f in all {
        if f["severity"] == "info" {
            continue;
        }
        let what = format!(
            "{} ({}): {}",
            f["kind"].as_str().unwrap(),
            f["severity"].as_str().unwrap(),
            f["title"].as_str().unwrap()
        );
        let comp = f["component"].as_str().unwrap();
        let is_covered = incs.iter().any(|(c, comps)| {
            f["incident_cause"].as_str() == Some(c.as_str())
                || ((c.starts_with("ddos:") || c == "stack-wide") && comps.contains(comp))
        });
        let own = deds.iter().filter(|d| d.1 == what).count();
        let folded = deds
            .iter()
            .filter(|d| {
                d.2.contains("scored once: also")
                    && d.2.contains(&format!(" {} ", f["id"].as_str().unwrap()))
            })
            .count();
        if is_covered {
            assert_eq!(own + folded, 0, "double counted: {what}");
            covered += 1;
        } else {
            assert_eq!(own + folded, 1, "scored exactly once: {what}");
            scored += 1;
        }
    }
    (covered, scored)
}

#[test]
fn real_score_v2_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-score-v2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Golden replay.
    let r = replay(&fx.join("cortex-replay"), &dir.join("replay-data"));
    let want: Value = serde_json::from_str(
        &std::fs::read_to_string(fx.join("cortex-replay.expected.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (
            r["score"].as_u64(),
            r["grade"].as_str(),
            r["pass"].as_bool()
        ),
        (Some(21), Some("F"), Some(false))
    );
    assert_eq!(r["deductions"], want["deductions"], "v2 keeps v1 points");
    let flood: Vec<u64> = r["deductions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["why"].as_str().unwrap().contains("`ddos:tuner`"))
        .map(|d| d["points"].as_u64().unwrap())
        .collect();
    assert_eq!(flood, [25], "stream flood scored once, as its incident");
    let (c, s) = check(&r);
    eprintln!(
        "replay fixture: score {} grade {} pass {}; {} deductions, findings covered {c} / scored {s}; ddos firing {}",
        r["score"], r["grade"], r["pass"], r["deductions"].as_array().unwrap().len(),
        r["cortex"]["ddos"]["firing"]
    );

    // 2. Live capture.
    let live = std::env::var("CORTEX_REPLAY_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/nerve-capture/replay1".into());
    if Path::new(&live).join("state-1.json").exists() {
        let r = replay(Path::new(&live), &dir.join("replay1-data"));
        assert_eq!(
            (r["score"].as_u64(), r["grade"].as_str()),
            (Some(61), Some("D")),
            "replay1 verdict changed"
        );
        let (c, s) = check(&r);
        eprintln!(
            "replay1 live capture: score 61 grade D (unchanged); findings covered {c} / scored {s}"
        );
    } else {
        eprintln!("replay1 live capture not on this box: skipped");
    }

    // 3. Serve on 19780.
    let svc = TcpListener::bind("127.0.0.1:19781").expect("19781 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        r#"[{"name":"live-svc","port":19781},{"name":"dead-svc","port":19782}]"#,
    )
    .unwrap();
    let strays = [
        TcpListener::bind("127.0.0.1:19783").expect("19783 free"),
        TcpListener::bind("127.0.0.1:19784").expect("19784 free"),
    ];
    let _nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", "127.0.0.1:19780")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_REPORT_EVERY", "1")
            .env("NERVE_DATA_DIR", dir.join("data-19780"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19780-19789")
            .env_remove("NERVE_SENSORS_OFF")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let stray_ded = |r: &Value| {
        r["deductions"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|d| d["what"].as_str().unwrap_or("").contains("19783 :19784"))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let r = wait_eval(19780, |r| {
        !stray_ded(r).is_empty() && !r["incidents"].as_array().unwrap().is_empty()
    });
    let sd = stray_ded(&r);
    assert_eq!(sd.len(), 1, "two strays, one finding, one deduction: {r}");
    assert_eq!(sd[0]["points"], 4);
    assert_eq!(
        sd[0]["what"],
        "vulnerability (warn): 2 loopback listener(s) no monitored unit owns: :19783 :19784"
    );
    let (c, s) = check(&r);
    let points: Vec<u64> = r["deductions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["points"].as_u64().unwrap())
        .collect();
    eprintln!(
        "serve :19780 cycle {} (strays up): score {} grade {} pass {}; deductions {points:?}; incidents {:?}; findings covered {c} / scored {s}",
        r["cycle"], r["score"], r["grade"], r["pass"],
        r["incidents"].as_array().unwrap().iter().map(|i| format!("{} {}", i["cause"], i["severity"])).collect::<Vec<_>>()
    );
    let before = r["score"].as_u64().unwrap();
    drop(strays);
    let r = wait_eval(19780, |r| {
        r["cycle"].as_u64().unwrap_or(0) > 0 && stray_ded(r).is_empty()
    });
    assert!(stray_ded(&r).is_empty(), "stray deduction cleared: {r}");
    assert_eq!(r["score"].as_u64().unwrap(), (before + 4).min(100));
    let (c, s) = check(&r);
    eprintln!(
        "serve :19780 cycle {} (strays gone): score {} grade {} pass {}; findings covered {c} / scored {s}",
        r["cycle"], r["score"], r["grade"], r["pass"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
