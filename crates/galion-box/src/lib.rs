// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! galion-box: the Galion OS sandbox launcher prototype (plan L3, docs/sandbox/galion-box.md).
//! Rootless: user + mount + pid + net + uts + ipc namespaces, pivot_root into a private
//! rootfs, default-deny network, no capabilities and no_new_privs for the payload.

#[cfg(not(any(target_os = "linux", target_os = "android")))]
compile_error!("galion-box only supports Linux and Android");

pub mod escape;
pub mod inside;
pub mod launch;
pub mod policy;
pub mod probe;
pub mod sys;
