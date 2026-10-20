// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life fuzz corpus replay (Rust port of the old
//! `scripts/real/fuzz-sast.sh`). Runs only with `GALION_REAL=1`.
//!
//! 1. Builds release `galion-origin` and `openmind-discovery` with the same
//!    cargo and target dir as this test.
//! 2. Starts `galion-origin` on 127.0.0.1:19700 (slot 100's range) with an
//!    empty root and throws the whole `http_parse_head` corpus at it, one TCP
//!    connection per file (byte 0 is the fuzz target's limits selector and is
//!    not sent). No connection may be refused and `/health` must stay 200.
//! 3. Starts `openmind-discovery announce` on mDNS test port 19701 (never
//!    5353), sends every `mdns_parse` corpus file as one UDP datagram, then a
//!    well-formed PTR query that must still get a parseable answer.
//!
//! Corpus: `fuzz/corpus/<target>` when it exists and is not empty (it is
//! git-ignored and grows on the box), else the committed `fuzz/seeds/<target>`.
//! `FUZZ_CORPUS_ROOT=/path/to/fuzz/corpus` overrides the location.

use openmind_discovery::dns::{build_query, parse_message, TYPE_PTR};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const HTTP_PORT: u16 = 19700;
const MDNS_PORT: u16 = 19701;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn now() -> String {
    // Box clock is Europe/Zagreb; `date` prints the offset.
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%:z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn step(msg: &str) {
    eprintln!("\n== {}  {msg}", now());
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus(target: &str) -> PathBuf {
    let root = std::env::var_os("FUZZ_CORPUS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("fuzz/corpus"));
    let dir = root.join(target);
    let non_empty = std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if non_empty {
        dir
    } else {
        repo_root().join("fuzz/seeds").join(target)
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    v.sort();
    v
}

fn release_dir() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"));
    target.join("release")
}

fn build_release() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .current_dir(repo_root())
        .args([
            "build",
            "-q",
            "--release",
            "-j",
            "2",
            "-p",
            "galion-origin",
            "--bin",
            "galion-origin",
            "-p",
            "openmind-discovery",
            "--bin",
            "openmind-discovery",
        ])
        .status()
        .expect("run cargo build");
    assert!(st.success(), "release build failed");
}

fn http_get_status(path: &str) -> Option<u16> {
    let mut s = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], HTTP_PORT)),
        Duration::from_millis(500),
    )
    .ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut buf);
    let head = buf.get(9..12)?;
    std::str::from_utf8(head).ok()?.parse().ok()
}

/// One TCP connection per corpus file; returns (refused, status histogram).
fn replay_http(dir: &Path) -> (u32, BTreeMap<String, u32>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], HTTP_PORT));
    let mut hist = BTreeMap::new();
    let mut refused = 0;
    for path in files(dir) {
        let data = std::fs::read(&path).unwrap_or_default();
        let wire = data.get(1..).unwrap_or(&[]);
        let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
            refused += 1;
            continue;
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
        let _ = s.write_all(wire);
        let _ = s.shutdown(Shutdown::Write);
        let mut reply = Vec::new();
        let _ = s.take(64 * 1024).read_to_end(&mut reply);
        let key = match reply.get(..12) {
            Some(h) if h.starts_with(b"HTTP/1.") => String::from_utf8_lossy(&h[9..12]).into_owned(),
            _ if reply.is_empty() => "no-reply".to_string(),
            _ => "garbage".to_string(),
        };
        *hist.entry(key).or_default() += 1;
    }
    (refused, hist)
}

struct MdnsOut {
    sent: u32,
    replies: u32,
    final_records: usize,
}

fn replay_mdns(dir: &Path, svc: &str) -> MdnsOut {
    let addr = SocketAddr::from(([127, 0, 0, 1], MDNS_PORT));
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind");
    sock.set_read_timeout(Some(Duration::from_millis(2)))
        .expect("timeout");
    let mut buf = [0u8; 9000];
    let (mut sent, mut replies) = (0, 0);
    for path in files(dir) {
        let data = std::fs::read(&path).unwrap_or_default();
        if data.is_empty() || data.len() > 9000 {
            continue;
        }
        if sock.send_to(&data, addr).is_ok() {
            sent += 1;
        }
        while sock.recv_from(&mut buf).is_ok() {
            replies += 1;
        }
    }
    // The responder rate-limits to 20 replies per second: let the window roll.
    std::thread::sleep(Duration::from_millis(1100));
    while sock.recv_from(&mut buf).is_ok() {
        replies += 1;
    }
    sock.set_read_timeout(Some(Duration::from_millis(100)))
        .expect("timeout");
    let query = build_query(0x4242, &[(svc, TYPE_PTR)], false);
    sock.send_to(&query, addr).expect("send query");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut final_records = 0;
    while Instant::now() < deadline && final_records == 0 {
        if let Ok((len, _)) = sock.recv_from(&mut buf) {
            if let Ok(msg) = parse_message(&buf[..len]) {
                if msg.id == 0x4242 && msg.response {
                    final_records = msg.records.len();
                }
            }
        }
    }
    MdnsOut {
        sent,
        replies,
        final_records,
    }
}

#[test]
fn fuzz_corpus_replay_against_release_binaries() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    step("build release galion-origin + openmind-discovery (-j 2)");
    build_release();
    let bin = release_dir();
    let work = std::env::temp_dir().join(format!("fuzz-sast-2-real.{}", std::process::id()));
    std::fs::create_dir_all(work.join("root")).unwrap();

    step(&format!(
        "start galion-origin on 127.0.0.1:{HTTP_PORT} (empty ORIGIN_ROOT)"
    ));
    let origin = Kill(
        Command::new(bin.join("galion-origin"))
            .env("ORIGIN_ADDR", format!("127.0.0.1:{HTTP_PORT}"))
            .env("ORIGIN_ROOT", work.join("root"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start galion-origin"),
    );
    let up = (0..50).any(|_| {
        if http_get_status("/health") == Some(200) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
        false
    });
    assert!(up, "galion-origin /health never came up");

    let dir = corpus("http_parse_head");
    step(&format!("replay http_parse_head corpus {}", dir.display()));
    let (refused, hist) = replay_http(&dir);
    eprintln!(
        "{{\"files\":{},\"refused\":{refused},\"status\":{hist:?}}}",
        files(&dir).len()
    );
    assert_eq!(refused, 0, "connections refused: galion-origin died?");
    assert!(!hist.contains_key("garbage"), "non-HTTP reply: {hist:?}");
    let mut origin = origin;
    assert!(
        origin.0.try_wait().unwrap().is_none(),
        "galion-origin exited during the replay"
    );
    let health = http_get_status("/health");
    eprintln!("GET /health after replay -> {health:?}");
    assert_eq!(health, Some(200));

    step(&format!(
        "start openmind-discovery announce on mDNS test port {MDNS_PORT} (up to 90 s)"
    ));
    let mut ann = Kill(
        Command::new(bin.join("openmind-discovery"))
            .args([
                "announce",
                "--mdns-port",
                &MDNS_PORT.to_string(),
                "--host",
                "fuzz-real",
                "--addr",
                "127.0.0.1",
                "--duration-s",
                "90",
                "--service",
                &format!("galion:fuzz-real:{HTTP_PORT}:v=1"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start announcer"),
    );
    std::thread::sleep(Duration::from_secs(1));
    let dir = corpus("mdns_parse");
    step(&format!("replay mdns_parse corpus {}", dir.display()));
    let out = replay_mdns(&dir, "_galion._tcp.local.");
    eprintln!(
        "{{\"files\":{},\"sent\":{},\"replies_during_replay\":{},\"final_records\":{}}}",
        files(&dir).len(),
        out.sent,
        out.replies,
        out.final_records
    );
    assert!(
        ann.0.try_wait().unwrap().is_none(),
        "announcer exited during the replay"
    );
    assert!(
        out.final_records >= 1,
        "well-formed PTR query got no answer after the replay"
    );

    step("tear down");
    drop(ann);
    drop(origin);
    let _ = std::fs::remove_dir_all(&work);
    eprintln!("PASS");
}
