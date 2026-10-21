// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life shadow-mode test (GALION_REAL=1): the real `galion-guard`
//! binary with `backend = "shadow"`, file source, driven through its CLI and
//! control socket. Proves: bans are detected and logged in fail2ban format,
//! the firewall is never touched (the configured `nft` path does not exist,
//! so any attempt would show as a firewall error), protected addresses are
//! never written, `teardown` is a no-op, and `compare` gives parity / exit 3
//! against a fail2ban-layout log.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-guard");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(BIN).args(args).output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn cli(cfg: &Path, args: &[&str]) -> (bool, String) {
    let mut v: Vec<&str> = args.to_vec();
    let c = cfg.to_str().unwrap();
    v.extend(["--config", c]);
    let (code, out, _) = run(&v);
    (code == 0, out)
}

fn status(cfg: &Path) -> serde_json::Value {
    let (_, out) = cli(cfg, &["status", "--json"]);
    serde_json::from_str(out.trim()).unwrap_or(serde_json::Value::Null)
}

fn start(cfg: &Path, sock: &Path, log: &Path) -> Child {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    let mut c = Command::new(BIN)
        .args(["run", "--config"])
        .arg(cfg)
        .stdout(Stdio::null())
        .stderr(f)
        .spawn()
        .unwrap();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if sock.exists() && status(cfg).is_object() {
            return c;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = c.kill();
    let _ = c.wait();
    panic!("daemon did not start");
}

fn wait_for(cfg: &Path, secs: u64, f: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let t = Instant::now();
    loop {
        let s = status(cfg);
        if f(&s) || t.elapsed() > Duration::from_secs(secs) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ts() -> String {
    common::stamp(galion_guard::state::now_secs())
}

fn say(msg: &str) {
    eprintln!("[{}] {msg}", zagreb_time::now_rfc3339());
}

/// Rewrite our shadow log as fail2ban would have written it on the VPS:
/// `fail2ban.*` loggers and UTC stamps (the VPS zone).
fn as_vps_fail2ban(shadow: &str) -> String {
    use galion_guard::f2blog::{fmt_time, parse_time, Zone};
    let mut out = String::new();
    for l in shadow.lines() {
        if l.contains("galion-guard.shadow") {
            continue; // fail2ban has no such lines
        }
        let Some(t) = parse_time(&l[..19], Zone::Zagreb) else {
            continue;
        };
        let mut rest = l[23..].replacen("galion-guard.", "fail2ban.", 1);
        // fail2ban pads the logger to 24 columns; ours is 4 longer.
        rest = rest.replacen("    [", "[", 1);
        // Re-render embedded stamps ("Found ip - <t>", "-> <t>)").
        for sep in [" - ", " -> "] {
            if let Some(i) = rest.find(sep) {
                let s = &rest[i + sep.len()..i + sep.len() + 19];
                if let Some(x) = parse_time(s, Zone::Zagreb) {
                    rest = format!(
                        "{}{}{}",
                        &rest[..i + sep.len()],
                        fmt_time(x, Zone::Utc),
                        &rest[i + sep.len() + 19..]
                    );
                }
            }
        }
        out += &format!("{},500{rest}\n", fmt_time(t, Zone::Utc));
    }
    out
}

#[test]
fn real_shadow_mode() {
    if !real() {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let dir: PathBuf =
        std::env::temp_dir().join(format!("guard-real-shadow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("guard.toml");
    let sock = dir.join("run/ctl.sock");
    let log = dir.join("auth.log");
    let dlog = dir.join("daemon.log");
    let shadow = dir.join("shadow.log");
    let canary = dir.join("nft-must-never-run");
    std::fs::write(&log, "").unwrap();
    std::fs::write(
        &cfg,
        format!(
            "[guard]\nbackend = \"shadow\"\nnft = \"{c}\"\nstate = \"{d}/state.json\"\nsocket = \"{s}\"\nignoreself = false\n[sshd]\nsource = \"file\"\nlogpath = \"{l}\"\npoll_ms = 100\nmaxretry = 3\nfindtime = \"60s\"\nbantime = \"4s\"\nprotect_accepted = \"0\"\nignoreip = [\"192.0.2.7\"]\nports = [19930]\n",
            c = canary.display(),
            d = dir.display(),
            s = sock.display(),
            l = log.display()
        ),
    )
    .unwrap();
    let (ok, out) = cli(&cfg, &["check-config"]);
    say(&format!("check-config: {}", out.trim()));
    assert!(ok && out.contains("backend=shadow"), "{out}");

    let mut d = start(&cfg, &sock, &dlog);
    let s = status(&cfg);
    say(&format!(
        "started: backend={} ready={} shadow={}",
        s["backend"], s["backend_ready"], s["shadow"]
    ));
    assert_eq!(s["backend"], "shadow");
    assert_eq!(s["backend_ready"], true);
    assert_eq!(s["shadow"]["firewall_writes"], 0);

    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    for i in 0..3 {
        writeln!(
            f,
            "{} box sshd[{}]: Invalid user u{i} from 203.0.113.66 port 4{i}",
            ts(),
            400 + i
        )
        .unwrap();
        // The allowlisted admin fails too: never banned, never written.
        writeln!(
            f,
            "{} box sshd[{}]: Invalid user a{i} from 192.0.2.7 port 5{i}",
            ts(),
            500 + i
        )
        .unwrap();
    }
    f.flush().unwrap();
    let s = wait_for(&cfg, 5, |s| s["currently_banned"] == 1);
    say(&format!(
        "after 3 failures: currently_banned={} total_failed={} protected_hits={}",
        s["currently_banned"], s["total_failed"], s["protected_hits"]
    ));
    assert_eq!(s["bans"][0]["ip"], "203.0.113.66", "{s}");
    assert_eq!(s["protected_hits"], 3);
    let (_, text) = cli(&cfg, &["status"]);
    assert!(
        text.contains("SHADOW (log-only, no firewall writes)"),
        "{text}"
    );
    say("status text shows SHADOW mode line");

    // Manual ban/unban work in memory; protected refusal still applies.
    assert!(cli(&cfg, &["ban", "203.0.113.67", "1m"]).0);
    assert!(!cli(&cfg, &["ban", "192.0.2.7"]).0);
    assert!(cli(&cfg, &["unban", "203.0.113.67"]).0);
    // Teardown never runs nft in shadow mode.
    let (ok, out) = cli(&cfg, &["teardown"]);
    say(&format!("teardown: {}", out.trim()));
    assert!(ok && out.contains("nothing to remove"), "{out}");

    // Expiry (bantime 4 s) is logged as Unban.
    let s = wait_for(&cfg, 8, |s| s["currently_banned"] == 0);
    assert_eq!(s["currently_banned"], 0, "{s}");
    assert!(cli(&cfg, &["stop"]).0);
    let _ = d.wait();

    let dl = std::fs::read_to_string(&dlog).unwrap();
    let sh = std::fs::read_to_string(&shadow).unwrap();
    say(&format!("shadow log ({} lines):", sh.lines().count()));
    for l in sh.lines() {
        eprintln!("   | {l}");
    }
    assert!(dl.contains("SHADOW MODE"), "{dl}");
    assert!(!dl.contains("firewall error"), "nft was attempted: {dl}");
    assert!(!canary.exists());
    assert!(!sh.contains("192.0.2.7") && !dl.contains("192.0.2.7"));
    assert_eq!(sh.matches("[sshd] Found 203.0.113.66").count(), 3);
    assert_eq!(sh.matches("NOTICE  [sshd] Ban 203.0.113.66").count(), 1);
    assert_eq!(sh.matches("NOTICE  [sshd] Unban 203.0.113.66").count(), 1);
    assert_eq!(sh.matches("<protected address withheld>").count(), 3);
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&shadow).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // compare: fail2ban-layout UTC copy -> parity (exit 0) ...
    let f2b = dir.join("fail2ban.log");
    std::fs::write(&f2b, as_vps_fail2ban(&sh)).unwrap();
    let (code, out, _) = run(&[
        "compare",
        f2b.to_str().unwrap(),
        shadow.to_str().unwrap(),
        "--timeline",
    ]);
    say(&format!(
        "compare (same events, UTC vs Zagreb) exit {code}:"
    ));
    for l in out.lines() {
        eprintln!("   | {l}");
    }
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("verdict: PARITY"));
    assert!(
        out.contains("failures (Found)                        3              3"),
        "{out}"
    );
    // ... and a fail2ban log missing the ban -> exit 3, ONLY GUARD.
    let cut: String = std::fs::read_to_string(&f2b)
        .unwrap()
        .lines()
        .filter(|l| !l.contains("] Ban 203.0.113.66"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&f2b, cut).unwrap();
    let (code, out, _) = run(&[
        "compare",
        f2b.to_str().unwrap(),
        shadow.to_str().unwrap(),
        "--json",
    ]);
    let j: serde_json::Value = serde_json::from_str(&out).unwrap();
    say(&format!(
        "compare (fail2ban ban removed) exit {code}: parity={} only_guard={}",
        j["parity"], j["bans_only_guard"]
    ));
    assert_eq!(code, 3);
    assert_eq!(j["bans_only_guard"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}
