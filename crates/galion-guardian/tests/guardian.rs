// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use galion_guardian::config::{self, Spec};
use galion_guardian::probe::{self, Env};
use galion_guardian::store::{incident_id, JsonlStore, Store};
use galion_guardian::{beat, tool_warn_format, write_tool_warn};
use openmind_survival::Runner;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

struct Fake;
impl Runner for Fake {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        match (program, args.first().copied()) {
            ("systemctl", Some("is-active")) => Ok("inactive\n".into()),
            ("df", Some("-P")) => Ok("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/x 100 90 10 90% /\n".into()),
            _ => Err(format!("unexpected {program} {args:?}")),
        }
    }
}

fn out(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/galion-guardian")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fake_proc(dir: &Path, comm: Option<&str>) -> PathBuf {
    let p = dir.join("proc");
    std::fs::create_dir_all(p.join("42")).unwrap();
    std::fs::write(p.join("42/comm"), format!("{}\n", comm.unwrap_or("bash"))).unwrap();
    std::fs::write(p.join("meminfo"), "MemTotal: 100 kB\nMemAvailable: 50 kB\n").unwrap();
    std::fs::write(p.join("loadavg"), "0.10 0.10 0.10 1/1 1\n").unwrap();
    p
}

/// Mock HTTP server answering a fixed sequence, one connection each.
fn serve(replies: Vec<&'static str>) -> (u16, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        for r in replies {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            s.write_all(r.as_bytes()).unwrap();
        }
    });
    (port, h)
}

#[test]
fn parses_staged_config() {
    let t = include_str!("../../../deploy/staged/guardian.toml");
    let f = config::parse(t).unwrap();
    assert!(f.probe.len() >= 8);
    assert_eq!(f.guardian.interval_s, 60);
    assert!(config::parse("[guardian]\ninterval_s=1\n[[probe]]\nkind=\"mem\"\n").is_err());
    assert!(config::parse("[guardian]\nstore=\"x\"\n[[probe]]\nkind=\"mem\"\n").is_err());
    assert!(config::parse("[[probe]]\nkind=\"nope\"\n").is_err());
    assert!(config::parse("").is_err());
}

#[test]
fn probe_statuses() {
    let d = out("statuses");
    let proc_root = fake_proc(&d, Some("cloudflared"));
    let env = Env {
        runner: &Fake,
        proc_root: &proc_root,
        has_systemd: true,
    };
    let (port, h) = serve(vec![
        "HTTP/1.0 401 Unauthorized\r\n\r\n",
        "HTTP/1.0 500 Oops\r\n\r\n",
        "HTTP/1.0 503 X\r\n\r\n{\"loading\":true}",
    ]);
    let http = |codes: Vec<u16>, loading_ok| Spec::Http {
        name: "svc".into(),
        host: "127.0.0.1".into(),
        port,
        path: "/health".into(),
        ok_codes: codes,
        loading_ok,
        timeout_ms: 2500,
    };
    assert_eq!(
        probe::run(&http(vec![200, 401], false), &env).status,
        "healthy"
    );
    let r = probe::run(&http(vec![200], false), &env);
    assert_eq!(
        (r.status, r.error.as_deref()),
        ("unhealthy", Some("http 500"))
    );
    let r = probe::run(&http(vec![200], true), &env);
    assert_eq!((r.status, r.detail.as_str()), ("healthy", "loading"));
    h.join().unwrap();
    assert_eq!(probe::run(&http(vec![200], false), &env).status, "critical");
    let unit = Spec::Unit {
        name: None,
        unit: "x".into(),
    };
    assert_eq!(probe::run(&unit, &env).status, "critical");
    let nosd = Env {
        runner: &Fake,
        proc_root: &proc_root,
        has_systemd: false,
    };
    let r = probe::run(&unit, &nosd);
    assert_eq!(
        (r.status, r.detail.as_str()),
        ("healthy", "skipped: no systemd")
    );
    let disk = Spec::Disk {
        path: "/".into(),
        warn: 85.0,
        crit: 95.0,
    };
    assert_eq!(probe::run(&disk, &env).status, "unhealthy");
    assert_eq!(probe::run(&Spec::Mem, &env).status, "healthy");
    assert_eq!(probe::run(&Spec::Load, &env).status, "healthy");
    let tun = Spec::Process {
        name: None,
        comm: "cloudflared".into(),
    };
    let r = probe::run(&tun, &env);
    assert_eq!((r.name.as_str(), r.status), ("tunnel", "healthy"));
    let none = fake_proc(&out("statuses2"), None);
    let env2 = Env {
        runner: &Fake,
        proc_root: &none,
        has_systemd: true,
    };
    let r = probe::run(&tun, &env2);
    assert_eq!(
        (r.status, r.detail.as_str()),
        ("critical", "cloudflared not running")
    );
}

#[test]
fn beat_incidents_streaks_sinks() {
    let d = out("beat");
    let proc_root = fake_proc(&d, None);
    let env = Env {
        runner: &Fake,
        proc_root: &proc_root,
        has_systemd: true,
    };
    let text = format!(
        "[guardian]\nstate_dir = \"{0}/state\"\nactivity = \"{0}/activity.jsonl\"\nnerve = \"{0}/nerve.jsonl\"\ndigest_text = \"Standup {{day}}\"\nbeat = \"{0}/last-guardian.ts\"\n[[probe]]\nkind = \"process\"\ncomm = \"cloudflared\"\n[[probe]]\nkind = \"mem\"\n",
        d.display()
    );
    let f = config::parse(&text).unwrap();
    let mut st = JsonlStore {
        dir: f.guardian.state_dir.clone(),
        keep_rows: 4000,
        keep_resolved: 500,
    };
    let mut last = None;
    for _ in 0..4 {
        last = Some(beat(&f.guardian, &f.probe, &env, &mut st).unwrap());
    }
    let b = last.unwrap();
    assert!(b.line().contains(" down tunnel"), "{}", b.line());
    assert!(b.stamp.ends_with("+02:00") || b.stamp.ends_with("+01:00"));
    assert_eq!(st.open_incidents().unwrap(), vec!["tunnel".to_string()]);
    let inc = st.incidents();
    assert_eq!(inc.len(), 1);
    assert_eq!(inc[0].severity, "high");
    assert_eq!(b.suggestions[0].level.as_str(), "suggest-restart");
    let health = std::fs::read_to_string(d.join("state/health.jsonl")).unwrap();
    assert_eq!(health.lines().count(), 8);
    // activity: one health row (900 s gate) + one digest row
    let act = std::fs::read_to_string(d.join("activity.jsonl")).unwrap();
    assert_eq!(
        act.lines()
            .filter(|l| l.contains("\"health fail tunnel\""))
            .count(),
        1
    );
    assert_eq!(act.lines().filter(|l| l.contains("\"digest\"")).count(), 1);
    // nerve: only the opening change
    let nerve = std::fs::read_to_string(d.join("nerve.jsonl")).unwrap();
    assert_eq!(nerve.lines().count(), 1);
    assert!(nerve.contains("\"name\":\"tunnel\",\"ok\":false"));
    assert!(d.join("state/beat").exists() && d.join("last-guardian.ts").exists());
    // tunnel comes back -> resolved, nerve line, streak reset
    std::fs::write(proc_root.join("42/comm"), "cloudflared\n").unwrap();
    let b = beat(&f.guardian, &f.probe, &env, &mut st).unwrap();
    assert!(b.line().contains(" watch"));
    assert!(b.events[0].service == "tunnel" && !b.events[0].opened);
    assert!(st.open_incidents().unwrap().is_empty());
    assert_eq!(
        st.incidents()[0].resolution.as_deref(),
        Some("beat healthy")
    );
    assert_eq!(
        std::fs::read_to_string(d.join("nerve.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn prune_rows() {
    let d = out("prune");
    let mut st = JsonlStore {
        dir: d.clone(),
        keep_rows: 10,
        keep_resolved: 2,
    };
    let row = galion_guardian::probe::Row {
        name: "a".into(),
        status: "healthy",
        ms: 1.0,
        error: None,
        detail: "ok".into(),
    };
    for _ in 0..30 {
        st.record(std::slice::from_ref(&row), "2026-10-18T21:00:00+02:00")
            .unwrap();
    }
    let n = std::fs::read_to_string(d.join("health.jsonl"))
        .unwrap()
        .lines()
        .count();
    assert!((10..=11).contains(&n), "{n}");
}

#[test]
fn ids_and_tool_warn() {
    assert_eq!(
        incident_id("2026-10-18T21:05:07+02:00", "whisper"),
        "20261018T210507whis"
    );
    let l = tool_warn_format(
        7,
        Some("terminal"),
        0.0,
        "x [Tool loop warning: same_call; count=3 ] y",
    )
    .unwrap();
    assert!(
        l.starts_with(
            "WARN id=7 code=same_call count=3 tool=terminal when=1970-01-01T01:00:00+01:00"
        ),
        "{l}"
    );
    assert!(tool_warn_format(1, None, 0.0, "no marker").is_none());
    let d = out("toolwarn");
    let p = d.join("tool-warn.txt");
    assert!(write_tool_warn(&p, &l).unwrap());
    assert!(!write_tool_warn(&p, &l).unwrap());
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_store_parity() {
    use galion_guardian::store::sqlite::SqliteStore;
    let d = out("sqlite");
    let mut st = SqliteStore::open(&d.join("guardian.db"), 4000).unwrap();
    let bad = galion_guardian::probe::Row {
        name: "voice".into(),
        status: "critical",
        ms: 2.0,
        error: Some("connect".into()),
        detail: "connect".into(),
    };
    let ev = st
        .record(std::slice::from_ref(&bad), "2026-10-18T21:00:00+02:00")
        .unwrap();
    assert!(ev[0].opened);
    assert!(st
        .record(std::slice::from_ref(&bad), "2026-10-18T21:01:00+02:00")
        .unwrap()
        .is_empty());
    assert_eq!(st.open_incidents().unwrap(), vec!["voice".to_string()]);
    let good = galion_guardian::probe::Row {
        status: "healthy",
        error: None,
        ..bad
    };
    let ev = st.record(&[good], "2026-10-18T21:02:00+02:00").unwrap();
    assert!(!ev[0].opened);
    assert!(st.open_incidents().unwrap().is_empty());
    // The query Galion One runs keeps working.
    let s: String = st.conn.query_row(
        "SELECT status FROM health_checks WHERE id IN (SELECT MAX(id) FROM health_checks GROUP BY service)",
        [], |r| r.get(0)).unwrap();
    assert_eq!(s, "healthy");
}
