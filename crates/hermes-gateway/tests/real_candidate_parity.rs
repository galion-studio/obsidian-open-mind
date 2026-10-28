// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 416: real-binary parity of the hermes-gateway deploy
//! candidate against a byte-identical copy of the live b7fbbc78 binary.
//! Harness pieces adapted from slot 355 `real_crashloop_r3.rs` (same repo, MIT).
//!
//! Gated: runs only with GALION_REAL=1.
//! - HERMES_GW_BIN       candidate (default $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release)
//! - HERMES_GW_LIVE_BIN  live binary bytes (box: /workspace/swarm/artifacts/t291u/hermes-gateway-b7fbbc78,
//!   VPS: /opt/galion/rust/bin/hermes-gateway-b7fbbc78, run read-only from its path)
//! - HERMES_GW_LIVE_SHA  optional expected sha256 of the live bytes
//! - HERMES_GW_CHECK_DROPIN_SHA=1  the candidate sha256 must equal the
//!   `# sha256` line of the staged 95-gateway-candidate.conf
//! - HERMES_PARITY_SANDBOX=1  also run the coding-ON start (sandboxed
//!   preflight + one real fs_read tool run in the sandbox). sandbox_vps_only:
//!   refused when the box marker /workspace/swarm/RULES.md exists.
//!
//! Ports 22860-22863 (or HERMES_PARITY_PORT_BASE..+3), loopback only, `--mock` (no network, no spend).

use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// First of the four ports; `HERMES_PARITY_PORT_BASE` moves them (slot 434 ran on 23040-23043).
fn port(offset: u16) -> u16 {
    std::env::var("HERMES_PARITY_PORT_BASE")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(22860)
        + offset
}
const API: &str = "t416-parity-gateway-key";
const DROPIN: &str =
    include_str!("../../../deploy/staged/hermes-rs.service.d/95-gateway-candidate.conf");
/// Tools a client can reach through the agent (`tool:<name>` on the mock).
const PROBE_TOOLS: [&str; 6] = [
    "clock",
    "delegate_task",
    "fs_read",
    "fs_write",
    "git",
    "cargo",
];

fn on(k: &str) -> bool {
    std::env::var(k).ok().as_deref() == Some("1")
}

fn cand_bin() -> PathBuf {
    if let Ok(b) = std::env::var("HERMES_GW_BIN") {
        return PathBuf::from(b);
    }
    let t = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "../../target".into());
    PathBuf::from(t).join("x86_64-unknown-linux-musl/release/hermes-gateway")
}

fn sha256(p: &Path) -> String {
    let out = Command::new("sha256sum")
        .arg(p)
        .output()
        .expect("sha256sum");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

/// (status, body) of one HTTP/1.1 request; status 0 = no connection.
fn http(port: u16, method: &str, path: &str, auth: bool, body: Option<&Value>) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if auth {
        req.push_str(&format!("Authorization: Bearer {API}\r\n"));
    }
    if body.is_some() {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        ));
    }
    req.push_str("\r\n");
    req.push_str(&payload);
    let _ = s.write_all(req.as_bytes());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (code, body)
}

struct Gw(Child, PathBuf);
impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Gw {
    fn log(&self) -> String {
        fs::read_to_string(&self.1).unwrap_or_default()
    }
}

fn cmd(exe: &Path, home: &Path, port: u16, envs: &[(String, String)]) -> Command {
    let mut c = Command::new(exe);
    c.env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", home)
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", home.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", home.join("rs-cron-state.json"))
        .env("HERMES_RS_CRON_OUTPUT", home.join("rs-cron-output"))
        // Live unit caps (70-research-budget.conf) and MCP off.
        .env("HERMES_RS_SESSION_USD_CAP", "5.00")
        .env("HERMES_RS_DAILY_USD_CAP", "7.00")
        .env("HERMES_RS_DAILY_CALL_CAP", "2000")
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_MOCK_DELAY_MS", "0")
        .env("API_SERVER_KEY", API);
    for (k, v) in envs {
        c.env(k, v);
    }
    c
}

fn start(exe: &Path, home: &Path, port: u16, envs: &[(String, String)]) -> Gw {
    let log = home.join("gateway.log");
    let f = fs::File::create(&log).unwrap();
    let ch = cmd(exe, home, port, envs)
        .arg("--mock")
        .stdout(Stdio::from(f.try_clone().unwrap()))
        .stderr(Stdio::from(f))
        .spawn()
        .expect("spawn");
    Gw(ch, log)
}

fn wait_up(port: u16) -> u128 {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if http(port, "GET", "/health", false, None).0 == 200 {
            return t0.elapsed().as_millis();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("gateway on :{port} not up in 20 s");
}

fn check_config(exe: &Path, home: &Path, port: u16, envs: &[(String, String)]) -> (i32, String) {
    let out = cmd(exe, home, port, envs)
        .arg("--check-config")
        .output()
        .expect("check-config");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

fn chat(port: u16, text: &str) -> (u16, String) {
    let body = json!({"model": "parity", "messages": [{"role": "user", "content": text}]});
    let (code, b) = http(port, "POST", "/v1/chat/completions", true, Some(&body));
    let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    (code, content)
}

/// Tool names the agent offers, seen from outside: the mock answers
/// `tool:<name>` with a tool call only when the tool is registered, and the
/// gateway then replies `done: ...`; otherwise `echo: ...`.
fn tools_seen(port: u16, args: &str) -> Vec<String> {
    let mut seen = vec![];
    for t in PROBE_TOOLS {
        let (code, c) = chat(port, &format!("tool:{t} {args}"));
        assert_eq!(code, 200, "chat for tool {t} on :{port}");
        if c.starts_with("done:") {
            seen.push(t.to_string());
        }
    }
    seen
}

fn fresh(root: &Path, name: &str) -> PathBuf {
    let p = root.join(name);
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

/// Volatile fields that legitimately differ between two runs.
fn scrub(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for k in [
                "id",
                "created",
                "uptime_s",
                "started_at",
                "ts",
                "now",
                "pid",
            ] {
                m.remove(k);
            }
            m.values_mut().for_each(scrub);
        }
        Value::Array(a) => a.iter_mut().for_each(scrub),
        _ => {}
    }
}

fn json_of(s: &str) -> Value {
    let mut v: Value = serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.into()));
    scrub(&mut v);
    v
}

#[test]
fn real_candidate_parity() {
    if !on("GALION_REAL") {
        eprintln!("SKIP: set GALION_REAL=1 for the real-life test");
        return;
    }
    let cand = cand_bin();
    let live = PathBuf::from(std::env::var("HERMES_GW_LIVE_BIN").expect("HERMES_GW_LIVE_BIN"));
    assert!(cand.is_file(), "no candidate at {cand:?}");
    assert!(live.is_file(), "no live binary at {live:?}");

    // P1 bytes: candidate = staged drop-in sha, live = the VPS bytes.
    let (cs, ls) = (sha256(&cand), sha256(&live));
    let csize = fs::metadata(&cand).unwrap().len();
    let lsize = fs::metadata(&live).unwrap().len();
    println!("P1 candidate {cand:?} {csize} B sha256 {cs}");
    println!("P1 live      {live:?} {lsize} B sha256 {ls}");
    assert!(csize <= 2 * 1024 * 1024, "candidate over the 2 MiB budget");
    if let Ok(want) = std::env::var("HERMES_GW_LIVE_SHA") {
        assert_eq!(ls, want, "live bytes are not the VPS bytes");
    }
    if on("HERMES_GW_CHECK_DROPIN_SHA") {
        let staged = DROPIN
            .lines()
            .find_map(|l| l.strip_prefix("# sha256 "))
            .unwrap();
        assert_eq!(cs, staged, "candidate is not the staged artifact");
        println!("P1 candidate sha == staged 95-gateway-candidate.conf sha");
    }

    let root = std::env::temp_dir().join(format!("t416-parity-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    // P2 --check-config with the live unit shape (coding off).
    let (lh, ch) = (fresh(&root, "live"), fresh(&root, "cand"));
    let (lc, lo) = check_config(&live, &lh, port(0), &[]);
    // Same port for both: --check-config never binds, it only prints it.
    let (cc, co) = check_config(&cand, &ch, port(0), &[]);
    let first = |s: &str| {
        s.lines()
            .next()
            .unwrap_or("")
            .replace(&*root.to_string_lossy(), "<root>")
    };
    println!(
        "P2 live --check-config exit {lc}: {:?} lines {}",
        first(&lo).replace("/live", "/H"),
        lo.lines().count()
    );
    println!(
        "P2 cand --check-config exit {cc}: {:?} lines {}",
        first(&co).replace("/cand", "/H"),
        co.lines().count()
    );
    assert_eq!((lc, cc), (0, 0));
    assert_eq!(
        first(&lo).replace("/live", "/H"),
        first(&co).replace("/cand", "/H"),
        "same parsed config"
    );
    assert!(co.lines().any(|l| l == "coding=off"), "{co}");
    // The folded slot-355 fix: a coding worktree under /var/lib must fail
    // the check on the candidate (live b7fbbc78 says ok = the blind spot).
    let bad = vec![
        (
            "HERMES_RS_CODING_WORKTREE".to_string(),
            "/var/lib/misc".to_string(),
        ),
        (
            "HERMES_RS_CODING_BRANCH".to_string(),
            "hermes/ticket".to_string(),
        ),
    ];
    let (lb, _) = check_config(&live, &lh, port(0), &bad);
    let (cb, cbo) = check_config(&cand, &ch, port(1), &bad);
    println!("P2 /var/lib worktree --check-config: live exit {lb}, candidate exit {cb}");
    assert_eq!(cb, 2, "{cbo}");
    assert!(cbo.contains("coding=error"), "{cbo}");

    // P3 HTTP parity, both started like the live unit (coding runtime OFF).
    let gl = start(&live, &lh, port(0), &[]);
    let gc = start(&cand, &ch, port(1), &[]);
    let (ul, uc) = (wait_up(port(0)), wait_up(port(1)));
    println!("P3 up: live {ul} ms, candidate {uc} ms");
    let routes: [(&str, &str, bool); 9] = [
        ("GET", "/health", false),
        ("GET", "/v1/health", false),
        ("GET", "/health/detailed", true),
        ("GET", "/v1/models", true),
        ("GET", "/v1/models", false),
        ("GET", "/v1/capabilities", true),
        ("GET", "/api/sessions", true),
        ("GET", "/api/jobs", true),
        ("GET", "/no/such/route", true),
    ];
    for (m, p, auth) in routes {
        let (a, ab) = http(port(0), m, p, auth, None);
        let (b, bb) = http(port(1), m, p, auth, None);
        println!("P3 {m} {p} auth={auth}: live {a} candidate {b}");
        assert_eq!(a, b, "{m} {p} status");
        assert_eq!(json_of(&ab), json_of(&bb), "{m} {p} body");
    }
    let (a, ac) = chat(port(0), "parity ping 416");
    let (b, bc) = chat(port(1), "parity ping 416");
    println!("P3 chat: live {a} {ac:?}, candidate {b} {bc:?}");
    assert_eq!((a, &ac), (b, &bc));
    assert_eq!(ac, "echo: parity ping 416");

    // P4 tools list, coding off: what a client can reach must be identical.
    let tl = tools_seen(port(0), "{}");
    let tc = tools_seen(port(1), "{}");
    println!("P4 tools (coding off): live {tl:?}, candidate {tc:?}");
    assert_eq!(tl, tc);
    assert!(
        tc.iter().all(|t| t == "clock" || t == "delegate_task"),
        "{tc:?}"
    );
    let (ll, lcl) = (gl.log(), gc.log());
    for (who, log) in [("live", &ll), ("candidate", &lcl)] {
        assert!(log.contains("listening on 127.0.0.1:"), "{who}: {log}");
        assert!(
            !log.contains("coding: tools on"),
            "{who}: coding must stay off: {log}"
        );
    }
    drop((gl, gc));
    assert_eq!(http(port(0), "GET", "/health", false, None).0, 0);
    assert_eq!(http(port(1), "GET", "/health", false, None).0, 0);

    // P5 sandboxed preflight + one sandboxed tool run (VPS only).
    if !on("HERMES_PARITY_SANDBOX") {
        println!("P5 SKIP: sandboxed preflight runs on the VPS only (HERMES_PARITY_SANDBOX=1)");
    } else {
        assert!(
            !Path::new("/workspace/swarm/RULES.md").exists(),
            "sandbox_vps_only: refusing the sandboxed preflight on the box"
        );
        let mut results = vec![];
        for (who, exe, port) in [("live", &live, port(2)), ("candidate", &cand, port(3))] {
            let home = fresh(&root, &format!("{who}-coding"));
            let wt = fresh(&root, &format!("{who}-wt"));
            fs::write(wt.join("README.md"), "parity 416\n").unwrap();
            let git = |args: &[&str]| {
                let ok = Command::new("git")
                    .args(args)
                    .current_dir(&wt)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                assert!(ok, "git {args:?}");
            };
            git(&["init", "-q", "-b", "hermes/ticket"]);
            git(&[
                "-c",
                "user.name=t416",
                "-c",
                "user.email=t416@invalid",
                "add",
                "README.md",
            ]);
            git(&[
                "-c",
                "user.name=t416",
                "-c",
                "user.email=t416@invalid",
                "commit",
                "-qm",
                "parity",
            ]);
            let envs = vec![
                (
                    "HERMES_RS_CODING_WORKTREE".to_string(),
                    wt.display().to_string(),
                ),
                (
                    "HERMES_RS_CODING_BRANCH".to_string(),
                    "hermes/ticket".to_string(),
                ),
                (
                    "HERMES_RS_CODING_STATE".to_string(),
                    home.join("coding-state").display().to_string(),
                ),
                // VPS host has no Landlock (PVE kernel): same override the owner would need.
                (
                    "HERMES_RS_CODING_ALLOW_NO_LANDLOCK".to_string(),
                    "1".to_string(),
                ),
            ];
            let (cc, co) = check_config(exe, &home, port, &envs);
            let g = start(exe, &home, port, &envs);
            let up = wait_up(port);
            let tools = tools_seen(port, r#"{"path":"README.md"}"#);
            let (_, read) = chat(port, r#"tool:fs_read {"path":"README.md"}"#);
            let log = g.log();
            let on_line = log
                .lines()
                .find(|l| l.contains("coding: tools on"))
                .map(|l| {
                    l.split_once("coding:")
                        .map(|x| x.1)
                        .unwrap_or(l)
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();
            let notes: Vec<String> = log
                .lines()
                .filter(|l| l.contains("coding: ") && !l.contains("tools on"))
                .map(|l| {
                    l.split_once("coding: ")
                        .map(|x| x.1)
                        .unwrap_or(l)
                        .trim()
                        .to_string()
                        // Paths differ only by the per-binary dir prefix.
                        .replace(&format!("/{who}-"), "/<bin>-")
                })
                .collect();
            println!(
                "P5 {who}: --check-config exit {cc} coding line {:?}; up {up} ms; {on_line}; tools reached {tools:?}; fs_read -> {:?}; notes {notes:?}",
                co.lines().find(|l| l.starts_with("coding")),
                read.chars().take(80).collect::<String>()
            );
            assert!(on_line.starts_with("tools on"), "{who}: {log}");
            assert!(
                read.contains("parity 416"),
                "{who}: sandboxed fs_read: {read}"
            );
            results.push((on_line, tools, notes));
            drop(g);
        }
        assert_eq!(results[0].0, results[1].0, "same coding tool list");
        assert_eq!(results[0].1, results[1].1, "same reachable tools");
        assert_eq!(results[0].2, results[1].2, "same preflight notes");
    }
    let _ = fs::remove_dir_all(&root);
    for p in port(0)..=port(3) {
        assert_eq!(
            http(p, "GET", "/health", false, None).0,
            0,
            "port {p} left open"
        );
    }
}
