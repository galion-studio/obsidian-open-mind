// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Europe/Zagreb time.
//!
//! Every timestamp in the Galion Open Mind stack is Europe/Zagreb RFC 3339 with
//! an explicit numeric offset: `+02:00` (CEST) in summer, `+01:00` (CET) in
//! winter. Never `Z`, never `+00:00`. The VPS clock is UTC - it is never printed.
//!
//! The tz database is compiled into `chrono-tz`, so this works offline and does
//! not depend on the host having `/usr/share/zoneinfo`.

use chrono::{DateTime, SecondsFormat, Utc};
use chrono_tz::Tz;

/// The zone every timestamp in this stack is written in.
pub const ZAGREB: Tz = Tz::Europe__Zagreb;

/// The epoch, used only as a last-resort fallback for an impossible unix second.
fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).expect("the unix epoch is a valid instant")
}

/// Turn an out-of-range unix second into a real instant. Real inputs always land
/// in range; the fallback exists so the API stays total.
fn instant(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).unwrap_or_else(epoch)
}

/// Render a UTC instant as Europe/Zagreb RFC 3339, seconds precision, explicit
/// offset, never `Z`.
fn rfc3339(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&ZAGREB)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// The current instant as Europe/Zagreb RFC 3339, e.g. `2026-10-18T19:05:00+02:00`.
pub fn now_rfc3339() -> String {
    rfc3339(Utc::now())
}

/// A unix second as Europe/Zagreb RFC 3339, the same format as [`now_rfc3339`].
pub fn from_unix(secs: i64) -> String {
    rfc3339(instant(secs))
}

/// A unix second as a human label, e.g. `2026-10-18 19:05 CEST (+02:00)`.
pub fn label(secs: i64) -> String {
    instant(secs)
        .with_timezone(&ZAGREB)
        .format("%Y-%m-%d %H:%M %Z (%:z)")
        .to_string()
}

/// Why [`require_rfc3339`] refused a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampReject {
    /// The timestamp ends in `Z`.
    Zulu,
    /// Not seconds-precision RFC 3339 with `+01:00` or `+02:00`.
    NotZagreb,
}

/// Refuse a timestamp that is not Europe/Zagreb RFC 3339.
///
/// A timestamp ending in `Z` is rejected. Accepted stamps match
/// [`now_rfc3339`]: seconds precision and an explicit `+01:00` or `+02:00`.
pub fn require_rfc3339(ts: &str) -> Result<(), StampReject> {
    if ts.ends_with('Z') || ts.ends_with('z') {
        return Err(StampReject::Zulu);
    }
    let offset_ok = ts.len() == 25 && (ts.ends_with("+01:00") || ts.ends_with("+02:00"));
    if !offset_ok {
        return Err(StampReject::NotZagreb);
    }
    if DateTime::<chrono::FixedOffset>::parse_from_rfc3339(ts).is_err() {
        return Err(StampReject::NotZagreb);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unix seconds for a UTC instant written the way a reader thinks of it.
    fn secs(utc: &str) -> i64 {
        DateTime::parse_from_rfc3339(utc)
            .expect("test input is valid RFC 3339")
            .timestamp()
    }

    #[test]
    fn summer_instant_has_cest_offset() {
        assert_eq!(
            from_unix(secs("2026-10-18T16:56:00Z")),
            "2026-10-18T18:56:00+02:00"
        );
    }

    #[test]
    fn dst_switch_2026_10_25_keeps_cest_one_second_before() {
        assert_eq!(
            from_unix(secs("2026-10-25T00:59:59Z")),
            "2026-10-25T02:59:59+02:00"
        );
    }

    #[test]
    fn dst_switch_2026_10_25_became_cet_at_0100z() {
        assert_eq!(
            from_unix(secs("2026-10-25T01:00:00Z")),
            "2026-10-25T02:00:00+01:00"
        );
    }

    #[test]
    fn winter_instant_has_cet_offset() {
        assert_eq!(
            from_unix(secs("2026-12-01T12:00:00Z")),
            "2026-12-01T13:00:00+01:00"
        );
    }

    #[test]
    fn winter_label_says_cet() {
        assert_eq!(
            label(secs("2026-12-01T12:00:00Z")),
            "2026-12-01 13:00 CET (+01:00)"
        );
    }

    #[test]
    fn summer_label_says_cest() {
        assert_eq!(
            label(secs("2026-10-18T16:56:00Z")),
            "2026-10-18 18:56 CEST (+02:00)"
        );
    }

    #[test]
    fn output_never_ends_in_z_and_never_carries_utc_offset() {
        let samples = [
            secs("2026-01-01T00:00:00Z"),
            secs("2026-03-29T00:59:59Z"),
            secs("2026-10-25T01:00:00Z"),
            secs("2026-12-01T12:00:00Z"),
        ];
        for s in samples {
            let out = from_unix(s);
            assert!(!out.ends_with('Z'), "unexpected Z: {out}");
            assert!(!out.contains("+00:00"), "unexpected +00:00: {out}");
            let lab = label(s);
            assert!(!lab.contains("+00:00"), "unexpected +00:00: {lab}");
        }
    }

    #[test]
    fn now_is_zagreb_rfc3339_seconds_with_explicit_offset() {
        let out = now_rfc3339();
        assert!(out.contains('T'), "no T separator: {out}");
        assert!(!out.ends_with('Z'), "now must not end in Z: {out}");
        assert!(!out.contains("+00:00"), "now must not be UTC: {out}");
        assert!(
            out.ends_with("+02:00") || out.ends_with("+01:00"),
            "now must carry the Zagreb offset: {out}"
        );
        // It parses back as a real RFC 3339 instant.
        DateTime::parse_from_rfc3339(&out).expect("now must be valid RFC 3339");
        // Seconds precision: `...:SS+NN:NN`, 25 characters.
        assert_eq!(out.len(), 25, "seconds precision expected: {out}");
    }

    #[test]
    fn timestamp_ending_in_z_is_rejected_and_now_uses_offset() {
        assert_eq!(
            require_rfc3339("2026-10-18T16:56:00Z"),
            Err(StampReject::Zulu)
        );
        let now = now_rfc3339();
        assert!(
            now.ends_with("+01:00") || now.ends_with("+02:00"),
            "now_rfc3339 must use +01:00 or +02:00: {now}"
        );
        assert_eq!(require_rfc3339(&now), Ok(()));
    }
}
