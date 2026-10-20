// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Prints the S3.1 red-team table (`docs/security/hades-redteam.md`):
//! one row per attempt, `refused` or `WouldApply`. `WouldApply` means the plan
//! passed the admin-safety rule, so a valid signed token for an allow-listed
//! action would make the gate return `Decision::WouldApply`.
//!
//! `cargo run -p hades-gate --example redteam_enum`

#[path = "../tests/support/redteam_corpus.rs"]
mod corpus;

use hades_gate::safety::review;

fn main() {
    println!("| # | Family | Attempt | Payload | Result |");
    println!("|---|--------|---------|---------|--------|");
    let mut fams: Vec<(&str, usize, usize)> = Vec::new();
    for (i, c) in corpus::ATTACKS.iter().enumerate() {
        let res = review(c.target.unwrap_or("galion-tuner"), c.payload.as_bytes());
        let shown = match c.target {
            Some(t) => format!("target={t:?} plan={:?}", c.payload),
            None => format!("{:?}", c.payload),
        };
        let shown = shown.replace('|', "\\|");
        let cell = match res {
            Ok(()) => "**WouldApply**".to_owned(),
            Err(h) => format!(
                "refused: {}",
                h.reason().trim_start_matches("admin-safety: ")
            ),
        };
        println!(
            "| {} | {} | {} | `` {} `` | {} |",
            i + 1,
            c.family,
            c.desc.replace('|', "\\|"),
            shown,
            cell
        );
        let pos = match fams.iter().position(|f| f.0 == c.family) {
            Some(p) => p,
            None => {
                fams.push((c.family, 0, 0));
                fams.len() - 1
            }
        };
        fams[pos].1 += 1;
        fams[pos].2 += usize::from(res.is_ok());
    }
    let mut total = 0;
    for (f, n, b) in &fams {
        total += b;
        eprintln!("{f}: {b} WouldApply / {n} attempts");
    }
    eprintln!("TOTAL WouldApply: {total} / {}", corpus::ATTACKS.len());
    let mut bad = 0;
    for (target, plan) in corpus::LEGIT {
        if let Err(h) = review(target, plan.as_bytes()) {
            bad += 1;
            eprintln!("LEGIT REFUSED: {plan:?}: {}", h.reason());
        }
    }
    eprintln!("legit refused: {bad} / {}", corpus::LEGIT.len());
}
