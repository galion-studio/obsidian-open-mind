// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 329 hermes-gateway-2mib: link the x86_64 musl release binary with
//! packed relative relocations (RELR, `-z pack-relative-relocs`).
//!
//! The static-pie binary carried ~50 KB of `.rela.dyn` R_X86_64_RELATIVE
//! entries (24 bytes each). RELR stores the same relocations as a bitmap in
//! ~1 KB. musl's static-pie start code (rcrt1/_dlstart) applies DT_RELR
//! since musl 1.2.4 (the Rust musl targets ship 1.2.5). This is a linker
//! size flag only: no CPU, codegen or SIMD flag (UNIVERSAL-CPU.md).
//!
//! Applied only when every condition holds, otherwise this script does
//! nothing:
//! - target x86_64 + linux + musl (the one target verified to start and pass
//!   the real tests with RELR; aarch64 is already under 2 MiB without it),
//! - the resolved linker is lld (rust-lld / ld.lld), which understands the
//!   raw `-z pack-relative-relocs` argument. A `cc` driver would need
//!   `-Wl,` and a GNU ld >= 2.38, so it is left alone.
//!
//! The same gate also discards `.eh_frame` via `link-nounwind.ld`, which
//! INSERTs a discard into lld's default script. A script that replaced the
//! default one grew this binary (docs/real-tests/hermes-gateway-2mib.md);
//! INSERT does not. Release builds use `panic = "abort"`, so those tables
//! are never walked. They were ~160 KB on x86_64 musl, which put this binary
//! over 2 MiB once off-host continuity linked in. A crash still aborts; it
//! has no unwind backtrace.
//!
//! `cargo:rustc-link-arg-bins` touches only this package's binaries; every
//! other workspace binary links exactly as before.

use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=link-nounwind.ld");
    println!("cargo:rerun-if-env-changed=RUSTC_LINKER");
    let var = |k: &str| env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_ARCH") != "x86_64"
        || var("CARGO_CFG_TARGET_OS") != "linux"
        || var("CARGO_CFG_TARGET_ENV") != "musl"
    {
        return;
    }
    let linker = var("RUSTC_LINKER");
    let name = Path::new(&linker)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.contains("lld") {
        println!("cargo:rustc-link-arg-bins=-zpack-relative-relocs");
        let script = Path::new(&var("CARGO_MANIFEST_DIR")).join("link-nounwind.ld");
        println!("cargo:rustc-link-arg-bins=-T{}", script.display());
    }
}
