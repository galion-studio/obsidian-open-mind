// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Prints one field of the default config, to measure the crate's cost.
fn main() {
    let cfg = openmind_config::load_from(None, &|_| None).expect("defaults");
    println!("{}", cfg.ports.ollama);
}
