// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Write the deterministic test Hermes home to the directory given as the only
// argument. Used by scripts/real/desk-linkwatch.sh; never point it at a live
// Hermes home (it overwrites state.db in the target directory).

#[path = "../tests/fixture/mod.rs"]
mod fixture;

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: fixture_home <dir>");
        std::process::exit(2);
    };
    let dir = std::path::PathBuf::from(dir);
    if dir.starts_with("/opt/galion") {
        eprintln!("fixture_home: refusing to write under /opt/galion");
        std::process::exit(2);
    }
    fixture::build(&dir);
    println!("{}", dir.display());
}
