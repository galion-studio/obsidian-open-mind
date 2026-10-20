// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life check of the Rust pages against live data (GALION_REAL=1).
//!
//! Run by `scripts/real/console-ui-rust.sh`, which starts the release
//! binary with `--ui rust` on 127.0.0.1:19375 and a read-only SSH forward
//! 127.0.0.1:19379 -> VPS 127.0.0.1:8080 (+ 19378 -> 8091). This test reads
//! the same live API (GET only) and checks that each page shows exactly what
//! the API says. It never sends a POST and never opens the markers stream.

use console_ui::client::{self, Forward};
use console_ui::json::{self, Value};
use console_ui::pages;
use std::net::SocketAddr;

fn env_addr(name: &str, default: &str) -> SocketAddr {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap()
}

fn get(addr: SocketAddr, path: &str, host: &str) -> (u16, Vec<u8>) {
    let fwd = Forward {
        host: host.into(),
        ..Forward::default()
    };
    let r = client::call(addr, "GET", path, &fwd, None).expect(path);
    (r.status, r.body)
}

fn api(addr: SocketAddr, path: &str) -> Value {
    let (code, body) = get(addr, path, "127.0.0.1");
    assert_eq!(code, 200, "{path}");
    json::parse(&body).expect(path)
}

#[test]
fn rust_pages_match_live_api() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 (see scripts/real/console-ui-rust.sh)");
        return;
    }
    let front = env_addr("CONSOLE_UI_FRONT", "127.0.0.1:19375");
    let up = env_addr("CONSOLE_UI_UPSTREAM", "127.0.0.1:19379");
    let pulse_addr = env_addr("CONSOLE_UI_PULSE", "127.0.0.1:19378");

    let health = api(up, "/api/v1/health");
    let services = pages::services_of(&api(up, "/api/v1/services"));
    let domains = api(up, "/api/v1/domains");
    let pulse = pages::pulses_of(&api(pulse_addr, "/api/v1/awake"));
    assert!(!services.is_empty());

    // /status
    let (code, body) = get(front, "/status", "127.0.0.1");
    let html = String::from_utf8_lossy(&body);
    assert_eq!(code, 200);
    let c = pages::status::counts(&services, &pulse);
    let line = format!("{} subsystems online · {} held", c.online, c.held);
    assert!(html.contains(&line), "status counts {line}");
    let healthy = health.get("status").as_str() == Some("healthy");
    assert!(html.contains(if healthy {
        "Light stack is up."
    } else {
        "Origins are dark."
    }));
    let hosts = domains.get("items").as_array();
    for d in hosts {
        let h = d.get("host").as_str().unwrap();
        assert!(html.contains(&console_ui::html::esc(h)), "host {h}");
    }
    eprintln!(
        "real: /status {} B, {line}, {} hosts, healthy={healthy}",
        body.len(),
        hosts.len()
    );

    // /services
    let (code, body) = get(front, "/services", "127.0.0.1");
    let html = String::from_utf8_lossy(&body);
    assert_eq!(code, 200);
    let (upg, downg, extras) = pages::services::groups(&services, &pulse);
    for s in services.iter() {
        assert!(
            html.contains(&console_ui::html::esc(&s.name)),
            "service {}",
            s.id
        );
    }
    let up_at = html.find("<h2>Up</h2>").unwrap();
    let down_at = html.find("<h2>Down</h2>").unwrap();
    for s in &upg {
        let at = html
            .find(&format!(">{}<", console_ui::html::esc(&s.name)))
            .unwrap();
        assert!(at > up_at && at < down_at, "{} in Up", s.id);
    }
    for s in &downg {
        let at = html
            .find(&format!(">{}<", console_ui::html::esc(&s.name)))
            .unwrap();
        assert!(at > down_at, "{} in Down", s.id);
    }
    eprintln!(
        "real: /services {} B, up {} down {} extras {} pulse {}",
        body.len(),
        upg.len(),
        downg.len(),
        extras.len(),
        pulse.len()
    );

    // /agents
    let (code, body) = get(front, "/agents", "127.0.0.1");
    let html = String::from_utf8_lossy(&body);
    assert_eq!(code, 200);
    let version = health.get("version").as_str().unwrap_or("");
    assert!(html.contains(&format!(">{version}<")), "version {version}");
    if let Some(pod) = health.get("target").get("pod").as_str() {
        assert!(html.contains(&format!(">{pod}<")), "pod {pod}");
    }
    eprintln!("real: /agents {} B, version {version}", body.len());

    // Gate: the public console host without the admin cookie gets the
    // pairing page (the upstream answers GET /api/v1/admin you:false), and
    // no page data.
    let (code, body) = get(front, "/agents", "console.galion.app");
    let html = String::from_utf8_lossy(&body);
    assert_eq!(code, 200);
    assert!(html.contains("This phone") && !html.contains(version));
    let (code, body) = get(front, "/status", "galion.app");
    assert_eq!(code, 200);
    assert!(String::from_utf8_lossy(&body).contains("Soon online."));
    eprintln!("real: gate console.galion.app -> pairing page; galion.app -> Soon");
}
