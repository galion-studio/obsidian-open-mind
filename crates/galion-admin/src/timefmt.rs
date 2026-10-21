// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Local wall-clock timestamps with an explicit offset (`2026-10-21T08:30:00+02:00`), never `Z`.
//! The offset comes from the C library's zone rules (TZ / /etc/localtime on Linux, the system
//! zone on Android), so the same code works on the phone and on Linux.

/// Days since 1970-01-01 -> (year, month, day). Proleptic Gregorian (H. Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format a Unix time as local time at `offset_secs` east of UTC, RFC 3339 with explicit offset.
pub fn format_rfc3339(epoch: i64, offset_secs: i64) -> String {
    let local = epoch + offset_secs;
    let (y, mo, d) = civil_from_days(local.div_euclid(86_400));
    let s = local.rem_euclid(86_400);
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let off = offset_secs.abs();
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        off / 3600,
        (off / 60) % 60
    )
}

/// UTC offset in seconds for `epoch` according to the C library's local zone.
///
/// `libc::time_t` is deprecated on musl targets: the crate still declares the pre-musl-1.2 width,
/// which on armv7 is 32 bits (served by musl's time32 compat symbols). Only the zone offset is
/// taken from it; the date itself is formatted from the 64-bit epoch, so a 32-bit time_t can only
/// give a wrong offset after 2038 (noted in docs/admin/galion-admin.md).
#[allow(deprecated)]
pub fn local_offset(epoch: i64) -> i64 {
    let t = epoch as libc::time_t;
    // SAFETY: localtime_r only writes into the zeroed tm we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::localtime_r(&t, &mut tm) };
    if r.is_null() {
        0
    } else {
        tm.tm_gmtoff as i64
    }
}

/// Seconds since the Unix epoch.
pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Current local time, e.g. `2026-10-21T08:30:00+02:00`.
pub fn now_local() -> String {
    let e = now_epoch();
    format_rfc3339(e, local_offset(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_leap_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn zagreb_dst_switch_2026() {
        // 2026-10-25 00:59:59 UTC = 02:59:59 CEST; one second later = 02:00:00 CET.
        let before = 1_792_889_999;
        assert_eq!(format_rfc3339(before, 7200), "2026-10-25T02:59:59+02:00");
        assert_eq!(
            format_rfc3339(before + 1, 3600),
            "2026-10-25T02:00:00+01:00"
        );
    }

    #[test]
    fn never_z_and_negative_offsets() {
        assert_eq!(format_rfc3339(0, 0), "1970-01-01T00:00:00+00:00");
        assert_eq!(
            format_rfc3339(0, -(3 * 3600 + 1800)),
            "1969-12-31T20:30:00-03:30"
        );
    }

    #[test]
    fn now_has_offset() {
        let s = now_local();
        assert_eq!(s.len(), 25, "{s}");
        assert!(!s.ends_with('Z'));
    }
}
