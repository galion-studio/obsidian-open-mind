// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! t460u (nerve-mcp-connectors-20261030): the nerve watches the Hermes MCP
//! connectors, read-only, with no model call and no token read.
//!
//! One release nerve (polling, 5 s) on a box port, pointed at a temp
//! `mcp-servers.yaml`, fake token files, a fake `/proc` tree and fake
//! `systemctl` / `journalctl` executables written here:
//! A) alpha (stdio, child alive, "12 tools") = healthy, beta (token file
//!    missing) = waiting, gamma (NOT CONNECTED, never healthy) = failing
//!    without an alert, delta (enabled: false) absent. `/api/v1/connectors`,
//!    `/state` `connectors` + `system.connectors` agree; no warn alert.
//! B) alpha's process goes away: alpha = failing, one warn incident
//!    `mcp-connector:alpha`, exactly one deduped alert over three cycles,
//!    score drops by the warn cost; beta still raises nothing.
//! C) no token byte and no address appears in any served body.
//! D) negative control: without `NERVE_MCP_SERVERS_FILE` the route is 404
//!    and `/state` has no `connectors`.
//!
//! Gated: `GALION_REAL=1`. Ports 19480-19489.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve \
//!   --test real_mcp_connectors -- --nocapture --test-threads=1
//! ```

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 19480;
const SECRET: &str = "ghp_RealTestNeverServeThisValue0123456789abcd";
const HERMES_PID: u32 = 4242;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn http(port: u16, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(2),
    ) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    );
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, body)
}

fn get_json(port: u16, path: &str) -> (Value, String) {
    let (c, b) = http(port, path);
    assert_eq!(c, 200, "{path}: {b}");
    (
        serde_json::from_str(&b).unwrap_or_else(|e| panic!("{path}: {e}: {b}")),
        b,
    )
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exe(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Temp world: servers file, tokens, fake proc, fake systemctl/journalctl.
fn world() -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-mcp-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for s in ["data", "mcp.d", "bin", "proc"] {
        std::fs::create_dir_all(d.join(s)).unwrap();
    }
    std::fs::write(d.join("components.json"), "[]").unwrap();
    let t = d.join("mcp.d");
    for n in ["alpha", "gamma"] {
        std::fs::write(
            t.join(format!("{n}.env")),
            format!("GITHUB_TOKEN={SECRET}\n"),
        )
        .unwrap();
        std::fs::set_permissions(
            t.join(format!("{n}.env")),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        // Saved before the (fake) Hermes start, so no "restart to use" note.
        std::fs::File::options()
            .write(true)
            .open(t.join(format!("{n}.env")))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }
    let alpha = d.join("bin/alpha-mcp");
    exe(&alpha, "#!/bin/sh\nexit 0\n");
    let yaml = format!(
        "# test servers\nmcp_servers:\n  alpha:\n    command: {a}\n    env_file: {t}/alpha.env\n    lazy: false\n  beta:\n    url: http://127.0.0.1:1/mcp\n    env_file: {t}/beta.env\n    lazy: true\n  gamma:\n    command: {a}\n    env_file: {t}/gamma.env\n    lazy: true\n  delta:\n    command: {a}\n    enabled: false\n",
        a = alpha.display(),
        t = t.display()
    );
    std::fs::write(d.join("mcp-servers.yaml"), yaml).unwrap();
    // Hermes started 60 s ago (past a 5 s start grace).
    let started = now() - 60;
    std::fs::write(
        d.join("show.txt"),
        format!("MainPID={HERMES_PID}\nActiveState=active\nExecMainStartTimestamp=@{started}\n"),
    )
    .unwrap();
    std::fs::write(
        d.join("journal.txt"),
        format!(
            "{s}.100000 box hermes-rs[{HERMES_PID}]: mcp: alpha: 12 tools (3 write tools gated off)\n\
             {s}.200000 box hermes-rs[{HERMES_PID}]: mcp: beta: NOT CONNECTED: env_file {t}/beta.env: No such file or directory (os error 2)\n\
             {s}.300000 box hermes-rs[{HERMES_PID}]: mcp: gamma: NOT CONNECTED: handshake failed Authorization: Bearer {SECRET} from 203.0.113.9\n\
             {s}.400000 box hermes-rs[{HERMES_PID}]: mcp: 12 tools\n",
            s = started + 1,
            t = t.display()
        ),
    )
    .unwrap();
    exe(
        &d.join("bin/systemctl"),
        &format!("#!/bin/sh\ncat {}\n", d.join("show.txt").display()),
    );
    exe(
        &d.join("bin/journalctl"),
        &format!("#!/bin/sh\ncat {}\n", d.join("journal.txt").display()),
    );
    // /proc/<hermes>/task/<hermes>/children -> 777 whose exe is alpha-mcp.
    let task = d.join(format!("proc/{HERMES_PID}/task/{HERMES_PID}"));
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(task.join("children"), "777 ").unwrap();
    let kid = d.join("proc/777");
    std::fs::create_dir_all(&kid).unwrap();
    std::os::unix::fs::symlink(&alpha, kid.join("exe")).unwrap();
    std::fs::write(kid.join("cmdline"), format!("{}\0", alpha.display())).unwrap();
    d
}

fn cmd(dir: &Path, port: u16, on: bool) -> Command {
    let mut c = Command::new(BIN);
    c.args(["serve", "--listen", &format!("127.0.0.1:{port}")])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env(
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
        )
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_EDGE_HOSTS", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if on {
        c.env("NERVE_MCP_SERVERS_FILE", dir.join("mcp-servers.yaml"))
            .env("NERVE_MCP_SYSTEMCTL", dir.join("bin/systemctl"))
            .env("NERVE_MCP_JOURNALCTL", dir.join("bin/journalctl"))
            .env("NERVE_MCP_PROC", dir.join("proc"))
            .env("NERVE_MCP_TTL_S", "1")
            .env("NERVE_MCP_START_GRACE_S", "5");
    }
    c
}

fn start(c: &mut Command, port: u16) -> Kill {
    let n = Kill(c.spawn().expect("spawn nerve"));
    let t = Instant::now();
    loop {
        let (code, b) = http(port, "/health");
        if code == 200
            && serde_json::from_str::<Value>(&b).is_ok_and(|v| v["cycle"].as_u64() >= Some(1))
        {
            return n;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "nerve on {port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_cycle(port: u16, after: u64) -> u64 {
    let t = Instant::now();
    loop {
        let (h, _) = get_json(port, "/health");
        let c = h["cycle"].as_u64().unwrap_or(0);
        if c > after {
            return c;
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "no cycle after {after}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn by_name<'a>(t: &'a Value, n: &str) -> &'a Value {
    t["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == n)
        .unwrap_or_else(|| panic!("{n} missing: {t}"))
}

fn mcp_alerts(port: u16) -> Vec<Value> {
    let (a, _) = get_json(port, "/api/v1/alerts");
    a.as_array()
        .unwrap()
        .iter()
        .filter(|x| x.to_string().contains("mcp-connector"))
        .cloned()
        .collect()
}

fn no_leak(what: &str, body: &str) {
    assert!(!body.contains(SECRET), "{what} leaks the token");
    assert!(!body.contains("RealTestNever"), "{what} leaks token bytes");
    assert!(!body.contains("203.0.113.9"), "{what} leaks an address");
}

#[test]
fn real_mcp_connectors() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = world();
    // D) negative control first: off by default.
    {
        let _n = start(&mut cmd(&dir, PORT + 1, false), PORT + 1);
        let (c, b) = http(PORT + 1, "/api/v1/connectors");
        assert_eq!(c, 404, "{b}");
        let (s, _) = get_json(PORT + 1, "/state");
        assert!(s["connectors"].is_null() && s["system"]["connectors"].is_null());
        println!(
            "== {} D off: /api/v1/connectors 404, no state keys",
            stamp()
        );
    }
    let _n = start(&mut cmd(&dir, PORT, true), PORT);
    let c1 = wait_cycle(PORT, 1);
    // A) states
    let (t, tb) = get_json(PORT, "/api/v1/connectors");
    no_leak("/api/v1/connectors", &tb);
    println!("== {} A table: {}", stamp(), t["summary"]);
    for s in t["servers"].as_array().unwrap() {
        println!(
            "   {} {} tools={} error={} note={}",
            s["name"], s["state"], s["tools"], s["last_error"], s["note"]
        );
    }
    assert_eq!(t["enabled"], true);
    assert_eq!(
        t["servers"].as_array().unwrap().len(),
        3,
        "delta is disabled"
    );
    let a = by_name(&t, "alpha");
    assert_eq!(a["state"], "healthy", "{a}");
    assert_eq!(a["tools"], 12);
    assert_eq!(by_name(&t, "beta")["state"], "waiting");
    let g = by_name(&t, "gamma");
    assert_eq!(g["state"], "failing", "{g}");
    assert!(g["last_error"]
        .as_str()
        .unwrap()
        .contains("handshake failed"));
    assert_eq!(t["summary"]["alerting"], 0);
    assert!(a["note"].is_null(), "{a}");
    let (s, sb) = get_json(PORT, "/state");
    no_leak("/state", &sb);
    assert_eq!(s["system"]["connectors"]["servers"], 3, "{}", s["system"]);
    assert_eq!(s["system"]["connectors"]["detail"], "/api/v1/connectors");
    assert_eq!(s["connectors"]["servers"].as_array().unwrap().len(), 3);
    assert!(
        mcp_alerts(PORT).is_empty(),
        "waiting/never-healthy must not alert"
    );
    let (h, _) = get_json(PORT, "/health");
    let score_a = h["score"].as_f64().unwrap();
    println!(
        "== {} A score {score_a}, no mcp alert (cycle {c1})",
        stamp()
    );
    // B) alpha's process goes away.
    std::fs::write(
        dir.join(format!("proc/{HERMES_PID}/task/{HERMES_PID}/children")),
        "",
    )
    .unwrap();
    let mut c = wait_cycle(PORT, c1);
    c = wait_cycle(PORT, c);
    let (t, tb) = get_json(PORT, "/api/v1/connectors");
    no_leak("/api/v1/connectors B", &tb);
    let a = by_name(&t, "alpha");
    assert_eq!(a["state"], "failing", "{a}");
    assert_eq!(t["summary"]["alerting"], 1);
    let al = mcp_alerts(PORT);
    assert_eq!(al.len(), 1, "{al:?}");
    assert_eq!(al[0]["severity"], "warn");
    assert!(al[0]["key"].as_str().unwrap().ends_with(":warn"));
    println!("== {} B alert: {}", stamp(), al[0]);
    for _ in 0..3 {
        c = wait_cycle(PORT, c);
    }
    let al = mcp_alerts(PORT);
    assert_eq!(al.len(), 1, "deduped over three more cycles: {al:?}");
    let (h, hb) = get_json(PORT, "/health");
    no_leak("/health", &hb);
    let score_b = h["score"].as_f64().unwrap();
    assert!(score_b <= score_a, "{score_a} -> {score_b}");
    let (se, seb) = get_json(PORT, "/self-eval");
    no_leak("/self-eval", &seb);
    let d: Vec<&Value> = se["deductions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d.to_string().contains("mcp-connector:alpha"))
        .collect();
    assert_eq!(d.len(), 1, "one deduction for the alpha incident: {d:?}");
    assert_eq!(d[0]["points"], 5, "warn incident costs 5: {}", d[0]);
    assert!(
        !se["deductions"].to_string().contains("beta"),
        "waiting costs nothing"
    );
    println!("== {} B deduction: {}", stamp(), d[0]["what"]);
    let (s, sb) = get_json(PORT, "/state");
    no_leak("/state B", &sb);
    assert_eq!(s["system"]["connectors"]["alerting"], 1);
    let (alb_c, alb) = http(PORT, "/api/v1/alerts");
    assert_eq!(alb_c, 200);
    no_leak("/api/v1/alerts", &alb);
    println!(
        "== {} B score {score_a} -> {score_b}, one alert after cycle {c}",
        stamp()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
