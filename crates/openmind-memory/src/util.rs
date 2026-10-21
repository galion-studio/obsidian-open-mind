// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Time and id helpers. Stored data keeps the gateway's formats (JS
//! `toISOString()` UTC strings, epoch ms); human-facing logs use Zagreb time.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Days since 1970-01-01 -> (y, m, d). Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// JS `new Date(ms).toISOString()`: `2026-10-18T19:00:00.000Z`. // repo-check: allow (documents the JS toISOString wire format kept for 8420 parity)
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let milli = ms.rem_euclid(1000);
    let (y, mo, d) = civil(secs.div_euclid(86400));
    let sod = secs.rem_euclid(86400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

/// UTC date `YYYY-MM-DD` (the gateway shards L0 JSONL by the host's local
/// date; the VPS clock is UTC, so UTC keeps the same shard names there).
pub fn utc_date(ms: i64) -> String {
    let (y, m, d) = civil(ms.div_euclid(1000).div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Random-ish hex (non-cryptographic; ids only). Portable: std's RandomState
/// is seeded from the OS on every platform.
pub fn rand_hex(nbytes: usize) -> String {
    let mut out = String::new();
    while out.len() < nbytes * 2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        h.write_i64(now_ms());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(nbytes * 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iso() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z"); // repo-check: allow (JS toISOString wire format kept for 8420 parity)
        assert_eq!(iso_utc(1_792_349_999_411), "2026-10-18T18:59:59.411Z"); // repo-check: allow (JS toISOString wire format kept for 8420 parity)
        assert_eq!(utc_date(951_782_400_000), "2000-02-29");
        assert_eq!(rand_hex(3).len(), 6);
    }
}
