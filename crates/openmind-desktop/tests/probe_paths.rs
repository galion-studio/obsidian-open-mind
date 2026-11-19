// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Probe paths the in-crate tests do not reach: spec edges, mode text that
//! is not a cortex mode, a reset peer, and the confirmed/summary report.

use openmind_desktop::probe::{observed_mode, Endpoint, ProbeReport};
use openmind_desktop::{probe_one, ProbeConfig, ProbeSet, Service, State, SERVICES};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn by_service(cfg: &ProbeConfig, service: Service) -> &Endpoint {
    cfg.endpoints
        .iter()
        .find(|endpoint| endpoint.service == service)
        .expect("service")
}

fn report(service: Service, enabled: bool, state: State, detail: &str) -> ProbeReport {
    ProbeReport {
        service,
        enabled,
        state,
        host: "localhost".into(),
        port: service.default_port(),
        path: service.default_path().into(),
        http_status: None,
        detail: detail.into(),
        observed_mode: None,
    }
}

#[test]
fn spec_edges_and_the_eight_names() {
    for service in SERVICES {
        assert_eq!(Service::parse(service.as_str()), Some(service));
    }
    assert!(Service::parse("cortex-mode").is_none());
    assert!(Service::parse("Galion One").is_none());

    let cfg = ProbeConfig::parse(
        " timeout = 50 ; nerve = [::1]:8670/health , galion-one = http://[::1]:8091/v1/health ",
    )
    .unwrap();
    assert_eq!(cfg.timeout, Duration::from_millis(50));
    let nerve = by_service(&cfg, Service::Nerve);
    assert_eq!(nerve.host, "::1");
    assert_eq!(nerve.port, 8670);
    assert_eq!(nerve.path, "/health");
    assert!(nerve.enabled);
    let one = by_service(&cfg, Service::GalionOne);
    assert_eq!(one.host, "::1");
    assert_eq!(one.port, 8091);
    assert_eq!(one.path, "/v1/health");
    assert_eq!(
        ProbeConfig::parse("timeout=10000").unwrap().timeout,
        Duration::from_millis(10_000)
    );
    let off = ProbeConfig::parse("nerve=-,,memory=-").unwrap();
    assert!(!by_service(&off, Service::Nerve).enabled);
    assert!(!by_service(&off, Service::Memory).enabled);
    assert!(by_service(&off, Service::Desk).enabled);

    for spec in [
        "timeout=49",
        "timeout=10001",
        "timeout=50,timeout=60",
        "nerve=[::1]/health",
        "nerve=[]:9/health",
        "nerve=:9/health",
        "nerve=localhost:65536/health",
        "nerve=bad_host:9/health",
        "nerve=localhost:8670/health#frag",
        "nerve=localhost:8670/a/../b",
        "nerve=http://",
        "desk=https://[::1]:8646/health",
    ] {
        assert!(ProbeConfig::parse(spec).is_err(), "{spec}");
    }
}

#[test]
fn cortex_mode_text_that_is_not_a_mode() {
    assert_eq!(
        observed_mode(Service::Cortex, r#"{"ok":true,"mode":"ab-1"}"#).as_deref(),
        Some("ab-1")
    );
    let mut rejected = vec![
        r#"{"mode":""}"#.to_string(),
        r#"{"mode":"has space"}"#.to_string(),
        r#"{"mode":"dry\"run"}"#.to_string(),
        r#"{"mode":"a\nb"}"#.to_string(),
        r#"{"mode":"a\/b"}"#.to_string(),
        r#"{"mode":1}"#.to_string(),
        r#"{"mode":"café"}"#.to_string(),
    ];
    rejected.push(format!(r#"{{"mode":"{}"}}"#, "a".repeat(33)));
    for body in &rejected {
        assert!(observed_mode(Service::Cortex, body).is_none(), "{body}");
    }
    assert!(observed_mode(Service::Nerve, r#"{"mode":"live"}"#).is_none());
    assert!(observed_mode(Service::GalionOne, r#"{"mode":"dry-run"}"#).is_none());
}

#[test]
fn empty_host_and_port_zero_do_not_dial() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(Mutex::new(0u32));
    let counted = Arc::clone(&hits);
    thread::spawn(move || {
        if listener.accept().is_ok() {
            *counted.lock().expect("hits") += 1;
        }
    });
    let mut endpoint = Endpoint {
        service: Service::Hermes,
        host: String::new(),
        port: addr.port(),
        path: "/health".into(),
        enabled: true,
    };
    let empty = probe_one(&endpoint, Duration::from_millis(200));
    assert!(empty.enabled);
    assert_eq!(empty.state, State::Unknown);
    assert_eq!(empty.detail, "not configured");
    endpoint.host = addr.ip().to_string();
    endpoint.port = 0;
    let zero = probe_one(&endpoint, Duration::from_millis(200));
    assert_eq!(zero.detail, "not configured");
    endpoint.port = addr.port();
    endpoint.path = "/a b".into();
    let bad = probe_one(&endpoint, Duration::from_millis(200));
    assert_eq!(bad.state, State::Unknown);
    assert_eq!(bad.detail, "invalid request");
    thread::sleep(Duration::from_millis(40));
    assert_eq!(*hits.lock().expect("hits"), 0);
}

#[test]
fn status_edges_keep_the_code_and_drop_a_bad_mode() {
    for (status, state) in [
        (204u16, State::Up),
        (299, State::Up),
        (300, State::Down),
        (404, State::Down),
    ] {
        let addr = serve(status, "");
        let report = probe_one(
            &Endpoint {
                service: Service::Desk,
                host: addr.ip().to_string(),
                port: addr.port(),
                path: "/health".into(),
                enabled: true,
            },
            Duration::from_secs(1),
        );
        assert_eq!(report.state, state, "{status}");
        assert_eq!(report.http_status, Some(status));
        assert!(report.observed_mode.is_none(), "{status}");
    }

    let spaced = serve(200, r#"{"mode":"not a mode"}"#);
    let spaced_report = probe_one(
        &Endpoint {
            service: Service::Cortex,
            host: spaced.ip().to_string(),
            port: spaced.port(),
            path: "/health".into(),
            enabled: true,
        },
        Duration::from_secs(1),
    );
    assert_eq!(spaced_report.state, State::Up);
    assert!(spaced_report.observed_mode.is_none());
}

#[test]
fn a_long_body_still_reports_the_status() {
    let body = format!(r#"{{"mode":"dry-run"}}{}"#, "x".repeat(8_000));
    let addr = serve(200, &body);
    let report = probe_one(
        &Endpoint {
            service: Service::Cortex,
            host: addr.ip().to_string(),
            port: addr.port(),
            path: "/health".into(),
            enabled: true,
        },
        Duration::from_secs(1),
    );
    assert_eq!(report.state, State::Up);
    assert_eq!(report.observed_mode.as_deref(), Some("dry-run"));
}

#[test]
fn confirmed_ignores_disabled_rows_and_json_escapes() {
    let up: Vec<ProbeReport> = SERVICES
        .into_iter()
        .map(|service| report(service, true, State::Up, "200"))
        .collect();
    let all = ProbeSet {
        timeout: Duration::from_millis(50),
        probes: up,
    };
    assert!(all.confirmed());
    assert!(all
        .summary()
        .starts_with("openmind-desktop: probes nerve=up"));
    assert!(all.summary().contains("galion-one=up"));

    let mut mixed = SERVICES
        .into_iter()
        .map(|service| report(service, false, State::Unknown, "not configured"))
        .collect::<Vec<_>>();
    mixed[0].enabled = true;
    mixed[0].state = State::Up;
    mixed[0].host = "h\"ost".into();
    mixed[0].detail = "say \"hi\"".into();
    let partial = ProbeSet {
        timeout: Duration::from_millis(50),
        probes: mixed,
    };
    assert!(partial.confirmed());
    let json = partial.to_json();
    assert!(json.contains("\"read_only\":true"));
    assert!(json.contains("\"writes\":0"));
    assert!(json.contains("say \\\"hi\\\""));
    assert!(json.contains("h\\\"ost"));

    let down = ProbeSet {
        timeout: Duration::from_millis(50),
        probes: vec![report(Service::Console, true, State::Down, "http 404")],
    };
    assert!(!down.confirmed());
    assert!(down
        .summary()
        .starts_with("openmind-desktop: probes console=down"));

    let none = ProbeSet {
        timeout: Duration::from_millis(50),
        probes: SERVICES
            .into_iter()
            .map(|service| report(service, false, State::Unknown, "not configured"))
            .collect(),
    };
    assert!(!none.confirmed());
    assert_eq!(none.summary(), "openmind-desktop: no probes enabled");
}

fn serve(status: u16, body: &str) -> std::net::SocketAddr {
    let body = body.to_string();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let bytes = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(bytes.as_bytes());
            let _ = sock.shutdown(Shutdown::Both);
        }
    });
    addr
}
