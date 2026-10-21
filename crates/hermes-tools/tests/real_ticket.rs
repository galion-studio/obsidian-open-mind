// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1): the hermes-rs agent loop, its real curl
//! provider, the real `hermes-mock-model` binary on 127.0.0.1:19172, and the
//! sandboxed tools drive fake ticket FAKE-1 end to end (read, cargo test red,
//! edit, cargo test green, diff, commit on the branch). No paid model call:
//! the provider runs with HERMES_RS_PROVIDER_LOOPBACK_ONLY=1 and no key.
//! The full HTTP run (hermes-gateway binary) is crates/galion-realtest/tests/real_sh_hermes_tools.rs.

use hermes_rs::agent::{boss, Turn};
use hermes_rs::config::Config;
use hermes_rs::provider::{resolve_target, CurlProvider, Reasoning};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use hermes_tools::CodingConfig;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MOCK_PORT: u16 = 19172;

struct Kill(Child, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=setup",
            "-c",
            "user.email=s@l",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn real_fake_ticket_through_agent_loop_and_mock_model() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 for the real-life test");
        return;
    }
    let fix = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let root = std::env::temp_dir().join(format!("hermes-tools-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let wt = root.join("ticket");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::copy(fix.join("ticket/Cargo.toml"), wt.join("Cargo.toml")).unwrap();
    std::fs::copy(fix.join("ticket/lib.rs.buggy"), wt.join("src/lib.rs")).unwrap();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    git(&wt, &["init", "-q"]);
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "FAKE-1: buggy adder"]);
    git(&wt, &["checkout", "-qb", "swarm/fake-ticket"]);

    let mock = Command::new(env!("CARGO_BIN_EXE_hermes-mock-model"))
        .args(["--port", &MOCK_PORT.to_string(), "--scripts"])
        .arg(fix.join("mock-scripts"))
        .spawn()
        .expect("start mock");
    let _guard = Kill(mock, root.clone());
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", MOCK_PORT)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let sysroot = String::from_utf8(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let env: HashMap<String, String> = HashMap::from([
        (
            "HERMES_HOME".into(),
            root.join("home").display().to_string(),
        ),
        (
            "OPENROUTER_BASE_URL".into(),
            format!("http://127.0.0.1:{MOCK_PORT}/v1"),
        ),
        ("HERMES_RS_SESSION_USD_CAP".into(), "1.00".into()),
        ("HERMES_RS_DAILY_USD_CAP".into(), "2.00".into()),
        ("HERMES_RS_CODING_WORKTREE".into(), wt.display().to_string()),
        ("HERMES_RS_CODING_BRANCH".into(), "swarm/fake-ticket".into()),
        (
            "HERMES_RS_CODING_STATE".into(),
            root.join("state").display().to_string(),
        ),
        (
            "HERMES_RS_CODING_TOOLCHAIN".into(),
            sysroot.trim().to_string(),
        ),
        ("HERMES_RS_PROVIDER_LOOPBACK_ONLY".into(), "1".into()),
    ]);
    let e2 = env.clone();
    let get = move |k: &str| e2.get(k).cloned();
    let cfg = Arc::new(Config::load(&get).unwrap());
    let mut cc = CodingConfig::from_env(&get, &cfg.home).unwrap().unwrap();
    for n in hermes_tools::preflight(&mut cc, &cfg.spend).unwrap() {
        eprintln!("preflight: {n}");
    }
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let mut tools = Registry::new();
    hermes_tools::register(&mut tools, cc, Some(spend.clone()));
    let e3 = env.clone();
    let provider = Arc::new(CurlProvider::new(Arc::new(move |k: &str| {
        e3.get(k).cloned()
    })));
    let agent = boss(cfg.clone(), provider, spend, tools);
    assert!(agent.tools.names().contains(&"delegate_task".to_string()));

    let turn = Turn {
        session_id: "real-ticket".into(),
        spend_session: "real-ticket".into(),
        target: resolve_target(&cfg, None, None),
        reasoning: Reasoning::Default,
        system: "You fix tickets.".into(),
        messages: vec![json!({"role": "user", "content": "Do FAKE-1. SCRIPT:ticket"})],
        max_tokens: None,
        depth: 0,
        max_iterations: 20,
        cancel: Arc::new(AtomicBool::new(false)),
        steer: Arc::new(Mutex::new(vec![])),
    };
    let res = agent.run(turn, &|_e| {});
    assert!(res.completed, "turn failed: {:?}", res.error);
    assert_eq!(res.tool_calls, 7);
    assert!(
        res.final_text.starts_with("FAKE-1 done"),
        "{}",
        res.final_text
    );
    // the red run, then the green run
    let tool_msgs: Vec<String> = res
        .added
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        tool_msgs[2].contains("test tests::adds ... FAILED"),
        "{}",
        tool_msgs[2]
    );
    assert!(
        tool_msgs[4].contains("test tests::adds ... ok"),
        "{}",
        tool_msgs[4]
    );
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%an|%s", "swarm/fake-ticket"]),
        "hermes-rs|fix(adder): add() adds instead of subtracting (FAKE-1)"
    );
    assert_eq!(
        git(&wt, &["log", "-1", "--format=%s", "main"]),
        "FAKE-1: buggy adder"
    );
    assert!(std::fs::read_to_string(wt.join("src/lib.rs"))
        .unwrap()
        .contains("a + b"));
}
