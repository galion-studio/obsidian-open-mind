// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests for the build-time SIMD guard (build_guard.rs, Gaia S5.4).
//! The real "does cargo really fail" proof is tests/real_rustflags_guard.rs.

#[allow(dead_code)]
mod guard {
    include!("../build_guard.rs");
}

const X86: &str = "x86_64-unknown-linux-musl";

#[test]
fn serial_passes() {
    assert_eq!(guard::check(Some("serial"), X86, &[]), Ok(()));
    // Override vars set but still carrying the cfg: fine.
    assert_eq!(
        guard::check(Some("serial"), X86, &["RUSTFLAGS".to_string()]),
        Ok(())
    );
}

#[test]
fn missing_cfg_fails_and_names_the_var() {
    let e = guard::check(None, X86, &["RUSTFLAGS".to_string()]).unwrap_err();
    assert!(e.contains("SIMD guard"), "{e}");
    assert!(e.contains("backend cfg not set"), "{e}");
    assert!(
        e.contains("REPLACE the workspace `build.rustflags`: RUSTFLAGS."),
        "{e}"
    );
    assert!(
        e.contains("--cfg curve25519_dalek_backend=\"serial\""),
        "{e}"
    );
    assert!(e.contains(X86), "{e}");
}

#[test]
fn missing_cfg_without_visible_env_lists_every_source() {
    let e = guard::check(None, "aarch64-unknown-linux-musl", &[]).unwrap_err();
    assert!(e.contains("target.<triple>.rustflags"), "{e}");
    assert!(e.contains("CARGO_TARGET_<TRIPLE>_RUSTFLAGS"), "{e}");
}

#[test]
fn other_backends_fail() {
    for b in ["simd", "fiat", "", "Serial", "serial "] {
        let e = guard::check(Some(b), X86, &[]).unwrap_err();
        assert!(e.contains(&format!("backend cfg \"{b}\"")), "{b}: {e}");
    }
}

#[test]
fn target_var_name() {
    assert_eq!(
        guard::target_rustflags_var("x86_64-unknown-linux-musl"),
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS"
    );
    assert_eq!(
        guard::target_rustflags_var("armv7-unknown-linux-musleabihf"),
        "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS"
    );
}

#[test]
fn message_never_echoes_flag_values() {
    // Only var names go into the message; a flag value (could carry a path
    // or anything else) is never passed in, so it cannot leak.
    let e = guard::check(
        None,
        X86,
        &[
            "RUSTFLAGS".to_string(),
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS".to_string(),
        ],
    )
    .unwrap_err();
    assert!(e.contains("RUSTFLAGS, CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS"));
}
