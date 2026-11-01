// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The account number: 16 decimal digits in 1000 0000 0000 0000 ..=
//! 9999 9999 9999 9999 (Mullvad's format, 9e15 values, about 53 bits),
//! drawn uniformly from the OS CSPRNG. It is both the user name and the
//! password, so it is shown once, grouped in fours, and never stored.

use std::fmt;

const LO: u64 = 1_000_000_000_000_000;
const HI: u64 = 9_999_999_999_999_999;
const SPAN: u64 = HI - LO + 1; // 9e15

/// A parsed, valid account number. `Debug` never prints the digits.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountNumber(u64);

impl AccountNumber {
    /// A fresh number, uniform over the whole range (rejection sampling, so
    /// no modulo bias).
    pub fn generate() -> AccountNumber {
        Self::generate_with(crate::rng::u64)
    }

    /// Same as [`generate`](Self::generate) with a caller-supplied u64
    /// source (tests).
    pub fn generate_with(mut next: impl FnMut() -> u64) -> AccountNumber {
        // Largest multiple of SPAN that fits in u64; draws above it are
        // rejected so every value is equally likely.
        let zone = u64::MAX - (u64::MAX % SPAN);
        loop {
            let v = next();
            if v < zone {
                return AccountNumber(LO + v % SPAN);
            }
        }
    }

    /// Parse user input: digits with any spaces, dashes or dots between
    /// them. Exactly 16 digits, first digit 1-9.
    pub fn parse(input: &str) -> Option<AccountNumber> {
        let mut digits = String::with_capacity(16);
        for c in input.chars() {
            match c {
                '0'..='9' => digits.push(c),
                ' ' | '-' | '.' | '\u{a0}' | '\t' => {}
                _ => return None,
            }
            if digits.len() > 16 {
                return None;
            }
        }
        if digits.len() != 16 {
            return None;
        }
        let v: u64 = digits.parse().ok()?;
        (LO..=HI).contains(&v).then_some(AccountNumber(v))
    }

    /// The 16 digits without spaces (what gets hashed).
    pub fn digits(&self) -> String {
        format!("{:016}", self.0)
    }

    /// `1234 5678 9012 3456` for display and for writing down.
    pub fn grouped(&self) -> String {
        let d = self.digits();
        format!("{} {} {} {}", &d[0..4], &d[4..8], &d[8..12], &d[12..16])
    }
}

impl fmt::Debug for AccountNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountNumber(****)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_in_range_and_grouped() {
        for _ in 0..2000 {
            let n = AccountNumber::generate();
            let d = n.digits();
            assert_eq!(d.len(), 16);
            assert_ne!(&d[..1], "0");
            let g = n.grouped();
            assert_eq!(g.len(), 19);
            assert_eq!(AccountNumber::parse(&g), Some(n));
        }
    }

    #[test]
    fn rejection_sampling_rejects_top_zone() {
        let mut calls = 0;
        let n = AccountNumber::generate_with(|| {
            calls += 1;
            if calls == 1 {
                u64::MAX
            } else {
                5
            }
        });
        assert_eq!(calls, 2);
        assert_eq!(n.digits(), "1000000000000005");
    }

    #[test]
    fn parse_accepts_separators_rejects_junk() {
        assert!(AccountNumber::parse("1234-5678-9012-3456").is_some());
        assert!(AccountNumber::parse(" 1234 5678 9012 3456 ").is_some());
        assert!(AccountNumber::parse("0234567890123456").is_none());
        assert!(AccountNumber::parse("123456789012345").is_none());
        assert!(AccountNumber::parse("12345678901234567").is_none());
        assert!(AccountNumber::parse("1234x678901234567").is_none());
        assert!(AccountNumber::parse("").is_none());
    }

    #[test]
    fn debug_hides_digits() {
        let n = AccountNumber::parse("1234567890123456").unwrap();
        assert!(!format!("{n:?}").contains("1234"));
    }
}
