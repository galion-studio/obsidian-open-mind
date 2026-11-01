// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Login and signup rate limits, in memory only.
//!
//! The client is identified by `sha256(salt || ip)` truncated to 16 bytes,
//! where `salt` is random per process and replaced every 24 hours, so the
//! table cannot be turned back into IP addresses and nothing survives a
//! restart or a day. Nothing here is ever written to disk or logged.
//!
//! * Login: 10 failures in 15 minutes from one client locks that client out
//!   for 15 minutes.
//! * Login, global: more than 600 failures in one minute across everyone
//!   (a spread-out guessing run) pauses all failed-login handling for the rest
//!   of that minute; logins with a correct number still succeed.
//! * Account creation: 20 per hour per client.

use std::collections::HashMap;
use std::sync::Mutex;

pub const FAIL_LIMIT: u32 = 10;
pub const FAIL_WINDOW: i64 = 15 * 60;
pub const LOCKOUT: i64 = 15 * 60;
pub const GLOBAL_FAIL_PER_MIN: u32 = 600;
pub const CREATE_PER_HOUR: u32 = 20;
const SALT_TTL: i64 = 86_400;

type Key = [u8; 16];

#[derive(Default)]
struct Window {
    start: i64,
    count: u32,
    locked_until: i64,
}

struct Inner {
    salt: [u8; 32],
    salt_born: i64,
    fails: HashMap<Key, Window>,
    creates: HashMap<Key, Window>,
    global_min: i64,
    global_fails: u32,
}

pub struct Limiter {
    inner: Mutex<Inner>,
}

/// Why a request was refused, with seconds to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Locked(i64),
    Busy(i64),
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub fn new() -> Limiter {
        let mut salt = [0u8; 32];
        crate::rng::fill(&mut salt);
        Limiter {
            inner: Mutex::new(Inner {
                salt,
                salt_born: crate::now_unix(),
                fails: HashMap::new(),
                creates: HashMap::new(),
                global_min: 0,
                global_fails: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn key(inner: &mut Inner, client: &str, now: i64) -> Key {
        if now - inner.salt_born >= SALT_TTL {
            crate::rng::fill(&mut inner.salt);
            inner.salt_born = now;
            inner.fails.clear();
            inner.creates.clear();
        }
        let mut h = galion_common::sha256::Sha256::new();
        h.update(&inner.salt);
        h.update(client.as_bytes());
        let d = h.finalize();
        let mut k = [0u8; 16];
        k.copy_from_slice(&d[..16]);
        k
    }

    /// Before checking a login: may this client try now?
    pub fn login_allowed(&self, client: &str, now: i64) -> Result<(), Refused> {
        let mut g = self.lock();
        let k = Self::key(&mut g, client, now);
        if let Some(w) = g.fails.get(&k) {
            if w.locked_until > now {
                return Err(Refused::Locked(w.locked_until - now));
            }
        }
        if g.global_min == now / 60 && g.global_fails >= GLOBAL_FAIL_PER_MIN {
            return Err(Refused::Busy(60 - now % 60));
        }
        Ok(())
    }

    /// Record a failed login.
    pub fn login_failed(&self, client: &str, now: i64) {
        let mut g = self.lock();
        let k = Self::key(&mut g, client, now);
        let minute = now / 60;
        if g.global_min != minute {
            g.global_min = minute;
            g.global_fails = 0;
        }
        g.global_fails += 1;
        let w = g.fails.entry(k).or_default();
        if now - w.start >= FAIL_WINDOW {
            w.start = now;
            w.count = 0;
        }
        w.count += 1;
        if w.count >= FAIL_LIMIT {
            w.locked_until = now + LOCKOUT;
            w.count = 0;
            w.start = now;
        }
        if g.fails.len() > 100_000 {
            g.fails
                .retain(|_, w| w.locked_until > now || now - w.start < FAIL_WINDOW);
        }
    }

    /// A good login clears that client's failures.
    pub fn login_ok(&self, client: &str, now: i64) {
        let mut g = self.lock();
        let k = Self::key(&mut g, client, now);
        g.fails.remove(&k);
    }

    /// Count an account creation; refused over [`CREATE_PER_HOUR`].
    pub fn create(&self, client: &str, now: i64) -> Result<(), Refused> {
        let mut g = self.lock();
        let k = Self::key(&mut g, client, now);
        let w = g.creates.entry(k).or_default();
        if now - w.start >= 3600 {
            w.start = now;
            w.count = 0;
        }
        if w.count >= CREATE_PER_HOUR {
            return Err(Refused::Locked(w.start + 3600 - now));
        }
        w.count += 1;
        if g.creates.len() > 100_000 {
            g.creates.retain(|_, w| now - w.start < 3600);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_after_ten() {
        let l = Limiter::new();
        let t = l.lock().salt_born;
        for i in 0..9 {
            assert!(l.login_allowed("1.2.3.4", t + i).is_ok());
            l.login_failed("1.2.3.4", t + i);
        }
        l.login_failed("1.2.3.4", t + 9);
        assert!(matches!(
            l.login_allowed("1.2.3.4", t + 10),
            Err(Refused::Locked(_))
        ));
        assert!(l.login_allowed("5.6.7.8", t + 10).is_ok());
        assert!(l.login_allowed("1.2.3.4", t + 9 + LOCKOUT).is_ok());
    }

    #[test]
    fn global_brake() {
        let l = Limiter::new();
        let t = (l.lock().salt_born / 60) * 60 + 60;
        for i in 0..GLOBAL_FAIL_PER_MIN {
            l.login_failed(&format!("10.0.{}.{}", i / 250, i % 250), t);
        }
        assert!(matches!(
            l.login_allowed("9.9.9.9", t + 1),
            Err(Refused::Busy(_))
        ));
        assert!(l.login_allowed("9.9.9.9", t + 61).is_ok());
    }

    #[test]
    fn create_limit_and_no_raw_ip_kept() {
        let l = Limiter::new();
        let t = l.lock().salt_born;
        for _ in 0..CREATE_PER_HOUR {
            l.create("203.0.113.7", t).unwrap();
        }
        assert!(l.create("203.0.113.7", t).is_err());
        assert!(l.create("203.0.113.7", t + 3600).is_ok());
        // Keys are salted hashes; the raw address is nowhere in the table.
        let g = l.lock();
        for k in g.creates.keys() {
            assert_ne!(&k[..], &b"203.0.113.7\0\0\0\0\0"[..]);
        }
    }
}
