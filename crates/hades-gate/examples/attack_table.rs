// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Print the S3.1 adversarial table (markdown) for
//! docs/real-tests/hades-adversarial.md. Every row is one attack from
//! `tests/attacks/mod.rs`; "actual" is the gate verdict with a validly
//! signed token from a throwaway key. Run:
//! `cargo run -p hades-gate --example attack_table`

#[allow(dead_code)] // GOOD is used by tests/adversarial.rs only
#[path = "../tests/attacks/mod.rs"]
mod attacks;

use ed25519_dalek::SigningKey;
use hades_gate::token::{plan_hash, Claims};
use hades_gate::{Approver, Request, Verdict};

const NOW: u64 = 1_792_000_000;

fn cell(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '|' => out.push_str("\\|"),
            '`' => out.push_str("\\`"),
            '\n' => out.push('⏎'),
            '\r' => out.push_str("\\r"),
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            c => out.push_str(&format!("U+{:04X}", u32::from(c))),
        }
    }
    out
}

fn main() {
    let dir = std::env::temp_dir().join(format!("hades-attack-table-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let admin = SigningKey::from_bytes(&[0x5a; 32]);
    let gate = Approver::new(
        admin.verifying_key(),
        dir.join("l.jsonl"),
        dir.join("D"),
        900,
    )
    .expect("approver");
    let mut bypass = 0usize;
    println!("| # | family | target | input (plan) | expected | actual |");
    println!("|---|---|---|---|---|---|");
    for (i, at) in attacks::ATTACKS.iter().enumerate() {
        let plan = at.plan.as_bytes();
        let mut nonce = [0u8; 16];
        nonce[..8].copy_from_slice(&(i as u64).to_le_bytes());
        let token = Claims {
            action: "heal-apply".into(),
            target: at.target.into(),
            plan_hash: plan_hash(plan),
            nonce,
            issued_at: NOW,
            expires_at: NOW + 300,
        }
        .sign(&admin);
        let v = gate.approve(
            Request {
                action: "heal-apply",
                target: at.target,
                plan,
                token: &token,
            },
            NOW,
        );
        let actual = match &v {
            Verdict::Approved(_) => {
                bypass += 1;
                "**WouldApply (BYPASS)**".to_owned()
            }
            Verdict::DryRun => "DryRun".to_owned(),
            Verdict::Refused(r) => format!("Refused: {}", cell(&r.reason())),
        };
        println!(
            "| {i} | {} | `{}` | `{}` | Refused | {actual} |",
            at.family.label(),
            cell(at.target),
            cell(at.plan)
        );
    }
    println!();
    println!(
        "attacks: {}, bypasses (WouldApply): {bypass}",
        attacks::ATTACKS.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
