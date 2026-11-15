// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Vault fixtures in a temp directory. No child process and no socket.

use openmind_mcp_notes::{NotesServer, BRAND_LINES, SERVER_NAME, TOOL_NAMES};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const STAMP: &str = "2026-10-19T16:39:10+02:00";

struct Tmp {
    root: PathBuf,
    vault: PathBuf,
}

impl Tmp {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("openmind-mcp-notes-{}-{n}", std::process::id()));
        let vault = root.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        Self { root, vault }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write_note(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn md(title: &str, tags: &str, updated: &str, body: &str) -> String {
    format!("---\ntitle: {title}\ntags: [{tags}]\nupdated: {updated}\n---\n\n{body}\n")
}

fn server(root: &Path) -> NotesServer {
    NotesServer::open_with_stamp(root, STAMP).unwrap()
}

fn call(server: &NotesServer, name: &str, args: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    let raw = serde_json::to_vec(&req).unwrap();
    let out = server.handle_bytes(&raw);
    let text = std::str::from_utf8(&out).unwrap();
    serde_json::from_str(text.lines().next().unwrap()).unwrap()
}

fn ok(response: &Value) -> Value {
    assert_eq!(response["result"]["isError"], json!(false), "{response}");
    response["result"]["structuredContent"].clone()
}

fn tool_err(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn published(root: &Path) -> BTreeMap<String, Vec<u8>> {
    tree(root)
        .into_iter()
        .filter(|(path, _)| path != "drafts" && !path.starts_with("drafts/"))
        .collect()
}

fn fixture(root: &Path) {
    write_note(
        root,
        "physics/photon.md",
        &md(
            "Photon",
            "wave",
            "2026-10-19T16:39:10+02:00",
            "A photon has a wavelength.",
        ),
    );
    write_note(
        root,
        "physics/matter.md",
        &md(
            "Matter",
            "mass",
            "2026-10-18T10:00:00+02:00",
            "See [[Photon]].",
        ),
    );
    write_note(
        root,
        "physics/later.md",
        &md("Later", "log", "2026-10-20T12:00:00+02:00", "A later note."),
    );
    write_note(
        root,
        "physics/code.md",
        &md(
            "Code",
            "sample",
            "2026-10-19T12:00:00+02:00",
            "```\n#hidden\n[[Photon]]\n```\n\n#shown\n",
        ),
    );
}

fn with_leaks(tmp: &Tmp) {
    let outside = tmp.root.join("outside.md");
    std::fs::write(&outside, "MARKER-OUTSIDE-NOTE\n").unwrap();
    let outdir = tmp.root.join("outdir");
    std::fs::create_dir_all(&outdir).unwrap();
    std::fs::write(outdir.join("hidden.md"), "MARKER-DIR-NOTE\n").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, tmp.vault.join("leak.md")).unwrap();
        std::os::unix::fs::symlink(&outdir, tmp.vault.join("alias")).unwrap();
    }
}

#[test]
fn tools_list_is_the_seven_research_tools() {
    let tmp = Tmp::new();
    fixture(&tmp.vault);
    let server = server(&tmp.vault);
    let raw = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#;
    let init: Value = serde_json::from_slice(
        server
            .handle_bytes(raw)
            .split(|b| *b == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("drafts/"));
    for line in BRAND_LINES {
        assert!(instructions.contains(line), "{line}");
    }
    let listed: Value = serde_json::from_slice(
        server
            .handle_bytes(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .split(|b| *b == b'\n')
            .next()
            .unwrap(),
    )
    .unwrap();
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, TOOL_NAMES);
    assert_eq!(
        listed["result"]["tools"][0]["inputSchema"]["additionalProperties"],
        json!(false)
    );
    assert!(server
        .handle_bytes(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_empty());
}

#[test]
fn read_tools_do_not_write() {
    let tmp = Tmp::new();
    fixture(&tmp.vault);
    with_leaks(&tmp);
    let server = server(&tmp.vault);
    let before = tree(&tmp.vault);
    let found = ok(&call(
        &server,
        "search_notes",
        json!({"query": "wavelength"}),
    ));
    assert_eq!(found["matched"], json!(1));
    assert_eq!(found["notes"][0]["path"], json!("physics/photon.md"));
    assert_eq!(found["notes"][0]["score"], json!(1));
    assert!(found["notes"][0]["snippet"]
        .as_str()
        .unwrap()
        .contains("wavelength"));
    let photon = ok(&call(&server, "search_notes", json!({"query": "Photon"})));
    assert_eq!(photon["notes"][0]["score"], json!(4));
    let recent = ok(&call(&server, "list_recent", json!({"limit": 1})));
    assert_eq!(recent["matched"], json!(4));
    assert_eq!(recent["notes"][0]["path"], json!("physics/later.md"));
    let full = ok(&call(&server, "list_recent", json!({})));
    let order: Vec<&str> = full["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        order,
        vec![
            "physics/later.md",
            "physics/photon.md",
            "physics/code.md",
            "physics/matter.md"
        ]
    );
    let wave = ok(&call(&server, "search_tags", json!({"tag": "WAVE"})));
    assert_eq!(wave["matched"], json!(1));
    assert_eq!(wave["notes"][0]["path"], json!("physics/photon.md"));
    let shown = ok(&call(&server, "search_tags", json!({"tag": "shown"})));
    assert_eq!(shown["notes"][0]["path"], json!("physics/code.md"));
    let hidden = ok(&call(&server, "search_tags", json!({"tag": "hidden"})));
    assert_eq!(hidden["matched"], json!(0));
    let note = ok(&call(
        &server,
        "read_note",
        json!({"path": "physics/photon.md"}),
    ));
    assert_eq!(note["title"], json!("Photon"));
    assert!(note["body"].as_str().unwrap().contains("wavelength"));
    let links = ok(&call(&server, "backlinks", json!({"note": "Photon"})));
    assert_eq!(links["resolved"], json!("physics/photon.md"));
    assert_eq!(links["matched"], json!(1));
    assert_eq!(links["notes"][0]["path"], json!("physics/matter.md"));
    assert_eq!(links["notes"][0]["via"], json!("Photon"));
    let text = format!(
        "{}{}{}",
        serde_json::to_string(&found).unwrap(),
        serde_json::to_string(&note).unwrap(),
        serde_json::to_string(&links).unwrap()
    );
    assert!(!text.contains("MARKER-OUTSIDE-NOTE"));
    assert!(!text.contains("MARKER-DIR-NOTE"));
    #[cfg(unix)]
    {
        assert_eq!(found["skipped"], json!(2));
        assert_eq!(
            tool_err(&call(&server, "read_note", json!({"path": "leak.md"}))),
            "path refused"
        );
    }
    assert_eq!(before, tree(&tmp.vault));
}

#[test]
fn read_refuses_escape_and_missing() {
    let tmp = Tmp::new();
    fixture(&tmp.vault);
    let server = server(&tmp.vault);
    assert_eq!(
        tool_err(&call(
            &server,
            "read_note",
            json!({"path": "../outside.md"})
        )),
        "path refused"
    );
    assert_eq!(
        tool_err(&call(
            &server,
            "read_note",
            json!({"path": "/etc/hostname"})
        )),
        "path refused"
    );
    assert_eq!(
        tool_err(&call(
            &server,
            "read_note",
            json!({"path": "physics/photon.txt"})
        )),
        "path refused"
    );
    assert_eq!(
        tool_err(&call(&server, "read_note", json!({"path": "missing.md"}))),
        "note not found"
    );
    let bad = call(&server, "search_notes", json!({"query": "x", "limit": 0}));
    assert_eq!(bad["error"]["code"], json!(-32602));
    let extra = call(
        &server,
        "create_draft_note",
        json!({"title": "Nope", "path": "physics/photon.md"}),
    );
    assert_eq!(extra["error"]["code"], json!(-32602));
    assert!(!tmp.vault.join("drafts").exists());
}

#[test]
fn drafts_are_the_only_writes() {
    let tmp = Tmp::new();
    fixture(&tmp.vault);
    let server = server(&tmp.vault);
    let before = published(&tmp.vault);
    let photon_before = std::fs::read(tmp.vault.join("physics/photon.md")).unwrap();
    let matter_before = std::fs::read(tmp.vault.join("physics/matter.md")).unwrap();

    let draft = ok(&call(
        &server,
        "create_draft_note",
        json!({"title": "Lab note", "body": "Draft body.", "tags": ["lab"]}),
    ));
    assert_eq!(draft["path"], json!("drafts/lab-note.md"));
    assert_eq!(draft["created"], json!(STAMP));
    assert_eq!(draft["title"], json!("Lab note"));
    assert!(draft["body"].as_str().unwrap().contains("Draft body."));
    let file = std::fs::read(tmp.vault.join("drafts/lab-note.md")).unwrap();
    assert_eq!(draft["bytes"], json!(file.len()));
    assert_eq!(before, published(&tmp.vault));

    let second = ok(&call(
        &server,
        "create_draft_note",
        json!({"title": "Lab note", "body": "Second."}),
    ));
    assert_eq!(second["path"], json!("drafts/lab-note-2.md"));
    assert_eq!(
        std::fs::read(tmp.vault.join("drafts/lab-note.md")).unwrap(),
        file
    );

    let sneaky = ok(&call(
        &server,
        "create_draft_note",
        json!({"title": "../../etc/passwd", "body": "stays in drafts"}),
    ));
    assert_eq!(sneaky["path"], json!("drafts/etc-passwd.md"));
    assert!(sneaky["path"].as_str().unwrap().starts_with("drafts/"));
    assert_eq!(before, published(&tmp.vault));

    let refused = call(
        &server,
        "create_draft_note",
        json!({"title": "Bad", "tags": ["bad tag"]}),
    );
    assert_eq!(tool_err(&refused), "tag refused");
    assert!(!tree(&tmp.vault).contains_key("drafts/bad.md"));

    let proposal = ok(&call(
        &server,
        "link_notes",
        json!({"from": "Matter", "to": "Photon"}),
    ));
    assert_eq!(proposal["kind"], json!("proposal"));
    assert_eq!(proposal["wrote"], json!(true));
    let proposal_path = proposal["path"].as_str().unwrap();
    assert!(proposal_path.starts_with("drafts/links/"));
    let proposal_body = std::fs::read_to_string(tmp.vault.join(proposal_path)).unwrap();
    assert!(proposal_body.contains("[[physics/matter.md]]"));
    assert!(proposal_body.contains("[[physics/photon.md]]"));
    assert_eq!(
        std::fs::read(tmp.vault.join("physics/photon.md")).unwrap(),
        photon_before
    );
    assert_eq!(
        std::fs::read(tmp.vault.join("physics/matter.md")).unwrap(),
        matter_before
    );
    let again = ok(&call(
        &server,
        "link_notes",
        json!({"from": "physics/matter.md", "to": "physics/photon.md"}),
    ));
    assert_eq!(again["wrote"], json!(false));
    assert_eq!(again["path"], json!(proposal_path));
    let link_files: Vec<_> = tree(&tmp.vault)
        .into_keys()
        .filter(|path| path.starts_with("drafts/links/"))
        .collect();
    assert_eq!(link_files.len(), 1);

    let appended = ok(&call(
        &server,
        "link_notes",
        json!({"from": "drafts/lab-note.md", "to": "Photon"}),
    ));
    assert_eq!(appended["kind"], json!("append"));
    assert_eq!(appended["wrote"], json!(true));
    assert_eq!(appended["path"], json!("drafts/lab-note.md"));
    let linked = std::fs::read_to_string(tmp.vault.join("drafts/lab-note.md")).unwrap();
    assert_eq!(linked.matches("[[physics/photon.md]]").count(), 1);
    let appended_again = ok(&call(
        &server,
        "link_notes",
        json!({"from": "drafts/lab-note.md", "to": "Photon"}),
    ));
    assert_eq!(appended_again["wrote"], json!(false));
    let linked_again = std::fs::read_to_string(tmp.vault.join("drafts/lab-note.md")).unwrap();
    assert_eq!(linked_again.matches("[[physics/photon.md]]").count(), 1);
    assert_eq!(before, published(&tmp.vault));

    let back = ok(&call(
        &server,
        "backlinks",
        json!({"note": "physics/photon.md"}),
    ));
    let paths: Vec<&str> = back["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"physics/matter.md"));
    assert!(paths.contains(&proposal_path));
    assert!(paths.contains(&"drafts/lab-note.md"));
}

#[test]
fn ambiguous_title_writes_nothing() {
    let tmp = Tmp::new();
    fixture(&tmp.vault);
    write_note(
        &tmp.vault,
        "a.md",
        &md("Twin", "a", "2026-10-19T16:39:10+02:00", "One twin."),
    );
    write_note(
        &tmp.vault,
        "b.md",
        &md("Twin", "b", "2026-10-19T16:39:10+02:00", "Other twin."),
    );
    let server = server(&tmp.vault);
    let before = tree(&tmp.vault);
    assert_eq!(
        tool_err(&call(
            &server,
            "link_notes",
            json!({"from": "Twin", "to": "Photon"})
        )),
        "note name is ambiguous"
    );
    assert_eq!(
        tool_err(&call(&server, "backlinks", json!({"note": "Twin"}))),
        "note name is ambiguous"
    );
    assert_eq!(before, tree(&tmp.vault));
}

#[test]
fn live_stamp_is_zagreb() {
    let tmp = Tmp::new();
    let server = NotesServer::open(&tmp.vault).unwrap();
    let draft = ok(&call(
        &server,
        "create_draft_note",
        json!({"title": "Clock", "body": "now"}),
    ));
    let stamp = draft["created"].as_str().unwrap();
    assert_eq!(stamp.len(), 25);
    assert!(stamp.ends_with("+01:00") || stamp.ends_with("+02:00"));
    assert!(!stamp.contains('Z'));
}

#[test]
fn doc_names_the_tools_and_the_old_list() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mcp-notes.md");
    let text = std::fs::read_to_string(path).unwrap();
    for name in TOOL_NAMES {
        assert!(text.contains(name), "{name}");
    }
    assert!(text.contains("stack_status"));
    assert!(text.contains("drafts/"));
}

#[test]
fn sources_do_not_spawn_delete_or_open_a_socket() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "std::net",
        "TcpListener",
        "reqwest",
        "tokio",
        "hyper",
        "Command::new",
        "remove_file",
        "remove_dir",
    ];
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{} contains {needle}",
                path.display()
            );
        }
    }
}
