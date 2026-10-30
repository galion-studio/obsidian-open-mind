// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! MCP writes behind the gate (2026-10-30): with `allow_writes: true` a
//! server's write tools are offered and called; with it off they are hidden
//! or gated (refused, nothing sent); every write call is one audit line with
//! secrets redacted; the Hermes deny list still refuses protected targets;
//! github-mcp-rs gets `--allow-writes` only when the entry says so.
//! Runs against a mock stdio MCP server that logs every call it receives.

use hermes_rs::config::{merge_mcp_servers, McpServerCfg, McpWriteTools};
use hermes_rs::tools::{Registry, Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn call(t: &Arc<dyn Tool>, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = ToolCtx {
        session_id: "audit-test",
        spend_session: "s",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &|_| {},
    };
    t.call(&args, &ctx)
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "hermes-mcpw-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

const MOCK_SECRET: &str = "mocksecret-0123456789abcdef";

fn mock(dir: &Path) -> McpServerCfg {
    use std::os::unix::fs::PermissionsExt;
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fake-mcp-writes.sh"
    );
    let sec = dir.join("mock.secrets");
    std::fs::write(&sec, format!("MOCK_SECRET={MOCK_SECRET}\n")).unwrap();
    std::fs::set_permissions(&sec, std::fs::Permissions::from_mode(0o600)).unwrap();
    McpServerCfg {
        name: "mockw".into(),
        command: "bash".into(),
        args: vec![script.into()],
        timeout_s: 5,
        connect_timeout_s: 5,
        env: [(
            "CALL_LOG".to_string(),
            dir.join("calls").display().to_string(),
        )]
        .into(),
        env_file: Some(sec.display().to_string()),
        writes_args: vec!["--allow-writes".into()],
        audit_file: Some(dir.join("audit.jsonl").display().to_string()),
        // keep the runner's uid (a user-namespace root cannot switch users)
        run_as: Some("root".into()),
        ..McpServerCfg::default()
    }
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

fn audit(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).expect("audit line is JSON"))
        .collect()
}

fn registry(cfg: &McpServerCfg) -> (Registry, Vec<hermes_rs::mcp::ServerReport>) {
    let servers = std::slice::from_ref(cfg);
    let (tools, reps) = hermes_rs::mcp::discover_report(servers);
    assert!(reps[0].error.is_none(), "{reps:?}");
    let mut reg = Registry::new();
    hermes_rs::mcp::register(&mut reg, servers, tools);
    (reg, reps)
}

#[test]
fn config_keys_writes_args_and_write_root() {
    let v = json!({"mcp_servers": {
        "github": {"command": "/x/bin/github-mcp-rs-abc", "allow_writes": true,
                   "write_root": "/var/lib/hermes-mcp/github", "audit_file": "/tmp/a.jsonl"},
        "ghoff": {"command": "/x/bin/github-mcp-rs-abc", "args": ["--allow-writes", "--timeout", "9"]},
        "other": {"command": "/x/bin/weather", "allow_writes": true},
        "custom": {"command": "/x/bin/w", "allow_writes": true, "writes_args": ["--rw"]},
        "badroot": {"command": "/x/bin/w", "write_root": "rel/dir"}
    }});
    let mut l = Vec::new();
    let warn = merge_mcp_servers(&mut l, &v, "t");
    assert!(
        warn.iter()
            .any(|w| w.contains("badroot") && w.contains("write_root")),
        "{warn:?}"
    );
    let get = |n: &str| l.iter().find(|s| s.name == n).unwrap().clone();
    let gh = get("github");
    assert!(gh.allow_writes);
    assert_eq!(gh.writes_args, ["--allow-writes"]);
    assert_eq!(gh.effective_args(), ["--allow-writes"]);
    assert_eq!(gh.write_root.as_deref(), Some("/var/lib/hermes-mcp/github"));
    assert_eq!(gh.audit_file.as_deref(), Some("/tmp/a.jsonl"));
    // writes off: a hand-written --allow-writes is stripped from argv
    assert_eq!(get("ghoff").effective_args(), ["--timeout", "9"]);
    assert!(get("other").effective_args().is_empty());
    assert_eq!(get("custom").effective_args(), ["--rw"]);
}

#[test]
fn write_tool_offered_called_and_audited_when_allowed() {
    let d = tmpdir("on");
    let mut cfg = mock(&d);
    cfg.allow_writes = true;
    cfg.write_root = Some(d.join("repo").display().to_string());
    let (reg, reps) = registry(&cfg);
    assert!(reps[0].gated.is_empty() && reps[0].skipped.is_empty());
    for n in ["argv", "push_files", "create_or_update_file"] {
        assert!(reg.get(&format!("mcp__mockw__{n}")).is_some(), "{n}");
    }
    let schemas = serde_json::to_string(&reg.schemas()).unwrap();
    assert!(
        schemas.contains("mcp__mockw__push_files"),
        "offered to the model"
    );
    // the server was started with its writes flag
    let o = call(&reg.get("mcp__mockw__argv").unwrap(), json!({}));
    assert!(o.content.contains("--allow-writes"), "{o:?}");
    // push_files has no deny-list write hint in its name: tagged as a write
    let push = reg.get("mcp__mockw__push_files").unwrap();
    assert!(
        push.deny_name().ends_with("__write"),
        "{}",
        push.deny_name()
    );
    let o = call(
        &push,
        json!({"files": [{"path": "docs/notes.md", "content": "hello"}],
               "message": format!("add notes (key {MOCK_SECRET})"),
               "token": "tok-should-never-be-logged"}),
    );
    assert!(!o.is_error, "{o:?}");
    assert!(
        o.content.contains("written ok, secret was [REDACTED]"),
        "{o:?}"
    );
    assert_eq!(calls(&d).len(), 2, "argv + push_files reached the server");
    assert!(calls(&d)[1].contains("docs/notes.md"));
    // audited: one line per write call, never for the read tool
    let a = audit(&d);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0]["tool"], "push_files");
    assert_eq!(a[0]["server"], "mockw");
    assert_eq!(a[0]["outcome"], "ok");
    assert_eq!(a[0]["session"], "audit-test");
    assert_eq!(a[0]["args"]["token"], "[REDACTED]");
    assert_eq!(a[0]["args"]["files"][0]["path"], "docs/notes.md");
    assert!(a[0]["result"].as_str().unwrap().contains("written ok"));
    let raw = std::fs::read_to_string(d.join("audit.jsonl")).unwrap();
    assert!(!raw.contains(MOCK_SECRET) && !raw.contains("tok-should-never"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(d.join("audit.jsonl")).unwrap();
        assert_eq!(m.permissions().mode() & 0o077, 0, "audit file is private");
    }
    // a server error is audited as an error
    let cu = reg.get("mcp__mockw__create_or_update_file").unwrap();
    let o = call(&cu, json!({"path": "src/lib.rs", "content": "x"}));
    assert!(!o.is_error);
    assert_eq!(audit(&d).len(), 2);
}

#[test]
fn write_tool_refused_when_allow_writes_is_false() {
    // hidden (default): not offered at all
    let d = tmpdir("off");
    let cfg = mock(&d);
    let (reg, reps) = registry(&cfg);
    assert!(reg.get("mcp__mockw__push_files").is_none());
    assert!(reg.get("mcp__mockw__create_or_update_file").is_none());
    assert!(reps[0]
        .skipped
        .iter()
        .any(|(n, w)| n == "push_files" && w.contains("allow_writes")));
    // the writes flag is not passed
    let o = call(&reg.get("mcp__mockw__argv").unwrap(), json!({}));
    assert!(!o.content.contains("--allow-writes"), "{o:?}");
    // gated: offered with its name, refused at call time, nothing sent, audited
    let d = tmpdir("gated");
    let mut cfg = mock(&d);
    cfg.write_tools = McpWriteTools::Gated;
    cfg.write_root = Some(d.join("repo").display().to_string());
    cfg.args.push("--allow-writes".into()); // hand-written flag is stripped
    let (reg, reps) = registry(&cfg);
    assert_eq!(reps[0].gated.len(), 2);
    let o = call(&reg.get("mcp__mockw__argv").unwrap(), json!({}));
    assert!(!o.content.contains("--allow-writes"), "{o:?}");
    let push = reg.get("mcp__mockw__push_files").unwrap();
    let o = call(
        &push,
        json!({"files": [{"path": "a.md", "content": "x"}], "message": "m"}),
    );
    assert!(o.is_error && o.content.contains("writes are OFF"), "{o:?}");
    assert_eq!(calls(&d).len(), 1, "only argv reached the server");
    let a = audit(&d);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0]["outcome"], "refused-writes-off");
    assert_eq!(a[0]["tool"], "push_files");
}

#[test]
fn deny_list_blocks_protected_targets_for_mcp_writes() {
    let d = tmpdir("deny");
    let mut cfg = mock(&d);
    cfg.allow_writes = true;
    // a virtual root (not created), like the GitHub entry
    cfg.write_root = Some(d.join("virtual-repo").display().to_string());
    let (reg, _) = registry(&cfg);
    let cu = reg.get("mcp__mockw__create_or_update_file").unwrap();
    let push = reg.get("mcp__mockw__push_files").unwrap();
    // allowed: an ordinary repo path
    let o = call(&cu, json!({"path": "README.md", "content": "ok"}));
    assert!(!o.is_error, "{o:?}");
    let n = calls(&d).len();
    for p in [
        "SOUL.md",
        "docs/SOUL.md",
        "AGENTS.md",
        ".env",
        ".github/workflows/ci.yml",
        "../outside.txt",
        "/etc/hermes-rs/config.yaml",
        "/etc/hermes-rs/mcp-servers.yaml",
        "/root/.hermes/config.yaml",
        "/root/.ssh/authorized_keys",
        "/opt/other/bin/tool",
        "/usr/local/bin/x",
    ] {
        let o = call(&cu, json!({"path": p, "content": "x"}));
        assert!(
            o.is_error && o.content.contains("deny list"),
            "{p} must be refused: {o:?}"
        );
        // push_files: paths nested in an array of objects, no write word in the name
        let o = call(
            &push,
            json!({"files": [{"path": "ok.md", "content": "x"}, {"path": p, "content": "x"}],
                   "message": "m"}),
        );
        assert!(
            o.is_error && o.content.contains("deny list"),
            "push_files {p} must be refused: {o:?}"
        );
    }
    assert_eq!(calls(&d).len(), n, "no refused write reached the server");
    // without a write_root (live today: no guard workspace) nothing is writable
    let d2 = tmpdir("noroot");
    let mut c2 = mock(&d2);
    c2.allow_writes = true;
    let (reg, _) = registry(&c2);
    let o = call(
        &reg.get("mcp__mockw__create_or_update_file").unwrap(),
        json!({"path": "README.md", "content": "x"}),
    );
    assert!(o.is_error && o.content.contains("deny list"), "{o:?}");
    assert!(calls(&d2).is_empty());
}

#[test]
fn github_mcp_rs_writes_follow_the_entry() {
    let d = tmpdir("gh");
    let gh = |allow: bool, args: &[&str]| {
        let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        a.extend([
            "--token-file".into(),
            d.join("none.env").display().to_string(),
        ]);
        let command: String = env!("CARGO_BIN_EXE_github-mcp-rs").into();
        McpServerCfg {
            name: "github".into(),
            writes_args: McpServerCfg::default_writes_args(&command),
            command,
            args: a,
            timeout_s: 20,
            connect_timeout_s: 10,
            allow_writes: allow,
            write_tools: McpWriteTools::Gated,
            trusted_descriptions: true,
            audit_file: Some(d.join("audit.jsonl").display().to_string()),
            run_as: Some("root".into()),
            ..McpServerCfg::default()
        }
    };
    let args = json!({"owner": "o", "repo": "r", "title": "t"});
    // writes on: hermes forwards the call and the server accepts writes
    // (it then stops at the missing token, before any network)
    let cfg = gh(true, &[]);
    assert_eq!(cfg.writes_args, ["--allow-writes"]);
    let (reg, reps) = registry(&cfg);
    assert!(reps[0].gated.is_empty());
    let t = reg.get("mcp__github__create_issue").expect("create_issue");
    let o = call(&t, args.clone());
    assert!(!o.content.contains("writes are OFF"), "{o:?}");
    assert!(o.content.contains("token"), "{o:?}");
    // writes off, even with a hand-written --allow-writes: refused
    let (reg, reps) = registry(&gh(false, &["--allow-writes"]));
    assert!(!reps[0].gated.is_empty());
    let t = reg.get("mcp__github__create_issue").unwrap();
    let o = call(&t, args);
    assert!(o.is_error && o.content.contains("writes are OFF"), "{o:?}");
    let a = audit(&d);
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a[1]["outcome"], "refused-writes-off");
}
