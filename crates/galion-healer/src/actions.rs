// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! The allow-list. These are the only things the healer can change, each
//! dry-run unless `apply` is true, each logged to actions.jsonl:
//!
//! 1. `clear-stale-gateway-lock` (revive.py `dead_lock`): remove
//!    gateway.pid/.lock/.sock in the Hermes home when the gateway port is
//!    closed and the pid in gateway.pid is not alive. Regular files and
//!    symlinks only, never directories, never outside the home.
//! 2. `restart-unit`: only units named in `[[restart]]`, never protected
//!    names, at most once per `restart_min_gap_s`. Backend `systemd` runs
//!    `systemctl restart <unit>` (no shell); `supervisor` writes a request
//!    file for `galion-boot supervise`; `none` only logs.
//! 3. `json-brace-repair` (heal-apply.py): one missing-final-brace repair on
//!    a file under the fixture root, with backup and rollback.
//!
//! Not here on purpose: secret sealing, kill/pkill, package installs,
//! `git checkout`, edits outside the fixture root.

use openmind_survival::Runner;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn mode(apply: bool) -> &'static str {
    if apply {
        "apply"
    } else {
        "dry-run"
    }
}

/// One actions.jsonl row.
pub fn log_row(action: &str, target: &str, apply: bool, result: &str) -> String {
    json!({
        "ts": zagreb_time::now_rfc3339(),
        "action": action,
        "target": target,
        "mode": mode(apply),
        "result": result,
    })
    .to_string()
}

fn pid_alive(proc_root: &Path, pid: u64) -> bool {
    pid > 0 && proc_root.join(pid.to_string()).exists()
}

/// revive.py `dead_lock()`. Returns a short result text.
pub fn clear_stale_gateway_lock(
    home: &Path,
    gateway_up: bool,
    proc_root: &Path,
    apply: bool,
) -> String {
    if gateway_up {
        return "skip: gateway up".into();
    }
    let pid_path = home.join("gateway.pid");
    let Ok(text) = std::fs::read_to_string(&pid_path) else {
        return "skip: no gateway.pid".into();
    };
    let pid = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("pid").and_then(Value::as_u64))
        .or_else(|| text.trim().parse().ok())
        .unwrap_or(0);
    if pid_alive(proc_root, pid) {
        return format!("skip: pid {pid} alive");
    }
    let mut gone = Vec::new();
    for name in ["gateway.pid", "gateway.lock", "gateway.sock"] {
        let p = home.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            continue;
        }
        if apply {
            if let Err(e) = std::fs::remove_file(&p) {
                return format!("error: {name}: {e}");
            }
        }
        gone.push(name);
    }
    let verb = if apply { "removed" } else { "would remove" };
    format!("{verb} {} (pid {pid} dead)", gone.join(","))
}

/// Restart one allow-listed unit through the configured backend.
pub fn restart_unit(
    unit: &str,
    backend: &str,
    supervisor_dir: &Path,
    runner: &dyn Runner,
    apply: bool,
) -> String {
    if crate::config::is_protected(unit) {
        return "refuse: protected".into();
    }
    if !apply {
        return format!("would restart via {backend}");
    }
    match backend {
        "systemd" => match runner.run("systemctl", &["restart", unit]) {
            Ok(_) => "restarted via systemd".into(),
            Err(e) => format!("error: {e}"),
        },
        "supervisor" => {
            let dir = supervisor_dir.join("restart");
            match std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(dir.join(unit), zagreb_time::now_rfc3339()))
            {
                Ok(_) => "requested from supervisor".into(),
                Err(e) => format!("error: {e}"),
            }
        }
        _ => "skip: restart_backend none".into(),
    }
}

/// heal-apply.py PROTECTED words.
const REPAIR_PROTECTED: &[&str] = &["gateway", "whisper", "vision", "keep.sh"];

fn under(path: &Path, parent: &Path) -> bool {
    match (path.canonicalize(), parent.canonicalize()) {
        (Ok(p), Ok(q)) => p.starts_with(q),
        _ => false,
    }
}

fn missing_final_brace(text: &str) -> Option<String> {
    let s = text.trim_end();
    (s.starts_with('{') && !s.ends_with('}')).then(|| format!("{s}}}"))
}

/// heal-apply.py `main()`: returns (json result, exit code). With
/// `apply=false` nothing is written and the result says what would happen.
pub fn json_brace_repair(
    fixture: &Path,
    root: &Path,
    file: &str,
    error: &str,
    apply: bool,
) -> (Value, i32) {
    let refuse = |r: &str| (json!({"ok": false, "refuse": r}), 2);
    if !root.is_dir() || !under(root, fixture) {
        return refuse("root");
    }
    let rel = PathBuf::from(file);
    let target = if rel.is_absolute() {
        rel
    } else {
        root.join(rel)
    };
    if !target.is_file() || !under(&target, root) || !under(&target, fixture) {
        return refuse("path");
    }
    let target = match target.canonicalize() {
        Ok(t) => t,
        Err(_) => return refuse("path"),
    };
    let low = format!("{error}\n{file}\n{}", target.display()).to_lowercase();
    if REPAIR_PROTECTED.iter().any(|t| low.contains(t)) {
        return refuse("protected");
    }
    if !error.to_lowercase().contains("json") {
        return refuse("no-fix");
    }
    let raw = match std::fs::read(&target) {
        Ok(b) => b,
        Err(_) => return refuse("path"),
    };
    let repaired = std::str::from_utf8(&raw).ok().and_then(missing_final_brace);
    let new = repaired
        .map(String::into_bytes)
        .unwrap_or_else(|| raw.clone());
    let parses = serde_json::from_slice::<Value>(&new).is_ok();
    if !apply {
        return (
            json!({"ok": parses, "dry_run": true, "edit": new != raw}),
            if parses { 0 } else { 1 },
        );
    }
    let backup = target.with_file_name(format!(
        "{}.bak",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
    ));
    if std::fs::copy(&target, &backup).is_err() {
        return refuse("path");
    }
    if parses {
        if new != raw && std::fs::write(&target, &new).is_err() {
            let _ = std::fs::copy(&backup, &target);
            return (json!({"ok": false, "rolled": true}), 1);
        }
        return (json!({"ok": true, "tries": 1, "edit": new != raw}), 0);
    }
    let _ = std::fs::copy(&backup, &target);
    (json!({"ok": false, "rolled": true}), 1)
}
