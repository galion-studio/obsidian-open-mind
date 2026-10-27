// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the Hermes owner-notify channel (wave-4 slot 266,
//! publish blocker B5), GALION_REAL=1.
//!
//! Runs the REAL `hermes-gateway` binary (`--mock` provider: no network, no
//! spend) and a mock Telegram Bot API server in this process on loopback.
//! No real Telegram call is ever made: the API base is pinned to loopback
//! and the token is a random throwaway made per run, kept in memory only.
//!
//! Ports (slot 266 range 21360-21369), loopback only:
//!   21360 mock Telegram   21361 gateway, no channel   21362 gateway, channel
//!   21363 gateway, bad API base   21364 closed port (transport failure)
//!
//! While curl is talking to the mock, the mock scans every `/proc/*/cmdline`
//! for the token: it must never be on any process's argv.
//!
//! `GALION_REAL=1 cargo test --release -p hermes-gateway --test real_telegram_notify -- --nocapture`

use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HERMES: &str = env!("CARGO_BIN_EXE_hermes-gateway");
const PMOCK: u16 = 21360;
const PNONE: u16 = 21361;
const PTG: u16 = 21362;
const PBAD: u16 = 21363;
const PCLOSED: u16 = 21364;
const KEY: &str = "slot266-real-test-gateway-key";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn ts() -> String {
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%:z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("urandom");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Debug, Clone)]
struct Hit {
    method: String,
    path: String,
    body: Value,
    argv_leak: bool,
}

/// argv of every process on the box, looking for `needle`.
fn any_argv_contains(needle: &str) -> bool {
    let Ok(rd) = fs::read_dir("/proc") else {
        return false;
    };
    rd.flatten().any(|e| {
        fs::read(e.path().join("cmdline"))
            .map(|b| String::from_utf8_lossy(&b).contains(needle))
            .unwrap_or(false)
    })
}

fn read_req(s: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = s.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let first = lines.next()?.to_string();
    let mut parts = first.split(' ');
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Some((method, path, body))
}

/// Mock Bot API: 200 ok, or 429 when the text asks for it.
fn mock_telegram(token: String, hits: Arc<Mutex<Vec<Hit>>>) {
    let l = TcpListener::bind(("127.0.0.1", PMOCK)).expect("bind mock telegram");
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let Some((method, path, body)) = read_req(&mut s) else {
                continue;
            };
            // curl is alive and waiting for our reply right now.
            let argv_leak = any_argv_contains(&token);
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let want_429 = body["text"]
                .as_str()
                .is_some_and(|t| t.contains("please-429"));
            let (code, reply) = if want_429 {
                (
                    "429 Too Many Requests",
                    json!({"ok": false, "error_code": 429, "description": "Too Many Requests: retry after 7", "parameters": {"retry_after": 7}}),
                )
            } else {
                (
                    "200 OK",
                    json!({"ok": true, "result": {"message_id": 1, "chat": {"id": body["chat_id"]}}}),
                )
            };
            hits.lock().unwrap().push(Hit {
                method,
                path,
                body,
                argv_leak,
            });
            let r = reply.to_string();
            let _ = write!(
                s,
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{r}",
                r.len()
            );
        }
    });
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>, auth: bool) -> (u16, Value) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, Value::Null);
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let b = body.unwrap_or("");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if auth {
        req.push_str(&format!("Authorization: Bearer {KEY}\r\n"));
    }
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{b}", b.len()));
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    let (head, rest) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (
        code,
        serde_json::from_str(rest.trim()).unwrap_or(Value::Null),
    )
}

fn wait_up(port: u16) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if http(port, "GET", "/health", None, false).0 == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct Gw {
    child: Child,
    log: PathBuf,
}

impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn base_cmd(work: &Path, port: u16, extra: &[(&str, String)]) -> Command {
    let home = work.join(format!("home-{port}"));
    fs::create_dir_all(&home).unwrap();
    let mut c = Command::new(HERMES);
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("HERMES_HOME", &home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env("API_SERVER_KEY", KEY);
    for (k, v) in extra {
        c.env(k, v);
    }
    c
}

fn start(work: &Path, port: u16, extra: &[(&str, String)]) -> Gw {
    let log = work.join(format!("gw-{port}.log"));
    let f = fs::File::create(&log).unwrap();
    let child = base_cmd(work, port, extra)
        .arg("--mock")
        .stdout(Stdio::from(f.try_clone().unwrap()))
        .stderr(Stdio::from(f))
        .spawn()
        .expect("spawn hermes-gateway");
    assert!(wait_up(port), "gateway :{port} up");
    Gw { child, log }
}

fn cli(
    work: &Path,
    extra: &[(&str, String)],
    args: &[&str],
    stdin: Option<&str>,
) -> (i32, String, String) {
    let mut c = base_cmd(work, 21369, extra);
    c.arg("notify").args(args);
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut ch = c.spawn().expect("spawn cli");
    {
        let mut si = ch.stdin.take().unwrap();
        if let Some(t) = stdin {
            si.write_all(t.as_bytes()).unwrap();
        }
    }
    let o = ch.wait_with_output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).trim().to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

#[test]
fn real_telegram_notify() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let t_start = ts();
    let work = std::env::temp_dir().join(format!("slot266-notify-{}", rand_hex(4)));
    fs::create_dir_all(&work).unwrap();
    // Throwaway token, Bot API shape; never printed.
    let token = format!(
        "{}:{}",
        u64::from_str_radix(&rand_hex(4), 16).unwrap() + 100_000,
        rand_hex(18)
    );
    let secret_half = token.split_once(':').unwrap().1.to_string();
    // Control: the argv scanner really sees a live process's argv.
    let marker = format!("slot266-argv-probe-{}", rand_hex(6));
    let mut probe = Command::new("sh")
        .args(["-c", "sleep 5; true", &marker])
        .spawn()
        .expect("sh");
    std::thread::sleep(Duration::from_millis(100));
    assert!(any_argv_contains(&marker), "argv scanner control");
    let _ = probe.kill();
    let _ = probe.wait();
    let hits = Arc::new(Mutex::new(Vec::<Hit>::new()));
    mock_telegram(token.clone(), hits.clone());
    let tg = |base: String, per_hour: &str| {
        vec![
            ("TELEGRAM_BOT_TOKEN", token.clone()),
            ("TELEGRAM_CHAT_ID", "-1001234567".to_string()),
            ("HERMES_RS_TELEGRAM_API_BASE", base),
            ("HERMES_RS_NOTIFY_PER_HOUR", per_hour.to_string()),
        ]
    };
    let mut out_all = String::new();
    let mut report = Vec::<String>::new();

    // 1. no channel: logged no-op
    let g0 = start(&work, PNONE, &[]);
    let (c, s) = http(PNONE, "GET", "/v1/notify/status", None, true);
    assert_eq!(
        (c, s["channel"].as_str()),
        (200, Some("no-channel-configured"))
    );
    let (c, r) = http(
        PNONE,
        "POST",
        "/v1/notify",
        Some(r#"{"kind":"proposal","text":"hoff-23 awaits Mate"}"#),
        true,
    );
    assert_eq!(
        (c, r["notify"].as_str()),
        (200, Some("no-channel-configured")),
        "{r}"
    );
    let (c, _) = http(PNONE, "POST", "/v1/notify", Some(r#"{"text":"x"}"#), false);
    assert_eq!(c, 401, "notify needs the gateway key");
    let (c, _) = http(PNONE, "POST", "/v1/notify", Some(r#"{"kind":"x"}"#), true);
    assert_eq!(c, 400, "text required");
    std::thread::sleep(Duration::from_millis(200));
    let l0 = fs::read_to_string(&g0.log).unwrap();
    assert!(l0.contains("notify: \"no-channel-configured\""), "{l0}");
    assert!(
        l0.contains("kind=proposal chars=19 outcome=no-channel-configured (no-op"),
        "{l0}"
    );
    assert!(!l0.contains("hoff-23"), "text never logged");
    assert!(hits.lock().unwrap().is_empty());
    report.push(format!(
        "no-channel: status={s} post={r} unauth=401 missing-text=400 mock_hits=0"
    ));
    out_all.push_str(&l0);
    drop(g0);

    // 2. channel to the loopback mock
    let g1 = start(&work, PTG, &tg(format!("http://127.0.0.1:{PMOCK}"), "3"));
    let (_, s) = http(PTG, "GET", "/v1/notify/status", None, true);
    assert_eq!(s["channel"], "telegram");
    assert_eq!(s["api_base_default"], false);
    let t0 = Instant::now();
    let (c, r) = http(
        PTG,
        "POST",
        "/v1/notify",
        Some(r#"{"kind":"proposal","text":"hoff-23 \"cloudflared flood\" awaits Mate\nline 2"}"#),
        true,
    );
    let ms = t0.elapsed().as_millis();
    assert_eq!(
        (c, r["notify"].as_str()),
        (200, Some("telegram-sent")),
        "{r}"
    );
    {
        let h = hits.lock().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].method, "POST");
        assert_eq!(h[0].path, format!("/bot{token}/sendMessage"));
        assert_eq!(h[0].body["chat_id"], "-1001234567");
        assert_eq!(
            h[0].body["text"],
            "[hermes] hoff-23 \"cloudflared flood\" awaits Mate\nline 2"
        );
        assert_eq!(h[0].body["disable_web_page_preview"], true);
        assert!(!h[0].argv_leak, "token on some process argv");
    }
    report.push(format!("channel: sent in {ms} ms, mock got POST /bot<token>/sendMessage chat_id=-1001234567, argv_leak=false"));
    let (_, r429) = http(
        PTG,
        "POST",
        "/v1/notify",
        Some(r#"{"text":"please-429"}"#),
        true,
    );
    assert_eq!(r429["notify"], "telegram-failed");
    assert!(
        r429["detail"]
            .as_str()
            .unwrap()
            .contains("http 429 Too Many Requests: retry after 7 retry_after=7s"),
        "{r429}"
    );
    let (_, r3) = http(PTG, "POST", "/v1/notify", Some(r#"{"text":"third"}"#), true);
    assert_eq!(r3["notify"], "telegram-sent");
    let (_, r4) = http(
        PTG,
        "POST",
        "/v1/notify",
        Some(r#"{"text":"fourth"}"#),
        true,
    );
    assert_eq!(r4["notify"], "rate-limited", "per-hour cap 3");
    assert_eq!(hits.lock().unwrap().len(), 3, "4th never left the box");
    assert!(hits.lock().unwrap().iter().all(|h| !h.argv_leak));
    let (_, s) = http(PTG, "GET", "/v1/notify/status", None, true);
    assert_eq!(s["sent_last_hour"], 3);
    report.push(format!("429: {r429}; 3rd={r3}; 4th={r4}; status={s}"));
    std::thread::sleep(Duration::from_millis(200));
    out_all.push_str(&fs::read_to_string(&g1.log).unwrap());
    out_all.push_str(&r429.to_string());
    drop(g1);

    // 3. API base outside loopback / api.telegram.org: refused, nothing sent
    let g2 = start(&work, PBAD, &tg("https://evil.example".into(), "3"));
    let (_, s) = http(PBAD, "GET", "/v1/notify/status", None, true);
    assert_eq!(s["channel"], "bad-api-base");
    let (_, r) = http(PBAD, "POST", "/v1/notify", Some(r#"{"text":"x"}"#), true);
    assert_eq!(r["notify"], "bad-api-base");
    assert_eq!(hits.lock().unwrap().len(), 3);
    report.push(format!("bad-api-base: status={s} post={r}"));
    out_all.push_str(&fs::read_to_string(&g2.log).unwrap());
    drop(g2);

    // 4. CLI
    let env_ok = tg(format!("http://127.0.0.1:{PMOCK}"), "6");
    let (code, out, err) = cli(
        &work,
        &env_ok,
        &["--kind", "cli", "--text", "cli says hi"],
        None,
    );
    assert_eq!(
        (code, out.as_str()),
        (0, r#"{"notify":"telegram-sent"}"#),
        "{err}"
    );
    let (code2, out2, err2) = cli(&work, &env_ok, &["--kind", "stdin"], Some("from stdin\n"));
    assert_eq!(
        (code2, out2.as_str()),
        (0, r#"{"notify":"telegram-sent"}"#),
        "{err2}"
    );
    {
        let h = hits.lock().unwrap();
        assert_eq!(h.len(), 5);
        assert_eq!(h[3].body["text"], "[hermes] cli says hi");
        assert_eq!(h[4].body["text"], "[hermes] from stdin");
        assert!(h.iter().all(|x| !x.argv_leak));
    }
    let (code3, out3, err3) = cli(&work, &[], &["--text", "nobody"], None);
    assert_eq!(
        (code3, out3.as_str()),
        (0, r#"{"notify":"no-channel-configured"}"#)
    );
    assert!(err3.contains("outcome=no-channel-configured"));
    let (code4, out4, err4) = cli(
        &work,
        &tg(format!("http://127.0.0.1:{PCLOSED}"), "6"),
        &["--text", "down"],
        None,
    );
    assert_eq!(code4, 1);
    let v4: Value = serde_json::from_str(&out4).unwrap();
    assert_eq!(v4["notify"], "telegram-failed");
    let (code5, _, _) = cli(&work, &[], &["--bogus"], None);
    assert_eq!(code5, 2);
    report.push(format!("cli: --text exit {code} {out}; stdin exit {code2} {out2}; no-env exit {code3} {out3}; closed port exit {code4} {out4}; bad arg exit {code5}"));
    out_all.push_str(&[out, err, out2, err2, out3, err3, out4, err4].concat());

    // 5. the token never surfaced anywhere we can see
    assert!(
        !out_all.contains(&token) && !out_all.contains(&secret_half),
        "token leaked to logs/stdout"
    );
    let _ = fs::remove_dir_all(&work);
    for p in [PNONE, PTG, PBAD] {
        assert!(
            TcpStream::connect(("127.0.0.1", p)).is_err(),
            "gateway :{p} stopped"
        );
    }
    eprintln!("REAL slot266 telegram notify {t_start} .. {}", ts());
    for l in report {
        eprintln!("  {l}");
    }
    eprintln!("  token_in_logs_or_stdout=false mock_hits_total=5");
}
