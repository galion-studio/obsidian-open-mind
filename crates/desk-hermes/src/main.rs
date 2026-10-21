// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! desk-hermes: print the Hermes mindpack JSON for galion-desk.
//!
//!   HERMES_HOME      default /opt/galion/hermes
//!   HERMES_STATE_DB  default $HERMES_HOME/state.db

/// `--check`: resolve HERMES_HOME and the state DB path; opens nothing.
fn check() -> Result<String, String> {
    let mut r = String::new();
    let (home, db) = desk_hermes::paths(&|k| std::env::var(k).ok());
    let ok = galion_common::flags::path_exists(&mut r, "home", &home, true);
    let ok = galion_common::flags::path_exists(&mut r, "state_db", &db, false) && ok;
    if ok {
        Ok(r)
    } else {
        Err(format!(
            "{r}HERMES_HOME or state.db missing (mindpack would be empty)"
        ))
    }
}

fn main() {
    galion_common::flags::early(
        "desk-hermes",
        env!("CARGO_PKG_VERSION"),
        "desk-hermes: prints the Hermes mindpack JSON (reads HERMES_HOME, state.db read-only).
env: HERMES_HOME, HERMES_STATE_DB
",
        check,
    );
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "--help" || a == "-h")
    {
        println!(
            "desk-hermes: prints the Hermes mindpack JSON (reads HERMES_HOME, state.db read-only)."
        );
        return;
    }
    let (home, db) = desk_hermes::paths(&|k| std::env::var(k).ok());
    println!(
        "{}",
        desk_hermes::render(&desk_hermes::mindpack(&home, &db))
    );
}
