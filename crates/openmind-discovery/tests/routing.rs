// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process routing flow: approved LAN row first, CPU fallback when it is
//! down, never a pending or denied row. Loopback stubs on ephemeral ports.

use openmind_discovery::admit::{self, ApprovalsLog, Decision};
use openmind_discovery::routing::{operator_row, Router};
use openmind_provider::route::{Capability, Kind};
use openmind_provider::{ProviderKind, Registry, StaticProvider};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct Stub {
    port: u16,
    hits: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl Stub {
    fn start(name: &'static str, status: u16) -> Stub {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, s) = (hits.clone(), stop.clone());
        std::thread::spawn(move || {
            for c in l.incoming() {
                if s.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut c) = c else { continue };
                let mut r = BufReader::new(c.try_clone().unwrap());
                let mut len = 0;
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    continue; // a `join` connect: no request
                }
                loop {
                    let mut hdr = String::new();
                    r.read_line(&mut hdr).unwrap();
                    if hdr.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = hdr.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut b = vec![0; len];
                r.read_exact(&mut b).unwrap();
                h.fetch_add(1, Ordering::SeqCst);
                let body = format!(r#"{{"model":"{name}"}}"#);
                let _ = write!(
                    c,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Stub { port, hits, stop }
    }

    fn kill(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("lan-routing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn chat(r: &Router) -> (Option<String>, Vec<String>) {
    let f = r
        .forward(
            Capability::Llm,
            "POST",
            "/v1/chat/completions",
            br#"{"model":"auto","messages":[]}"#,
        )
        .unwrap();
    (
        f.provider,
        f.attempts
            .iter()
            .map(|a| format!("{}:{}", a.id, a.outcome))
            .collect(),
    )
}

#[test]
fn approve_route_kill_fallback_deny_never_used() {
    let d = dir("flow");
    let lan = Stub::start("lan", 200);
    let pend = Stub::start("pending", 200);
    let cpu = Stub::start("cpu", 200);
    let reg_path = d.join("providers.json");
    let log = ApprovalsLog::new(d.join("approvals.log"));
    let mut reg = Registry::load(&reg_path).unwrap();
    for (id, port) in [("lan-gpu", lan.port), ("lan-pending", pend.port)] {
        let p = StaticProvider::try_new(
            id,
            ProviderKind::Lan,
            format!("http://127.0.0.1:{port}"),
            vec![openmind_provider::Capability::Llm],
        )
        .unwrap();
        reg.register(&p).unwrap();
    }
    let router = Router {
        registry: reg_path.clone(),
        extra: vec![operator_row(
            "cpu-local",
            Kind::CpuLocal,
            &format!("http://127.0.0.1:{}", cpu.port),
        )
        .unwrap()],
        timeout: Duration::from_millis(500),
    };

    // Nothing approved yet: CPU.
    assert_eq!(chat(&router).0.as_deref(), Some("cpu-local"));

    // Approved but not joined: still CPU. Joined up: LAN.
    admit::decide(&mut reg, &log, "lan-gpu", Decision::Approve, "t", None).unwrap();
    assert_eq!(chat(&router).0.as_deref(), Some("cpu-local"));
    admit::join(&mut reg, &log, "lan-gpu", "t", Duration::from_millis(500)).unwrap();
    assert_eq!(chat(&router).0.as_deref(), Some("lan-gpu"));
    assert_eq!(lan.hits.load(Ordering::SeqCst), 1);

    // Kill it: same request falls through to CPU at once.
    lan.kill();
    let (p, tried) = chat(&router);
    assert_eq!(p.as_deref(), Some("cpu-local"));
    assert_eq!(tried, ["lan-gpu:connect-failed", "cpu-local:ok"]);
    // join records it down: no longer even tried.
    let line = admit::join(&mut reg, &log, "lan-gpu", "t", Duration::from_millis(300)).unwrap();
    assert_eq!(line["health"], "down");
    assert_eq!(chat(&router).1, ["cpu-local:ok"]);

    // Bring a server back on a new port is a different story (moved); here deny it.
    admit::decide(&mut reg, &log, "lan-gpu", Decision::Deny, "t", None).unwrap();
    assert_eq!(chat(&router).1, ["cpu-local:ok"]);

    // The pending row was never dialed.
    assert_eq!(pend.hits.load(Ordering::SeqCst), 0);

    // CPU down too: 503 with every attempt listed.
    cpu.kill();
    let f = router
        .forward(Capability::Llm, "POST", "/v1/chat/completions", b"{}")
        .unwrap();
    assert_eq!(f.status, 503);
    assert!(f.provider.is_none());
    pend.kill();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_5xx_falls_through_and_a_4xx_does_not() {
    let d = dir("5xx");
    let bad = Stub::start("bad", 503);
    let cpu = Stub::start("cpu", 200);
    let mut extra = vec![operator_row(
        "gpu-0",
        Kind::Gpu,
        &format!("http://127.0.0.1:{}", bad.port),
    )
    .unwrap()];
    extra.push(
        operator_row(
            "cpu-local",
            Kind::CpuLocal,
            &format!("http://127.0.0.1:{}", cpu.port),
        )
        .unwrap(),
    );
    let router = Router {
        registry: d.join("none.json"),
        extra,
        timeout: Duration::from_millis(500),
    };
    assert_eq!(chat(&router).1, ["gpu-0:upstream-503", "cpu-local:ok"]);
    let four = Stub::start("four", 404);
    let router = Router {
        registry: d.join("none.json"),
        extra: vec![
            operator_row(
                "gpu-0",
                Kind::Gpu,
                &format!("http://127.0.0.1:{}", four.port),
            )
            .unwrap(),
            operator_row(
                "cpu-local",
                Kind::CpuLocal,
                &format!("http://127.0.0.1:{}", cpu.port),
            )
            .unwrap(),
        ],
        timeout: Duration::from_millis(500),
    };
    assert_eq!(chat(&router).1, ["gpu-0:http-404"]);
    for s in [bad, cpu, four] {
        s.kill();
    }
    let _ = std::fs::remove_dir_all(&d);
}
