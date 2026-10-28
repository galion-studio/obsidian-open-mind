// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-console-serve`: console-dist cutover alias of `galion-console-static`.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    console_static::entry::run()
}
