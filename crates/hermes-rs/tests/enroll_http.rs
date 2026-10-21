// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Enrollment routing through the real HTTP stack (loopback, mock provider):
//! off = 404 for every /openmind path, owner routes need the gateway key,
//! node routes never accept the owner key or a forged token.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::enroll::{Enroll, EnrollCfg};
use hermes_rs::provider::MockProvider;
use hermes_rs::session::MemStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const KEY: &str = "enroll-test-key-0123456789";

fn gateway(enroll: bool, tag: &str) -> (SocketAddr, std::path::PathBuf) {
    let home =
        std::env::temp_dir().join(format!("hermes-enroll-http-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let cfg = Arc::new(Config::defaults(&home));
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let agent = hermes_rs::agent::boss(
        cfg.clone(),
        Arc::new(MockProvider::echo()),
        spend,
        Registry::new(),
    );
    let mut gw = Gateway::new(
        cfg.clone(),
        agent,
        Arc::new(MemStore::default()),
        Some(KEY.into()),
    );
    if enroll {
        let env = |k: &str| match k {
            "HERMES_RS_ENROLL" => Some("1".to_string()),
            "HERMES_RS_ENROLL_URL" => Some("http://127.0.0.1:19329/openmind/enroll".to_string()),
            _ => None,
        };
        let ec = EnrollCfg::from_env(&cfg, &env).unwrap().unwrap();
        gw.enroll = Some(Arc::new(Enroll::open(ec).unwrap()));
    }
    (
        hermes_rs::api::spawn(Arc::new(gw), "127.0.0.1:0").unwrap(),
        home,
    )
}

fn status(addr: SocketAddr, method: &str, path: &str, auth: Option<&str>, body: &str) -> u16 {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let auth = auth
        .map(|a| format!("Authorization: Bearer {a}\r\n"))
        .unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn off_by_default_every_openmind_path_is_404() {
    let (a, home) = gateway(false, "off");
    for (m, p) in [
        ("POST", "/openmind/enroll"),
        ("POST", "/openmind/enroll/claim"),
        ("GET", "/openmind/enroll/pending"),
        ("GET", "/openmind/node/whoami"),
        ("GET", "/p/default/openmind/enroll"),
    ] {
        assert_eq!(status(a, m, p, None, "{}"), 404, "{m} {p}");
        assert_eq!(status(a, m, p, Some(KEY), "{}"), 404, "{m} {p} with key");
    }
    // the rest of the API is unchanged
    assert_eq!(status(a, "GET", "/v1/models", None, ""), 401);
    assert_eq!(status(a, "GET", "/v1/models", Some(KEY), ""), 200);
    std::fs::remove_dir_all(home).ok();
}

#[test]
fn on_owner_and_node_routes_are_separate() {
    let (a, home) = gateway(true, "on");
    assert_eq!(status(a, "GET", "/openmind/enroll/pending", None, ""), 401);
    assert_eq!(
        status(
            a,
            "GET",
            "/openmind/enroll/pending",
            Some("wrong-key-0123456789"),
            ""
        ),
        401
    );
    assert_eq!(
        status(a, "GET", "/openmind/enroll/pending", Some(KEY), ""),
        200
    );
    assert_eq!(
        status(a, "GET", "/openmind/enroll/devices", Some(KEY), ""),
        200
    );
    // the owner key is not a node credential
    assert_eq!(
        status(a, "GET", "/openmind/node/whoami", Some(KEY), ""),
        401
    );
    let forged = format!("omn1_{}", "0".repeat(64));
    assert_eq!(
        status(a, "GET", "/openmind/node/whoami", Some(&forged), ""),
        401
    );
    assert_eq!(status(a, "POST", "/openmind/enroll", None, "{}"), 400);
    assert_eq!(
        status(a, "POST", "/p/default/openmind/enroll", None, "{}"),
        400
    );
    assert_eq!(status(a, "GET", "/openmind/nope", Some(KEY), ""), 404);
    std::fs::remove_dir_all(home).ok();
}
