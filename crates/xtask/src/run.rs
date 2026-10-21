// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Process helpers: run a child with inherited stdio and turn a non-zero exit
// into an error. Every command is echoed first so CI logs show exactly what ran.

use std::path::{Path, PathBuf};
use std::process::Command;

pub type Res = Result<(), String>;

/// The repository root: the parent of crates/xtask (compile-time path), or the
/// current directory when the binary was moved elsewhere.
pub fn repo_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match here.parent().and_then(|p| p.parent()) {
        Some(root) if root.join("Cargo.toml").is_file() => root.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// Build a command rooted at the repository.
pub fn cmd(program: &str, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(args).current_dir(repo_root());
    c
}

/// Run with inherited stdio; Err on spawn failure or non-zero exit.
pub fn status(mut c: Command) -> Res {
    eprintln!("+ {}", describe(&c));
    let st = c
        .status()
        .map_err(|e| format!("cannot start {}: {e}", describe(&c)))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("`{}` failed ({st})", describe(&c)))
    }
}

/// Run and capture stdout (stderr inherited).
pub fn output(mut c: Command) -> Result<Vec<u8>, String> {
    let out = c
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot start {}: {e}", describe(&c)))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!("`{}` failed ({})", describe(&c), out.status))
    }
}

pub fn cargo(args: &[&str]) -> Res {
    status(cmd(&cargo_bin(), args))
}

pub fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn describe(c: &Command) -> String {
    let mut s = c.get_program().to_string_lossy().into_owned();
    for a in c.get_args() {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

/// Tracked files (git ls-files -z), relative to the repo root.
pub fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let mut c = Command::new("git");
    c.args(["ls-files", "-z"]).current_dir(root);
    let out = output(c)?;
    Ok(out
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect())
}

/// Append `line` to the file named by env var `var` (GITHUB_ENV,
/// GITHUB_STEP_SUMMARY, ...) when it is set; a no-op locally.
pub fn append_github(var: &str, text: &str) -> Res {
    use std::io::Write;
    let Ok(path) = std::env::var(var) else {
        return Ok(());
    };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{var}: {e}"))?;
    f.write_all(text.as_bytes())
        .map_err(|e| format!("{var}: {e}"))
}

/// Directory for downloaded CI tools: $RUNNER_TEMP/galion-tools on a runner,
/// target/xtask-tools locally.
pub fn tools_root() -> PathBuf {
    match std::env::var_os("RUNNER_TEMP") {
        Some(t) => PathBuf::from(t).join("galion-tools"),
        None => {
            let td = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| repo_root().join("target"));
            td.join("xtask-tools")
        }
    }
}

/// $CARGO_TARGET_DIR (absolute) or <repo>/target.
pub fn target_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        Some(p) => repo_root().join(p),
        None => repo_root().join("target"),
    }
}
