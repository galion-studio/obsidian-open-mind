// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/hermes-tools.sh` (slot 17; deleted on
//! swarm/realscript-port-1). Real-life test for hermes-tools:
//!
//! * builds the release hermes-gateway with the coding tools
//!   (`--no-default-features --features coding`, kept as
//!   `release/hermes-gateway-coding-only`) and the scripted mock model server,
//! * starts both on 127.0.0.1,
//! * drives three sessions through the real HTTP API:
//!   - ticket: git status, read, cargo test (red), edit, cargo test (green),
//!     diff, commit on the work branch
//!   - escape: traversal, absolute, symlink, .git hooks, .cargo config,
//!     "shell rm -rf /", cargo arg injection, git push, then a test file that
//!     tries network / writes / rm -rf / read / git push from inside cargo
//!     test in the sandbox
//!   - spend: mock reports $0.60 per call; the $1.00 session cap and the
//!     $2.00 daily cap stop it
//! * checks the outcome, prints evidence, and kills everything it started.
//!
//! No real model call: HERMES_RS_PROVIDER_LOOPBACK_ONLY=1, no provider key in
//! the environment (`env_clear`), provider base URL = the local mock.
//!
//! Ports: block 19170 (`GALION_REAL_PORT_BASE`): +0 gateway, +1 mock model.
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_hermes_tools -- --nocapture`

use galion_realtest::http;
use galion_realtest::script::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn git(args: &[&str]) -> std::process::Output {
    let mut c = Command::new("git");
    c.args([
        "-c",
        "user.name=setup",
        "-c",
        "user.email=setup@local",
        "-c",
        "init.defaultBranch=main",
    ])
    .args(args);
    let o = run(c);
    assert!(o.status.success(), "git {args:?}: {}", err(&o));
    o
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// `grep -o "ESCAPE [a-z_.:0-9-]*: [A-Z]*" | sort -u`
fn escapes(text: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("ESCAPE ") {
        let tail = &rest[i + 7..];
        let name_len = tail
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || "_.:-".contains(c)))
            .unwrap_or(tail.len());
        // The name class includes ':', so (like grep's backtracking) the
        // ": " separator may be the name's last ':' plus the space after it.
        let name_end = if tail[..name_len].ends_with(':') && tail[name_len..].starts_with(' ') {
            name_len - 1
        } else {
            name_len
        };
        if let Some(word) = tail[name_end..].strip_prefix(": ") {
            let w_len = word
                .find(|c: char| !c.is_ascii_uppercase())
                .unwrap_or(word.len());
            v.push(format!("ESCAPE {}: {}", &tail[..name_end], &word[..w_len]));
        }
        rest = &tail[name_len..];
    }
    v.sort();
    v.dedup();
    v
}

#[test]
fn real_sh_hermes_tools() {
    if !enabled("hermes-tools") {
        return;
    }
    let base = port_base(19170);
    let (gw_port, mock_port) = (base, base + 1);
    require_free(&[gw_port, mock_port]);
    build(&["-p", "hermes-tools", "--bin", "hermes-mock-model"]);
    build(&[
        "-p",
        "hermes-gateway",
        "--no-default-features",
        "--features",
        "coding",
    ]);
    let gw = release_bin("hermes-gateway-coding-only");
    if std::env::var("GALION_REAL_SKIP_BUILD").as_deref() != Ok("1") {
        fs::copy(release_bin("hermes-gateway"), &gw).expect("keep the coding-only gateway");
        // Put the default-feature gateway back (a cached re-link, no rebuild).
        build(&["-p", "hermes-gateway"]);
    }
    let gw = need_bin(&gw);
    let mock = need_bin(&release_bin("hermes-mock-model"));
    let fix = repo_root().join("crates/hermes-tools/tests/fixtures");

    let dir = TempDir(scratch("hermes-tools-real"));
    let run_dir = dir.0.clone();
    for d in ["home", "canary"] {
        fs::create_dir_all(run_dir.join(d)).unwrap();
    }
    fs::write(run_dir.join("canary/keep.txt"), "keep\n").unwrap();
    let wt = run_dir.join("ticket");
    fs::create_dir_all(wt.join("src")).unwrap();
    fs::copy(fix.join("ticket/Cargo.toml"), wt.join("Cargo.toml")).unwrap();
    fs::copy(fix.join("ticket/lib.rs.buggy"), wt.join("src/lib.rs")).unwrap();
    fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    fs::write(wt.join("README.md"), "# adder (fake ticket FAKE-1)\n").unwrap();
    std::os::unix::fs::symlink("/etc/hostname", wt.join("link-out")).unwrap();
    let remote = run_dir.join("remote.git");
    git(&["init", "-q", "--bare", s(&remote)]);
    git(&["-C", s(&wt), "init", "-q"]);
    git(&["-C", s(&wt), "add", "-A"]);
    git(&["-C", s(&wt), "commit", "-qm", "FAKE-1: buggy adder"]);
    git(&["-C", s(&wt), "remote", "add", "origin", s(&remote)]);
    git(&["-C", s(&wt), "push", "-q", "origin", "main"]);
    git(&["-C", s(&wt), "checkout", "-qb", "swarm/fake-ticket"]);
    say(&format!(
        "fake ticket repo {} on swarm/fake-ticket, origin = {}",
        wt.display(),
        remote.display()
    ));

    let sysroot = out(&run({
        let mut c = Command::new("rustc");
        c.args(["--print", "sysroot"]);
        c
    }))
    .trim()
    .to_string();
    let key = random_hex(24); // test-only gateway key, never printed
    let host_file = repo_root().join("Cargo.toml"); // outside the ticket worktree

    let mut procs = Procs::new();
    let mut c = Command::new(&mock);
    c.args([
        "--port",
        &mock_port.to_string(),
        "--scripts",
        s(&fix.join("mock-scripts")),
    ])
    .args(["--log", s(&run_dir.join("mock.jsonl"))])
    .args([
        "--var",
        &format!("CANARY={}", run_dir.join("canary").display()),
    ])
    .args(["--var", &format!("MOCK_PORT={mock_port}")])
    .args(["--var", &format!("HOST_FILE={}", host_file.display())])
    .stdout(fs::File::create(run_dir.join("mock.out")).unwrap())
    .stderr(fs::File::create(run_dir.join("mock.err")).unwrap());
    procs.spawn("hermes-mock-model", c);

    let mut c = Command::new(&gw);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TZ", "Europe/Zagreb")
        .env("HERMES_HOME", run_dir.join("home"))
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", gw_port.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_CRON", "0")
        .env("API_SERVER_KEY", &key)
        .env(
            "OPENROUTER_BASE_URL",
            format!("http://127.0.0.1:{mock_port}/v1"),
        )
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env("HERMES_RS_SESSION_USD_CAP", "1.00")
        .env("HERMES_RS_DAILY_USD_CAP", "2.00")
        .env("HERMES_RS_CODING_WORKTREE", &wt)
        .env("HERMES_RS_CODING_BRANCH", "swarm/fake-ticket")
        .env("HERMES_RS_CODING_STATE", run_dir.join("state"))
        .env("HERMES_RS_CODING_TOOLCHAIN", &sysroot)
        .stdin(Stdio::null())
        .stdout(fs::File::create(run_dir.join("gateway.out")).unwrap())
        .stderr(fs::File::create(run_dir.join("gateway.err")).unwrap());
    procs.spawn("hermes-gateway", c);

    let gw_addr = format!("127.0.0.1:{gw_port}");
    let mock_addr = format!("127.0.0.1:{mock_port}");
    let up = wait_until(Duration::from_secs(10), || {
        http::get(&gw_addr, "/health").is_ok_and(|r| r.status == 200)
            && http::get(&mock_addr, "/health").is_ok_and(|r| r.status == 200)
    });
    say("gateway log:");
    println!("{}", tail(&run_dir.join("gateway.err"), 30));
    assert!(up, "gateway/mock did not come up");
    let h = http::get(&gw_addr, "/health")
        .map(|r| r.text())
        .unwrap_or_default();
    say(&format!(
        "GET /health: {}",
        h.chars().take(300).collect::<String>()
    ));

    let auth = format!("Bearer {key}");
    let chat = |session: &str, script: &str| -> String {
        let body = serde_json::json!({"model": "default", "messages": [{"role": "user", "content": format!("Run SCRIPT:{script}")}]})
            .to_string();
        let r = http::request(
            &gw_addr,
            "POST",
            "/v1/chat/completions",
            &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
                ("X-Hermes-Session-Id", session),
            ],
            body.as_bytes(),
            Duration::from_secs(900),
        );
        r.map(|r| r.text()).unwrap_or_else(|e| format!("error {e}"))
    };
    let answer = |resp: &str| -> String {
        jt(&json(resp), "choices.0.message.content")
            .chars()
            .take(600)
            .collect()
    };
    let refs = || -> String {
        out(&git(&[
            "-C",
            s(&remote),
            "for-each-ref",
            "--format=%(refname)",
        ]))
        .lines()
        .collect::<Vec<_>>()
        .join(" ")
    };
    let mut ck = Checks::new();

    say("=== session 1: fake ticket FAKE-1 ===");
    let r = chat("real-ticket-1", "ticket");
    fs::write(run_dir.join("ticket.resp"), &r).unwrap();
    say(&format!("assistant: {}", answer(&r)));
    print!(
        "{}",
        out(&git(&[
            "-C",
            s(&wt),
            "log",
            "--format=  %h %an <%ae> %s",
            "-3",
            "swarm/fake-ticket"
        ]))
    );
    ck.eq(
        "commit on swarm/fake-ticket by hermes-rs",
        "hermes-rs",
        out(&git(&[
            "-C",
            s(&wt),
            "log",
            "-1",
            "--format=%an",
            "swarm/fake-ticket",
        ]))
        .trim(),
    );
    ck.eq(
        "main untouched",
        "FAKE-1: buggy adder",
        out(&git(&["-C", s(&wt), "log", "-1", "--format=%s", "main"])).trim(),
    );
    ck.eq(
        "origin has no new branch (nothing pushed)",
        "refs/heads/main",
        refs(),
    );
    ck.has("src/lib.rs fixed", "a + b", &read(&wt.join("src/lib.rs")));
    let mocklog = read(&run_dir.join("mock.jsonl"));
    for l in mocklog
        .lines()
        .filter(|l| l.contains("\"script\":\"ticket\""))
    {
        let mut v = json(l);
        if let Some(o) = v.as_object_mut() {
            o.remove("tools_offered");
        }
        println!(
            "  mock: {}",
            v.to_string().chars().take(260).collect::<String>()
        );
    }

    say("=== session 2: escape attempts ===");
    let r = chat("real-escape-1", "escape");
    fs::write(run_dir.join("escape.resp"), &r).unwrap();
    say(&format!("assistant: {}", answer(&r)));
    let mocklog = read(&run_dir.join("mock.jsonl"));
    for l in mocklog
        .lines()
        .filter(|l| l.contains("\"script\":\"escape\""))
    {
        let v = json(l);
        println!(
            "  step result: {}  -> next: {}",
            jt(&v, "last_tool_result")
                .chars()
                .take(200)
                .collect::<String>(),
            jt(&v, "reply").chars().take(120).collect::<String>()
        );
    }
    ck.ok(
        "canary intact",
        read(&run_dir.join("canary/keep.txt")).trim() == "keep"
            && !run_dir.join("canary/pwned.txt").exists(),
        "",
    );
    ck.ok(
        "no .git/hooks/pre-commit written",
        !wt.join(".git/hooks/pre-commit").exists(),
        "",
    );
    ck.ok(
        "no .cargo/config.toml written",
        !wt.join(".cargo/config.toml").exists(),
        "",
    );
    ck.eq("origin still only main", "refs/heads/main", refs());
    let esc = escapes(&mocklog);
    for e in &esc {
        println!("  {e}");
    }
    let blocked = esc.iter().filter(|e| e.ends_with("BLOCKED")).count();
    ck.ok(
        "escape test ran inside the sandbox: 7 attempts, all BLOCKED",
        blocked == 7 && !esc.iter().any(|e| e.ends_with("SUCCEEDED")),
        &format!("{blocked} BLOCKED of {}", esc.len()),
    );

    say("=== session 3+4+5: spend caps ($0.60 per mock call; session cap $1.00, daily $2.00) ===");
    for n in 1..=3 {
        let r = chat(&format!("real-spend-{n}"), "spend");
        fs::write(run_dir.join(format!("spend{n}.resp")), &r).unwrap();
        say(&format!(
            "spend-{n}: {}",
            r.chars().take(300).collect::<String>()
        ));
    }
    let n1 = read(&run_dir.join("mock.jsonl"))
        .matches("\"script\":\"spend\"")
        .count();
    ck.eq(
        "mock saw exactly 4 spend calls (2 per session, 0 for the third)",
        "4",
        n1.to_string(),
    );

    say("=== audit log summary (state/audit.jsonl) ===");
    let audit = read(&run_dir.join("state/audit.jsonl"));
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for l in audit.lines() {
        let v = json(l);
        *counts.entry(jt(&v, "decision")).or_default() += 1;
        if v["decision"] == "refused" {
            println!(
                "  refused {}: {}",
                jt(&v, "tool"),
                jt(&v, "reason").chars().take(180).collect::<String>()
            );
        }
    }
    for (k, v) in &counts {
        println!("  {v:>5} {k}");
    }
    ck.ok(
        "mock never saw a key in Authorization",
        !read(&run_dir.join("mock.jsonl")).contains("\"authorization_header_sent\":true"),
        "",
    );
    ck.ok(
        "no non-loopback connection from the gateway",
        !read(&run_dir.join("gateway.err")).contains("refusing non-loopback"),
        "",
    );
    let all_out = [
        read(&run_dir.join("gateway.err")),
        read(&run_dir.join("gateway.out")),
        read(&run_dir.join("mock.jsonl")),
        audit,
    ];
    ck.ok(
        "test gateway key never in logs",
        !all_out.iter().any(|b| b.contains(&key)),
        "",
    );

    drop(procs);
    std::thread::sleep(Duration::from_millis(300));
    let open = [gw_port, mock_port]
        .iter()
        .filter(|p| port_open(**p))
        .count();
    say(&format!(
        "processes stopped; listeners left on our ports: {open}"
    ));
    ck.eq("listeners left", "0", open.to_string());
    ck.finish("hermes-tools");
}

#[test]
fn escapes_parse_like_grep() {
    let t = r#"ESCAPE net_loopback_mock_model: BLOCKED (x)\nESCAPE dns_github.com: BLOCKED\nESCAPE a:b: SUCCEEDED"#;
    assert_eq!(
        escapes(t),
        vec![
            "ESCAPE a:b: SUCCEEDED".to_string(),
            "ESCAPE dns_github.com: BLOCKED".to_string(),
            "ESCAPE net_loopback_mock_model: BLOCKED".to_string()
        ]
    );
}
