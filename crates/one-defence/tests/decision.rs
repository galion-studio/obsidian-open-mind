// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! A defence decision does not apply a firewall change, does not spawn
//! iptables or nft, and does not open a socket.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture() -> PathBuf {
    manifest().join("tests/fixtures/health.db")
}

fn socket_ids() -> Vec<String> {
    let mut found = Vec::new();
    let entries = fs::read_dir("/proc/self/fd").unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        if let Ok(target) = fs::read_link(entry.path()) {
            let text = target.to_string_lossy();
            if let Some(rest) = text.strip_prefix("socket:") {
                found.push(rest.to_owned());
            }
        }
    }
    found.sort();
    found
}

fn child_pids() -> Vec<String> {
    let mut pids = Vec::new();
    let tasks = fs::read_dir("/proc/self/task").unwrap();
    for task in tasks {
        let task = task.unwrap();
        let path = task.path().join("children");
        if let Ok(text) = fs::read_to_string(path) {
            pids.extend(text.split_whitespace().map(str::to_owned));
        }
    }
    pids.sort();
    pids
}

fn source_tokens(src: &str) -> Vec<String> {
    src.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn crate_sources() -> String {
    let mut names: Vec<PathBuf> = fs::read_dir(manifest().join("src"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    names.sort();
    let mut combined = String::new();
    for path in names {
        combined.push_str(&fs::read_to_string(&path).unwrap());
        combined.push('\n');
    }
    combined
}

fn write_trap(dir: &Path, name: &str, log: &Path) {
    let path = dir.join(name);
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$0\" >> '{}'\nexit 99\n",
        log.display()
    );
    fs::write(&path, body).unwrap();
    let mut perm = fs::metadata(&path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&path, perm).unwrap();
}

#[test]
fn defence_decision_does_not_apply_a_firewall_change() {
    let rows = one_defence::query_path(&fixture()).unwrap();
    let sockets_before = socket_ids();
    let children_before = child_pids();

    let decision = one_defence::decide(&rows);
    let empty = one_defence::decide(&[]);

    let sockets_after = socket_ids();
    let children_after = child_pids();

    assert_eq!(sockets_before, sockets_after, "decision opened a socket");
    assert_eq!(
        children_before, children_after,
        "decision spawned a process"
    );

    assert!(!decision.applies_firewall_change());
    assert!(!decision.spawns_firewall_tool());
    assert!(!decision.opens_socket());
    assert!(!empty.applies_firewall_change());
    assert!(!empty.spawns_firewall_tool());
    assert!(!empty.opens_socket());
    assert!(empty.notes().is_empty());

    assert_eq!(decision.notes().len(), rows.len());
    let whisper = decision
        .notes()
        .iter()
        .find(|note| note.service == "whisper")
        .expect("whisper");
    assert_eq!(whisper.status, "degraded");
    assert!(!whisper.healthy);
    assert_eq!(whisper.ms, 0);
    let console = decision
        .notes()
        .iter()
        .find(|note| note.service == "console")
        .expect("console");
    assert!(console.healthy);
    assert_eq!(console.ms, 45);

    let src = crate_sources();
    let tokens = source_tokens(&src);
    for banned in ["iptables", "nft", "nftables", "ip6tables"] {
        assert!(
            !tokens.iter().any(|word| word == banned),
            "crate source names {banned}"
        );
    }
    for banned in [
        "std::process::Command",
        "process::Command",
        "Command::",
        ".spawn(",
        ".output(",
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "UnixStream",
        "UnixListener",
    ] {
        assert!(!src.contains(banned), "{banned} in crate source");
    }

    let trap_dir = std::env::temp_dir().join(format!("one-defence-trap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&trap_dir);
    fs::create_dir_all(&trap_dir).unwrap();
    let log = trap_dir.join("spawned");
    for name in ["iptables", "nft", "iptables-nft", "ip6tables"] {
        write_trap(&trap_dir, name, &log);
    }
    let path = {
        let old = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", trap_dir.display(), old)
    };
    let out = Command::new(env!("CARGO_BIN_EXE_one-defence"))
        .arg("--json")
        .arg(fixture())
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "status {:?}\nstderr {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !log.exists(),
        "spawned iptables or nft: {}",
        fs::read_to_string(&log).unwrap_or_default()
    );

    let again = socket_ids();
    assert_eq!(
        sockets_after, again,
        "binary run left a socket in this test"
    );
}
