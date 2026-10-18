// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Stderr logging with Europe/Zagreb timestamps. Never logs bodies or secrets.

pub fn info(msg: &str) {
    eprintln!("{} [openmind-memory] {msg}", zagreb_time::now_rfc3339());
}

pub fn warn(msg: &str) {
    eprintln!(
        "{} [openmind-memory] WARN {msg}",
        zagreb_time::now_rfc3339()
    );
}
