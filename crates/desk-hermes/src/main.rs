// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! desk-hermes: print the Hermes mindpack JSON for galion-desk.
//!
//!   HERMES_HOME      default /opt/galion/hermes
//!   HERMES_STATE_DB  default $HERMES_HOME/state.db

fn main() {
    galion_common::build_info::exit_if_version("desk-hermes", env!("CARGO_PKG_VERSION"));
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "--help" || a == "-h")
    {
        println!(
            "desk-hermes: prints the Hermes mindpack JSON (reads HERMES_HOME, state.db read-only)."
        );
        return;
    }
    // NERVE push (no-op unless NERVE_URL is set): started.
    nerve_client::event("desk-hermes", "started", "mindpack");
    let (home, db) = desk_hermes::paths(&|k| std::env::var(k).ok());
    println!(
        "{}",
        desk_hermes::render(&desk_hermes::mindpack(&home, &db))
    );
}
