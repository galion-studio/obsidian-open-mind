// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end: `--instances DIR` on the real server over loopback. The
//! owner gets a code on `/instances`, the real `galion-join` agent code
//! (in process) joins with it and reports, a phone joins on `/join`, and
//! the gates hold (no list for strangers, no code reuse, CSRF refused,
//! nothing secret on disk or in the list).

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-inst-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(dir: &std::path::Path, lan: Option<u16>) -> SocketAddr {
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--upstream".into(),
        "127.0.0.1:9".into(),
        "--local-api".into(),
        "--instances".into(),
        dir.display().to_string(),
        "--quiet".into(),
    ];
    if let Some(p) = lan {
        args.push("--instances-lan".into());
        args.push(p.to_string());
    }
    let cfg = Config::from_args(args).unwrap();
    let routes = Routes::parse("page /\n").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(Site::default(), routes, cfg));
    thread::spawn(move || server::serve(l, state));
    addr
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn send(addr: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(a: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post(a: SocketAddr, path: &str, host: &str, extra: &str, ct: &str, body: &str) -> Resp {
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

const FORM: &str = "application/x-www-form-urlencoded";
const SAME: &str = "Sec-Fetch-Site: same-origin\r\n";

fn code_from(html: &str) -> String {
    let i = html.find("class=\"fl-code\">").expect("code shown") + "class=\"fl-code\">".len();
    html[i..i + 9].to_string()
}

#[test]
fn owner_code_agent_join_and_report() {
    let dir = tmp("agent");
    let a = start(&dir, None);
    let host = format!("localhost:{}", a.port());

    let page = get(a, "/instances", &host, "");
    assert_eq!(page.code, 200, "{}", page.body);
    assert!(page.body.contains("Your devices"));
    assert!(page.body.contains("This console"));
    assert!(page.head.contains("Content-Security-Policy"));
    assert!(
        !page.body.contains("<script"),
        "the owner page has no script"
    );
    assert!(page.body.contains("/_ui/instances."), "fleet sheet linked");

    // CSRF: a cross-site form post does nothing.
    let x = post(
        a,
        "/instances",
        &host,
        "Sec-Fetch-Site: cross-site\r\n",
        FORM,
        "op=add",
    );
    assert_eq!(x.code, 403);

    let made = post(
        a,
        "/instances",
        &host,
        SAME,
        FORM,
        "op=add&name=Studio+laptop",
    );
    assert_eq!(made.code, 200, "{}", made.body);
    let code = code_from(&made.body);
    assert!(made
        .body
        .contains(&format!("galion-join {code} --console http://{host}")));
    // Shown once.
    assert!(!get(a, "/instances", &host, "").body.contains(&code));

    // The real agent joins and reports once.
    let state = dir.join("join.json");
    let mut out = Vec::new();
    let rc = galion_instances::agent::main_with(
        vec![
            code.clone(),
            "--console".into(),
            format!("http://{host}"),
            "--state".into(),
            state.display().to_string(),
            "--once".into(),
        ],
        &mut out,
    );
    let said = String::from_utf8_lossy(&out).to_string();
    assert_eq!(rc, 0, "{said}");
    assert!(!said.contains(&code), "the agent never prints the code");
    let saved = std::fs::read_to_string(&state).unwrap();
    let token = serde_json::from_str::<serde_json::Value>(&saved).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!said.contains(&token), "nor the token");

    // Reusing the code fails.
    let again = post(
        a,
        galion_instances::API_JOIN,
        &host,
        "",
        "application/json",
        &format!("{{\"code\":\"{code}\"}}"),
    );
    assert_eq!(again.code, 401);

    // Keep reporting (no code) works from the saved state.
    let mut out = Vec::new();
    let rc = galion_instances::agent::main_with(
        vec![
            "--state".into(),
            state.display().to_string(),
            "--once".into(),
        ],
        &mut out,
    );
    assert_eq!(rc, 0, "{}", String::from_utf8_lossy(&out));

    let page = get(a, "/instances", &host, "");
    assert!(page.body.contains("Studio laptop"), "{}", page.body);
    assert!(page.body.contains("Online"));

    let list = get(a, galion_instances::API_LIST, &host, "");
    assert_eq!(list.code, 200);
    assert!(list.body.contains("Studio laptop"));
    assert!(!list.body.contains(&token));

    // Nothing secret on disk.
    let disk = std::fs::read_to_string(dir.join("fleet.json")).unwrap();
    assert!(!disk.contains(&token) && !disk.contains(&code));

    // Remove: the agent is told it was removed (exit 3).
    let id = serde_json::from_str::<serde_json::Value>(&saved).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let gone = post(
        a,
        "/instances",
        &host,
        SAME,
        FORM,
        &format!("op=forget&id={id}"),
    );
    assert_eq!(gone.code, 200);
    let mut out = Vec::new();
    let rc = galion_instances::agent::main_with(
        vec![
            "--state".into(),
            state.display().to_string(),
            "--once".into(),
        ],
        &mut out,
    );
    assert_eq!(rc, 3, "{}", String::from_utf8_lossy(&out));
}

#[test]
fn strangers_get_nothing() {
    let dir = tmp("gate");
    let a = start(&dir, None);
    // A public console host without the admin cookie: no list, no page.
    let host = "console.example.test";
    let r = get(a, galion_instances::API_LIST, host, "");
    assert_eq!(r.code, 401);
    let r = get(a, "/instances", host, "");
    assert_eq!(r.code, 401);
    assert!(!r.body.contains("Your devices"));
    let r = post(a, "/instances", host, SAME, FORM, "op=add");
    assert_eq!(r.code, 401);
    // Device routes need a valid code/token.
    let r = post(
        a,
        galion_instances::API_BEAT,
        host,
        "",
        "application/json",
        r#"{"token":"00"}"#,
    );
    assert_eq!(r.code, 401);
    let r = post(
        a,
        galion_instances::API_JOIN,
        host,
        "",
        "application/json",
        r#"{"code":"AAAA-AAAA"}"#,
    );
    assert_eq!(r.code, 401);
    let r = get(a, galion_instances::API_JOIN, host, "");
    assert_eq!(r.code, 405);
    // The join page itself is public on a console host, and carries the script.
    let r = get(a, "/join", host, "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("Add this device"));
    assert!(r.body.contains("<script src=\"/_ui/join."));
    assert!(r.head.contains("script-src 'self'"));
    let js = r
        .body
        .split("<script src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let j = get(a, &js, host, "");
    assert_eq!(j.code, 200);
    assert!(j.head.to_ascii_lowercase().contains("javascript"));
}

#[test]
fn phone_joins_in_the_browser() {
    let dir = tmp("phone");
    let a = start(&dir, None);
    let host = format!("localhost:{}", a.port());
    let code = code_from(&post(a, "/instances", &host, SAME, FORM, "op=add&name=Phone").body);
    let r = post(
        a,
        "/join",
        &host,
        SAME,
        FORM,
        &format!("code={code}&metrics=%7B%22cores%22%3A6%2C%22ram_total_mb%22%3A4096%7D"),
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let c = r
        .head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("cookie")
        .to_string();
    assert!(c.contains("HttpOnly") && c.contains("Path=/join") && c.contains("SameSite=Strict"));
    let cookie = c
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let b = post(
        a,
        "/join/beat",
        &host,
        &format!("Cookie: {cookie}\r\n"),
        "application/json",
        r#"{"cores":6,"cpu_pct":12}"#,
    );
    assert_eq!(b.code, 200, "{}", b.body);
    let page = get(a, "/instances", &host, "");
    assert!(page.body.contains("Phone"));
    assert!(
        page.body.contains("6 cores") || page.body.contains("6 core"),
        "{}",
        page.body
    );
}

#[test]
fn lan_discovery_finds_the_console() {
    let dir = tmp("lan");
    // An ephemeral-ish UDP port unlikely to clash.
    let port = 47000 + (std::process::id() % 400) as u16;
    let a = start(&dir, Some(port));
    thread::sleep(Duration::from_millis(200));
    let found = galion_instances::lan::ask(
        &[format!("127.0.0.1:{port}").parse().unwrap()],
        Duration::from_millis(800),
    )
    .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    // The answer names the configured HTTP port (--bind, default 8090) and
    // the console name; the test listener is separate.
    let _ = a;
    assert_eq!(found[0].addr.port(), 8090);
    assert_eq!(found[0].name, "Galion console");
}
