// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Synthesized auth.log lines in the shapes seen on the VPS
//! (docs/guard-rust.md section 1). Documentation-range addresses only.
#![allow(dead_code)]

use galion_guard::timefmt::days_from_civil;

pub const T0: i64 = 1_792_519_200; // 2026-10-20T18:00:00+00:00 (20:00 +02:00)

pub fn stamp(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Find the civil date by scanning from 2026-01-01 (test helper only).
    let mut y = 2026;
    let mut base = days_from_civil(y, 1, 1);
    while days < base {
        y -= 1;
        base = days_from_civil(y, 1, 1);
    }
    while days >= days_from_civil(y + 1, 1, 1) {
        y += 1;
    }
    let mut m = 1;
    while m < 12 && days >= days_from_civil(y, m + 1, 1) {
        m += 1;
    }
    let d = days - days_from_civil(y, m, 1) + 1;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000001+00:00",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

/// One brute-force connection that counts once (rule 5) and closes (rule 19).
pub fn invalid_user(t: i64, pid: u32, ip: &str, user: &str) -> Vec<String> {
    vec![
        format!(
            "{} ip50 sshd[{pid}]: Invalid user {user} from {ip} port {}",
            stamp(t),
            40000 + pid % 20000
        ),
        format!(
            "{} ip50 sshd[{pid}]: Connection closed by invalid user {user} {ip} port {} [preauth]",
            stamp(t),
            40000 + pid % 20000
        ),
    ]
}

/// Rule 14 + the OpenSSH 9 disconnect line (not counted, parity).
pub fn max_auth(t: i64, pid: u32, ip: &str) -> Vec<String> {
    vec![
        format!(
            "{} ip50 sshd[{pid}]: error: maximum authentication attempts exceeded for root from {ip} port 4{pid} ssh2 [preauth]",
            stamp(t)
        ),
        format!(
            "{} ip50 sshd[{pid}]: Disconnecting authenticating user root {ip} port 4{pid}: Too many authentication failures [preauth]",
            stamp(t)
        ),
    ]
}

pub fn accepted(t: i64, pid: u32, ip: &str) -> Vec<String> {
    vec![format!(
        "{} ip50 sshd[{pid}]: Accepted publickey for root from {ip} port 5{pid} ssh2: ED25519 SHA256:AAAAexample",
        stamp(t)
    )]
}

/// Noise that must never count (top VPS shapes).
pub fn noise(t: i64, pid: u32, ip: &str) -> Vec<String> {
    vec![
        format!("{} ip50 sshd[{pid}]: Connection closed by {ip} port 4{pid} [preauth]", stamp(t)),
        format!("{} ip50 sshd[{pid}]: Received disconnect from {ip} port 4{pid}:11: Bye Bye [preauth]", stamp(t)),
        format!("{} ip50 sshd[{pid}]: fatal: Timeout before authentication for {ip} port 4{pid}", stamp(t)),
        format!("{} ip50 sshd[{pid}]: Connection reset by authenticating user root {ip} port 4{pid} [preauth]", stamp(t)),
    ]
}
