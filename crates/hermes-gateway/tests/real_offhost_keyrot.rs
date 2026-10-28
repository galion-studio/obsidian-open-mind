// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 340: real-binary test for off-host HMAC key rotation
//! (dual-key verify window, `HERMES_OFFHOST_HMAC_PREV_FILE` +
//! `HERMES_OFFHOST_HMAC_PREV_UNTIL`). Harness pieces adapted from slot 314
//! `real_offhost_f1f2.rs` (same repo, MIT).
//!
//! Gated: runs only with GALION_REAL=1. Uses the release hermes-gateway from
//! HERMES_GW_BIN (else $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release).
//! Optional HERMES_GW_BASE_BIN = a build of swarm/hermes-offhost-fixes
//! a885d473: it must show the gap (no window: a key change strands the old
//! snapshot). Ports 22100-22109, loopback only. Keys are throwaway random
//! bytes made at run time, never printed; key files are mode 0400 like a
//! systemd credential. Runs on the box and, copied, on the VPS in a
//! /var/tmp folder (no live service touched).

use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const P_GW: u16 = 22100;
const P_STANDBY: u16 = 22101;
const API: &str = "t340-keyrot-gateway-key";
/// (name, extra env, wanted exit code, needle in the output) for one gate.
type Gate<'a> = (&'a str, Vec<(&'a str, String)>, i32, &'a str);

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

fn now_secs() -> i64 {
    hermes_rs::time::now_secs()
}

fn zagreb(secs: i64) -> String {
    hermes_rs::time::zagreb_rfc3339(secs)
}

fn http_get(port: u16, path: &str, auth: bool) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if auth {
        req.push_str(&format!("Authorization: Bearer {API}\r\n"));
    }
    req.push_str("\r\n");
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

fn chat(port: u16, sid: &str, text: &str) -> u16 {
    let body =
        json!({"model": "mock", "messages": [{"role": "user", "content": text}]}).to_string();
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: Bearer {API}\r\nX-Hermes-Session-Id: {sid}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(req.as_bytes());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out)
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
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
        .env("API_SERVER_KEY", API)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn wait_up(port: u16) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(15) {
        if http_get(port, "/health", false).0 == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
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

/// (exit code, stdout, stderr) of a one-shot run with extra env and args.
fn run(exe: &Path, home: &Path, envs: &[(&str, String)], arg: &str) -> (i32, String, String) {
    let mut c = base_cmd(exe, home, P_GW);
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.arg(arg).output().expect("run gateway");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Start a standby on a fresh home; return its /health JSON and stderr.
fn standby(exe: &Path, home: &Path, envs: &[(&str, String)]) -> (Value, String) {
    fs::create_dir_all(home).unwrap();
    let mut c = base_cmd(exe, home, P_STANDBY);
    for (k, v) in envs {
        c.env(k, v);
    }
    let g = Gw(c.arg("--offhost-standby").spawn().expect("spawn standby"));
    assert!(wait_up(P_STANDBY), "standby on {P_STANDBY} did not come up");
    let (code, body) = http_get(P_STANDBY, "/health", false);
    assert_eq!(code, 200);
    let err = stop(g);
    (serde_json::from_str(&body).unwrap_or(Value::Null), err)
}

/// Throwaway key from the kernel, mode 0400 (like a systemd credential).
fn write_key(p: &Path) -> Vec<u8> {
    use std::os::unix::fs::PermissionsExt;
    let mut k = [0u8; 48];
    fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut k)
        .unwrap();
    fs::write(p, k).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o400)).unwrap();
    k.to_vec()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

fn store_key_id(store: &Path) -> String {
    let d: Value = serde_json::from_slice(&fs::read(store).unwrap()).unwrap();
    d["key_id"].as_str().unwrap_or("").to_string()
}

fn check_line(out: &str) -> String {
    out.lines()
        .find(|l| l.contains("sealed="))
        .map(|l| l[l.find("sealed=").unwrap()..].to_string())
        .unwrap_or_default()
}

#[test]
fn real_offhost_keyrot() {
    if !on() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let exe = bin();
    assert!(exe.exists(), "missing binary {}", exe.display());
    let root = std::env::temp_dir().join(format!("t340-keyrot-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let keys = root.join("keys");
    let ext = root.join("store");
    let home = root.join("primary");
    for d in [&keys, &ext, &home.join("cron")] {
        fs::create_dir_all(d).unwrap();
    }
    let store = ext.join("continuity.json");
    let ka = keys.join("hmac-a.key");
    let kb = keys.join("hmac-b.key");
    let raw_a = write_key(&ka);
    let raw_b = write_key(&kb);
    let mut transcript: Vec<u8> = Vec::new();
    let t0 = now_secs();
    println!("START {} exe={}", zagreb(t0), exe.display());

    // Seed: two chat sessions and one cron job on the primary.
    let g = Gw(base_cmd(&exe, &home, P_GW).arg("--mock").spawn().unwrap());
    assert!(wait_up(P_GW));
    assert_eq!(chat(P_GW, "t340-a", "rotation note one"), 200);
    assert_eq!(chat(P_GW, "t340-b", "rotation note two"), 200);
    transcript.extend(stop(g).into_bytes());
    let jobs = json!({"jobs": [{"id": "t340keep", "name": "keep", "prompt": "noop", "schedule": {"kind": "interval", "minutes": 1440, "display": "every 1d"}, "enabled": false}], "updated_at": "t340"}).to_string();
    fs::write(home.join("cron/jobs.json"), &jobs).unwrap();

    let path = ("HERMES_OFFHOST_PATH", ext.display().to_string());
    let on1 = ("HERMES_OFFHOST", "1".to_string());
    let key = |p: &Path| ("HERMES_OFFHOST_HMAC_FILE", p.display().to_string());
    let prev = |p: &Path| ("HERMES_OFFHOST_HMAC_PREV_FILE", p.display().to_string());
    let until = |s: i64| ("HERMES_OFFHOST_HMAC_PREV_UNTIL", zagreb(s));
    let open_until = t0 + 3600;

    // 1) Old primary exports with key A.
    let (rc, out, err) = run(
        &exe,
        &home,
        &[on1.clone(), path.clone(), key(&ka)],
        "--check-config",
    );
    assert_eq!(rc, 0, "{err}");
    let id_a = check_line(&out)
        .trim_start_matches("sealed=")
        .split(' ')
        .next()
        .unwrap()
        .to_string();
    println!("1 key A check-config: {}", check_line(&out));
    transcript.extend((out + &err).into_bytes());
    let (rc, out, err) = run(
        &exe,
        &home,
        &[on1.clone(), path.clone(), key(&ka)],
        "--offhost-once",
    );
    assert_eq!(rc, 0, "{err}");
    assert_eq!(store_key_id(&store), id_a);
    println!(
        "1 export with A rc={rc} store key_id={}",
        store_key_id(&store)
    );
    transcript.extend((out + &err).into_bytes());

    // 2) Config gates (exit 2 each), and the open/closed check-config lines.
    let gates: Vec<Gate> = vec![
        (
            "prev without until",
            vec![key(&kb), prev(&ka)],
            2,
            "PREV_UNTIL is required",
        ),
        (
            "until without prev",
            vec![key(&kb), until(open_until)],
            2,
            "PREV_FILE is not",
        ),
        (
            "until in UTC Z",
            vec![
                key(&kb),
                prev(&ka),
                (
                    "HERMES_OFFHOST_HMAC_PREV_UNTIL",
                    hermes_rs::time::utc_rfc3339(open_until).replace("+00:00", "Z"),
                ),
            ],
            2,
            "Zagreb",
        ),
        (
            "until 31 days",
            vec![key(&kb), prev(&ka), until(t0 + 31 * 86_400)],
            2,
            "30 days",
        ),
        (
            "prev == current",
            vec![key(&kb), prev(&kb), until(open_until)],
            2,
            "same key",
        ),
        (
            "prev without current",
            vec![prev(&ka), until(open_until)],
            2,
            "HMAC_FILE is required",
        ),
        (
            "window open",
            vec![key(&kb), prev(&ka), until(open_until)],
            0,
            "prev_window=open",
        ),
        (
            "window closed",
            vec![key(&kb), prev(&ka), until(t0 - 60)],
            0,
            "prev_window=closed",
        ),
    ];
    for (name, extra, want, needle) in &gates {
        let mut envs = vec![on1.clone(), path.clone()];
        envs.extend(extra.iter().cloned());
        let (rc, out, err) = run(&exe, &home, &envs, "--check-config");
        let shown = if rc == 0 {
            check_line(&out)
        } else {
            err.trim().to_string()
        };
        println!("2 gate {name}: exit {rc} :: {shown}");
        assert_eq!(rc, *want, "{name}: {out}{err}");
        assert!(shown.contains(needle), "{name}: {shown}");
        transcript.extend((out + &err).into_bytes());
    }
    // A loose (0640) previous key file is refused.
    {
        use std::os::unix::fs::PermissionsExt;
        let loose = keys.join("hmac-a-loose.key");
        fs::copy(&ka, &loose).unwrap();
        fs::set_permissions(&loose, fs::Permissions::from_mode(0o640)).unwrap();
        let (rc, out, err) = run(
            &exe,
            &home,
            &[
                on1.clone(),
                path.clone(),
                key(&kb),
                prev(&loose),
                until(open_until),
            ],
            "--check-config",
        );
        println!("2 gate prev mode 0640: exit {rc} :: {}", err.trim());
        assert_eq!(rc, 2);
        assert!(err.contains("PREV_FILE must be mode 0600"));
        transcript.extend((out + &err).into_bytes());
    }

    // Optional: the a885d473 base has no window, so B strands A's snapshot.
    if let Ok(base) = std::env::var("HERMES_GW_BASE_BIN") {
        let bh = root.join("base-standby");
        let (h, err) = standby(
            Path::new(&base),
            &bh,
            &[path.clone(), key(&kb), prev(&ka), until(open_until)],
        );
        let blk = h["offhost"]["blockers"].to_string();
        println!(
            "BASE standby B (+ignored PREV vars) loaded={} auth={} blockers={blk}",
            h["offhost"]["loaded"], h["offhost"]["auth"]
        );
        assert_eq!(h["offhost"]["loaded"], json!(false));
        assert!(blk.contains("different key"));
        assert!(!bh.join("cron/jobs.json").exists());
        transcript.extend(err.into_bytes());
    }

    // 3) Standby with B only: refuses A's snapshot, writes nothing.
    let s1 = root.join("standby-b-only");
    let (h, err) = standby(&exe, &s1, &[path.clone(), key(&kb)]);
    let blk = h["offhost"]["blockers"].to_string();
    println!(
        "3 standby B only: loaded={} auth={} blockers={blk}",
        h["offhost"]["loaded"], h["offhost"]["auth"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(false));
    assert!(blk.contains("different key"));
    assert!(!s1.join("cron/jobs.json").exists());
    transcript.extend(err.into_bytes());

    // 4) Standby first: B + previous A, window open -> restores.
    let s2 = root.join("standby-window");
    let (h, err) = standby(
        &exe,
        &s2,
        &[path.clone(), key(&kb), prev(&ka), until(open_until)],
    );
    println!(
        "4 standby B+prevA open: loaded={} auth={} restored={} kept={} state={}",
        h["offhost"]["loaded"],
        h["offhost"]["auth"],
        h["offhost"]["restored"],
        h["offhost"]["kept"],
        h["gateway_state"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(true));
    assert_eq!(h["offhost"]["auth"], json!("hmac-sha256+previous-key"));
    assert_eq!(h["gateway_state"], json!("standby"));
    assert!(fs::read_to_string(s2.join("cron/jobs.json"))
        .unwrap()
        .contains("t340keep"));
    transcript.extend(err.into_bytes());

    // 5) Primary rotates: same window, next export re-seals with B.
    let (rc, out, err) = run(
        &exe,
        &home,
        &[
            on1.clone(),
            path.clone(),
            key(&kb),
            prev(&ka),
            until(open_until),
        ],
        "--offhost-once",
    );
    assert_eq!(rc, 0, "{err}");
    let id_b = store_key_id(&store);
    println!("5 primary export B (window open) rc={rc} store key_id {id_a} -> {id_b}");
    assert_ne!(id_b, id_a);
    transcript.extend((out + &err).into_bytes());

    // 6) Window removed: B alone reads the new store; A alone is retired.
    let s3 = root.join("standby-b-after");
    let (h, err) = standby(&exe, &s3, &[path.clone(), key(&kb)]);
    println!(
        "6 standby B only after re-seal: loaded={} auth={}",
        h["offhost"]["loaded"], h["offhost"]["auth"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(true));
    assert_eq!(h["offhost"]["auth"], json!("hmac-sha256"));
    assert!(fs::read_to_string(s3.join("cron/jobs.json"))
        .unwrap()
        .contains("t340keep"));
    transcript.extend(err.into_bytes());
    let s4 = root.join("standby-a-retired");
    let (h, err) = standby(&exe, &s4, &[path.clone(), key(&ka)]);
    println!(
        "6 standby A only after re-seal: loaded={} blockers={}",
        h["offhost"]["loaded"], h["offhost"]["blockers"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(false));
    assert!(!s4.join("cron/jobs.json").exists());
    transcript.extend(err.into_bytes());

    // 7) Closed window: an A-sealed store is refused, nothing written.
    let (rc, out, err) = run(
        &exe,
        &home,
        &[on1.clone(), path.clone(), key(&ka)],
        "--offhost-once",
    );
    assert_eq!(rc, 0);
    assert_eq!(store_key_id(&store), id_a);
    transcript.extend((out + &err).into_bytes());
    let s5 = root.join("standby-closed");
    let (h, err) = standby(
        &exe,
        &s5,
        &[path.clone(), key(&kb), prev(&ka), until(t0 - 60)],
    );
    let blk = h["offhost"]["blockers"].to_string();
    println!(
        "7 standby B+prevA closed: loaded={} blockers={blk}",
        h["offhost"]["loaded"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(false));
    assert!(blk.contains("rotation window closed at"));
    assert!(!s5.join("cron/jobs.json").exists());
    transcript.extend(err.into_bytes());

    // 8) Tampered A-sealed snapshot inside an open window: refused.
    let mut d: Value = serde_json::from_slice(&fs::read(&store).unwrap()).unwrap();
    let p = d["payload"]
        .as_str()
        .unwrap()
        .replace("t340keep", "t340evil");
    d["payload"] = json!(p);
    fs::write(&store, serde_json::to_vec(&d).unwrap()).unwrap();
    let s6 = root.join("standby-tamper");
    let (h, err) = standby(
        &exe,
        &s6,
        &[path.clone(), key(&kb), prev(&ka), until(open_until)],
    );
    let blk = h["offhost"]["blockers"].to_string();
    println!(
        "8 tampered A snapshot, window open: loaded={} blockers={blk}",
        h["offhost"]["loaded"]
    );
    assert_eq!(h["offhost"]["loaded"], json!(false));
    assert!(blk.contains("MAC mismatch"));
    assert!(!s6.join("cron/jobs.json").exists());
    transcript.extend(err.into_bytes());

    // 9) No key bytes anywhere: logs, store, restored homes.
    let mut files: Vec<PathBuf> = vec![store.clone()];
    for dir in [&s2, &s3] {
        for e in fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_file() {
                files.push(e.path());
            }
        }
    }
    let mut hay = transcript.clone();
    for f in &files {
        hay.extend(fs::read(f).unwrap_or_default());
    }
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    for (name, raw) in [("A", &raw_a), ("B", &raw_b)] {
        assert!(!contains(&hay, raw), "raw key {name} leaked");
        assert!(
            !contains(&hay, hex(raw).as_bytes()),
            "hex key {name} leaked"
        );
        assert!(!contains(&hay, &raw[..16]), "key {name} prefix leaked");
    }
    println!(
        "9 no key bytes (raw, hex, 16-byte prefix) in {} B of logs+store+restored files",
        hay.len()
    );
    let _ = fs::remove_dir_all(&root);
    println!("END {} ok", zagreb(now_secs()));
}
