// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `gaia typed-plan SYSTEM [--intensity N] [--out F]` (HADES Option B, slot
//! 152): print the canonical `hades-plan/v1` typed plan GAIA HADES would ask
//! approval for, its SHA-256 and the gate action, so the admin can
//! `hades-gate mint --plan-file F` and the gate can `check-typed` it.
//!
//! Plans only: nothing is executed, nothing is sent anywhere. `--out` writes
//! the exact canonical bytes (no trailing newline) to a NEW file (never
//! overwritten). The prose `gaia plan` / `dispatch` path is unchanged.

use gaia_core::{Json, System};
use gaia_hades::typed::{typed_action, typed_plan_for};
use hades_gate::token::{plan_hash, to_hex};

/// Exit code when the system has no typed plan (not HADES / not expressible).
pub const EXIT_REFUSED: u8 = 4;

/// Run the subcommand. `args` starts after `typed-plan`. Returns the JSON
/// result and the process exit code (0 ok, [`EXIT_REFUSED`]).
///
/// # Errors
/// Usage errors (unknown system, bad `--intensity`, `--out` not writable).
pub fn run(args: &[&str]) -> Result<(Json, u8), String> {
    let name = match args.first() {
        Some(s) if !s.starts_with("--") => *s,
        _ => return Err("typed-plan needs SYSTEM".into()),
    };
    let system = System::parse(name).ok_or_else(|| format!("unknown system {name:?}"))?;
    let flag = |k: &str| {
        args.iter()
            .position(|a| *a == k)
            .map(|i| args.get(i + 1).copied().ok_or(format!("{k} needs a value")))
            .transpose()
    };
    let intensity = match flag("--intensity")? {
        None => 100,
        Some(v) => v
            .parse::<u8>()
            .map_err(|_| format!("--intensity {v:?}: need 0..=255"))?,
    };
    let out = flag("--out")?;
    let base = Json::obj()
        .with("system", system.name())
        .with("intensity", i64::from(intensity))
        .with("format", hades_gate::typed::MAGIC);
    let plan = match typed_plan_for(system, intensity) {
        Ok(p) => p,
        Err(e) => {
            return Ok((
                base.with("ok", false)
                    .with("code", e.code())
                    .with("reason", e.reason()),
                EXIT_REFUSED,
            ))
        }
    };
    let bytes = plan.canonical_bytes();
    if let Some(path) = out {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut f| f.write_all(&bytes))
            .map_err(|e| format!("--out {path}: {e}"))?;
    }
    let canonical = plan.canonical();
    Ok((
        base.with("ok", true)
            .with("action", typed_action(system))
            .with("plan_sha256", to_hex(&plan_hash(&bytes)))
            .with("canonical", canonical.as_str())
            .with(
                "steps",
                plan.steps()
                    .iter()
                    .map(|s| Json::from(s.canonical().as_str()))
                    .collect::<Vec<_>>(),
            )
            .with("prose", plan.render_prose())
            .with("out", out),
        0,
    ))
}
