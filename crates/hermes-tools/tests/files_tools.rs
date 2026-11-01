// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The Hermes file tools: config refusals, read-only by default, the write
//! round trip (version check, backups, trash), secret names, audit lines.

use hermes_rs::tools::{Registry, ToolCtx};
use hermes_tools::files::{register_files, secret_name, FilesConfig, READ_TOOLS, WRITE_TOOLS};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn scratch(name: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("hermes-files-{}-{name}-{n}", std::process::id()));
    std::fs::create_dir_all(d.join("share/docs")).unwrap();
    std::fs::create_dir_all(d.join("home")).unwrap();
    std::fs::write(d.join("share/docs/a.md"), "# hi\n").unwrap();
    std::fs::write(d.join("share/.env"), "KEY=not-a-real-key\n").unwrap();
    std::fs::write(d.join("share/blob.bin"), [0u8, 1, 2, 255, 254]).unwrap();
    d.canonicalize().unwrap()
}

fn cfg(d: &std::path::Path, write: bool) -> FilesConfig {
    let mut m = HashMap::new();
    m.insert(
        "HERMES_RS_FILES_ROOT",
        d.join("share").display().to_string(),
    );
    if write {
        m.insert("HERMES_RS_FILES_WRITE", "1".to_string());
    }
    FilesConfig::from_env(&|k| m.get(k).cloned(), &d.join("home"))
        .unwrap()
        .unwrap()
}

fn call(reg: &Registry, tool: &str, args: Value) -> (bool, Value) {
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: hermes_rs::tools::Event| {};
    let ctx = ToolCtx {
        session_id: "s1",
        spend_session: "s1",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    let out = reg.get(tool).expect(tool).call(&args, &ctx);
    (!out.is_error, serde_json::from_str(&out.content).unwrap())
}

#[test]
fn config_off_relative_and_locked_roots() {
    let d = scratch("cfg");
    let none = FilesConfig::from_env(&|_| None, &d).unwrap();
    assert!(none.is_none());
    let rel = FilesConfig::from_env(
        &|k| (k == "HERMES_RS_FILES_ROOT").then(|| "share".to_string()),
        &d,
    );
    assert!(rel.is_err());
    let slash = FilesConfig::from_env(
        &|k| (k == "HERMES_RS_FILES_ROOT").then(|| "/".to_string()),
        &d,
    );
    assert!(slash.is_err());
    // A folder holding hermes's SOUL.md is refused as a share.
    std::fs::write(d.join("home/SOUL.md"), "locked").unwrap();
    let home = d.join("home").display().to_string();
    let h = FilesConfig::from_env(&|k| (k == "HERMES_RS_FILES_ROOT").then(|| home.clone()), &d);
    assert!(h.unwrap_err().contains("SOUL.md"));
}

#[test]
fn read_only_by_default() {
    let d = scratch("ro");
    let mut reg = Registry::new();
    let names = register_files(&mut reg, cfg(&d, false), None).unwrap();
    assert_eq!(names, READ_TOOLS.to_vec());
    for w in WRITE_TOOLS {
        assert!(reg.get(w).is_none(), "{w} must not be offered read-only");
    }
    let (ok, v) = call(&reg, "files_list", json!({"path": ""}));
    assert!(ok, "{v}");
    let names: Vec<&str> = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"docs") && !names.contains(&".env"),
        "{names:?}"
    );
    let (ok, v) = call(&reg, "files_read", json!({"path": "docs/a.md"}));
    assert!(ok && v["text"] == "# hi\n", "{v}");
    let (ok, v) = call(&reg, "files_read", json!({"path": "blob.bin"}));
    assert!(
        ok && v["dump"].as_str().unwrap().contains("00 01 02 ff fe"),
        "{v}"
    );
    let (ok, v) = call(&reg, "files_read", json!({"path": ".env"}));
    assert!(
        !ok && v["error"].as_str().unwrap().contains("secrets"),
        "{v}"
    );
    let (ok, _) = call(&reg, "files_read", json!({"path": "../home/x"}));
    assert!(!ok);
    let (ok, v) = call(
        &reg,
        "files_read",
        json!({"path": "docs/a.md", "sneaky": 1}),
    );
    assert!(!ok && v["error"].as_str().unwrap().contains("not allowed"));
    let (ok, v) = call(&reg, "files_kinds", json!({}));
    assert!(ok && v.to_string().contains("rar"), "kinds table");
    // Every call left an audit line, contents never logged.
    let log = std::fs::read_to_string(d.join("home/files-audit.jsonl")).unwrap();
    assert!(log.lines().count() >= 7);
    assert!(!log.contains("not-a-real-key"));
}

#[test]
fn write_round_trip_never_deletes() {
    let d = scratch("rw");
    let mut reg = Registry::new();
    register_files(&mut reg, cfg(&d, true), None).unwrap();
    let share = d.join("share");
    // An edit needs the version from a read.
    let (ok, v) = call(
        &reg,
        "files_write",
        json!({"path": "docs/a.md", "content": "x"}),
    );
    assert!(
        !ok && v["error"].as_str().unwrap().contains("version"),
        "{v}"
    );
    let (_, t) = call(&reg, "files_read", json!({"path": "docs/a.md"}));
    let ver = t["version"].as_str().unwrap().to_string();
    let (ok, v) = call(
        &reg,
        "files_write",
        json!({"path": "docs/a.md", "content": "# changed\n", "version": ver}),
    );
    assert!(ok, "{v}");
    // The same old version is now stale.
    let (ok, _) = call(
        &reg,
        "files_write",
        json!({"path": "docs/a.md", "content": "again", "version": ver}),
    );
    assert!(!ok);
    assert_eq!(
        std::fs::read_to_string(share.join("docs/a.md")).unwrap(),
        "# changed\n"
    );
    let (ok, _) = call(
        &reg,
        "files_write",
        json!({"path": "docs/new.txt", "content": "n", "create": true}),
    );
    assert!(ok);
    let (ok, _) = call(
        &reg,
        "files_write",
        json!({"path": "docs/new.txt", "content": "n", "create": true}),
    );
    assert!(!ok, "create refuses an existing name");
    let (ok, _) = call(&reg, "files_mkdir", json!({"dir": "", "name": "out"}));
    assert!(ok);
    let (ok, _) = call(
        &reg,
        "files_rename",
        json!({"path": "docs/new.txt", "new_name": "a.md"}),
    );
    assert!(!ok, "rename never overwrites");
    let (ok, _) = call(
        &reg,
        "files_rename",
        json!({"path": "docs/new.txt", "new_name": "b.txt"}),
    );
    assert!(ok);
    let (ok, _) = call(
        &reg,
        "files_move",
        json!({"path": "docs/b.txt", "dest_dir": "out", "copy": true}),
    );
    assert!(ok);
    let (ok, _) = call(
        &reg,
        "files_move",
        json!({"path": "docs/b.txt", "dest_dir": "out"}),
    );
    assert!(!ok, "move never overwrites");
    let (ok, v) = call(&reg, "files_trash", json!({"path": "docs/b.txt"}));
    assert!(ok, "{v}");
    assert!(!share.join("docs/b.txt").exists());
    let trashed: Vec<_> = std::fs::read_dir(share.join(".trash")).unwrap().collect();
    assert!(
        trashed.len() >= 2,
        "trash keeps the file and the edits folder"
    );
    // Extract a stored zip into a fresh folder.
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("in/x.txt", o).unwrap();
    std::io::Write::write_all(&mut z, b"zipped").unwrap();
    std::fs::write(share.join("pack.zip"), z.finish().unwrap().into_inner()).unwrap();
    let (ok, v) = call(
        &reg,
        "files_read",
        json!({"path": "pack.zip", "mode": "archive"}),
    );
    assert!(ok, "{v}");
    let (ok, v) = call(&reg, "files_extract", json!({"path": "pack.zip"}));
    assert!(ok, "{v}");
    assert_eq!(
        std::fs::read_to_string(share.join("pack/in/x.txt")).unwrap(),
        "zipped"
    );
    assert!(share.join("pack.zip").exists());
}

#[test]
fn secret_names() {
    for n in [
        ".env",
        "a/.env.local",
        "id_rsa",
        "x/id_ed25519.pub",
        "tls.key",
        "c.pem",
        ".netrc",
    ] {
        assert!(secret_name(n), "{n}");
    }
    for n in ["README.md", "env.rs", "keys.txt", "main.rs"] {
        assert!(!secret_name(n), "{n}");
    }
}
