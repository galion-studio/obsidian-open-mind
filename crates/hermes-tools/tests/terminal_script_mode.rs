// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Script mode end to end: the real terminal tool, the real deny list
//! (HERMES_RS_GUARD_SCRIPT_MODE=1) and the real sandbox. One test in this
//! file on purpose: the policy is read from the environment once per process.

use hermes_rs::tools::{Event, Registry, ToolCtx, ToolOutput};
use hermes_tools::agent::{self, AgentToolsConfig};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn call(reg: &Registry, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: Event| {};
    let ctx = ToolCtx {
        session_id: "s1",
        spend_session: "s1",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    reg.get("terminal").unwrap().call(&args, &ctx)
}

#[test]
fn workspace_scripts_run_and_bad_ones_are_refused() {
    let root = std::env::temp_dir().join(format!(
        "hermes-script-mode-{}-{}",
        std::process::id(),
        hermes_rs::new_id("t")
    ));
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let ws = ws.canonicalize().unwrap();
    // set before the first Policy::global() call in this process
    std::env::set_var("HERMES_RS_GUARD_WORKSPACE", &ws);
    std::env::set_var("HERMES_RS_GUARD_SCRIPT_MODE", "1");

    let mut reg = Registry::new();
    agent::register(&mut reg, AgentToolsConfig::for_tests(&ws), None, None, None);

    let desc = reg.get("terminal").unwrap().description();
    assert!(desc.contains("script"), "{desc}");

    std::fs::write(
        ws.join("build.sh"),
        "#!/bin/sh\nset -eu\nN=${1:-3}\nOUT=\"out-$N.txt\"\n: > \"$OUT\"\n\
         for i in $(seq 1 \"$N\"); do\n  echo \"line $i of $N\" >> \"$OUT\"\ndone\n\
         if [ -s \"$OUT\" ]; then\n  echo \"wrote $(wc -l < \"$OUT\") lines\"\nfi\n",
    )
    .unwrap();
    std::fs::write(
        ws.join("bad.sh"),
        "#!/bin/sh\necho hi\ncurl http://example.com\n",
    )
    .unwrap();
    std::fs::write(ws.join("sneaky.sh"), "#!/bin/sh\nPATH=/tmp\nls\n").unwrap();

    // a rejected script never runs, sandbox or not
    for (cmd, why) in [
        ("sh bad.sh", "network tool inside the script"),
        ("sh sneaky.sh", "PATH rewrite inside the script"),
        ("sh /etc/profile", "script outside the workspace"),
    ] {
        let o = call(&reg, json!({"command": cmd}));
        assert!(o.is_error, "{why}: {}", o.content);
    }

    if std::env::var_os("CI").is_some() || !openmind_sandbox::features().userns_full {
        eprintln!("SKIP: no sandbox here, run part skipped");
        return;
    }
    for cmd in ["sh build.sh 4", "./build.sh 4"] {
        if cmd.starts_with("./") {
            use std::os::unix::fs::PermissionsExt;
            let p = ws.join("build.sh");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let o = call(&reg, json!({"command": cmd}));
        let v: Value = serde_json::from_str(&o.content).unwrap_or(json!({}));
        assert_eq!(v["exit_code"], 0, "{cmd}: {}", o.content);
        assert!(
            v["stdout"].as_str().unwrap_or("").contains("wrote 4 lines"),
            "{cmd}: {}",
            o.content
        );
        std::fs::remove_file(ws.join("out-4.txt")).unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
}
