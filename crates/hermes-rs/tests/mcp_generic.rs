// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Generic MCP client: config file, secrets file, env passing, writes off,
//! allowlist, caps, lazy, deny-list class, Streamable HTTP and legacy SSE.

use hermes_rs::config::{merge_mcp_servers, McpClass, McpServerCfg, McpTransport};
use hermes_rs::tools::{Registry, Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn call(t: &Arc<dyn Tool>, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = ToolCtx {
        session_id: "s",
        spend_session: "s",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &|_| {},
    };
    t.call(&args, &ctx)
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "hermes-mcp-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn secret_file(dir: &std::path::Path, body: &str, mode: u32) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("s.env");
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    p.display().to_string()
}

fn fake2(dir: &std::path::Path) -> McpServerCfg {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-mcp2.sh");
    McpServerCfg {
        name: "fake2".into(),
        command: "bash".into(),
        args: vec![script.into()],
        timeout_s: 5,
        connect_timeout_s: 5,
        env: [(
            "START_LOG".to_string(),
            dir.join("starts").display().to_string(),
        )]
        .into(),
        ..McpServerCfg::default()
    }
}

#[test]
fn write_tools_and_trusted_descriptions_parse() {
    let v = json!({"mcp_servers": {
        "g": {"command": "x", "write_tools": "gated", "trusted_descriptions": true},
        "bad": {"command": "x", "write_tools": "maybe"}
    }});
    let mut l = Vec::new();
    let w = merge_mcp_servers(&mut l, &v, "t");
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].write_tools, hermes_rs::config::McpWriteTools::Gated);
    assert!(l[0].trusted_descriptions && !l[0].allow_writes);
    assert!(
        w.iter()
            .any(|x| x.contains("bad") && x.contains("write_tools")),
        "{w:?}"
    );
}

#[test]
fn config_file_overrides_and_loud_warnings() {
    let base = hermes_rs::yaml::parse(
        "mcp_servers:\n  openmind:\n    command: /bin/old\n    args: [\"--key-file\", \"/x/.env\"]\n  remote:\n    url: https://example.com/mcp\n  broken:\n    args: [\"x\"]\n",
    )
    .unwrap();
    let mut list = Vec::new();
    let w = merge_mcp_servers(&mut list, &base, "config.yaml");
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("NOT LOADED") && w[0].contains("broken"),
        "{w:?}"
    );
    let remote = list.iter().find(|m| m.name == "remote").unwrap();
    assert_eq!(remote.transport, McpTransport::Http, "url entries load now");
    let file = hermes_rs::yaml::parse(
        r#"mcp_servers:
  openmind:
    command: /bin/new
    env_file: /etc/hermes-rs/mcp.d/openmind.env
    allow_tools: [memory_search, "vault_*"]
    allow_writes: true
    lazy: true
    timeout: 20
    max_result_chars: 1000
  remote:
    enabled: false
  gh:
    url: https://api.githubcopilot.com/mcp/readonly
    headers: {Authorization: "Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}"}
    class: read
  inline:
    command: x
    env: {GITHUB_PERSONAL_ACCESS_TOKEN: "placeholder-not-a-real-value"}
  inlinehdr:
    url: https://x/mcp
    headers: {Authorization: "Bearer abc"}
  badclass:
    command: x
    class: root
"#,
    )
    .unwrap();
    let w = merge_mcp_servers(&mut list, &file, "mcp-servers.yaml");
    assert_eq!(w.len(), 3, "{w:?}");
    assert!(w.iter().any(|x| x.contains("inline secret")));
    assert!(w.iter().any(|x| x.contains("inline Authorization")));
    assert!(w.iter().any(|x| x.contains("unknown class")));
    let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["openmind", "gh"]);
    let om = &list[0];
    assert_eq!(om.command, "/bin/new");
    assert!(om.args.is_empty(), "no more --key-file .env");
    assert!(om.lazy && om.allow_writes && om.timeout_s == 20);
    assert_eq!(om.max_result_chars, 1000);
    assert_eq!(om.source, "mcp-servers.yaml");
    assert!(!list[1].allow_writes, "writes default off");
    assert_eq!(list[1].class, McpClass::Read);
}

#[test]
fn servers_file_via_env_and_run_as_default() {
    let d = tmpdir("cfgenv");
    let f = d.join("servers.yaml");
    std::fs::write(&f, "mcp_servers:\n  a:\n    command: /bin/true\n  b:\n    command: /bin/true\n    run_as: \"1234:1234\"\n").unwrap();
    let mut c = hermes_rs::config::Config::defaults(std::path::Path::new("/h"));
    let fs = f.display().to_string();
    let env = |k: &str| match k {
        "HERMES_RS_MCP_SERVERS_FILE" => Some(fs.clone()),
        "HERMES_RS_MCP_RUN_AS" => Some("999:999".into()),
        _ => None,
    };
    c.apply_env(&env).unwrap();
    assert_eq!(c.mcp_servers.len(), 2);
    assert_eq!(c.mcp_servers[0].run_as.as_deref(), Some("999:999"));
    assert_eq!(c.mcp_servers[1].run_as.as_deref(), Some("1234:1234"));
    let mut c = hermes_rs::config::Config::defaults(std::path::Path::new("/h"));
    let env = |k: &str| (k == "HERMES_RS_MCP_SERVERS_FILE").then(|| "/nonexistent/x.yaml".into());
    c.apply_env(&env).unwrap();
    assert!(
        c.mcp_warnings[0].contains("NOT LOADED"),
        "{:?}",
        c.mcp_warnings
    );
}

#[test]
fn stdio_env_secrets_writes_off_caps_lazy_ping() {
    let d = tmpdir("stdio");
    let mut cfg = fake2(&d);
    cfg.env_file = Some(secret_file(
        &d,
        "# test\nFAKE_TOKEN=tok-abcdefghijkl-123\nOTHER_SECRET=zzzzzzzzzzzz\n",
        0o600,
    ));
    cfg.pass_env = Some(vec!["FAKE_TOKEN".into()]);
    cfg.require_env = vec!["FAKE_TOKEN".into()];
    cfg.lazy = true;
    cfg.max_result_chars = 1000;
    let (tools, reps) = hermes_rs::mcp::discover_report(std::slice::from_ref(&cfg));
    assert!(reps[0].error.is_none(), "{reps:?}");
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    // create_thing is a write tool: off by default
    assert_eq!(
        names,
        [
            "mcp__fake2__env_names",
            "mcp__fake2__secret_echo",
            "mcp__fake2__big",
            "mcp__fake2__pinger"
        ]
    );
    assert!(reps[0]
        .skipped
        .iter()
        .any(|(n, w)| n == "create_thing" && w.contains("allow_writes")));
    // descriptions: untrusted label, control chars stripped
    let desc = tools[0].description();
    assert!(desc.starts_with("[MCP server fake2; untrusted description]"));
    assert!(!desc.contains('\u{1b}'), "{desc:?}");
    let starts = || {
        std::fs::read_to_string(d.join("starts"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    assert_eq!(starts(), 1);
    // lazy: stopped after discovery, started again on the first call
    let o = call(&tools[0], json!({}));
    assert!(!o.is_error, "{o:?}");
    assert_eq!(starts(), 2);
    // only the passed secret + base env reach the child; no gateway env
    let envs = o.content.split('\n').nth(1).unwrap().to_string();
    let list = envs.split(' ').next().unwrap();
    let mut got: Vec<&str> = list.split(',').filter(|s| !s.is_empty()).collect();
    got.retain(|n| !matches!(*n, "LANG" | "TZ"));
    assert_eq!(got, ["FAKE_TOKEN", "HOME", "PATH", "START_LOG"], "{envs}");
    assert!(envs.contains("cwd=/ ") || envs.ends_with("cwd=/"), "{envs}");
    // the secret value never comes back to the model
    let o = call(&tools[1], json!({}));
    assert!(o.content.contains("token is [REDACTED]"), "{o:?}");
    assert!(!o.content.contains("abcdefghijkl"));
    // result cap
    let o = call(&tools[2], json!({}));
    assert!(o.content.len() < 1200 && o.content.contains("[…truncated]"));
    // server -> client ping is answered
    let o = call(&tools[3], json!({}));
    assert!(o.content.ends_with("pong-ok"), "{o:?}");
    assert_eq!(starts(), 2, "stays up after the first call");
    // writes on: create_thing registers
    let mut w = cfg.clone();
    w.allow_writes = true;
    w.allow_tools = Some(vec!["create_*".into()]);
    let (tools, _) = hermes_rs::mcp::discover_report(&[w]);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name(), "mcp__fake2__create_thing");
    // write_tools: gated -> registered with its name, refused at call time
    let mut g = cfg.clone();
    g.write_tools = hermes_rs::config::McpWriteTools::Gated;
    g.allow_tools = Some(vec!["create_*".into(), "env_names".into()]);
    let (tools, reps) = hermes_rs::mcp::discover_report(&[g]);
    assert_eq!(tools.len(), 2);
    assert_eq!(reps[0].gated, vec!["mcp__fake2__create_thing".to_string()]);
    let ct = tools
        .iter()
        .find(|t| t.name() == "mcp__fake2__create_thing")
        .unwrap();
    let before = starts();
    let o = call(ct, json!({}));
    assert!(o.is_error && o.content.contains("writes are OFF"), "{o:?}");
    assert!(o.content.contains("Nothing was sent"));
    let _ = before;
    assert!(ct
        .description()
        .starts_with("[MCP server fake2; untrusted description]"));
}

#[test]
fn secrets_file_must_be_private_and_present() {
    let d = tmpdir("perm");
    let mut cfg = fake2(&d);
    cfg.env_file = Some(secret_file(&d, "FAKE_TOKEN=abcdefghijklmnop\n", 0o644));
    let (_t, reps) = hermes_rs::mcp::discover_report(std::slice::from_ref(&cfg));
    let e = reps[0].error.clone().unwrap();
    assert!(e.contains("mode 644"), "{e}");
    assert!(!e.contains("abcdefghijklmnop"));
    // missing token file: waiting, not started
    let mut w = fake2(&d);
    w.env_file = Some(d.join("nope.env").display().to_string());
    let (_t, reps) = hermes_rs::mcp::discover_report(&[w]);
    assert!(reps[0].error.as_deref().unwrap().contains("secrets file"));
    let mut w = fake2(&d);
    w.env_file = Some(secret_file(&d, "OTHER=1\n", 0o600));
    w.require_env = vec!["GITHUB_PERSONAL_ACCESS_TOKEN".into()];
    let (_t, reps) = hermes_rs::mcp::discover_report(&[w]);
    let e = reps[0].error.clone().unwrap();
    assert!(
        e.contains("waiting for secret(s) GITHUB_PERSONAL_ACCESS_TOKEN"),
        "{e}"
    );
    assert!(
        std::fs::read_to_string(d.join("starts")).is_err(),
        "never started"
    );
    // oauth is a clear stub
    let mut o = fake2(&d);
    o.auth = Some("oauth".into());
    let (_t, reps) = hermes_rs::mcp::discover_report(&[o]);
    assert!(reps[0].error.as_deref().unwrap().contains("TODO"));
}

#[test]
fn deny_list_uses_the_server_class() {
    let d = tmpdir("deny");
    let mut cfg = fake2(&d);
    cfg.allow_tools = Some(vec!["env_names".into()]);
    let (tools, _) = hermes_rs::mcp::discover_report(std::slice::from_ref(&cfg));
    let mut reg = Registry::new();
    reg.add(tools[0].clone());
    let t = reg.get("mcp__fake2__env_names").unwrap();
    assert_eq!(t.deny_name(), "mcp__fake2__env_names");
    // read class: a harmless call passes
    assert!(!call(&t, json!({})).is_error);
    // a command string to any MCP tool still gets the command checks
    let o = call(&t, json!({"command": "rm -rf /opt"}));
    assert!(o.content.contains("deny list"), "{o:?}");
    // shell class: no inspectable command -> refused before the server runs
    let mut sh = cfg.clone();
    sh.class = McpClass::Shell;
    sh.allow_writes = true;
    let (tools, _) = hermes_rs::mcp::discover_report(&[sh]);
    let mut reg = Registry::new();
    reg.add(tools[0].clone());
    let t = reg.get("mcp__fake2__env_names").unwrap();
    assert!(t.deny_name().ends_with("__shell"));
    let o = call(&t, json!({}));
    assert!(
        o.is_error && o.content.contains("S9-shell-no-command"),
        "{o:?}"
    );
    // write class: protected files are refused as write targets
    let mut wr = cfg.clone();
    wr.class = McpClass::Write;
    wr.allow_writes = true;
    let (tools, _) = hermes_rs::mcp::discover_report(&[wr]);
    let mut reg = Registry::new();
    reg.add(tools[0].clone());
    let t = reg.get("mcp__fake2__env_names").unwrap();
    let o = call(&t, json!({"path": "/opt/galion/hermes/SOUL.md"}));
    assert!(o.is_error && o.content.contains("deny list"), "{o:?}");
}

// ------------------------------------------------------- fake HTTP servers

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn read_req(s: &mut BufReader<TcpStream>) -> Option<Req> {
    let mut line = String::new();
    s.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut l = String::new();
        s.read_line(&mut l).ok()?;
        let l = l.trim_end().to_string();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    s.read_exact(&mut body).ok()?;
    Some(Req {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into(),
    })
}

fn hdr<'a>(r: &'a Req, k: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
}

fn tools_result() -> Value {
    json!({"tools": [
        {"name": "echo", "description": "Echo", "annotations": {"readOnlyHint": true},
         "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}},
        {"name": "delete_everything", "description": "nope"}
    ]})
}

/// Streamable HTTP fake: bearer auth, session ids, SSE and JSON replies,
/// one forced session expiry.
fn http_server(expire_once: Arc<AtomicBool>, inits: Arc<AtomicUsize>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let expire_once = expire_once.clone();
            let inits = inits.clone();
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut rd = BufReader::new(s);
                let Some(r) = read_req(&mut rd) else { return };
                let send = |w: &mut TcpStream,
                            status: &str,
                            extra: &str,
                            ctype: &str,
                            body: &str| {
                    let _ = write!(
                        w,
                        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                        body.len()
                    );
                };
                if r.method != "POST" || r.path != "/mcp" {
                    return send(&mut w, "405 Method Not Allowed", "", "text/plain", "no");
                }
                if hdr(&r, "authorization") != Some("Bearer tok-http-123456") {
                    return send(
                        &mut w,
                        "401 Unauthorized",
                        "WWW-Authenticate: Bearer\r\n",
                        "text/plain",
                        "auth",
                    );
                }
                let v: Value = serde_json::from_str(&r.body).unwrap();
                let id = v["id"].clone();
                let m = v["method"].as_str().unwrap_or("");
                if m == "initialize" {
                    let n = inits.fetch_add(1, Ordering::SeqCst) + 1;
                    let b = json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": "2025-03-26",
                                   "capabilities": {"tools": {}}, "serverInfo": {"name": "fakehttp", "version": "1"}}});
                    return send(
                        &mut w,
                        "200 OK",
                        &format!("Mcp-Session-Id: sess-{n}\r\n"),
                        "application/json",
                        &b.to_string(),
                    );
                }
                let Some(sid) = hdr(&r, "mcp-session-id").map(String::from) else {
                    return send(&mut w, "400 Bad Request", "", "text/plain", "no session");
                };
                if id.is_null() {
                    return send(&mut w, "202 Accepted", "", "text/plain", "");
                }
                if m == "tools/call" && expire_once.swap(false, Ordering::SeqCst) {
                    return send(&mut w, "404 Not Found", "", "text/plain", "gone");
                }
                if m == "tools/list" {
                    // SSE reply that stays open after the answer
                    let _ = write!(w, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n");
                    let note =
                        json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {}});
                    let res = json!({"jsonrpc": "2.0", "id": id, "result": tools_result()});
                    let _ = write!(w, ": keepalive\n\nevent: message\ndata: {note}\n\nevent: message\ndata: {res}\n\n");
                    let _ = w.flush();
                    std::thread::sleep(Duration::from_secs(8));
                    return;
                }
                let text = format!(
                    "echo:{} session={sid} proto={}",
                    v["params"]["arguments"]["text"].as_str().unwrap_or(""),
                    hdr(&r, "mcp-protocol-version").unwrap_or("-")
                );
                let b = json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}]}});
                send(&mut w, "200 OK", "", "application/json", &b.to_string())
            });
        }
    });
    format!("http://127.0.0.1:{}/mcp", addr.port())
}

#[test]
fn streamable_http_session_sse_auth_and_expiry() {
    let d = tmpdir("http");
    let expire = Arc::new(AtomicBool::new(false));
    let inits = Arc::new(AtomicUsize::new(0));
    let url = http_server(expire.clone(), inits.clone());
    let cfg = McpServerCfg {
        name: "fakehttp".into(),
        url: Some(url.clone()),
        transport: McpTransport::Http,
        env_file: Some(secret_file(&d, "TOK=tok-http-123456\n", 0o600)),
        headers: [("Authorization".to_string(), "Bearer ${TOK}".to_string())].into(),
        timeout_s: 5,
        connect_timeout_s: 5,
        ..McpServerCfg::default()
    };
    let t0 = std::time::Instant::now();
    let (tools, reps) = hermes_rs::mcp::discover_report(std::slice::from_ref(&cfg));
    assert!(reps[0].error.is_none(), "{reps:?}");
    assert!(
        t0.elapsed() < Duration::from_secs(4),
        "SSE reply read stops at the answer, not at stream end"
    );
    assert_eq!(tools.len(), 1, "delete_everything is off (writes off)");
    assert_eq!(tools[0].name(), "mcp__fakehttp__echo");
    let o = call(&tools[0], json!({"text": "hi"}));
    assert!(!o.is_error, "{o:?}");
    assert!(
        o.content
            .ends_with("echo:hi session=sess-1 proto=2025-03-26"),
        "{o:?}"
    );
    // the server forgets the session once: client re-initialises and retries
    expire.store(true, Ordering::SeqCst);
    let o = call(&tools[0], json!({"text": "again"}));
    assert!(
        o.content
            .ends_with("echo:again session=sess-2 proto=2025-03-26"),
        "{o:?}"
    );
    assert_eq!(inits.load(Ordering::SeqCst), 2);
    // wrong token: clear error, token not echoed
    let mut bad = cfg.clone();
    bad.env_file = Some(secret_file(&d, "TOK=wrong-token-value-x\n", 0o600));
    let (_t, reps) = hermes_rs::mcp::discover_report(&[bad]);
    let e = reps[0].error.clone().unwrap();
    assert!(e.contains("unauthorized (HTTP 401)"), "{e}");
    assert!(!e.contains("wrong-token-value-x"));
}

/// Legacy HTTP+SSE fake: GET /sse streams `endpoint` then replies;
/// POST /messages answers 202 and pushes the reply onto the stream.
fn sse_server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let streams: Arc<Mutex<Vec<Sender<String>>>> = Arc::new(Mutex::new(Vec::new()));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let streams = streams.clone();
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut rd = BufReader::new(s);
                let Some(r) = read_req(&mut rd) else { return };
                if r.method == "GET" && r.path == "/sse" {
                    let (tx, rx) = channel::<String>();
                    streams.lock().unwrap().push(tx);
                    let _ = write!(w, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\nevent: endpoint\ndata: /messages?session=1\n\n");
                    let _ = w.flush();
                    for msg in rx {
                        if write!(w, "event: message\ndata: {msg}\n\n").is_err() {
                            break;
                        }
                        let _ = w.flush();
                    }
                    return;
                }
                let _ = write!(w, "HTTP/1.1 202 Accepted\r\nContent-Length: 8\r\nConnection: close\r\n\r\nAccepted");
                let v: Value = serde_json::from_str(&r.body).unwrap_or(json!({}));
                if v["id"].is_null() {
                    return;
                }
                let id = v["id"].clone();
                let res = match v["method"].as_str().unwrap_or("") {
                    "initialize" => {
                        json!({"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "fakesse", "version": "1"}})
                    }
                    "tools/list" => tools_result(),
                    _ => {
                        json!({"content": [{"type": "text", "text": format!("sse-echo:{}", v["params"]["arguments"]["text"].as_str().unwrap_or(""))}]})
                    }
                };
                let msg = json!({"jsonrpc": "2.0", "id": id, "result": res}).to_string();
                for tx in streams.lock().unwrap().iter() {
                    let _ = tx.send(msg.clone());
                }
            });
        }
    });
    format!("http://127.0.0.1:{}/sse", addr.port())
}

#[test]
fn legacy_sse_transport() {
    let url = sse_server();
    let cfg = McpServerCfg {
        name: "fakesse".into(),
        url: Some(url),
        transport: McpTransport::Sse,
        timeout_s: 5,
        connect_timeout_s: 5,
        ..McpServerCfg::default()
    };
    let (tools, reps) = hermes_rs::mcp::discover_report(&[cfg]);
    assert!(reps[0].error.is_none(), "{reps:?}");
    assert_eq!(tools.len(), 1);
    let o = call(&tools[0], json!({"text": "yo"}));
    assert!(o.content.ends_with("sse-echo:yo"), "{o:?}");
}

#[test]
fn plain_http_to_a_remote_host_is_refused() {
    let cfg = McpServerCfg {
        name: "remote".into(),
        url: Some("http://example.invalid/mcp".into()),
        transport: McpTransport::Http,
        ..McpServerCfg::default()
    };
    let (_t, reps) = hermes_rs::mcp::discover_report(&[cfg]);
    assert!(reps[0].error.as_deref().unwrap().contains("loopback"));
}
