// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The workspace MSRV is 1.89.0.
//!
//! Measured on 2026-11-15 (Europe/Zagreb), with `--ignore-rust-version`
//! so the pin itself did not hide the compiler errors:
//! - `cargo +1.83.0 check -p galion-common` fails with E0658
//!   `unsigned_is_multiple_of` and `slice_as_chunks`.
//! - `cargo +1.87.0 check -p galion-common` still fails with E0658
//!   `slice_as_chunks` (four errors). `is_multiple_of` is stable by then.
//! - `cargo +1.88.0 check -p galion-common` finishes, but
//!   `cargo +1.88.0 check -p hades-gate` fails with E0658 `file_lock`
//!   (`File::lock` in `crates/hades-gate/src/ledger.rs`).
//! - `cargo +1.89.0` compiles and tests the workspace.
//!
//! Locked crates.io deps declare at most rust-version 1.88.0 (opus-pure
//! 0.2.2 and zip 9.0.5). Rewriting `as_chunks` would not lower the floor:
//! `File::lock` still needs 1.89, and those dependencies already need 1.88.
//! CI installs `rust-toolchain.toml` instead of floating stable.
//!
//! The two files are embedded with `include_str!`. Miri's isolation refuses
//! `open`, and this test is part of the galion-common Miri pass.

#[test]
fn rust_version_matches_toolchain_file() {
    let cargo = include_str!("../../../Cargo.toml");
    let tool = include_str!("../../../rust-toolchain.toml");
    assert!(
        cargo.contains("rust-version = \"1.89.0\""),
        "workspace rust-version is not 1.89.0"
    );
    assert!(
        tool.contains("channel = \"1.89.0\""),
        "rust-toolchain.toml channel is not 1.89.0"
    );
}
