// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A valid ELF that hangs: `--version` never returns, and as a service it stays
//! alive without ever binding (process "active", health never 2xx).
//! Exits cleanly on SIGTERM because the default action of SIGTERM is to terminate.

fn main() {
    let first = std::env::args().nth(1).unwrap_or_default();
    eprintln!("sim-hang: started with {first:?}; sleeping forever, never binding");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
