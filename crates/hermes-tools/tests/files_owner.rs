// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The Hermes files tools hand what they write to the sandbox user, under
//! the live gateway's UMask=0077: files_write / files_mkdir / files_move
//! (copy and move) give owner + 0644 files and 0755 folders, so the
//! sandboxed terminal can read them. As a normal user the owner is the
//! test user; as root (the VPS harness run) the config default is
//! 65534:65534, exactly like the agent tools' HERMES_RS_TOOLS_RUN_AS.

use hermes_rs::tools::{Registry, ToolCtx};
use hermes_tools::agent::run_as_from_env;
use hermes_tools::files::{register_files, FilesConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn scratch(name: &str) -> PathBuf {
    // SAFETY: umask(2) only sets the process file-mode mask.
    unsafe {
        libc::umask(0o077);
    }
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("hermes-owner-{}-{name}-{n}", std::process::id()));
    std::fs::create_dir_all(d.join("share")).unwrap();
    std::fs::create_dir_all(d.join("home")).unwrap();
    std::fs::set_permissions(d.join("share"), std::fs::Permissions::from_mode(0o755)).unwrap();
    d.canonicalize().unwrap()
}

fn cfg(d: &Path) -> FilesConfig {
    let mut m = HashMap::new();
    m.insert(
        "HERMES_RS_FILES_ROOT",
        d.join("share").display().to_string(),
    );
    m.insert("HERMES_RS_FILES_WRITE", "1".to_string());
    let mut c = FilesConfig::from_env(&|k| m.get(k).cloned(), &d.join("home"))
        .unwrap()
        .unwrap();
    let me = std::fs::metadata(d).unwrap();
    if me.uid() == 0 {
        assert_eq!(c.owner, Some((65534, 65534)), "root default = sandbox user");
    } else {
        assert_eq!(c.owner, None, "non-root default: unchanged");
        c.owner = Some((me.uid(), me.gid()));
    }
    c
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

fn check(p: &Path, w: (u32, u32), mode: u32) {
    let m = std::fs::symlink_metadata(p).unwrap();
    assert_eq!(
        m.mode() & 0o7777,
        mode,
        "{} mode {:o}",
        p.display(),
        m.mode()
    );
    assert_eq!((m.uid(), m.gid()), w, "{} owner", p.display());
}

#[test]
fn files_tools_hand_new_entries_to_the_sandbox_user() {
    let d = scratch("tools");
    let c = cfg(&d);
    let w = c.owner.unwrap();
    let mut reg = Registry::with_policy(hermes_rs::denylist::Policy::workspace(d.join("share")));
    register_files(&mut reg, c, None).unwrap();
    let s = d.join("share");
    let (ok, v) = call(&reg, "files_mkdir", json!({"dir": "", "name": "plan"}));
    assert!(ok, "{v}");
    check(&s.join("plan"), w, 0o755);
    let (ok, v) = call(
        &reg,
        "files_write",
        json!({"path": "plan/notes.md", "content": "# plan\n", "create": true}),
    );
    assert!(ok && v.get("owner_failed").is_none(), "{v}");
    check(&s.join("plan/notes.md"), w, 0o644);
    // Edit with the version from files_read.
    let (ok, r) = call(&reg, "files_read", json!({"path": "plan/notes.md"}));
    assert!(ok, "{r}");
    let (ok, v) = call(
        &reg,
        "files_write",
        json!({"path": "plan/notes.md", "content": "# plan v2\n", "version": r["version"]}),
    );
    assert!(ok, "{v}");
    check(&s.join("plan/notes.md"), w, 0o644);
    check(&s.join(".trash/edits"), w, 0o755);
    // Copy and move.
    let (ok, _) = call(&reg, "files_mkdir", json!({"dir": "", "name": "out"}));
    assert!(ok);
    let (ok, v) = call(
        &reg,
        "files_move",
        json!({"path": "plan", "dest_dir": "out", "copy": true}),
    );
    assert!(ok, "{v}");
    check(&s.join("out/plan"), w, 0o755);
    check(&s.join("out/plan/notes.md"), w, 0o644);
    // A root-only file left by the old build (0600) is fixed when moved.
    std::fs::write(s.join("old.json"), "{}").unwrap();
    assert_eq!(
        std::fs::metadata(s.join("old.json")).unwrap().mode() & 0o777,
        0o600
    );
    let (ok, v) = call(
        &reg,
        "files_move",
        json!({"path": "old.json", "dest_dir": "out"}),
    );
    assert!(ok, "{v}");
    check(&s.join("out/old.json"), w, 0o644);
}

#[test]
fn run_as_parsing_matches_the_agent_tools() {
    let none = |_: &str| None;
    assert_eq!(run_as_from_env(&none, 0).unwrap(), Some((65534, 65534)));
    assert_eq!(run_as_from_env(&none, 1000).unwrap(), None);
    let gw = |k: &str| (k == "HERMES_RS_TOOLS_RUN_AS").then(|| "gateway".to_string());
    assert_eq!(run_as_from_env(&gw, 0).unwrap(), None);
    let set = |k: &str| (k == "HERMES_RS_TOOLS_RUN_AS").then(|| "1234:4321".to_string());
    assert_eq!(run_as_from_env(&set, 0).unwrap(), Some((1234, 4321)));
    assert!(run_as_from_env(&set, 1000).is_err(), "needs a root gateway");
    let root = |k: &str| (k == "HERMES_RS_TOOLS_RUN_AS").then(|| "0:0".to_string());
    assert!(run_as_from_env(&root, 0).is_err());
    let bad = |k: &str| (k == "HERMES_RS_TOOLS_RUN_AS").then(|| "x".to_string());
    assert!(run_as_from_env(&bad, 0).is_err());
}
