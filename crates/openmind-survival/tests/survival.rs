// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use openmind_survival::incident::{apply, Event, Status};
use openmind_survival::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

struct Fake(Result<String, String>);
impl Runner for Fake {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        assert!(program == "systemctl" || program == "df");
        assert!(!args.is_empty());
        self.0.clone()
    }
}

fn out_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test-out/survival");
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn http_probe_mock() {
    // Ephemeral port: a fixed 18080 collides with any other run on the box.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        for code in ["200 OK", "503 Service Unavailable"] {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            write!(s, "HTTP/1.0 {code}\r\nContent-Length: 0\r\n\r\n").unwrap();
        }
    });
    let a = check_http("mock", port, "/health");
    assert!(a.ok, "{a:?}");
    assert_eq!(a.detail, "status 200");
    let b = check_http("mock", port, "/health");
    assert!(!b.ok);
    assert_eq!(b.detail, "status 503");
    h.join().unwrap();
    let c = check_http("closed", port, "/");
    assert!(!c.ok);
}

#[test]
fn proc_parsers() {
    let mem = include_str!("fixtures/meminfo.txt");
    let p = parse_meminfo(mem).unwrap();
    assert!((p - 50.0).abs() < 0.01);
    assert!(check_meminfo(mem).ok);
    assert!(!check_meminfo("garbage").ok);
    let hi = "MemTotal: 100 kB\nMemAvailable: 10 kB\n";
    assert!(check_meminfo(hi).detail.contains("warn"));
    let crit = "MemTotal: 100 kB\nMemAvailable: 2 kB\n";
    assert!(check_meminfo(crit).detail.contains("crit"));
    let la = include_str!("fixtures/loadavg.txt");
    assert_eq!(parse_loadavg(la), Some((0.5, 0.25, 0.1)));
    assert!(check_loadavg(la, 4).ok);
    assert!(!check_loadavg("9.0 1 1 1/1 1", 4).ok);
    assert!(!check_loadavg("x", 4).ok);
}

#[test]
fn disk_and_unit_fake() {
    let df = include_str!("fixtures/df.txt");
    assert_eq!(parse_df(df), Some(42.0));
    assert!(check_disk(&Fake(Ok(df.into())), "/").ok);
    assert!(
        !check_disk(
            &Fake(Ok("Filesystem 1 2 3 96% /\nx 1 2 3 96% /".into())),
            "/"
        )
        .ok
    );
    assert!(!check_disk(&Fake(Err("no df".into())), "/").ok);
    assert!(check_unit(&Fake(Ok("active\n".into())), "demo.service").ok);
    let r = check_unit(&Fake(Ok("inactive\n".into())), "demo.service");
    assert!(!r.ok);
    assert_eq!(r.detail, "inactive");
}

#[test]
fn freshness_file() {
    let p = out_dir().join("fresh.txt");
    std::fs::write(&p, "x").unwrap();
    assert!(check_freshness(&p, 60).ok);
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    let f = std::fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(old).unwrap();
    assert!(!check_freshness(&p, 60).ok);
    assert!(check_freshness(&p, 10_000).ok);
    assert!(!check_freshness(&out_dir().join("missing"), 60).ok);
}

#[test]
fn cloudflared_scan() {
    let root = out_dir().join("fakeproc");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("100")).unwrap();
    std::fs::create_dir_all(root.join("self")).unwrap();
    std::fs::write(root.join("100/comm"), "bash\n").unwrap();
    assert!(!check_cloudflared(&root).ok);
    std::fs::create_dir_all(root.join("200")).unwrap();
    std::fs::write(root.join("200/comm"), "cloudflared\n").unwrap();
    let r = check_cloudflared(&root);
    assert!(r.ok);
    assert_eq!(r.detail, "pid 200");
}

#[test]
fn incidents_fake_clock() {
    let mut v = Vec::new();
    let t = |n: u32| format!("2026-10-18T20:0{n}:00+02:00");
    assert_eq!(apply(&mut v, "a", Status::Healthy, "", &t(0)), None);
    assert_eq!(
        apply(&mut v, "a", Status::Degraded, "slow", &t(1)),
        Some(Event::Opened("a".into()))
    );
    assert_eq!(apply(&mut v, "a", Status::Critical, "down", &t(2)), None);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].severity, "low");
    assert!(apply(&mut v, "b", Status::Critical, "down", &t(3)).is_some());
    assert_eq!(v.len(), 2);
    assert_eq!(v[1].severity, "high");
    assert_eq!(
        apply(&mut v, "a", Status::Healthy, "", &t(4)),
        Some(Event::Resolved("a".into()))
    );
    assert_eq!(v[0].resolved_at.as_deref(), Some(t(4).as_str()));
    assert!(apply(&mut v, "a", Status::Critical, "again", &t(5)).is_some());
    assert_eq!(
        v.iter().filter(|i| i.service == "a" && i.is_open()).count(),
        1
    );
}

#[test]
fn ts_has_zagreb_offset() {
    let r = check_unit(&Fake(Ok("active".into())), "u");
    assert!(
        r.ts.ends_with("+01:00") || r.ts.ends_with("+02:00"),
        "{}",
        r.ts
    );
    assert!(!r.ts.ends_with('Z'));
}

#[test]
fn http_get_and_verdict() {
    use std::time::Duration;
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let h = std::thread::spawn(move || {
        for (code, body) in [
            ("200 OK", "ok"),
            ("503 Service Unavailable", "{\"status\":\"loading\"}"),
            ("503 Service Unavailable", "down"),
        ] {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            write!(
                s,
                "HTTP/1.0 {code}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let t = Duration::from_millis(2500);
    let a = http_get(addr, "/health", t);
    assert_eq!(a.result, Ok((200, "ok".to_string())));
    assert_eq!(verdict(&a.result, &[200], true), (true, "200".into()));
    let b = http_get(addr, "/health", t);
    assert_eq!(verdict(&b.result, &[200], true), (true, "loading".into()));
    assert_eq!(
        verdict(&b.result, &[200], false),
        (false, "http 503".into())
    );
    let c = http_get(addr, "/health", t);
    assert_eq!(verdict(&c.result, &[200], true), (false, "http 503".into()));
    h.join().unwrap();
    let d = http_get(addr, "/", t);
    let (ok, detail) = verdict(&d.result, &[200], true);
    assert!(!ok && detail.starts_with("connect:"), "{detail}");
    assert_eq!(truncate("héllo", 2), "h");
}
