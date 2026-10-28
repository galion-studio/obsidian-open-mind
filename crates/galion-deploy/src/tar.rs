// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal POSIX ustar writer and reader (regular files, directories,
//! symlinks). The implementation lives in the shared std-only `galion-tar`
//! crate (moved there unchanged from this file); this re-export keeps the
//! `galion_deploy::tar` paths. Output is plain `tar -xpf` compatible, with
//! relative member names, so a single path can be restored with
//! `tar -xpf backup.tar -C / opt/galion/rust/bin/galion-nerve`.

pub use galion_tar::ustar::*;
