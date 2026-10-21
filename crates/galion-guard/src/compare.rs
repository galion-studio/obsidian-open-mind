// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Side-by-side comparison of a fail2ban log and a galion-guard shadow log
//! (both in fail2ban format, see `f2blog`). Answers the shadow-mode question:
//! "would galion-guard have done exactly what fail2ban did?"

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use serde::Serialize;

use crate::f2blog::Record;

#[derive(Debug, Clone)]
pub struct Options {
    /// A pair of bans of the same address this close counts as a match.
    pub tolerance: i64,
    /// Compare only events in [since, until]; `None` = the overlap of the
    /// two logs (shadow mode usually starts later than fail2ban).
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Replace addresses with stable labels (`ip-1`, `ip-2`, ...) in the
    /// report, for pasting into docs.
    pub mask: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tolerance: 5,
            since: None,
            until: None,
            mask: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BanRow {
    pub ip: String,
    /// Ban time on each side (unix seconds), if that side banned.
    pub fail2ban: Option<i64>,
    pub guard: Option<i64>,
    /// Ban number (1 = first, 2 = first increase, ...).
    pub f2b_count: Option<u32>,
    pub guard_count: Option<u32>,
}

impl BanRow {
    pub fn delta(&self) -> Option<i64> {
        Some(self.guard? - self.fail2ban?)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Side {
    pub found: u64,
    pub found_addresses: usize,
    pub bans: u64,
    pub ban_addresses: usize,
    pub increased_bans: u64,
    pub restored_bans: u64,
    /// Unbans of bans made inside the window.
    pub unbans: u64,
    /// Unbans of bans from before the window (e.g. restored after a
    /// restart); informational, not compared.
    pub unbans_of_earlier_bans: u64,
    /// ban number -> how many bans had it.
    pub ban_numbers: BTreeMap<u32, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub since: i64,
    pub until: i64,
    pub tolerance_s: i64,
    pub fail2ban: Side,
    pub guard: Side,
    pub bans_same_second: u64,
    pub bans_within_tolerance: u64,
    pub bans_only_fail2ban: u64,
    pub bans_only_guard: u64,
    pub ban_number_mismatches: u64,
    pub found_addresses_differing: u64,
    pub max_abs_delta_s: i64,
    pub parity: bool,
    /// Every ban in the window, merged and in time order.
    pub rows: Vec<BanRow>,
    /// Addresses whose `Found` count differs: (ip, fail2ban, guard).
    pub found_diffs: Vec<(String, u64, u64)>,
}

struct Ban {
    at: i64,
    ip: IpAddr,
    count: u32,
}

/// Bans (not restores) with their ban number. fail2ban logs `Increase Ban`
/// right after the `Ban` it modifies; a `Ban` without one is ban #1.
fn bans(recs: &[Record]) -> Vec<Ban> {
    let mut out: Vec<Ban> = Vec::new();
    for r in recs {
        match r {
            Record::Ban {
                at,
                ip,
                restored: false,
            } => out.push(Ban {
                at: *at,
                ip: *ip,
                count: 1,
            }),
            Record::Increase { at, ip, count, .. } => {
                if let Some(b) = out
                    .iter_mut()
                    .rev()
                    .take(64)
                    .find(|b| b.ip == *ip && (at - b.at).abs() <= 5)
                {
                    b.count = *count;
                }
            }
            _ => {}
        }
    }
    out
}

fn bounds(recs: &[Record]) -> Option<(i64, i64)> {
    let min = recs.iter().map(Record::at).min()?;
    let max = recs.iter().map(Record::at).max()?;
    Some((min, max))
}

fn side(recs: &[Record], bans: &[&Ban], since: i64, until: i64) -> (Side, BTreeMap<IpAddr, u64>) {
    let inside = |t: i64| t >= since && t <= until;
    let mut s = Side::default();
    let mut found: BTreeMap<IpAddr, u64> = BTreeMap::new();
    for r in recs {
        match r {
            // By report time, like the window bounds (the log-line time can
            // be a second earlier and fall just outside the window).
            Record::Found { ip, at, .. } if inside(*at) => {
                *found.entry(*ip).or_default() += 1;
                s.found += 1;
            }
            Record::Ban {
                at, restored: true, ..
            } if inside(*at) => s.restored_bans += 1,
            Record::Unban { at, ip } if inside(*at) => {
                if bans.iter().any(|b| b.ip == *ip && b.at <= *at) {
                    s.unbans += 1;
                } else {
                    s.unbans_of_earlier_bans += 1;
                }
            }
            _ => {}
        }
    }
    s.found_addresses = found.len();
    s.bans = bans.len() as u64;
    s.ban_addresses = bans.iter().map(|b| b.ip).collect::<BTreeSet<_>>().len();
    for b in bans {
        *s.ban_numbers.entry(b.count).or_default() += 1;
        if b.count >= 2 {
            s.increased_bans += 1;
        }
    }
    (s, found)
}

pub fn compare(f2b: &[Record], guard: &[Record], o: &Options) -> Report {
    let (fa, fb) = bounds(f2b).unwrap_or((0, 0));
    let (ga, gb) = bounds(guard).unwrap_or((0, 0));
    let since = o.since.unwrap_or(fa.max(ga));
    let until = o.until.unwrap_or(fb.min(gb));
    let inside = |t: i64| t >= since && t <= until;
    let fb_all = bans(f2b);
    let gb_all = bans(guard);
    let f_in: Vec<&Ban> = fb_all.iter().filter(|b| inside(b.at)).collect();
    let g_in: Vec<&Ban> = gb_all.iter().filter(|b| inside(b.at)).collect();
    let (f_side, f_found) = side(f2b, &f_in, since, until);
    let (g_side, g_found) = side(guard, &g_in, since, until);

    // Match: for each fail2ban ban, the closest unused guard ban of the same
    // address within the tolerance (guard bans just outside the window may
    // match, so edge effects do not show up as differences).
    let mut used = vec![false; gb_all.len()];
    let mut rows: Vec<(i64, BanRow, IpAddr)> = Vec::new();
    let (mut same, mut within, mut only_f, mut nummis, mut maxd) = (0u64, 0u64, 0u64, 0u64, 0i64);
    for b in &f_in {
        let best = gb_all
            .iter()
            .enumerate()
            .filter(|(i, g)| !used[*i] && g.ip == b.ip && (g.at - b.at).abs() <= o.tolerance)
            .min_by_key(|(_, g)| (g.at - b.at).abs())
            .map(|(i, _)| i);
        let mut row = BanRow {
            ip: String::new(),
            fail2ban: Some(b.at),
            guard: None,
            f2b_count: Some(b.count),
            guard_count: None,
        };
        match best {
            Some(i) => {
                used[i] = true;
                let g = &gb_all[i];
                row.guard = Some(g.at);
                row.guard_count = Some(g.count);
                let d = (g.at - b.at).abs();
                maxd = maxd.max(d);
                if d == 0 {
                    same += 1;
                } else {
                    within += 1;
                }
                if g.count != b.count {
                    nummis += 1;
                }
            }
            None => only_f += 1,
        }
        rows.push((b.at, row, b.ip));
    }
    let mut only_g = 0u64;
    for (i, g) in gb_all.iter().enumerate() {
        if used[i] || !inside(g.at) {
            continue;
        }
        // A guard ban whose fail2ban twin lies just outside the window.
        let twin_outside = fb_all
            .iter()
            .any(|b| b.ip == g.ip && !inside(b.at) && (g.at - b.at).abs() <= o.tolerance);
        if twin_outside {
            continue;
        }
        only_g += 1;
        rows.push((
            g.at,
            BanRow {
                ip: String::new(),
                fail2ban: None,
                guard: Some(g.at),
                f2b_count: None,
                guard_count: Some(g.count),
            },
            g.ip,
        ));
    }
    rows.sort_by_key(|(t, _, ip)| (*t, *ip));

    let mut labels: BTreeMap<IpAddr, String> = BTreeMap::new();
    let mut label = |ip: IpAddr| -> String {
        if !o.mask {
            return ip.to_string();
        }
        let n = labels.len() + 1;
        labels
            .entry(ip)
            .or_insert_with(|| format!("ip-{n}"))
            .clone()
    };
    let rows: Vec<BanRow> = rows
        .into_iter()
        .map(|(_, mut r, ip)| {
            r.ip = label(ip);
            r
        })
        .collect();
    let mut found_diffs = Vec::new();
    let ips: BTreeSet<IpAddr> = f_found.keys().chain(g_found.keys()).copied().collect();
    for ip in ips {
        let a = f_found.get(&ip).copied().unwrap_or(0);
        let b = g_found.get(&ip).copied().unwrap_or(0);
        if a != b {
            found_diffs.push((label(ip), a, b));
        }
    }
    let parity = only_f == 0 && only_g == 0 && nummis == 0 && found_diffs.is_empty();
    Report {
        since,
        until,
        tolerance_s: o.tolerance,
        fail2ban: f_side,
        guard: g_side,
        bans_same_second: same,
        bans_within_tolerance: within,
        bans_only_fail2ban: only_f,
        bans_only_guard: only_g,
        ban_number_mismatches: nummis,
        found_addresses_differing: found_diffs.len() as u64,
        max_abs_delta_s: maxd,
        parity,
        rows,
        found_diffs,
    }
}

fn hist(m: &BTreeMap<u32, u64>) -> String {
    if m.is_empty() {
        return "-".into();
    }
    m.iter()
        .map(|(k, v)| format!("#{k}x{v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Human report: summary table, then the merged ban timeline in two columns.
/// All times Europe/Zagreb with offset.
pub fn render(r: &Report, timeline: bool) -> String {
    let z = zagreb_time::from_unix;
    let mut s = String::new();
    s += "galion-guard shadow comparison (sshd jail)\n";
    s += &format!(
        "window: {} .. {} ({}), match tolerance {} s\n\n",
        z(r.since),
        z(r.until),
        crate::duration::format((r.until - r.since).max(0) as u64),
        r.tolerance_s
    );
    s += &format!(
        "{:<28} {:>12} {:>14}\n",
        "metric", "fail2ban", "galion-guard"
    );
    let mut row = |name: &str, a: String, b: String| {
        let mark = if a == b { "" } else { "   <- differs" };
        s.push_str(&format!("{name:<28} {a:>12} {b:>14}{mark}\n"));
    };
    let (f, g) = (&r.fail2ban, &r.guard);
    row("failures (Found)", f.found.to_string(), g.found.to_string());
    row(
        "failing addresses",
        f.found_addresses.to_string(),
        g.found_addresses.to_string(),
    );
    row("bans", f.bans.to_string(), g.bans.to_string());
    row(
        "banned addresses",
        f.ban_addresses.to_string(),
        g.ban_addresses.to_string(),
    );
    row(
        "increased bans (#>=2)",
        f.increased_bans.to_string(),
        g.increased_bans.to_string(),
    );
    row("ban numbers", hist(&f.ban_numbers), hist(&g.ban_numbers));
    row(
        "unbans (of these bans)",
        f.unbans.to_string(),
        g.unbans.to_string(),
    );
    s += &format!(
        "{:<28} {:>12} {:>14}   (restarts only, not compared)\n",
        "restored bans", f.restored_bans, g.restored_bans
    );
    s += &format!(
        "{:<28} {:>12} {:>14}   (bans from before the window, not compared)\n",
        "unbans of earlier bans", f.unbans_of_earlier_bans, g.unbans_of_earlier_bans
    );
    s += "\n";
    s += &format!(
        "bans matched: {} same second, {} within {} s (max |delta| {} s)\n",
        r.bans_same_second, r.bans_within_tolerance, r.tolerance_s, r.max_abs_delta_s
    );
    s += &format!(
        "bans only fail2ban: {}   only galion-guard: {}   ban-number mismatches: {}\n",
        r.bans_only_fail2ban, r.bans_only_guard, r.ban_number_mismatches
    );
    s += &format!(
        "addresses with a different failure count: {}\n",
        r.found_addresses_differing
    );
    for (ip, a, b) in r.found_diffs.iter().take(50) {
        s += &format!("   {ip}: fail2ban {a}, galion-guard {b}\n");
    }
    s += &format!(
        "\nverdict: {}\n",
        if r.parity {
            "PARITY (galion-guard would have done exactly what fail2ban did)"
        } else {
            "DIFFERENCES (see above and the timeline)"
        }
    );
    if timeline {
        s += &format!(
            "\n{:<26} {:<26} {:<12} {}\n",
            "fail2ban ban", "galion-guard ban", "delta", "address (ban #)"
        );
        for b in &r.rows {
            let t = |x: Option<i64>| x.map(z).unwrap_or_else(|| "-".into());
            let d = match (b.delta(), b.fail2ban, b.guard) {
                (Some(0), ..) => "=".to_string(),
                (Some(d), ..) => format!("{d:+} s"),
                (None, Some(_), None) => "ONLY F2B".into(),
                _ => "ONLY GUARD".into(),
            };
            let n = |x: Option<u32>| x.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
            s += &format!(
                "{:<26} {:<26} {:<12} {} (#{} / #{})\n",
                t(b.fail2ban),
                t(b.guard),
                d,
                b.ip,
                n(b.f2b_count),
                n(b.guard_count)
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f2blog::{parse_log, Zone};

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    const F2B: &str = "\
2026-10-21 02:00:00,100 fail2ban.filter         [1]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 02:00:00
2026-10-21 02:00:01,100 fail2ban.filter         [1]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 02:00:01
2026-10-21 02:00:01,200 fail2ban.actions        [1]: NOTICE  [sshd] Ban 203.0.113.5
2026-10-21 02:00:01,300 fail2ban.observer       [1]: NOTICE  [sshd] Increase Ban 203.0.113.5 (2 # 2h -> 2026-10-21 04:00:01)
2026-10-21 02:10:00,100 fail2ban.filter         [1]: INFO    [sshd] Found 198.51.100.7 - 2026-10-21 02:10:00
2026-10-21 02:10:00,200 fail2ban.actions        [1]: NOTICE  [sshd] Ban 198.51.100.7
2026-10-21 03:00:00,200 fail2ban.actions        [1]: NOTICE  [sshd] Unban 198.51.100.7
";

    // Same events in Zagreb time (+02:00), one ban 3 s late.
    const GUARD: &str = "\
2026-10-21 04:00:00,000 galion-guard.filter     [9]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 04:00:00
2026-10-21 04:00:01,000 galion-guard.filter     [9]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 04:00:01
2026-10-21 04:00:01,000 galion-guard.actions    [9]: NOTICE  [sshd] Ban 203.0.113.5
2026-10-21 04:00:01,000 galion-guard.observer   [9]: NOTICE  [sshd] Increase Ban 203.0.113.5 (2 # 2h -> 2026-10-21 06:00:01)
2026-10-21 04:10:00,000 galion-guard.filter     [9]: INFO    [sshd] Found 198.51.100.7 - 2026-10-21 04:10:00
2026-10-21 04:10:03,000 galion-guard.actions    [9]: NOTICE  [sshd] Ban 198.51.100.7
2026-10-21 05:00:00,000 galion-guard.actions    [9]: NOTICE  [sshd] Unban 198.51.100.7
";

    #[test]
    fn parity_across_zones() {
        let f = parse_log(F2B, Zone::Utc, "sshd");
        let g = parse_log(GUARD, Zone::Zagreb, "sshd");
        let r = compare(&f, &g, &Options::default());
        assert!(r.parity, "{}", render(&r, true));
        assert_eq!(r.bans_same_second, 1);
        assert_eq!(r.bans_within_tolerance, 1);
        assert_eq!(r.max_abs_delta_s, 3);
        assert_eq!(r.fail2ban.increased_bans, 1);
        assert_eq!(r.guard.ban_numbers.get(&2), Some(&1));
        assert_eq!(r.fail2ban.found, 3);
        assert_eq!(r.guard.unbans, 1);
        let text = render(&r, true);
        assert!(text.contains("verdict: PARITY"));
        assert!(text.contains("2026-10-21T04:10:00+02:00"));
        assert!(!text.contains("Z "));
    }

    #[test]
    fn differences_are_reported() {
        let f = parse_log(F2B, Zone::Utc, "sshd");
        let mut g = parse_log(GUARD, Zone::Zagreb, "sshd");
        // Guard misses the second ban, sees an extra one, and counts the
        // first address once more.
        g.retain(|r| !matches!(r, Record::Ban { ip: i, .. } if *i == ip("198.51.100.7")));
        let t = g[0].at();
        g.push(Record::Ban {
            at: t + 30,
            ip: ip("192.0.2.44"),
            restored: false,
        });
        g.push(Record::Found {
            at: t + 2,
            ip: ip("203.0.113.5"),
            logged: t + 2,
        });
        let r = compare(
            &f,
            &g,
            &Options {
                mask: true,
                ..Options::default()
            },
        );
        assert!(!r.parity);
        assert_eq!(r.bans_only_fail2ban, 1);
        assert_eq!(r.bans_only_guard, 1);
        assert_eq!(r.found_addresses_differing, 1);
        let text = render(&r, true);
        assert!(text.contains("ONLY F2B") && text.contains("ONLY GUARD"));
        assert!(!text.contains("203.0.113.5"), "masked: {text}");
        assert!(text.contains("<- differs"));
    }

    #[test]
    fn ban_number_mismatch_and_tolerance() {
        let f = parse_log(F2B, Zone::Utc, "sshd");
        let g: Vec<Record> = parse_log(GUARD, Zone::Zagreb, "sshd")
            .into_iter()
            .filter(|r| !matches!(r, Record::Increase { .. }))
            .collect();
        let r = compare(
            &f,
            &g,
            &Options {
                tolerance: 1,
                ..Options::default()
            },
        );
        assert_eq!(r.ban_number_mismatches, 1);
        // The 3 s late ban no longer matches at tolerance 1.
        assert_eq!(r.bans_only_fail2ban, 1);
        assert_eq!(r.bans_only_guard, 1);
        assert!(!r.parity);
    }

    #[test]
    fn window_edges_do_not_count() {
        let f = parse_log(F2B, Zone::Utc, "sshd");
        let g = parse_log(GUARD, Zone::Zagreb, "sshd");
        // Explicit window that excludes the first fail2ban ban by 1 s while
        // the guard ban is the same second: no difference either way.
        let since = f[2].at() + 1;
        let r = compare(
            &f,
            &g,
            &Options {
                since: Some(since),
                ..Options::default()
            },
        );
        assert_eq!(r.bans_only_guard, 0);
        assert_eq!(r.bans_only_fail2ban, 0);
        let empty = compare(&[], &[], &Options::default());
        assert!(empty.parity && empty.rows.is_empty());
    }
}
