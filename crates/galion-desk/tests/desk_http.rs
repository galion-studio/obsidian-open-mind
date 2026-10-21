// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Behaviour tests over real HTTP. Each test runs its own desk (the built
// binary or an in-process `Desk`) on an ephemeral loopback port with every
// path under target/test-out and every probe pointed at fake or closed
// loopback services. Never touches the live state directory or port 8646.

mod support;

use galion_desk::config::{Probes, Settings};
use galion_desk::Desk;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command};
use support::*;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn settings(dir: &Path, ollama: &str, probes: Probes, hermes_bin: &str) -> Settings {
    Settings {
        bind: "127.0.0.1:0".into(),
        ollama: ollama.into(),
        state_dir: dir.join("state"),
        one_dir: dir.join("one"),
        hermes_dir: dir.join("hermes"),
        hermes_bin: hermes_bin.into(),
        probes,
    }
}

fn all_probes(addr: &str) -> Probes {
    Probes {
        hermes: addr.into(),
        one: addr.into(),
        tunnel: addr.into(),
        console: addr.into(),
        origin: addr.into(),
        room: addr.into(),
        tulu: addr.into(),
    }
}

/// Start an in-process desk and return its address.
fn start(settings: Settings) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let desk = Desk::new(settings);
    std::thread::spawn(move || desk.serve(listener));
    addr
}

const LIVE_KEYS: [&str; 22] = [
    "foundation",
    "at",
    "line",
    "model",
    "vram",
    "ctx",
    "hermes",
    "one",
    "tunnel",
    "voice",
    "heard",
    "said",
    "turns",
    "waking",
    "resting",
    "hold",
    "services",
    "board",
    "events",
    "mindpack",
    "focus",
    "notes",
];
const BOARD_KEYS: [&str; 13] = [
    "studio",
    "gpu",
    "cpu",
    "laptop_ram",
    "pod_ram",
    "hermes_ms",
    "model_ms",
    "agents",
    "version",
    "disk",
    "load",
    "pod",
    "france",
];

#[test]
fn binary_health_desk_note_404_and_413() {
    let dir = scratch("binary");
    std::fs::write(dir.join("one/voice"), "jarvis\n").unwrap();
    let addr = closed_addr();
    let closed = closed_addr();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_galion-desk"));
    cmd.env("DESK_BIND", &addr)
        .env("DESK_STATE_DIR", dir.join("state"))
        .env("DESK_ONE_DIR", dir.join("one"))
        .env("DESK_HERMES_DIR", dir.join("hermes"))
        .env("DESK_OLLAMA", &closed)
        .env("DESK_HERMES_BIN", "/bin/false")
        .env("OPENMIND_CONFIG", dir.join("missing.toml"));
    for key in [
        "HERMES", "ONE", "TUNNEL", "CONSOLE", "ORIGIN", "ROOM", "TULU",
    ] {
        cmd.env(format!("DESK_PROBE_{key}"), &closed);
    }
    let _child = Kill(cmd.spawn().unwrap());

    let health = get(&addr, "/health");
    assert!(health.starts_with("HTTP/1.1 200 OK\r\n"), "{health}");
    assert!(health.contains("content-type: application/json\r\n"));
    assert!(health.ends_with("\r\n\r\n{\"ok\":true}"), "{health}");

    let desk = json(&get(&addr, "/desk"));
    let keys: Vec<&str> = desk
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, LIVE_KEYS);
    let board: Vec<&str> = desk["board"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(board, BOARD_KEYS);
    assert_eq!(desk["foundation"], "rust");
    assert_eq!(desk["voice"], "jarvis");
    assert_eq!(desk["line"], false);
    assert_eq!(desk["mindpack"], serde_json::json!({}));
    for svc in desk["services"].as_array().unwrap() {
        assert_eq!(svc["up"], false, "{svc}");
    }

    let after = json(&post(&addr, r#"{"act":"note","text":"hello desk"}"#));
    assert_eq!(after["notes"][0]["text"], "hello desk");
    let notes = std::fs::read_to_string(dir.join("state/desk-notes.jsonl")).unwrap();
    assert!(notes.contains("hello desk"));

    assert!(get(&addr, "/nope").starts_with("HTTP/1.1 404 Not Found\r\n"));
    let big = raw(
        &addr,
        b"POST /desk HTTP/1.1\r\nContent-Length: 999999\r\n\r\n{}",
    );
    assert!(big.starts_with("HTTP/1.1 413"), "{big}");
}

#[test]
fn binary_reports_bind_failure_instead_of_panicking() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = taken.local_addr().unwrap().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_galion-desk"))
        .env("DESK_BIND", &addr)
        .env("OPENMIND_CONFIG", "/nonexistent/openmind.toml")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("bind"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}

#[test]
fn acts_change_exactly_their_files() {
    let dir = scratch("acts");
    let closed = closed_addr();
    let addr = start(settings(&dir, &closed, all_probes(&closed), "/bin/false"));
    let state = dir.join("state");

    // note: newlines flattened, 160-char cut, blank ignored
    post(
        &addr,
        &format!(r#"{{"act":"note","text":"a\nb {}"}}"#, "x".repeat(300)),
    );
    post(&addr, r#"{"act":"note","text":"   "}"#);
    post(&addr, r#"{"act":"note","text":"second \"quoted\""}"#);
    let notes = std::fs::read_to_string(state.join("desk-notes.jsonl")).unwrap();
    let lines: Vec<&str> = notes.lines().collect();
    assert_eq!(lines.len(), 2, "{notes}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["text"].as_str().unwrap().chars().count(), 160);
    assert!(first["text"].as_str().unwrap().starts_with("a b xxx"));
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["text"], "second \"quoted\"");

    // drop removes the last note only
    let snap = json(&post(&addr, r#"{"act":"drop"}"#));
    assert_eq!(snap["notes"].as_array().unwrap().len(), 1);

    // voice: only the two names are accepted
    post(&addr, r#"{"act":"voice","name":"jarvis"}"#);
    assert_eq!(
        std::fs::read_to_string(dir.join("one/voice")).unwrap(),
        "jarvis\n"
    );
    let snap = json(&post(&addr, r#"{"act":"voice","name":"../../etc"}"#));
    assert_eq!(snap["voice"], "jarvis");
    assert_eq!(
        std::fs::read_to_string(dir.join("one/voice")).unwrap(),
        "jarvis\n"
    );

    // hold toggles a marker file
    assert_eq!(json(&post(&addr, r#"{"act":"hold"}"#))["hold"], true);
    assert_eq!(
        std::fs::read_to_string(state.join("desk-hold")).unwrap(),
        "held\n"
    );
    assert_eq!(json(&post(&addr, r#"{"act":"hold"}"#))["hold"], false);
    assert!(!state.join("desk-hold").exists());

    // focus is sanitised; empty clears
    let snap = json(&post(&addr, r#"{"act":"focus","text":"task-7/../x y"}"#));
    assert_eq!(snap["focus"], "task-7xy");
    let snap = json(&post(&addr, r#"{"act":"focus","text":"!!"}"#));
    assert_eq!(snap["focus"], "");
    assert!(!state.join("desk-focus").exists());

    // unknown act and garbage body change nothing
    let before = std::fs::read_to_string(state.join("desk-notes.jsonl")).unwrap();
    post(&addr, r#"{"act":"format-disk"}"#);
    post(&addr, "not json at all");
    assert_eq!(
        std::fs::read_to_string(state.join("desk-notes.jsonl")).unwrap(),
        before
    );
}

#[test]
fn stamp_reports_line_and_hermes_from_probes() {
    let dir = scratch("stamp");
    let closed = closed_addr();
    let hermes = Fake::start("{\"status\":\"ok\"}");
    let mut probes = all_probes(&closed);
    probes.hermes = hermes.addr.clone();
    let addr = start(settings(&dir, &closed, probes, "/bin/false"));
    let snap = json(&post(&addr, r#"{"act":"stamp"}"#));
    assert_eq!(snap["notes"][0]["text"], "Stamp. Line down. Hermes up.");
    assert_eq!(snap["hermes"], true);
    let hermes_row = snap["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Hermes")
        .unwrap()
        .clone();
    assert_eq!(hermes_row["up"], true);
}

#[test]
fn wake_and_rest_post_to_the_configured_ollama() {
    let dir = scratch("wake");
    let closed = closed_addr();
    let ollama = Fake::start(
        r#"{"models":[{"name":"galion:latest","size_vram":5000,"context_length":8192}]}"#,
    );
    let addr = start(settings(
        &dir,
        &ollama.addr,
        all_probes(&closed),
        "/bin/false",
    ));

    let snap = json(&get(&addr, "/desk"));
    assert_eq!(snap["line"], true);
    assert_eq!(snap["model"], "galion:latest");
    assert_eq!(snap["vram"], 5000);
    assert_eq!(snap["ctx"], 8192);
    let laptop = snap["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Laptop")
        .unwrap()
        .clone();
    assert_eq!(laptop["up"], true, "Laptop follows the Ollama address");

    post(&addr, r#"{"act":"wake"}"#);
    assert!(
        ollama.wait_for("\"num_predict\":1"),
        "{:?}",
        ollama.requests()
    );
    post(&addr, r#"{"act":"rest"}"#);
    assert!(
        ollama.wait_for("\"keep_alive\":0"),
        "{:?}",
        ollama.requests()
    );
    let posts: Vec<String> = ollama
        .requests()
        .into_iter()
        .filter(|r| r.starts_with("POST /api/generate"))
        .collect();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(posts
        .iter()
        .any(|p| p.contains("\"model\":\"galion:latest\",\"keep_alive\":0")));
}

#[test]
fn rest_without_a_loaded_model_posts_nothing() {
    let dir = scratch("rest-idle");
    let closed = closed_addr();
    let ollama = Fake::start(r#"{"models":[]}"#);
    let addr = start(settings(
        &dir,
        &ollama.addr,
        all_probes(&closed),
        "/bin/false",
    ));
    post(&addr, r#"{"act":"rest"}"#);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        ollama.requests().iter().all(|r| !r.starts_with("POST")),
        "{:?}",
        ollama.requests()
    );
}

#[test]
fn snapshot_reads_link_board_session_and_mindpack() {
    let dir = scratch("snapshot");
    let closed = closed_addr();
    let state = dir.join("state");
    std::fs::write(
        state.join("link.json"),
        r#"{"at": "x", "laptop_port": false, "model": "galion:7b ctx 4096 (idle)", "model_ms": 12, "hermes": true, "hermes_ms": 7, "france": true, "laptop": {"name": "Mate's laptop", "gpu": "NVIDIA GeForce RTX 4060 Laptop GPU, 8188 MiB", "cpu": "13th Gen Intel(R) Core(TM) i7-13620H", "ram_gb": 32, "free_gb": 20.5}, "events": ["10:00:00  laptop up  model none  hermes up", "10:00:03  laptop down  model none  hermes up", "a", "b", "c [x]"], "urls": [{"url": "https://example.com/", "ok": true}]}"#,
    )
    .unwrap();
    std::fs::write(state.join("survival.json"), r#"{"volume_used_pct": 41.5}"#).unwrap();
    std::fs::write(state.join("machine.json"), r#"{"publicIp": "203.0.113.9"}"#).unwrap();
    std::fs::write(
        dir.join("hermes/gateway_state.json"),
        r#"{"active_agents": 3, "code_version": "0.1.0"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("one/session.jsonl"),
        "{\"heard\":\"old\",\"said\":\"old\"}\n{\"heard\":\"how are you\",\"said\":\"fine \\u00e9\"}\n",
    )
    .unwrap();
    std::fs::write(
        state.join("desk-notes.jsonl"),
        "{\"at\":1,\"text\":\"ok\"}\n{\"at\":2,\"text\":\"torn\n",
    )
    .unwrap();
    let mp = dir.join("mindpack.sh");
    std::fs::write(&mp, "#!/bin/sh\necho '{\"mind\":{\"line\":\"hi\"}}'\n").unwrap();
    make_exec(&mp);

    let addr = start(settings(
        &dir,
        &closed,
        all_probes(&closed),
        mp.to_str().unwrap(),
    ));
    let snap = json(&get(&addr, "/desk"));
    assert_eq!(snap["model"], "galion:7b");
    assert_eq!(snap["vram"], 0);
    assert_eq!(snap["ctx"], 4096);
    assert_eq!(snap["heard"], "how are you");
    assert_eq!(snap["said"], "fine é");
    assert_eq!(snap["turns"], 2);
    assert_eq!(snap["voice"], "samantha");
    assert_eq!(snap["mindpack"]["mind"]["line"], "hi");
    assert_eq!(
        snap["notes"].as_array().unwrap().len(),
        1,
        "torn line skipped"
    );
    assert_eq!(
        snap["events"],
        serde_json::json!([
            "10:00:03  laptop down  model none  hermes up",
            "a",
            "b",
            "c [x]"
        ])
    );
    let b = &snap["board"];
    assert_eq!(b["studio"], "Mate's laptop");
    assert_eq!(b["gpu"], "RTX 4060, 8188 MiB");
    assert_eq!(b["cpu"], "i7-13620H");
    assert_eq!(b["laptop_ram"], "20.5 / 32 GB");
    assert_eq!(b["hermes_ms"], 7);
    assert_eq!(b["model_ms"], 12);
    assert_eq!(b["agents"], 3);
    assert_eq!(b["version"], "0.1.0");
    assert_eq!(b["disk"], "41.5%");
    assert_eq!(b["pod"], "203.0.113.9");
    assert_eq!(b["france"], true);
    assert!(b["pod_ram"].as_str().unwrap().ends_with(" GB"));
}

#[test]
fn broken_mindpack_falls_back_to_empty_object() {
    let dir = scratch("mindpack-bad");
    let closed = closed_addr();
    let mp = dir.join("bad.sh");
    std::fs::write(&mp, "#!/bin/sh\necho 'Traceback (most recent call last)'\n").unwrap();
    make_exec(&mp);
    let addr = start(settings(
        &dir,
        &closed,
        all_probes(&closed),
        mp.to_str().unwrap(),
    ));
    assert_eq!(
        json(&get(&addr, "/desk"))["mindpack"],
        serde_json::json!({})
    );
    let addr = start(settings(
        &dir,
        &closed,
        all_probes(&closed),
        "/nonexistent/desk-hermes",
    ));
    assert_eq!(
        json(&get(&addr, "/desk"))["mindpack"],
        serde_json::json!({})
    );
}

#[test]
fn split_post_body_is_applied_whole() {
    use std::io::Write;
    let dir = scratch("split");
    let closed = closed_addr();
    let addr = start(settings(&dir, &closed, all_probes(&closed), "/bin/false"));
    let body = r#"{"act":"note","text":"arrived in two packets"}"#;
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    s.write_all(
        format!(
            "POST /desk HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    )
    .unwrap();
    s.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(150));
    s.write_all(body.as_bytes()).unwrap();
    let mut out = String::new();
    use std::io::Read;
    s.read_to_string(&mut out).unwrap();
    assert!(out.contains("arrived in two packets"), "{out}");
}

fn make_exec(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}
