// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//! Prints one field of the default config, to measure the crate's cost.
fn main() {
    let cfg = openmind_config::load_from(None, &|_| None).expect("defaults");
    println!("{}", cfg.ports.ollama);
}
