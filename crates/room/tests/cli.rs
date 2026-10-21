// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Hermetic end-to-end tests of the `room` binary: every CLI mode and a full
//! HTTP session on a free loopback port (`127.0.0.1:0`, no fixed port).

mod common;

use common::{check_session, Room, Root};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_room");

fn run(args: &[&str], root: &Root) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .env("ROOM_ROOT", &root.0)
        .env_remove("ROOM_BIND")
        .output()
        .expect("run room");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn help_version_and_usage_errors() {
    let root = Root::new("cli");
    let (code, out, _) = run(&["--help"], &root);
    assert_eq!(code, 0);
    assert!(out.starts_with("usage: room"));
    let (code, out, _) = run(&["--version"], &root);
    assert_eq!(code, 0);
    assert!(out.starts_with("room 0."));
    for bad in [
        &["--bogus"][..],
        &["--fixture"],
        &["--listen", "0.0.0.0:80"],
        &["a", "b"],
    ] {
        let (code, out, err) = run(bad, &root);
        assert_eq!(code, 2, "{bad:?}");
        assert!(out.is_empty(), "{bad:?}");
        assert!(err.contains("usage: room"), "{bad:?}: {err}");
    }
}

#[test]
fn page_mode_reads_the_configured_root() {
    let root = Root::new("page");
    let (code, out, err) = run(&[], &root);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("{\"objects\":["));
    assert!(out.contains("\"note\":\"Operator guide\""));
    assert!(out.contains("\"name\":\"galion-app\""));
    assert!(!out.contains("secret-keys"));
    assert!(!out.contains("private outside"));
    // Manifests that do not exist are no longer listed.
    assert!(!out.contains("galion-console"));
}

#[test]
fn fixture_mode_single_file_dir_and_missing() {
    let root = Root::new("fixture");
    let (code, out, _) = run(&["--fixture", &root.path("galion-light/docs")], &root);
    assert_eq!(code, 0);
    assert!(out.contains("Operator guide"));
    assert!(!out.contains("\"kind\":\"live\""));
    let (code, out, _) = run(
        &["--fixture", &root.path("galion-light/docs/guide.md")],
        &root,
    );
    assert_eq!(code, 0);
    assert_eq!(out.matches("\"kind\":").count(), 1);
    let (code, out, _) = run(&["--fixture", &root.path("nope.md")], &root);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "{\"objects\":[]}");
}

#[test]
fn listen_full_session_on_a_free_port() {
    let root = Root::new("listen");
    let room = Room::start(BIN, "127.0.0.1:0", &root.0);
    assert!(room.addr.starts_with("127.0.0.1:"));
    assert!(room.objects_line.starts_with("room: ") && room.objects_line.ends_with(" objects"));
    check_session(&room.addr, &root);
}

#[test]
fn listen_bind_failure_is_a_clean_error() {
    let root = Root::new("bindfail");
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap().to_string();
    let out = Command::new(BIN)
        .arg("--listen")
        .env("ROOM_BIND", &addr)
        .env("ROOM_ROOT", &root.0)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("cannot bind {addr}")), "{err}");
}
