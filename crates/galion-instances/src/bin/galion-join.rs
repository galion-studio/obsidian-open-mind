// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-join`: see [`galion_instances::agent`].

fn main() {
    let mut out = std::io::stdout();
    std::process::exit(galion_instances::agent::main_with(
        std::env::args().skip(1).collect(),
        &mut out,
    ));
}
