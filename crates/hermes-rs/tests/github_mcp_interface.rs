// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The GitHub tools Hermes registers must be EXACTLY the Grok Bot
//! `cursor-github` interface: every `mcp__github__<name>` with the reference
//! name, description and input schema, nothing more, nothing less; write
//! tools registered but gated off. Any mismatch fails with the differences.

#[allow(dead_code)]
#[path = "../src/bin/github-mcp-rs/server.rs"]
mod github_mcp_rs;

use hermes_rs::config::{McpServerCfg, McpWriteTools};
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const REF_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/grokbot-github-mcp-schemas.json"
);
/// The swarm copy (unchunked), when present, must equal the crate copy once
/// its chunked descriptions are joined.
const SWARM_REF: &str = "/workspace/swarm/specs/grokbot-github-mcp-schemas.json";

fn call(t: &Arc<dyn Tool>, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = ToolCtx {
        session_id: "s",
        spend_session: "s",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &|_| {},
    };
    t.call(&args, &ctx)
}

fn github_cfg(extra: &[&str]) -> McpServerCfg {
    let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    if !args.iter().any(|a| a == "--token-file") {
        args.extend([
            "--token-file".to_string(),
            "/nonexistent/hermes-test/github.env".to_string(),
        ]);
    }
    McpServerCfg {
        name: "github".into(),
        command: env!("CARGO_BIN_EXE_github-mcp-rs").into(),
        args,
        timeout_s: 20,
        connect_timeout_s: 10,
        write_tools: McpWriteTools::Gated,
        trusted_descriptions: true,
        run_as: Some("root".into()),
        ..McpServerCfg::default()
    }
}

/// Independent of the server's own joiner: `description` arrays -> strings.
fn unchunk(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                match x {
                    Value::Array(p) if k == "description" && p.iter().all(|e| e.is_string()) => {
                        *x = Value::String(p.iter().map(|e| e.as_str().unwrap()).collect())
                    }
                    _ => unchunk(x),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(unchunk),
        _ => {}
    }
}

fn reference() -> Vec<Value> {
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(REF_PATH).unwrap()).unwrap();
    unchunk(&mut v);
    v["tools"].as_array().unwrap().clone()
}

#[test]
fn reference_copies_agree() {
    let raw = std::fs::read_to_string(REF_PATH).unwrap();
    assert!(
        raw.lines().all(|l| l.len() < 400),
        "a reference line is 400+ chars"
    );
    if let Ok(s) = std::fs::read_to_string(SWARM_REF) {
        let swarm: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(
            swarm["tools"].as_array().unwrap(),
            &reference(),
            "{SWARM_REF} and the crate reference differ"
        );
    }
}

#[test]
fn hermes_sees_exactly_the_grokbot_github_interface() {
    let (tools, reports) = hermes_rs::mcp::discover_report(&[github_cfg(&[])]);
    assert_eq!(reports.len(), 1);
    assert!(reports[0].error.is_none(), "{:?}", reports[0].error);
    assert!(
        reports[0].skipped.is_empty(),
        "skipped: {:?}",
        reports[0].skipped
    );

    let refs = reference();
    let want: BTreeMap<String, &Value> = refs
        .iter()
        .map(|t| (format!("mcp__github__{}", t["name"].as_str().unwrap()), t))
        .collect();
    let got: BTreeMap<String, &Arc<dyn Tool>> = tools.iter().map(|t| (t.name(), t)).collect();
    let mut diffs = Vec::new();
    for n in want.keys().filter(|n| !got.contains_key(*n)) {
        diffs.push(format!("missing in Hermes: {n}"));
    }
    for n in got.keys().filter(|n| !want.contains_key(*n)) {
        diffs.push(format!("extra in Hermes: {n}"));
    }
    for (n, r) in &want {
        let Some(t) = got.get(n) else { continue };
        if t.description() != r["description"].as_str().unwrap() {
            diffs.push(format!(
                "{n}: description differs:\n  hermes: {}\n  ref:    {}",
                t.description(),
                r["description"]
            ));
        }
        if t.parameters() != r["inputSchema"] {
            diffs.push(format!(
                "{n}: input schema differs:\n  hermes: {}\n  ref:    {}",
                t.parameters(),
                r["inputSchema"]
            ));
        }
    }
    assert!(
        diffs.is_empty(),
        "{} interface mismatch(es):\n{}",
        diffs.len(),
        diffs.join("\n")
    );
    assert_eq!(tools.len(), 75);

    // Read/write split: exactly the non-read tools are gated.
    let mut gated = reports[0].gated.clone();
    gated.sort();
    let mut writes: Vec<String> = want
        .keys()
        .filter(|n| !github_mcp_rs::is_read(n.trim_start_matches("mcp__github__")))
        .cloned()
        .collect();
    writes.sort();
    assert_eq!(gated, writes);
    assert_eq!(gated.len(), 24);

    // A gated write never reaches the server; a read without a token waits.
    let merge = got["mcp__github__merge_pull_request"];
    let o = call(merge, json!({"owner": "o", "repo": "r", "pull_number": 1}));
    assert!(
        o.is_error && o.content.contains("writes are OFF"),
        "{}",
        o.content
    );
    let me = got["mcp__github__get_me"];
    let o = call(me, json!({}));
    assert!(
        o.is_error && o.content.contains("not configured yet") && o.content.contains("github.env"),
        "{}",
        o.content
    );
}

#[test]
fn writes_hidden_mode_registers_only_reads() {
    let mut c = github_cfg(&[]);
    c.write_tools = McpWriteTools::Hidden;
    let (tools, rep) = hermes_rs::mcp::discover_report(&[c]);
    assert_eq!(tools.len(), 51);
    assert_eq!(rep[0].skipped.len(), 24);
}

/// Minimal fake GitHub API: records requests, answers a few endpoints.
fn fake_github(log: Arc<Mutex<Vec<String>>>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut first = String::new();
            if r.read_line(&mut first).is_err() {
                continue;
            }
            let mut auth = String::new();
            let mut accept = String::new();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                let lower = h.to_ascii_lowercase();
                if lower.starts_with("authorization:") {
                    auth = h[14..].trim().to_string();
                }
                if lower.starts_with("accept:") {
                    accept = h[7..].trim().to_string();
                }
                if lower.starts_with("content-length:") {
                    len = h[15..].trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let target = first.split_whitespace().nth(1).unwrap_or("").to_string();
            log.lock().unwrap().push(format!(
                "{} {target} auth={auth} accept={accept}",
                first.split_whitespace().next().unwrap_or("")
            ));
            let (code, out) = if auth != "Bearer test-token-123" {
                (401, r#"{"message":"Bad credentials"}"#.to_string())
            } else if target == "/user" {
                (200, r#"{"login":"octo"}"#.into())
            } else if target.starts_with("/repos/o/r/issues/7") {
                (200, r#"{"number":7,"title":"hello"}"#.into())
            } else if target.starts_with("/search/issues") {
                (
                    200,
                    format!(
                        r#"{{"total_count":0,"items":[],"echo":"{}"}}"#,
                        target.replace('"', "")
                    ),
                )
            } else if target.starts_with("/repos/o/r/contents/") {
                (200, "raw file text".into())
            } else {
                (404, r#"{"message":"Not Found"}"#.into())
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                out.len()
            );
        }
    });
    url
}

#[test]
fn reads_reach_github_with_the_token_and_are_marked_untrusted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("gh-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tf = dir.join("github.env");
    std::fs::write(&tf, "# test\nGITHUB_PERSONAL_ACCESS_TOKEN=test-token-123\n").unwrap();
    std::fs::set_permissions(&tf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let api = fake_github(log.clone());
    let tfs = tf.display().to_string();
    let (tools, _) =
        hermes_rs::mcp::discover_report(&[github_cfg(&["--api-url", &api, "--token-file", &tfs])]);
    let t = |n: &str| {
        tools
            .iter()
            .find(|t| t.name() == format!("mcp__github__{n}"))
            .unwrap()
            .clone()
    };

    let o = call(
        &t("get_issue"),
        json!({"owner": "o", "repo": "r", "issue_number": 7}),
    );
    assert!(!o.is_error, "{}", o.content);
    assert!(
        o.content
            .starts_with("[MCP server github output: untrusted data"),
        "{}",
        o.content
    );
    assert!(o.content.contains("\"title\":\"hello\""));
    assert!(!o.content.contains("test-token-123"));

    let o = call(&t("get_me"), json!({}));
    assert!(o.content.contains(r#"[{"login":"octo"}]"#), "{}", o.content);

    let o = call(&t("search_issues"), json!({"q": "repo:o/r bug"}));
    assert!(o.content.contains("is%3Aissue"), "{}", o.content);

    let o = call(
        &t("get_file_contents"),
        json!({"owner": "o", "repo": "r", "path": "src/a b.rs"}),
    );
    assert!(o.content.contains("raw file text"), "{}", o.content);

    let o = call(&t("get_issue"), json!({"owner": "o", "repo": "r"}));
    assert!(
        o.is_error && o.content.contains("issue_number: required"),
        "{}",
        o.content
    );

    let o = call(&t("get_repository"), json!({"owner": "o", "repo": "nope"}));
    assert!(o.is_error && o.content.contains("404"), "{}", o.content);

    let l = log.lock().unwrap().join("\n");
    assert!(l.contains("GET /repos/o/r/contents/src/a%20b.rs auth=Bearer test-token-123 accept=application/vnd.github.raw"), "{l}");
    assert!(!l.contains("POST"), "a read sent a POST: {l}");

    // Writes stay refused at the server too (no --allow-writes), even if Hermes allowed them.
    let mut c = github_cfg(&["--api-url", &api, "--token-file", &tfs]);
    c.allow_writes = true;
    let (tools, _) = hermes_rs::mcp::discover_report(&[c]);
    let ci = tools
        .iter()
        .find(|t| t.name() == "mcp__github__create_issue")
        .unwrap();
    let o = call(ci, json!({"owner": "o", "repo": "r", "title": "x"}));
    assert!(
        o.is_error
            && o.content
                .contains("writes are OFF on this GitHub MCP server"),
        "{}",
        o.content
    );
    assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("POST")));
    let _ = std::fs::rename(&tf, dir.join("github.env.bak"));
}

/// Sample arguments that satisfy a schema's required fields.
fn sample(schema: &Value) -> Value {
    let mut o = serde_json::Map::new();
    for r in schema["required"].as_array().into_iter().flatten() {
        let k = r.as_str().unwrap();
        let p = &schema["properties"][k];
        let v = match (k, p["type"].as_str()) {
            ("owner", _) => json!("galion-studio"),
            ("repo", _) => json!("openmind"),
            ("path", _) => json!("docs/README.md"),
            ("q", _) => json!("repo:galion-studio/openmind mcp"),
            ("ref", _) | ("tree_sha", _) | ("branch", _) | ("base", _) | ("head", _) => {
                json!("main")
            }
            ("tag", _) => json!("v1.0.0"),
            (_, Some("integer")) => json!(1),
            (_, Some("boolean")) => json!(false),
            (_, Some("string")) => json!("x"),
            _ => json!(1),
        };
        o.insert(k.to_string(), v);
    }
    Value::Object(o)
}

#[test]
fn deny_list_lets_every_read_tool_through_and_classes_writes() {
    use hermes_rs::denylist::{check_tool_call, Policy};
    let mut pol = Policy::from_env();
    for mode in [
        hermes_rs::denylist::Mode::Denylist,
        hermes_rs::denylist::Mode::Allowlist,
    ] {
        pol.mode = mode;
        for t in reference() {
            let n = t["name"].as_str().unwrap();
            let full = format!("mcp__github__{n}");
            let args = sample(&t["inputSchema"]);
            let r = check_tool_call(&full, &args, &pol);
            if github_mcp_rs::is_read(n) {
                assert!(r.is_ok(), "{full} {args} refused: {r:?}");
            }
        }
    }
}
