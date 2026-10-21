// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! In-process auth tests for POSEIDON (Gaia S4.1, slot 205, ports
//! 20750-20751): start-up refusals, loopback unchanged, and 401 for
//! unauthenticated non-loopback peers on a real non-loopback test bind
//! (the box's own LAN-side address, found without sending any packet).

use openmind_queue::{http, server};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SECRET: &str = "s205-test-only-QWxhZGRpbjpvcGVuIHNlc2FtZQ-not-a-real-secret";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("poseidon-auth-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn token_file(dir: &Path, mode: u32) -> PathBuf {
    let f = dir.join("token");
    std::fs::write(&f, format!("{SECRET}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
    f
}

/// A local non-loopback IPv4 address. UDP connect only picks a route; it
/// sends nothing (192.0.2.1 is TEST-NET-1).
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Raw request so the test can see headers. Returns (status, head, body).
fn raw(addr: &str, req: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let txt = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = txt.split_once("\r\n\r\n").unwrap();
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, head.to_string(), body.to_string())
}

fn call(addr: &str, m: &str, p: &str, b: Option<Value>, tok: Option<&str>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request_auth(
        addr,
        m,
        p,
        body.as_deref(),
        tok.map(str::as_bytes),
        Duration::from_secs(5),
    )
    .unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

#[test]
fn startup_refusals() {
    let d = tmp("refuse");
    let base = server::Options {
        data: d.join("data"),
        ..server::Options::default()
    };
    // Non-loopback without --allow-remote: refused (unchanged).
    let e = server::start(server::Options {
        bind: "0.0.0.0:20750".into(),
        ..base.clone()
    })
    .err()
    .expect("must refuse");
    assert!(e.to_string().contains("--allow-remote"), "{e}");
    // --allow-remote alone is no longer enough.
    let e = server::start(server::Options {
        bind: "0.0.0.0:20750".into(),
        allow_remote: true,
        ..base.clone()
    })
    .err()
    .expect("must refuse a LAN bind without a token file");
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(e.to_string().contains("--token-file"), "{e}");
    // token-on-loopback without a secret: refused.
    let e = server::start(server::Options {
        bind: "127.0.0.1:20750".into(),
        token_on_loopback: true,
        ..base.clone()
    })
    .err()
    .expect("must refuse");
    assert!(e.to_string().contains("--token-file"), "{e}");
    // Group/other-readable token file: refused, secret not echoed.
    for mode in [0o644, 0o640, 0o606] {
        let f = token_file(&d, mode);
        let e = server::start(server::Options {
            bind: "127.0.0.1:20750".into(),
            token_file: Some(f),
            ..base.clone()
        })
        .err()
        .expect("must refuse loose mode");
        assert!(e.to_string().contains("0600"), "{e}");
        assert!(!e.to_string().contains(SECRET));
    }
    // Missing token file: refused.
    assert!(server::start(server::Options {
        bind: "127.0.0.1:20750".into(),
        token_file: Some(d.join("absent")),
        ..base.clone()
    })
    .is_err());
    // Nothing bound after the refusals.
    assert!(TcpStream::connect("127.0.0.1:20750").is_err());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn loopback_unchanged_and_lan_needs_token() {
    let d = tmp("lan");
    let f = token_file(&d, 0o600);

    // 1. Loopback with a token file configured: local clients need nothing.
    let h = server::start(server::Options {
        bind: "127.0.0.1:20750".into(),
        data: d.join("lo"),
        token_file: Some(f.clone()),
        ..server::Options::default()
    })
    .unwrap();
    let a = h.addr.to_string();
    let (c, _) = call(&a, "POST", "/jobs", Some(json!({"kind":"echo"})), None);
    assert_eq!(c, 201);
    let (c, l) = call(&a, "POST", "/lease", Some(json!({"worker":"w"})), None);
    assert_eq!(c, 200);
    let (c, _) = call(
        &a,
        "POST",
        &format!("/jobs/{}/ack", l["id"]),
        Some(json!({"token": l["token"]})),
        None,
    );
    assert_eq!(c, 200);
    // A wrong token from loopback is ignored, not punished (unchanged).
    let (c, st) = call(&a, "GET", "/stats", None, Some("wrong"));
    assert_eq!(c, 200);
    assert_eq!(st["auth"]["mode"], "token-for-remote");
    assert_eq!(st["auth"]["verdicts"]["loopback"], 4);
    h.stop();

    // 2. Real non-loopback test bind on this box's own LAN-side address.
    let Some(ip) = lan_ip() else {
        eprintln!("skipped LAN part: no non-loopback IPv4 on this host");
        return;
    };
    let lan = format!("{ip}:20751");
    let h = server::start(server::Options {
        bind: lan.clone(),
        data: d.join("lan"),
        allow_remote: true,
        token_file: Some(f.clone()),
        ..server::Options::default()
    })
    .unwrap();
    assert_eq!(h.addr.ip(), ip);
    // Not listening on loopback at all.
    assert!(TcpStream::connect("127.0.0.1:20751").is_err());

    let paths = [
        ("GET", "/healthz", ""),
        ("GET", "/stats", ""),
        ("GET", "/dlq", ""),
        ("GET", "/vram", ""),
        ("GET", "/gaia/status", ""),
        ("POST", "/jobs", r#"{"kind":"echo"}"#),
        ("POST", "/lease", r#"{"worker":"x"}"#),
        ("POST", "/caps", r#"{"scope":"*","cap":1}"#),
        ("POST", "/quotas", r#"{"agent":"a","max_pending":1,"max_inflight":1}"#),
        ("POST", "/admin/compact", ""),
        ("DELETE", "/vram/devices/x", ""),
        ("GET", "/nope", ""),
    ];
    let bad_auth = [
        String::new(),
        "Authorization: Bearer wrong\r\n".into(),
        format!("Authorization: Basic {SECRET}\r\n"),
        format!("Authorization: Bearer {SECRET}x\r\n"),
        format!("Authorization: Bearer {}\r\n", &SECRET[..SECRET.len() - 1]),
        "Authorization: Bearer\r\n".into(),
        format!("Authorization: Bearer {}\r\n", SECRET.to_uppercase()),
    ];
    let mut refused = 0;
    for (m, p, b) in paths {
        for hdr in &bad_auth {
            let req = format!(
                "{m} {p} HTTP/1.1\r\nHost: x\r\n{hdr}Content-Length: {}\r\n\r\n{b}",
                b.len()
            );
            let (code, head, body) = raw(&lan, &req);
            assert_eq!(code, 401, "{m} {p} [{hdr}] -> {code} {body}");
            assert!(head.contains("WWW-Authenticate: Bearer"), "{head}");
            assert!(!body.contains(SECRET) && !head.contains(SECRET));
            refused += 1;
        }
    }
    // Duplicate Authorization headers: rejected as a bad request.
    let dup = format!(
        "GET /stats HTTP/1.1\r\nAuthorization: Bearer {SECRET}\r\nAuthorization: Bearer x\r\n\r\n"
    );
    assert_eq!(raw(&lan, &dup).0, 400);

    // Nothing the refused requests asked for happened.
    let (c, st) = call(&lan, "GET", "/stats", None, Some(SECRET));
    assert_eq!(c, 200, "{st}");
    assert_eq!(st["depth"]["ready"], 0, "a refused POST /jobs leaked through");
    assert_eq!(st["global"]["cap"], Value::Null);
    assert_eq!(st["auth"]["verdicts"]["token"], 1);
    let rej = st["auth"]["verdicts"]["missing"].as_u64().unwrap()
        + st["auth"]["verdicts"]["invalid"].as_u64().unwrap();
    assert_eq!(rej, refused);
    assert!(!st.to_string().contains(SECRET));

    // With the token, the full job cycle works from the LAN address.
    let (c, _) = call(&lan, "POST", "/jobs", Some(json!({"kind":"echo"})), Some(SECRET));
    assert_eq!(c, 201);
    let (_, l) = call(&lan, "POST", "/lease", Some(json!({"worker":"w"})), Some(SECRET));
    let (c, _) = call(
        &lan,
        "POST",
        &format!("/jobs/{}/ack", l["id"]),
        Some(json!({"token": l["token"]})),
        Some(SECRET),
    );
    assert_eq!(c, 200);
    // Lower-case scheme accepted (RFC 7235: case-insensitive).
    let (code, _, _) = raw(
        &lan,
        &format!("GET /healthz HTTP/1.1\r\nauthorization: bearer {SECRET}\r\n\r\n"),
    );
    assert_eq!(code, 200);
    h.stop();

    // 3. token-on-loopback: loopback now needs the token as well.
    let h = server::start(server::Options {
        bind: "127.0.0.1:20750".into(),
        data: d.join("lo2"),
        token_file: Some(f),
        token_on_loopback: true,
        ..server::Options::default()
    })
    .unwrap();
    let a = h.addr.to_string();
    assert_eq!(call(&a, "GET", "/healthz", None, None).0, 401);
    assert_eq!(call(&a, "GET", "/healthz", None, Some("nope")).0, 401);
    let (c, st) = call(&a, "GET", "/stats", None, Some(SECRET));
    assert_eq!(c, 200);
    assert_eq!(st["auth"]["mode"], "token-for-all");
    h.stop();
    std::fs::remove_dir_all(&d).unwrap();
}
