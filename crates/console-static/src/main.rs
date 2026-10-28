// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-console-static`: see the crate docs and `docs/real-tests/console-rust-serve.md`.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    console_static::entry::run()
}
