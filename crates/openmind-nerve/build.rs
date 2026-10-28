// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 348 nerve-size-headroom (D-337-3): linker size flags for the
//! openmind-nerve release binary on the musl targets.
//!
//! - `-z pack-relative-relocs` (RELR): the static-pie binary carried ~40 KB
//!   of `.rela.dyn` R_*_RELATIVE entries (24 bytes each); RELR stores the
//!   same relocations as a bitmap in ~1 KB. musl's static-pie start code
//!   (rcrt1 / _dlstart) applies DT_RELR since musl 1.2.4 (the Rust musl
//!   targets ship 1.2.5). Same flag as slot 329 for hermes-gateway.
//! - `--icf=all` (identical code folding): lld merges byte-identical
//!   functions (~20 KB here, mostly monomorphised generics). Rust gives no
//!   guarantee that distinct functions have distinct addresses (rustc marks
//!   them `unnamed_addr`), so folding is allowed by the language.
//!
//! These are linker size flags only: no CPU, codegen or SIMD flag
//! (UNIVERSAL-CPU.md). Applied only when every condition holds, otherwise
//! this script does nothing and the binary links exactly as before:
//! - target linux + musl, arch x86_64 or aarch64 (both run the real smoke:
//!   x86_64 native, aarch64 under qemu user mode; armv7/android untouched),
//! - the resolved linker is lld (rust-lld / ld.lld), which understands the
//!   raw arguments. A `cc` driver would need `-Wl,` and a GNU ld >= 2.38
//!   (and GNU ld has no ICF), so it is left alone.
//!
//! `cargo:rustc-link-arg-bins` touches only this package's binaries (not its
//! tests or any other workspace binary). Opt out for a comparison build with
//! `NERVE_NO_SIZE_LINK_ARGS=1`.

use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC_LINKER");
    println!("cargo:rerun-if-env-changed=NERVE_NO_SIZE_LINK_ARGS");
    let var = |k: &str| env::var(k).unwrap_or_default();
    if var("NERVE_NO_SIZE_LINK_ARGS") == "1" {
        return;
    }
    let arch = var("CARGO_CFG_TARGET_ARCH");
    if !(arch == "x86_64" || arch == "aarch64")
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
        println!("cargo:rustc-link-arg-bins=--icf=all");
    }
}
