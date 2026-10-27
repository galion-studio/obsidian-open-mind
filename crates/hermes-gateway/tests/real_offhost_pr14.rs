// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Wave-4 slot 306: real-binary review test for PR #14 (hermes-offhost).
//!
//! Gated: runs only with GALION_REAL=1. Uses the release hermes-gateway
//! binary from HERMES_GW_BIN (else target/x86_64-unknown-linux-musl/release).
//! Ports 21760-21769 only. Every secret here is a fake built at run time.
//! Prints FINDING lines instead of failing on review findings, and asserts
//! the behaviour PR #14 itself claims.

use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const P_GW: u16 = 21760;
const P_STANDBY: u16 = 21761;
const P_STORE: u16 = 21762;
const P_PUBLIC: u16 = 21763;
const KEY: &str = "pr14-review-gateway-key";

fn on() -> bool {
    std::env::var("GALION_REAL").ok().as_deref() == Some("1")
}

fn bin() -> PathBuf {
    if let Ok(b) = std::env::var("HERMES_GW_BIN") {
        return PathBuf::from(b);
    }
    let t = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "../../target".into());
    PathBuf::from(t).join("x86_64-unknown-linux-musl/release/hermes-gateway")
}

fn fake_pat() -> String {
    format!("{}{}", "github_pat_", "Q".repeat(40))
}
fn fake_sk() -> String {
    format!("{}{}", "sk-", "R".repeat(40))
}

#[derive(Default)]
struct Store {
    body: Option<Vec<u8>>,
    serve: Option<Vec<u8>>,
    auth: Vec<String>,
    puts: u32,
    gets: u32,
}

type Req = (String, Vec<(String, String)>, Vec<u8>);

fn read_req(s: &mut TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut one = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        if s.read(&mut one).ok()? == 0 {
            return None;
        }
        buf.push(one[0]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next()?.to_string();
    let hdrs: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len: usize = hdrs
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if hdrs
        .iter()
        .any(|(k, v)| k == "expect" && v.eq_ignore_ascii_case("100-continue"))
    {
        let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).ok()?;
    Some((first, hdrs, body))
}

fn spawn_store(st: Arc<Mutex<Store>>) {
    let l = TcpListener::bind(("127.0.0.1", P_STORE)).expect("store bind");
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut c = c;
            let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
            let Some((first, hdrs, body)) = read_req(&mut c) else {
                continue;
            };
            let mut g = st.lock().unwrap();
            if let Some((_, v)) = hdrs.iter().find(|(k, _)| k == "authorization") {
                g.auth
                    .push(v.split_whitespace().next().unwrap_or("").to_string());
            }
            let resp: Vec<u8> = if first.starts_with("PUT ") {
                g.puts += 1;
                g.body = Some(body);
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec()
            } else {
                g.gets += 1;
                match g.serve.clone().or_else(|| g.body.clone()) {
                    Some(b) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            b.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(&b);
                        r
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                }
            };
            drop(g);
            let _ = c.write_all(&resp);
        }
    });
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    auth: bool,
    body: Option<&Value>,
    sid: Option<&str>,
) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n", b.len());
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
    }
    if auth {
        req.push_str(&format!("Authorization: Bearer {KEY}\r\n"));
    }
    if let Some(id) = sid {
        req.push_str(&format!("X-Hermes-Session-Id: {id}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&b);
    let _ = s.write_all(req.as_bytes());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (code, body)
}

struct Gw(Child);
impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn base_cmd(home: &Path, port: u16) -> Command {
    let mut c = Command::new(bin());
    c.env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", home.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", home.join("rs-cron-state.json"))
        .env("API_SERVER_KEY", KEY)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn start(mut c: Command, port: u16) -> Gw {
    let child = c.arg("--mock").spawn().expect("spawn gateway");
    let g = Gw(child);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(15) {
        if http(port, "GET", "/health", false, None, None).0 == 200 {
            return g;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("gateway on {port} did not come up");
}

fn stop(mut g: Gw) -> String {
    let _ = g.0.kill();
    let mut err = String::new();
    if let Some(mut e) = g.0.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let _ = g.0.wait();
    err
}

fn once(home: &Path, envs: &[(&str, String)]) -> (i32, String, String) {
    let mut c = base_cmd(home, P_GW);
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.arg("--offhost-once").output().expect("once");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn snapshot_files(snap: &Value) -> Vec<Value> {
    snap["files"].as_array().cloned().unwrap_or_default()
}

fn decoded_contains(entry: &Value, needle: &str) -> bool {
    if let Some(t) = entry["text"].as_str() {
        return t.contains(needle);
    }
    let Some(b) = entry["b64"].as_str() else {
        return false;
    };
    let bytes = hermes_b64(b);
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn hermes_b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    };
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for ch in clean.chunks(4) {
        let pad = ch.iter().filter(|&&c| c == b'=').count();
        let n = ch.iter().fold(0u32, |n, &c| (n << 6) | val(c));
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    out
}

fn sha_hex(b: &[u8]) -> String {
    // sha256 via coreutils (not Python): the gateway uses its own sha256.
    let mut c = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(b).unwrap();
    let o = c.wait_with_output().unwrap();
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

fn mode(p: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| format!("{:o}", m.permissions().mode() & 0o777))
        .unwrap_or("-".into())
}

fn age(p: &Path) {
    let _ = Command::new("touch")
        .args(["-d", "@1000000000"])
        .arg(p)
        .status();
}

fn chat(port: u16, sid: &str, text: &str) -> u16 {
    let body = json!({"model": "mock", "messages": [{"role": "user", "content": text}]});
    http(
        port,
        "POST",
        "/v1/chat/completions",
        true,
        Some(&body),
        Some(sid),
    )
    .0
}

#[test]
fn real_offhost_pr14_review() {
    if !on() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    assert!(bin().exists(), "missing binary {}", bin().display());
    let root = std::env::temp_dir().join(format!("pr14-review-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let ext = root.join("ext");
    fs::create_dir_all(home.join("cron")).unwrap();
    fs::create_dir_all(&ext).unwrap();
    let store = Arc::new(Mutex::new(Store::default()));
    spawn_store(store.clone());
    let uri = format!("http://127.0.0.1:{P_STORE}/snap/continuity.json");
    let tok = "pr14-fake-store-token-0001".to_string();

    // S1 seed real state through the real gateway (mock provider).
    let g = start(base_cmd(&home, P_GW), P_GW);
    let c1 = chat(
        P_GW,
        "pr14-a",
        &format!("remember this pat {} please", fake_pat()),
    );
    println!("S1 chat pr14-a (fake github_pat_ in text) -> {c1}");
    let (code, msgs) = http(
        P_GW,
        "GET",
        "/api/sessions/pr14-a/messages",
        true,
        None,
        None,
    );
    println!(
        "S1 /api/sessions/pr14-a/messages -> {code} bytes={}",
        msgs.len()
    );
    let h_off = http(P_GW, "GET", "/health", false, None, None).1;
    println!("S1 /health (offhost off) -> {h_off}");
    assert!(
        !h_off.contains("offhost_enabled"),
        "offhost off must not change /health"
    );
    stop(g);
    let db = home.join("rs-state.db");
    println!(
        "S1 files: rs-state.db={}B mode={} spend={} gateway_state={}",
        fs::metadata(&db).map(|m| m.len()).unwrap_or(0),
        mode(&db),
        home.join("rs-spend.json").exists(),
        home.join("gateway_state.json").exists()
    );
    fs::write(
        home.join("cron/jobs.json"),
        json!({"jobs": [], "updated_at": "pr14"}).to_string(),
    )
    .unwrap();

    // S2 export (path + http uri + token).
    let envs = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_PATH", ext.display().to_string()),
        ("HERMES_OFFHOST_URI", uri.clone()),
        ("HERMES_OFFHOST_TOKEN", tok.clone()),
        (
            "HERMES_OFFHOST_SURVIVAL",
            root.join("survival.json").display().to_string(),
        ),
    ];
    let t0 = Instant::now();
    let (rc, out, err) = once(&home, &envs);
    println!(
        "S2 --offhost-once rc={rc} in {} ms\n  stdout: {}",
        t0.elapsed().as_millis(),
        out.trim()
    );
    assert_eq!(rc, 0, "{err}");
    assert!(
        !out.contains(&tok) && !err.contains(&tok),
        "token must never print"
    );
    let ext_file = ext.join("continuity.json");
    let snap_bytes = fs::read(&ext_file).expect("external snapshot");
    let snap: Value = serde_json::from_slice(&snap_bytes).unwrap();
    println!(
        "S2 external continuity.json {} B mode={}",
        snap_bytes.len(),
        mode(&ext_file)
    );
    for f in snapshot_files(&snap) {
        println!(
            "  file role={} included={} bytes={} reason={}",
            f["role"], f["included"], f["bytes"], f["reason"]
        );
    }
    {
        let g = store.lock().unwrap();
        println!(
            "S2 store puts={} auth_schemes={:?} body_same_as_path={}",
            g.puts,
            g.auth,
            g.body.as_deref() == Some(&snap_bytes[..])
        );
        if !g.auth.is_empty() {
            println!("FINDING http-bearer: Authorization header sent over plain http:// (check_uri allows http)");
        }
    }
    let pat_leak = snapshot_files(&snap)
        .iter()
        .any(|f| decoded_contains(f, &fake_pat()));
    println!("S2 fake github_pat_ present in snapshot body: {pat_leak}");
    if pat_leak {
        println!("FINDING binary-secret-gap: session_db is copied raw; secret_like() only knows sk-/ghp_/xoxb-, so github_pat_ goes off-host");
    }
    let surv: Value =
        serde_json::from_slice(&fs::read(root.join("survival.json")).unwrap()).unwrap();
    println!(
        "S2 survival.json keys={:?}",
        surv.as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
    );
    assert!(surv.get("cash_usd").is_none());
    assert!(!surv.to_string().contains(&tok));

    // S3 a sk- key in any session drops the whole DB from the bundle.
    let g = start(base_cmd(&home, P_GW), P_GW);
    let c3 = chat(P_GW, "pr14-b", &format!("my key is {}", fake_sk()));
    stop(g);
    let (rc3, out3, _) = once(&home, &envs);
    let snap3: Value = serde_json::from_slice(&fs::read(&ext_file).unwrap()).unwrap();
    let db3 = snapshot_files(&snap3)
        .into_iter()
        .find(|f| f["role"] == "session_db")
        .unwrap_or(Value::Null);
    println!("S3 chat pr14-b (fake sk-) -> {c3}; once rc={rc3}; session_db included={} reason={}\n  stdout: {}", db3["included"], db3["reason"], out3.trim());
    if db3["included"] == false {
        println!("FINDING all-or-nothing-db: one secret-shaped string in one session removes every session from the off-host copy (blocker only)");
    }

    // S4 disk loss with the clean S2 snapshot: delete local state, resume from path.
    let snap3_bytes = fs::read(&ext_file).unwrap();
    let wipe = |home: &Path| {
        for f in [
            "rs-state.db",
            "rs-spend.json",
            "gateway_state.json",
            "rs-cron-state.json",
            "cron/jobs.json",
        ] {
            let _ = fs::remove_file(home.join(f));
        }
    };
    let run_off = |home: &Path, envs: &[(&str, String)], tag: &str| -> (Value, u16, String) {
        let mut c = base_cmd(home, P_GW);
        for (k, v) in envs {
            c.env(k, v);
        }
        let g = start(c, P_GW);
        let h = http(P_GW, "GET", "/health", false, None, None).1;
        let (dc, det) = http(P_GW, "GET", "/health/detailed", true, None, None);
        let (dn, _) = http(P_GW, "GET", "/health/detailed", false, None, None);
        let (sc, _) = http(
            P_GW,
            "GET",
            "/api/sessions/pr14-a/messages",
            true,
            None,
            None,
        );
        let err = stop(g);
        println!("{tag} /health -> {h}");
        println!("{tag} /health/detailed auth={dc} noauth={dn}; session pr14-a messages -> {sc}");
        let mut resume_line = String::new();
        for l in err.lines().filter(|l| l.contains("offhost resume")) {
            println!("  log: {l}");
            resume_line = l.to_string();
        }
        (
            serde_json::from_str(&det).unwrap_or(Value::Null),
            sc,
            resume_line,
        )
    };
    wipe(&home);
    fs::write(&ext_file, &snap_bytes).unwrap();
    let (det4, s4, _) = run_off(&home, &envs, "S4");
    assert!(det4["offhost"]["enabled"] == true);
    assert_eq!(s4, 200, "clean snapshot must bring session pr14-a back");
    if det4["offhost"]["restored"].is_null() {
        println!("FINDING status-overwrite: /health/detailed offhost view shows the first beat, not the resume result (restored/kept only in the log)");
    }
    println!(
        "S4 after resume: cron/jobs.json exists={} (cron.rs says hermes-rs never writes jobs.json)",
        home.join("cron/jobs.json").exists()
    );

    // S5 nothing lost, files merely idle > stale_after: does resume rewrite them?
    for f in ["rs-spend.json", "cron/jobs.json", "rs-state.db"] {
        let p = home.join(f);
        let _ = Command::new("chmod").arg("644").arg(&p).status();
        age(&p);
    }
    let before: Vec<(String, String)> = ["rs-spend.json", "rs-state.db"]
        .iter()
        .map(|f| {
            (
                f.to_string(),
                fs::read(home.join(f))
                    .map(|b| sha_hex(&b))
                    .unwrap_or_default(),
            )
        })
        .collect();
    let (_, s5, l5) = run_off(&home, &envs, "S5");
    assert_eq!(s5, 200);
    for (f, sha) in before {
        let after = fs::read(home.join(&f))
            .map(|b| sha_hex(&b))
            .unwrap_or_default();
        println!(
            "  {f}: content_changed={} mode_now={}",
            sha != after,
            mode(&home.join(&f))
        );
    }
    if l5.contains("restored=")
        && !l5.contains("restored= ")
        && l5.contains("session_db")
        && l5
            .split("restored=")
            .nth(1)
            .map(|r| r.split(' ').next().unwrap_or("").contains("session_db"))
            .unwrap_or(false)
    {
        println!("FINDING mtime-stale: an idle but intact local session DB was overwritten from the snapshot on a plain restart");
    }

    // S4b disk loss when the latest snapshot omitted the DB (S3 sk- case).
    wipe(&home);
    fs::write(&ext_file, &snap3_bytes).unwrap();
    let (_, s4b, _) = run_off(&home, &envs, "S4b");
    println!("S4b resume from a snapshot with session_db omitted -> session pr14-a messages {s4b}");
    if s4b != 200 {
        println!("FINDING all-or-nothing-db (data loss): after disk loss every session is gone, not only the one with the secret");
    }

    // S6 hostile store: tampered snapshot (recomputed sha) with an agent cron job.
    fs::remove_file(home.join("cron/jobs.json")).ok();
    let mut evil: Value = serde_json::from_slice(&fs::read(&ext_file).unwrap()).unwrap();
    let jobs = json!({"jobs": [{"id": "pr14evil", "name": "injected", "prompt": "injected by store", "schedule": {"kind": "interval", "minutes": 1440, "display": "every 1d"}, "enabled": true}], "updated_at": "evil"}).to_string();
    if let Some(files) = evil["files"].as_array_mut() {
        for f in files.iter_mut() {
            if f["role"] == "cron_jobs" {
                f["text"] = json!(jobs);
                f["included"] = json!(true);
                f["sha256"] = json!(sha_hex(jobs.as_bytes()));
            }
        }
    }
    store.lock().unwrap().serve = Some(serde_json::to_vec(&evil).unwrap());
    let envs6 = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_URI", uri.clone()),
        ("HERMES_OFFHOST_TOKEN", tok.clone()),
    ];
    let mut c = base_cmd(&home, P_GW);
    for (k, v) in &envs6 {
        c.env(k, v);
    }
    let g = start(c, P_GW);
    let (_, jl) = http(P_GW, "GET", "/api/jobs", true, None, None);
    stop(g);
    let injected = jl.contains("pr14evil");
    println!("S6 tampered store snapshot -> /api/jobs lists injected job: {injected}");
    if injected {
        println!("FINDING no-authenticity: snapshot sha256 is in the same document, so whoever controls the store can plant cron jobs / sessions / spend on resume");
    }
    store.lock().unwrap().serve = None;

    // S7 standby: loopback health only; non-loopback refused; --offhost-once refused.
    let mut c = base_cmd(&home, P_STANDBY);
    c.env("HERMES_OFFHOST_PATH", ext.display().to_string())
        .arg("--offhost-standby");
    let child = c.spawn().unwrap();
    let sb = Gw(child);
    let t0 = Instant::now();
    let mut hs = (0, String::new());
    while t0.elapsed() < Duration::from_secs(10) && hs.0 != 200 {
        std::thread::sleep(Duration::from_millis(150));
        hs = http(P_STANDBY, "GET", "/health", false, None, None);
    }
    let chat_sb = chat(P_STANDBY, "pr14-sb", "hi");
    let models_sb = http(P_STANDBY, "GET", "/v1/models", true, None, None).0;
    stop(sb);
    let hsv: Value = serde_json::from_str(&hs.1).unwrap_or(Value::Null);
    println!(
        "S7 standby /health={} gateway_state={} loaded={} source={}; chat={} models={}",
        hs.0, hsv["gateway_state"], hsv["loaded"], hsv["source"], chat_sb, models_sb
    );
    assert_eq!(hs.0, 200);
    assert_eq!(hsv["gateway_state"], "standby");
    assert_eq!(chat_sb, 404);
    let mut c = base_cmd(&home, P_PUBLIC);
    c.env("HERMES_RS_HOST", "0.0.0.0").arg("--offhost-standby");
    let o = c.output().unwrap();
    println!("S7 standby on 0.0.0.0 -> exit {:?}", o.status.code());
    assert_eq!(o.status.code(), Some(2));
    let mut c = base_cmd(&home, P_GW);
    c.args(["--offhost-standby", "--offhost-once"]);
    let o = c.output().unwrap();
    println!("S7 standby + --offhost-once -> exit {:?}", o.status.code());
    assert_eq!(o.status.code(), Some(2));

    // S8 bad config refused.
    for (k, v) in [
        ("HERMES_OFFHOST_URI", "https://openrouter.ai/x"),
        ("HERMES_OFFHOST_URI", "https://u:p@example.invalid/x"),
        ("HERMES_OFFHOST_INTERVAL_S", "5"),
    ] {
        let mut c = base_cmd(&home, P_GW);
        c.env("HERMES_OFFHOST", "1").env(k, v).arg("--check-config");
        let o = c.output().unwrap();
        println!("S8 {k} bad -> exit {:?}", o.status.code());
        assert_eq!(o.status.code(), Some(2));
    }
    // S9 live-sized and large DBs through the http store (curl -K config body).
    for (sz, cap) in [(1_347_584usize, None), (6_000_000usize, Some("16777216"))] {
        let h9 = root.join(format!("h9-{sz}"));
        fs::create_dir_all(&h9).unwrap();
        let mut rnd = vec![0u8; sz];
        fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut rnd)
            .unwrap();
        fs::write(h9.join("rs-state.db"), &rnd).unwrap();
        store.lock().unwrap().body = None;
        let mut e9 = vec![
            ("HERMES_OFFHOST", "1".to_string()),
            ("HERMES_OFFHOST_URI", uri.clone()),
        ];
        if let Some(c) = cap {
            e9.push(("HERMES_OFFHOST_MAX_BYTES", c.to_string()));
        }
        let t0 = Instant::now();
        let (rc, out, _) = once(&h9, &e9);
        let got = store
            .lock()
            .unwrap()
            .body
            .as_ref()
            .map(|b| b.len())
            .unwrap_or(0);
        println!(
            "S9 db={sz} cap={cap:?} rc={rc} {} ms store_body={got}\n  stdout: {}",
            t0.elapsed().as_millis(),
            out.trim().chars().take(300).collect::<String>()
        );
    }
    let _ = fs::remove_dir_all(&root);
}
