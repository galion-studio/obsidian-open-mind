// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Prints the S5.2 corpus table (`docs/security/hades-safety-corpus.md`):
//! one row per request through `adapter::check_gaia` without a token, with
//! the refusal code and reason. Throwaway key and temp ledger; nothing is run.
//!
//! `cargo run -p hades-gate --example safety_corpus_enum`

#[path = "../tests/support/safety_corpus.rs"]
mod corpus;

use hades_gate::adapter::check_gaia;
use hades_gate::{Approver, Verdict};

fn main() {
    let dir = std::env::temp_dir().join(format!("hades-corpus-enum-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let key = ed25519_dalek::SigningKey::from_bytes(&[0x5a; 32]);
    let gate = Approver::new(
        key.verifying_key(),
        dir.join("ledger.jsonl"),
        dir.join("DISABLED"),
        900,
    )
    .expect("approver");
    println!("| # | Category | Case | System | Target | Plan steps | Result |");
    println!("|---|----------|------|--------|--------|------------|--------|");
    let mut per: Vec<(&str, usize, usize)> = Vec::new();
    for (i, c) in corpus::ATTACKS.iter().enumerate() {
        let steps: Vec<String> = c.steps.iter().map(|s| (*s).to_owned()).collect();
        let v = check_gaia(&gate, c.system, c.target, &steps, "", 1_792_000_000);
        let cell = match &v {
            Verdict::Refused(r) => format!("refused `{}`: {}", r.code(), r.reason()),
            Verdict::DryRun => "**passes safety (dry-run)**".to_owned(),
            Verdict::Approved(_) => "**approved**".to_owned(),
        };
        let esc = |s: String| {
            s.chars()
                .map(|ch| {
                    if ch.is_ascii() {
                        ch.to_string()
                    } else {
                        ch.escape_unicode().to_string()
                    }
                })
                .collect::<String>()
                .replace('|', "\\|")
        };
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            i + 1,
            c.cat,
            esc(c.desc.to_owned()),
            esc(format!("`{}`", c.system.escape_default())),
            esc(format!("`{}`", c.target.escape_default())),
            esc(c
                .steps
                .iter()
                .map(|s| format!("`{}`", s.escape_default()))
                .collect::<Vec<_>>()
                .join(" / ")),
            esc(cell)
        );
        match per.iter_mut().find(|(k, _, _)| *k == c.cat) {
            Some(e) => {
                e.1 += 1;
                e.2 += usize::from(matches!(v, Verdict::Refused(_)));
            }
            None => per.push((c.cat, 1, usize::from(matches!(v, Verdict::Refused(_))))),
        }
    }
    println!();
    for (cat, n, refused) in per {
        println!("- {cat}: {refused}/{n} refused");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
