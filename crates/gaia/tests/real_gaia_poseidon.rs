// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary test, gated: runs only with `GALION_REAL=1`. Starts the
//! release `openmind-queue` on 127.0.0.1:19241 (slot 24), drives it through
//! the release `gaia poseidon ...` CLI (submit, inspect, spend-cap and quota
//! refusals, VRAM status), kills it with SIGKILL, restarts it on the same
//! data directory and checks through gaia that jobs, spend and caps survived.
//! Binaries: `$CARGO_TARGET_DIR/release/{gaia,openmind-queue}`. The full
//! scenario is `scripts/real/gaia-poseidon.sh`.

use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): port block movable by `GAIA_POSEIDON_REAL_PORT_BASE`; unset keeps
/// the old 19240 block.
static PORTS: Block = Block::new("GAIA_POSEIDON_REAL_PORT_BASE", 19240, 10);
fn addr_at() -> &'static str {
    PORTS.addr(1)
}

fn bin(name: &str) -> String {
    let t = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/target/gaia-poseidon".into());
    format!("{t}/release/{name}")
}

fn start(data: &std::path::Path) -> Child {
    let mut child = Command::new(bin("openmind-queue"))
        .args(["serve", "--bind", addr_at(), "--data"])
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release openmind-queue (build it first)");
    for _ in 0..50 {
        if std::net::TcpStream::connect(addr_at()).is_ok() {
            return child;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!(
        "openmind-queue did not come up on {addr_at}",
        addr_at = addr_at()
    );
}

/// Operator call with the queue's own CLI (Gaia never does these).
fn op(args: &[&str]) {
    let st = Command::new(bin("openmind-queue"))
        .args(args)
        .args(["--addr", addr_at()])
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "operator {args:?}");
}

/// `gaia poseidon ARGS --queue ADDR --compact` -> (exit code, result JSON).
fn gaia(args: &[&str]) -> (i32, Value) {
    let out = Command::new(bin("gaia"))
        .arg("poseidon")
        .args(args)
        .args(["--queue", addr_at(), "--compact"])
        .output()
        .expect("run release gaia");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap_or(-1), v["result"].clone())
}

fn metric(v: &Value, k: &str) -> i64 {
    v["report"]["metrics"][k].as_i64().unwrap_or(i64::MIN)
}

#[test]
fn gaia_poseidon_against_real_queue() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skipped: set GALION_REAL=1 (and build the release binaries)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("gaia-poseidon-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut q = start(&dir);

    op(&["post", "/caps", r#"{"scope":"agent-a","cap":50}"#]);
    op(&[
        "post",
        "/quotas",
        r#"{"agent":"agent-b","max_pending":1,"max_inflight":1}"#,
    ]);
    op(&[
        "post",
        "/vram/devices",
        r#"{"id":"gpu0","kind":"gpu","vram_gb":24,"state":"approved"}"#,
    ]);

    let (c, s) = gaia(&["status"]);
    assert_eq!(c, 0, "{s}");
    assert_eq!(s["state"], "terraforming");

    let (c, r) = gaia(&[
        "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "30",
    ]);
    assert_eq!(c, 0, "{r}");
    let id = metric(&r, "job_id");
    // The operator's worker spends the reservation; Gaia never leases or acks.
    op(&[
        "worker",
        "--worker",
        "rt-w",
        "--max-jobs",
        "1",
        "--idle-exit-ms",
        "1000",
    ]);
    let (c, r) = gaia(&[
        "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "30",
    ]);
    assert_eq!((c, metric(&r, "http_status")), (4, 402), "spend cap: {r}");
    let (c, r) = gaia(&[
        "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "10",
    ]);
    assert_eq!(c, 0, "{r}");
    let live = metric(&r, "job_id");
    assert!(live > id);

    let (c, _) = gaia(&["submit", "--agent", "agent-b", "--kind", "echo"]);
    assert_eq!(c, 0);
    let (c, r) = gaia(&["submit", "--agent", "agent-b", "--kind", "echo"]);
    assert_eq!((c, metric(&r, "http_status")), (4, 429), "quota: {r}");

    let (c, r) = gaia(&["vram"]);
    assert_eq!((c, metric(&r, "approved_gb")), (0, 24));

    // kill -9 and restart on the same data
    q.kill().unwrap();
    q.wait().unwrap();
    let (c, _) = gaia(&["job", &live.to_string()]);
    assert_eq!(c, 3, "queue down must be visible");
    let mut q = start(&dir);
    let (c, r) = gaia(&["job", &live.to_string()]);
    assert_eq!((c, metric(&r, "live")), (0, 1), "{r}");
    let (_, r) = gaia(&["caps"]);
    assert_eq!(metric(&r, "spent:agent-a"), 30, "{r}");
    assert_eq!(metric(&r, "cap:agent-a"), 50);
    let (c, _) = gaia(&[
        "submit", "--agent", "agent-a", "--kind", "echo", "--cost", "30",
    ]);
    assert_eq!(c, 4, "cap enforced after restart");

    q.kill().unwrap();
    q.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
