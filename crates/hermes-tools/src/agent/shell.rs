// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `terminal`: one shell command in the Hermes workspace (the prior
//! `terminal` tool, local backend, cwd /workspace, timeout 1800 s, approvals
//! off). Now:
//! * the deny list judges the `command` string before anything starts (the
//!   registry wrapper; S1–S9 incl. S6-secret and S9);
//! * `/bin/sh -c` runs inside `openmind_sandbox::run`: env cleared (PATH,
//!   LANG, TZ only; HOME/TMPDIR in the workspace), empty network namespace
//!   (fails closed if it can not be made), read-only root with only the
//!   workspace writable, secret/gate/Hermes-config dirs hidden under an
//!   empty tmpfs, private PID namespace and masked /proc, rlimits
//!   (CPU = timeout, 1 GiB address space, 64 MiB per file, 256 fds), wall
//!   timeout (default 60 s, max 300 s) and an output cap per stream.

use super::Shared;
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use openmind_sandbox::{SandboxPolicy, Status};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One sandboxed shell at a time per gateway.
static ONE_SHELL: Mutex<()> = Mutex::new(());

/// The sandbox policy every `terminal` run uses.
pub fn policy(cfg: &super::AgentToolsConfig, timeout: Duration) -> SandboxPolicy {
    let ws = &cfg.workspace;
    let mut p = SandboxPolicy::new(ws);
    let tmp = ws.join(".tmp");
    let _ = std::fs::create_dir_all(&tmp);
    cfg.own(&tmp);
    p.env_allowlist = vec!["LANG".into(), "LC_ALL".into(), "TZ".into()];
    p.env_set = vec![
        ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ("HOME".into(), ws.display().to_string()),
        ("TMPDIR".into(), tmp.display().to_string()),
        ("PWD".into(), ws.display().to_string()),
    ];
    p.wall_timeout = timeout;
    p.cpu_secs = timeout.as_secs().max(1);
    p.mem_bytes = 1 << 30;
    p.nofile = 256;
    p.fsize_bytes = 64 << 20;
    p.output_cap = cfg.output_cap;
    p.net = false;
    p.require_net_isolation = true;
    p.namespaces = true;
    p.ro_root = true;
    p.landlock = true;
    p.fs_write = vec![ws.clone(), PathBuf::from("/dev/null")];
    p.fs_read.push(PathBuf::from("/dev/null"));
    p.fs_read.push(PathBuf::from("/usr/local"));
    p.fs_read.extend(cfg.read_roots.iter().cloned());
    p.hide_paths.extend(cfg.hide.iter().cloned());
    p.cgroup_parent = cfg.cgroup_parent.clone();
    p.cgroup_pids_max = 128;
    p.cgroup_mem_bytes = 1 << 30;
    p.run_as = cfg.run_as;
    p
}

pub fn outcome_json(out: &openmind_sandbox::Outcome) -> Value {
    let (code, signal) = match out.status {
        Status::Exited(c) => (json!(c), Value::Null),
        Status::Signaled(s) => (Value::Null, json!(s)),
        Status::Error => (Value::Null, Value::Null),
    };
    let weak: Vec<String> = out
        .enforced
        .iter()
        .filter(|(n, e)| {
            !e.is_enforced() && matches!(n.as_str(), "net" | "ro_root" | "mountns" | "hide_home")
        })
        .map(|(n, _)| n.clone())
        .collect();
    json!({
        "exit_code": code,
        "signal": signal,
        "timed_out": out.timed_out,
        "wall_ms": out.wall.as_millis() as u64,
        "stdout": hermes_rs::redact(&String::from_utf8_lossy(&out.stdout)),
        "stderr": hermes_rs::redact(&String::from_utf8_lossy(&out.stderr)),
        "stdout_truncated": out.stdout_truncated,
        "stderr_truncated": out.stderr_truncated,
        "sandbox_layers_not_enforced": weak,
    })
}

/// Refuse to report success if the run could not be isolated.
fn isolation_ok(out: &openmind_sandbox::Outcome) -> Result<(), String> {
    if out.status == Status::Error {
        return Err(out
            .error
            .clone()
            .unwrap_or_else(|| "sandbox failed to start".into()));
    }
    Ok(())
}

pub struct Terminal(pub Arc<Shared>);

impl Tool for Terminal {
    fn name(&self) -> String {
        "terminal".into()
    }
    fn description(&self) -> String {
        let c = &self.0.cfg;
        let scripts = if hermes_rs::denylist::Policy::global().script_mode {
            " Scripts: write a .sh file in the workspace and run it with `sh file.sh` or `./file.sh`; its variables, $( ), here-documents, if/for/while and functions are allowed, every command inside is checked, and a script may not start another script."
        } else {
            ""
        };
        format!(
            "Run one shell command (sh -c) in the Hermes workspace sandbox: no network, read-only system, only the workspace writable, secrets hidden, every command checked by the Hermes deny list first. Default timeout {} s (timeout_s up to {}). Output is capped.{scripts}",
            c.shell_timeout_s, c.shell_timeout_max_s
        )
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "command": {"type": "string"},
            "timeout_s": {"type": "integer", "minimum": 1, "maximum": self.0.cfg.shell_timeout_max_s}
        }, "required": ["command"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        if let Err(e) = sh.gate(ctx) {
            return ToolOutput::err(format!("refused: {e}"));
        }
        if let Value::Object(o) = args {
            if let Some(k) = o
                .keys()
                .find(|k| !["command", "timeout_s"].contains(&k.as_str()))
            {
                return ToolOutput::err(format!("refused: unknown argument {k:?}"));
            }
        }
        let cmd = args["command"].as_str().unwrap_or("").trim();
        if cmd.is_empty() {
            return ToolOutput::err("refused: empty command");
        }
        let t = args["timeout_s"]
            .as_u64()
            .unwrap_or(sh.cfg.shell_timeout_s)
            .clamp(1, sh.cfg.shell_timeout_max_s);
        let sh_bin = ["/bin/sh", "/usr/bin/sh"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "/bin/sh".into());
        let pol = policy(&sh.cfg, Duration::from_secs(t));
        sh.log("terminal", ctx, &format!("run timeout={t}s cmd={cmd}"));
        let out = {
            let _one = ONE_SHELL.lock().unwrap_or_else(|e| e.into_inner());
            openmind_sandbox::run(&[sh_bin, "-c".into(), cmd.to_string()], &pol)
        };
        if let Err(e) = isolation_ok(&out) {
            sh.log("terminal", ctx, &format!("sandbox error: {e}"));
            return ToolOutput::err(format!("refused: the sandbox could not start: {e}"));
        }
        let j = outcome_json(&out);
        sh.log(
            "terminal",
            ctx,
            &format!(
                "done exit={} timed_out={} wall_ms={}",
                j["exit_code"], j["timed_out"], j["wall_ms"]
            ),
        );
        let failed = !matches!(out.status, Status::Exited(0)) || out.timed_out;
        ToolOutput {
            content: j.to_string(),
            is_error: failed,
        }
    }
}
