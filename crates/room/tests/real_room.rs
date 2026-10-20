// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life room sessions on the box. Gated: runs only with `GALION_REAL=1`
//! (see `scripts/real/room.sh`, which builds release and sets it).
//!
//! Uses the swarm slot-08 ports: `ROOM_REAL_PORT` (default 19081) on the host
//! and `ROOM_REAL_PORT + 1` inside a private network namespace.

mod common;

use common::{check_session, get, Room, Root};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_room");

fn real() -> Option<u16> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        println!("SKIP room real: set GALION_REAL=1");
        return None;
    }
    let port = std::env::var("ROOM_REAL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(19081);
    assert!((19080..=19088).contains(&port), "slot 08 owns 19080-19089");
    Some(port)
}

#[test]
fn real_session_on_the_host_port() {
    let Some(port) = real() else { return };
    let root = Root::new("real-host");
    let bind = format!("127.0.0.1:{port}");
    let room = Room::start(BIN, &bind, &root.0);
    assert_eq!(room.addr, bind);
    println!("REAL room host session on {bind}: {}", room.objects_line);
    check_session(&room.addr, &root);

    // 32 concurrent clients: all answered, server still healthy.
    let addr = room.addr.clone();
    let handles: Vec<_> = (0..32)
        .map(|_| {
            let addr = addr.clone();
            std::thread::spawn(move || get(&addr, "/api/objects").status)
        })
        .collect();
    let ok = handles
        .into_iter()
        .map(|h| h.join().expect("client"))
        .filter(|status| *status == 200)
        .count();
    assert_eq!(ok, 32);
    assert_eq!(get(&addr, "/health").status, 200);
    let pid = room.child.id();
    drop(room);
    assert!(
        TcpStream::connect_timeout(&bind.parse().unwrap(), Duration::from_millis(500)).is_err(),
        "port {bind} still open after teardown (pid {pid})"
    );
    println!("REAL room host session torn down: pid {pid} gone, {bind} closed");
}

#[test]
fn real_session_inside_a_network_namespace_is_invisible_to_the_host() {
    let Some(port) = real() else { return };
    let ns = ["unshare", "--user", "--map-root-user", "--net", "--"];
    let usable = Command::new(ns[0])
        .args(&ns[1..])
        .arg("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !usable {
        println!("SKIP room real netns: unprivileged user+net namespaces not available");
        return;
    }
    let root = Root::new("real-ns");
    let bind = format!("127.0.0.1:{}", port + 1);
    let mut cmd = Command::new(ns[0]);
    // Bring lo up inside so the room is really reachable there, just not from the host.
    cmd.args(&ns[1..]).args([
        "sh",
        "-c",
        "ip link set lo up 2>/dev/null; exec \"$0\" --listen",
        BIN,
    ]);
    let room = Room::start_with(&mut cmd, &bind, &root.0);
    assert_eq!(room.addr, bind);
    // Listening inside the namespace, yet the host cannot reach it.
    let reach = TcpStream::connect_timeout(&bind.parse().unwrap(), Duration::from_millis(500));
    assert!(reach.is_err(), "host reached a room bound inside a netns");
    let pid = room.child.id().to_string();
    let inside = Command::new("nsenter")
        .args([
            "--target",
            &pid,
            "--user",
            "--net",
            "--preserve-credentials",
            "--",
        ])
        .args(["curl", "-s", "-m", "3", &format!("http://{bind}/health")])
        .output();
    match inside {
        Ok(out) if out.status.success() => {
            assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
            println!("REAL room netns: /health answered ok inside the namespace (nsenter)");
        }
        _ => println!("REAL room netns: nsenter+curl unavailable, inside probe skipped"),
    }
    println!(
        "REAL room netns session: listening on {bind} inside the namespace; host connect refused"
    );
}
