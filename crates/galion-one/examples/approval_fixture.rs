// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Local headless run only: write one fake, fully signed self-change entry
//! (test-only keys) so the Approvals screen has a card to show.
//! Usage: approval_fixture <queue dir> <keys dir> <id>

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: approval_fixture <queue dir> <keys dir> <id>");
        std::process::exit(2);
    }
    let now = std::time::SystemTime::now() // repo-check: allow (unix epoch number for signed_at in a test fixture; never printed)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    galion_owner_key::testkit::fake_entry(a[1].as_ref(), a[2].as_ref(), &a[3], now)
        .expect("write fixture");
    println!("fixture {} written", a[3]);
}
