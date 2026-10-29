// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-self-modify`: read or change the owner-only self-modify switch.
//!
//!   galion-self-modify status [--file F] [--owner-pub P]
//!       JSON status; exit 0, or 3 when locked but a target is world-writable.
//!   galion-self-modify lock --by NAME [--reason TEXT] [--file F]
//!       Write an unsigned locked record (locking needs no key).
//!   galion-self-modify sign --state allowed|locked --by NAME --owner-seed SEED
//!       [--reason TEXT] --out F
//!       Owner only: sign a record with the owner seed. Copy the output to
//!       the switch path as root. The seed never goes to the VPS.

use galion_self_modify as sm;
use std::path::{Path, PathBuf};

fn arg(a: &[String], k: &str) -> Option<String> {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1))
        .cloned()
}

fn die(code: i32, m: &str) -> ! {
    eprintln!("galion-self-modify: {m}");
    std::process::exit(code)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let file = PathBuf::from(arg(&a, "--file").unwrap_or_else(|| sm::PATH.into()));
    match a.first().map(String::as_str) {
        Some("status") => {
            let owner =
                PathBuf::from(arg(&a, "--owner-pub").unwrap_or_else(|| sm::OWNER_PUB.into()));
            let targets: Vec<&Path> = sm::TARGETS.iter().map(Path::new).collect();
            let v = sm::status(&file, &owner, &targets);
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            if v["lock_holds"] != true {
                std::process::exit(3);
            }
        }
        Some("lock") => {
            let by = arg(&a, "--by").unwrap_or_else(|| die(2, "missing --by"));
            let r = sm::Record::locked(&by, &arg(&a, "--reason").unwrap_or_default());
            sm::write(&file, &r).unwrap_or_else(|e| die(1, &e));
            println!("locked: {}", file.display());
        }
        Some("sign") => {
            let state = arg(&a, "--state").unwrap_or_else(|| die(2, "missing --state"));
            let by = arg(&a, "--by").unwrap_or_else(|| die(2, "missing --by"));
            let seed_path =
                arg(&a, "--owner-seed").unwrap_or_else(|| die(2, "missing --owner-seed"));
            let out = PathBuf::from(arg(&a, "--out").unwrap_or_else(|| die(2, "missing --out")));
            let text =
                std::fs::read_to_string(&seed_path).unwrap_or_else(|e| die(1, &e.to_string()));
            let sk = sm::seed(&text).unwrap_or_else(|e| die(1, &e));
            let r = sm::Record::signed(&state, &by, &arg(&a, "--reason").unwrap_or_default(), &sk)
                .unwrap_or_else(|e| die(2, &e));
            sm::write(&out, &r).unwrap_or_else(|e| die(1, &e));
            println!("signed {state} by key {}: {}", r.key_id, out.display());
        }
        _ => die(2, "usage: status | lock | sign (see the source header)"),
    }
}
