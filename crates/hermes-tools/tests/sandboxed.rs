// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The tools for real, through openmind-sandbox, on a throwaway git repo.
//! Skips (with a SKIP line) where the kernel has neither Landlock nor user
//! namespaces, or git/dd/install are missing.

use hermes_rs::config::SpendCfg;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{Event, Registry, ToolCtx, ToolOutput};
use hermes_tools::CodingConfig;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn skip() -> bool {
    let f = openmind_sandbox::features();
    let missing = ["git", "dd", "install"]
        .iter()
        .any(|b| hermes_tools::exec::system_bin(b).is_none());
    if (f.landlock_abi < 1 && !f.userns) || missing {
        eprintln!("SKIP: no landlock/userns or no git/dd/install on this machine");
        return true;
    }
    false
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

struct Fx {
    root: PathBuf,
    wt: PathBuf,
    reg: Registry,
    spend: Arc<SpendTracker>,
}

fn fixture(name: &str, caps: SpendCfg, extra: &[(&str, String)]) -> Fx {
    let root = std::env::temp_dir().join(format!("hermes-tools-sbx-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("wt/src")).unwrap();
    let root = root.canonicalize().unwrap();
    let wt = root.join("wt");
    std::fs::write(wt.join("src/lib.rs"), "pub fn one() -> u32 { 1 }\n").unwrap();
    std::fs::write(wt.join("README.md"), "hello\n").unwrap();
    git(&wt, &["init", "-q"]);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "init"]);
    git(&wt, &["checkout", "-qb", "swarm/test-ticket"]);
    std::os::unix::fs::symlink("/etc/hostname", wt.join("link-out")).unwrap();
    let mut vars: HashMap<String, String> = HashMap::from([
        ("HERMES_RS_CODING_WORKTREE".into(), wt.display().to_string()),
        ("HERMES_RS_CODING_BRANCH".into(), "swarm/test-ticket".into()),
        (
            "HERMES_RS_CODING_STATE".into(),
            root.join("state").display().to_string(),
        ),
    ]);
    for (k, v) in extra {
        vars.insert(k.to_string(), v.clone());
    }
    let mut cfg = CodingConfig::from_env(&move |k| vars.get(k).cloned(), &root.join("home"))
        .unwrap()
        .unwrap();
    hermes_tools::preflight(&mut cfg, &caps).unwrap();
    let spend = Arc::new(SpendTracker::new(caps, None));
    let mut reg = Registry::new();
    hermes_tools::register(&mut reg, cfg, Some(spend.clone()));
    Fx {
        root,
        wt,
        reg,
        spend,
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn caps() -> SpendCfg {
    SpendCfg {
        session_usd_cap: 1.0,
        daily_usd_cap: 2.0,
        daily_call_cap: 0,
    }
}

fn call(fx: &Fx, session: &str, tool: &str, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: Event| {};
    let ctx = ToolCtx {
        session_id: session,
        spend_session: session,
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    fx.reg.get(tool).unwrap().call(&args, &ctx)
}

fn body(o: &ToolOutput) -> Value {
    serde_json::from_str(&o.content).unwrap_or(json!({"raw": o.content}))
}

#[test]
fn read_write_git_commit_on_branch() {
    if skip() {
        return;
    }
    let fx = fixture("flow", caps(), &[]);
    let r = call(&fx, "s1", "fs_read", json!({"path": "src/lib.rs"}));
    assert!(!r.is_error, "{}", r.content);
    assert_eq!(body(&r)["content"], "pub fn one() -> u32 { 1 }\n");
    let r = call(
        &fx,
        "s1",
        "fs_read",
        json!({"path": "src/lib.rs", "offset": 4, "max_bytes": 2}),
    );
    assert_eq!(body(&r)["content"], "fn");
    let w = call(
        &fx,
        "s1",
        "fs_write",
        json!({"path": "src/lib.rs", "content": "pub fn one() -> u32 { 2 }\n"}),
    );
    assert!(!w.is_error, "{}", w.content);
    let w = call(
        &fx,
        "s1",
        "fs_write",
        json!({"path": "src/deep/new.rs", "content": "// new\n"}),
    );
    assert!(!w.is_error, "{}", w.content);
    assert_eq!(
        std::fs::read_to_string(fx.wt.join("src/deep/new.rs")).unwrap(),
        "// new\n"
    );
    let s = call(&fx, "s1", "git", json!({"op": "status"}));
    assert!(!s.is_error, "{}", s.content);
    let st = body(&s)["status"].as_str().unwrap().to_string();
    assert!(
        st.contains("M src/lib.rs") && st.contains("?? src/deep/new.rs"),
        "{st}"
    );
    let d = call(&fx, "s1", "git", json!({"op": "diff"}));
    assert!(body(&d)["diff"]
        .as_str()
        .unwrap()
        .contains("+pub fn one() -> u32 { 2 }"));
    let c = call(
        &fx,
        "s1",
        "git",
        json!({"op": "commit", "message": "fix: one is two"}),
    );
    assert!(!c.is_error, "{}", c.content);
    let sha = body(&c)["commit"].as_str().unwrap().to_string();
    assert_eq!(sha.len(), 40);
    assert_eq!(body(&c)["pushed"], false);
    assert_eq!(git(&fx.wt, &["rev-parse", "swarm/test-ticket"]), sha);
    assert_eq!(
        git(&fx.wt, &["log", "-1", "--format=%an %s"]),
        "hermes-rs fix: one is two"
    );
    // main untouched
    assert_eq!(git(&fx.wt, &["log", "-1", "--format=%s", "main"]), "init");
    // audit: allowed + ran lines, content never logged
    let audit = std::fs::read_to_string(fx.root.join("state/audit.jsonl")).unwrap();
    assert!(audit.lines().count() >= 10);
    assert!(audit.contains("\"decision\":\"allowed\"") && audit.contains("\"decision\":\"ran\""));
    assert!(
        !audit.contains("pub fn one() -> u32 { 2 }"),
        "file contents leaked into the audit log"
    );
    assert!(
        audit.contains("+02:00") || audit.contains("+01:00"),
        "Zagreb offset"
    );
}

#[test]
fn escapes_are_refused_and_audited() {
    if skip() {
        return;
    }
    let fx = fixture("esc", caps(), &[]);
    let canary = fx.root.join("canary.txt");
    std::fs::write(&canary, "keep").unwrap();
    let cases = [
        (
            "fs_read",
            json!({"path": "../../../../etc/passwd"}),
            "traversal",
        ),
        (
            "fs_read",
            json!({"path": "/etc/shadow"}),
            "absolute path outside",
        ),
        ("fs_read", json!({"path": "link-out"}), "symlink"),
        ("fs_read", json!({"path": ".git/config"}), "off-limits"),
        (
            "fs_write",
            json!({"path": "../canary.txt", "content": "gone"}),
            "traversal",
        ),
        (
            "fs_write",
            json!({"path": canary.display().to_string(), "content": "gone"}),
            "absolute path outside",
        ),
        (
            "fs_write",
            json!({"path": ".git/hooks/pre-commit", "content": "#!/bin/sh\nrm -rf /\n"}),
            "off-limits",
        ),
        (
            "fs_write",
            json!({"path": ".cargo/config.toml", "content": "[build]\nrustc-wrapper='/bin/rm'"}),
            "not allowed",
        ),
        ("git", json!({"op": "push"}), "never allowed"),
        (
            "git",
            json!({"op": "push", "remote": "origin"}),
            "not allowed",
        ),
        (
            "git",
            json!({"op": "checkout", "paths": ["main"]}),
            "not allowed",
        ),
        ("git", json!({"op": "config"}), "not allowed"),
        (
            "git",
            json!({"op": "commit", "message": "x", "paths": ["../../etc"]}),
            "traversal",
        ),
        (
            "git",
            json!({"op": "diff", "paths": ["--output=/tmp/pwn"]}),
            "",
        ),
        ("cargo", json!({"command": "run"}), "not allowed"),
        (
            "cargo",
            json!({"command": "test", "args": "; rm -rf /"}),
            "not allowed",
        ),
    ];
    for (tool, args, want) in cases {
        let o = call(&fx, "s-esc", tool, args.clone());
        if want.is_empty() {
            // `--output=...` is a path, so it lands as a pathspec after `--`
            // (or is refused); it must never create the file.
            assert!(!Path::new("/tmp/pwn").exists());
            continue;
        }
        assert!(o.is_error, "{tool} {args} was not refused: {}", o.content);
        assert!(o.content.contains(want), "{tool} {args}: {}", o.content);
    }
    assert_eq!(std::fs::read_to_string(&canary).unwrap(), "keep");
    let audit = std::fs::read_to_string(fx.root.join("state/audit.jsonl")).unwrap();
    let refused = audit
        .lines()
        .filter(|l| l.contains("\"decision\":\"refused\""))
        .count();
    assert!(refused >= 15, "{refused} refusals logged");
}

#[test]
fn wrong_branch_is_refused() {
    if skip() {
        return;
    }
    let fx = fixture("branch", caps(), &[]);
    git(&fx.wt, &["checkout", "-q", "main"]);
    let o = call(
        &fx,
        "s",
        "git",
        json!({"op": "commit", "message": "sneak onto main"}),
    );
    assert!(
        o.is_error && o.content.contains("not the allowlisted branch"),
        "{}",
        o.content
    );
    assert_eq!(git(&fx.wt, &["log", "-1", "--format=%s"]), "init");
}

#[test]
fn spend_cap_and_call_cap_block_tools() {
    if skip() {
        return;
    }
    let fx = fixture(
        "caps",
        caps(),
        &[("HERMES_RS_CODING_MAX_CALLS", "2".into())],
    );
    fx.spend.record("rich", "anthropic/claude-opus-5.5", 1.5);
    let o = call(&fx, "rich", "fs_read", json!({"path": "README.md"}));
    assert!(
        o.is_error && o.content.contains("session spend cap"),
        "{}",
        o.content
    );
    assert!(!call(&fx, "c", "fs_read", json!({"path": "README.md"})).is_error);
    assert!(!call(&fx, "c", "fs_read", json!({"path": "README.md"})).is_error);
    let o = call(&fx, "c", "fs_read", json!({"path": "README.md"}));
    assert!(
        o.is_error && o.content.contains("call cap"),
        "{}",
        o.content
    );
}

#[test]
fn sandbox_layers_are_reported_for_runs() {
    if skip() {
        return;
    }
    let fx = fixture("layers", caps(), &[]);
    call(&fx, "s", "fs_read", json!({"path": "README.md"}));
    let audit = std::fs::read_to_string(fx.root.join("state/audit.jsonl")).unwrap();
    let ran: Value = audit
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["decision"] == "ran")
        .unwrap();
    let on: Vec<&str> = ran["result"]["sandbox"]["enforced"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for need in ["rlimits", "env", "no_new_privs", "net"] {
        assert!(on.contains(&need), "{need} not enforced: {ran}");
    }
}
