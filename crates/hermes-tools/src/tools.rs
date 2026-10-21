// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The four coding tools: `fs_read`, `fs_write`, `git`, `cargo`.
//!
//! Every call goes: spend cap check -> per-session call cap -> argument
//! allowlist -> path/branch checks -> audit line -> sandboxed run -> audit
//! line. Anything refused is logged with the reason and returned to the model
//! as a tool error; nothing outside the allowlist ever starts a process.

use crate::audit::{cut, Audit};
use crate::config::CodingConfig;
use crate::exec::{self, system_bin, Kind};
use crate::guard::{self, Access};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const GIT_OPS: &[&str] = &["status", "diff", "commit"];
pub const CARGO_CMDS: &[&str] = &["build", "check", "test", "clippy"];

/// State shared by the four tools.
pub struct Shared {
    pub cfg: CodingConfig,
    pub audit: Audit,
    pub spend: Option<Arc<SpendTracker>>,
    calls: Mutex<HashMap<String, u64>>,
}

impl Shared {
    pub fn new(cfg: CodingConfig, spend: Option<Arc<SpendTracker>>) -> Self {
        let audit = Audit::new(cfg.audit_path.clone());
        Shared {
            cfg,
            audit,
            spend,
            calls: Mutex::new(HashMap::new()),
        }
    }

    /// Spend and call caps, then an audit line saying the call is allowed
    /// to proceed to validation. `Err` = refused.
    fn gate(&self, tool: &str, ctx: &ToolCtx) -> Result<(), String> {
        if let Some(s) = &self.spend {
            s.check(ctx.spend_session).map_err(|b| b.message())?;
        }
        let mut c = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let n = c.entry(ctx.spend_session.to_string()).or_insert(0);
        if *n >= self.cfg.max_calls_per_session {
            return Err(format!(
                "coding tool call cap reached for this session ({} calls)",
                self.cfg.max_calls_per_session
            ));
        }
        *n += 1;
        let _ = tool;
        Ok(())
    }

    fn refuse(&self, tool: &str, ctx: &ToolCtx, args: &Value, why: &str) -> ToolOutput {
        self.audit.record(json!({
            "tool": tool, "session": ctx.session_id, "decision": "refused",
            "reason": why, "args": log_args(args),
        }));
        ToolOutput::err(format!("refused: {why}"))
    }

    fn allowed(&self, tool: &str, ctx: &ToolCtx, args: &Value, cmd: &[String]) -> bool {
        self.audit.record(json!({
            "tool": tool, "session": ctx.session_id, "decision": "allowed",
            "args": log_args(args), "argv": cmd,
        }))
    }

    fn done(&self, tool: &str, ctx: &ToolCtx, result: &Value) {
        let mut r = result.clone();
        for k in ["content", "stdout", "stderr", "diff", "status", "stat"] {
            if let Some(s) = r.get(k).and_then(Value::as_str) {
                r[k] = json!(format!("<{} bytes>", s.len()));
            }
        }
        self.audit.record(json!({
            "tool": tool, "session": ctx.session_id, "decision": "ran", "result": r,
        }));
    }

    /// Run one sandboxed step (audited). `Err` = refused or failed to start.
    fn step(
        &self,
        tool: &str,
        ctx: &ToolCtx,
        args: &Value,
        kind: Kind,
        cmd: Vec<String>,
    ) -> Result<openmind_sandbox::Outcome, String> {
        if !self.allowed(tool, ctx, args, &cmd) {
            return Err(format!(
                "audit log {} is not writable; refusing to run unaudited",
                self.audit.path().display()
            ));
        }
        let out = exec::run(&self.cfg, kind, &cmd)?;
        self.done(tool, ctx, &exec::summary(&out));
        Ok(out)
    }

    /// Write `data` to a fresh 0600 file in the hermes-owned stage dir.
    fn stage(&self, data: &[u8]) -> Result<PathBuf, String> {
        let p = self
            .cfg
            .state_dir
            .join("stage")
            .join(hermes_rs::new_id("stage_"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)
            .map_err(|e| format!("stage {}: {e}", p.display()))?;
        f.write_all(data).map_err(|e| format!("stage write: {e}"))?;
        Ok(p)
    }
}

fn log_args(args: &Value) -> Value {
    let mut a = args.clone();
    if let Some(c) = a.get("content").and_then(Value::as_str) {
        a["content"] = json!(format!("<{} bytes>", c.len()));
    }
    if let Some(m) = a.get("message").and_then(Value::as_str) {
        a["message"] = json!(cut(m, 200));
    }
    json!(cut(&a.to_string(), 600))
}

fn bin(name: &str) -> Result<String, String> {
    system_bin(name)
        .map(|p| p.display().to_string())
        .ok_or_else(|| format!("{name} not found in /usr/bin or /bin"))
}

fn rel_arg(rel: &std::path::Path) -> String {
    format!("./{}", rel.display())
}

// ---------------------------------------------------------------- fs_read

pub struct FsRead(pub Arc<Shared>);

impl Tool for FsRead {
    fn name(&self) -> String {
        "fs_read".into()
    }
    fn description(&self) -> String {
        "Read a file inside the allowlisted worktree (path relative to the worktree root). Optional byte offset and max_bytes.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string"},
            "offset": {"type": "integer", "minimum": 0},
            "max_bytes": {"type": "integer", "minimum": 1}
        }, "required": ["path"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        let t = "fs_read";
        if let Err(e) = sh.gate(t, ctx) {
            return sh.refuse(t, ctx, args, &e);
        }
        if let Err(e) = guard::only_keys(args, &["path", "offset", "max_bytes"]) {
            return sh.refuse(t, ctx, args, &e);
        }
        let path = args["path"].as_str().unwrap_or("");
        let rel = match guard::resolve(&sh.cfg.worktree, path, Access::Read) {
            Ok(r) => r,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        let offset = args["offset"].as_u64().unwrap_or(0);
        let max = args["max_bytes"]
            .as_u64()
            .unwrap_or(sh.cfg.max_read_bytes)
            .min(sh.cfg.max_read_bytes);
        let dd = match bin("dd") {
            Ok(b) => b,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        // dd: nofollow on the final component, byte offsets, bounded count.
        let cmd = vec![
            dd,
            format!("if={}", rel_arg(&rel)),
            "iflag=skip_bytes,count_bytes,nofollow".into(),
            "bs=65536".into(),
            format!("skip={offset}"),
            format!("count={max}"),
            "status=none".into(),
        ];
        match sh.step(t, ctx, args, Kind::Fs, cmd) {
            Err(e) => sh.refuse(t, ctx, args, &e),
            Ok(out) if !exec::ok(&out) => {
                ToolOutput::err(format!("read failed: {}", exec::failure(&out, 2000)))
            }
            Ok(out) => {
                let size = std::fs::metadata(sh.cfg.worktree.join(&rel))
                    .map(|m| m.len())
                    .unwrap_or(0);
                ToolOutput::ok(
                    json!({
                        "path": rel.display().to_string(),
                        "offset": offset,
                        "bytes": out.stdout.len(),
                        "file_size": size,
                        "truncated": offset + (out.stdout.len() as u64) < size,
                        "content": out.stdout_str(),
                    })
                    .to_string(),
                )
            }
        }
    }
}

// --------------------------------------------------------------- fs_write

pub struct FsWrite(pub Arc<Shared>);

impl Tool for FsWrite {
    fn name(&self) -> String {
        "fs_write".into()
    }
    fn description(&self) -> String {
        "Create or replace a file inside the allowlisted worktree with the given full content. Parent directories are created. .git, .cargo and CI config are off-limits.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string"},
            "content": {"type": "string"}
        }, "required": ["path", "content"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        let t = "fs_write";
        if let Err(e) = sh.gate(t, ctx) {
            return sh.refuse(t, ctx, args, &e);
        }
        if let Err(e) = guard::only_keys(args, &["path", "content"]) {
            return sh.refuse(t, ctx, args, &e);
        }
        let Some(content) = args["content"].as_str() else {
            return sh.refuse(t, ctx, args, "content must be a string");
        };
        if content.len() as u64 > sh.cfg.max_write_bytes {
            return sh.refuse(
                t,
                ctx,
                args,
                &format!(
                    "content is {} bytes, over the {} byte cap",
                    content.len(),
                    sh.cfg.max_write_bytes
                ),
            );
        }
        let path = args["path"].as_str().unwrap_or("");
        let rel = match guard::resolve(&sh.cfg.worktree, path, Access::Write) {
            Ok(r) => r,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        let install = match bin("install") {
            Ok(b) => b,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        let staged = match sh.stage(content.as_bytes()) {
            Ok(p) => p,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        // The copy into the worktree happens inside the sandbox, so Landlock
        // and the read-only root still stop a write that escaped the guard.
        let cmd = vec![
            install,
            "-D".into(),
            "-m".into(),
            "0644".into(),
            "-T".into(),
            "--".into(),
            staged.display().to_string(),
            rel_arg(&rel),
        ];
        let r = sh.step(t, ctx, args, Kind::Fs, cmd);
        let _ = std::fs::remove_file(&staged);
        match r {
            Err(e) => sh.refuse(t, ctx, args, &e),
            Ok(out) if !exec::ok(&out) => {
                ToolOutput::err(format!("write failed: {}", exec::failure(&out, 2000)))
            }
            Ok(_) => ToolOutput::ok(
                json!({"path": rel.display().to_string(), "bytes": content.len(), "written": true})
                    .to_string(),
            ),
        }
    }
}

// -------------------------------------------------------------------- git

pub struct Git(pub Arc<Shared>);

impl Git {
    fn base(&self) -> Result<Vec<String>, String> {
        let c = &self.0.cfg;
        let mut v = vec![bin("git")?, "--no-pager".to_string()];
        for kv in [
            "core.hooksPath=/dev/null".to_string(),
            "core.fsmonitor=false".into(),
            "core.pager=cat".into(),
            "credential.helper=".into(),
            "protocol.allow=never".into(),
            "commit.gpgsign=false".into(),
            "color.ui=false".into(),
            // The worktree cap runs every git call on a fresh tmpfs copy (new
            // inodes and ctimes): compare only mtime/size/mode so the index is
            // not rewritten on every call, and never auto-gc (deleted loose
            // objects are not propagated back; a gc only grows the host .git).
            "core.checkStat=minimal".into(),
            "core.trustctime=false".into(),
            "gc.auto=0".into(),
            format!("user.name={}", c.author_name),
            format!("user.email={}", c.author_email),
            format!("safe.directory={}", c.worktree.display()),
        ] {
            v.push("-c".into());
            v.push(kv);
        }
        Ok(v)
    }

    fn cmd(&self, extra: &[&str]) -> Result<Vec<String>, String> {
        let mut v = self.base()?;
        v.extend(extra.iter().map(|s| s.to_string()));
        Ok(v)
    }
}

impl Tool for Git {
    fn name(&self) -> String {
        "git".into()
    }
    fn description(&self) -> String {
        "git in the allowlisted worktree, on its one allowlisted branch. op = status | diff | commit. commit stages the given paths (or all changes) and commits with the message. Push, fetch, checkout, reset, config and every other git command are refused.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "op": {"type": "string", "enum": GIT_OPS},
            "paths": {"type": "array", "items": {"type": "string"}},
            "staged": {"type": "boolean"},
            "message": {"type": "string"}
        }, "required": ["op"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        let t = "git";
        if let Err(e) = sh.gate(t, ctx) {
            return sh.refuse(t, ctx, args, &e);
        }
        if let Err(e) = guard::only_keys(args, &["op", "paths", "staged", "message"]) {
            return sh.refuse(t, ctx, args, &e);
        }
        let op = args["op"].as_str().unwrap_or("");
        if !GIT_OPS.contains(&op) {
            let why = if op.contains("push") {
                format!("git {op:?} is never allowed: hermes-rs does not push (the owner pushes)")
            } else {
                format!(
                    "git op {op:?} is not allowed (allowed: {})",
                    GIT_OPS.join(", ")
                )
            };
            return sh.refuse(t, ctx, args, &why);
        }
        let mut paths = vec![];
        if let Some(a) = args.get("paths") {
            let Some(list) = a.as_array() else {
                return sh.refuse(t, ctx, args, "paths must be an array of strings");
            };
            if list.len() > 100 {
                return sh.refuse(t, ctx, args, "more than 100 paths");
            }
            for p in list {
                let Some(s) = p.as_str() else {
                    return sh.refuse(t, ctx, args, "paths must be strings");
                };
                // Write access rules: a deleted file may no longer exist.
                match guard::resolve(&sh.cfg.worktree, s, Access::Write) {
                    Ok(r) => paths.push(rel_arg(&r)),
                    Err(e) => return sh.refuse(t, ctx, args, &e),
                }
            }
        }
        // Branch pin, checked inside the sandbox on every call.
        let head = match self
            .cmd(&["symbolic-ref", "--short", "-q", "HEAD"])
            .and_then(|c| sh.step(t, ctx, args, Kind::Git, c))
        {
            Ok(o) => o.stdout_str().trim().to_string(),
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        if head != sh.cfg.branch {
            return sh.refuse(
                t,
                ctx,
                args,
                &format!(
                    "worktree is on {head:?}, not the allowlisted branch {:?}",
                    sh.cfg.branch
                ),
            );
        }
        let run = |extra: Vec<String>| -> Result<openmind_sandbox::Outcome, String> {
            let mut c = self.base()?;
            c.extend(extra);
            sh.step(t, ctx, args, Kind::Git, c)
        };
        let sv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match op {
            "status" => match run(sv(&[
                "status",
                "--porcelain=v1",
                "--branch",
                "--untracked-files=all",
            ])) {
                Err(e) => sh.refuse(t, ctx, args, &e),
                Ok(o) if !exec::ok(&o) => ToolOutput::err(exec::failure(&o, 2000)),
                Ok(o) => {
                    ToolOutput::ok(json!({"branch": head, "status": o.stdout_str()}).to_string())
                }
            },
            "diff" => {
                let mut c = sv(&[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--no-color",
                    "--stat",
                    "--patch",
                ]);
                if args["staged"].as_bool() == Some(true) {
                    c.push("--cached".into());
                }
                c.push("--".into());
                c.extend(paths);
                match run(c) {
                    Err(e) => sh.refuse(t, ctx, args, &e),
                    Ok(o) if !exec::ok(&o) => ToolOutput::err(exec::failure(&o, 2000)),
                    Ok(o) => ToolOutput::ok(
                        json!({"branch": head, "diff": exec::tail(&o.stdout, 60_000), "truncated": o.stdout_truncated})
                            .to_string(),
                    ),
                }
            }
            _ => {
                let msg = args["message"].as_str().unwrap_or("");
                if let Err(e) = guard::commit_message_ok(msg) {
                    return sh.refuse(t, ctx, args, &e);
                }
                let mut add = sv(&["add", "-A", "--"]);
                if paths.is_empty() {
                    add.push(".".into());
                } else {
                    add.extend(paths);
                }
                match run(add) {
                    Err(e) => return sh.refuse(t, ctx, args, &e),
                    Ok(o) if !exec::ok(&o) => {
                        return ToolOutput::err(format!("git add: {}", exec::failure(&o, 2000)))
                    }
                    Ok(_) => {}
                }
                let mf = match sh.stage(msg.as_bytes()) {
                    Ok(p) => p,
                    Err(e) => return sh.refuse(t, ctx, args, &e),
                };
                let r = run(vec![
                    "commit".into(),
                    "--no-verify".into(),
                    "--no-gpg-sign".into(),
                    "--cleanup=strip".into(),
                    "-F".into(),
                    mf.display().to_string(),
                ]);
                let _ = std::fs::remove_file(&mf);
                match r {
                    Err(e) => return sh.refuse(t, ctx, args, &e),
                    Ok(o) if !exec::ok(&o) => {
                        return ToolOutput::err(format!(
                            "git commit: {}{}",
                            exec::tail(&o.stdout, 1000),
                            exec::failure(&o, 1000)
                        ))
                    }
                    Ok(_) => {}
                }
                match run(sv(&[
                    "show",
                    "--no-color",
                    "--stat",
                    "--format=%H%n%s",
                    "HEAD",
                ])) {
                    Err(e) => sh.refuse(t, ctx, args, &e),
                    Ok(o) => {
                        let s = o.stdout_str();
                        let mut lines = s.lines();
                        let sha = lines.next().unwrap_or("").to_string();
                        ToolOutput::ok(
                            json!({"branch": head, "commit": sha, "pushed": false, "stat": lines.collect::<Vec<_>>().join("\n")})
                                .to_string(),
                        )
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ cargo

pub struct Cargo(pub Arc<Shared>);

impl Cargo {
    /// The exact argv for an allowlisted request (pure; tested).
    pub fn argv(cfg: &CodingConfig, args: &Value) -> Result<Vec<String>, String> {
        guard::only_keys(
            args,
            &["command", "package", "release", "test_filter", "nocapture"],
        )?;
        let sub = args["command"].as_str().unwrap_or("");
        if !CARGO_CMDS.contains(&sub) {
            return Err(format!(
                "cargo {sub:?} is not allowed (allowed: {})",
                CARGO_CMDS.join(", ")
            ));
        }
        let tc = cfg
            .toolchain
            .as_ref()
            .ok_or("the cargo tool is not configured (HERMES_RS_CODING_TOOLCHAIN is unset)")?;
        let mut v = vec![
            bin("nice")?,
            "-n".into(),
            cfg.nice.to_string(),
            tc.join("bin/cargo").display().to_string(),
            sub.into(),
            "--offline".into(),
            "--color".into(),
            "never".into(),
            "-j".into(),
            cfg.cargo_jobs.to_string(),
        ];
        if let Some(p) = args.get("package") {
            let p = p.as_str().unwrap_or("");
            if !guard::ident_ok(p) {
                return Err(format!("bad package name {p:?}"));
            }
            v.push("-p".into());
            v.push(p.into());
        }
        match args.get("release") {
            None => {}
            Some(Value::Bool(true)) => v.push("--release".into()),
            Some(Value::Bool(false)) => {}
            Some(_) => return Err("release must be a boolean".into()),
        }
        let filter = match args.get("test_filter") {
            None => None,
            Some(f) => {
                let f = f.as_str().unwrap_or("-");
                if sub != "test" || !guard::test_filter_ok(f) {
                    return Err(format!("bad test_filter {f:?}"));
                }
                Some(f.to_string())
            }
        };
        match sub {
            "clippy" => v.extend(["--all-targets", "--", "-D", "warnings"].map(String::from)),
            "test" => {
                if let Some(f) = filter {
                    v.push(f);
                }
                v.extend([
                    "--".into(),
                    "--test-threads".into(),
                    cfg.cargo_jobs.to_string(),
                ]);
                if args.get("nocapture") == Some(&Value::Bool(true)) {
                    v.push("--nocapture".into());
                }
            }
            _ => {}
        }
        if args.get("nocapture").is_some() && (sub != "test" || !args["nocapture"].is_boolean()) {
            return Err("nocapture is a boolean for cargo test only".into());
        }
        Ok(v)
    }
}

impl Tool for Cargo {
    fn name(&self) -> String {
        "cargo".into()
    }
    fn description(&self) -> String {
        "cargo build | check | test | clippy in the allowlisted worktree, offline, niced and with a job limit, inside the sandbox (no network). Optional package, release, test_filter, nocapture (test only).".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "command": {"type": "string", "enum": CARGO_CMDS},
            "package": {"type": "string"},
            "release": {"type": "boolean"},
            "test_filter": {"type": "string"},
            "nocapture": {"type": "boolean"}
        }, "required": ["command"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.0;
        let t = "cargo";
        if let Err(e) = sh.gate(t, ctx) {
            return sh.refuse(t, ctx, args, &e);
        }
        let cmd = match Cargo::argv(&sh.cfg, args) {
            Ok(c) => c,
            Err(e) => return sh.refuse(t, ctx, args, &e),
        };
        match sh.step(t, ctx, args, Kind::Cargo, cmd) {
            Err(e) => sh.refuse(t, ctx, args, &e),
            Ok(out) => {
                let mut v = exec::summary(&out);
                v["ok"] = json!(exec::ok(&out));
                v["stdout"] = json!(exec::tail(&out.stdout, 12_000));
                v["stderr"] = json!(exec::tail(&out.stderr, 12_000));
                if let Some(p) = exec::copy_back_problem(&out) {
                    v["copy_back"] = json!(p);
                }
                ToolOutput {
                    content: v.to_string(),
                    is_error: !exec::ok(&out),
                }
            }
        }
    }
}
