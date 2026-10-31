// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-sm-sign: the signing account's CLI (runs as galion-sm-sign, holds
//! the NEW sandbox key). It never executes tested code or any other program:
//! it re-hashes the artifact and logs itself, verifies the stack signature
//! and signs.
//!
//!   galion-sm-sign keygen      --seed-out SEED                 (prints the public key only)
//!   galion-sm-sign sign-test   --entry E --stack-pub P --key SEED
//!   galion-sm-sign sign-dryrun --entry E --stack-pub P --key SEED
//!
//! Refuses to run as root. Exit: 0 signed pass, 1 refused or signed fail, 2 usage/I-O.

use galion_self_modify::{records, util};
use std::path::PathBuf;

fn die(code: i32, m: &str) -> ! {
    eprintln!("galion-sm-sign: {m}");
    std::process::exit(code)
}

fn req(a: &[String], k: &str) -> PathBuf {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| die(2, &format!("missing {k}")))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if util::euid() == 0 {
        die(1, "refused: run as the signing account, never as root");
    }
    let cmd = a.first().map(String::as_str).unwrap_or("");
    let rec = match cmd {
        "keygen" => match util::keygen_to(&req(&a, "--seed-out")) {
            Ok(p) => {
                println!("{p}");
                return;
            }
            Err(e) => die(2, &e),
        },
        "sign-test" | "sign-dryrun" => {
            let stack = util::load_pub(&req(&a, "--stack-pub")).unwrap_or_else(|e| die(2, &e));
            let key = util::load_seed(&req(&a, "--key")).unwrap_or_else(|e| die(2, &e));
            let entry = req(&a, "--entry");
            let now = util::now_secs();
            let r = if cmd == "sign-test" {
                records::sign_test(&entry, &stack, &key, now)
            } else {
                records::sign_dryrun(&entry, &stack, &key, now)
            };
            r.unwrap_or_else(|e| die(1, &e))
        }
        _ => die(
            2,
            "usage: keygen | sign-test | sign-dryrun (see source header)",
        ),
    };
    println!("{rec}");
    if rec["verdict"] != "pass" {
        std::process::exit(1);
    }
}
