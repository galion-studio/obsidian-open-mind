// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the built `hades-gate` binary. Runs only with
//! `GALION_REAL=1` (it binds 127.0.0.1:19021 and sleeps for the expiry case).
//! Throwaway keys in a temp dir; token strings are never printed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_hades-gate");
const PORT: u16 = 19021;

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn tmp() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("hades-real-cli-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run hades-gate")
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn mint(
    dir: &Path,
    seed: &str,
    action: &str,
    target: &str,
    plan: &Path,
    ttl: &str,
    name: &str,
) -> PathBuf {
    let out = dir.join(name);
    let o = run(&[
        "mint",
        "--seed",
        seed,
        "--action",
        action,
        "--target",
        target,
        "--plan-file",
        s(plan),
        "--ttl",
        ttl,
        "--out",
        s(&out),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    out
}

fn check(cfg: &Path, target: &str, plan: &Path, token: &Path) -> (i32, String) {
    let o = run(&[
        "check",
        "--config",
        s(cfg),
        "--action",
        "heal-apply",
        "--target",
        target,
        "--plan-file",
        s(plan),
        "--token-file",
        s(token),
    ]);
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn http(method: &str, path: &str, body: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", PORT)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        c,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn real_cli_and_http_round_trip() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = tmp();
    let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
    assert!(run(&["keygen", "--seed", s(&seed), "--pub", s(&public)])
        .status
        .success());
    // keygen never overwrites.
    assert!(!run(&["keygen", "--seed", s(&seed), "--pub", s(&public)])
        .status
        .success());
    let cfg = dir.join("hades.toml");
    std::fs::write(
        &cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            s(&public),
            s(&dir.join("ledger.jsonl")),
            s(&dir.join("DISABLED"))
        ),
    )
    .unwrap();
    let plan = dir.join("plan.txt");
    std::fs::write(&plan, "restart galion-tuner").unwrap();
    let seed_s = s(&seed).to_owned();

    // Approve once, then replay.
    let t = mint(
        &dir,
        &seed_s,
        "heal-apply",
        "galion-tuner",
        &plan,
        "60",
        "a.tok",
    );
    let (rc, out) = check(&cfg, "galion-tuner", &plan, &t);
    assert_eq!(rc, 0, "{out}");
    assert!(
        out.contains("\"WouldApply\"") && out.contains("\"executed\":false"),
        "{out}"
    );
    let (rc, out) = check(&cfg, "galion-tuner", &plan, &t);
    assert_eq!((rc, out.contains("\"replayed\"")), (2, true), "{out}");

    // Tamper: change one payload digit.
    let t2 = mint(
        &dir,
        &seed_s,
        "heal-apply",
        "galion-tuner",
        &plan,
        "60",
        "b.tok",
    );
    let text = std::fs::read_to_string(&t2).unwrap();
    let mut parts: Vec<String> = text.split('.').map(str::to_owned).collect();
    let last = parts[1].pop().unwrap();
    parts[1].push(if last == '3' { '2' } else { '3' });
    let tampered = dir.join("tampered.tok");
    std::fs::write(&tampered, parts.join(".")).unwrap();
    let (rc, out) = check(&cfg, "galion-tuner", &plan, &tampered);
    assert_eq!((rc, out.contains("\"forged\"")), (2, true), "{out}");

    // Wrong scope.
    let (rc, out) = check(&cfg, "galion-one", &plan, &t2);
    assert_eq!((rc, out.contains("\"wrong-scope\"")), (2, true), "{out}");

    // Expiry.
    let t3 = mint(
        &dir,
        &seed_s,
        "heal-apply",
        "galion-tuner",
        &plan,
        "1",
        "c.tok",
    );
    std::thread::sleep(Duration::from_secs(2));
    let (rc, out) = check(&cfg, "galion-tuner", &plan, &t3);
    assert_eq!((rc, out.contains("\"expired\"")), (2, true), "{out}");

    // HTTP: health, approve, replay, stop.
    let mut child = Command::new(BIN)
        .args(["serve", "--config", s(&cfg), "--port", &PORT.to_string()])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut up = false;
    for _ in 0..50 {
        if TcpStream::connect(("127.0.0.1", PORT)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(up, "server did not start");
    assert!(http("GET", "/health", "").contains("\"mode\":\"dry-run\""));
    let token = std::fs::read_to_string(&t2).unwrap();
    let body = serde_json::json!({
        "action": "heal-apply", "target": "galion-tuner",
        "plan": "restart galion-tuner", "token": token,
    })
    .to_string();
    let first = http("POST", "/v1/approve", &body);
    assert!(
        first.starts_with("HTTP/1.1 200") && first.contains("WouldApply"),
        "{first}"
    );
    let again = http("POST", "/v1/approve", &body);
    assert!(
        again.starts_with("HTTP/1.1 403") && again.contains("replayed"),
        "{again}"
    );
    assert!(http("POST", "/v1/stop", "").contains("stopping"));
    let status = child.wait().unwrap();
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&dir);
}
