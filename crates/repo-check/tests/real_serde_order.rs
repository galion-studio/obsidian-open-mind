// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the serde_json `preserve_order` pin (gated:
//! `GALION_REAL=1`, slot 146 ports 20160-20169).
//!
//! 1. For every workspace member that depends on serde_json, asks cargo
//!    itself (`cargo tree -e features`) whether `preserve_order` is on, once
//!    for `-p <crate>` alone and once for `--workspace`. Both must say yes.
//! 2. Builds the real `openmind-nerve` release binary with `-p
//!    openmind-nerve` ONLY (no other crate that could switch the feature on),
//!    starts `openmind-nerve serve` on 127.0.0.1:20160, and checks the raw
//!    `/health` bytes keep source order (`status, service, version, ...`).
//!    Before the pin that build sorted keys (`appended, cycle, score, ...`),
//!    exactly what the live VPS shadow nerve on :8672 prints today.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 20160;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn gated() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return false;
    }
    true
}

/// Member crate names under crates/*.
fn members() -> Vec<String> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(root().join("crates")).unwrap().flatten() {
        let Ok(t) = std::fs::read_to_string(e.path().join("Cargo.toml")) else {
            continue;
        };
        let name = t
            .lines()
            .find_map(|l| l.strip_prefix("name = \""))
            .and_then(|r| r.split('"').next())
            .map(str::to_string);
        v.extend(name);
    }
    v.sort();
    v
}

/// `Some(true/false)` = preserve_order on/off, `None` = no serde_json.
fn preserve_order(sel: &[&str], edges: &str) -> Option<bool> {
    let out = cargo()
        .args(["tree", "--offline", "-e", edges, "-i", "serde_json"])
        .args(sel)
        .output()
        .expect("cargo tree");
    let s = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || s.trim().is_empty() {
        return None;
    }
    Some(s.contains("feature \"preserve_order\""))
}

#[test]
fn real_cargo_resolves_preserve_order_for_every_member() {
    if !gated() {
        return;
    }
    let mut on = Vec::new();
    let mut off = Vec::new();
    for edges in ["features", "features,normal"] {
        for m in members() {
            match preserve_order(&["-p", &m], edges) {
                Some(true) => on.push(format!("{edges}:{m}")),
                Some(false) => off.push(format!("{edges}:{m}")),
                None => {}
            }
        }
    }
    eprintln!("preserve_order with -p: on={} off={}", on.len(), off.len());
    assert!(off.is_empty(), "-p builds without preserve_order: {off:?}");
    assert!(on.len() >= 40, "{on:?}");
    assert_eq!(preserve_order(&["--workspace"], "features"), Some(true));
}

fn get(path: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{PORT}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

/// Top-level keys in byte order, read from the raw body (no re-parse, which
/// would itself reorder).
fn top_keys(body: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let (mut depth, mut in_str, mut esc, mut start) = (0i32, false, false, None);
    let b = body.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
                if let Some(s) = start.take() {
                    let rest = body[i + 1..].trim_start();
                    if depth == 1 && rest.starts_with(':') {
                        keys.push(body[s..i].to_string());
                    }
                }
            }
            continue;
        }
        match c {
            b'"' => {
                in_str = true;
                start = Some(i + 1);
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
    }
    keys
}

#[test]
fn top_keys_reads_byte_order() {
    assert_eq!(
        top_keys(r#"{"b":1,"a":{"z":"x\"y","c":[{"q":1}]},"c":"k:"}"#),
        ["b", "a", "c"]
    );
}

#[test]
fn real_nerve_built_alone_keeps_source_order() {
    if !gated() {
        return;
    }
    // Own sub-dir: the outer `cargo test` may hold the main build-dir lock.
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target"))
        .join("serde-order-pin-alone");
    let st = cargo()
        .args([
            "build",
            "--offline",
            "--release",
            "-j",
            "2",
            "-p",
            "openmind-nerve",
            "--bin",
            "openmind-nerve",
        ])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .expect("cargo build");
    assert!(st.success());
    let bin = target.join("release/openmind-nerve");
    let dir = std::env::temp_dir().join(format!("serde-order-pin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"dead-svc","port":{}}}]"#, PORT + 1),
    )
    .unwrap();
    let _nerve = Kill(
        Command::new(&bin)
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{PORT}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", format!("{}-{}", PORT, PORT + 2))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    while get("/healthz").is_none() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let (code, body) = get("/health").expect("GET /health");
    eprintln!("GET /health -> {code} {body}");
    assert_eq!(code, 200);
    assert_eq!(
        top_keys(&body),
        ["status", "service", "version", "appended", "ts", "system", "score", "cycle"],
        "{body}"
    );
    assert!(body.starts_with(r#"{"status":"ok","service":"openmind-nerve""#));
    let (code, state) = get("/api/v1/state").expect("GET /api/v1/state");
    assert_eq!(code, 200);
    let keys = top_keys(&state);
    eprintln!(
        "GET /api/v1/state -> {code}, {} B, keys {keys:?}",
        state.len()
    );
    let mut sorted = keys.clone();
    sorted.sort();
    assert!(keys.len() > 3, "{state}");
    assert_ne!(
        keys, sorted,
        "state keys came out sorted: preserve_order is off"
    );
    drop(_nerve);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(get("/healthz").is_none(), "nerve still listening on {PORT}");
}
