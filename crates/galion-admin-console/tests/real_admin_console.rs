// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1): starts the real `galion-admin-console`
//! binary on 127.0.0.1:21770 (slot 307 range 21770-21779) and drives the
//! phone flow over real HTTP. Upstreams: a nerve-shaped server on :21771 and
//! a console-health server on :21772, or the live VPS nerve when
//! `GALION_ADMIN_REAL_NERVE=127.0.0.1:PORT` points at a read-only ssh forward
//! of the VPS 127.0.0.1:8670 (GET only); `GALION_ADMIN_REAL_CONSOLE` likewise for the
//! live console :8080 (`/api/v1/health` GET only).
//!
//! Run: `GALION_REAL=1 cargo test --release -p galion-admin-console --test real_admin_console -- --nocapture`

use galion_common::http::{self, ClientRequest, Limits, Response};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-admin-console");
const PORT: u16 = 21770;
const ORIGIN: &str = "http://127.0.0.1:21770";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn addr(p: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], p))
}

fn fake(port: u16, path: &'static str, body: &'static str) {
    let l = TcpListener::bind(addr(port)).expect("fake upstream bind");
    std::thread::spawn(move || {
        let h = move |r: &http::Request| {
            if r.path() == path {
                Response::json(200, body)
            } else {
                Response::text(404, "no\n")
            }
        };
        let _ = http::serve(l, Limits::default(), Arc::new(h));
    });
}

const NERVE: &str = r#"{"ts":"2026-10-27T21:00:01+01:00","cycle":19,"mode":"live",
"self_eval":{"score":44,"grade":"F","deductions":[{"what":"edge: host.example :3010"}]},
"components":[{"name":"console","status":"healthy","port":8080},{"name":"galion-one","status":"failing","port":8091},
{"name":"hermes-rs","status":"healthy"}],
"incidents":{"open":[{"id":"inc-06ffb32f","severity":"error","cause":"edge","title":"host.example down"}]}}"#;
const HEALTH: &str =
    r#"{"status":"healthy","service":"galion-console-static","implementation":"rust"}"#;

fn call(method: &str, path: &str, hdrs: &[(&str, &str)], body: &[u8]) -> http::ClientResponse {
    let mut req = if method == "POST" {
        ClientRequest::post(path, "127.0.0.1", "application/x-www-form-urlencoded", body)
    } else {
        ClientRequest::get(path, "127.0.0.1")
    };
    req.method = if method == "POST" { "POST" } else { "GET" };
    for (k, v) in hdrs {
        req.headers.push((k.to_string(), v.to_string()));
    }
    http::send(addr(PORT), &req, Duration::from_secs(10), 1 << 20).expect("request")
}

fn call_json(body: &str) -> http::ClientResponse {
    let req = ClientRequest::post(
        "/api/v1/admin",
        "127.0.0.1",
        "application/json",
        body.as_bytes(),
    );
    http::send(addr(PORT), &req, Duration::from_secs(10), 1 << 20).expect("request")
}

fn cli(state: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(BIN)
        .arg(args[0])
        .arg("--state")
        .arg(state)
        .args(&args[1..])
        .output()
        .expect("cli");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn state_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("gac-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn real_phone_pairing_flow() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let real_nerve = std::env::var("GALION_ADMIN_REAL_NERVE").ok();
    let nerve = match &real_nerve {
        Some(a) => a.clone(),
        None => {
            fake(21771, "/state", NERVE);
            "127.0.0.1:21771".into()
        }
    };
    let console = match std::env::var("GALION_ADMIN_REAL_CONSOLE") {
        Ok(a) => a,
        Err(_) => {
            fake(21772, "/api/v1/health", HEALTH);
            "127.0.0.1:21772".into()
        }
    };
    let state = state_dir();
    let t0 = Instant::now();
    let _srv = Kill(
        Command::new(BIN)
            .args(["serve", "--bind", "127.0.0.1:21770", "--state"])
            .arg(&state)
            .args([
                "--nerve",
                &nerve,
                "--console",
                &console,
                "--origin",
                ORIGIN,
                "--insecure-cookie",
            ])
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn serve"),
    );
    let mut up = false;
    for _ in 0..100 {
        if http::send(
            addr(PORT),
            &ClientRequest::get("/healthz", "x"),
            Duration::from_millis(300),
            4096,
        )
        .is_ok_and(|r| r.status == 200)
        {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(up, "server did not come up");
    eprintln!("startup {} ms", t0.elapsed().as_millis());

    // 1. unpaired
    assert_eq!(
        call("GET", "/api/v1/admin", &[], b"").text(),
        r#"{"you":false,"paired":false}"#
    );
    let r = call("GET", "/api/v1/admin/status", &[], b"");
    assert_eq!(
        (r.status, r.text().as_str()),
        (401, r#"{"error":"phone required"}"#)
    );
    assert!(call("GET", "/admin", &[], b"")
        .text()
        .contains("Pair this phone"));

    // 2. code from the server terminal
    let (rc, out) = cli(&state, &["issue-code"]);
    assert_eq!(rc, 0);
    let code = out.trim().to_string();
    assert_eq!(code.len(), 9);
    let codes = std::fs::read_to_string(state.join("codes")).unwrap();
    assert!(
        !codes.contains(&code.replace('-', "")),
        "code stored in clear"
    );

    // 3. cross-origin refused, bad code refused, right code pairs
    let body = format!("code={}&label=Redmi+Note", code.to_lowercase());
    let r = call(
        "POST",
        "/api/v1/admin/pair",
        &[("Origin", "https://evil.example")],
        body.as_bytes(),
    );
    assert_eq!(r.status, 403);
    let r = call(
        "POST",
        "/api/v1/admin/pair",
        &[("Origin", ORIGIN)],
        b"code=0000-0000",
    );
    assert_eq!(
        (r.status, r.header("location")),
        (303, Some("/admin?e=bad"))
    );
    let r = call(
        "POST",
        "/api/v1/admin/pair",
        &[("Origin", ORIGIN)],
        body.as_bytes(),
    );
    assert_eq!(r.status, 303);
    let sc = r.header("set-cookie").expect("cookie").to_string();
    assert!(sc.contains("HttpOnly") && sc.contains("SameSite=Strict"));
    let cookie = sc.split(';').next().unwrap().to_string();
    let token = cookie.trim_start_matches("galion_admin=").to_string();
    assert!(token.len() >= 43);

    // 4. paired: status + page
    let me = call("GET", "/api/v1/admin", &[("Cookie", &cookie)], b"").text();
    assert!(
        me.contains(r#""paired":true"#) && me.contains("node-"),
        "{me}"
    );
    let st = call("GET", "/api/v1/admin/status", &[("Cookie", &cookie)], b"");
    assert_eq!(st.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&st.body).unwrap();
    eprintln!("status: {}", st.text());
    assert_eq!(v["nerve"]["ok"], true, "{}", st.text());
    assert!(v["nerve"]["grade"].is_string());
    assert_eq!(v["console"]["http"], 200);
    if real_nerve.is_none() {
        assert_eq!(v["nerve"]["grade"], "F");
        assert!(!st.text().contains("host.example") && !st.text().contains("8091"));
    }
    let page = call("GET", "/admin", &[("Cookie", &cookie)], b"").text();
    assert!(page.contains("Unpair this phone") && page.contains("nerve cycle"));
    assert_eq!(
        call(
            "POST",
            "/api/v1/admin/pair",
            &[("Origin", ORIGIN)],
            body.as_bytes()
        )
        .header("location"),
        Some("/admin?e=bad")
    );

    // 4b. console-ui gate contract (JSON pair on /api/v1/admin, "you")
    let (_, c2) = cli(&state, &["issue-code"]);
    let r = call_json(&format!(r#"{{"code":"{}"}}"#, c2.trim()));
    assert_eq!(r.status, 200, "{}", r.text());
    let ck2 = r
        .header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(call("GET", "/api/v1/admin", &[("Cookie", &ck2)], b"")
        .text()
        .contains(r#""you":true"#));
    assert_eq!(call_json(r#"{"code":"0000-0000"}"#).status, 401);

    // 5. CLI views
    let (rc, devs) = cli(&state, &["devices"]);
    assert_eq!(rc, 0);
    assert!(
        devs.contains("active")
            && devs.contains("Redmi Note")
            && devs.contains("+01:00") | devs.contains("+02:00")
    );
    let node = devs.split('\t').next().unwrap().to_string();

    // 6. lockout: 5 wrong codes, then even a fresh right code is refused
    for _ in 0..4 {
        call(
            "POST",
            "/api/v1/admin/pair",
            &[("Origin", ORIGIN)],
            b"code=ZZZZ-ZZZZ",
        );
    }
    let (_, fresh) = cli(&state, &["issue-code"]);
    let r = call(
        "POST",
        "/api/v1/admin/pair",
        &[("Origin", ORIGIN)],
        format!("code={}", fresh.trim()).as_bytes(),
    );
    assert_eq!(r.header("location"), Some("/admin?e=locked"));

    // 7. oversized body refused by the server limits
    let big = vec![b'a'; 8192];
    assert_eq!(
        call("POST", "/api/v1/admin/pair", &[("Origin", ORIGIN)], &big).status,
        413
    );

    // 8. revoke from the terminal kills the phone session
    let (rc, _) = cli(&state, &["revoke", &node]);
    assert_eq!(rc, 0);
    assert_eq!(
        call("GET", "/api/v1/admin/status", &[("Cookie", &cookie)], b"").status,
        401
    );

    // 9. audit: chain ok, no token/code; tamper detected (exit 4)
    let (rc, out) = cli(&state, &["audit-verify"]);
    assert_eq!(rc, 0, "{out}");
    eprintln!("audit: {}", out.trim());
    let log = std::fs::read_to_string(state.join("audit.jsonl")).unwrap();
    assert!(!log.contains(&token) && !log.contains(&code) && !log.contains(fresh.trim()));
    assert!(log.contains("pair.ok") && log.contains("locked") && log.contains("revoke"));
    std::fs::write(
        state.join("audit.jsonl"),
        log.replacen("pair.ok", "pair.OK", 1),
    )
    .unwrap();
    assert_eq!(cli(&state, &["audit-verify"]).0, 4);

    // 10. modes
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m(&state), 0o700);
        assert_eq!(m(&state.join("devices")), 0o600);
        assert_eq!(m(&state.join("audit.jsonl")), 0o600);
    }

    // 11. refused configs
    let o = Command::new(BIN)
        .args(["serve", "--bind", "0.0.0.0:21774", "--state"])
        .arg(&state)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = Command::new(BIN)
        .args([
            "serve",
            "--bind",
            "127.0.0.1:21774",
            "--nerve",
            "10.0.0.1:8670",
            "--state",
        ])
        .arg(&state)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    drop(_srv);
    let _ = std::fs::remove_dir_all(&state);
}
