// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Real test for wave-4 slot 280 (desk-py-callers-audit): prove what the
// workspace galion-desk forks on every /desk request, and that it is never
// Python. The live Oct 10 desk forks `python3 /opt/galion/bin/desk-hermes.py`
// (seen live 2026-10-27 19:16 +01:00); this build must fork only the binary
// named by DESK_HERMES_BIN.
//
// harness = false: when GALION_DESK_STUB is set this executable is the
// mindpack stub (records argv/ppid/stdin, prints a JSON object, exits 0);
// otherwise it is the test driver. Gated with GALION_REAL=1.
// Optional GALION_REAL_DESK_HERMES=<release desk-hermes> adds a run with the
// real Rust mindpack binary. Ports 21500-21509 (slot 280) only.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const STUB_JSON: &str = r#"{"stub":"slot-280","mind":{"sessions":7}}"#;

fn main() {
    if let Ok(record) = std::env::var("GALION_DESK_STUB") {
        stub(&record);
        return;
    }
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        println!("SKIP real_mindpack_exec: GALION_REAL=1 not set");
        return;
    }
    let mut passed = 0;
    for (name, f) in [
        ("s1_stub_is_the_only_child", s1 as fn()),
        ("s2_failing_bin_gives_empty_mindpack", s2),
        ("s3_missing_bin_gives_empty_mindpack", s3),
        ("s4_binary_has_no_python_string", s4),
        ("s5_real_desk_hermes", s5),
    ] {
        f();
        println!("test {name} ... ok");
        passed += 1;
    }
    println!("real_mindpack_exec: {passed} passed; 0 failed");
}

/// Mindpack stub: append one line per call, then print the JSON.
fn stub(record: &str) {
    let ppid = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            let after = s.rsplit_once(')')?.1.to_string();
            after.split_whitespace().nth(1).map(str::to_string)
        })
        .unwrap_or_default();
    let stdin = std::fs::read_link("/proc/self/fd/0")
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "closed".into());
    let args = std::env::args().count() - 1;
    let line = format!("ppid={ppid} args={args} stdin={stdin}\n");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)
        .expect("stub record");
    f.write_all(line.as_bytes()).unwrap();
    println!("{STUB_JSON}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("test-out/real-mindpack-exec")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["state", "one", "hermes"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

struct Desk {
    child: Child,
    addr: String,
}

impl Drop for Desk {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(port: u16, dir: &Path, bin: &str, extra: &[(&str, &str)]) -> Desk {
    let addr = format!("127.0.0.1:{port}");
    let closed = "127.0.0.1:21509";
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_galion-desk"));
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("OPENMIND_CONFIG", dir.join("absent.toml"))
        .env("DESK_BIND", &addr)
        .env("DESK_OLLAMA", closed)
        .env("DESK_STATE_DIR", dir.join("state"))
        .env("DESK_ONE_DIR", dir.join("one"))
        .env("DESK_HERMES_DIR", dir.join("hermes"))
        .env("DESK_HERMES_BIN", bin);
    for k in [
        "DESK_PROBE_HERMES",
        "DESK_PROBE_ONE",
        "DESK_PROBE_TUNNEL",
        "DESK_PROBE_CONSOLE",
        "DESK_PROBE_ORIGIN",
        "DESK_PROBE_ROOM",
        "DESK_PROBE_TULU",
    ] {
        cmd.env(k, closed);
    }
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start galion-desk");
    Desk { child, addr }
}

fn http(addr: &str, req: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(mut s) = TcpStream::connect(addr) {
            s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            s.write_all(req.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            return out;
        }
        assert!(Instant::now() < deadline, "no listener on {addr}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn desk_json(addr: &str, method: &str) -> serde_json::Value {
    let req = if method == "POST" {
        "POST /desk HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}".to_string()
    } else {
        "GET /desk HTTP/1.1\r\nHost: x\r\n\r\n".to_string()
    };
    let resp = http(addr, &req);
    assert!(resp.starts_with("HTTP/1.1 200"), "status: {resp}");
    let body = resp.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("bad json ({e}): {body}"))
}

fn health(addr: &str) {
    let resp = http(addr, "GET /health HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(resp.starts_with("HTTP/1.1 200"), "health: {resp}");
}

/// S1: every GET/POST /desk forks DESK_HERMES_BIN exactly once, as a direct
/// child of galion-desk, no args, stdin /dev/null; /health forks nothing.
fn s1() {
    let dir = scratch("s1");
    let record = dir.join("calls.txt");
    let me = std::env::current_exe().unwrap();
    let rec = record.display().to_string();
    let desk = start(
        21500,
        &dir,
        me.to_str().unwrap(),
        &[("GALION_DESK_STUB", &rec)],
    );
    health(&desk.addr);
    assert!(!record.exists(), "/health must not fork the mindpack");
    let want: serde_json::Value = serde_json::from_str(STUB_JSON).unwrap();
    for method in ["GET", "GET", "POST"] {
        let v = desk_json(&desk.addr, method);
        assert_eq!(v["mindpack"], want, "{method} mindpack");
    }
    health(&desk.addr);
    let calls = std::fs::read_to_string(&record).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 3, "one fork per /desk: {calls}");
    let pid = desk.child.id();
    for l in &lines {
        assert_eq!(*l, format!("ppid={pid} args=0 stdin=/dev/null"), "{calls}");
    }
}

/// S2: a mindpack binary that fails gives `{}` and the desk keeps serving.
fn s2() {
    let dir = scratch("s2");
    let desk = start(21501, &dir, "/bin/false", &[]);
    assert_eq!(
        desk_json(&desk.addr, "GET")["mindpack"],
        serde_json::json!({})
    );
    health(&desk.addr);
}

/// S3: a missing mindpack binary (e.g. desk-hermes not installed yet) gives
/// `{}`; nothing falls back to python3 or a .py file.
fn s3() {
    let dir = scratch("s3");
    let missing = dir.join("no-such-desk-hermes");
    let desk = start(21502, &dir, missing.to_str().unwrap(), &[]);
    assert_eq!(
        desk_json(&desk.addr, "GET")["mindpack"],
        serde_json::json!({})
    );
    health(&desk.addr);
}

/// S4: the built galion-desk carries no python3 / desk-hermes.py string
/// (the live Oct 10 binary carries both).
fn s4() {
    let bin = std::fs::read(env!("CARGO_BIN_EXE_galion-desk")).unwrap();
    for needle in [
        &b"python3"[..],
        b"desk-hermes.py",
        b"pw.py",
        b"replicate-picture",
    ] {
        assert!(
            !bin.windows(needle.len()).any(|w| w == needle),
            "galion-desk binary contains {:?}",
            String::from_utf8_lossy(needle)
        );
    }
}

/// S5 (optional): the real Rust desk-hermes on an empty Hermes home gives a
/// JSON mindpack object through the desk.
fn s5() {
    let Ok(real) = std::env::var("GALION_REAL_DESK_HERMES") else {
        println!("note s5: GALION_REAL_DESK_HERMES not set, real desk-hermes run skipped");
        return;
    };
    let dir = scratch("s5");
    let home = dir.join("hermes").display().to_string();
    let desk = start(21503, &dir, &real, &[("HERMES_HOME", &home)]);
    let v = desk_json(&desk.addr, "GET");
    let mp = v["mindpack"].as_object().expect("mindpack object");
    assert!(!mp.is_empty(), "real desk-hermes mindpack empty: {v}");
    println!("note s5: real desk-hermes mindpack keys {}", mp.len());
}
