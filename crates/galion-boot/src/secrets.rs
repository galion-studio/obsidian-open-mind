// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Secret restore from the age bundles, replacing keep.sh's plaintext
//! `secrets/persist/*` copies, lock-secrets.sh and persist-boot.sh's key copy.
//!
//! Rules (each one fixes a known issue):
//! - Source is only an age-encrypted tar (`*.tar.age`) decrypted with an
//!   identity file: `age -d -i <identity> <bundle> | tar -x`. No plaintext
//!   volume copies, no openssl, no `SEALED` stubs written anywhere.
//! - A live file that exists, is non-empty and is not a `SEALED` stub is
//!   never overwritten. A stub or empty live file is replaced by the real one.
//! - A bundle member that is itself a `SEALED` stub is refused.
//! - Dry-run by default: lists the bundle (`tar -t`), writes nothing.
//! - Contents are never printed or logged; only names and actions.
//! - Writes are 0400 via temp file + rename inside the live dir; the
//!   extraction dir is 0700 inside the live dir and removed afterwards.
//! - /root/.ssh is not a target (B17): `github_ed25519` lands in the live dir.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// lock-secrets.sh `is_sealed`: any line that is exactly `SEALED`.
pub fn is_sealed_stub(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes)
        .lines()
        .any(|l| l.trim_end_matches('\r') == "SEALED")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    Missing,
    Empty,
    Stub,
    Real,
}

pub fn live_state(path: &Path) -> Live {
    match std::fs::read(path) {
        Err(_) => Live::Missing,
        Ok(b) if b.is_empty() => Live::Empty,
        Ok(b) if is_sealed_stub(&b) => Live::Stub,
        Ok(_) => Live::Real,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub action: &'static str,
}

/// Decide per name. `in_bundle(name)` says whether the bundle has it.
pub fn plan(live_dir: &Path, names: &[String], in_bundle: &dyn Fn(&str) -> bool) -> Vec<Step> {
    names
        .iter()
        .map(|n| {
            let action = match (live_state(&live_dir.join(n)), in_bundle(n)) {
                (Live::Real, _) => "keep (live file is real)",
                (_, false) => "missing from bundle",
                (Live::Stub, true) => "install (replaces SEALED stub)",
                (Live::Empty, true) => "install (live file empty)",
                (Live::Missing, true) => "install",
            };
            Step {
                name: n.clone(),
                action,
            }
        })
        .collect()
}

/// Safe member/file name: one path component, no traversal.
pub fn safe_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains('/') && !n.contains('\0')
}

pub struct Tools<'a> {
    pub age: &'a str,
    pub tar: &'a str,
}

/// `age -d -i id bundle | tar <tar_args>`; returns tar stdout.
fn age_pipe(
    t: &Tools,
    identity: &Path,
    bundle: &Path,
    tar_args: &[&str],
) -> Result<String, String> {
    let mut age = Command::new(t.age)
        .arg("-d")
        .arg("-i")
        .arg(identity)
        .arg(bundle)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", t.age))?;
    let age_out = age.stdout.take().ok_or("no age stdout")?;
    let tar = Command::new(t.tar)
        .args(tar_args)
        .stdin(Stdio::from(age_out))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{}: {e}", t.tar))?;
    let st = age.wait().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("age could not decrypt the bundle (wrong identity?)".into());
    }
    if !tar.status.success() {
        return Err(format!(
            "tar failed: {}",
            String::from_utf8_lossy(&tar.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&tar.stdout).into_owned())
}

/// Bundle member basenames (`tar -t`, no plaintext touches the disk).
pub fn list(t: &Tools, identity: &Path, bundle: &Path) -> Result<Vec<String>, String> {
    let out = age_pipe(t, identity, bundle, &["-t", "-f", "-"])?;
    Ok(out
        .lines()
        .filter(|l| !l.ends_with('/'))
        .filter_map(|l| l.rsplit('/').next())
        .map(str::to_string)
        .collect())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let meta = std::fs::symlink_metadata(&p).ok()?;
        if meta.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if meta.is_file() && e.file_name().to_string_lossy() == name {
            return Some(p);
        }
    }
    None
}

/// Install one file 0400 via temp + rename. Never over a real live file.
pub fn install(src: &Path, live: &Path) -> Result<(), String> {
    if live_state(live) == Live::Real {
        return Err("refuse: live file is real".into());
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    if bytes.is_empty() || is_sealed_stub(&bytes) {
        return Err("refuse: bundle member is empty or a SEALED stub".into());
    }
    let tmp = live.with_extension("restore~");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, live).map_err(|e| e.to_string())
}

/// Full restore. Returns the per-name steps with the final action text.
pub fn restore(
    t: &Tools,
    bundle: &Path,
    identity: &Path,
    live_dir: &Path,
    names: &[String],
    apply: bool,
) -> Result<Vec<Step>, String> {
    if let Some(bad) = names.iter().find(|n| !safe_name(n)) {
        return Err(format!("bad name {bad}"));
    }
    let members = list(t, identity, bundle)?;
    let steps = plan(live_dir, names, &|n| members.iter().any(|m| m == n));
    if !apply || !steps.iter().any(|s| s.action.starts_with("install")) {
        return Ok(steps);
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(live_dir)
        .map_err(|e| format!("{}: {e}", live_dir.display()))?;
    let work = live_dir.join(format!(".restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&work)
        .map_err(|e| e.to_string())?;
    let wd = work.to_string_lossy().into_owned();
    let res = age_pipe(t, identity, bundle, &["-x", "-f", "-", "-C", &wd]);
    let mut done = Vec::new();
    if let Err(e) = res {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    for s in steps {
        let action = if s.action.starts_with("install") {
            match find_file(&work, &s.name)
                .ok_or("not extracted".to_string())
                .and_then(|src| install(&src, &live_dir.join(&s.name)))
            {
                Ok(()) => "installed",
                Err(_) => "install failed",
            }
        } else {
            s.action
        };
        done.push(Step {
            name: s.name,
            action,
        });
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(done)
}

/// Tunnel token as an EnvironmentFile (`TUNNEL_TOKEN=...`, 0600), so it is
/// never on a command line (keep.sh put it in argv; it leaked into
/// gateway-deaths.log through `ps`). Returns the token byte length only.
pub fn tunnel_env(token_file: &Path, out: &Path, apply: bool) -> Result<String, String> {
    let raw = std::fs::read(token_file).map_err(|e| format!("{}: {e}", token_file.display()))?;
    if is_sealed_stub(&raw) {
        return Err("refuse: token file is a SEALED stub".into());
    }
    let tok = String::from_utf8(raw).map_err(|_| "token is not text".to_string())?;
    let tok = tok.trim();
    if tok.len() < 20 || tok.contains(char::is_whitespace) {
        return Err("refuse: token file does not hold one token".into());
    }
    if !apply {
        return Ok(format!(
            "would write {} ({} byte token, mode 0600)",
            out.display(),
            tok.len()
        ));
    }
    let tmp = out.with_extension("tmp~");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(format!("TUNNEL_TOKEN={tok}\n").as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    Ok(format!(
        "wrote {} ({} byte token, mode 0600)",
        out.display(),
        tok.len()
    ))
}
