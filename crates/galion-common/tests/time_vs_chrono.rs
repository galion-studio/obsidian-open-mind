// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The std-only time module must print exactly what zagreb-time (chrono-tz)
//! prints, around every DST switch from 1996 to 2099 and on a coarse sweep. (chrono-tz stops
//! applying the EU rule in 2100; this module keeps applying it.)

use galion_common::time;

#[test]
fn every_switch_1996_to_2100() {
    // Under Miri (an interpreter, ~1000x slower) the full sweep takes hours,
    // so it checks 2026 + 2027 (the next switches) and every 26th year from
    // 1996. Normal builds still sweep every year. Test-only.
    #[cfg(not(miri))]
    let years: Vec<i64> = (1996..=2099).collect();
    #[cfg(miri)]
    let years: Vec<i64> = (1996..=2099).step_by(26).chain([2026, 2027]).collect();
    for year in years {
        for month in [3u32, 10] {
            // Sweep the last 8 days of the month, hourly, plus +-1 s at each hour.
            let start = time::days_from_civil(year, month, 24) * 86_400;
            for h in 0..(8 * 24) {
                for d in [-1i64, 0, 1] {
                    let t = start + h * 3600 + d;
                    assert_eq!(time::from_unix(t), zagreb_time::from_unix(t), "t={t}");
                }
            }
        }
    }
}

#[test]
fn coarse_sweep_and_labels() {
    let mut t = time::days_from_civil(1996, 1, 1) * 86_400;
    let end = time::days_from_civil(2100, 1, 1) * 86_400;
    while t < end {
        assert_eq!(time::from_unix(t), zagreb_time::from_unix(t));
        assert_eq!(time::label(t), zagreb_time::label(t));
        t += 86_400 * 3 + 3_599;
    }
}

#[test]
fn stamp_checks_agree() {
    for s in [
        "2026-10-20T19:00:00+02:00",
        "2026-10-20T19:00:00Z",
        "2026-10-20T19:00:00+00:00",
        "2026-02-29T19:00:00+01:00",
        "2024-02-29T19:00:00+01:00",
        "2026-13-01T19:00:00+01:00",
        "2026-10-20t19:00:00+02:00",
        "2026-10-20 19:00:00+02:00",
        "2026-10-20T24:00:00+02:00",
        "2026-10-20T19:00:00.5+02:00",
        "garbage",
    ] {
        let a = time::require_rfc3339(s).is_ok();
        let b = zagreb_time::require_rfc3339(s).is_ok();
        assert_eq!(a, b, "{s}");
    }
}
