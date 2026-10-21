// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Fails the hades-gate build when the target rustflags lost
//! `--cfg curve25519_dalek_backend="serial"` (Gaia S5.4). std only.
//! `src/lib.rs` has a `compile_error!` backstop for non-cargo builds.

#[allow(dead_code)]
mod guard {
    include!("build_guard.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_guard.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    let tvar = guard::target_rustflags_var(&target);
    for v in guard::OVERRIDE_VARS {
        println!("cargo:rerun-if-env-changed={v}");
    }
    println!("cargo:rerun-if-env-changed={tvar}");
    // CARGO_ENCODED_RUSTFLAGS is always set by cargo for build scripts (the
    // final list, maybe empty), so it says nothing about who set the flags.
    let mut set: Vec<String> = ["RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS"]
        .iter()
        .filter(|v| std::env::var_os(v).is_some())
        .map(|v| v.to_string())
        .collect();
    if std::env::var_os(&tvar).is_some() {
        set.push(tvar);
    }
    let backend = std::env::var("CARGO_CFG_CURVE25519_DALEK_BACKEND").ok();
    if let Err(msg) = guard::check(backend.as_deref(), &target, &set) {
        // cargo shows build-script stderr on failure.
        eprintln!("error: {msg}");
        std::process::exit(1);
    }
}
