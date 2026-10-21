// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

// Shared by build.rs and tests/rustflags_guard.rs (via `include!`), so the
// exact check and message the build runs are also unit-tested.
//
// Project rule 1: no SIMD. curve25519-dalek 4.x (under ed25519-dalek) picks
// its AVX2/AVX-512 backend on x86_64 unless the target rustflags carry
// `--cfg curve25519_dalek_backend="serial"`. The workspace .cargo/config.toml
// sets that in `build.rustflags`, but any of RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS,
// CARGO_BUILD_RUSTFLAGS, CARGO_TARGET_<TRIPLE>_RUSTFLAGS or a
// `target.<triple>.rustflags` config entry replaces that list without a word.
// This check turns that silent drop into a build failure.

/// The only backend the project allows.
pub const REQUIRED_BACKEND: &str = "serial";

/// Env vars that replace `build.rustflags` (names only; values never printed).
pub const OVERRIDE_VARS: &[&str] = &[
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
];

/// `CARGO_TARGET_<TRIPLE>_RUSTFLAGS` for a target triple.
pub fn target_rustflags_var(target: &str) -> String {
    let t: String = target
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("CARGO_TARGET_{t}_RUSTFLAGS")
}

/// Decide from the backend cfg cargo reports for the target
/// (`CARGO_CFG_CURVE25519_DALEK_BACKEND`, `None` when unset) and the names of
/// override vars that are present. `Err` holds the full build-failure text.
pub fn check(backend: Option<&str>, target: &str, set_vars: &[String]) -> Result<(), String> {
    if backend == Some(REQUIRED_BACKEND) {
        return Ok(());
    }
    let got = match backend {
        None => "not set".to_string(),
        Some(v) => format!("\"{v}\""),
    };
    let why = if set_vars.is_empty() {
        "Something replaced the workspace `build.rustflags`: a RUSTFLAGS, \
         CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_RUSTFLAGS or \
         CARGO_TARGET_<TRIPLE>_RUSTFLAGS env var (cargo hides some of them \
         from build scripts), or a `target.<triple>.rustflags` / \
         `target.'cfg(..)'.rustflags` config entry."

            .to_string()
    } else {
        format!(
            "These env vars are set and REPLACE the workspace `build.rustflags`: {}.",
            set_vars.join(", ")
        )
    };
    Err(format!(
        "hades-gate: SIMD guard (project rule 1, no SIMD): the target rustflags \
         do not carry --cfg curve25519_dalek_backend=\"serial\" (backend cfg {got}, \
         target {target}). Without it curve25519-dalek builds its AVX2/AVX-512 \
         backend on x86_64. {why} Fix: unset them, or append \
         --cfg curve25519_dalek_backend=\"serial\" to them. See .cargo/config.toml \
         and docs/hades-rustflags-guard.md."
    ))
}
