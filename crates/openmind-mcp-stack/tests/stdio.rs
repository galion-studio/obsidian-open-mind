// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! End-to-end over stdio: the real `openmind-mcp-stack` binary, a real
//! `openmind-memory` server (the repo's own `App` on an ephemeral loopback
//! port, temp data dir, Bearer key on) and a temp Obsidian vault.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

const KEY: &str = "test-key-not-a-secret";

fn tmpdir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Start the repo's openmind-memory on 127.0.0.1:<ephemeral>. Returns the URL.
fn start_memory(data: &Path) -> String {
    let d = data.to_string_lossy().to_string();
    let cfg = openmind_memory::config::Config::from_env(&move |k| match k {
        "TDAI_DATA_DIR" => Some(d.clone()),
        "TDAI_INSTANCE_ID" => Some("galion".into()),
        "TDAI_GATEWAY_API_KEY" => Some(KEY.into()),
        _ => None,
    });
    let app = Arc::new(openmind_memory::api::App::new(
        openmind_memory::engine::Engine::new(cfg),
    ));
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        openmind_memory::http::serve(
            l,
            1 << 20,
            16,
            Arc::new(move |r: openmind_memory::http::Request| app.handle(r)),
        )
    });
    format!("http://127.0.0.1:{port}")
}

fn make_vault(root: &Path) {
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::write(
        root.join("00-summary.md"),
        "# Summary\nThe Galion build session.\nKettle is red.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("sub/deep.md"),
        "deep note\nmentions the kettle again\n",
    )
    .unwrap();
    std::fs::write(root.join("notes.txt"), "not a note").unwrap();
    std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
    std::fs::write(root.join(".obsidian/app.json"), "{}").unwrap();
    let outside = root.parent().unwrap().join("outside-secret.md");
    std::fs::write(&outside, "outside the vault").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link-out.md")).unwrap();
    std::os::unix::fs::symlink("/etc", root.join("etc-link")).unwrap();
}

/// (relative path, bytes) of every entry under root, links not followed.
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let md = std::fs::symlink_metadata(e.path()).unwrap();
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if md.file_type().is_symlink() {
                out.push((
                    rel,
                    std::fs::read_link(e.path())
                        .unwrap()
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                ));
            } else if md.is_dir() {
                out.push((rel, Vec::new()));
                stack.push(e.path());
            } else {
                out.push((rel, std::fs::read(e.path()).unwrap()));
            }
        }
    }
    out.sort();
    out
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    id: u64,
}

impl Mcp {
    fn spawn(args: &[&str], key: Option<&str>) -> Mcp {
        let mut c = Command::new(env!("CARGO_BIN_EXE_openmind-mcp-stack"));
        c.args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(k) = key {
            c.env("TDAI_GATEWAY_API_KEY", k);
        }
        let mut child = c.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Mcp {
            child,
            stdin,
            out,
            id: 0,
        }
    }
    fn send(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], self.id, "{v}");
        v
    }
    fn call(&mut self, tool: &str, args: Value) -> (bool, String, Value) {
        let v = self.send("tools/call", json!({"name": tool, "arguments": args}));
        let r = &v["result"];
        (
            r["isError"] == true,
            r["content"][0]["text"].as_str().unwrap_or("").to_string(),
            r["structuredContent"].clone(),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn protocol_handshake_and_tool_list() {
    let root = tmpdir("proto");
    let vault = root.join("vault");
    make_vault(&vault);
    let url = start_memory(&root.join("mem"));
    let mut m = Mcp::spawn(
        &["--memory-url", &url, "--vault", vault.to_str().unwrap()],
        Some(KEY),
    );
    let v = m.send(
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}}),
    );
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(v["result"]["serverInfo"]["name"], "openmind-mcp-stack");
    writeln!(
        m.stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    let v = m.send("tools/list", json!({}));
    let names: Vec<&str> = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "memory_search",
            "memory_recall",
            "memory_save",
            "memory_recent",
            "vault_list",
            "vault_read",
            "vault_search"
        ]
    );
    assert!(!names.iter().any(|n| n.contains("write")));
    let v = m.send("no/such", json!({}));
    assert_eq!(v["error"]["code"], -32601);
    // garbage line -> parse error, server keeps running
    writeln!(m.stdin, "not json").unwrap();
    let mut line = String::new();
    m.out.read_line(&mut line).unwrap();
    assert!(line.contains("-32700"), "{line}");
    let v = m.send("ping", json!({}));
    assert_eq!(v["result"], json!({}));
}

#[test]
fn memory_save_recall_search_recent_round_trip() {
    let root = tmpdir("mem");
    let url = start_memory(&root.join("mem"));
    let mut m = Mcp::spawn(
        &["--memory-url", &url, "--session-key", "mcp-test"],
        Some(KEY),
    );
    m.send("initialize", json!({}));
    let marker = format!("zebra-marker-{}", std::process::id());
    let (e, t, s) = m.call(
        "memory_save",
        json!({"text": format!("Mate prefers the {marker} kettle colour red")}),
    );
    assert!(!e, "{t}");
    assert_eq!(s["saved"], 1, "{t}");

    // Server here is the repo build: /recall echoes scope only once the
    // session-scope change is merged. scope=all works on both.
    let (e, t, s) = m.call("memory_recall", json!({"query": marker, "scope": "all"}));
    assert!(!e, "{t}");
    assert!(s["context"].as_str().unwrap().contains(&marker), "{t}");

    let (e, t, _) = m.call(
        "memory_search",
        json!({"query": marker, "kind": "conversations"}),
    );
    assert!(!e, "{t}");
    assert!(t.contains(&marker), "{t}");

    let (e, t, s) = m.call("memory_recent", json!({"limit": 3}));
    assert!(!e, "{t}");
    assert!(
        s["items"][0]["text"].as_str().unwrap().contains(&marker),
        "{t}"
    );
    assert_eq!(s["items"][0]["session_key"], "mcp-test");

    let (e, t, _) = m.call("memory_save", json!({}));
    assert!(e && t.contains("text is required"));
}

#[test]
fn session_scope_fails_closed_or_is_confirmed() {
    let root = tmpdir("scope");
    let url = start_memory(&root.join("mem"));
    let mut m = Mcp::spawn(&["--memory-url", &url], Some(KEY));
    m.send("initialize", json!({}));
    let (e, t, s) = m.call("memory_recall", json!({"query": "anything"}));
    // Either the server confirmed scope=session, or the tool refused.
    if e {
        assert!(t.contains("did not confirm session scope"), "{t}");
    } else {
        assert_eq!(s["scope"], "session", "{t}");
    }
}

#[test]
fn wrong_key_is_an_error_and_key_is_never_echoed() {
    let root = tmpdir("key");
    let url = start_memory(&root.join("mem"));
    let mut m = Mcp::spawn(&["--memory-url", &url], Some("wrong-key-value"));
    m.send("initialize", json!({}));
    let (e, t, _) = m.call("memory_recent", json!({}));
    assert!(e, "{t}");
    assert!(t.contains("401"), "{t}");
    assert!(!t.contains("wrong-key-value"));
}

#[test]
fn memory_down_is_a_tool_error_not_a_crash() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let mut m = Mcp::spawn(&["--memory-url", &format!("http://127.0.0.1:{port}")], None);
    m.send("initialize", json!({}));
    let (e, t, _) = m.call("memory_search", json!({"query": "x y"}));
    assert!(e && t.contains("unreachable"), "{t}");
    let v = m.send("ping", json!({}));
    assert_eq!(v["result"], json!({}));
}

#[test]
fn vault_read_only_tools_and_refusals() {
    let root = tmpdir("vault");
    let vault = root.join("vault");
    make_vault(&vault);
    let before = snapshot(&vault);
    let mut m = Mcp::spawn(&["--no-memory", "--vault", vault.to_str().unwrap()], None);
    m.send("initialize", json!({}));
    let v = m.send("tools/list", json!({}));
    assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 3);

    let (e, t, s) = m.call("vault_list", json!({}));
    assert!(!e, "{t}");
    let paths: Vec<&str> = s["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        ["00-summary.md", "sub/deep.md"],
        "no dot dirs, no symlinks, no .txt"
    );
    assert!(s["notes"][0]["modified"].as_str().unwrap().ends_with(":00"));

    let (e, t, _) = m.call("vault_read", json!({"path": "00-summary.md"}));
    assert!(!e && t.contains("Kettle is red."), "{t}");
    let (e, t, s) = m.call("vault_read", json!({"path": "sub/deep.md", "max_bytes": 4}));
    assert!(!e && t.ends_with("deep") && s["truncated"] == true, "{t}");

    let (e, t, s) = m.call("vault_search", json!({"query": "KETTLE"}));
    assert!(!e, "{t}");
    assert_eq!(s["count"], 2, "{t}");

    for bad in [
        "../outside-secret.md",
        "sub/../../outside-secret.md",
        "/etc/passwd",
        "link-out.md",
        "etc-link/passwd.md",
        ".git/config",
        ".obsidian/app.json",
        "notes.txt",
        "sub",
    ] {
        let (e, t, _) = m.call("vault_read", json!({"path": bad}));
        assert!(e, "{bad} must be refused: {t}");
        assert!(
            !t.contains("outside the vault") && !t.contains("[core]") && !t.contains("root:"),
            "{bad}: {t}"
        );
    }
    let (e, t, _) = m.call("vault_list", json!({"folder": ".git"}));
    assert!(e, "{t}");
    let (e, t, _) = m.call("vault_list", json!({"folder": "etc-link"}));
    assert!(e, "{t}");

    for (tool, args) in [
        (
            "vault_write",
            json!({"path": "00-summary.md", "content": "pwned"}),
        ),
        ("vault_delete", json!({"path": "00-summary.md"})),
        ("vault_create", json!({"path": "new.md", "content": "x"})),
        (
            "vault_rename",
            json!({"from": "00-summary.md", "to": "x.md"}),
        ),
    ] {
        let (e, t, _) = m.call(tool, args);
        assert!(e && t.contains("read-only"), "{tool}: {t}");
    }
    // extra fields on a read tool do not turn it into a write
    let (e, _, _) = m.call(
        "vault_read",
        json!({"path": "00-summary.md", "content": "pwned", "mode": "w"}),
    );
    assert!(!e);
    drop(m);
    assert_eq!(
        before,
        snapshot(&vault),
        "vault must be byte-identical after every call"
    );
}

#[test]
fn vault_missing_root_fails_at_start() {
    let mut c = Command::new(env!("CARGO_BIN_EXE_openmind-mcp-stack"));
    let st = c
        .args(["--no-memory", "--vault", "/nonexistent/vault-dir"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.code(), Some(2));
}
