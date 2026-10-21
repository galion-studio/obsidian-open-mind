// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the S2 sensors `sshd` (T2.1) and `rustsec` (T2.4)
//! (gated: `GALION_REAL=1`; run with `--release` for the release binary).
//! Slot 55: ports 19550-19559 only. No network: the advisory DB is the
//! embedded offline snapshot.
//!
//! 1. `NERVE_SSHD=1 openmind-nerve replay fixtures/cortex-sshd` (the real
//!    binary): crit ssh flood + 2 vulnerability warns; no fixture address or
//!    name in the output.
//! 2. `openmind-nerve serve` on 127.0.0.1:19550 (live) with a fixture
//!    Cargo.lock and a scratch auth log: rustsec findings appear as a crit
//!    B33 event; the auth log grows 300 KiB then 1.1 MiB (warn then high
//!    bleeding); the lock is fixed and the log rotated: both clear.
//! 3. `serve` on 127.0.0.1:19552 against the workspace's own Cargo.lock:
//!    findings printed for the record (not asserted: the lock moves).

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn wait_state(port: u16, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(port, "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(secs) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn find(st: &Value, sec: &str, id: &str) -> Option<Value> {
    st["cortex"][sec]
        .as_array()?
        .iter()
        .find(|f| f["id"] == id)
        .cloned()
}

fn rustsec_ids(st: &Value) -> Vec<String> {
    st["cortex"]["vulnerabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["source"] == "rustsec")
        .filter_map(|f| f["id"].as_str().map(String::from))
        .collect()
}

fn spawn(port: u16, dir: &Path, comps: &Path, extra: &[(&str, String)]) -> Kill {
    let mut c = Command::new(BIN);
    c.arg("serve")
        .env_clear()
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
        .env("NERVE_COMPONENTS", comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", "19550-19559")
        .stderr(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    Kill(c.spawn().expect("spawn nerve"))
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn sensors_us(st: &Value) -> String {
    st["cortex_sensors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| format!("{}={}us", s["name"].as_str().unwrap_or("?"), s["us"]))
        .collect::<Vec<_>>()
        .join(" ")
}

fn append(p: &Path, n: usize) {
    let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
    f.write_all(&vec![b'.'; n]).unwrap();
}

#[test]
fn real_cortex_s2a_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-s2a-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Replay through the real binary.
    let o = Command::new(BIN)
        .env_clear()
        .env("NERVE_DATA_DIR", dir.join("replay-data"))
        .env("NERVE_SSHD", "1")
        .env("NERVE_SSHD_AUTH_LOG", "0")
        .arg("replay")
        .arg(fx.join("cortex-sshd"))
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let r: Value = serde_json::from_str(&out).unwrap();
    let flood = r["cortex"]["ddos"]["findings"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|f| f["id"] == "sshd-credential-flood")
                .cloned()
        })
        .expect("flood finding in replay output");
    assert_eq!(flood["severity"], "critical", "{flood}");
    assert_eq!(flood["source"], "auth.log");
    for leak in ["198.51.100", "203.0.113", "192.0.2", "fixturedeployer"] {
        assert!(!out.contains(leak), "{leak} in replay output");
    }
    let vulns: Vec<&str> = r["cortex"]["vulnerabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["source"] == "auth.log")
        .filter_map(|f| f["id"].as_str())
        .collect();
    assert_eq!(
        vulns,
        ["sshd-accepted-publickey-other", "sshd-invalid-user"]
    );
    eprintln!(
        "replay cortex-sshd: exit {:?} score {} grade {}; flood: {}; vulns {vulns:?}",
        o.status.code(),
        r["score"],
        r["grade"],
        flood["title"]
    );

    // 2. Live serve on 19550.
    let svc = TcpListener::bind("127.0.0.1:19551").expect("19551 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let comps = dir.join("components.json");
    std::fs::write(&comps, r#"[{"name":"live-svc","port":19551}]"#).unwrap();
    let lock = dir.join("Cargo.lock");
    std::fs::copy(fx.join("rustsec/Cargo.lock"), &lock).unwrap();
    let auth = dir.join("auth.log");
    std::fs::write(&auth, vec![b'.'; 4096]).unwrap();
    let env = [
        ("NERVE_SSHD", "1".to_string()),
        ("NERVE_SSHD_AUTH_LOG", auth.display().to_string()),
        ("NERVE_RUSTSEC_LOCK", lock.display().to_string()),
        ("NERVE_RUSTSEC_COMPONENT", "demo-app".to_string()),
    ];
    let nerve = spawn(19550, &dir, &comps, &env);
    let st = wait_state(19550, 25, |s| !rustsec_ids(s).is_empty());
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .expect("cortex_sensors")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "listeners",
            "scanners",
            "edge",
            "components",
            "ddos",
            "sshd",
            "rustsec",
            // T2.12 edge-rate is on by default since cortex-merge-all (NERVE_EDGE_HOSTS=0 turns it off).
            "edge-rate"
        ]
    );
    assert_eq!(
        rustsec_ids(&st),
        ["rustsec-smallvec-1.6.0", "rustsec-time-0.1.45"]
    );
    let ev = st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "rustsec")
        .cloned()
        .expect("rustsec event");
    let ev: NerveEvent = serde_json::from_value(ev).expect("B33 NerveEvent");
    assert_eq!(
        (ev.kind.as_str(), ev.severity.as_str()),
        ("vulnerability", "crit")
    );
    assert!(ev.ts.ends_with("+02:00") || ev.ts.ends_with("+01:00"));
    eprintln!(
        "serve :19550 cycle {}: rustsec {:?}; event {}; sensors {}; cycle_ms {}; rss {} KiB",
        st["cycle"],
        rustsec_ids(&st),
        serde_json::to_string(&ev).unwrap(),
        sensors_us(&st),
        st["cycle_ms"],
        rss_kib(nerve.0.id())
    );
    // Auth log disk bleed: +300 KiB -> warn, +1.1 MiB more -> high.
    let c0 = st["cycle"].as_u64().unwrap_or(0);
    wait_state(19550, 15, |s| s["cycle"].as_u64().unwrap_or(0) > c0);
    append(&auth, 300 * 1024);
    let st = wait_state(19550, 20, |s| {
        find(s, "bleeding", "sshd-auth-log-growth").is_some()
    });
    let b = find(&st, "bleeding", "sshd-auth-log-growth").expect("auth log growth");
    assert_eq!(
        (b["severity"].as_str(), b["source"].as_str()),
        (Some("warn"), Some("auth.log"))
    );
    eprintln!(
        "serve :19550 cycle {}: +300 KiB -> {} [{}]",
        st["cycle"], b["title"], b["severity"]
    );
    append(&auth, 1100 * 1024);
    let st = wait_state(19550, 20, |s| {
        find(s, "bleeding", "sshd-auth-log-growth").is_some_and(|b| b["severity"] == "error")
    });
    let b = find(&st, "bleeding", "sshd-auth-log-growth").unwrap();
    assert_eq!(b["severity"], "error");
    let ev = st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "auth.log" && e["kind"] == "bleeding")
        .cloned()
        .expect("auth.log bleeding event");
    assert_eq!(ev["severity"], "high");
    eprintln!(
        "serve :19550 cycle {}: +1.1 MiB -> {} [{}], event {}",
        st["cycle"],
        b["title"],
        b["severity"],
        serde_json::to_string(&ev).unwrap()
    );
    // Fix the lock, rotate the log: both clear.
    std::thread::sleep(Duration::from_millis(50));
    std::fs::copy(fx.join("rustsec/Cargo.lock.fixed"), &lock).unwrap();
    std::fs::write(&auth, b"").unwrap();
    let st = wait_state(19550, 20, |s| {
        s["cycle"].as_u64().unwrap_or(0) > 0
            && rustsec_ids(s).is_empty()
            && find(s, "bleeding", "sshd-auth-log-growth").is_none()
    });
    assert!(rustsec_ids(&st).is_empty(), "{}", st["cortex"]);
    assert!(find(&st, "bleeding", "sshd-auth-log-growth").is_none());
    eprintln!(
        "serve :19550 cycle {}: lock fixed + log rotated -> cleared; sensors {}; cycle_ms {}; rss {} KiB",
        st["cycle"],
        sensors_us(&st),
        st["cycle_ms"],
        rss_kib(nerve.0.id())
    );
    drop(nerve);

    // 3. The workspace's own lock.
    let ws_lock = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let env = [
        ("NERVE_RUSTSEC_LOCK", ws_lock.display().to_string()),
        ("NERVE_RUSTSEC_COMPONENT", "openmind-workspace".to_string()),
    ];
    let nerve = spawn(19552, &dir, &comps, &env);
    let st = wait_state(19552, 25, |s| s["cycle"].as_u64().unwrap_or(0) >= 1);
    for f in st["cortex"]["vulnerabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["source"] == "rustsec")
    {
        eprintln!(
            "serve :19552 workspace lock: [{}] {}",
            f["severity"], f["title"]
        );
    }
    eprintln!(
        "serve :19552 cycle {}: sensors {}; rss {} KiB",
        st["cycle"],
        sensors_us(&st),
        rss_kib(nerve.0.id())
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
