// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Wave-4 slot 314: real-binary test for the PR #14 review fixes
//! F1 (snapshot authenticity), F2 (strict secret filter) and F4 (https).
//! Harness adapted from slot 306 `docs/reviews/hermes-offhost-pr14/
//! real_offhost_pr14.rs` (same repo, MIT).
//!
//! Gated: runs only with GALION_REAL=1. Uses the release hermes-gateway from
//! HERMES_GW_BIN (else $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release).
//! Optional HERMES_GW_BASE_BIN = a build of PR #14 tip 1e0f427: the same
//! leak + tamper steps are run against it first and must reproduce the bugs.
//! Ports 21840-21849 only, loopback only. Every secret here is a fake built
//! at run time. No VPS, no network beyond 127.0.0.1.

use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const P_GW: u16 = 21840;
const P_STANDBY: u16 = 21841;
const P_STORE: u16 = 21842;
const KEY: &str = "t314-review-gateway-key";

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
    format!("{}{}", "github_pat_", "Q1w2E3r4".repeat(6))
}
fn fake_sk() -> String {
    format!("{}{}", "sk-", "R".repeat(40))
}
fn fake_aws() -> String {
    format!("{}{}", "AK", "IAQWERTYUIOPASDF")
}

#[derive(Default)]
struct Store {
    body: Option<Vec<u8>>,
    serve: Option<Vec<u8>>,
    puts: u32,
    gets: u32,
}

type Req = (String, Vec<(String, String)>, Vec<u8>);
/// (name, extra env, wanted exit code) for one --check-config gate.
type Gate<'a> = (&'a str, Vec<(&'a str, String)>, i32);

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
            let Some((first, _hdrs, body)) = read_req(&mut c) else {
                continue;
            };
            let mut g = st.lock().unwrap();
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
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        b.len()
    );
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

fn base_cmd(exe: &Path, home: &Path, port: u16) -> Command {
    let mut c = Command::new(exe);
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

fn once(exe: &Path, home: &Path, envs: &[(&str, String)]) -> (i32, String, String) {
    let mut c = base_cmd(exe, home, P_GW);
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

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Inner snapshot of a store document (sealed envelope or plain v1).
fn inner(doc: &Value) -> Value {
    if doc["kind"] == hermes_rs::offhost_guard::SEALED_KIND {
        serde_json::from_str(doc["payload"].as_str().unwrap()).unwrap()
    } else {
        doc.clone()
    }
}

/// Every decoded file body in a snapshot, concatenated.
fn decoded_bodies(snap: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    for f in snap["files"].as_array().cloned().unwrap_or_default() {
        if let Some(t) = f["text"].as_str() {
            out.extend_from_slice(t.as_bytes());
        } else if let Some(b) = f["b64"].as_str() {
            out.extend(hermes_rs::offhost::b64_decode(b).unwrap_or_default());
        }
    }
    out
}

fn leaks(snap_doc: &[u8]) -> Vec<&'static str> {
    let doc: Value = serde_json::from_slice(snap_doc).unwrap_or(Value::Null);
    let mut all = snap_doc.to_vec();
    all.extend(decoded_bodies(&inner(&doc)));
    let mut l = Vec::new();
    if contains(&all, &fake_pat()) {
        l.push("github_pat_");
    }
    if contains(&all, &fake_sk()) {
        l.push("sk-");
    }
    if contains(&all, &fake_aws()) {
        l.push("AKIA");
    }
    l
}

/// Attacker with write access to the store: plant a cron job with a matching
/// per-file sha256 (no key needed for that), keep whatever mac was there.
fn plant_job(doc: &[u8]) -> Vec<u8> {
    let mut d: Value = serde_json::from_slice(doc).unwrap();
    let mut snap = inner(&d);
    let jobs = json!({"jobs": [{"id": "t314evil", "name": "injected", "prompt": "injected by store", "schedule": {"kind": "interval", "minutes": 1440, "display": "every 1d"}, "enabled": true}], "updated_at": "evil"}).to_string();
    for f in snap["files"].as_array_mut().unwrap() {
        if f["role"] == "cron_jobs" {
            f["text"] = json!(jobs);
            f["included"] = json!(true);
            f["sha256"] = json!(galion_sha_hex(jobs.as_bytes()));
        }
    }
    if d["kind"] == hermes_rs::offhost_guard::SEALED_KIND {
        d["payload"] = json!(serde_json::to_string(&snap).unwrap());
    } else {
        d = snap;
    }
    serde_json::to_vec(&d).unwrap()
}

fn galion_sha_hex(b: &[u8]) -> String {
    hermes_rs::mcp::sha256(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn write_key(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // Random key bytes from the kernel; never printed.
    let mut k = [0u8; 48];
    fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut k)
        .unwrap();
    fs::write(p, k).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

fn seed(exe: &Path, home: &Path) {
    let g = start(base_cmd(exe, home, P_GW), P_GW);
    assert_eq!(
        chat(
            P_GW,
            "t314-a",
            &format!("remember this pat {} please", fake_pat())
        ),
        200
    );
    assert_eq!(
        chat(
            P_GW,
            "t314-b",
            &format!("my key is {} and aws {}", fake_sk(), fake_aws())
        ),
        200
    );
    assert_eq!(chat(P_GW, "t314-c", "plain note without secrets"), 200);
    stop(g);
    fs::write(
        home.join("cron/jobs.json"),
        json!({"jobs": [], "updated_at": "t314"}).to_string(),
    )
    .unwrap();
}

fn wipe(home: &Path) {
    for f in [
        "rs-state.db",
        "rs-spend.json",
        "gateway_state.json",
        "rs-cron-state.json",
        "cron/jobs.json",
    ] {
        let _ = fs::remove_file(home.join(f));
    }
}

/// Start with offhost env, return (/api/jobs body, session codes, log lines).
fn boot(
    exe: &Path,
    home: &Path,
    envs: &[(&str, String)],
) -> (String, Vec<(String, u16, String)>, String) {
    let mut c = base_cmd(exe, home, P_GW);
    for (k, v) in envs {
        c.env(k, v);
    }
    let g = start(c, P_GW);
    let jobs = http(P_GW, "GET", "/api/jobs", true, None, None).1;
    let mut sess = Vec::new();
    for s in ["t314-a", "t314-b", "t314-c"] {
        let (code, body) = http(
            P_GW,
            "GET",
            &format!("/api/sessions/{s}/messages"),
            true,
            None,
            None,
        );
        sess.push((s.to_string(), code, body));
    }
    let err = stop(g);
    let log = err
        .lines()
        .filter(|l| l.contains("offhost"))
        .collect::<Vec<_>>()
        .join("\n");
    (jobs, sess, log)
}

/// Same leak + tamper steps on the PR #14 tip binary: must reproduce F1/F2/F4.
fn base_repro(base: &Path, root: &Path, uri: &str, store: &Arc<Mutex<Store>>) {
    let home = root.join("base-home");
    let ext = root.join("base-ext");
    fs::create_dir_all(home.join("cron")).unwrap();
    fs::create_dir_all(&ext).unwrap();
    // Only review-miss shapes here: a classic sk- would make base drop the
    // whole DB (F3) and hide the F2 leak.
    let g = start(base_cmd(base, &home, P_GW), P_GW);
    let msg = format!("pat {} aws {}", fake_pat(), fake_aws());
    assert_eq!(chat(P_GW, "t314-a", &msg), 200);
    stop(g);
    fs::write(
        home.join("cron/jobs.json"),
        json!({"jobs": [], "updated_at": "t314"}).to_string(),
    )
    .unwrap();
    let envs = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_PATH", ext.display().to_string()),
    ];
    let (rc, out, _) = once(base, &home, &envs);
    let doc = fs::read(ext.join("continuity.json")).unwrap();
    let l = leaks(&doc);
    println!(
        "BASE export rc={rc} unsigned (no key needed) leaks={l:?}\n  stdout: {}",
        out.trim()
    );
    // F4 on base: plain http to a non-loopback host is accepted at config.
    let mut c = base_cmd(base, &home, P_GW);
    c.env("HERMES_OFFHOST", "1")
        .env("HERMES_OFFHOST_URI", "http://192.0.2.10/snap")
        .arg("--check-config");
    let o = c.output().unwrap();
    println!(
        "BASE http://192.0.2.10 (non-loopback) --check-config -> exit {:?}",
        o.status.code()
    );
    // F1 on base: tampered store snapshot plants a cron job.
    wipe(&home);
    store.lock().unwrap().serve = Some(plant_job(&doc));
    let (jobs, _, _) = boot(
        base,
        &home,
        &[
            ("HERMES_OFFHOST", "1".to_string()),
            ("HERMES_OFFHOST_URI", uri.to_string()),
        ],
    );
    store.lock().unwrap().serve = None;
    let planted = jobs.contains("t314evil");
    println!("BASE tampered store -> /api/jobs lists planted job: {planted}");
    assert!(!l.is_empty(), "base must reproduce the F2 leak");
    assert!(planted, "base must reproduce the F1 tamper");
    assert_eq!(o.status.code(), Some(0), "base accepts plain http");
}

#[test]
fn real_offhost_f1f2_fixed() {
    if !on() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let exe = bin();
    assert!(exe.exists(), "missing binary {}", exe.display());
    let root = std::env::temp_dir().join(format!("t314-offhost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let store = Arc::new(Mutex::new(Store::default()));
    spawn_store(store.clone());
    let uri = format!("http://127.0.0.1:{P_STORE}/snap/continuity.json");
    let tok = "t314-fake-store-token-0001".to_string();

    if let Ok(base) = std::env::var("HERMES_GW_BASE_BIN") {
        base_repro(Path::new(&base), &root, &uri, &store);
    } else {
        println!("BASE skipped (HERMES_GW_BASE_BIN unset)");
    }

    let home = root.join("home");
    let ext = root.join("ext");
    let keys = root.join("keys");
    fs::create_dir_all(home.join("cron")).unwrap();
    fs::create_dir_all(&ext).unwrap();
    fs::create_dir_all(&keys).unwrap();
    seed(&exe, &home);
    let key = keys.join("offhost.key");
    write_key(&key, 0o600);

    // C1 config gates (F1 fail-closed, key hygiene, F4).
    let loose = keys.join("loose.key");
    write_key(&loose, 0o644);
    let inside = ext.join("in-store.key");
    write_key(&inside, 0o600);
    let gates: Vec<Gate> = vec![
        (
            "path, no key",
            vec![("HERMES_OFFHOST_PATH", ext.display().to_string())],
            2,
        ),
        ("uri, no key", vec![("HERMES_OFFHOST_URI", uri.clone())], 2),
        (
            "key mode 0644",
            vec![
                ("HERMES_OFFHOST_PATH", ext.display().to_string()),
                ("HERMES_OFFHOST_HMAC_FILE", loose.display().to_string()),
            ],
            2,
        ),
        (
            "key inside store",
            vec![
                ("HERMES_OFFHOST_PATH", ext.display().to_string()),
                ("HERMES_OFFHOST_HMAC_FILE", inside.display().to_string()),
            ],
            2,
        ),
        (
            "http non-loopback",
            vec![
                ("HERMES_OFFHOST_URI", "http://192.0.2.10/snap".to_string()),
                ("HERMES_OFFHOST_HMAC_FILE", key.display().to_string()),
            ],
            2,
        ),
        (
            "http loopback + key",
            vec![
                ("HERMES_OFFHOST_URI", uri.clone()),
                ("HERMES_OFFHOST_HMAC_FILE", key.display().to_string()),
            ],
            0,
        ),
    ];
    for (name, envs, want) in gates {
        let mut c = base_cmd(&exe, &home, P_GW);
        c.env("HERMES_OFFHOST", "1");
        for (k, v) in &envs {
            c.env(k, v);
        }
        let o = c.arg("--check-config").output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        let line = err.lines().last().unwrap_or("").to_string();
        println!(
            "C1 {name} -> exit {:?} {}",
            o.status.code(),
            line.chars().take(160).collect::<String>()
        );
        assert_eq!(o.status.code(), Some(want), "{name}: {err}");
    }

    // E1 sealed export to path + loopback http store.
    let envs = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_PATH", ext.display().to_string()),
        ("HERMES_OFFHOST_URI", uri.clone()),
        ("HERMES_OFFHOST_TOKEN", tok.clone()),
        ("HERMES_OFFHOST_HMAC_FILE", key.display().to_string()),
    ];
    let t0 = Instant::now();
    let (rc, out, err) = once(&exe, &home, &envs);
    println!(
        "E1 --offhost-once rc={rc} in {} ms\n  stdout: {}",
        t0.elapsed().as_millis(),
        out.trim()
    );
    assert_eq!(rc, 0, "{err}");
    assert!(out.contains("sealed=true"));
    assert!(!out.contains(&tok) && !err.contains(&tok));
    let ext_file = ext.join("continuity.json");
    let doc = fs::read(&ext_file).unwrap();
    let env_doc: Value = serde_json::from_slice(&doc).unwrap();
    assert_eq!(env_doc["kind"], hermes_rs::offhost_guard::SEALED_KIND);
    assert_eq!(env_doc["alg"], "hmac-sha256");
    let snap = inner(&env_doc);
    let db = snap["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "session_db")
        .cloned()
        .unwrap_or(Value::Null);
    let put = store.lock().unwrap().body.clone().unwrap_or_default();
    println!(
        "E1 envelope kind={} alg={} key_id_len={} bytes={} store_put_same={}",
        env_doc["kind"],
        env_doc["alg"],
        env_doc["key_id"].as_str().unwrap_or("").len(),
        doc.len(),
        put == doc
    );
    println!(
        "E1 session_db included={} redacted_values={} reason={}",
        db["included"], db["redacted_values"], db["reason"]
    );
    let l_path = leaks(&doc);
    let l_put = leaks(&put);
    println!("E1 leaks path={l_path:?} store_put={l_put:?}");
    assert!(
        l_path.is_empty() && l_put.is_empty(),
        "F2: a fake secret left the host"
    );
    assert_eq!(db["included"], true, "F2/F3: DB sanitised, not dropped");
    assert!(db["redacted_values"].as_u64().unwrap_or(0) >= 3);
    assert!(!String::from_utf8_lossy(&doc).contains(&tok));

    // R1 disk loss -> resume from the sealed path snapshot.
    wipe(&home);
    let (jobs, sess, log) = boot(&exe, &home, &envs);
    println!("R1 log: {log}");
    for (s, code, body) in &sess {
        println!(
            "R1 session {s} -> {code} redacted={} has_fake_secret={}",
            body.contains("[REDACTED]"),
            contains(body.as_bytes(), &fake_pat())
                || contains(body.as_bytes(), &fake_sk())
                || contains(body.as_bytes(), &fake_aws())
        );
        assert_eq!(*code, 200, "{s} must come back after disk loss");
        assert!(!contains(body.as_bytes(), &fake_pat()));
        assert!(!contains(body.as_bytes(), &fake_sk()));
    }
    assert!(sess[0].2.contains("[REDACTED]") && sess[1].2.contains("[REDACTED]"));
    assert!(sess[2].2.contains("plain note without secrets"));
    assert!(log.contains("auth=hmac-sha256"));
    assert!(!jobs.contains("t314evil"));

    // T1 hostile store (uri only): planted job, old mac kept.
    let uri_envs = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_URI", uri.clone()),
        ("HERMES_OFFHOST_TOKEN", tok.clone()),
        ("HERMES_OFFHOST_HMAC_FILE", key.display().to_string()),
    ];
    let tamper_cases: Vec<(&str, Vec<u8>, &str)> = {
        let planted = plant_job(&doc);
        let plain = serde_json::to_vec(&inner(&serde_json::from_slice(&planted).unwrap())).unwrap();
        let other = hermes_rs::offhost_guard::SealKey::from_bytes(&[b'e'; 48]).unwrap();
        let resealed = other.seal(&serde_json::from_slice(&plain).unwrap());
        vec![
            ("payload edited, mac kept", planted, "MAC mismatch"),
            ("unsigned plain doc", plain, "unsigned"),
            ("resealed with attacker key", resealed, "different key"),
        ]
    };
    for (name, body, want) in tamper_cases {
        wipe(&home);
        store.lock().unwrap().serve = Some(body);
        let (jobs, sess, log) = boot(&exe, &home, &uri_envs);
        let planted = jobs.contains("t314evil");
        println!(
            "T1 {name} -> planted={planted} jobs_json_written={} session_a={} log: {}",
            home.join("cron/jobs.json").exists(),
            sess[0].1,
            log.lines()
                .find(|l| l.contains("offhost resume"))
                .unwrap_or("")
        );
        assert!(!planted, "{name}: F1 tamper must be refused");
        assert!(!home.join("cron/jobs.json").exists());
        assert!(log.contains(want), "{name}: {log}");
        assert!(log.contains("loaded=false"));
    }
    store.lock().unwrap().serve = None;

    // S1 standby with the same key: genuine loads, tampered is refused.
    for (name, body, loaded) in [
        ("genuine", doc.clone(), true),
        ("tampered", plant_job(&doc), false),
    ] {
        let sb_home = root.join(format!("sb-{name}"));
        let sb_ext = root.join(format!("sb-ext-{name}"));
        fs::create_dir_all(&sb_home).unwrap();
        fs::create_dir_all(&sb_ext).unwrap();
        fs::write(sb_ext.join("continuity.json"), &body).unwrap();
        let mut c = base_cmd(&exe, &sb_home, P_STANDBY);
        c.env("HERMES_OFFHOST_PATH", sb_ext.display().to_string())
            .env("HERMES_OFFHOST_HMAC_FILE", key.display().to_string())
            .arg("--offhost-standby");
        let sb = Gw(c.spawn().unwrap());
        let t0 = Instant::now();
        let mut hs = (0, String::new());
        while t0.elapsed() < Duration::from_secs(10) && hs.0 != 200 {
            std::thread::sleep(Duration::from_millis(150));
            hs = http(P_STANDBY, "GET", "/health", false, None, None);
        }
        stop(sb);
        let v: Value = serde_json::from_str(&hs.1).unwrap_or(Value::Null);
        println!(
            "S1 standby {name} -> /health={} loaded={} auth={} jobs_json={}",
            hs.0,
            v["loaded"],
            v["auth"],
            sb_home.join("cron/jobs.json").exists()
        );
        assert_eq!(hs.0, 200);
        assert_eq!(v["loaded"], loaded);
        if !loaded {
            assert!(!sb_home.join("cron/jobs.json").exists());
        }
    }

    // P1 live-sized DB (1,347,584 B like VPS rs-state.db): sealed export time.
    let h9 = root.join("h9");
    fs::create_dir_all(&h9).unwrap();
    {
        // Real SQLite DB grown through the real gateway is slow; use the
        // seeded DB padded with plain messages via the gateway instead.
        let g = start(base_cmd(&exe, &h9, P_GW), P_GW);
        let pad = "lorem ipsum dolor sit amet ".repeat(400);
        for i in 0..60 {
            assert_eq!(chat(P_GW, &format!("pad-{i}"), &pad), 200);
        }
        assert_eq!(
            chat(P_GW, "pad-secret", &format!("x {} y", fake_pat())),
            200
        );
        stop(g);
    }
    let dbsz = fs::metadata(h9.join("rs-state.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    let e9 = vec![
        ("HERMES_OFFHOST", "1".to_string()),
        ("HERMES_OFFHOST_URI", uri.clone()),
        ("HERMES_OFFHOST_HMAC_FILE", key.display().to_string()),
        ("HERMES_OFFHOST_MAX_BYTES", "16777216".to_string()),
    ];
    store.lock().unwrap().body = None;
    let t0 = Instant::now();
    let (rc, out, _) = once(&exe, &h9, &e9);
    let ms = t0.elapsed().as_millis();
    let put = store.lock().unwrap().body.clone().unwrap_or_default();
    println!(
        "P1 db={dbsz} B rc={rc} {ms} ms store_body={} leaks={:?}\n  stdout: {}",
        put.len(),
        leaks(&put),
        out.trim()
    );
    assert_eq!(rc, 0);
    assert!(leaks(&put).is_empty());
    let _ = fs::remove_dir_all(&root);
}
