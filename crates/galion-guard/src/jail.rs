// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! The ban engine: failure windows (findtime/maxretry), ban times with
//! fail2ban-style increment, expiry, and the never-ban protections.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::ipnet::{canonical, is_hard_protected, Net};

#[derive(Debug, Clone, PartialEq)]
pub struct JailParams {
    pub maxretry: u32,
    pub findtime: u64,
    pub bantime: u64,
    pub increment: bool,
    pub factor: f64,
    /// Cap for incremented ban times (fail2ban `bantime.maxtime`).
    pub maxtime: Option<u64>,
    /// Ban history is forgotten this long after the last ban ended
    /// (fail2ban `dbpurgeage`).
    pub purge_age: u64,
    /// Never ban an address that logged in successfully this recently
    /// (0 = off). Not a fail2ban feature: admin lock-out protection.
    pub protect_accepted: u64,
}

impl Default for JailParams {
    /// The values the VPS fail2ban runs with (docs/guard-rust.md).
    fn default() -> Self {
        JailParams {
            maxretry: 5,
            findtime: 600,
            bantime: 3600,
            increment: true,
            factor: 1.0,
            maxtime: None,
            purge_age: 86_400,
            protect_accepted: 86_400,
        }
    }
}

/// Why an address is never banned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protected {
    Loopback,
    Allowlist,
    SelfAddress,
    RecentLogin,
}

#[derive(Debug, Clone, Default)]
pub struct Protect {
    pub allow: Vec<Net>,
    pub self_addrs: Vec<IpAddr>,
}

impl Protect {
    pub fn check(&self, ip: IpAddr) -> Option<Protected> {
        let ip = canonical(ip);
        if is_hard_protected(ip) {
            return Some(Protected::Loopback);
        }
        if self.allow.iter().any(|n| n.contains(ip)) {
            return Some(Protected::Allowlist);
        }
        if self.self_addrs.iter().any(|a| canonical(*a) == ip) {
            return Some(Protected::SelfAddress);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    pub ip: IpAddr,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    /// 1 for the first ban within the history window, 2 for the second ...
    pub count: u32,
    /// Failures that triggered it (0 for a manual ban).
    pub failures: u32,
    #[serde(default)]
    pub manual: bool,
}

/// Failure bookkeeping for one address (see [`Jail::on_failure`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// fail2ban "retry": weighted, rate-rescaled failure count.
    pub retry: u32,
    pub first: i64,
    pub last: i64,
    /// Real number of failure lines.
    pub failures: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub ip: IpAddr,
    pub count: u32,
    pub last_end: i64,
    /// Start of the last ban (0 = unknown: treat as `last_end`).
    #[serde(default)]
    pub last_start: i64,
}

impl History {
    /// fail2ban keeps a ban record until `timeofban + 3 * bantime` is older
    /// than `dbpurgeage` (its hourly purge adds up to 1 h of lag, which we
    /// do not imitate).
    pub fn remembered(&self, purge_age: u64, now: i64) -> bool {
        let start = if self.last_start == 0 {
            self.last_end
        } else {
            self.last_start
        };
        let len = (self.last_end - start).max(0);
        start
            .saturating_add(len.saturating_mul(3))
            .saturating_add(purge_age as i64)
            >= now
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Ban(Ban),
    Unban { ip: IpAddr, reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailResult {
    Counted(u32),
    Banned(Ban),
    AlreadyBanned,
    Protected(Protected),
    TooOld,
}

/// Counters for `status` (like `fail2ban-client status sshd`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Totals {
    pub failed: u64,
    pub banned: u64,
    pub protected_hits: u64,
}

#[derive(Debug)]
pub struct Jail {
    pub params: JailParams,
    pub protect: Protect,
    failures: HashMap<IpAddr, Window>,
    bans: BTreeMap<IpAddr, Ban>,
    history: HashMap<IpAddr, History>,
    accepted: HashMap<IpAddr, i64>,
    pub totals: Totals,
}

/// Bound on tracked failing addresses (memory cap under a log flood).
pub const MAX_TRACKED: usize = 65_536;

impl Jail {
    pub fn new(params: JailParams, protect: Protect) -> Self {
        Jail {
            params,
            protect,
            failures: HashMap::new(),
            bans: BTreeMap::new(),
            history: HashMap::new(),
            accepted: HashMap::new(),
            totals: Totals::default(),
        }
    }

    /// Protection reason for `ip` at `now`, if any.
    pub fn protected(&self, ip: IpAddr, now: i64) -> Option<Protected> {
        if let Some(p) = self.protect.check(ip) {
            return Some(p);
        }
        let ip = canonical(ip);
        if self.params.protect_accepted > 0 {
            if let Some(t) = self.accepted.get(&ip) {
                if now - *t <= self.params.protect_accepted as i64 {
                    return Some(Protected::RecentLogin);
                }
            }
        }
        None
    }

    /// Record a successful login. Returns an unban action when the address
    /// was banned (it just proved it holds a key: never keep it locked out).
    pub fn on_accepted(&mut self, ip: IpAddr, t: i64, now: i64) -> Option<Action> {
        let ip = canonical(ip);
        let t = t.min(now);
        if self.params.protect_accepted == 0 {
            return None;
        }
        let e = self.accepted.entry(ip).or_insert(t);
        *e = (*e).max(t);
        self.failures.remove(&ip);
        self.bans.remove(&ip).map(|_| Action::Unban {
            ip,
            reason: "recent-login",
        })
    }

    /// Count one failure seen at log time `t` (clamped to `now`).
    ///
    /// Window semantics are fail2ban's (1.0 `FailTicket.adjustTime`), not a
    /// strict sliding window: per address we keep a retry count and the
    /// start of its interval; when a failure arrives more than findtime
    /// after that start, the count is rescaled to the observed rate over one
    /// findtime (`round_half_even(retry / (t - first) * findtime)`) and the
    /// interval start moves to `t - findtime`. An address whose last failure
    /// is older than findtime starts again from zero.
    pub fn on_failure(&mut self, ip: IpAddr, t: i64, now: i64) -> FailResult {
        let ip = canonical(ip);
        let t = t.min(now);
        let window = self.params.findtime as i64;
        if t < now - window {
            return FailResult::TooOld;
        }
        if let Some(p) = self.protected(ip, now) {
            self.totals.protected_hits += 1;
            return FailResult::Protected(p);
        }
        self.totals.failed += 1;
        if self.bans.contains_key(&ip) {
            return FailResult::AlreadyBanned;
        }
        if !self.failures.contains_key(&ip) && self.failures.len() >= MAX_TRACKED {
            self.prune_failures(now);
            if self.failures.len() >= MAX_TRACKED {
                // Still full: drop the address with the oldest last failure.
                if let Some(old) = self
                    .failures
                    .iter()
                    .min_by_key(|(_, w)| w.last)
                    .map(|(k, _)| *k)
                {
                    self.failures.remove(&old);
                }
            }
        }
        let max = self.params.maxretry.max(1);
        let weight = self.failure_weight(ip, now);
        let w = self.failures.entry(ip).or_insert(Window {
            retry: 0,
            first: t,
            last: t,
            failures: 0,
        });
        if w.last < t - window {
            // Stale (fail2ban's cleanup would have dropped it): restart.
            *w = Window {
                retry: 0,
                first: t,
                last: t,
                failures: 0,
            };
        }
        if t > w.last {
            if w.first < t - window {
                let est = w.retry as f64 / (t - w.first) as f64 * window as f64;
                w.retry = est.round_ties_even() as u32;
                w.first = t - window;
            }
            w.last = t;
        }
        w.retry = w.retry.saturating_add(weight);
        w.failures = w.failures.saturating_add(1);
        let n = w.retry;
        let real = w.failures;
        if n >= max {
            self.failures.remove(&ip);
            let ban = self.make_ban(ip, now, real, false, None);
            self.bans.insert(ip, ban.clone());
            self.totals.banned += 1;
            return FailResult::Banned(ban);
        }
        FailResult::Counted(n)
    }

    /// fail2ban `bantime.increment` also makes known offenders count faster:
    /// with `k` earlier bans (within `purge_age`) one failure weighs
    /// `2^k / 2 + 1` (2, 3, 5, 9, ...), capped at maxretry.
    pub fn failure_weight(&self, ip: IpAddr, now: i64) -> u32 {
        if !self.params.increment {
            return 1;
        }
        let prev = match self.history.get(&ip) {
            Some(h) if h.remembered(self.params.purge_age, now) => h.count,
            _ => 0,
        };
        if prev == 0 {
            return 1;
        }
        let w = (1u64 << prev.min(20)) / 2 + 1;
        w.min(self.params.maxretry.max(1) as u64) as u32
    }

    fn make_ban(
        &mut self,
        ip: IpAddr,
        now: i64,
        failures: u32,
        manual: bool,
        dur: Option<u64>,
    ) -> Ban {
        let prev = match self.history.get(&ip) {
            Some(h) if h.remembered(self.params.purge_age, now) => h.count,
            _ => 0,
        };
        let count = prev.saturating_add(1);
        let secs = dur.unwrap_or_else(|| self.ban_secs(count));
        let end = now.saturating_add(secs.min(i64::MAX as u64 / 2) as i64);
        self.history.insert(
            ip,
            History {
                ip,
                count,
                last_end: end,
                last_start: now,
            },
        );
        Ban {
            ip,
            start: now,
            end,
            count,
            failures,
            manual,
        }
    }

    /// Ban length for the `count`-th ban: `bantime * factor * 2^min(count-1, 20)`.
    pub fn ban_secs(&self, count: u32) -> u64 {
        let base = self.params.bantime.max(1);
        if !self.params.increment {
            return base;
        }
        let exp = count.saturating_sub(1).min(20);
        let v = base as f64 * self.params.factor.max(0.0) * (1u64 << exp) as f64;
        let v = if v.is_finite() {
            v.max(1.0) as u64
        } else {
            u64::MAX / 4
        };
        match self.params.maxtime {
            Some(m) => v.min(m.max(1)),
            None => v,
        }
    }

    /// Operator ban (CLI). Refused for protected addresses.
    pub fn manual_ban(&mut self, ip: IpAddr, now: i64, dur: Option<u64>) -> Result<Ban, Protected> {
        let ip = canonical(ip);
        if let Some(p) = self.protected(ip, now) {
            return Err(p);
        }
        let ban = self.make_ban(ip, now, 0, true, dur);
        self.bans.insert(ip, ban.clone());
        self.totals.banned += 1;
        Ok(ban)
    }

    pub fn unban(&mut self, ip: IpAddr) -> Option<Ban> {
        self.bans.remove(&canonical(ip))
    }

    /// Expired bans, and bans of addresses that became protected (for
    /// example after an allowlist change): both are lifted.
    pub fn tick(&mut self, now: i64) -> Vec<Action> {
        let mut out = Vec::new();
        let ips: Vec<IpAddr> = self.bans.keys().copied().collect();
        for ip in ips {
            let b = &self.bans[&ip];
            let reason = if b.end <= now {
                Some("expired")
            } else if self.protect.check(ip).is_some() {
                Some("protected")
            } else {
                None
            };
            if let Some(reason) = reason {
                self.bans.remove(&ip);
                out.push(Action::Unban { ip, reason });
            }
        }
        out
    }

    /// Drop stale failure windows, history and login marks.
    pub fn prune(&mut self, now: i64) {
        self.prune_failures(now);
        let purge = self.params.purge_age as i64;
        self.history.retain(|_, h| h.remembered(purge as u64, now));
        let pa = self.params.protect_accepted as i64;
        self.accepted.retain(|_, t| now - *t <= pa);
    }

    fn prune_failures(&mut self, now: i64) {
        let w = self.params.findtime as i64;
        self.failures.retain(|_, x| x.last >= now - w);
    }

    pub fn bans(&self) -> impl Iterator<Item = &Ban> {
        self.bans.values()
    }

    pub fn ban_of(&self, ip: IpAddr) -> Option<&Ban> {
        self.bans.get(&canonical(ip))
    }

    pub fn currently_failed(&self) -> usize {
        self.failures.len()
    }

    pub fn history(&self) -> impl Iterator<Item = &History> {
        self.history.values()
    }

    pub fn accepted(&self) -> impl Iterator<Item = (&IpAddr, &i64)> {
        self.accepted.iter()
    }

    /// Restore from persisted state. Protected or expired bans are dropped
    /// here and reported as unbans (so the firewall is cleaned too).
    pub fn restore(
        &mut self,
        bans: Vec<Ban>,
        history: Vec<History>,
        accepted: Vec<(IpAddr, i64)>,
        totals: Totals,
        now: i64,
    ) -> Vec<Action> {
        self.totals = totals;
        for h in history {
            self.history.insert(canonical(h.ip), h);
        }
        for (ip, t) in accepted {
            self.accepted.insert(canonical(ip), t);
        }
        let mut out = Vec::new();
        for b in bans {
            let ip = canonical(b.ip);
            if b.end <= now {
                out.push(Action::Unban {
                    ip,
                    reason: "expired",
                });
            } else if self.protected(ip, now).is_some() {
                out.push(Action::Unban {
                    ip,
                    reason: "protected",
                });
            } else {
                self.bans.insert(ip, Ban { ip, ..b });
            }
        }
        self.prune(now);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn jail() -> Jail {
        Jail::new(
            JailParams::default(),
            Protect {
                allow: vec![Net::parse("198.51.100.0/24").unwrap()],
                self_addrs: vec![ip("192.0.2.10")],
            },
        )
    }

    #[test]
    fn bans_on_fifth_failure_within_findtime() {
        let mut j = jail();
        let a = ip("203.0.113.1");
        for i in 0..4 {
            assert_eq!(
                j.on_failure(a, 1000 + i * 100, 1000 + i * 100),
                FailResult::Counted(i as u32 + 1)
            );
        }
        match j.on_failure(a, 1500, 1500) {
            FailResult::Banned(b) => {
                assert_eq!((b.start, b.end, b.count), (1500, 5100, 1));
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(j.on_failure(a, 1501, 1501), FailResult::AlreadyBanned);
    }

    #[test]
    fn window_rescales_like_fail2ban() {
        let mut j = jail();
        let a = ip("203.0.113.2");
        for t in [0, 100, 200, 300] {
            j.on_failure(a, t, t);
        }
        // 701: round(4 / 701 * 600) = 3, +1.
        assert_eq!(j.on_failure(a, 701, 701), FailResult::Counted(4));
        // 750: first = 101 -> round(4 / 649 * 600) = 4, +1 = 5: ban.
        assert!(matches!(j.on_failure(a, 750, 750), FailResult::Banned(_)));
        // A gap longer than findtime starts from zero.
        let b = ip("203.0.113.3");
        for t in [0, 100, 200, 300] {
            j.on_failure(b, t, t);
        }
        assert_eq!(j.on_failure(b, 901, 901), FailResult::Counted(1));
    }

    #[test]
    fn real_vps_sequence_bans_on_the_eighth_failure() {
        // Failure offsets (s) of one attacker on 2026-10-18/19 that fail2ban
        // banned on the 8th line although no 10-minute span holds 5 of them.
        let mut j = jail();
        let a = ip("203.0.113.30");
        let offs = [0, 380, 442, 570, 1121, 1249, 1313, 1374];
        let mut got = vec![];
        for t in offs {
            got.push(j.on_failure(a, t, t));
        }
        let counts: Vec<u32> = got[..7]
            .iter()
            .map(|r| match r {
                FailResult::Counted(n) => *n,
                o => panic!("{o:?}"),
            })
            .collect();
        assert_eq!(counts, vec![1, 2, 3, 4, 3, 3, 4]);
        assert!(matches!(got[7], FailResult::Banned(_)));
    }

    #[test]
    fn too_old_lines_ignored() {
        let mut j = jail();
        assert_eq!(
            j.on_failure(ip("203.0.113.3"), 0, 10_000),
            FailResult::TooOld
        );
    }

    #[test]
    fn increment_doubles_and_history_purges() {
        let mut j = jail();
        let a = ip("203.0.113.4");
        let mut now = 10_000;
        let mut lens = vec![];
        for _ in 0..6 {
            for k in 0..5 {
                j.on_failure(a, now + k, now + k);
            }
            let b = j.ban_of(a).unwrap().clone();
            lens.push(b.end - b.start);
            now = b.end + 1;
            assert_eq!(j.tick(now).len(), 1);
        }
        // 1h, 2h, 4h, 8h, 16h, 32h (= the VPS "6 # 1d 8h").
        assert_eq!(lens, vec![3600, 7200, 14_400, 28_800, 57_600, 115_200]);
        // Still remembered just before start + 3 * bantime + purge_age ...
        let h = j.history().next().unwrap().clone();
        assert!(h.remembered(86_400, h.last_start + 3 * 115_200 + 86_400));
        assert!(!h.remembered(86_400, h.last_start + 3 * 115_200 + 86_401));
        // ... after that the count restarts.
        now = h.last_start + 3 * 115_200 + 86_401;
        j.prune(now);
        for k in 0..5 {
            j.on_failure(a, now + k, now + k);
        }
        let b = j.ban_of(a).unwrap();
        assert_eq!((b.count, b.end - b.start), (1, 3600));
    }

    #[test]
    fn repeat_offenders_count_faster_like_fail2ban() {
        let mut j = jail();
        let a = ip("203.0.113.40");
        let mut now = 1000;
        let mut needed = vec![];
        for _ in 0..5 {
            let mut n = 0;
            loop {
                n += 1;
                if let FailResult::Banned(_) = j.on_failure(a, now, now) {
                    break;
                }
                now += 1;
            }
            needed.push(n);
            now = j.ban_of(a).unwrap().end;
            j.tick(now);
            now += 1;
        }
        // Weights 1, 2, 3, 5, 9 (capped at maxretry 5) -> 5, 3, 2, 1, 1 failures.
        assert_eq!(needed, vec![5, 3, 2, 1, 1]);
        assert_eq!(j.failure_weight(a, now), 5);
    }

    #[test]
    fn maxtime_caps() {
        let p = JailParams {
            maxtime: Some(5000),
            ..Default::default()
        };
        let j = Jail::new(p, Protect::default());
        assert_eq!(j.ban_secs(1), 3600);
        assert_eq!(j.ban_secs(2), 5000);
        assert_eq!(j.ban_secs(99), 5000);
    }

    #[test]
    fn huge_counts_do_not_overflow() {
        let j = jail();
        assert_eq!(j.ban_secs(u32::MAX), 3600 << 20);
    }

    #[test]
    fn never_bans_protected() {
        let mut j = jail();
        for a in [
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "::ffff:127.0.0.1",
            "198.51.100.77",
            "192.0.2.10",
            "0.0.0.0",
        ] {
            for t in 0..20 {
                let r = j.on_failure(ip(a), 100 + t, 100 + t);
                assert!(matches!(r, FailResult::Protected(_)), "{a}: {r:?}");
            }
            assert!(j.manual_ban(ip(a), 200, None).is_err());
        }
        assert_eq!(j.bans().count(), 0);
    }

    #[test]
    fn recent_login_protects_and_unbans() {
        let mut j = jail();
        let a = ip("203.0.113.5");
        j.manual_ban(a, 100, None).unwrap();
        assert!(matches!(
            j.on_accepted(a, 150, 150),
            Some(Action::Unban { .. })
        ));
        for t in 0..10 {
            assert_eq!(
                j.on_failure(a, 200 + t, 200 + t),
                FailResult::Protected(Protected::RecentLogin)
            );
        }
        // Protection lapses after protect_accepted.
        let later = 150 + 86_400 + 1;
        assert_eq!(j.on_failure(a, later, later), FailResult::Counted(2)); // manual ban in history: weight 2
    }

    #[test]
    fn allowlist_change_lifts_ban() {
        let mut j = jail();
        let a = ip("203.0.113.6");
        j.manual_ban(a, 100, None).unwrap();
        j.protect.allow.push(Net::parse("203.0.113.0/24").unwrap());
        assert_eq!(
            j.tick(101),
            vec![Action::Unban {
                ip: a,
                reason: "protected"
            }]
        );
    }

    #[test]
    fn mapped_v6_is_same_address() {
        let mut j = jail();
        for t in 0..5 {
            j.on_failure(ip("::ffff:203.0.113.7"), t, t);
        }
        assert!(j.ban_of(ip("203.0.113.7")).is_some());
    }

    #[test]
    fn restore_drops_expired_and_protected() {
        let mut j = jail();
        let keep = Ban {
            ip: ip("203.0.113.8"),
            start: 0,
            end: 1000,
            count: 1,
            failures: 5,
            manual: false,
        };
        let old = Ban {
            ip: ip("203.0.113.9"),
            start: 0,
            end: 50,
            count: 1,
            failures: 5,
            manual: false,
        };
        let prot = Ban {
            ip: ip("198.51.100.1"),
            start: 0,
            end: 1000,
            count: 1,
            failures: 5,
            manual: false,
        };
        let acts = j.restore(
            vec![keep.clone(), old, prot],
            vec![],
            vec![],
            Totals::default(),
            100,
        );
        assert_eq!(acts.len(), 2);
        assert_eq!(j.bans().cloned().collect::<Vec<_>>(), vec![keep]);
    }

    #[test]
    fn tracked_addresses_bounded() {
        let p = JailParams {
            maxretry: 1000,
            ..Default::default()
        };
        let mut j = Jail::new(p, Protect::default());
        for i in 0..(MAX_TRACKED as u32 + 10) {
            let a = IpAddr::V4(std::net::Ipv4Addr::from(0x0b00_0000 + i));
            j.on_failure(a, 10, 10);
        }
        assert!(j.currently_failed() <= MAX_TRACKED);
    }
}
