// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only thing stored about an account number: a keyed, deterministic
//! argon2id hash ("lookup hash").
//!
//! Why deterministic (one fixed salt per deployment) and not a per-row salt
//! plus a short plain-text lookup prefix: login is the number alone, so the
//! server must find the row from the number. With per-row salts it would need
//! a non-secret prefix to narrow the search, and every prefix digit stored in
//! clear is a digit an attacker with the database no longer has to guess.
//! A fixed salt lets the hash itself be the unique index: one argon2id per
//! login, nothing about the number in clear. The cost is that one guess can
//! be tested against all rows at once, which the 9e15 search space and the
//! secret pepper (argon2 `secret` input, kept outside the database) cover.
//! DESIGN.md section 5 has the numbers.

use argon2::{Algorithm, Argon2, Params, Version};

use crate::number::AccountNumber;

/// Domain-separation salt (argon2 needs >= 8 bytes). Not secret.
const SALT: &[u8] = b"galion-accounts/lookup/v1";

/// argon2id cost. Production default: OWASP's 19 MiB, t=2, p=1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Cost {
    pub const PRODUCTION: Cost = Cost {
        m_kib: 19 * 1024,
        t: 2,
        p: 1,
    };
    /// Tiny cost for unit tests only.
    pub const TEST: Cost = Cost {
        m_kib: 64,
        t: 1,
        p: 1,
    };
}

/// Hashes account numbers with the deployment's secret pepper.
pub struct Hasher {
    pepper: Vec<u8>,
    cost: Cost,
}

impl std::fmt::Debug for Hasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hasher")
            .field("pepper", &"<secret>")
            .field("cost", &self.cost)
            .finish()
    }
}

impl Hasher {
    /// `pepper` must be at least 32 bytes from a CSPRNG.
    pub fn new(pepper: Vec<u8>, cost: Cost) -> Result<Hasher, String> {
        if pepper.len() < 32 {
            return Err("pepper must be at least 32 bytes".into());
        }
        Params::new(cost.m_kib, cost.t, cost.p, Some(32)).map_err(|e| e.to_string())?;
        Ok(Hasher { pepper, cost })
    }

    /// The 32-byte lookup hash of `n`.
    pub fn lookup(&self, n: &AccountNumber) -> [u8; 32] {
        let params = Params::new(self.cost.m_kib, self.cost.t, self.cost.p, Some(32))
            .expect("checked in new");
        let a = Argon2::new_with_secret(&self.pepper, Algorithm::Argon2id, Version::V0x13, params)
            .expect("pepper length is valid");
        let mut out = [0u8; 32];
        a.hash_password_into(n.digits().as_bytes(), SALT, &mut out)
            .expect("argon2 parameters checked");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_keyed() {
        let n = AccountNumber::parse("1234 5678 9012 3456").unwrap();
        let h1 = Hasher::new(vec![1; 32], Cost::TEST).unwrap();
        let h2 = Hasher::new(vec![2; 32], Cost::TEST).unwrap();
        assert_eq!(h1.lookup(&n), h1.lookup(&n));
        assert_ne!(h1.lookup(&n), h2.lookup(&n));
        let m = AccountNumber::parse("1234 5678 9012 3457").unwrap();
        assert_ne!(h1.lookup(&n), h1.lookup(&m));
    }

    #[test]
    fn short_pepper_refused() {
        assert!(Hasher::new(vec![0; 16], Cost::TEST).is_err());
    }
}
